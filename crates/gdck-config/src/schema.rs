//! What `gdck.toml` may say, and what each setting means.
//!
//! The file is read by the [`toml`] crate. What is left here is the part a
//! deserialiser cannot know: which values are in range, which settings only
//! mean something alongside another, and which unknown key the author probably
//! meant.
//!
//! Every key is listed in [`KEYS`], and one that is not is an error rather than
//! something quietly ignored. A misspelled key that does nothing is the classic
//! way to spend an afternoon wondering why a setting had no effect, and it
//! costs nothing to refuse it — the message names the nearest key that exists.
//!
//! The lint thresholds are named after the rules they configure, so
//! `lint.max-returns` is the limit `max-returns` reports on. `gdtoolkit`'s own
//! spellings live in [`crate::compat`], where they belong.

use serde::Deserialize;
use toml::Spanned;

use crate::{CodeOrderFix, Config, IndentStyle, Problem};

/// Every key `gdck.toml` accepts, in the order [`to_toml`] writes them.
///
/// The structs below are the authority on what is *read*; this list exists so
/// that an unknown key can be answered with the nearest one that is not. A test
/// keeps the two in step.
pub(crate) const KEYS: &[&str] = &[
    "format.line-length",
    "format.indent",
    "format.indent-width",
    "format.safety-checks",
    "lint.max-line-length",
    "lint.max-file-lines",
    "lint.max-public-methods",
    "lint.max-returns",
    "lint.max-arguments",
    "lint.code-order",
    "lint.disable",
    "files.exclude",
];

/// The shape of the file.
///
/// Every setting is optional, so that leaving one out is distinguishable from
/// writing its default — which matters for `line-length`, where one key moves
/// another that was not named.
///
/// Values are [`Spanned`] so that a setting which parsed but will not do can
/// still be reported at the line it is on.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct File {
    #[serde(default)]
    format: FormatTable,
    #[serde(default)]
    lint: LintTable,
    #[serde(default)]
    files: FilesTable,
}

