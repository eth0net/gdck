//! Code that does not do what it looks like it does.
//!
//! `unused-argument`, `duplicated-load`, `expression-not-assigned`,
//! `comparison-with-itself` and `unnecessary-pass`. None of these are style;
//! each is a line that could be deleted, or one that was meant to say
//! something else.
//!
//! Those five match `gdtoolkit`'s checks of the same names, deliberately.
//! These are the findings a project has already triaged, and a
//! reimplementation that quietly widened one would present old code as newly
//! broken.
//!
//! `doc-tag` has no counterpart there. It belongs here rather than with the
//! style rules because a mistyped documentation tag is not a matter of taste:
//! Godot ignores it in silence, and the documentation the author wrote simply
//! never appears.

use std::collections::HashMap;

use gdck_format::literal;
use gdck_syntax::{SyntaxKind, SyntaxNode, TextRange, Token};

use super::{Context, Sink, callee_name, name_token, significant_range, significant_tokens};
use crate::{Edit, Fix};

pub(crate) fn check(context: &Context<'_>, sink: &mut Sink) {
    check_duplicated_load(context, sink);

    for token in super::all_tokens(context.root()) {
        if token.kind == SyntaxKind::DocComment {
            check_doc_tag(context, sink, token);
        }
    }

    for node in context.root().descendants() {
        match node.kind() {
            SyntaxKind::SourceFile | SyntaxKind::Block => {
                check_unnecessary_pass(context, sink, node);
                check_expression_statements(sink, node);
            }
            SyntaxKind::FuncDecl => check_unused_arguments(context, sink, node),
            SyntaxKind::BinaryExpr => check_self_comparison(context, sink, node),
            SyntaxKind::ForStmt => check_loop_variable_assignment(context, sink, node),
            _ => {}
        }
    }
}

// -- unnecessary-pass -------------------------------------------------------

/// `pass` exists to give an otherwise empty block a body. Beside a real
/// statement it does nothing.
fn check_unnecessary_pass(context: &Context<'_>, sink: &mut Sink, block: SyntaxNode<'_>) {
    let statements: Vec<SyntaxNode<'_>> = block.child_nodes().collect();
    let passes: Vec<SyntaxNode<'_>> = statements
        .iter()
        .copied()
        .filter(|node| node.kind() == SyntaxKind::PassStmt)
        .collect();
    if passes.len() == statements.len() {
        return;
    }

    for pass in passes {
        let range = significant_range(pass);
        match whole_line(context, range) {
            Some(line) => sink.report_with_fix(
                "unnecessary-pass",
                range,
                "`pass` is unnecessary in a block that has other statements",
                Fix::new(vec![Edit::delete(line)]),
            ),
            // Sharing a line with something else, so removing it is an edit to
            // that line rather than the deletion of one. `one-statement-per-
            // line` is the formatter's business; this rule stays out of it.
            None => sink.report(
                "unnecessary-pass",
                range,
                "`pass` is unnecessary in a block that has other statements",
            ),
        }
    }
}

/// The full line `range` sits on, including its line feed, when `range` is the
/// only thing on it.
fn whole_line(context: &Context<'_>, range: TextRange) -> Option<TextRange> {
    let source = context.source;
    let start = source[..range.start() as usize]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    if !source[start..range.start() as usize].trim().is_empty() {
        return None;
    }
    let rest = &source[range.end() as usize..];
    let end = match rest.find('\n') {
        Some(index) => {
            if !rest[..index].trim().is_empty() {
                return None;
            }
            range.end() as usize + index + 1
        }
        None if rest.trim().is_empty() => source.len(),
        None => return None,
    };
    Some(TextRange::new(start as u32, end as u32))
}

// -- expression-not-assigned ------------------------------------------------

/// An expression written as a statement has to do something.
///
/// A call might; `a + 1` cannot. The tree makes this a question about the
/// statement's only child, since assignments are their own node kind and never
/// arrive here.
fn check_expression_statements(sink: &mut Sink, block: SyntaxNode<'_>) {
    for statement in block.child_nodes() {
        if statement.kind() != SyntaxKind::ExprStmt {
            continue;
        }
        let Some(expression) = statement.child_nodes().next() else {
            continue;
        };
        let expression = super::unwrap_parens(expression);
        if has_an_effect(expression) {
            continue;
        }
        sink.report(
            "expression-not-assigned",
            significant_range(statement),
            "this expression is not used and has no effect",
        );
    }
}

