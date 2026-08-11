//! Reading `gdtoolkit`'s `gdlintrc` and `gdformatrc`.
//!
//! A project that has been using `gdtoolkit` has already decided its line
//! length and which rules it does not want. Making it restate all of that
//! before `gdck` will behave is a reason not to try `gdck` at all, so the
//! existing files are read where `gdck` has an equivalent setting.
//!
//! # What this is not
//!
//! These files are YAML, and this is not a YAML parser. It reads the shapes
//! `gdtoolkit` writes and hand-written files take — `key: value`, a block or
//! flow sequence, and the `!!set` mapping that `yaml.dump` produces for
//! `excluded_directories` — and says so when it finds a line it cannot place.
//!
//! The rule for a foreign file is the opposite of the one for `gdck.toml`:
//! nothing here is fatal. A `gdlintrc` legitimately holds settings `gdck` has
//! no equivalent for, and refusing to run over one would defeat the purpose.
//! What it must not do is *silently* drop something that changes behaviour, so
//! anything ignored that a reader would expect to matter comes back as a note.

use yaml_serde::Value;

use crate::{Config, IndentStyle, Problem};

/// Apply a `gdlintrc` to a configuration, returning what could not be honoured.
pub(crate) fn apply_gdlintrc(text: &str, config: &mut Config) -> Result<Vec<Problem>, Problem> {
    let mut notes = Vec::new();
    for item in read(text)? {
        let note = match item.key.as_str() {
            "disable" => match item.strings() {
                Some(rules) => {
                    config.lint.disabled = rules;
                    None
                }
                None => Some(wanted(&item, "a list of rule names")),
            },
            "max-line-length" => set_u16(&item, &mut config.lint.max_line_length),
            "max-file-lines" => set_u32(&item, &mut config.lint.max_file_lines),
            "max-public-methods" => set_u32(&item, &mut config.lint.max_public_methods),
            "max-returns" => set_u32(&item, &mut config.lint.max_returns),
            "function-arguments-number" => set_u32(&item, &mut config.lint.max_function_arguments),
            "excluded_directories" => match item.strings() {
                Some(dirs) => {
                    config.excluded_dirs = dirs;
                    None
                }
                None => Some(wanted(&item, "a list of directory names")),
            },
            // The order is the style guide's, and it is not configurable —
            // `code-order` is the guide's rule rather than a project's taste.
            "class-definitions-order" => item.strings().and_then(|order| {
                (order != DEFAULT_DEFINITIONS_ORDER).then(|| {
                    note(
                        &item,
                        "gdck orders declarations the way the style guide does; \
                         `class-definitions-order` is not applied",
                    )
                })
            }),
            "tab-characters" => (item.integer() != Some(1)).then(|| {
                note(
                    &item,
                    "gdck indents one tab per level; `tab-characters` is not applied",
                )
            }),
            key => match naming_default(key) {
                // A naming pattern gdck cannot honour, because it checks the
                // guide's conventions directly rather than with a regular
                // expression. Left at its default it changes nothing, so only
                // a project that customised one hears about it.
                Some(default) => item.text().filter(|text| *text != default).map(|_| {
                    note(
                        &item,
                        format!(
                            "gdck checks `{key}` against the style guide's convention \
                             rather than a pattern; this one is not applied"
                        ),
                    )
                }),
                None => unrecognised(&item),
            },
        };
        notes.extend(note);
    }
    Ok(notes)
}

/// Apply a `gdformatrc` to a configuration, returning what could not be honoured.
pub(crate) fn apply_gdformatrc(text: &str, config: &mut Config) -> Result<Vec<Problem>, Problem> {
    let mut notes = Vec::new();
    for item in read(text)? {
        let note = match item.key.as_str() {
            "line_length" => {
                let note = set_u16(&item, &mut config.format.line_length);
                // As in gdck.toml: a project that widened its lines meant both
                // tools, and gdformatrc has nowhere to say otherwise.
                config.lint.max_line_length = config.format.line_length;
                note
            }
            // Absent or null means tabs, which is already the default.
            "use_spaces" => match (item.integer(), item.is_null()) {
                (Some(width @ 1..=16), _) => {
                    let width = u8::try_from(width).expect("the range was just checked");
                    config.format.indent = IndentStyle::Spaces(width);
                    None
                }
                (None, true) => None,
                _ => Some(wanted(&item, "a number of spaces between 1 and 16")),
            },
            "safety_checks" => match (item.boolean(), item.is_null()) {
                (Some(value), _) => {
                    config.format.safety_checks = value;
                    None
                }
                (None, true) => None,
                _ => Some(wanted(&item, "true or false")),
            },
            "excluded_directories" => match item.strings() {
                Some(dirs) => {
                    config.excluded_dirs = dirs;
                    None
                }
                None => Some(wanted(&item, "a list of directory names")),
            },
            _ => unrecognised(&item),
        };
        notes.extend(note);
    }
    Ok(notes)
}

