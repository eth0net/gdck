//! The style guide's naming conventions.
//!
//! | Rule | Applies to | Convention |
//! |---|---|---|
//! | `class-name` | `class_name X` | `PascalCase` |
//! | `sub-class-name` | `class X:` | `PascalCase` |
//! | `function-name` | `func f()` | `snake_case` |
//! | `variable-name` | `var x`, `for x in` | `snake_case` |
//! | `argument-name` | parameters | `snake_case` |
//! | `constant-name` | `const X` | `CONSTANT_CASE` |
//! | `signal-name` | `signal x` | `snake_case` |
//! | `enum-name` | `enum X` | `PascalCase` |
//! | `enum-member-name` | enum members | `CONSTANT_CASE` |
//! | `file-name` | the file itself | `snake_case`, or `PascalCase` by setting `lint.file-name` |
//!
//! Every convention above is fixed except the last, which is the only one
//! whose subject the language never sees. A file name is not an identifier, so
//! a project naming files after the classes in them has a convention of its
//! own rather than a mistake, and can say so.
//!
//! Two things soften that table, both from the guide itself.
//!
//! A single leading underscore marks a member as private and is accepted
//! wherever a name is checked. The guide states the convention for virtual
//! methods, private functions and private variables; there is no reading on
//! which a private signal or a private constant is then wrong.
//!
//! A name that holds a *class* may be `PascalCase` whatever else it is,
//! because the class it holds is: "Also use PascalCase when loading a class
//! into a constant or a variable", illustrated with
//! `const Weapon = preload("res://weapon.gd")`.
//!
//! No naming rule offers a fix. A name is reached from places one file cannot
//! see — other scripts, scene files, `call()` with a string, signals connected
//! in the editor — so a rename is a decision for a person with a project-wide
//! search in front of them.

use gdck_config::FileNameCase;
use gdck_syntax::{SyntaxKind, SyntaxNode, TextRange};

use super::{Context, Sink, callee_name, name_token, unwrap_parens};
use crate::names;

pub(crate) fn check(context: &Context<'_>, sink: &mut Sink) {
    check_file_name(context, sink);

    for node in context.root().descendants() {
        match node.kind() {
            SyntaxKind::ClassNameDecl => {
                check_name(context, sink, node, "class-name", "class", Case::Pascal);
            }
            SyntaxKind::ClassDecl => check_name(
                context,
                sink,
                node,
                "sub-class-name",
                "inner class",
                Case::Pascal,
            ),
            SyntaxKind::FuncDecl => {
                check_name(
                    context,
                    sink,
                    node,
                    "function-name",
                    "function",
                    Case::Snake,
                );
            }
            SyntaxKind::SignalDecl => {
                check_name(context, sink, node, "signal-name", "signal", Case::Snake);
            }
            SyntaxKind::EnumDecl => {
                check_name(context, sink, node, "enum-name", "enum", Case::Pascal);
            }
            SyntaxKind::EnumVariant => check_name(
                context,
                sink,
                node,
                "enum-member-name",
                "enum member",
                Case::Constant,
            ),
            SyntaxKind::Param => {
                check_name(
                    context,
                    sink,
                    node,
                    "argument-name",
                    "argument",
                    Case::Snake,
                );
            }
            SyntaxKind::ForStmt => check_name(
                context,
                sink,
                node,
                "variable-name",
                "loop variable",
                Case::Snake,
            ),
            SyntaxKind::VarDecl => {
                let case = if holds_a_class(context, node) {
                    Case::SnakeOrClass
                } else {
                    Case::Snake
                };
                check_name(context, sink, node, "variable-name", "variable", case);
            }
            SyntaxKind::ConstDecl => {
                let case = if holds_a_class(context, node) {
                    Case::ConstantOrClass
                } else {
                    Case::Constant
                };
                check_name(context, sink, node, "constant-name", "constant", case);
            }
            _ => {}
        }
    }
}

/// Which spellings a name is allowed to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Pascal,
    Snake,
    Constant,
    /// A variable holding a class: `snake_case` like a variable, or
    /// `PascalCase` like the class it holds.
    SnakeOrClass,
    /// A constant holding a class, likewise.
    ConstantOrClass,
}

impl Case {
    fn accepts(self, name: &str) -> bool {
        match self {
            Self::Pascal => names::is_private_pascal_case(name),
            Self::Snake => names::is_private_snake_case(name),
            Self::Constant => names::is_private_constant_case(name),
            Self::SnakeOrClass => {
                names::is_private_snake_case(name) || names::is_private_pascal_case(name)
            }
            Self::ConstantOrClass => {
                names::is_private_constant_case(name) || names::is_private_pascal_case(name)
            }
        }
    }

