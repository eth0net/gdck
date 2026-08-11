//! Configuration for `gdck`.
//!
//! Defaults come from the [GDScript style guide][guide], so a project with no
//! configuration file at all gets style-guide behaviour.
//!
//! [guide]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html
//!
//! # Reading a project's settings
//!
//! [`resolve`] walks up from a directory and returns the settings in force
//! there, along with the files they came from:
//!
//! ```no_run
//! let loaded = gdck_config::resolve(std::path::Path::new("."))?;
//! println!("{} columns", loaded.config.format.line_length);
//! # Ok::<(), gdck_config::Error>(())
//! ```
//!
//! The nearest `gdck.toml` wins outright. Failing that, `gdtoolkit`'s own
//! `gdformatrc` and `gdlintrc` are read, so a project already using `gdformat`
//! and `gdlint` keeps its line length and its disabled rules without writing
//! anything new. See `docs/CONFIG.md` for the schema and the precedence.
//!
//! # Two file formats, two approaches
//!
//! `gdck.toml` is read by the [`toml`] crate, and validated here for the
//! things a deserialiser cannot know — ranges, and settings that only mean
//! something alongside another.
//!
//! The `gdtoolkit` files are YAML, where there is no equivalent crate to
//! reach for: `serde_yaml` is deprecated and its successors are young. They
//! also want reading in a way `serde` is not shaped for — a `gdlintrc` holds
//! dozens of settings `gdck` has no equivalent for, each of which should be
//! skipped individually with a note rather than failing the file. So
//! [`compat`] reads the handful of shapes those files actually take.

mod compat;
mod schema;

use std::fmt;
use std::path::{Path, PathBuf};

/// Config file names searched for, in priority order, walking up from the
/// working directory.
pub const CONFIG_FILE_NAMES: &[&str] = &["gdck.toml", ".gdck.toml"];

/// `gdformat`'s configuration file, read for compatibility.
pub const GDFORMAT_FILE_NAMES: &[&str] = &["gdformatrc", ".gdformatrc"];

/// `gdlint`'s configuration file, read for compatibility.
pub const GDLINT_FILE_NAMES: &[&str] = &["gdlintrc", ".gdlintrc"];

/// Directories skipped when collecting `.gd` files.
///
/// `.godot` holds the editor's generated import cache and `addons` is usually
/// third-party code a project does not want reformatted.
pub const DEFAULT_EXCLUDED_DIRS: &[&str] = &[".git", ".godot", ".import", "addons"];

/// Indentation style. The style guide mandates tabs; spaces are available
/// because some existing projects have already committed to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentStyle {
    #[default]
    Tabs,
    Spaces(u8),
}

/// Formatting options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatConfig {
    /// Hard wrap width. The style guide says keep lines under 100 characters.
    pub line_length: u16,
    pub indent: IndentStyle,
    /// Re-run the formatter on its own output and reject the result if it is
    /// not stable, and check that no comments were dropped. Cheap insurance
    /// against a formatter bug silently eating code.
    pub safety_checks: bool,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            line_length: 100,
            indent: IndentStyle::Tabs,
            safety_checks: true,
        }
    }
}

/// How aggressively `gdck fix` may reorder class members.
///
/// Reordering is not purely cosmetic: class-level initialisers run in
/// declaration order, so moving a public variable above a private one it reads
/// changes behaviour. See `docs/DESIGN.md` for the full analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodeOrderFix {
    /// Report violations; only reorder when `--fix-order` is passed.
    #[default]
    ReportOnly,
    /// Reorder a file when every required move is provably safe, and leave the
    /// file completely untouched otherwise.
    WholeFileWhenSafe,
    /// Never reorder, and do not report ordering problems either.
    Off,
}

/// Lint options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintConfig {
    pub max_line_length: u16,
    pub max_file_lines: u32,
    pub max_public_methods: u32,
    pub max_returns: u32,
    pub max_function_arguments: u32,
    pub code_order: CodeOrderFix,
    /// Rule names switched off for this project.
    pub disabled: Vec<String>,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            max_line_length: 100,
            max_file_lines: 1000,
            max_public_methods: 20,
            max_returns: 6,
            max_function_arguments: 10,
            code_order: CodeOrderFix::default(),
            disabled: Vec::new(),
        }
    }
}

