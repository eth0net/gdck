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

use crate::{Config, IndentStyle, Problem};

/// Apply a `gdlintrc` to a configuration, returning what could not be honoured.
pub(crate) fn apply_gdlintrc(text: &str, config: &mut Config) -> Vec<Problem> {
    let mut notes = Vec::new();
    for item in read(text, &mut notes) {
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
    notes
}

/// Apply a `gdformatrc` to a configuration, returning what could not be honoured.
pub(crate) fn apply_gdformatrc(text: &str, config: &mut Config) -> Vec<Problem> {
    let mut notes = Vec::new();
    for item in read(text, &mut notes) {
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
    notes
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

// -- the YAML subset ---------------------------------------------------------

/// One top-level `key:` and whatever followed it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    key: String,
    value: Node,
    line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Null,
    Bool(bool),
    Integer(i64),
    Text(String),
    List(Vec<String>),
}

impl Item {
    fn is_null(&self) -> bool {
        self.value == Node::Null
    }

    fn integer(&self) -> Option<i64> {
        match self.value {
            Node::Integer(value) => Some(value),
            _ => None,
        }
    }

    fn boolean(&self) -> Option<bool> {
        match self.value {
            Node::Bool(value) => Some(value),
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match &self.value {
            Node::Text(value) => Some(value),
            _ => None,
        }
    }

    /// The value as a list of strings. A list that is empty in the file still
    /// counts, so `disable: []` clears the list rather than being ignored.
    fn strings(&self) -> Option<Vec<String>> {
        match &self.value {
            Node::List(items) => Some(items.clone()),
            _ => None,
        }
    }
}

/// Read the top-level mapping, noting any line that cannot be placed.
fn read(text: &str, notes: &mut Vec<Problem>) -> Vec<Item> {
    let lines: Vec<&str> = text.lines().collect();
    let mut items = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let number = index as u32 + 1;
        index += 1;

        let trimmed = strip_comment(line);
        if trimmed.trim().is_empty() {
            continue;
        }
        if trimmed.starts_with([' ', '\t']) {
            // Indented content belongs to the key above, which consumed it.
            // Reaching one here means the file opens with an indented line or
            // uses a shape the block reader did not take.
            notes.push(Problem {
                line: number,
                message: "not understood; it is ignored".to_string(),
            });
            continue;
        }

        let Some((key, rest)) = split_key(trimmed) else {
            notes.push(Problem {
                line: number,
                message: "not a `key: value` line; it is ignored".to_string(),
            });
            continue;
        };

        // A block belongs to this key when it is indented under it, whether or
        // not a tag like `!!set` came first.
        let rest = rest.trim();
        let tagged = rest.starts_with("!!");
        let value = if rest.is_empty() || tagged {
            let block = take_block(&lines, &mut index);
            let Some(value) = block_value(&block) else {
                notes.push(Problem {
                    line: number,
                    message: format!("the value of `{key}` is not understood; it is ignored"),
                });
                continue;
            };
            value
        } else {
            scalar_or_flow(rest)
        };

        items.push(Item {
            key,
            value,
            line: number,
        });
    }
    items
}

/// The indented lines following a key, consumed from the line list.
fn take_block<'a>(lines: &[&'a str], index: &mut usize) -> Vec<&'a str> {
    let mut block = Vec::new();
    while *index < lines.len() {
        let line = strip_comment(lines[*index]);
        if line.trim().is_empty() {
            *index += 1;
            continue;
        }
        if !line.starts_with([' ', '\t']) {
            break;
        }
        block.push(line);
        *index += 1;
    }
    block
}

/// A block of indented lines as a value.
///
/// Two shapes carry a list: a sequence, and the mapping `yaml.dump` writes for
/// a set. Anything else is left for the caller to note.
fn block_value(block: &[&str]) -> Option<Node> {
    if block.is_empty() {
        return Some(Node::Null);
    }
    if block.iter().all(|line| line.trim_start().starts_with("- ")) {
        return Some(Node::List(
            block
                .iter()
                .map(|line| unquote(line.trim_start().trim_start_matches("- ").trim()))
                .collect(),
        ));
    }
    // `excluded_directories: !!set` followed by `  .git: null`, where the keys
    // are the members and the values are all null.
    let mut members = Vec::new();
    for line in block {
        let (key, rest) = split_key(line.trim())?;
        if !matches!(scalar(rest.trim()), Node::Null) {
            return None;
        }
        members.push(key);
    }
    Some(Node::List(members))
}

fn scalar_or_flow(text: &str) -> Node {
    if let Some(inner) = text.strip_prefix('[').and_then(|to| to.strip_suffix(']')) {
        return flow_list(inner);
    }
    if let Some(inner) = text.strip_prefix('{').and_then(|to| to.strip_suffix('}')) {
        // A flow set, `{a, b}`, which is how a short `excluded_directories`
        // gets written by hand.
        return flow_list(inner);
    }
    scalar(text)
}

fn flow_list(inner: &str) -> Node {
    if inner.trim().is_empty() {
        return Node::List(Vec::new());
    }
    Node::List(
        inner
            .split(',')
            .map(|item| unquote(item.trim()))
            .filter(|item| !item.is_empty())
            .collect(),
    )
}