fn set_u16(item: &Item, field: &mut u16) -> Option<Problem> {
    match item.integer().and_then(|value| u16::try_from(value).ok()) {
        Some(value) if value > 0 => {
            *field = value;
            None
        }
        _ => Some(wanted(item, "a positive number")),
    }
}

fn set_u32(item: &Item, field: &mut u32) -> Option<Problem> {
    match item.integer().and_then(|value| u32::try_from(value).ok()) {
        Some(value) => {
            *field = value;
            None
        }
        None => Some(wanted(item, "a positive number")),
    }
}

fn note(item: &Item, message: impl Into<String>) -> Problem {
    Problem {
        line: item.line,
        message: message.into(),
    }
}

fn wanted(item: &Item, wanted: &str) -> Problem {
    note(
        item,
        format!("`{}` should be {wanted}; it is ignored", item.key),
    )
}

/// A key with a value that is not a setting `gdck` knows.
///
/// A bare `key:` with nothing after it is how `gdtoolkit` writes "this rule is
/// on", which is the default anyway, so those pass without comment.
fn unrecognised(item: &Item) -> Option<Problem> {
    (!item.is_null()).then(|| {
        note(
            item,
            format!("gdck has no setting matching `{}`; it is ignored", item.key),
        )
    })
}

// -- gdtoolkit's own defaults ------------------------------------------------

const PASCAL_CASE: &str = "([A-Z][a-z0-9]*)+";
const SNAKE_CASE: &str = "[a-z][a-z0-9]*(_[a-z0-9]+)*";
const PRIVATE_SNAKE_CASE: &str = "_?[a-z][a-z0-9]*(_[a-z0-9]+)*";
const UPPER_SNAKE_CASE: &str = "[A-Z][A-Z0-9]*(_[A-Z0-9]+)*";
const PRIVATE_UPPER_SNAKE_CASE: &str = "_?[A-Z][A-Z0-9]*(_[A-Z0-9]+)*";

/// `gdtoolkit`'s default pattern for a naming check, if it has one.
///
/// Comparing against these is what keeps `gdlint --dump-default-config` output
/// quiet: every pattern in it is the default, so none of them is an override
/// and none of them is worth a note.
fn naming_default(key: &str) -> Option<String> {
    let pattern = match key {
        "function-name" => {
            return Some(format!(
                "(_on_{PASCAL_CASE}(_[a-z0-9]+)*|{PRIVATE_SNAKE_CASE})"
            ));
        }
        "class-name" | "enum-name" | "function-preload-variable-name" => PASCAL_CASE,
        "sub-class-name" => return Some(format!("_?{PASCAL_CASE}")),
        "class-load-variable-name" => {
            return Some(format!("({PASCAL_CASE}|{PRIVATE_SNAKE_CASE})"));
        }
        "load-constant-name" => {
            return Some(format!("({PASCAL_CASE}|{PRIVATE_UPPER_SNAKE_CASE})"));
        }
        "signal-name" | "function-variable-name" => SNAKE_CASE,
        "class-variable-name" | "function-argument-name" | "loop-variable-name" => {
            PRIVATE_SNAKE_CASE
        }
        "enum-element-name" => UPPER_SNAKE_CASE,
        "constant-name" => PRIVATE_UPPER_SNAKE_CASE,
        _ => return None,
    };
    Some(pattern.to_string())
}

/// The order `gdlint` checks by default, which is the one `gdck` implements.
const DEFAULT_DEFINITIONS_ORDER: &[&str] = &[
    "tools",
    "classnames",
    "extends",
    "docstrings",
    "signals",
    "enums",
    "consts",
    "staticvars",
    "exports",
    "pubvars",
    "prvvars",
    "onreadypubvars",
    "onreadyprvvars",
    "others",
];

// -- reading the file -------------------------------------------------------

/// One top-level `key:` and whatever followed it.
#[derive(Debug, Clone)]
struct Item {
    key: String,
    value: Value,
    line: u32,
}

impl Item {
    fn is_null(&self) -> bool {
        self.value.is_null()
    }

    fn integer(&self) -> Option<i64> {
        self.value.as_i64()
    }

    fn boolean(&self) -> Option<bool> {
        self.value.as_bool()
    }