// `_` and `-` are the same separator here. Nothing in the schema needs to tell
// them apart, and a reader who guesses wrong is right anyway.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FormatTable {
    #[serde(alias = "line_length")]
    line_length: Option<Spanned<u16>>,
    indent: Option<Spanned<Indent>>,
    #[serde(alias = "indent_width")]
    indent_width: Option<Spanned<u8>>,
    #[serde(alias = "safety_checks")]
    safety_checks: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct LintTable {
    #[serde(alias = "max_line_length")]
    max_line_length: Option<Spanned<u16>>,
    #[serde(alias = "max_file_lines")]
    max_file_lines: Option<u32>,
    #[serde(alias = "max_public_methods")]
    max_public_methods: Option<u32>,
    #[serde(alias = "max_returns")]
    max_returns: Option<u32>,
    #[serde(alias = "max_arguments")]
    max_arguments: Option<u32>,
    #[serde(alias = "code_order")]
    code_order: Option<CodeOrder>,
    disable: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FilesTable {
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Indent {
    Tabs,
    Spaces,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CodeOrder {
    Report,
    FixWhenSafe,
    Off,
}

impl From<CodeOrder> for CodeOrderFix {
    fn from(order: CodeOrder) -> Self {
        match order {
            CodeOrder::Report => Self::ReportOnly,
            CodeOrder::FixWhenSafe => Self::WholeFileWhenSafe,
            CodeOrder::Off => Self::Off,
        }
    }
}

/// Used when `indent = "spaces"` says nothing about how many.
const DEFAULT_INDENT_WIDTH: u8 = 4;

/// Read a whole `gdck.toml` into a [`Config`].
pub(crate) fn read(text: &str) -> Result<Config, Problem> {
    let file: File = toml::from_str(text).map_err(|error| translate(text, &error))?;
    let mut config = Config::default();

    if let Some(checks) = file.format.safety_checks {
        config.format.safety_checks = checks;
    }
    if let Some(lines) = file.lint.max_file_lines {
        config.lint.max_file_lines = lines;
    }
    if let Some(methods) = file.lint.max_public_methods {
        config.lint.max_public_methods = methods;
    }
    if let Some(returns) = file.lint.max_returns {
        config.lint.max_returns = returns;
    }
    if let Some(arguments) = file.lint.max_arguments {
        config.lint.max_function_arguments = arguments;
    }
    if let Some(order) = file.lint.code_order {
        config.lint.code_order = order.into();
    }
    if let Some(disable) = file.lint.disable {
        config.lint.disabled = disable;
    }
    if let Some(exclude) = file.files.exclude {
        config.excluded_dirs = exclude;
    }

    config.format.indent = indent_of(text, &file.format)?;

    // A project that widens its lines means both tools. Leaving the linter at
    // 100 would have it report exactly the lines the formatter just produced.
    if let Some(length) = &file.format.line_length {
        config.format.line_length = positive(text, length, "format.line-length")?;
        config.lint.max_line_length = config.format.line_length;
    }
    if let Some(length) = &file.lint.max_line_length {
        config.lint.max_line_length = positive(text, length, "lint.max-line-length")?;
    }

    Ok(config)
}

fn indent_of(text: &str, format: &FormatTable) -> Result<IndentStyle, Problem> {
    let width = match &format.indent_width {
        Some(width) => {
            let value = *width.get_ref();
            if !(1..=16).contains(&value) {
                return Err(Problem {
                    line: line_of(text, width.span().start),
                    message: "`format.indent-width` must be between 1 and 16".to_string(),
                });
            }
            Some(value)
        }
        None => None,
    };

    match (
        format.indent.as_ref().map(Spanned::get_ref),
        &format.indent_width,
    ) {
        (Some(Indent::Spaces), _) => Ok(IndentStyle::Spaces(width.unwrap_or(DEFAULT_INDENT_WIDTH))),
        // Indenting with tabs has no width to set: the formatter measures a tab
        // as four columns because that is what the style guide's samples line
        // up as, and moving it would put the formatter and the linter into
        // disagreement about which lines are too long.
        (_, Some(spanned)) => Err(Problem {
            line: line_of(text, spanned.span().start),
            message: "`format.indent-width` applies only when `format.indent` is \"spaces\""
                .to_string(),
        }),
        _ => Ok(IndentStyle::Tabs),
    }
}

/// A width has to be a width. `u16` already caps the top end.
fn positive(text: &str, value: &Spanned<u16>, key: &str) -> Result<u16, Problem> {
    let width = *value.get_ref();
    if width == 0 {
        return Err(Problem {
            line: line_of(text, value.span().start),
            message: format!("`{key}` must be between 1 and {}", u16::MAX),
        });
    }
    Ok(width)
}

// -- error messages ----------------------------------------------------------

/// Turn one of `toml`'s errors into a line and a sentence.
///
/// Its own message is kept, since it is a good one, except where it says a key
/// is unknown. There the list of every field that would have been accepted is
/// less use than the single one the author probably meant.
fn translate(text: &str, error: &toml::de::Error) -> Problem {
    let line = error.span().map_or(1, |span| line_of(text, span.start));
    let message = error.message();

    if let Some(name) = unknown_field(message) {
        return Problem {
            line,
            message: match nearest(name) {
                Some(nearest) => format!("unknown setting `{name}`; did you mean `{nearest}`?"),
                None => format!("unknown setting `{name}`"),
            },
        };
    }
    Problem {
        line,
        message: message.to_string(),
    }
}

/// The field name out of ``unknown field `x`, expected ...``.
///
/// Reading another crate's message is a thing to do carefully, so a shape that
/// does not match leaves the message alone rather than mangling it.
fn unknown_field(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("unknown field `")?;
    let end = rest.find('`')?;
    Some(&rest[..end])
}

fn line_of(text: &str, offset: usize) -> u32 {
    let offset = offset.min(text.len());
    u32::try_from(text[..offset].bytes().filter(|byte| *byte == b'\n').count() + 1)
        .unwrap_or(u32::MAX)
}

/// The key the author most likely meant, if there is an obvious candidate.
///
/// Two things go wrong often enough to be worth catching: a typo, and putting a
/// setting under the wrong table. The second is the more common of the two,
/// since which table a threshold belongs to is a thing you have to remember —
/// and `toml` reports only the bare name, so matching on the last segment
/// catches it without needing to know which table it was found in.
fn nearest(name: &str) -> Option<&'static str> {
    let name = name.replace('_', "-");
    if let Some(moved) = KEYS.iter().find(|key| last_segment(key) == name) {
        return Some(moved);
    }
    KEYS.iter()
        .map(|key| (distance(&name, last_segment(key)), *key))
        // Beyond a third of the name's length the "suggestion" is a guess, and
        // a wrong one sends the reader looking in the wrong place entirely.
        .filter(|(distance, _)| *distance * 3 <= name.len().max(3))
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, key)| key)
}