/// Naming patterns from the style guide's conventions table.
///
/// Written out as source strings because that is the least ambiguous way to
/// state a convention, and because it is the form a project would use to
/// override one. The linter does not compile them: `is_snake_case` and its
/// two siblings are a dozen lines each and read better than the equivalent
/// pattern, so these stand as documentation of what those functions accept.
/// See [`crate::naming`] and `gdck_lint::names`.
pub mod naming {
    pub const PASCAL_CASE: &str = r"([A-Z][a-z0-9]*)+";
    pub const SNAKE_CASE: &str = r"[a-z][a-z0-9]*(_[a-z0-9]+)*";
    pub const PRIVATE_SNAKE_CASE: &str = r"_?[a-z][a-z0-9]*(_[a-z0-9]+)*";
    pub const CONSTANT_CASE: &str = r"[A-Z][A-Z0-9]*(_[A-Z0-9]+)*";
    pub const PRIVATE_CONSTANT_CASE: &str = r"_?[A-Z][A-Z0-9]*(_[A-Z0-9]+)*";
}

/// The full configuration for a run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub format: FormatConfig,
    pub lint: LintConfig,
    pub excluded_dirs: Vec<String>,
}

impl Config {
    /// Whether a directory name should be skipped when collecting files.
    #[must_use]
    pub fn is_excluded_dir(&self, name: &str) -> bool {
        if self.excluded_dirs.is_empty() {
            return DEFAULT_EXCLUDED_DIRS.contains(&name);
        }
        self.excluded_dirs.iter().any(|dir| dir == name)
    }

    /// Write these settings out as a `gdck.toml`.
    ///
    /// Every setting is written, including the ones left at their default,
    /// because the question this answers is what a run is actually using.
    #[must_use]
    pub fn to_toml(&self) -> String {
        schema::to_toml(self)
    }
}

// -- errors and notes -------------------------------------------------------

/// Something wrong at a line of a file, before the file is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) line: u32,
    pub(crate) message: String,
}

/// A configuration file that could not be used.
///
/// A broken configuration file is always fatal rather than a fall back to the
/// defaults. Settings that quietly do not apply are worse than a run that
/// stops and says why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub path: PathBuf,
    /// The line at fault, when the file was read but not understood.
    pub line: Option<u32>,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.path.display(), self.message),
            None => write!(f, "{}: {}", self.path.display(), self.message),
        }
    }
}

impl std::error::Error for Error {}

/// Something a configuration file asked for that `gdck` cannot honour.
///
/// Only the `gdtoolkit` files produce these. `gdck.toml` refuses what it
/// cannot do, but a foreign file is allowed to hold settings that mean nothing
/// here — and saying so is the difference between a setting that does not
/// apply and one that silently does not apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub path: PathBuf,
    pub line: u32,
    pub message: String,
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.path.display(), self.line, self.message)
    }
}

/// Settings, and where they came from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Loaded {
    pub config: Config,
    /// The files read, in the order they were applied. Empty when nothing was
    /// found and the defaults are in force.
    pub files: Vec<PathBuf>,
    pub notes: Vec<Note>,
}

// -- discovery and loading --------------------------------------------------

/// Find the nearest `gdck.toml`, searching `start` and then each ancestor.
///
/// Returns `None` when the search reaches the filesystem root without a match,
/// which is the normal case for a project that has not configured anything.
#[must_use]
pub fn discover(start: &Path) -> Option<PathBuf> {
    discover_named(start, CONFIG_FILE_NAMES)
}