    fn text(&self) -> Option<&str> {
        self.value.as_str()
    }

    /// The value as a list of strings.
    ///
    /// Two shapes carry one. A sequence is the obvious spelling, and the
    /// mapping-of-nulls is what `yaml.dump` writes for a Python set, which is
    /// how `gdtoolkit` stores `excluded_directories`. An empty list still
    /// counts, so `disable: []` clears the list rather than being ignored.
    fn strings(&self) -> Option<Vec<String>> {
        match &self.value {
            Value::Sequence(items) => items
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect(),
            Value::Mapping(members) => members
                .iter()
                .map(|(key, value)| {
                    value
                        .is_null()
                        .then(|| key.as_str().map(str::to_string))
                        .flatten()
                })
                .collect(),
            _ => None,
        }
    }
}

/// Read the top-level mapping.
///
/// Unlike anything else about these files, a parse failure *is* fatal. If the
/// document cannot be read then none of its settings apply, which is the one
/// outcome worse than not running: a project formatted by rules it had written
/// down and rejected, with nothing said about it.
fn read(text: &str) -> Result<Vec<Item>, Problem> {
    let document: Value = yaml_serde::from_str(text).map_err(|error| Problem {
        line: error.location().map_or(1, |at| at.line() as u32),
        message: error.to_string(),
    })?;

    let mapping = match document {
        // An empty file is an empty document, not a broken one.
        Value::Null => return Ok(Vec::new()),
        Value::Mapping(mapping) => mapping,
        other => {
            return Err(Problem {
                line: 1,
                message: format!("expected a mapping of settings, found {}", describe(&other)),
            });
        }
    };

    Ok(mapping
        .into_iter()
        .filter_map(|(key, value)| {
            let key = key.as_str()?.to_string();
            let line = line_of(text, &key);
            Some(Item { key, value, line })
        })
        .collect())
}

fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "nothing",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Sequence(_) => "a list",
        Value::Mapping(_) => "a mapping",
        Value::Tagged(_) => "a tagged value",
    }
}

/// Which line a top-level key was written on.
///
/// The deserialiser reports a location for what it could not read, but not for
/// what it could, and every note here names a line. Top-level keys are unique
/// in a mapping, so finding the one that opens a line is unambiguous — and a
/// key that somehow cannot be found falls back to the top of the file rather
/// than to a wrong answer.
fn line_of(text: &str, key: &str) -> u32 {
    text.lines()
        .position(|line| opens_with_key(line, key))
        .map_or(1, |index| index as u32 + 1)
}

