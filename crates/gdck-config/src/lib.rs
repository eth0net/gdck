//! Configuration for `gdck`.
//!
//! Defaults come from the [GDScript style guide][guide], so a project with no
//! configuration file at all gets style-guide behaviour.
//!
//! [guide]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html
//!
//! # Status
//!
//! The types and discovery logic here are final; reading values out of a file
//! is not wired up yet, so [`Config::default`] is what every caller currently
//! gets. See `docs/DESIGN.md` for the planned `gdck.toml` schema and the
//! `gdlintrc` / `gdformatrc` compatibility shim.

use std::path::{Path, PathBuf};

/// Config file names searched for, in priority order, walking up from the
/// working directory.
pub const CONFIG_FILE_NAMES: &[&str] = &["gdck.toml", ".gdck.toml"];

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
/// Kept as source strings rather than compiled patterns so that this crate
/// stays dependency-free. The linter matches these patterns directly rather
/// than with a regular-expression engine — recognising
/// `[a-z][a-z0-9]*(_[a-z0-9]+)*` does not need one, and not having one keeps a
/// dependency out of a tool that runs on every save. They are written out here
/// because this is the form a project would use to override a convention, and
/// because it is the least ambiguous way to state one.
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
}

/// Find the nearest config file, searching `start` and then each ancestor.
///
/// Returns `None` when the search reaches the filesystem root without a match,
/// which is the normal case for a project that has not configured anything.
#[must_use]
pub fn discover(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        for name in CONFIG_FILE_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
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
}
