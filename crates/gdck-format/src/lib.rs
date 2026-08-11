//! GDScript formatter.
//!
//! Formatting runs in two stages. Lowering turns the concrete syntax tree
//! into a document describing where lines *may* break, and the renderer decides
//! where they *do*, given the configured width. Keeping those apart means the
//! style-guide rules live in one place instead of being spread across string
//! concatenation.
//!
//! # What the guide asks for
//!
//! Most of it falls out of the document IR: the 100-column wrap, one space
//! around operators and after commas, two blank lines around top-level
//! definitions and one inside a class, trailing commas on collections that
//! break, and two indent levels on continuation lines against one inside
//! arrays, dictionaries and enums.
//!
//! The rest is explicit: quote style chosen to minimise escapes, lowercase
//! hexadecimal, a digit either side of a float's point, single-line inner
//! class declarations, and redundant parentheses dropped. Those live in
//! the `literal` and `lower` modules.
//!
//! # Safety checks
//!
//! Before returning, the formatter re-parses its own output and checks that it
//! still parses, that the tree still means the same thing, that no comment was
//! dropped, and that a second pass is a no-op. A formatter that silently eats code is far
//! worse than one that refuses to run, so these are on by default;
//! [`FormatConfig::safety_checks`] turns them off.

mod doc;
pub mod literal;
mod lower;
mod trivia;

use std::fmt;

use gdck_config::FormatConfig;
use gdck_syntax::{Element, SyntaxKind, SyntaxNode, SyntaxTree};

use crate::lower::Lowerer;
use crate::trivia::Trivia;

/// Why formatting could not be completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The input could not be parsed, so there is nothing safe to rewrite.
    Unparseable,
    /// Formatting changed the meaning of the code. Always a bug in `gdck`.
    SafetyCheckFailed(&'static str),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unparseable => f.write_str("cannot format a file with syntax errors"),
            Self::SafetyCheckFailed(what) => {
                write!(f, "formatting was rejected by a safety check: {what}")
            }
        }
    }
}

impl std::error::Error for FormatError {}

/// Format a parsed GDScript file.
///
/// # Errors
///
/// Returns [`FormatError::Unparseable`] if the tree holds syntax errors, or
/// [`FormatError::SafetyCheckFailed`] if the output does not survive the
/// checks described on this module.
pub fn format(tree: &SyntaxTree, config: &FormatConfig) -> Result<String, FormatError> {
    if tree.has_errors() {
        return Err(FormatError::Unparseable);
    }

    let output = render(tree, config);

    if !config.safety_checks {
        return Ok(output);
    }

    let reparsed = gdck_syntax::parse(&output);
    if reparsed.has_errors() {
        return Err(FormatError::SafetyCheckFailed(
            "the formatted output does not parse",
        ));
    }
    if canonical(tree) != canonical(&reparsed) {
        return Err(FormatError::SafetyCheckFailed(
            "formatting changed the code",
        ));
    }
    let before = Trivia::collect(tree);
    let after = Trivia::collect(&reparsed);
    if before.all_comments() != after.all_comments() {
        return Err(FormatError::SafetyCheckFailed("a comment was lost"));
    }
    let second = render(&reparsed, config);
    if second != output {
        return Err(FormatError::SafetyCheckFailed(
            "formatting is not idempotent",
        ));
    }

    Ok(output)
}

/// Format source text directly, parsing it first.
///
/// # Errors
///
/// As [`format()`].
pub fn format_source(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    format(&gdck_syntax::parse(source), config)
}

fn render(tree: &SyntaxTree, config: &FormatConfig) -> String {
    let trivia = Trivia::collect(tree);
    let lowerer = Lowerer::new(tree, &trivia);
    let document = lowerer.source_file(tree.root());
    let mut output = doc::render(&document, config.line_length as usize, config.indent);

    // A document always ends with the file's final break; collapse whatever
    // that produced to exactly one line feed.
    while output.ends_with('\n') {
        output.pop();
    }
    if !output.is_empty() {
        output.push('\n');
    }
    output
}

/// One step of a tree's canonical form. See [`canonical`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Enter(SyntaxKind),
    Token(SyntaxKind, String),
    /// The operator of an initializer, spelled the same however it was written.
    Operator(&'static str),
}

