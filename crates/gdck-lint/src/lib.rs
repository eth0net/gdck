//! GDScript linter.
//!
//! Rules are grouped visitors over the concrete syntax tree. Each produces a
//! [`Diagnostic`] carrying a byte range and, where a mechanical rewrite exists,
//! a [`Fix`]. [`apply_fixes`] applies non-overlapping fixes back to front so
//! that earlier offsets stay valid.
//!
//! # What is checked
//!
//! [`RULES`] is the catalogue, and `docs/RULES.md` documents it for users. It
//! covers the style guide's naming conventions, its formatting rules, the
//! declaration order it prescribes, the design thresholds inherited from
//! `gdtoolkit`, and a handful of correctness smells.
//!
//! # Suppression
//!
//! The comment syntax existing GDScript projects already use is honoured:
//!
//! ```gdscript
//! var BadName = 1  # gdlint: ignore=variable-name
//!
//! # gdlint: disable=variable-name
//! var AlsoBad = 2
//! # gdlint: enable=variable-name
//! ```
//!
//! `gdtoolkit`'s rule names are accepted as aliases wherever a rule is named,
//! so a project's existing suppression comments keep working.
//!
//! # Example
//!
//! ```
//! use gdck_config::LintConfig;
//!
//! let tree = gdck_syntax::parse("var BadName = 1\n");
//! let diagnostics = gdck_lint::lint(&tree, &LintConfig::default());
//! assert_eq!(diagnostics[0].rule, "variable-name");
//! ```

mod names;
mod rules;
mod suppress;

use std::collections::HashSet;
use std::fmt;

use gdck_config::LintConfig;
use gdck_syntax::{LineIndex, SyntaxTree, TextRange};

/// How much a diagnostic matters.
///
/// The distinction is advisory: `gdck` exits non-zero for either. An error is
/// code that does not do what it looks like it does; a warning is code that
/// works but departs from the style guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

/// One replacement of a byte range with new text.
///
/// An insertion is an edit over an empty range, and a deletion is an edit with
/// empty text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: TextRange,
    pub text: String,
}

impl Edit {
    #[must_use]
    pub fn replace(range: TextRange, text: impl Into<String>) -> Self {
        Self {
            range,
            text: text.into(),
        }
    }

    #[must_use]
    pub fn delete(range: TextRange) -> Self {
        Self {
            range,
            text: String::new(),
        }
    }
}

/// A mechanical rewrite that resolves a diagnostic.
///
/// Several edits because one problem is not always one place: dropping a
/// redundant pair of parentheses removes two tokens that are not adjacent.
/// Either all of a fix's edits are applied or none are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub edits: Vec<Edit>,
}

impl Fix {
    #[must_use]
    pub fn new(edits: Vec<Edit>) -> Self {
        Self { edits }
    }
}

/// One reported problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Kebab-case rule name, e.g. `function-name`. Always one of [`RULES`].
    pub rule: &'static str,
    pub severity: Severity,
    pub range: TextRange,
    pub message: String,
    /// The rewrite `--fix` would apply, when one exists.
    pub fix: Option<Fix>,
}

/// One entry in the rule catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    /// Kebab-case name, used in reports, in `disable` lists and in `# gdlint:`
    /// comments.
    pub name: &'static str,
    /// One line, phrased as the thing the rule wants to be true.
    pub description: &'static str,
    pub severity: Severity,
    /// Whether the rule ever carries a fix. Some carry one only sometimes.
    pub fixable: bool,
    /// `gdtoolkit` names accepted as this rule, so existing configuration and
    /// suppression comments keep working.
    pub aliases: &'static [&'static str],
}

