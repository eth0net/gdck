//! The binary, run as a user runs it.
//!
//! The unit tests beside `main.rs` check that the arguments parse. What only a
//! real process can answer is the part the README makes promises about: which
//! stream each kind of output goes to, which exit code comes back, and whether
//! a file on disk was touched. Those three are the interface — everything else
//! is an implementation detail — and until this file existed, none of them was
//! covered.

use std::path::PathBuf;

use assert_cmd::Command;

/// A file that is already formatted and already lint-clean, so that any output
/// at all from a command run over it is a bug.
const CLEAN: &str = "extends Node\n\n\nfunc _ready() -> void:\n\tprint(\"hi\")\n";

/// Formatted wrongly, but valid GDScript that no lint rule objects to. Isolates
/// the formatter from the linter.
const UNFORMATTED: &str = "extends Node\n\n\nfunc _ready() -> void:\n\tprint( \"hi\" )\n";

/// Formatted correctly, but breaks two naming rules.
const UNCLEAN: &str =
    "extends Node\n\n\nfunc DoThing() -> void:\n\tvar MyVar = 1\n\tprint(MyVar)\n";

fn gdck() -> Command {
    Command::new(env!("CARGO_BIN_EXE_gdck"))
}

/// A throwaway directory tree, removed when the test ends.
///
/// Named after the test rather than randomly, so a failure can be looked at
/// where it happened.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("gdck-cli-{}-{name}", std::process::id()));
        // A previous run killed before its cleanup would otherwise leave files
        // that change what this one finds.
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("should create the sandbox");
        Self { root }
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, text).expect("should write the file");
        path
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.root.join(name)).expect("should read the file")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

// -- the one rule the interface is built on ---------------------------------

#[test]
fn nothing_is_written_to_disk_without_fix() {
    let sandbox = Sandbox::new("no-write");
    sandbox.write("a.gd", UNFORMATTED);
    sandbox.write("b.gd", UNCLEAN);

    // Every verb that has anything to say about these files, short of `fix`.
    for args in [
        vec!["check"],
        vec!["check", "--diff"],
        vec!["format"],
        vec!["format", "--diff"],
        vec!["format", "--check"],
        vec!["lint"],
        vec!["parse"],
    ] {
        // The exit code is not the point here — several of these report
        // problems, and correctly. What matters is the disk afterwards.
        let _ = gdck()
            .args(&args)
            .arg(&sandbox.root)
            .arg("--no-config")
            .output()
            .expect("should run");
        assert_eq!(sandbox.read("a.gd"), UNFORMATTED, "{args:?} wrote a.gd");
        assert_eq!(sandbox.read("b.gd"), UNCLEAN, "{args:?} wrote b.gd");
    }
}

// -- streams ----------------------------------------------------------------

#[test]
fn summaries_go_to_standard_error_leaving_standard_output_empty() {
    // Standard output carries content or nothing. On a clean tree there is no
    // content, so every one of these has to say its piece on standard error and
    // leave the pipe untouched.
    let sandbox = Sandbox::new("streams-clean");
    sandbox.write("a.gd", CLEAN);

    for verb in ["check", "format", "lint", "parse", "fix"] {
        let output = gdck()
            .arg(verb)
            .arg(&sandbox.root)
            .arg("--no-config")
            .output()
            .expect("should run");
        assert_eq!(
            text(&output.stdout),
            "",
            "`gdck {verb}` put a summary on standard output"
        );
        assert!(
            !output.stderr.is_empty(),
            "`gdck {verb}` said nothing at all"
        );
    }
}

#[test]
fn diagnostics_go_to_standard_output_and_name_their_rule() {
    let sandbox = Sandbox::new("diagnostics");
    sandbox.write("a.gd", UNCLEAN);

    let output = gdck()
        .args(["lint", "--no-config"])
        .arg(&sandbox.root)
        .output()
        .expect("should run");

    let stdout = text(&output.stdout);
    // The rule name is on the line so that the fix can be copied straight out
    // of the report into a `disable` list or an ignore comment.
    assert!(stdout.contains("[function-name]"), "{stdout}");
    assert!(stdout.contains("[variable-name]"), "{stdout}");
    assert!(stdout.contains("a.gd:4:"), "{stdout}");
    // And the count is a summary, so it is not in the pipe.
    assert!(!stdout.contains("Found 2"), "{stdout}");
    assert!(text(&output.stderr).contains("Found 2"));
}

