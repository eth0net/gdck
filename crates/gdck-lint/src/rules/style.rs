//! Style-guide rules about how code is written rather than where it sits.
//!
//! `boolean-operators`, `unnecessary-parens`, `comment-space`, `quote-style`,
//! `number-format`, `redundant-type-hint` and `ambiguous-inferred-type`.
//!
//! Where the formatter already performs a rewrite, the fix offered here is the
//! formatter's own: `quote-style` and `number-format` call
//! [`gdck_format::literal`] rather than reimplementing it. Two implementations
//! that disagreed would show up as `gdck lint --fix` producing something
//! `gdck format` then changed again.

use gdck_format::literal;
use gdck_syntax::{SyntaxKind, SyntaxNode, TextRange, Token};

use super::{Context, Sink, all_tokens, callee_name, significant_tokens, unwrap_parens};
use crate::{Edit, Fix};

pub(crate) fn check(context: &Context<'_>, sink: &mut Sink) {
    for token in all_tokens(context.root()) {
        match token.kind {
            SyntaxKind::AmpAmp => replace_operator(context, sink, token, "and"),
            SyntaxKind::PipePipe => replace_operator(context, sink, token, "or"),
            SyntaxKind::Bang => replace_operator(context, sink, token, "not"),
            SyntaxKind::Comment | SyntaxKind::DocComment => check_comment(context, sink, token),
            SyntaxKind::Int | SyntaxKind::Float => check_number(context, sink, token),
            SyntaxKind::Str
            | SyntaxKind::StringName
            | SyntaxKind::NodePath
            | SyntaxKind::GetNode
            | SyntaxKind::UniqueNode => check_string(context, sink, token),
            _ => {}
        }
    }

    for node in context.root().descendants() {
        match node.kind() {
            SyntaxKind::IfStmt | SyntaxKind::ElifClause | SyntaxKind::WhileStmt => {
                check_condition_parens(context, sink, node);
            }
            SyntaxKind::VarDecl | SyntaxKind::ConstDecl => check_type_hint(context, sink, node),
            _ => {}
        }
    }
}

// -- boolean-operators ------------------------------------------------------

/// Swap a C-style boolean operator for the word the guide asks for.
///
/// The replacement carries whatever spaces it needs, since `a&&b` becoming
/// `aandb` would be a different program rather than a badly spaced one.
fn replace_operator(context: &Context<'_>, sink: &mut Sink, token: Token, word: &str) {
    let source = context.source;
    let before = source[..token.range.start() as usize].chars().next_back();
    let after = source[token.range.end() as usize..].chars().next();

    let mut text = String::with_capacity(word.len() + 2);
    // Nothing needs separating from an opening bracket, and `(not a)` is what
    // the spacing rules want anyway.
    if before.is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[' | '{')) {
        text.push(' ');
    }
    text.push_str(word);
    // `!` twice over is the one case where the operator's neighbour is another
    // operator this rule is about to rewrite, and that one supplies the space.
    if after.is_some_and(|c| !c.is_whitespace() && c != '!') {
        text.push(' ');
    }

    sink.report_with_fix(
        "boolean-operators",
        token.range,
        format!("use `{word}` rather than `{}`", context.token_text(token)),
        Fix::new(vec![Edit::replace(token.range, text)]),
    );
}

// -- unnecessary-parens -----------------------------------------------------

/// Parentheses around a whole condition group nothing the keyword does not.
///
/// `if (a) and (b)` is untouched: there the parentheses wrap operands, and the
/// condition itself is the `and`.
fn check_condition_parens(context: &Context<'_>, sink: &mut Sink, node: SyntaxNode<'_>) {
    let Some(condition) = node
        .child_nodes()
        .find(|child| child.kind() != SyntaxKind::Block)
    else {
        return;
    };
    if condition.kind() != SyntaxKind::ParenExpr {
        return;
    }

    // Every layer at once, so `if ((a)):` takes one run rather than two.
    let mut edits = Vec::new();
    let mut layer = condition;
    while layer.kind() == SyntaxKind::ParenExpr {
        let open = layer.child_token_of(SyntaxKind::LParen);
        let close = layer
            .child_tokens()
            .filter(|token| token.kind == SyntaxKind::RParen)
            .last();
        let (Some(open), Some(close)) = (open, close) else {
            break;
        };
        // `if(a):` needs the parenthesis replaced by a space rather than
        // removed, or the keyword runs into the condition. Only the outermost
        // layer can be up against the keyword; every inner one is preceded by
        // a parenthesis that is going away too.
        let joined = edits.is_empty()
            && context.source[..open.range.start() as usize]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace());
        edits.push(if joined {
            Edit::replace(open.range, " ")
        } else {
            Edit::delete(open.range)
        });
        edits.push(Edit::delete(close.range));

        match layer.child_nodes().next() {
            Some(inner) => layer = inner,
            None => break,
        }
    }
    if edits.is_empty() {
        return;
    }

    sink.report_with_fix(
        "unnecessary-parens",
        condition.range(),
        "parentheses around a condition can be removed",
        Fix::new(edits),
    );
}

