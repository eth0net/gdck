//! Lossless lexing and parsing for [GDScript](https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/).
//!
//! The tree this crate produces keeps every byte of the input, including
//! whitespace, comments and blank lines. That is a deliberate constraint rather
//! than an implementation detail: a formatter that rewrites one declaration has
//! to leave the comments around it exactly where they were, and a linter that
//! reports a problem has to point at a real byte offset.
//!
//! Parsing never fails. Malformed input produces [`SyntaxKind::Error`] nodes
//! and diagnostics on [`SyntaxTree::errors`], so tools can report several
//! problems per run and editors can work with a half-typed buffer.
//!
//! # Example
//!
//! ```
//! let tree = gdck_syntax::parse("func _ready() -> void:\n\tpass\n");
//! assert!(!tree.has_errors());
//! // The source is always recoverable from the tree.
//! assert_eq!(tree.text(), "func _ready() -> void:\n\tpass\n");
//! ```

mod error;
mod kind;
mod lexer;
mod parser;
mod text;
mod tree;

pub use error::SyntaxError;
pub use kind::SyntaxKind;
pub use lexer::{LexResult, Token, tokenize};
pub use parser::parse;
pub use text::{LineCol, LineIndex, TextRange};
pub use tree::{Checkpoint, Descendants, Element, NodeId, SyntaxNode, SyntaxTree, TreeBuilder};

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant every downstream tool relies on.
    fn assert_round_trips(source: &str) {
        let tree = parse(source);
        assert_eq!(tree.text(), source, "tree must reproduce its input exactly");
    }

    fn assert_parses(source: &str) -> SyntaxTree {
        let tree = parse(source);
        assert!(
            !tree.has_errors(),
            "expected a clean parse of:\n{source}\ngot: {:?}",
            tree.errors()
        );
        assert_eq!(tree.text(), source);
        tree
    }

    /// Collect the kinds of every node in the tree, for structural assertions.
    fn node_kinds(tree: &SyntaxTree) -> Vec<SyntaxKind> {
        tree.root().descendants().map(SyntaxNode::kind).collect()
    }

    #[test]
    fn parses_an_empty_file() {
        let tree = assert_parses("");
        assert_eq!(tree.root().kind(), SyntaxKind::SourceFile);
    }

    #[test]
    fn parses_class_level_declarations() {
        let tree = assert_parses(
            "@tool\nclass_name Player\nextends CharacterBody2D\n\n## The player.\nsignal died\nsignal hit(damage: int, source: Node)\n\nenum State { IDLE, WALKING = 2, }\n\nconst MAX_SPEED := 300.0\nconst GRAVITY: float = 9.8\n\n@export var health: int = 100\n@export_range(0, 10) var lives := 3\nvar _private_state: State = State.IDLE\n@onready var sprite: Sprite2D = $Sprite2D\n",
        );
        let kinds = node_kinds(&tree);
        assert!(kinds.contains(&SyntaxKind::ClassNameDecl));
        assert!(kinds.contains(&SyntaxKind::ExtendsDecl));
        assert!(kinds.contains(&SyntaxKind::SignalDecl));
        assert!(kinds.contains(&SyntaxKind::EnumDecl));
        assert!(kinds.contains(&SyntaxKind::ConstDecl));
        assert!(kinds.contains(&SyntaxKind::VarDecl));
        assert!(kinds.contains(&SyntaxKind::Annotation));
    }

    #[test]
    fn annotations_attach_to_the_declaration_they_modify() {
        let tree = assert_parses("@export var health := 100\n");
        let var_decl = tree
            .root()
            .descendants()
            .find(|node| node.kind() == SyntaxKind::VarDecl)
            .expect("a var declaration");
        // `@export` must live inside the VarDecl, not beside it, so moving the
        // declaration moves its annotations too.
        assert!(
            var_decl
                .child_nodes()
                .any(|child| child.kind() == SyntaxKind::Annotation),
            "annotation should be a child of the declaration"
        );
    }

    #[test]
    fn file_level_annotations_stay_separate() {
        let tree = assert_parses("@tool\nclass_name Foo\n");
        let class_name = tree
            .root()
            .descendants()
            .find(|node| node.kind() == SyntaxKind::ClassNameDecl)
            .expect("a class_name declaration");
        // `@tool` describes the file, not the class_name, so it must not be
        // swallowed by it.
        assert!(
            !class_name
                .child_nodes()
                .any(|child| child.kind() == SyntaxKind::Annotation)
        );
    }

    #[test]
    fn parses_functions_and_control_flow() {
        assert_parses(
            "func _physics_process(delta: float) -> void:\n\tif health <= 0:\n\t\tdied.emit()\n\telif health < 20:\n\t\tblink()\n\telse:\n\t\tpass\n\n\tfor i in range(10):\n\t\tprint(i)\n\n\twhile true:\n\t\tbreak\n\n\tmatch state:\n\t\tState.IDLE:\n\t\t\tpass\n\t\tvar other when other > 2:\n\t\t\tpass\n\t\t_:\n\t\t\treturn\n",
        );
    }

    #[test]
    fn parses_static_and_abstract_members() {
        assert_parses("static var count := 0\n\nstatic func reset() -> void:\n\tcount = 0\n");
    }

    #[test]
    fn parses_inner_classes() {
        let tree = assert_parses(
            "class Inventory extends RefCounted:\n\tvar items: Array[String] = []\n\n\tfunc add(item: String) -> void:\n\t\titems.append(item)\n",
        );
        assert!(node_kinds(&tree).contains(&SyntaxKind::ClassDecl));
    }

    #[test]
    fn parses_property_accessors() {
        let tree = assert_parses(
            "var health := 100:\n\tset(value):\n\t\thealth = maxi(value, 0)\n\tget:\n\t\treturn health\n",
        );
        let kinds = node_kinds(&tree);
        assert!(kinds.contains(&SyntaxKind::Setter));
        assert!(kinds.contains(&SyntaxKind::Getter));
    }

    #[test]
    fn parses_expressions() {
        assert_parses(
            "func f():\n\tvar a = 1 + 2 * 3 - -4\n\tvar b = a > 1 and not a < 0 or false\n\tvar c = [1, 2, {\"key\": \"value\", other = 2}]\n\tvar d = c[0].method(1, 2).field\n\tvar e = \"yes\" if a else \"no\"\n\tvar g = a as float\n\tvar h = a is int\n\tvar i = await something()\n\tvar j = preload(\"res://x.tscn\")\n\tvar k = func(x): return x * 2\n\tvar m = 2 ** 3 ** 2\n",
        );
    }

    #[test]
    fn parses_assignment_operators() {
        let tree = assert_parses(
            "func f():\n\tx = 1\n\tx += 1\n\tx **= 2\n\tx >>= 1\n\tx[0] = 2\n\tx.y.z = 3\n",
        );
        let assignments = node_kinds(&tree)
            .iter()
            .filter(|kind| **kind == SyntaxKind::AssignStmt)
            .count();
        assert_eq!(assignments, 6);
    }

    #[test]
    fn parses_inline_bodies_and_semicolons() {
        assert_parses("func f():\n\tif true: pass\n\tvar a = 1; var b = 2\n");
    }

    #[test]
    fn parses_wrapped_lines() {
        assert_parses(
            "func f():\n\tvar a = [\n\t\t1,\n\t\t2,\n\t]\n\tif a \\\n\t\t\tand true:\n\t\tpass\n\tvar b = (\n\t\t1\n\t\t+ 2\n\t)\n",
        );
    }

    #[test]
    fn round_trips_regardless_of_errors() {
        // Losslessness must survive syntax errors, or a formatter could destroy
        // a file it merely failed to understand.
        for source in [
            "func f(:\n\tpass\n",
            "var = 5\n",
            "class_name\n",
            "func f():\n\treturn ]\n",
            "@\n",
            "if if if\n",
            "enum { \n",
            "var s = \"unterminated\n",
            "\t\tweird_indent()\n",
            "% ^ &\n",
        ] {
            assert_round_trips(source);
        }
    }

    #[test]
    fn reports_errors_without_giving_up() {
        let tree = parse("func f(:\n\tpass\n\nfunc g():\n\tpass\n");
        assert!(tree.has_errors());
        // Recovery must find the second function despite the broken first one.
        let functions = node_kinds(&tree)
            .iter()
            .filter(|kind| **kind == SyntaxKind::FuncDecl)
            .count();
        assert_eq!(functions, 2, "parser should recover and see both functions");
    }

    #[test]
    fn preserves_comments_in_the_tree() {
        let source = "# leading\nfunc f():  # trailing\n\t# inner\n\tpass\n";
        let tree = assert_parses(source);
        let comments: Vec<_> = tree
            .root()
            .descendants()
            .flat_map(SyntaxNode::child_tokens)
            .filter(|token| token.kind.is_comment())
            .map(|token| token.text(source))
            .collect();
        assert_eq!(comments, vec!["# leading", "# trailing", "# inner"]);
    }

    #[test]
    fn leading_comments_attach_to_what_they_document() {
        let tree =
            assert_parses("extends Node\n\n# Sets things up.\nfunc _ready() -> void:\n\tpass\n");
        let func = tree
            .root()
            .descendants()
            .find(|node| node.kind() == SyntaxKind::FuncDecl)
            .expect("a function declaration");
        // The comment describes `_ready`, so it must live inside FuncDecl and
        // not trail the `extends` above it — otherwise reordering or
        // reformatting the function would leave its comment behind.
        assert!(
            func.child_tokens().any(|token| token.kind.is_comment()),
            "leading comment should belong to the declaration it precedes"
        );
    }

    #[test]
    fn distinguishes_inferred_from_explicit_types() {
        let tree = assert_parses("var a := 1\nvar b: int = 1\n");
        let has_type_hint = tree
            .root()
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::VarDecl)
            .map(|node| {
                node.child_nodes()
                    .any(|child| child.kind() == SyntaxKind::TypeHint)
            })
            .collect::<Vec<_>>();
        // `:=` carries no TypeHint; `: int` does. This is what the
        // static-typing style rules key off.
        assert_eq!(has_type_hint, vec![false, true]);
    }

    #[test]
    fn parses_variadic_parameters() {
        assert_parses("func foo(a, ...rest: Array) -> void:\n\tpass\n");
    }

    #[test]
    fn parses_not_in_as_one_operator() {
        let tree = assert_parses("func f():\n\tvar a = 1 not in [1] not in [true]\n");
        // Two chained binary operators, not a prefix `not` and a stray `in`.
        let binaries = node_kinds(&tree)
            .iter()
            .filter(|kind| **kind == SyntaxKind::BinaryExpr)
            .count();
        assert_eq!(binaries, 2);
    }

    #[test]
    fn parses_abstract_annotations() {
        // `@abstract` functions have no body at all.
        assert_parses("@abstract\nclass_name Shape\n\n@abstract func area() -> float\n");
        // `abstract` stays a normal identifier everywhere else.
        assert_parses("func f():\n\tvar abstract = 1\n");
    }

    #[test]
    fn parses_annotations_inside_function_bodies() {
        assert_parses(
            "func a():\n\t@warning_ignore(\"unused_variable\")\n\tvar x: Array[int] = [1, 2]\n\nfunc b():\n\t@warning_ignore(\"shadowed\") @warning_ignore(\"unused\") var y = 1\n",
        );
    }

    #[test]
    fn parses_semicolon_separated_declarations() {
        assert_parses("const x = 1; const y = 2\nconst z = 3;\n");
    }

    #[test]
    fn parses_docstrings_at_class_level() {
        assert_parses("\"\"\"docstring\n\"\"\"\n\n\"another\"\n");
    }

    #[test]
    fn parses_absolute_node_paths() {
        assert_parses(
            "func f():\n\t$/root.name = \"x\"\n\t$/root/A/B/C.free()\n\t$../Sibling.show()\n",
        );
    }

    #[test]
    fn sigil_literals_are_recognised_at_the_start_of_a_line() {
        // The previous line ends in a value, which must not make `^` and `&`
        // read as bitwise operators here.
        assert_parses(
            "func f():\n\tvar a = 1\n\t^\"node/path\"\n\t&\"string_name\"\n\t%Unique.show()\n",
        );
    }

    #[test]
    fn parses_inline_property_accessors() {
        assert_parses(
            "var p1: set = __set\nvar p2: set = __set, get = __get\nvar p3:\n\tget = __get,\n\tset = __set\n",
        );
    }

    #[test]
    fn parses_spaced_inference_operator() {
        assert_parses("var is_enabled : = true\n");
    }

    #[test]
    fn parses_match_patterns() {
        assert_parses(
            "func f(x):\n\tmatch x:\n\t\t[1, 2, [1, {1: 2, 2: var z, ..}]]:\n\t\t\tpass\n\t\t{\"name\", \"age\"}:\n\t\t\tpass\n\t\t{\"key\": \"v\", ..}:\n\t\t\tpass\n\t\t1 if true else 2:\n\t\t\tpass\n\t\tvar other when other > 2:\n\t\t\tpass\n\t\t_:\n\t\t\tpass\n",
        );
    }

    #[test]
    fn parses_multiline_lambdas_inside_brackets() {
        // Indentation is meaningless inside brackets, except within a lambda
        // body — the one case where the lexer has to turn it back on.
        assert_parses(
            "func f(source):\n\tstack(func():\n\t\tprint(\"foo\")\n\t\tif source == 1:\n\t\t\tpass)\n",
        );
        // A lambda body spanning lines inside a call spread over lines.
        assert_parses(
            "func f(button):\n\tbutton.pressed.connect(\n\t\tfunc() -> void:\n\t\t\tvar test := \"\"\n\t\t\tuse(test)\n\t)\n",
        );
        // Two lambdas in one array, the comma ending the first body.
        assert_parses("func f():\n\tvar fs = [func():\n\t\treturn [1, 2, 3], func():\n\t\tpass]\n");
        // A lambda whose body starts on the line it opened on.
        assert_parses("func f():\n\tvar g = [func():\n\t\tpass]\n\tuse(g)\n");
    }

    #[test]
    fn nested_lambdas_close_in_the_right_order() {
        assert_parses(
            "func f():\n\tvar a = [func():\n\t\tpass\n\t\tvar b = func():\n\t\t\tpass\n\t\t\tvar d = {\"f\": func():\n\t\t\t\tpass\n\t\t\t\treturn [1, 2, 3]}]\n",
        );
    }

    #[test]
    fn brackets_inside_a_lambda_body_still_suppress_indentation() {
        // The array spans lines inside the lambda; only the lambda body itself
        // gets indent handling back.
        assert_parses(
            "func f():\n\tcall(func():\n\t\tvar xs = [\n\t\t\t1,\n\t\t\t2,\n\t\t]\n\t\tuse(xs))\n",
        );
    }

    #[test]
    fn handles_crlf_and_missing_trailing_newline() {
        assert_parses("var a = 1\r\nvar b = 2\r\n");
        assert_parses("func f():\n\tpass");
    }

    #[test]
    fn node_ranges_are_consistent_with_text() {
        let source = "func hello() -> void:\n\tprint(\"hi\")\n";
        let tree = parse(source);
        for node in tree.root().descendants() {
            let range = node.range();
            assert!(
                range.end() as usize <= source.len(),
                "{:?} range {range} escapes the source",
                node.kind()
            );
            assert_eq!(node.text(), range.slice(source));
        }
    }
}
