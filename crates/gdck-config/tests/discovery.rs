//! Finding a project's configuration on a real filesystem.
//!
//! The readers are unit-tested against strings; what is left is the part that
//! only a directory tree can answer — which file is found from where, and which
//! one wins when there is more than one.

use std::path::{Path, PathBuf};

use gdck_config::{CodeOrderFix, DeclarationGroup, IndentStyle};

/// A throwaway directory tree, removed when the test ends.
///
/// Named rather than random, so that a failure can be looked at where it
/// happened, and carrying the `write`/`dir` helpers every test here wants.
/// `tempfile` would do the removal part; the rest is the useful part.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("gdck-{}-{name}", std::process::id()));
        // A previous run that was killed before its cleanup would otherwise
        // leave files that change what this one finds.
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("should create the sandbox");
        // Symlinked temporary directories are the norm on macOS, and an
        // unresolved one makes the paths in assertions disagree with the ones
        // discovery reports.
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        Self { root }
    }

    /// Write a file, creating the directories above it.
    fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("should create the parent");
        }
        std::fs::write(&path, text).expect("should write the file");
        path
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(&path).expect("should create the directory");
        path
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn resolve(start: &Path) -> gdck_config::Loaded {
    gdck_config::resolve(start).expect("should resolve")
}

#[test]
fn nothing_found_leaves_the_style_guides_defaults_in_force() {
    let sandbox = Sandbox::new("bare");
    let loaded = resolve(&sandbox.root);
    assert_eq!(loaded.config, gdck_config::Config::default());
    assert!(loaded.files.is_empty());
    assert!(loaded.notes.is_empty());
}

#[test]
fn a_config_is_found_from_a_directory_below_it() {
    let sandbox = Sandbox::new("below");
    let path = sandbox.write("gdck.toml", "[format]\nline-length = 120\n");
    let deep = sandbox.dir("scenes/player/states");

    let loaded = resolve(&deep);
    assert_eq!(loaded.config.format.line_length, 120);
    assert_eq!(loaded.files, [path]);
}

#[test]
fn the_nearest_config_is_the_one_that_applies() {
    let sandbox = Sandbox::new("nearest");
    sandbox.write("gdck.toml", "[format]\nline-length = 120\n");
    let inner = sandbox.write("addon/gdck.toml", "[format]\nline-length = 80\n");

    let loaded = resolve(&sandbox.root.join("addon"));
    assert_eq!(loaded.config.format.line_length, 80);
    assert_eq!(loaded.files, [inner]);
}

#[test]
fn a_dot_prefixed_name_is_found_too() {
    let sandbox = Sandbox::new("dotted");
    let path = sandbox.write(".gdck.toml", "[lint]\nmax-returns = 2\n");
    assert_eq!(resolve(&sandbox.root).files, [path]);
}

#[test]
fn the_gdtoolkit_files_are_read_when_there_is_no_gdck_toml() {
    let sandbox = Sandbox::new("gdtoolkit");
    let gdformatrc = sandbox.write("gdformatrc", "line_length: 120\nuse_spaces: 4\n");
    let gdlintrc = sandbox.write(
        "gdlintrc",
        "max-returns: 3\ndisable:\n  - max-public-methods\n",
    );

    let loaded = resolve(&sandbox.root);
    assert_eq!(loaded.config.format.line_length, 120);
    assert_eq!(loaded.config.format.indent, IndentStyle::Spaces(4));
    assert_eq!(loaded.config.lint.max_returns, 3);
    assert_eq!(loaded.config.lint.disabled, ["max-public-methods"]);
    // Both, in the order they were applied.
    assert_eq!(loaded.files, [gdformatrc, gdlintrc]);
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
}

#[test]
fn a_gdck_toml_wins_outright_over_the_gdtoolkit_files() {
    // Not merged: a project that has written a gdck.toml has said what it
    // wants, and mixing in a gdlintrc would make the result unpredictable.
    let sandbox = Sandbox::new("wins");
    let path = sandbox.write("gdck.toml", "[lint]\nmax-returns = 2\n");
    sandbox.write("gdlintrc", "max-returns: 9\nmax-file-lines: 50\n");

    let loaded = resolve(&sandbox.root);
    assert_eq!(loaded.config.lint.max_returns, 2);
    assert_eq!(loaded.config.lint.max_file_lines, 1000, "the default");
    assert_eq!(loaded.files, [path]);
}