// -- comment-space ----------------------------------------------------------

/// Comments the guide requires to have no space after the hash.
const REGION_MARKERS: &[&str] = &["region", "endregion"];

/// A comment starts with a space; commented-out code does not.
///
/// Telling those apart is the whole difficulty, and the guide draws the line
/// by intent rather than by syntax. The test used here is whether the text
/// after the hash *is* GDScript — which this crate has a parser to answer.
/// Parsing alone is too generous, since a single English word parses as an
/// expression, so the text must also look like code rather than prose: hold a
/// bracket or an `=`, or open with a keyword.
///
/// The bias is deliberate. Reporting a comment that was disabled code would be
/// telling the author to break the thing the guide asked them to do; missing
/// one that was prose costs a space.
fn check_comment(context: &Context<'_>, sink: &mut Sink, token: Token) {
    let text = context.token_text(token);
    let hashes = text.len() - text.trim_start_matches('#').len();
    let body = &text[hashes..];

    if body.is_empty() || body.starts_with([' ', '\t']) {
        return;
    }
    if REGION_MARKERS.iter().any(|marker| {
        body.strip_prefix(marker)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    }) {
        return;
    }
    if looks_like_code(body) {
        return;
    }

    let at = TextRange::empty(token.range.start() + hashes as u32);
    sink.report_with_fix(
        "comment-space",
        token.range,
        "a comment starts with a space; only disabled code does not",
        Fix::new(vec![Edit::replace(at, " ")]),
    );
}

fn looks_like_code(body: &str) -> bool {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return false;
    }
    let structural = trimmed.contains(['(', ')', '[', ']', '{', '}', '=']);
    let opens_with_keyword = trimmed
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()
        .and_then(SyntaxKind::from_keyword)
        .is_some();
    (structural || opens_with_keyword) && parses_as_gdscript(trimmed)
}

/// Whether a line of text is GDScript, at class level or inside a function.
///
/// Both have to be tried: `const X = 1` is only legal at class level and
/// `return null` only inside a function, and a comment could have been either.
fn parses_as_gdscript(code: &str) -> bool {
    !gdck_syntax::parse(code).has_errors()
        || !gdck_syntax::parse(&format!("func _gdck_probe():\n\t{code}\n")).has_errors()
}

// -- quote-style and number-format ------------------------------------------

fn check_number(context: &Context<'_>, sink: &mut Sink, token: Token) {
    let text = context.token_text(token);
    let wanted = literal::normalize_number(text);
    if wanted == text {
        return;
    }
    sink.report_with_fix(
        "number-format",
        token.range,
        format!("write `{text}` as `{wanted}`"),
        Fix::new(vec![Edit::replace(token.range, wanted)]),
    );
}

