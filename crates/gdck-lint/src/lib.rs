//! GDScript linter.
//!
//! # Status
//!
//! Not implemented. [`lint`] returns an empty list for every input. The rule
//! catalogue below is the planned scope and is what `docs/RULES.md` documents
//! for users.
//!
//! # Planned approach
//!
//! Rules are visitors over the CST, run in a single pass. Each produces a
//! [`Diagnostic`] carrying a byte range and, where a mechanical fix exists, an
//! edit. `gdck lint --fix` applies non-overlapping edits back to front so that
//! earlier offsets stay valid.
//!
//! Suppression follows the comment syntax existing GDScript projects already
//! use: `# gdlint: ignore=rule-name` for one line and
//! `# gdlint: disable=rule-name` / `enable=` for a region.

use gdck_config::LintConfig;
use gdck_syntax::{SyntaxTree, TextRange};

/// How much a diagnostic matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// One reported problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Kebab-case rule name, e.g. `function-name`.
    pub rule: &'static str,
    pub severity: Severity,
    pub range: TextRange,
    pub message: String,
}

/// The rules `gdck` intends to ship, grouped as the style guide groups them.
///
/// Naming, formatting and code-order rules come straight from the style guide.
/// The design rules are thresholds inherited from `gdtoolkit`, which existing
/// GDScript projects are already configured against.
pub const PLANNED_RULES: &[(&str, &str)] = &[
    // Naming conventions.
    ("class-name", "class names use PascalCase"),
    ("function-name", "function names use snake_case"),
    ("variable-name", "variable names use snake_case"),
    ("constant-name", "constant names use CONSTANT_CASE"),
    ("signal-name", "signal names use snake_case and past tense"),
    ("enum-name", "enum names use PascalCase"),
    ("enum-member-name", "enum members use CONSTANT_CASE"),
    ("file-name", "file names use snake_case"),
    // Formatting, where the fix is the formatter itself.
    ("line-too-long", "lines stay under the configured width"),
    ("trailing-whitespace", "no whitespace at end of line"),
    ("mixed-indentation", "no mixing of tabs and spaces"),
    ("tab-indentation", "indent with tabs, not spaces"),
    ("final-newline", "files end with exactly one line feed"),
    // Style-guide rules the formatter cannot infer on its own.
    ("code-order", "declarations follow the style guide's order"),
    (
        "boolean-operators",
        "use `and`/`or`/`not`, not `&&`/`||`/`!`",
    ),
    (
        "unnecessary-parens",
        "no parentheses around a bare condition",
    ),
    (
        "comment-space",
        "comments start with a space, disabled code does not",
    ),
    (
        "quote-style",
        "prefer double quotes unless that adds escapes",
    ),
    (
        "number-format",
        "lowercase hex, leading and trailing zeros on floats",
    ),
    (
        "inferred-type",
        "use `:=` when the type is obvious, else annotate",
    ),
    // Design thresholds.
    ("max-file-lines", "files stay under the configured length"),
    (
        "max-public-methods",
        "classes stay under the method threshold",
    ),
    ("max-returns", "functions stay under the return threshold"),
    (
        "max-arguments",
        "functions stay under the argument threshold",
    ),
    // Correctness smells.
    ("unused-argument", "function arguments are used"),
    ("duplicated-load", "the same resource is not loaded twice"),
    ("expression-not-assigned", "expression results are used"),
    (
        "comparison-with-itself",
        "a value is not compared to itself",
    ),
    (
        "unnecessary-pass",
        "`pass` only appears in an otherwise empty block",
    ),
];

/// Lint a parsed GDScript file.
#[must_use]
pub fn lint(_tree: &SyntaxTree, _config: &LintConfig) -> Vec<Diagnostic> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_nothing_until_rules_are_written() {
        let tree = gdck_syntax::parse("var BadName = 1\n");
        assert!(lint(&tree, &LintConfig::default()).is_empty());
    }

    #[test]
    fn rule_names_are_unique_and_kebab_case() {
        let mut names: Vec<_> = PLANNED_RULES.iter().map(|(name, _)| *name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate rule name");
        for (name, description) in PLANNED_RULES {
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{name} is not kebab-case"
            );
            assert!(!description.is_empty(), "{name} has no description");
        }
    }
}