    /// How the message describes what was wanted.
    fn wanted(self) -> &'static str {
        match self {
            Self::Pascal => "PascalCase",
            Self::Snake => "snake_case",
            Self::Constant => "CONSTANT_CASE",
            Self::SnakeOrClass => "snake_case, or PascalCase since it holds a class",
            Self::ConstantOrClass => "CONSTANT_CASE, or PascalCase since it holds a class",
        }
    }
}

fn check_name(
    context: &Context<'_>,
    sink: &mut Sink,
    node: SyntaxNode<'_>,
    rule: &'static str,
    what: &str,
    case: Case,
) {
    // An anonymous `enum { A, B }` and a half-typed declaration both have no
    // name token, and neither is this rule's business.
    let Some(token) = name_token(node) else {
        return;
    };
    let name = context.token_text(token);
    if case.accepts(name) {
        return;
    }
    sink.report(
        rule,
        token.range,
        format!("{what} name `{name}` should be {}", case.wanted()),
    );
}

/// Whether a declaration's initialiser produces a class rather than a value.
///
/// `preload("res://weapon.gd")` and `load("res://weapon.gd")` both do, which is
/// what earns the `PascalCase` spelling the guide shows.
fn holds_a_class(context: &Context<'_>, decl: SyntaxNode<'_>) -> bool {
    let Some(initializer) = decl.child_node_of(SyntaxKind::Initializer) else {
        return false;
    };
    let Some(value) = initializer.child_nodes().next() else {
        return false;
    };
    match unwrap_parens(value) {
        value if value.kind() == SyntaxKind::PreloadExpr => true,
        value if value.kind() == SyntaxKind::CallExpr => {
            callee_name(value, context.source) == Some("load")
        }
        _ => false,
    }
}