fn scalar(text: &str) -> Node {
    match text {
        "" | "null" | "Null" | "NULL" | "~" => Node::Null,
        "true" | "True" | "TRUE" | "yes" | "on" => Node::Bool(true),
        "false" | "False" | "FALSE" | "no" | "off" => Node::Bool(false),
        _ => match text.parse::<i64>() {
            Ok(number) => Node::Integer(number),
            Err(_) => Node::Text(unquote(text)),
        },
    }
}

/// Split `key: value`, respecting quotes around the key.
fn split_key(line: &str) -> Option<(String, &str)> {
    let line = line.trim();
    if let Some(quote) = line
        .chars()
        .next()
        .filter(|char| *char == '"' || *char == '\'')
    {
        let end = line[1..].find(quote)? + 1;
        let rest = line[end + 1..].strip_prefix(':')?;
        return Some((line[1..end].to_string(), rest));
    }
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    if key.is_empty() {
        return None;
    }
    Some((key.to_string(), &line[colon + 1..]))
}

/// Drop a trailing comment, which YAML requires be preceded by a space.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (offset, char) in line.char_indices() {
        match (quote, char) {
            (None, '"' | '\'') => quote = Some(char),
            (Some(open), char) if char == open => quote = None,
            (None, '#') if offset == 0 || line[..offset].ends_with([' ', '\t']) => {
                return &line[..offset];
            }
            _ => {}
        }
    }
    line
}

fn unquote(text: &str) -> String {
    let text = text.trim();
    for quote in ['"', '\''] {
        if text.len() >= 2 && text.starts_with(quote) && text.ends_with(quote) {
            return text[1..text.len() - 1].to_string();
        }
    }
    text.to_string()
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
        let notes = apply_gdlintrc(text, &mut config);
        (config, notes.into_iter().map(|note| note.message).collect())
    }

    fn format(text: &str) -> (Config, Vec<String>) {
        let mut config = Config::default();
        let notes = apply_gdformatrc(text, &mut config);
        (config, notes.into_iter().map(|note| note.message).collect())
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
        let (_, notes) = lint(&format!(
            "class-name: {PASCAL_CASE}\nsignal-name: {SNAKE_CASE}\n"
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
    fn a_line_that_cannot_be_placed_is_reported_rather_than_skipped() {
        let (_, notes) = lint("this is not yaml at all\n");
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("not a `key: value` line"), "{notes:?}");
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
        let notes = apply_gdlintrc("max-returns: 3\n\nmax-locals: 15\n", &mut config);
        assert_eq!(notes[0].line, 3);
    }

    #[test]
    fn gdlints_whole_default_config_is_read_without_a_word() {
        // The file `gdlint --dump-default-config` writes. Every value in it is
        // a default, so nothing in it is an override and nothing needs saying.
        let text = format!(
            "class-definitions-order:\n{}\
             class-load-variable-name: ({PASCAL_CASE}|{PRIVATE_SNAKE_CASE})\n\
             class-name: {PASCAL_CASE}\n\
             class-variable-name: {PRIVATE_SNAKE_CASE}\n\
             comparison-with-itself: null\n\
             constant-name: {PRIVATE_UPPER_SNAKE_CASE}\n\
             disable: []\n\
             duplicated-load: null\n\
             enum-element-name: {UPPER_SNAKE_CASE}\n\
             enum-name: {PASCAL_CASE}\n\
             excluded_directories: !!set\n  .git: null\n\
             expression-not-assigned: null\n\
             function-argument-name: {PRIVATE_SNAKE_CASE}\n\
             function-arguments-number: 10\n\
             function-name: (_on_{PASCAL_CASE}(_[a-z0-9]+)*|{PRIVATE_SNAKE_CASE})\n\
             function-preload-variable-name: {PASCAL_CASE}\n\
             function-variable-name: {SNAKE_CASE}\n\
             load-constant-name: ({PASCAL_CASE}|{PRIVATE_UPPER_SNAKE_CASE})\n\
             loop-variable-name: {PRIVATE_SNAKE_CASE}\n\
             max-file-lines: 1000\n\
             max-line-length: 100\n\
             max-public-methods: 20\n\
             max-returns: 6\n\
             mixed-tabs-and-spaces: null\n\
             no-elif-return: null\n\
             no-else-return: null\n\
             signal-name: {SNAKE_CASE}\n\
             sub-class-name: _?{PASCAL_CASE}\n\
             tab-characters: 1\n\
             trailing-whitespace: null\n\
             unnecessary-pass: null\n\
             unused-argument: null\n",
            written_order(),
        );
        let (config, notes) = lint(&text);
        assert!(notes.is_empty(), "{notes:?}");
        // And it is gdtoolkit's defaults, which are gdck's too.
        assert_eq!(config.lint.max_returns, 6);
        assert_eq!(config.lint.max_line_length, 100);
        assert_eq!(config.excluded_dirs, [".git"]);
    }

    #[test]
    fn gdformats_whole_default_config_is_read_without_a_word() {
        let (config, notes) = format(
            "excluded_directories: !!set\n  .git: null\n\
             line_length: 100\n\
             safety_checks: null\n\
             use_spaces: null\n",
        );
        assert_eq!(config.format.line_length, 100);
        assert_eq!(config.format.indent, IndentStyle::Tabs);
        assert!(config.format.safety_checks);
        assert_eq!(config.excluded_dirs, [".git"]);
        assert!(notes.is_empty(), "{notes:?}");
    }
}