fn has_an_effect(expression: SyntaxNode<'_>) -> bool {
    match expression.kind() {
        // `super.method()` and `$Node.method()` reach here as calls too.
        SyntaxKind::CallExpr | SyntaxKind::AwaitExpr | SyntaxKind::PreloadExpr => true,
        // A bare string is a docstring, which is documentation rather than a
        // discarded value.
        SyntaxKind::Literal => significant_tokens(expression)
            .first()
            .is_some_and(|token| token.kind == SyntaxKind::Str),
        _ => false,
    }
}

// -- duplicated-load --------------------------------------------------------

/// Loading the same path twice does the work twice, and gives two names for
/// one thing.
fn check_duplicated_load(context: &Context<'_>, sink: &mut Sink) {
    let mut seen: HashMap<String, u32> = HashMap::new();

    for node in context.root().descendants() {
        let path = match node.kind() {
            SyntaxKind::PreloadExpr => loaded_path(context, node),
            SyntaxKind::CallExpr
                if matches!(callee_name(node, context.source), Some("load" | "preload")) =>
            {
                loaded_path(context, node)
            }
            _ => None,
        };
        let Some((token, path)) = path else { continue };
        // Compared after normalising the quotes, so changing `'x'` to `"x"`
        // does not change the answer.
        let key = literal::normalize_string(path);
        if let Some(first) = seen.get(&key) {
            let line = gdck_syntax::LineIndex::new(context.source)
                .line_col(*first)
                .line;
            sink.report(
                "duplicated-load",
                token.range,
                format!("{path} is already loaded on line {line}"),
            );
        } else {
            seen.insert(key, token.range.start());
        }
    }
}

/// The string literal a `load` or `preload` was given, if it was given one.
fn loaded_path<'a>(context: &Context<'a>, call: SyntaxNode<'a>) -> Option<(Token, &'a str)> {
    let arguments = call.child_node_of(SyntaxKind::ArgList)?;
    let first = arguments.child_nodes().next()?;
    if first.kind() != SyntaxKind::Literal {
        return None;
    }
    let token = *significant_tokens(first).first()?;
    if token.kind != SyntaxKind::Str {
        return None;
    }
    Some((token, context.token_text(token)))
}

// -- unused-argument --------------------------------------------------------

/// An argument the body never mentions.
///
/// A leading underscore is the convention for one that is deliberately unused
/// — an overridden virtual method has to take what it is given — so those are
/// not reported. That is also the fix, which is why none is offered: renaming
/// is a decision about the interface, not a mechanical edit.
fn check_unused_arguments(context: &Context<'_>, sink: &mut Sink, func: SyntaxNode<'_>) {
    let Some(params) = func.child_node_of(SyntaxKind::ParamList) else {
        return;
    };
    // `@abstract func area() -> float` has no body to use anything in.
    if func.child_node_of(SyntaxKind::Block).is_none() {
        return;
    }

    // Every identifier in the function, the parameter list included, so a name
    // used exactly as often as it is declared was never read.
    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    for token in significant_tokens(func) {
        if token.kind == SyntaxKind::Ident {
            *occurrences.entry(context.token_text(token)).or_default() += 1;
        }
    }

    for param in params.child_nodes() {
        if param.kind() != SyntaxKind::Param {
            continue;
        }
        let Some(token) = name_token(param) else {
            continue;
        };
        let name = context.token_text(token);
        if name.starts_with('_') {
            continue;
        }
        // The type hint on `func f(a: int)` is a child node of the parameter,
        // so the only Ident the declaration itself contributes is the name.
        let declared = params
            .child_nodes()
            .filter(|other| other.kind() == SyntaxKind::Param)
            .filter_map(name_token)
            .filter(|other| context.token_text(*other) == name)
            .count();
        if occurrences.get(name).copied().unwrap_or_default() > declared {
            continue;
        }
        sink.report(
            "unused-argument",
            token.range,
            format!("argument `{name}` is unused; rename it to `_{name}` if that is deliberate"),
        );
    }
}

// -- comparison-with-itself -------------------------------------------------