#[test]
fn a_gdck_toml_further_up_still_wins() {
    let sandbox = Sandbox::new("wins-up");
    let path = sandbox.write("gdck.toml", "[lint]\nmax-returns = 2\n");
    sandbox.write("game/gdlintrc", "max-returns: 9\n");

    let loaded = resolve(&sandbox.root.join("game"));
    assert_eq!(loaded.config.lint.max_returns, 2);
    assert_eq!(loaded.files, [path]);
}

#[test]
fn a_setting_gdck_cannot_honour_comes_back_as_a_note() {
    let sandbox = Sandbox::new("notes");
    let path = sandbox.write("gdlintrc", "max-locals: 15\n");

    let loaded = resolve(&sandbox.root);
    assert_eq!(loaded.notes.len(), 1);
    assert_eq!(loaded.notes[0].path, path);
    assert_eq!(loaded.notes[0].line, 1);
    assert!(loaded.notes[0].message.contains("max-locals"));
}

#[test]
fn a_broken_config_stops_the_run_and_says_where() {
    let sandbox = Sandbox::new("broken");
    let path = sandbox.write("gdck.toml", "[format]\n\nline-lenght = 100\n");

    let error = gdck_config::resolve(&sandbox.root).expect_err("should not resolve");
    assert_eq!(error.path, path);
    assert_eq!(error.line, Some(3));
    assert!(error.to_string().ends_with(
        "gdck.toml:3: unknown setting `line-lenght`; did you mean `format.line-length`?"
    ));
}

#[test]
fn an_explicit_file_is_read_by_the_kind_its_name_says() {
    let sandbox = Sandbox::new("explicit");
    let gdlintrc = sandbox.write("elsewhere/gdlintrc", "max-returns: 4\n");
    let toml = sandbox.write("elsewhere/settings.toml", "[lint]\nmax-returns = 5\n");

    assert_eq!(
        gdck_config::load(&gdlintrc)
            .expect("should load")
            .config
            .lint
            .max_returns,
        4
    );
    // A name matching nothing known is read as a gdck.toml, which is what
    // `--config ci-settings.toml` means.
    assert_eq!(
        gdck_config::load(&toml)
            .expect("should load")
            .config
            .lint
            .max_returns,
        5
    );
}

#[test]
fn a_file_that_is_not_there_is_an_error_rather_than_a_shrug() {
    let sandbox = Sandbox::new("missing");
    let error = gdck_config::load(&sandbox.root.join("nope.toml")).expect_err("should not load");
    assert_eq!(error.line, None);
    assert!(!error.message.is_empty());
}

#[test]
fn everything_a_project_can_set_survives_the_round_trip_to_disk() {
    let sandbox = Sandbox::new("round-trip");
    sandbox.write(
        "gdck.toml",
        "# A project that has made up its mind.\n\
         [format]\n\
         line-length = 88\n\
         indent = \"spaces\"\n\
         indent-width = 2\n\
         safety-checks = false\n\
         \n\
         [lint]\n\
         max-file-lines = 400\n\
         code-order = \"fix-when-safe\"\n\
         disable = [\"max-returns\"]\n\
         \n\
         [files]\n\
         exclude = [\".git\", \"vendor\"]\n",
    );

    let config = resolve(&sandbox.root).config;
    assert_eq!(config.format.line_length, 88);
    assert_eq!(config.format.indent, IndentStyle::Spaces(2));
    assert!(!config.format.safety_checks);
    assert_eq!(config.lint.max_line_length, 88, "the linter follows suit");
    assert_eq!(config.lint.max_file_lines, 400);
    assert_eq!(config.lint.code_order, CodeOrderFix::WholeFileWhenSafe);
    assert_eq!(config.lint.disabled, ["max-returns"]);
    assert!(config.is_excluded_dir("vendor"));
    assert!(
        !config.is_excluded_dir("addons"),
        "the list replaces, not adds"
    );

    // And writing it back out and reading it again is the same configuration.
    sandbox.write("again/gdck.toml", &config.to_toml());
    let again = resolve(&sandbox.root.join("again")).config;

    // Except for the order, which this project never set. Writing a config out
    // resolves "unset" to what unset means — the guide's order — so that the
    // file answers the question instead of leaving the reader to infer it.
    // Reading it back therefore gives the value rather than the absence, and
    // the two configurations check the same code.
    assert_eq!(
        again.lint.declaration_order,
        Some(DeclarationGroup::GUIDE_ORDER.to_vec())
    );
    let mut normalised = again.clone();
    normalised.lint.declaration_order = None;
    assert_eq!(normalised, config);
}