#[test]
fn the_list_of_files_that_would_change_is_the_output_of_a_bare_format() {
    let sandbox = Sandbox::new("would-change");
    let path = sandbox.write("a.gd", UNFORMATTED);
    sandbox.write("b.gd", CLEAN);

    let output = gdck()
        .args(["format", "--no-config"])
        .arg(&sandbox.root)
        .output()
        .expect("should run");

    // Exactly the paths that would change, one per line, so the result pipes
    // into anything that takes a file list.
    assert_eq!(text(&output.stdout), format!("{}\n", path.display()));
}

// -- exit codes -------------------------------------------------------------

#[test]
fn a_clean_tree_exits_zero() {
    let sandbox = Sandbox::new("exit-clean");
    sandbox.write("a.gd", CLEAN);
    for verb in ["check", "format", "lint", "parse"] {
        gdck()
            .arg(verb)
            .arg(&sandbox.root)
            .arg("--no-config")
            .assert()
            .success();
    }
}

#[test]
fn problems_found_exits_one() {
    let sandbox = Sandbox::new("exit-problems");
    sandbox.write("format.gd", UNFORMATTED);
    sandbox.write("lint.gd", UNCLEAN);
    sandbox.write("syntax.gd", "func (\n");

    for (verb, file) in [
        ("format", "format.gd"),
        ("lint", "lint.gd"),
        ("parse", "syntax.gd"),
        ("check", "format.gd"),
        ("check", "lint.gd"),
    ] {
        gdck()
            .arg(verb)
            .arg(sandbox.root.join(file))
            .arg("--no-config")
            .assert()
            .code(1);
    }
}

#[test]
fn a_run_that_cannot_be_completed_exits_two() {
    let sandbox = Sandbox::new("exit-error");
    sandbox.write("a.gd", CLEAN);
    // A configuration file that cannot be read is fatal. Falling back to the
    // defaults would format a project by rules it explicitly rejected.
    sandbox.write("gdck.toml", "[format]\nline-lenght = 100\n");

    let output = gdck()
        .arg("check")
        .arg(&sandbox.root)
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    // And it says which file, which line, and what was probably meant.
    let stderr = text(&output.stderr);
    assert!(stderr.contains("gdck.toml:2"), "{stderr}");
    assert!(stderr.contains("format.line-length"), "{stderr}");
}

// -- writing ----------------------------------------------------------------

#[test]
fn format_fix_writes_and_leaves_the_file_formatted() {
    let sandbox = Sandbox::new("format-fix");
    sandbox.write("a.gd", UNFORMATTED);

    gdck()
        .args(["format", "--fix", "--no-config"])
        .arg(&sandbox.root)
        .assert()
        .success();

    let after = sandbox.read("a.gd");
    assert_ne!(after, UNFORMATTED);
    assert!(after.contains("print(\"hi\")"), "{after}");
    // And a second run has nothing left to do, which is the property that
    // makes it safe to put in a pre-commit hook.
    gdck()
        .args(["format", "--no-config"])
        .arg(&sandbox.root)
        .assert()
        .success();
}

#[test]
fn fix_applies_the_lint_fixes_and_the_formatting_together() {
    let sandbox = Sandbox::new("fix");
    // Double-quoted after normalisation, and the spacing settled by the
    // formatter — the two stages have to agree on the result.
    sandbox.write(
        "a.gd",
        "extends Node\n\n\nfunc _ready() -> void:\n\tprint( 'hi' )\n",
    );

    gdck()
        .args(["fix", "--no-config"])
        .arg(&sandbox.root)
        .assert()
        .success();

    assert_eq!(sandbox.read("a.gd"), CLEAN);
}

// -- standard input ---------------------------------------------------------

#[test]
fn standard_input_makes_format_a_filter() {
    // The README's promise: standard output carries the file itself when
    // reading from `-`. There is no file to write back to, so this holds with
    // or without --fix, and whether or not anything changed.
    for args in [
        vec!["format", "-", "--no-config"],
        vec!["format", "--fix", "-", "--no-config"],
    ] {
        let output = gdck()
            .args(&args)
            .write_stdin(UNFORMATTED)
            .output()
            .expect("should run");
        assert_eq!(
            text(&output.stdout),
            CLEAN,
            "{args:?} did not emit the formatted file"
        );
    }
}