const COMPARISONS: &[SyntaxKind] = &[
    SyntaxKind::EqEq,
    SyntaxKind::BangEq,
    SyntaxKind::Lt,
    SyntaxKind::LtEq,
    SyntaxKind::Gt,
    SyntaxKind::GtEq,
];

/// `a == a` is always true and `a < a` always false, so one side is a typo.
fn check_self_comparison(context: &Context<'_>, sink: &mut Sink, node: SyntaxNode<'_>) {
    let operator = node
        .child_tokens()
        .find(|token| COMPARISONS.contains(&token.kind));
    let Some(operator) = operator else { return };

    let operands: Vec<SyntaxNode<'_>> = node.child_nodes().collect();
    let [left, right] = operands[..] else { return };
    if !same_code(context, left, right) {
        return;
    }
    sink.report(
        "comparison-with-itself",
        significant_range(node),
        format!(
            "both sides of `{}` are the same expression",
            context.token_text(operator)
        ),
    );
}

/// Whether two subtrees are the same code, ignoring how they were laid out.
fn same_code(context: &Context<'_>, left: SyntaxNode<'_>, right: SyntaxNode<'_>) -> bool {
    let spelling = |node: SyntaxNode<'_>| -> Vec<(SyntaxKind, String)> {
        significant_tokens(node)
            .into_iter()
            .map(|token| (token.kind, context.token_text(token).to_string()))
            .collect()
    };
    let left = spelling(left);
    // A call on both sides may still differ in what it returns, so comparing
    // it to itself is not necessarily pointless. Names and members are.
    if left.iter().any(|(kind, _)| *kind == SyntaxKind::LParen) {
        return false;
    }
    !left.is_empty() && left == spelling(right)
}

// -- doc-tag ----------------------------------------------------------------

/// The tags Godot recognises in a `##` block.
///
/// `@tutorial` also takes a title, as `@tutorial(Title): url`.
const DOC_TAGS: &[&str] = &["tutorial", "deprecated", "experimental"];

/// A `##` tag Godot will silently ignore.
///
/// The documentation is explicit that a tag "must be at the beginning of a
/// line (ignoring preceding white space) and must have the format `@`,
/// followed by the keyword", and warns that "if there is any space in between
/// the tag name and colon, for example `@tutorial  :`, it won't be treated as
/// a valid tag and will be ignored".
///
/// Ignored means ignored in silence. Godot reports nothing, the tutorial link
/// or the deprecation mark just never reaches the help window, and the author
/// has no way of finding out short of going to look.
///
/// Only something already reaching for a known tag is reported. A `##` line
/// that happens to start with `@` and resembles no tag — prose about
/// `@export`, an email address — is left alone, since guessing there would
/// cost more than the rule is worth.
fn check_doc_tag(context: &Context<'_>, sink: &mut Sink, token: Token) {
    let text = context.token_text(token);
    let Some(body) = text.strip_prefix("##") else {
        return;
    };
    let indent = body.len() - body.trim_start().len();
    let rest = body.trim_start();
    let Some(after_at) = rest.strip_prefix('@') else {
        return;
    };

    let keyword: String = after_at
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    if keyword.is_empty() {
        return;
    }
    // Where the `@` sits, for a range that points at the tag itself.
    let at = token.range.start() + 2 + indent as u32;
    let tag_len = 1 + keyword.len() as u32;
    let range = TextRange::new(at, at + tag_len);

    if let Some(known) = DOC_TAGS.iter().find(|tag| **tag == keyword) {
        // A known keyword, so the only thing that can be wrong is what comes
        // after it. `@deprecated` and `@experimental` take nothing; only the
        // ones that take a colon can be spoiled by a space before it.
        let tail = &after_at[keyword.len()..];
        let title = if tail.starts_with('(') {
            tail.find(')').map_or(0, |end| end + 1)
        } else {
            0
        };
        let after_title = &tail[title..];
        let spaces = after_title.len() - after_title.trim_start().len();
        if spaces > 0 && after_title.trim_start().starts_with(':') {
            let space_at = at + tag_len + title as u32;
            sink.report_with_fix(
                "doc-tag",
                range,
                format!("`@{known}` is followed by a space before its colon, so Godot ignores it"),
                Fix::new(vec![Edit::delete(TextRange::new(
                    space_at,
                    space_at + spaces as u32,
                ))]),
            );
        }
        return;
    }

    // Not a tag Godot knows. Reported only when it is close enough to one to
    // be an attempt at it rather than prose that opens with an `@`.
    if let Some(near) = DOC_TAGS.iter().find(|tag| is_near_miss(&keyword, tag)) {
        sink.report(
            "doc-tag",
            range,
            format!("`@{keyword}` is not a documentation tag; did you mean `@{near}`?"),
        );
    }
}