fn last_segment(key: &str) -> &str {
    key.rsplit('.').next().unwrap_or(key)
}

/// Levenshtein distance, over the two rows the algorithm actually needs.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, from) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, to) in b.iter().enumerate() {
            let substitute = previous[j] + usize::from(from != to);
            current[j + 1] = substitute.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

// -- writing it back out -----------------------------------------------------

/// Write a configuration back out as a `gdck.toml`.
///
/// Every value is written, including the ones left at their default, because
/// the point of `gdck config` is to answer what a run is actually using. Done
/// by hand rather than by serialising [`File`], because that struct is built
/// around what was *left out* — which is the one thing this must not do.
pub(crate) fn to_toml(config: &Config) -> String {
    use std::fmt::Write;

    let format = &config.format;
    let lint = &config.lint;
    let mut out = String::new();

    // Writing to a String cannot fail, so the results go nowhere.
    let _ = writeln!(out, "[format]");
    let _ = writeln!(out, "line-length = {}", format.line_length);
    match format.indent {
        IndentStyle::Tabs => {
            let _ = writeln!(out, "indent = \"tabs\"");
        }
        IndentStyle::Spaces(width) => {
            let _ = writeln!(out, "indent = \"spaces\"");
            let _ = writeln!(out, "indent-width = {width}");
        }
    }
    let _ = writeln!(out, "safety-checks = {}", format.safety_checks);

    let _ = writeln!(out, "\n[lint]");
    let _ = writeln!(out, "max-line-length = {}", lint.max_line_length);
    let _ = writeln!(out, "max-file-lines = {}", lint.max_file_lines);
    let _ = writeln!(out, "max-public-methods = {}", lint.max_public_methods);
    let _ = writeln!(out, "max-returns = {}", lint.max_returns);
    let _ = writeln!(out, "max-arguments = {}", lint.max_function_arguments);
    let order = match lint.code_order {
        CodeOrderFix::ReportOnly => "report",
        CodeOrderFix::WholeFileWhenSafe => "fix-when-safe",
        CodeOrderFix::Off => "off",
    };
    let _ = writeln!(out, "code-order = {}", quoted(order));
    let _ = writeln!(out, "disable = {}", array(&lint.disabled));

    let _ = writeln!(out, "\n[files]");
    let _ = writeln!(out, "exclude = {}", array(&effective_exclusions(config)));
    out
}

/// What the walker will actually skip, which is the defaults when nothing was
/// configured.
fn effective_exclusions(config: &Config) -> Vec<String> {
    if config.excluded_dirs.is_empty() {
        return crate::DEFAULT_EXCLUDED_DIRS
            .iter()
            .map(|dir| (*dir).to_string())
            .collect();
    }
    config.excluded_dirs.clone()
}