fn opens_with_key(line: &str, key: &str) -> bool {
    // Only a line with no indentation can hold a top-level key.
    if line.starts_with([' ', '\t']) {
        return false;
    }
    let rest = line
        .strip_prefix(key)
        .or_else(|| line.strip_prefix(&format!("\"{key}\"")))
        .or_else(|| line.strip_prefix(&format!("'{key}'")))
        .map(str::trim_start);
    rest.is_some_and(|rest| rest.starts_with(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `class-definitions-order` written out as a YAML block sequence.
    fn written_order() -> String {
        use std::fmt::Write;
        DEFAULT_DEFINITIONS_ORDER
            .iter()
            .fold(String::new(), |mut out, item| {
                let _ = writeln!(out, "  - {item}");
                out
            })
    }

    fn lint(text: &str) -> (Config, Vec<String>) {
        let mut config = Config::default();
        let notes = apply_gdlintrc(text, &mut config).expect("should read");
        (config, notes.into_iter().map(|note| note.message).collect())
    }

    fn format(text: &str) -> (Config, Vec<String>) {
        let mut config = Config::default();
        let notes = apply_gdformatrc(text, &mut config).expect("should read");
        (config, notes.into_iter().map(|note| note.message).collect())
    }

    /// What a file that cannot be read at all reports.
    fn refused(text: &str) -> Problem {
        apply_gdlintrc(text, &mut Config::default()).expect_err("should not read")
    }

    #[test]
    fn the_thresholds_a_gdlintrc_sets_are_applied() {
        let (config, notes) = lint(
            "max-line-length: 120\n\
             max-file-lines: 500\n\
             max-public-methods: 12\n\
             max-returns: 3\n\
             function-arguments-number: 5\n",
        );
        assert_eq!(config.lint.max_line_length, 120);
        assert_eq!(config.lint.max_file_lines, 500);
        assert_eq!(config.lint.max_public_methods, 12);
        assert_eq!(config.lint.max_returns, 3);
        assert_eq!(config.lint.max_function_arguments, 5);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_disable_list_carries_over_in_gdtoolkits_own_names() {
        // Which the linter resolves as aliases, so they need no translating.
        let (config, notes) = lint("disable:\n  - max-public-methods\n  - class-variable-name\n");
        assert_eq!(
            config.lint.disabled,
            ["max-public-methods", "class-variable-name"]
        );
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_disable_list_can_be_written_inline() {
        let (config, _) = lint("disable: [max-returns, unused-argument]\n");
        assert_eq!(config.lint.disabled, ["max-returns", "unused-argument"]);
        let (config, _) = lint("disable: []\n");
        assert!(config.lint.disabled.is_empty());
    }

    #[test]
    fn the_set_that_yaml_dump_writes_is_read_as_a_list() {
        let (config, notes) = lint("excluded_directories: !!set\n  .git: null\n  addons: null\n");
        assert_eq!(config.excluded_dirs, [".git", "addons"]);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_gdformatrc_sets_the_width_and_the_indent() {
        let (config, notes) = format("line_length: 120\nuse_spaces: 4\nsafety_checks: false\n");
        assert_eq!(config.format.line_length, 120);
        // The linter has to agree, or it reports what the formatter produced.
        assert_eq!(config.lint.max_line_length, 120);
        assert_eq!(config.format.indent, IndentStyle::Spaces(4));
        assert!(!config.format.safety_checks);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn the_nulls_gdtoolkit_writes_for_a_default_are_left_alone() {
        let (config, notes) = format("use_spaces: null\nsafety_checks: null\nline_length: 100\n");
        assert_eq!(config, Config::default());
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_rule_switched_on_by_naming_it_needs_no_comment() {
        // `gdlint --dump-default-config` writes every rule as `name: null`.
        let (_, notes) = lint("duplicated-load: null\nunnecessary-pass: null\n");
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_default_naming_pattern_passes_without_comment() {
        // Every pattern in a dumped default config is a default, so a project
        // that has not touched them hears nothing.
        // Quoted the way PyYAML writes a scalar that opens with `[`.
        let (_, notes) = lint(&format!(
            "class-name: {PASCAL_CASE}\nsignal-name: '{SNAKE_CASE}'\n"
        ));
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_customised_naming_pattern_is_reported_as_not_applied() {
        let (_, notes) = lint("function-name: '_on_.*'\n");
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("function-name"), "{notes:?}");
        assert!(notes[0].contains("not applied"), "{notes:?}");
    }

    #[test]
    fn a_setting_with_no_equivalent_is_reported_rather_than_dropped() {
        let (_, notes) = lint("max-locals: 15\n");
        assert_eq!(notes.len(), 1);
        assert!(
            notes[0].contains("no setting matching `max-locals`"),
            "{notes:?}"
        );
    }

    #[test]
    fn a_reordered_definitions_order_is_reported() {
        let (_, notes) = lint("class-definitions-order:\n  - enums\n  - signals\n");
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("style guide"), "{notes:?}");
        // The default order is the one gdck implements, so it says nothing.
        let default = written_order();
        let (_, notes) = lint(&format!("class-definitions-order:\n{default}"));
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let (config, notes) = lint("# a note\n\nmax-returns: 3  # inline\n");
        assert_eq!(config.lint.max_returns, 3);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_file_that_is_not_a_mapping_is_refused() {
        // Nothing in one of these files is ignorable when the whole document
        // cannot be read: none of its settings would apply, and a project
        // would be formatted by rules it had written down and rejected.
        assert!(
            refused("this is not a mapping\n")
                .message
                .contains("expected a mapping")
        );
        assert!(refused("- a\n- b\n").message.contains("expected a mapping"));
    }

    #[test]
    fn a_file_that_is_not_yaml_is_refused_at_the_line_it_broke_on() {
        let problem = refused("max-returns: 3\ndisable: [unclosed\n");
        assert_eq!(problem.line, 3);
        assert!(!problem.message.is_empty());
    }

    #[test]
    fn an_empty_file_is_read_as_saying_nothing() {
        let (config, notes) = lint("");
        assert_eq!(config, Config::default());
        assert!(notes.is_empty());
        assert!(lint("# only a comment\n").1.is_empty());
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_reported() {
        let (config, notes) = lint("max-returns: lots\n");
        assert_eq!(config.lint.max_returns, 6, "the default should survive");
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("positive number"), "{notes:?}");
    }

    #[test]
    fn the_note_carries_the_line_it_is_about() {
        let mut config = Config::default();
        let notes =
            apply_gdlintrc("max-returns: 3\n\nmax-locals: 15\n", &mut config).expect("should read");
        assert_eq!(notes[0].line, 3);
    }
}