/// Whether `keyword` is one edit away from `tag`.
///
/// One substitution, insertion or deletion. Enough for a typo, tight enough
/// that an unrelated word starting with `@` is not dragged in.
fn is_near_miss(keyword: &str, tag: &str) -> bool {
    if keyword == tag {
        return false;
    }
    let (a, b): (Vec<char>, Vec<char>) = (keyword.chars().collect(), tag.chars().collect());
    match a.len().abs_diff(b.len()) {
        0 => a.iter().zip(&b).filter(|(x, y)| x != y).count() == 1,
        1 => {
            // The longer one with a single character removed must equal the
            // shorter: walk both, allowing one skip.
            let (long, short) = if a.len() > b.len() {
                (&a, &b)
            } else {
                (&b, &a)
            };
            let mut skipped = false;
            let mut j = 0;
            for &c in long {
                if j < short.len() && short[j] == c {
                    j += 1;
                } else if skipped {
                    return false;
                } else {
                    skipped = true;
                }
            }
            j == short.len()
        }
        _ => false,
    }
}

// -- loop-variable-assignment -----------------------------------------------

/// Writing to the loop variable, where the value is then never read.
///
/// The language reference is direct about it:
///
/// > The loop variable is local to the for-loop and assigning to it will not
/// > change the value on the array.
///
/// So `for s in strings: s = "x"` writes nothing anywhere. The author almost
/// always believed they were writing back into the collection, and nothing
/// says otherwise — it is a silent no-op.
///
/// Using the loop variable as an ordinary local is a different thing and
/// perfectly reasonable:
///
/// ```gdscript
/// for s in strings:
///     s = s.strip_edges()
///     print(s)
/// ```
///
/// That is why the value has to be dead before this reports. A rule that
/// fired on the above would be switched off within a day, and take the real
/// findings with it.
fn check_loop_variable_assignment(context: &Context<'_>, sink: &mut Sink, node: SyntaxNode<'_>) {
    let Some(variable) = node
        .child_tokens()
        .find(|token| token.kind == SyntaxKind::Ident)
    else {
        return;
    };
    let name = context.token_text(variable);
    let Some(body) = node.child_node_of(SyntaxKind::Block) else {
        return;
    };

    // A nested loop binding the same name would make "which variable is this?"
    // a question worth answering properly. Rather than answer it badly, say
    // nothing about either loop.
    let shadowed = body
        .descendants()
        .filter(|inner| inner.kind() == SyntaxKind::ForStmt)
        .any(|inner| {
            inner
                .child_tokens()
                .find(|token| token.kind == SyntaxKind::Ident)
                .is_some_and(|token| context.token_text(token) == name)
        });
    if shadowed {
        return;
    }

    let tokens = significant_tokens(body);
    for assignment in body
        .descendants()
        .filter(|inner| inner.kind() == SyntaxKind::AssignStmt)
    {
        // A plain `=`. `+=` and friends read the variable as well, and a
        // target that is a field or an index — `s.x = 1`, `s[0] = 1` — reaches
        // through to an object and does have an effect.
        if assignment.child_token_of(SyntaxKind::Eq).is_none() {
            continue;
        }
        let Some(target) = assignment.child_nodes().next() else {
            continue;
        };
        let target_tokens = significant_tokens(target);
        if target_tokens.len() != 1 || context.token_text(target_tokens[0]) != name {
            continue;
        }

        // Anything after the assignment that names the variable again is a
        // read of what was just written, which makes it an ordinary local.
        // The assignment's own right-hand side is excluded by starting at its
        // end, since `s = s + 1` reads the old value, not the new one.
        let end = assignment.range().end();
        let read_later = tokens.iter().any(|token| {
            token.range.start() >= end
                && token.kind == SyntaxKind::Ident
                && context.token_text(*token) == name
        });
        if read_later {
            continue;
        }

        sink.report(
            "loop-variable-assignment",
            assignment.range(),
            format!(
                "assigning to `{name}` changes nothing: it is the loop variable, so the \
                 collection is untouched, and the value is never read"
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use gdck_config::LintConfig;

    use crate::Diagnostic;

    fn diagnostics(source: &str) -> Vec<Diagnostic> {
        crate::lint(&gdck_syntax::parse(source), &LintConfig::default())
    }

    fn fired(source: &str) -> Vec<&'static str> {
        diagnostics(source)
            .into_iter()
            .map(|diagnostic| diagnostic.rule)
            .collect()
    }

    fn fixed(source: &str) -> String {
        crate::apply_fixes(source, &diagnostics(source))
    }

    #[test]
    fn pass_beside_a_statement_is_removed() {
        assert_eq!(
            fired("func f():\n\tprint(1)\n\tpass\n"),
            ["unnecessary-pass"]
        );
        assert_eq!(
            fixed("func f():\n\tprint(1)\n\tpass\n"),
            "func f():\n\tprint(1)\n"
        );
    }

    #[test]
    fn pass_alone_in_a_block_is_what_it_is_for() {
        assert_eq!(fired("func f():\n\tpass\n"), Vec::<&str>::new());
        assert_eq!(
            fired("func f(a):\n\tif a:\n\t\tpass\n\telse:\n\t\tprint(a)\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn pass_sharing_a_line_is_reported_without_a_fix() {
        // Removing it is an edit to a line, not the deletion of one.
        let found = diagnostics("func f():\n\tprint(1); pass\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "unnecessary-pass");
        assert!(found[0].fix.is_none());
    }

    #[test]
    fn an_expression_statement_has_to_do_something() {
        assert_eq!(fired("func f(a):\n\ta + 1\n"), ["expression-not-assigned"]);
        assert_eq!(fired("func f(a):\n\ta.b\n"), ["expression-not-assigned"]);
        assert_eq!(fired("func f(a):\n\tprint(a)\n"), Vec::<&str>::new());
        assert_eq!(fired("func f(a):\n\ta.method()\n"), Vec::<&str>::new());
        assert_eq!(fired("func f(a):\n\tawait a.ready\n"), Vec::<&str>::new());
    }

    #[test]
    fn a_bare_string_is_a_docstring() {
        assert_eq!(
            fired("extends Node\n\n\"A description.\"\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn an_assignment_target_is_not_an_expression_statement() {
        // The tree nests one inside the assignment; reaching it would report
        // every assignment in the file.
        assert_eq!(
            fired("func f():\n\tvar a = 1\n\ta = 2\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn loading_the_same_path_twice_is_reported_once() {
        let found =
            diagnostics("const A = preload(\"res://x.gd\")\nconst B = preload(\"res://x.gd\")\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "duplicated-load");
        assert!(found[0].message.contains("line 1"));
    }

    #[test]
    fn the_quote_style_does_not_hide_a_duplicate_load() {
        assert!(
            fired("const A = preload(\"res://x.gd\")\nconst B = preload('res://x.gd')\n")
                .contains(&"duplicated-load")
        );
        assert_eq!(
            fired("const A = preload(\"res://x.gd\")\nconst B = preload(\"res://y.gd\")\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn an_unused_argument_is_reported() {
        assert_eq!(fired("func f(a, b):\n\tprint(a)\n"), ["unused-argument"]);
        assert_eq!(fired("func f(a, b):\n\tprint(a, b)\n"), Vec::<&str>::new());
    }

    #[test]
    fn an_underscore_says_the_argument_is_unused_on_purpose() {
        // Which is what an overridden virtual method needs.
        assert_eq!(
            fired("func _process(_delta):\n\tprint(1)\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn an_argument_used_only_in_a_lambda_still_counts() {
        assert_eq!(
            fired("func f(a):\n\tvar g = func():\n\t\treturn a\n\treturn g\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_function_with_no_body_has_no_unused_arguments() {
        assert_eq!(
            fired("@abstract\nclass_name Shape\n\n@abstract func scale(factor: float) -> void\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn comparing_a_value_to_itself_is_reported() {
        assert_eq!(
            fired("func f(a):\n\tif a == a:\n\t\tpass\n"),
            ["comparison-with-itself"]
        );
        assert_eq!(
            fired("func f(a):\n\tif a.b < a.b:\n\t\tpass\n"),
            ["comparison-with-itself"]
        );
        assert_eq!(
            fired("func f(a, b):\n\tif a == b:\n\t\tpass\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_call_compared_to_itself_may_still_differ() {
        // `randi() == randi()` is a question, not a tautology.
        assert_eq!(
            fired("func f():\n\tif randi() == randi():\n\t\tpass\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_doc_tag_godot_would_ignore_is_reported() {
        // The documentation warns that a space before the colon makes the tag
        // invalid, and Godot then drops it without a word.
        assert_eq!(
            fired("## Doc.\n## @tutorial  : https://example.com\nextends Node\n"),
            vec!["doc-tag"]
        );
        assert_eq!(
            fixed("## Doc.\n## @tutorial  : https://example.com\nextends Node\n"),
            "## Doc.\n## @tutorial: https://example.com\nextends Node\n"
        );
        // The titled form has the same trap after the closing bracket.
        assert_eq!(
            fixed("## @tutorial(Two)  : https://example.com\nextends Node\n"),
            "## @tutorial(Two): https://example.com\nextends Node\n"
        );
    }

    #[test]
    fn a_misspelled_doc_tag_is_named_but_not_fixed() {
        // Which tag was meant is the author's to say, so this reports only.
        let source = "## @experimentl\nextends Node\n";
        assert_eq!(fired(source), vec!["doc-tag"]);
        assert_eq!(fixed(source), source);
    }

    #[test]
    fn well_formed_tags_and_ordinary_prose_are_left_alone() {
        for source in [
            "## @tutorial: https://example.com\nextends Node\n",
            "## @tutorial(Title): https://example.com\nextends Node\n",
            "## @deprecated\nextends Node\n",
            "## @experimental\nextends Node\n",
            // Prose that opens with an `@` and is not reaching for a tag.
            "## @export is written about here, not used.\nextends Node\n",
            "## @someunrelatedthing\nextends Node\n",
            // A plain comment is not a documentation comment at all.
            "# @tutorial  : https://example.com\nextends Node\n",
        ] {
            assert_eq!(fired(source), Vec::<&str>::new(), "for {source:?}");
        }
    }

    #[test]
    fn a_dead_write_to_the_loop_variable_is_reported() {
        assert_eq!(
            fired("func f(a):\n\tfor s in a:\n\t\ts = \"x\"\n"),
            vec!["loop-variable-assignment"]
        );
    }

    #[test]
    fn using_the_loop_variable_as_a_local_is_left_alone() {
        // The value is read after the write, so this is an ordinary local and
        // the author knows what they are doing.
        assert_eq!(
            fired("func f(a):\n\tfor s in a:\n\t\ts = s.strip_edges()\n\t\tprint(s)\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn writing_through_the_loop_variable_is_left_alone() {
        // `n.add_to_group(...)` is the docs' own counter-example: calling a
        // method on the loop variable does reach the object.
        assert_eq!(
            fired("func f(a):\n\tfor n in a:\n\t\tn.add_to_group(\"g\")\n"),
            Vec::<&str>::new()
        );
        // A field or an index reaches through in the same way.
        assert_eq!(
            fired("func f(a):\n\tfor n in a:\n\t\tn.x = 1\n"),
            Vec::<&str>::new()
        );
        assert_eq!(
            fired("func f(a):\n\tfor n in a:\n\t\tn[0] = 1\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_compound_assignment_reads_the_variable_so_is_left_alone() {
        assert_eq!(
            fired("func f(a):\n\tfor s in a:\n\t\ts += \"x\"\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_nested_loop_binding_the_same_name_is_reported_once() {
        // The write belongs to the inner `s` and is dead, so it is a real
        // finding — but only the inner loop may claim it. Without the guard
        // the outer loop reported the same line a second time, as though its
        // own variable had been written.
        assert_eq!(
            fired("func f(a):\n\tfor s in a:\n\t\tfor s in a:\n\t\t\ts = \"x\"\n"),
            vec!["loop-variable-assignment"]
        );
    }

    #[test]
    fn an_outer_loop_does_not_claim_an_inner_loops_write() {
        // Different names, so no shadowing: the inner write is the inner
        // variable's, and the outer variable is never written at all.
        assert_eq!(
            fired("func f(a):\n\tfor s in a:\n\t\tfor t in a:\n\t\t\tt = \"x\"\n"),
            vec!["loop-variable-assignment"]
        );
    }
}