fn check_string(context: &Context<'_>, sink: &mut Sink, token: Token) {
    let text = context.token_text(token);
    let wanted = literal::normalize_string(text);
    if wanted == text {
        return;
    }
    sink.report_with_fix(
        "quote-style",
        token.range,
        format!("write `{text}` as `{wanted}`"),
        Fix::new(vec![Edit::replace(token.range, wanted)]),
    );
}

// -- redundant-type-hint and ambiguous-inferred-type ------------------------

/// The two halves of the guide's advice on inferred types.
///
/// "Prefer `:=` when the type is written on the same line as the assignment,
/// otherwise prefer writing the type explicitly." Each half has a sample the
/// guide marks bad, and each of those is what one of these rules looks for.
fn check_type_hint(context: &Context<'_>, sink: &mut Sink, decl: SyntaxNode<'_>) {
    let Some(initializer) = decl.child_node_of(SyntaxKind::Initializer) else {
        return;
    };
    let Some(value) = initializer.child_nodes().next() else {
        return;
    };
    let value = unwrap_parens(value);

    match decl.child_node_of(SyntaxKind::TypeHint) {
        Some(hint) => check_redundant_hint(context, sink, decl, hint, initializer, value),
        // The guide's advice about what inference leaves unsaid is written
        // about `var`. A `const` is its value, spelled out on the line and
        // never reassigned, so there is nothing later for a reader to be
        // uncertain about.
        None if decl.kind() == SyntaxKind::VarDecl => {
            check_ambiguous_inference(context, sink, decl, initializer, value);
        }
        None => {}
    }
}

/// `var direction: Vector3 = Vector3(1, 2, 3)` — the type is written twice.
///
/// Only the constructor call the guide shows counts. A hint that names a
/// supertype (`var node: Node2D = Sprite2D.new()`) is carrying real
/// information, and the two names differing is what says so.
fn check_redundant_hint(
    context: &Context<'_>,
    sink: &mut Sink,
    decl: SyntaxNode<'_>,
    hint: SyntaxNode<'_>,
    initializer: SyntaxNode<'_>,
    value: SyntaxNode<'_>,
) {
    if value.kind() != SyntaxKind::CallExpr {
        return;
    }
    let Some(constructed) = callee_name(value, context.source) else {
        return;
    };
    let hint_tokens = significant_tokens(hint);
    // `:` then the type, and nothing else — a generic like `Array[int]` is
    // never the plain name a constructor call carries.
    let [colon, named] = hint_tokens[..] else {
        return;
    };
    if colon.kind != SyntaxKind::Colon || context.token_text(named) != constructed {
        return;
    }
    let Some(equals) = initializer.child_token_of(SyntaxKind::Eq) else {
        return;
    };

    let range = TextRange::new(colon.range.start(), equals.range.end());
    let spaced = context.source[..colon.range.start() as usize]
        .chars()
        .next_back()
        .is_some_and(char::is_whitespace);
    let replacement = if spaced { ":=" } else { " :=" };

    sink.report_with_fix(
        "redundant-type-hint",
        super::significant_range(decl),
        format!("`{constructed}` is already named on this line; use `:=`"),
        Fix::new(vec![Edit::replace(range, replacement)]),
    );
}

/// `var health := 0` and `@onready var bar := get_node("UI/Bar")` — the two
/// shapes the guide shows where inference does not say enough.
fn check_ambiguous_inference(
    context: &Context<'_>,
    sink: &mut Sink,
    decl: SyntaxNode<'_>,
    initializer: SyntaxNode<'_>,
    value: SyntaxNode<'_>,
) {
    // Written with `=` rather than `:=`, so nothing was inferred and there is
    // no type here at all to be ambiguous.
    if initializer.child_token_of(SyntaxKind::ColonEq).is_none()
        && initializer.child_token_of(SyntaxKind::Colon).is_none()
    {
        return;
    }

    let range = super::significant_range(decl);
    if is_bare_integer(value) {
        sink.report(
            "ambiguous-inferred-type",
            range,
            "`0` infers as int but may have been meant as a float; write the type explicitly",
        );
        return;
    }
    if is_node_lookup(context, value) {
        sink.report(
            "ambiguous-inferred-type",
            range,
            "a node lookup infers as `Node`; write the type explicitly, or cast with `as`",
        );
    }
}