/// Find the nearest file with one of `names`, searching `start` and then each
/// ancestor.
#[must_use]
pub fn discover_named(start: &Path, names: &[&str]) -> Option<PathBuf> {
    for dir in start.ancestors() {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The settings in force in a directory.
///
/// The nearest `gdck.toml` wins outright: a project that has written one has
/// said what it wants, and quietly mixing in a `gdlintrc` from three
/// directories further up would make the result impossible to predict. Only
/// when there is no `gdck.toml` are `gdformatrc` and `gdlintrc` read, so a
/// project already set up for `gdtoolkit` keeps its settings.
pub fn resolve(start: &Path) -> Result<Loaded, Error> {
    /// One of the `gdtoolkit` readers: settings in, notes out.
    type Reader = fn(&str, &mut Config) -> Vec<Problem>;

    if let Some(path) = discover(start) {
        return load(&path);
    }

    let mut loaded = Loaded::default();
    // Formatting first, so a `gdlintrc` naming its own line length has the
    // last word on what the linter reports.
    let readers: [(&[&str], Reader); 2] = [
        (GDFORMAT_FILE_NAMES, compat::apply_gdformatrc),
        (GDLINT_FILE_NAMES, compat::apply_gdlintrc),
    ];
    for (names, apply) in readers {
        let Some(path) = discover_named(start, names) else {
            continue;
        };
        let text = read_to_string(&path)?;
        let problems = apply(&text, &mut loaded.config);
        loaded.notes.extend(notes_at(&path, problems));
        loaded.files.push(path);
    }
    Ok(loaded)
}

/// Read one configuration file, whatever kind it is.
///
/// The kind is decided by the file's name, so `--config` can be pointed at a
/// `gdlintrc` as readily as at a `gdck.toml`. A name matching none of them is
/// read as a `gdck.toml`.
pub fn load(path: &Path) -> Result<Loaded, Error> {
    let text = read_to_string(path)?;
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());

    let mut loaded = Loaded {
        files: vec![path.to_path_buf()],
        ..Loaded::default()
    };
    let problems = if GDLINT_FILE_NAMES.contains(&name.as_str()) {
        compat::apply_gdlintrc(&text, &mut loaded.config)
    } else if GDFORMAT_FILE_NAMES.contains(&name.as_str()) {
        compat::apply_gdformatrc(&text, &mut loaded.config)
    } else {
        loaded.config = schema::read(&text).map_err(|problem| Error {
            path: path.to_path_buf(),
            line: Some(problem.line),
            message: problem.message,
        })?;
        Vec::new()
    };
    loaded.notes = notes_at(path, problems);
    Ok(loaded)
}

fn notes_at(path: &Path, problems: Vec<Problem>) -> Vec<Note> {
    problems
        .into_iter()
        .map(|problem| Note {
            path: path.to_path_buf(),
            line: problem.line,
            message: problem.message,
        })
        .collect()
}

fn read_to_string(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|error| Error {
        path: path.to_path_buf(),
        line: None,
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_style_guide() {
        let config = Config::default();
        assert_eq!(config.format.line_length, 100);
        assert_eq!(config.format.indent, IndentStyle::Tabs);
        assert!(config.format.safety_checks);
        // Reordering must be opt-in until the dependency analysis is proven.
        assert_eq!(config.lint.code_order, CodeOrderFix::ReportOnly);
    }

    #[test]
    fn excluded_dirs_fall_back_to_defaults() {
        let config = Config::default();
        assert!(config.is_excluded_dir(".git"));
        assert!(config.is_excluded_dir(".godot"));
        assert!(!config.is_excluded_dir("src"));
    }

    #[test]
    fn explicit_excluded_dirs_replace_the_defaults() {
        let config = Config {
            excluded_dirs: vec!["vendor".to_string()],
            ..Config::default()
        };
        assert!(config.is_excluded_dir("vendor"));
        assert!(!config.is_excluded_dir(".git"));
    }

    #[test]
    fn discover_returns_none_when_nothing_is_configured() {
        // A directory that certainly holds no gdck.toml above it is hard to
        // guarantee, so just check the call is total and does not panic.
        let _ = discover(Path::new("/"));
    }

    #[test]
    fn an_error_reads_as_a_place_and_a_reason() {
        let error = Error {
            path: PathBuf::from("gdck.toml"),
            line: Some(4),
            message: "unknown setting `foo`".to_string(),
        };
        assert_eq!(error.to_string(), "gdck.toml:4: unknown setting `foo`");
        let error = Error {
            line: None,
            ..error
        };
        assert_eq!(error.to_string(), "gdck.toml: unknown setting `foo`");
    }
}