fn array(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|item| quoted(item)).collect();
    format!("[{}]", items.join(", "))
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_ok(text: &str) -> Config {
        read(text).expect("should read")
    }

    fn message(text: &str) -> String {
        read(text).expect_err("should not read").message
    }

    #[test]
    fn an_empty_file_leaves_every_default_alone() {
        assert_eq!(read_ok(""), Config::default());
        assert_eq!(read_ok("# nothing to say\n"), Config::default());
    }

    #[test]
    fn every_setting_can_be_read() {
        let config = read_ok(
            "[format]\n\
             line-length = 120\n\
             indent = \"spaces\"\n\
             indent-width = 2\n\
             safety-checks = false\n\
             \n\
             [lint]\n\
             max-line-length = 110\n\
             max-file-lines = 500\n\
             max-public-methods = 12\n\
             max-returns = 3\n\
             max-arguments = 5\n\
             code-order = \"fix-when-safe\"\n\
             disable = [\"max-returns\", \"line-too-long\"]\n\
             \n\
             [files]\n\
             exclude = [\"vendor\"]\n",
        );
        assert_eq!(config.format.line_length, 120);
        assert_eq!(config.format.indent, IndentStyle::Spaces(2));
        assert!(!config.format.safety_checks);
        assert_eq!(config.lint.max_line_length, 110);
        assert_eq!(config.lint.max_file_lines, 500);
        assert_eq!(config.lint.max_public_methods, 12);
        assert_eq!(config.lint.max_returns, 3);
        assert_eq!(config.lint.max_function_arguments, 5);
        assert_eq!(config.lint.code_order, CodeOrderFix::WholeFileWhenSafe);
        assert_eq!(config.lint.disabled, ["max-returns", "line-too-long"]);
        assert_eq!(config.excluded_dirs, ["vendor"]);
    }

    #[test]
    fn underscores_and_dashes_are_the_same_separator() {
        assert_eq!(
            read_ok("[format]\nline_length = 120\n").format.line_length,
            120
        );
        assert_eq!(read_ok("[lint]\nmax_returns = 2\n").lint.max_returns, 2);
    }

    #[test]
    fn a_dotted_key_means_the_same_as_a_table() {
        assert_eq!(
            read_ok("format.line-length = 120\n").format.line_length,
            120
        );
    }

    #[test]
    fn spaces_default_to_four_of_them() {
        assert_eq!(
            read_ok("[format]\nindent = \"spaces\"\n").format.indent,
            IndentStyle::Spaces(4)
        );
    }

    #[test]
    fn tabs_have_no_width_to_set() {
        // The formatter measures a tab as four columns and the linter has to
        // agree with it, so this would not do what it looks like it does.
        let reported = message("[format]\nindent = \"tabs\"\nindent-width = 2\n");
        assert!(reported.contains("only when"), "{reported}");
        // And the order the two are written in makes no difference.
        assert!(message("[format]\nindent-width = 2\nindent = \"tabs\"\n").contains("only when"));
        // Nor does leaving `indent` out entirely, since tabs is the default.
        assert!(message("[format]\nindent-width = 2\n").contains("only when"));
    }

    #[test]
    fn widening_the_lines_widens_them_for_the_linter_too() {
        // Otherwise the linter reports exactly the lines the formatter made.
        let config = read_ok("[format]\nline-length = 120\n");
        assert_eq!(config.lint.max_line_length, 120);
        // Unless the file asks for something else, which it is allowed to.
        let config = read_ok("[format]\nline-length = 120\n\n[lint]\nmax-line-length = 100\n");
        assert_eq!(config.format.line_length, 120);
        assert_eq!(config.lint.max_line_length, 100);
    }

    #[test]
    fn an_unknown_setting_is_refused_rather_than_ignored() {
        assert!(message("[lint]\nmax-recursion = 3\n").contains("unknown setting"));
        // Including a table nobody has heard of.
        assert!(message("[linting]\nmax-returns = 3\n").contains("unknown setting"));
    }

    #[test]
    fn a_near_miss_is_named() {
        assert_eq!(
            message("[format]\nline-lenght = 100\n"),
            "unknown setting `line-lenght`; did you mean `format.line-length`?"
        );
        // The commonest mistake of all: the right key under the wrong table.
        assert_eq!(
            message("[format]\nmax-returns = 3\n"),
            "unknown setting `max-returns`; did you mean `lint.max-returns`?"
        );
    }

    #[test]
    fn a_wild_guess_is_not_offered_as_a_suggestion() {
        let reported = message("[lint]\nfavourite-colour = \"blue\"\n");
        assert_eq!(reported, "unknown setting `favourite-colour`");
    }

    #[test]
    fn the_wrong_kind_of_value_says_what_it_wanted() {
        assert!(message("[format]\nline-length = \"100\"\n").contains("expected u16"));
        assert!(message("[format]\nsafety-checks = 1\n").contains("expected a boolean"));
        assert!(message("[lint]\ndisable = \"max-returns\"\n").contains("expected a sequence"));
        assert!(message("[lint]\ndisable = [1]\n").contains("expected a string"));
    }

    #[test]
    fn a_value_outside_its_range_is_refused() {
        assert!(message("[format]\nline-length = 0\n").contains("must be between 1 and 65535"));
        assert!(message("[format]\nline-length = 70000\n").contains("expected u16"));
        assert!(message("[lint]\nmax-returns = -1\n").contains("expected u32"));
        assert!(message("[format]\nindent-width = 0\n").contains("must be between 1 and 16"));
    }

    #[test]
    fn a_choice_lists_the_choices() {
        let reported = message("[lint]\ncode-order = \"sometimes\"\n");
        assert!(
            reported.contains("unknown variant `sometimes`"),
            "{reported}"
        );
        assert!(reported.contains("fix-when-safe"), "{reported}");
    }

    #[test]
    fn a_syntax_error_is_reported_as_one() {
        assert!(read("[format\n").is_err());
        assert!(read("line-length = = 1\n").is_err());
    }

    #[test]
    fn the_line_number_is_the_settings_own() {
        // Both when the deserialiser found it...
        let problem = read("[format]\n\n# a note\nline-length = true\n").expect_err("should fail");
        assert_eq!(problem.line, 4);
        // ...and when the validation after it did.
        let problem = read("[format]\n\n\nindent-width = 2\n").expect_err("should fail");
        assert_eq!(problem.line, 4);
    }

    #[test]
    fn what_is_written_out_can_be_read_back_in() {
        // Which also proves every key in the schema has a spelling `read`
        // accepts, since `to_toml` writes all of them.
        let config = Config {
            format: crate::FormatConfig {
                line_length: 120,
                indent: IndentStyle::Spaces(2),
                safety_checks: false,
            },
            lint: crate::LintConfig {
                max_line_length: 110,
                max_file_lines: 500,
                max_public_methods: 12,
                max_returns: 3,
                max_function_arguments: 5,
                code_order: CodeOrderFix::Off,
                disabled: vec!["max-returns".to_string()],
            },
            excluded_dirs: vec!["vendor".to_string()],
        };
        assert_eq!(read_ok(&config.to_toml()), config);
    }

    #[test]
    fn the_defaults_round_trip_as_the_defaults() {
        let read_back = read_ok(&Config::default().to_toml());
        // The one difference is deliberate: an empty exclusion list means the
        // defaults, and `gdck config` writes out what they are.
        assert_eq!(read_back.format, Config::default().format);
        assert_eq!(read_back.lint, Config::default().lint);
        assert_eq!(read_back.excluded_dirs, crate::DEFAULT_EXCLUDED_DIRS);
    }

    #[test]
    fn every_key_in_the_catalogue_is_one_read_accepts() {
        for key in KEYS {
            let reported = message(&format!("{key} = \"probe\"\n"));
            assert!(
                !reported.contains("unknown setting"),
                "`{key}` is listed but not read: {reported}"
            );
        }
    }
}