/// An integer literal, which is the guide's example of inference saying too
/// little: "Typed as int, but it could be that float was intended."
///
/// A float literal is not ambiguous in that way — `300.0` says which it is —
/// so only integers are reported.
fn is_bare_integer(value: SyntaxNode<'_>) -> bool {
    // A unary minus in front of a literal is still a literal to a reader.
    let inner = if value.kind() == SyntaxKind::UnaryExpr {
        match value.child_nodes().next() {
            Some(inner) => inner,
            None => return false,
        }
    } else {
        value
    };
    inner.kind() == SyntaxKind::Literal
        && significant_tokens(inner)
            .first()
            .is_some_and(|token| token.kind == SyntaxKind::Int)
}

/// `get_node(...)`, `$Node` and `%Unique`, none of which the compiler can
/// resolve to anything narrower than `Node`. A cast says what it is, and the
/// guide endorses that spelling, so `CastExpr` is not reported.
fn is_node_lookup(context: &Context<'_>, value: SyntaxNode<'_>) -> bool {
    match value.kind() {
        SyntaxKind::CallExpr => callee_name(value, context.source) == Some("get_node"),
        SyntaxKind::Literal | SyntaxKind::NameRef => {
            significant_tokens(value).first().is_some_and(|token| {
                matches!(token.kind, SyntaxKind::GetNode | SyntaxKind::UniqueNode)
            })
        }
        _ => false,
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
    fn c_style_boolean_operators_become_words() {
        assert_eq!(
            fixed("func f():\n\tif a && b:\n\t\tpass\n"),
            "func f():\n\tif a and b:\n\t\tpass\n"
        );
        assert_eq!(
            fixed("func f():\n\tif a || b:\n\t\tpass\n"),
            "func f():\n\tif a or b:\n\t\tpass\n"
        );
        assert_eq!(
            fixed("func f():\n\tif !a:\n\t\tpass\n"),
            "func f():\n\tif not a:\n\t\tpass\n"
        );
    }

    #[test]
    fn the_replacement_brings_the_spaces_it_needs() {
        // `a&&b` becoming `aandb` would be a different program.
        assert_eq!(
            fixed("func f():\n\tif a&&b:\n\t\tpass\n"),
            "func f():\n\tif a and b:\n\t\tpass\n"
        );
        assert_eq!(
            fixed("func f():\n\tif (!a):\n\t\tpass\n"),
            "func f():\n\tif not a:\n\t\tpass\n"
        );
    }

    #[test]
    fn not_equal_is_not_a_boolean_operator() {
        assert_eq!(
            fired("func f():\n\tif a != b:\n\t\tpass\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn parentheses_around_a_whole_condition_are_dropped() {
        // The guide's own sample.
        assert_eq!(
            fixed("func f():\n\tif (is_colliding()):\n\t\tqueue_free()\n"),
            "func f():\n\tif is_colliding():\n\t\tqueue_free()\n"
        );
        assert_eq!(
            fixed("func f():\n\twhile ((ready)):\n\t\tpass\n"),
            "func f():\n\twhile ready:\n\t\tpass\n"
        );
        assert_eq!(
            fixed("func f():\n\tif(ready):\n\t\tpass\n"),
            "func f():\n\tif ready:\n\t\tpass\n"
        );
    }

    #[test]
    fn parentheses_that_group_operands_are_left_alone() {
        assert_eq!(
            fired("func f():\n\tif (a or b) and c:\n\t\tpass\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_comment_gets_a_space_but_disabled_code_does_not() {
        // Both halves of the guide's good example.
        assert_eq!(
            fired("#print(\"This is disabled code\")\n"),
            Vec::<&str>::new()
        );
        assert_eq!(fired("# This is a comment.\n"), Vec::<&str>::new());
        assert_eq!(fired("#This is a comment.\n"), ["comment-space"]);
        assert_eq!(fixed("#This is a comment.\n"), "# This is a comment.\n");
        // Doc comments follow the same rule.
        assert_eq!(
            fixed("##Documents the class.\n"),
            "## Documents the class.\n"
        );
    }

    #[test]
    fn region_markers_must_not_take_a_space() {
        assert_eq!(fired("#region Setup\n#endregion\n"), Vec::<&str>::new());
    }

    #[test]
    fn disabled_code_is_recognised_by_parsing_it() {
        for disabled in ["#var x = 1", "#pass", "#return null", "#\tprint(1)"] {
            assert_eq!(
                fired(&format!("{disabled}\n")),
                Vec::<&str>::new(),
                "{disabled} is code, not prose"
            );
        }
    }

    #[test]
    fn literals_are_reported_with_the_spelling_the_formatter_would_use() {
        assert_eq!(fired("var x = 0xFB8C0B\n"), ["number-format"]);
        assert_eq!(fixed("var x = 0xFB8C0B\n"), "var x = 0xfb8c0b\n");
        assert_eq!(fixed("var x = .234\n"), "var x = 0.234\n");
        assert_eq!(fixed("var x = 13.\n"), "var x = 13.0\n");
        assert_eq!(fired("var s = 'plain'\n"), ["quote-style"]);
        assert_eq!(fixed("var s = 'plain'\n"), "var s = \"plain\"\n");
        // Single quotes that earn their place stay.
        assert_eq!(fired("var s = 'say \"hi\"'\n"), Vec::<&str>::new());
    }

    #[test]
    fn a_type_written_twice_becomes_an_inference() {
        // The guide: "The type hint has redundant information."
        assert_eq!(
            fired("var direction: Vector3 = Vector3(1, 2, 3)\n"),
            ["redundant-type-hint"]
        );
        assert_eq!(
            fixed("var direction: Vector3 = Vector3(1, 2, 3)\n"),
            "var direction := Vector3(1, 2, 3)\n"
        );
    }

    #[test]
    fn a_hint_that_carries_information_is_left_alone() {
        assert_eq!(
            fired("var node: Node2D = Sprite2D.new()\n"),
            Vec::<&str>::new()
        );
        assert_eq!(fired("var health: int = 0\n"), Vec::<&str>::new());
        // The guide's own good sample for the case inference cannot handle.
        assert_eq!(
            fired("@onready var health_bar: ProgressBar = get_node(\"UI/LifeBar\")\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn inference_that_says_too_little_is_reported() {
        // "Typed as int, but it could be that float was intended."
        assert_eq!(fired("var health := 0\n"), ["ambiguous-inferred-type"]);
        assert_eq!(fired("var offset := -1\n"), ["ambiguous-inferred-type"]);
        // A float literal already says which it is.
        assert_eq!(fired("var speed := 300.0\n"), Vec::<&str>::new());
        // And a constant is its value, so nothing is left open.
        assert_eq!(fired("const MAX_LIVES := 3\n"), Vec::<&str>::new());
        // "The compiler can't infer the exact type and will use Node."
        assert_eq!(
            fired("@onready var health_bar := get_node(\"UI/LifeBar\")\n"),
            ["ambiguous-inferred-type"]
        );
        assert_eq!(
            fired("@onready var sprite := $Sprite2D\n"),
            ["ambiguous-inferred-type"]
        );
    }

    #[test]
    fn a_cast_answers_the_question_inference_could_not() {
        // The guide endorses exactly this spelling.
        assert_eq!(
            fired("@onready var health_bar := get_node(\"UI/LifeBar\") as ProgressBar\n"),
            Vec::<&str>::new()
        );
        // And an unambiguous constructor was never a problem.
        assert_eq!(
            fired("var direction := Vector3(1, 2, 3)\n"),
            Vec::<&str>::new()
        );
    }
}