#[test]
fn an_already_clean_file_still_comes_back_out_of_the_pipe() {
    // Or `gdck format - < in.gd > out.gd` empties a file that had nothing
    // wrong with it.
    let output = gdck()
        .args(["format", "-", "--no-config"])
        .write_stdin(CLEAN)
        .output()
        .expect("should run");
    assert_eq!(text(&output.stdout), CLEAN);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn a_diff_from_standard_input_is_a_diff_and_not_the_file() {
    let output = gdck()
        .args(["format", "--diff", "-", "--no-config"])
        .write_stdin(UNFORMATTED)
        .output()
        .expect("should run");
    let stdout = text(&output.stdout);
    assert!(stdout.starts_with("--- -\n"), "{stdout}");
    assert!(stdout.contains("@@"), "{stdout}");
    assert!(stdout.contains("-\tprint( \"hi\" )"), "{stdout}");
    assert!(stdout.contains("+\tprint(\"hi\")"), "{stdout}");
}

// -- configuration ----------------------------------------------------------

#[test]
fn no_config_ignores_a_file_that_would_otherwise_be_fatal() {
    let sandbox = Sandbox::new("no-config");
    sandbox.write("a.gd", CLEAN);
    sandbox.write("gdck.toml", "[format]\nline-lenght = 100\n");

    gdck()
        .args(["check", "--no-config"])
        .arg(&sandbox.root)
        .assert()
        .success();
}

#[test]
fn a_named_config_is_read_from_wherever_it_is() {
    let sandbox = Sandbox::new("named-config");
    sandbox.write("a.gd", CLEAN);
    let config = sandbox.write("ci.toml", "[lint]\ndisable = [\"function-name\"]\n");

    let output = gdck()
        .arg("config")
        .arg("--config")
        .arg(&config)
        .output()
        .expect("should run");
    assert!(text(&output.stdout).contains("disable = [\"function-name\"]"));
    assert!(text(&output.stderr).contains("ci.toml"));
}

#[test]
fn init_carries_gdtoolkit_settings_across_without_losing_any() {
    // The whole point of the command. A shell redirect could not do this:
    // `gdck config > gdck.toml` has the shell create the file first, gdck then
    // finds an empty `gdck.toml`, which beats a `gdlintrc` outright, and writes
    // the defaults over the settings it was run to keep.
    let sandbox = Sandbox::new("init-carries");
    sandbox.write("a.gd", CLEAN);
    sandbox.write(
        ".gdlintrc",
        "max-line-length: 120\nmax-public-methods: 40\ndisable:\n  - max-returns\n",
    );

    let before = gdck()
        .arg("config")
        .arg(&sandbox.root)
        .output()
        .expect("should run");

    gdck().arg("init").arg(&sandbox.root).assert().success();

    // The settings in force must be the same ones, now that the gdck.toml is
    // what answers for them rather than the gdlintrc.
    let after = gdck()
        .arg("config")
        .arg(&sandbox.root)
        .output()
        .expect("should run");
    assert_eq!(text(&before.stdout), text(&after.stdout));

    let written = sandbox.read("gdck.toml");
    assert!(written.contains("max-line-length = 120"), "{written}");
    assert!(written.contains("max-public-methods = 40"), "{written}");
    // What the project did not choose stays commented, so the live lines are
    // its decisions and a later change to a default still reaches it.
    assert!(written.contains("# max-returns = 6"), "{written}");
    assert!(written.contains("# line-length = 100"), "{written}");
}

#[test]
fn init_refuses_to_overwrite_without_force() {
    let sandbox = Sandbox::new("init-force");
    sandbox.write("a.gd", CLEAN);
    sandbox.write("gdck.toml", "[lint]\nmax-returns = 3\n");

    let refused = gdck()
        .arg("init")
        .arg(&sandbox.root)
        .output()
        .expect("should run");
    assert_eq!(refused.status.code(), Some(2));
    assert!(text(&refused.stderr).contains("--force"));
    // Untouched, which is the part that matters.
    assert_eq!(sandbox.read("gdck.toml"), "[lint]\nmax-returns = 3\n");

    gdck()
        .args(["init", "--force"])
        .arg(&sandbox.root)
        .assert()
        .success();
    assert!(sandbox.read("gdck.toml").contains("max-returns = 3"));
}

#[test]
fn init_says_what_it_could_not_carry_across() {
    // A setting gdtoolkit has and gdck does not is named before the file is
    // written. Migrating quietly past one is how a project ends up governed by
    // rules it thought it had set.
    let sandbox = Sandbox::new("init-notes");
    sandbox.write("a.gd", CLEAN);
    sandbox.write(".gdlintrc", "max-locals: 15\n");

    let output = gdck()
        .arg("init")
        .arg(&sandbox.root)
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(
        text(&output.stderr).contains("max-locals"),
        "{:?}",
        text(&output.stderr)
    );
}

#[test]
fn init_can_start_from_the_defaults_instead() {
    let sandbox = Sandbox::new("init-defaults");
    sandbox.write("a.gd", CLEAN);
    sandbox.write(".gdlintrc", "max-line-length: 120\n");

    gdck()
        .args(["init", "--no-config"])
        .arg(&sandbox.root)
        .assert()
        .success();

    let written = sandbox.read("gdck.toml");
    assert!(written.contains("# max-line-length = 100"), "{written}");
    assert!(!written.contains("\nmax-line-length = 120"), "{written}");
}

#[test]
fn config_prints_a_gdck_toml_that_can_be_read_back() {
    // `gdck config` is the other question — what a run is using — and its
    // output still has to be a file gdck accepts. `gdck init` is the way to
    // write one; see `init_carries_gdtoolkit_settings_across_without_losing_any`.
    let sandbox = Sandbox::new("config-round-trip");
    sandbox.write("a.gd", CLEAN);

    let first = gdck()
        .args(["config", "--no-config"])
        .arg(&sandbox.root)
        .output()
        .expect("should run");
    let printed = text(&first.stdout);
    assert!(printed.contains("[format]"), "{printed}");
    assert!(printed.contains("[lint]"), "{printed}");

    sandbox.write("gdck.toml", &printed);
    let second = gdck()
        .arg("config")
        .arg(&sandbox.root)
        .output()
        .expect("should run");
    assert_eq!(second.status.code(), Some(0), "{}", text(&second.stderr));
    assert_eq!(
        text(&second.stdout),
        printed,
        "the settings changed on the way through a file"
    );
}

#[test]
fn a_gdtoolkit_setting_gdck_cannot_honour_is_a_note_and_not_a_failure() {
    let sandbox = Sandbox::new("gdtoolkit-note");
    sandbox.write("a.gd", CLEAN);
    sandbox.write("gdlintrc", "max-returns: 3\nmax-locals: 15\n");

    let output = gdck()
        .arg("check")
        .arg(&sandbox.root)
        .output()
        .expect("should run");

    // The run happens, with what could be honoured applied...
    assert_eq!(output.status.code(), Some(0));
    // ...and what could not is said out loud rather than dropped.
    let stderr = text(&output.stderr);
    assert!(stderr.contains("max-locals"), "{stderr}");
    assert!(!stderr.contains("max-returns"), "{stderr}");
}

// -- paths ------------------------------------------------------------------

#[test]
fn a_path_that_is_not_there_is_reported_rather_than_ignored() {
    let sandbox = Sandbox::new("missing-path");
    let output = gdck()
        .arg("check")
        .arg(sandbox.root.join("nope.gd"))
        .arg("--no-config")
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("nope.gd"));
}