/// The file itself is named like a variable, and after the class it holds.
///
/// Unless the project says otherwise. This is the one naming rule whose
/// subject the language never sees — a file name is not an identifier — and a
/// project that names files after their classes is keeping a convention rather
/// than failing to. `lint.file-name` says which one, and the rule goes on
/// working either way; switching it off would only stop it noticing anything.
fn check_file_name(context: &Context<'_>, sink: &mut Sink) {
    let Some(file_name) = context.file_name else {
        return;
    };
    let Some(stem) = file_name.strip_suffix(".gd") else {
        // Nothing else is a GDScript file, so there is no convention to hold
        // it to.
        return;
    };
    let (matches, wanted) = match context.config.file_name {
        FileNameCase::SnakeCase => (names::is_private_snake_case(stem), "snake_case"),
        FileNameCase::PascalCase => (names::is_private_pascal_case(stem), "PascalCase"),
    };
    if matches {
        return;
    }
    // The whole file is the subject, so anchor at its first byte rather than
    // nowhere.
    sink.report(
        "file-name",
        TextRange::empty(0),
        format!("file name `{file_name}` should be {wanted}"),
    );
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

    fn message(source: &str) -> String {
        diagnostics(source)
            .into_iter()
            .next()
            .expect("a diagnostic")
            .message
    }

    #[test]
    fn the_guides_own_examples_are_accepted() {
        assert_eq!(
            fired(
                "class_name YAMLParser\nextends Object\n\nsignal door_opened\nsignal score_changed\n\nenum Element {\n\tEARTH,\n\tWATER,\n}\n\nconst MAX_SPEED = 200\nconst Weapon = preload(\"res://weapon.gd\")\n\nvar particle_effect\nvar _counter = 0\n\n\nfunc load_level():\n\tpass\n\n\nfunc _recalculate_path():\n\tpass\n"
            ),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_class_name_is_pascal_case() {
        assert_eq!(fired("class_name my_class\n"), ["class-name"]);
        assert_eq!(fired("class_name MyClass\n"), Vec::<&str>::new());
    }

    #[test]
    fn an_inner_class_name_is_pascal_case() {
        assert_eq!(fired("class inventory_slot:\n\tpass\n"), ["sub-class-name"]);
        assert_eq!(fired("class InventorySlot:\n\tpass\n"), Vec::<&str>::new());
    }

    #[test]
    fn functions_and_variables_are_snake_case() {
        assert_eq!(fired("func LoadLevel():\n\tpass\n"), ["function-name"]);
        assert_eq!(fired("var particleEffect\n"), ["variable-name"]);
        assert_eq!(
            fired("func f(items):\n\tfor Item in items:\n\t\tprint(Item)\n"),
            ["variable-name"]
        );
    }

    #[test]
    fn arguments_are_snake_case() {
        assert_eq!(
            fired("func f(maxHealth):\n\tprint(maxHealth)\n"),
            ["argument-name"]
        );
        // A setter's value is an argument too.
        assert_eq!(
            fired("var health = 0:\n\tset(newHealth):\n\t\thealth = newHealth\n"),
            ["argument-name"]
        );
    }

    #[test]
    fn a_leading_underscore_marks_a_private_member() {
        assert_eq!(
            fired("var _speed = 0\n\n\nfunc _recalculate():\n\tpass\n"),
            Vec::<&str>::new()
        );
        // Two is not the convention.
        assert_eq!(fired("var __speed = 0\n"), ["variable-name"]);
    }

    #[test]
    fn constants_are_constant_case() {
        assert_eq!(fired("const MaxSpeed = 200\n"), ["constant-name"]);
        assert_eq!(fired("const MAX_SPEED = 200\n"), Vec::<&str>::new());
    }

    #[test]
    fn a_name_holding_a_class_may_be_pascal_case() {
        // The guide: "Also use PascalCase when loading a class into a constant
        // or a variable."
        assert_eq!(
            fired("const Weapon = preload(\"res://weapon.gd\")\n"),
            Vec::<&str>::new()
        );
        assert_eq!(
            fired("var Weapon = load(\"res://weapon.gd\")\n"),
            Vec::<&str>::new()
        );
        // The exemption is for what it holds, not for the spelling alone.
        assert_eq!(fired("var Weapon = 1\n"), ["variable-name"]);
        // And a method that happens to be called `load` is not the global one.
        assert_eq!(
            fired("var Weapon = resources.load(\"res://weapon.gd\")\n"),
            ["variable-name"]
        );
    }

    #[test]
    fn enums_are_pascal_case_with_constant_case_members() {
        assert_eq!(
            fired("enum element {\n\tearth,\n\twater,\n}\n"),
            ["enum-name", "enum-member-name", "enum-member-name"]
        );
        assert_eq!(
            fired("enum Element {\n\tEARTH,\n\tWATER,\n}\n"),
            Vec::<&str>::new()
        );
        // An anonymous enum has no name to check.
        assert_eq!(fired("enum {\n\tEARTH,\n}\n"), Vec::<&str>::new());
    }

    #[test]
    fn signals_are_snake_case() {
        assert_eq!(fired("signal DoorOpened\n"), ["signal-name"]);
        assert_eq!(fired("signal door_opened\n"), Vec::<&str>::new());
    }

    #[test]
    fn a_file_name_is_only_checked_when_it_is_known() {
        let tree = gdck_syntax::parse("extends Node\n");
        let config = LintConfig::default();
        assert!(crate::lint(&tree, &config).is_empty());
        assert!(crate::lint_file(&tree, &config, Some("yaml_parser.gd")).is_empty());
        let found = crate::lint_file(&tree, &config, Some("YAMLParser.gd"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "file-name");
    }

    /// A project that names files after their classes says so, and keeps the
    /// rule rather than turning it off — the point being that it still catches
    /// the file that follows neither convention.
    #[test]
    fn a_project_can_ask_for_pascal_case_file_names() {
        let tree = gdck_syntax::parse("extends Node\n");
        let config = LintConfig {
            file_name: gdck_config::FileNameCase::PascalCase,
            ..LintConfig::default()
        };
        assert!(crate::lint_file(&tree, &config, Some("GdUnitCmdTool.gd")).is_empty());
        assert!(crate::lint_file(&tree, &config, Some("RPCMessage.gd")).is_empty());

        let found = crate::lint_file(&tree, &config, Some("yaml_parser.gd"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "file-name");
        assert!(
            found[0].message.contains("PascalCase"),
            "the message should name the convention the project chose, got {:?}",
            found[0].message
        );
    }

    /// Neither setting has an opinion about a file that is not GDScript.
    #[test]
    fn a_file_name_setting_only_applies_to_gd_files() {
        let tree = gdck_syntax::parse("extends Node\n");
        for case in [
            gdck_config::FileNameCase::SnakeCase,
            gdck_config::FileNameCase::PascalCase,
        ] {
            let config = LintConfig {
                file_name: case,
                ..LintConfig::default()
            };
            assert!(crate::lint_file(&tree, &config, Some("README.md")).is_empty());
        }
    }

    #[test]
    fn the_message_names_the_convention_it_wanted() {
        assert_eq!(
            message("var particleEffect\n"),
            "variable name `particleEffect` should be snake_case"
        );
        assert_eq!(
            message("const Weapon = 1\n"),
            "constant name `Weapon` should be CONSTANT_CASE"
        );
    }
}
