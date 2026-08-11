//! Code that does not do what it looks like it does.
//!
//! `unused-argument`, `duplicated-load`, `expression-not-assigned`,
//! `comparison-with-itself` and `unnecessary-pass`. None of these are style;
//! each is a line that could be deleted, or one that was meant to say
//! something else.
//!
//! They match `gdtoolkit`'s checks of the same names, deliberately. These are
//! the findings a project has already triaged, and a reimplementation that
//! quietly widened one would present old code as newly broken.

use std::collections::HashMap;

use gdck_format::literal;
use gdck_syntax::{SyntaxKind, SyntaxNode, TextRange, Token};

use super::{Context, Sink, callee_name, name_token, significant_range, significant_tokens};
use crate::{Edit, Fix};

pub(crate) fn check(context: &Context<'_>, sink: &mut Sink) {
    check_duplicated_load(context, sink);

    for node in context.root().descendants() {
        match node.kind() {
            SyntaxKind::SourceFile | SyntaxKind::Block => {
                check_unnecessary_pass(context, sink, node);
                check_expression_statements(sink, node);
            }
            SyntaxKind::FuncDecl => check_unused_arguments(context, sink, node),
            SyntaxKind::BinaryExpr => check_self_comparison(context, sink, node),
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
}