#[test]
fn an_explicit_file_is_processed_whatever_it_is_called() {
    // Directories are filtered to `.gd`; a path given by name is taken as
    // given, so `gdck parse odd_name.txt` does what was asked.
    let sandbox = Sandbox::new("odd-name");
    let path = sandbox.write("odd_name.txt", CLEAN);
    gdck()
        .arg("parse")
        .arg(&path)
        .arg("--no-config")
        .assert()
        .success();
}

#[test]
fn excluded_directories_are_not_walked() {
    let sandbox = Sandbox::new("excluded");
    sandbox.write("a.gd", CLEAN);
    let addons = sandbox.root.join("addons");
    std::fs::create_dir_all(&addons).expect("should create");
    std::fs::write(addons.join("vendored.gd"), UNCLEAN).expect("should write");

    // `addons` is excluded by default, so the tree is clean despite what is
    // sitting in it.
    gdck()
        .args(["check", "--no-config"])
        .arg(&sandbox.root)
        .assert()
        .success();

    // Naming the file directly still works: exclusions apply to walking, not
    // to what the user asked for by name.
    gdck()
        .args(["lint", "--no-config"])
        .arg(addons.join("vendored.gd"))
        .assert()
        .code(1);
}

#[test]
fn an_empty_tree_is_success_and_says_so() {
    let sandbox = Sandbox::new("empty");
    let output = gdck()
        .args(["check", "--no-config"])
        .arg(&sandbox.root)
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stdout), "");
    assert!(text(&output.stderr).contains("No .gd files found."));
}

// -- the interface itself ---------------------------------------------------

#[test]
fn every_subcommand_has_help_and_the_binary_has_a_version() {
    for verb in ["check", "fix", "format", "lint", "parse", "config"] {
        let output = gdck().arg(verb).arg("--help").output().expect("should run");
        assert!(output.status.success(), "`gdck {verb} --help` failed");
        assert!(
            !output.stdout.is_empty(),
            "`gdck {verb} --help` said nothing"
        );
    }
    let version = gdck().arg("--version").output().expect("should run");
    assert!(text(&version.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn a_bad_invocation_is_refused_rather_than_guessed_at() {
    // clap exits 2 for a usage error, which is the same "could not complete"
    // code the run itself uses.
    gdck().arg("frobnicate").assert().code(2);
    gdck()
        .args(["lint", "--no-config", "--config", "ci.toml"])
        .assert()
        .code(2);
}