/// The rules `gdck` ships, grouped as the style guide groups them.
///
/// Naming, formatting and code-order rules come from the style guide. The
/// design thresholds are inherited from `gdtoolkit`, which existing GDScript
/// projects are already configured against.
pub const RULES: &[Rule] = &[
    // ---- Naming conventions -------------------------------------------
    // Renaming is never offered as a fix. A name is referenced from places
    // this crate cannot see — other scripts, scene files, `call()` by string,
    // the editor's signal connections — so a rename is a decision for a person
    // with a project-wide search, not for a linter with one file.
    Rule {
        name: "class-name",
        description: "class_name declarations use PascalCase",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "sub-class-name",
        description: "inner class names use PascalCase",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "function-name",
        description: "function names use snake_case",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "variable-name",
        description: "variable names use snake_case",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[
            "class-variable-name",
            "function-variable-name",
            "loop-variable-name",
            "class-load-variable-name",
            "function-preload-variable-name",
        ],
    },
    Rule {
        name: "argument-name",
        description: "function arguments use snake_case",
        severity: Severity::Warning,
        fixable: false,
        aliases: &["function-argument-name"],
    },
    Rule {
        name: "constant-name",
        description: "constants use CONSTANT_CASE, or PascalCase when they hold a class",
        severity: Severity::Warning,
        fixable: false,
        aliases: &["load-constant-name"],
    },
    Rule {
        name: "signal-name",
        description: "signal names use snake_case",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "enum-name",
        description: "enum names use PascalCase",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "enum-member-name",
        description: "enum members use CONSTANT_CASE",
        severity: Severity::Warning,
        fixable: false,
        aliases: &["enum-element-name"],
    },
    Rule {
        name: "file-name",
        description: "file names use snake_case",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    // ---- Formatting ----------------------------------------------------
    Rule {
        name: "line-too-long",
        description: "lines stay under the configured width",
        severity: Severity::Warning,
        fixable: false,
        aliases: &["max-line-length"],
    },
    Rule {
        name: "trailing-whitespace",
        description: "no whitespace at the end of a line",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
    Rule {
        name: "mixed-indentation",
        description: "no mixing of tabs and spaces in one line's indentation",
        severity: Severity::Warning,
        fixable: false,
        aliases: &["mixed-tabs-and-spaces"],
    },
    Rule {
        name: "tab-indentation",
        description: "indent with tabs, not spaces",
        severity: Severity::Warning,
        fixable: false,
        aliases: &["tab-characters"],
    },
    Rule {
        name: "line-ending",
        description: "lines end with a line feed, not CRLF or CR",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
    Rule {
        name: "final-newline",
        description: "files end with exactly one line feed",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
    // ---- Style-guide rules the formatter cannot infer on its own -------
    Rule {
        name: "boolean-operators",
        description: "use `and`, `or` and `not` rather than `&&`, `||` and `!`",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
    Rule {
        name: "unnecessary-parens",
        description: "no parentheses around a bare condition",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
    Rule {
        name: "comment-space",
        description: "comments start with a space; commented-out code does not",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
    Rule {
        name: "quote-style",
        description: "prefer double quotes unless that adds escapes",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
    Rule {
        name: "number-format",
        description: "lowercase hexadecimal, and a digit on both sides of a float's point",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
    Rule {
        name: "redundant-type-hint",
        description: "use `:=` when the type is already written on the line",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
    Rule {
        name: "ambiguous-inferred-type",
        description: "write the type explicitly when `:=` cannot make it obvious",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "code-order",
        description: "declarations follow the style guide's order",
        severity: Severity::Warning,
        fixable: false,
        aliases: &["class-definitions-order"],
    },
    // ---- Design thresholds ---------------------------------------------
    Rule {
        name: "max-file-lines",
        description: "files stay under the configured length",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "max-public-methods",
        description: "classes stay under the public method threshold",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "max-returns",
        description: "functions stay under the return threshold",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "max-arguments",
        description: "functions stay under the argument threshold",
        severity: Severity::Warning,
        fixable: false,
        aliases: &["function-arguments-number"],
    },
    // ---- Correctness smells --------------------------------------------
    Rule {
        name: "unused-argument",
        description: "function arguments are used, or named with a leading underscore",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "duplicated-load",
        description: "the same resource is not loaded twice",
        severity: Severity::Warning,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "expression-not-assigned",
        description: "an expression statement has an effect",
        severity: Severity::Error,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "comparison-with-itself",
        description: "a value is not compared to itself",
        severity: Severity::Error,
        fixable: false,
        aliases: &[],
    },
    Rule {
        name: "unnecessary-pass",
        description: "`pass` appears only in an otherwise empty block",
        severity: Severity::Warning,
        fixable: true,
        aliases: &[],
    },
];

/// Look a rule up by its name or by one of its `gdtoolkit` aliases.
#[must_use]
pub fn rule(name: &str) -> Option<&'static Rule> {
    RULES
        .iter()
        .find(|rule| rule.name == name || rule.aliases.contains(&name))
}

/// Lint a parsed GDScript file.
///
/// Use [`lint_file`] when the file's name is known; the `file-name` rule has
/// nothing to check without it.
#[must_use]
pub fn lint(tree: &SyntaxTree, config: &LintConfig) -> Vec<Diagnostic> {
    lint_file(tree, config, None)
}

/// Lint a parsed GDScript file whose name is known.
///
/// `file_name` is the final path component, e.g. `player.gd`.
///
/// A tree with syntax errors is still linted. Unlike formatting, reporting on
/// a file cannot damage it, and the rules that can still say something useful
/// about a half-written file are the ones you most want while typing.
#[must_use]
pub fn lint_file(
    tree: &SyntaxTree,
    config: &LintConfig,
    file_name: Option<&str>,
) -> Vec<Diagnostic> {
    let disabled: HashSet<&'static str> = config
        .disabled
        .iter()
        .filter_map(|name| rule(name))
        .map(|rule| rule.name)
        .collect();

    let context = rules::Context {
        tree,
        source: tree.text(),
        config,
        file_name,
    };
    let mut sink = rules::Sink::new(disabled);
    rules::run(&context, &mut sink);
    let mut diagnostics = sink.finish();

    // Sorting by position rather than by the order the rule groups ran in is
    // what makes a report read like the file.
    diagnostics.sort_by_key(|diagnostic| (diagnostic.range.start(), diagnostic.rule));

    let suppressions = suppress::Suppressions::collect(tree);
    if !suppressions.is_empty() {
        let lines = LineIndex::new(tree.text());
        diagnostics.retain(|diagnostic| {
            let line = lines.line_col(diagnostic.range.start()).line;
            !suppressions.suppresses(diagnostic.rule, line)
        });
    }

    diagnostics
}

/// What came of trying to put a file's declarations in the guide's order.
///
/// See [`reorder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reorder {
    /// Every declaration is already where the guide puts it.
    Unchanged,
    /// The reordered file. Its blank lines have moved with the declarations
    /// they sat above, so run the formatter over it before writing it out.
    Reordered(String),
    /// Nothing was moved, and this is why.
    Blocked(String),
}

/// Put a file's declarations in the style guide's order.
///
/// Reordering is not cosmetic. Class-level variable initialisers run in
/// declaration order, so hoisting a public variable above the private one it
/// reads leaves it initialised from `null` — a game that breaks at runtime
/// with no error and no diff to blame.
///
/// So this is all or nothing for the whole file: either every move it needs is
/// provably safe, or the file is returned untouched with the declaration that
/// blocked it named. Safety is judged conservatively — an initialiser that
/// calls a function defined in this file, or touches `self`, is treated as
/// reading every member above it. That occasionally refuses a file that would
/// have been fine, and cannot be wrong in the direction that breaks a game.
#[must_use]
pub fn reorder(source: &str, config: &LintConfig) -> Reorder {
    rules::reorder(source, config)
}

/// How many times [`fix_source`] re-lints. A fix deferred for overlapping
/// another is picked up on the next pass, and in practice one more pass always
/// settles it; the cap is a backstop against a rule that fights itself.
const MAX_FIX_PASSES: usize = 5;

/// Apply every available fix, repeatedly, until nothing more changes.
///
/// Returns the source unchanged if there was nothing to fix.
///
/// Two fixes that overlap cannot both be applied in one pass, so this re-lints
/// and applies again until it settles. If a pass ever makes a file that parsed
/// stop parsing, the result is discarded and the last good text returned: a
/// linter that breaks a file is worse than one that leaves a warning standing.
#[must_use]
pub fn fix_source(source: &str, config: &LintConfig, file_name: Option<&str>) -> String {
    let mut text = source.to_string();
    let was_valid = !gdck_syntax::parse(&text).has_errors();

    for _ in 0..MAX_FIX_PASSES {
        let tree = gdck_syntax::parse(&text);
        let next = apply_fixes(&text, &lint_file(&tree, config, file_name));
        if next == text {
            break;
        }
        if was_valid && gdck_syntax::parse(&next).has_errors() {
            break;
        }
        text = next;
    }
    text
}

/// Apply every fix whose edits do not overlap one already applied.
///
/// Edits go in back to front so that offsets computed against the original
/// text stay valid as the string shrinks and grows. Where two fixes touch the
/// same bytes the earlier one in the list wins and the other is left for the
/// next run — applying both would splice one rewrite into the middle of
/// another.
///
/// Overlap is judged edit by edit rather than over the span a fix covers.
/// Dropping the parentheses from `if (!a):` is two deletions with the `!`
/// between them, and rewriting that `!` in the same pass is not a conflict.
#[must_use]
pub fn apply_fixes(source: &str, diagnostics: &[Diagnostic]) -> String {
    let mut applied: Vec<TextRange> = Vec::new();
    let mut edits: Vec<&Edit> = Vec::new();

    for diagnostic in diagnostics {
        let Some(fix) = &diagnostic.fix else { continue };
        if fix.edits.is_empty() {
            continue;
        }
        let conflicts = fix
            .edits
            .iter()
            .any(|edit| applied.iter().any(|other| overlaps(*other, edit.range)));
        if conflicts {
            continue;
        }
        applied.extend(fix.edits.iter().map(|edit| edit.range));
        edits.extend(&fix.edits);
    }

    // Descending, so applying one cannot move the next.
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.start()));

    let mut output = source.to_string();
    for edit in edits {
        let range = edit.range.start() as usize..edit.range.end() as usize;
        output.replace_range(range, &edit.text);
    }
    output
}

/// Whether two spans share a byte, treating an insertion point inside another
/// span as an overlap.
fn overlaps(a: TextRange, b: TextRange) -> bool {
    a.start() < b.end() && b.start() < a.end()
        || a.is_empty() && a.start() >= b.start() && a.start() <= b.end()
        || b.is_empty() && b.start() >= a.start() && b.start() <= a.end()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostics(source: &str) -> Vec<Diagnostic> {
        lint(&gdck_syntax::parse(source), &LintConfig::default())
    }

    /// Every rule fired by `source`, in order.
    fn fired(source: &str) -> Vec<&'static str> {
        diagnostics(source)
            .into_iter()
            .map(|diagnostic| diagnostic.rule)
            .collect()
    }

    #[test]
    fn rule_names_are_unique_and_kebab_case() {
        let mut names: Vec<&str> = RULES
            .iter()
            .flat_map(|rule| std::iter::once(rule.name).chain(rule.aliases.iter().copied()))
            .collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "a rule name or alias is used twice");

        for rule in RULES {
            assert!(
                rule.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{} is not kebab-case",
                rule.name
            );
            assert!(
                !rule.description.is_empty(),
                "{} has no description",
                rule.name
            );
        }
    }

    #[test]
    fn aliases_resolve_to_their_rule() {
        assert_eq!(
            rule("variable-name").map(|rule| rule.name),
            Some("variable-name")
        );
        assert_eq!(
            rule("class-variable-name").map(|rule| rule.name),
            Some("variable-name")
        );
        assert_eq!(
            rule("function-arguments-number").map(|rule| rule.name),
            Some("max-arguments")
        );
        assert_eq!(rule("no-such-rule"), None);
    }

    #[test]
    fn a_clean_file_reports_nothing() {
        assert_eq!(
            fired(
                "class_name Player\nextends Node\n\nconst MAX_SPEED := 300.0\n\n\nfunc _ready() -> void:\n\tpass\n"
            ),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn diagnostics_come_back_in_source_order() {
        let source = "var Bad = 1\nvar AlsoBad = 2\n";
        let found = diagnostics(source);
        assert_eq!(found.len(), 2);
        assert!(found[0].range.start() < found[1].range.start());
    }

    #[test]
    fn a_disabled_rule_reports_nothing() {
        let config = LintConfig {
            disabled: vec!["variable-name".to_string()],
            ..LintConfig::default()
        };
        let tree = gdck_syntax::parse("var BadName = 1\n");
        assert!(lint(&tree, &config).is_empty());
    }

    #[test]
    fn a_rule_can_be_disabled_by_its_gdtoolkit_name() {
        // So that a `gdlintrc` carried over from gdtoolkit keeps working.
        let config = LintConfig {
            disabled: vec!["class-variable-name".to_string()],
            ..LintConfig::default()
        };
        let tree = gdck_syntax::parse("var BadName = 1\n");
        assert!(lint(&tree, &config).is_empty());
    }

    #[test]
    fn a_file_with_syntax_errors_is_still_linted() {
        // Reporting cannot damage a file, so unlike formatting there is no
        // reason to refuse.
        let tree = gdck_syntax::parse("var BadName = 1\nfunc f(:\n\tpass\n");
        assert!(tree.has_errors());
        assert!(
            lint(&tree, &LintConfig::default())
                .iter()
                .any(|diagnostic| diagnostic.rule == "variable-name")
        );
    }

    #[test]
    fn fixes_are_applied_back_to_front() {
        let source = "var a = 1   \nvar b = 2  \n";
        let found = diagnostics(source);
        assert_eq!(apply_fixes(source, &found), "var a = 1\nvar b = 2\n");
    }

    #[test]
    fn fixing_repeats_until_nothing_changes() {
        // The parentheses and the `!` inside them are two fixes on one line.
        let source = "func f(a):\n\tif (!a):\n\t\tprint(a)   \n";
        assert_eq!(
            fix_source(source, &LintConfig::default(), None),
            "func f(a):\n\tif not a:\n\t\tprint(a)\n"
        );
    }

    #[test]
    fn fixing_a_clean_file_changes_nothing() {
        let source = "extends Node\n\n\nfunc _ready() -> void:\n\tpass\n";
        assert_eq!(fix_source(source, &LintConfig::default(), None), source);
    }

    #[test]
    fn overlapping_fixes_leave_one_for_the_next_run() {
        let mut first = Diagnostic {
            rule: "trailing-whitespace",
            severity: Severity::Warning,
            range: TextRange::new(0, 3),
            message: String::new(),
            fix: Some(Fix::new(vec![Edit::replace(TextRange::new(0, 3), "x")])),
        };
        let second = Diagnostic {
            range: TextRange::new(1, 4),
            fix: Some(Fix::new(vec![Edit::replace(TextRange::new(1, 4), "y")])),
            ..first.clone()
        };
        first.message = String::new();
        assert_eq!(apply_fixes("abcdef", &[first, second]), "xdef");
    }
}