/// A canonical form capturing what the program *means*.
///
/// Comparing flat token streams would be simpler, but it would reject the
/// rewrites the style guide asks for: hoisting an inner class's `extends` onto
/// the declaration line moves tokens, and dropping a redundant parenthesis
/// removes them. Comparing tree shape instead is both weaker in the right
/// places and stronger in the important one — grouping is encoded by the
/// nesting, so a parenthesis that actually mattered shows up as a differently
/// shaped expression rather than as two missing tokens.
///
/// Elided deliberately:
///
/// * `ParenExpr`, which only ever expressed grouping the tree already records.
/// * Commas and semicolons, which separate siblings the tree already orders.
/// * The position of an inner class's `extends`, canonicalised to the header.
/// * Literal spelling, since quote style and hexadecimal case may change.
fn canonical(tree: &SyntaxTree) -> Vec<Step> {
    let mut steps = Vec::new();
    walk(tree.root(), tree.text(), &mut steps);
    steps
}

fn walk(node: SyntaxNode<'_>, source: &str, steps: &mut Vec<Step>) {
    match node.kind() {
        // Transparent: its only contribution was grouping, which is now the
        // shape of the tree around it.
        SyntaxKind::ParenExpr => {
            for child in node.child_nodes() {
                walk(child, source, steps);
            }
            return;
        }
        SyntaxKind::ClassDecl => {
            walk_class_decl(node, source, steps);
            return;
        }
        SyntaxKind::Initializer => {
            steps.push(Step::Enter(SyntaxKind::Initializer));
            // `:=`, `: =` and `=` all reduce to which of the two forms it is.
            let inferred = node
                .child_tokens()
                .any(|token| matches!(token.kind, SyntaxKind::ColonEq | SyntaxKind::Colon));
            steps.push(Step::Operator(if inferred { ":=" } else { "=" }));
            for child in node.child_nodes() {
                walk(child, source, steps);
            }
            return;
        }
        _ => {}
    }

    steps.push(Step::Enter(node.kind()));
    for element in node.children() {
        match element {
            Element::Node(id) => walk(node.tree().node(id), source, steps),
            Element::Token(token) => push_token(token, source, steps),
        }
    }
}

/// Emit a class declaration with its `extends` in the header position.
///
/// GDScript allows the parent either there or as the body's first statement,
/// and the formatter moves it, so the comparison has to see both spellings as
/// the same program.
fn walk_class_decl(node: SyntaxNode<'_>, source: &str, steps: &mut Vec<Step>) {
    steps.push(Step::Enter(SyntaxKind::ClassDecl));

    let block = node.child_node_of(SyntaxKind::Block);
    let mut members: Vec<SyntaxNode<'_>> = block
        .map(|block| block.child_nodes().collect())
        .unwrap_or_default();

    let mut extends = node.child_node_of(SyntaxKind::ExtendsDecl);
    if extends.is_none() {
        let body_level = members
            .iter()
            .position(|member| member.kind() == SyntaxKind::ExtendsDecl);
        if let Some(index) = body_level {
            extends = Some(members.remove(index));
        }
    }

    for token in node.child_tokens() {
        push_token(token, source, steps);
    }
    if let Some(extends) = extends {
        walk(extends, source, steps);
    }
    if block.is_some() {
        steps.push(Step::Enter(SyntaxKind::Block));
        for member in members {
            walk(member, source, steps);
        }
    }
}

fn push_token(token: gdck_syntax::Token, source: &str, steps: &mut Vec<Step>) {
    if token.kind.is_trivia()
        || matches!(
            token.kind,
            SyntaxKind::Indent
                | SyntaxKind::Dedent
                | SyntaxKind::Eof
                // Separators the sibling order already records.
                | SyntaxKind::Comma
                | SyntaxKind::Semicolon
        )
    {
        return;
    }
    steps.push(Step::Token(
        token.kind,
        normalize_for_comparison(token, source),
    ));
}

/// Compare literals by value rather than spelling, since the formatter is
/// allowed to change quote style and hexadecimal case.
fn normalize_for_comparison(token: gdck_syntax::Token, source: &str) -> String {
    let text = token.text(source);
    match token.kind {
        SyntaxKind::Int | SyntaxKind::Float => literal::normalize_number(text),
        SyntaxKind::Str
        | SyntaxKind::StringName
        | SyntaxKind::NodePath
        | SyntaxKind::GetNode
        | SyntaxKind::UniqueNode => literal::normalize_string(text),
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str, expected: &str) {
        let formatted = format_source(source, &FormatConfig::default())
            .unwrap_or_else(|error| panic!("failed to format {source:?}: {error}"));
        assert_eq!(formatted, expected, "\ninput was:\n{source}");
    }

    /// Formatting an already-formatted file must change nothing.
    fn check_stable(source: &str) {
        check(source, source);
    }

    #[test]
    fn refuses_to_format_unparseable_input() {
        let tree = gdck_syntax::parse("func f(:\n");
        assert_eq!(
            format(&tree, &FormatConfig::default()),
            Err(FormatError::Unparseable)
        );
    }

    #[test]
    fn an_empty_file_stays_empty() {
        check("", "");
    }

    #[test]
    fn carriage_returns_are_normalised_away() {
        // "Use line feed (LF) characters to break lines, not CRLF or CR."
        check("var x = 1\r\nvar y = 2\r\n", "var x = 1\nvar y = 2\n");
    }

    #[test]
    fn a_file_ends_with_exactly_one_newline() {
        check("var x = 1", "var x = 1\n");
        check("var x = 1\n\n\n", "var x = 1\n");
    }

    #[test]
    fn operators_get_one_space_and_commas_one_after() {
        check(
            "func f():\n\tposition.x=5\n\tmy_array = [4,5,6]\n\tdict [\"key\"] = 5\n\tprint ( \"foo\" )\n",
            "func f():\n\tposition.x = 5\n\tmy_array = [4, 5, 6]\n\tdict[\"key\"] = 5\n\tprint(\"foo\")\n",
        );
    }

    #[test]
    fn an_inner_class_declares_its_parent_on_one_line() {
        // The guide: "For inner classes, use single-line declarations".
        check_stable("class Child extends Parent:\n\tpass\n");
        check(
            "class Child:\n\textends Parent\n\tpass\n",
            "class Child extends Parent:\n\tpass\n",
        );
    }

    #[test]
    fn a_file_level_class_declares_its_parent_on_the_next_line() {
        // The counterpart rule: at file level the two are separate lines.
        check(
            "class_name Player extends Node\n",
            "class_name Player\nextends Node\n",
        );
        check_stable("class_name Player\nextends Node\n");
    }

    #[test]
    fn abstract_stays_on_the_inner_class_line() {
        check_stable("@abstract class MyNode extends Node:\n\tpass\n");
    }

    #[test]
    fn a_functions_annotations_take_a_line_each() {
        // How the Godot documentation writes them: `@rpc(...)` above the func,
        // `@export_range(...)` beside the var.
        check_stable("@rpc(\"any_peer\")\nfunc ping() -> void:\n\tpass\n");
        check(
            "@rpc(\"any_peer\") func ping() -> void:\n\tpass\n",
            "@rpc(\"any_peer\")\nfunc ping() -> void:\n\tpass\n",
        );
        check_stable("@export_range(0, 10) var lives = 3\n");
        // `@abstract` is a modifier, and the language reference writes it
        // inline: `@abstract func draw()`.
        check_stable("@abstract\nclass_name Shape\n\n\n@abstract func area() -> float\n");
    }

    #[test]
    fn one_statement_per_line() {
        check(
            "func f():\n\tif flag: print(\"flagged\")\n",
            "func f():\n\tif flag:\n\t\tprint(\"flagged\")\n",
        );
        check("var a = 1; var b = 2\n", "var a = 1\nvar b = 2\n");
    }

    #[test]
    fn the_ternary_operator_is_the_exception_to_that() {
        check_stable("func f():\n\tnext_state = \"idle\" if is_on_floor() else \"fall\"\n");
    }

    #[test]
    fn definitions_get_two_blank_lines_at_file_level() {
        check(
            "func a():\n\tpass\nfunc b():\n\tpass\n",
            "func a():\n\tpass\n\n\nfunc b():\n\tpass\n",
        );
    }

    #[test]
    fn definitions_get_one_blank_line_inside_a_class() {
        // The guide's own example ends with exactly this shape.
        check_stable("class State:\n\tvar foo = 0\n\n\tfunc _init():\n\t\tprint(\"Hello!\")\n");
    }

    #[test]
    fn blank_line_runs_collapse_to_one() {
        check("var a = 1\n\n\n\nvar b = 2\n", "var a = 1\n\nvar b = 2\n");
    }

    #[test]
    fn redundant_parentheses_are_dropped() {
        check(
            "func f():\n\tif (is_colliding()):\n\t\tqueue_free()\n",
            "func f():\n\tif is_colliding():\n\t\tqueue_free()\n",
        );
    }

    #[test]
    fn parentheses_that_carry_meaning_are_kept() {
        check_stable("var x = (a + b) * c\n");
        check_stable("func f():\n\tif (foo and bar) or not baz:\n\t\tprint(\"yes\")\n");
    }

    #[test]
    fn a_single_line_dictionary_gets_spaces_inside_its_braces() {
        check(
            "var my_dictionary = {key = \"value\"}\n",
            "var my_dictionary = { key = \"value\" }\n",
        );
        check_stable("var empty = {}\n");
    }

    #[test]
    fn collections_take_one_indent_level_and_a_trailing_comma() {
        let long = "var party = [\"Godot\", \"Godette\", \"Steve\", \"a name quite long indeed\", \"and one more that certainly pushes it over\"]\n";
        check(
            long,
            "var party = [\n\t\"Godot\",\n\t\"Godette\",\n\t\"Steve\",\n\t\"a name quite long indeed\",\n\t\"and one more that certainly pushes it over\",\n]\n",
        );
    }

    #[test]
    fn a_short_collection_stays_on_one_line_without_a_trailing_comma() {
        check("var array = [1, 2, 3,]\n", "var array = [1, 2, 3]\n");
        // An array the author spread over several lines stays that way, and
        // gains the trailing comma the guide asks for.
        check(
            "var array = [\n\t1,\n\t2\n]\n",
            "var array = [\n\t1,\n\t2,\n]\n",
        );
    }

    #[test]
    fn comments_stay_with_what_they_document() {
        check_stable("# Sets things up.\nfunc _ready():\n\tpass\n");
        check_stable("var x = 1 # why\n");
        // A comment above a definition belongs to it, so the two blank lines
        // go before the comment rather than between it and the function.
        check(
            "var a = 1\n# Documents f.\nfunc f():\n\tpass\n",
            "var a = 1\n\n\n# Documents f.\nfunc f():\n\tpass\n",
        );
    }

    #[test]
    fn a_trailing_comment_keeps_one_space_before_it() {
        check("var x = 1    # why\n", "var x = 1 # why\n");
    }

    #[test]
    fn comments_at_the_end_of_a_file_survive() {
        check_stable("var x = 1\n\n# the end\n");
    }

    #[test]
    fn a_lambda_written_inline_stays_inline() {
        check_stable("var double = func(x): return x * 2\n");
    }

    #[test]
    fn wrapped_expressions_take_two_indent_levels() {
        // The guide: continuation lines use 2 indent levels so they cannot be
        // mistaken for the block that follows.
        check_stable(
            "var position = Vector2(250, 350)\n\n\nfunc f():\n\tif (\n\t\t\tposition.x > 200\n\t\t\tand position.x < 400\n\t\t\tand position.y > 300\n\t\t\tand position.y < 400\n\t):\n\t\tpass\n",
        );
    }

    #[test]
    fn a_multi_line_lambda_keeps_its_block() {
        check_stable(
            "func f():\n\tbutton.pressed.connect(\n\t\t\tfunc() -> void:\n\t\t\t\tdo_something()\n\t)\n",
        );
    }

    #[test]
    fn accessors_keep_the_form_they_were_written_in() {
        check_stable("var health = max_health:\n\tset(new_health):\n\t\thealth = new_health\n");
        check_stable("var is_active = true:\n\tset = set_is_active\n");
    }

    #[test]
    fn the_safety_check_catches_a_lost_comment() {
        // Nothing should trip this; the test exists so the wiring is exercised
        // rather than merely present.
        let tree = gdck_syntax::parse("# a\nvar x = 1  # b\n## c\nfunc f():\n\tpass\n");
        assert!(format(&tree, &FormatConfig::default()).is_ok());
    }

    #[test]
    fn a_comment_moved_onto_its_own_line_keeps_no_inline_space() {
        // A comment between `=` and its value cannot stay there: anything
        // after it on the line would be commented out, including the comma.
        check(
            "var x = {\n\tname = # why\n\t1\n}\n",
            "var x = {\n\t# why\n\tname = 1,\n}\n",
        );
    }

    #[test]
    fn formatting_is_idempotent_on_awkward_input() {
        let source = "class_name A extends B\nvar x={'k':1,}\nfunc f(a,b=2):\n\tif (a): return\n";
        let first = format_source(source, &FormatConfig::default()).expect("formats");
        let second = format_source(&first, &FormatConfig::default()).expect("formats");
        assert_eq!(first, second);
    }
}
