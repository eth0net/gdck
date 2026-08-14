//! The `gdck` command-line interface.
//!
//! One rule governs the whole surface: **nothing is written to disk without
//! `--fix`**. `check` and `fix` are the everything-at-once verbs; `format` and
//! `lint` are the narrower ones. `--check` is accepted everywhere as a no-op so
//! that muscle memory from `gdformat` and `black` does not produce an error.

mod files;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use gdck_config::{Config, Loaded};
use gdck_syntax::LineIndex;
use similar::TextDiff;

/// Exit code meaning the run found problems.
const EXIT_PROBLEMS: u8 = 1;
/// Exit code meaning the run could not be completed.
const EXIT_ERROR: u8 = 2;

#[derive(Debug, Parser)]
#[command(
    name = "gdck",
    version,
    about = "A fast GDScript formatter and linter",
    long_about = "A fast GDScript formatter and linter that follows the official \
                  GDScript style guide.\n\n\
                  Nothing is written to disk unless you pass --fix."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Report formatting and lint problems without changing anything
    Check(CheckArgs),
    /// Apply every available fix: formatting and fixable lint rules
    Fix(FixArgs),
    /// Report formatting problems, or apply them with --fix
    Format(FormatArgs),
    /// Report lint problems, or apply fixable ones with --fix
    Lint(LintArgs),
    /// Parse files and report syntax errors
    Parse(ParseArgs),
    /// Print the settings a run would use, as a gdck.toml
    Config(CommonArgs),
}

/// The paths to work on, and where the settings come from.
#[derive(Debug, Args)]
struct CommonArgs {
    /// Files or directories to process, and where the search for settings
    /// starts. Use `-` to read standard input
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,
    /// Read settings from this file instead of searching for one
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Ignore every configuration file and use the style guide's defaults
    #[arg(long, conflicts_with = "config")]
    no_config: bool,
}

#[derive(Debug, Args)]
struct CheckArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Show a unified diff of the changes that would be made
    #[arg(short, long)]
    diff: bool,
}

#[derive(Debug, Args)]
struct FixArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Also reorder declarations to the style guide's order
    ///
    /// Only applies when every required move in a file is provably safe; a
    /// file with an initialiser dependency is left completely untouched.
    #[arg(long)]
    fix_order: bool,
    /// Skip the safety checks that verify formatting preserved the code
    #[arg(long)]
    fast: bool,
}

// Flags on a command-line struct are naturally independent booleans; grouping
// them into an enum would only obscure what `--help` shows.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Args)]
struct FormatArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Write the formatted result back to disk
    #[arg(long)]
    fix: bool,
    /// Show a unified diff instead of a summary
    #[arg(short, long)]
    diff: bool,
    /// Accepted for familiarity; reporting is already the default
    #[arg(short, long, hide = true)]
    check: bool,
    /// Skip the safety checks that verify formatting preserved the code
    #[arg(long)]
    fast: bool,
    /// Override the configured line length
    #[arg(short, long, value_name = "COLUMNS")]
    line_length: Option<u16>,
}

#[derive(Debug, Args)]
struct LintArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Apply the fixes for rules that have one
    #[arg(long)]
    fix: bool,
    /// Accepted for familiarity; reporting is already the default
    #[arg(short, long, hide = true)]
    check: bool,
}

#[derive(Debug, Args)]
struct ParseArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Print the concrete syntax tree
    #[arg(short, long)]
    tree: bool,
    /// Print the token stream, before any tree is built
    #[arg(long)]
    tokens: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("gdck: {error:#}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run(cli: &Cli) -> Result<ExitCode> {
    let common = match &cli.command {
        Command::Parse(args) => &args.common,
        Command::Check(args) => &args.common,
        Command::Fix(args) => &args.common,
        Command::Format(args) => &args.common,
        Command::Lint(args) => &args.common,
        Command::Config(args) => args,
    };
    let loaded = settings(common)?;

    if let Command::Config(_) = &cli.command {
        return Ok(run_config(&loaded));
    }

    // Anything a configuration file asked for that gdck cannot do is said out
    // loud, once, before the run it would otherwise silently affect.
    for note in &loaded.notes {
        eprintln!("gdck: {note}");
    }
    report_unknown_rules(&loaded);

    let config = &loaded.config;
    match &cli.command {
        Command::Parse(args) => run_parse(args, config),
        Command::Check(args) => run_check(args, config),
        Command::Fix(args) => run_fix(args, config),
        Command::Format(args) => run_format(args, config),
        Command::Lint(args) => run_lint(args, config),
        Command::Config(_) => unreachable!("handled above"),
    }
}

/// The settings for this run.
///
/// A configuration file that cannot be read is fatal. Falling back to the
/// defaults would mean formatting a project by rules it explicitly rejected,
/// and doing so without saying anything.
fn settings(common: &CommonArgs) -> Result<Loaded> {
    if common.no_config {
        return Ok(Loaded::default());
    }
    match &common.config {
        Some(path) => Ok(gdck_config::load(path)?),
        None => Ok(gdck_config::resolve(&base_dir(&common.paths))?),
    }
}

/// Where the search for a configuration file starts.
///
/// The directory the given paths have in common, so that `gdck check ../game`
/// picks up `../game/gdck.toml` rather than whatever sits above the shell's
/// working directory. One configuration governs the whole run: a monorepo with
/// a `gdck.toml` per project needs one invocation per project.
fn base_dir(paths: &[PathBuf]) -> PathBuf {
    let mut common: Option<PathBuf> = None;
    for path in paths {
        // Standard input is not anywhere, so it says nothing about where to
        // look. A run that is only `-` falls through to the working directory.
        if path.as_os_str() == files::STDIN {
            continue;
        }
        // Relative paths have to be made absolute first, or the walk upwards
        // stops at the front of the path rather than at the filesystem root.
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        let dir = if absolute.is_file() {
            absolute
                .parent()
                .map_or(absolute.clone(), Path::to_path_buf)
        } else {
            absolute
        };
        common = Some(match common {
            None => dir,
            Some(current) => common_prefix(&current, &dir),
        });
    }
    common
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn common_prefix(left: &Path, right: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for (left, right) in left.components().zip(right.components()) {
        if left != right {
            break;
        }
        out.push(left);
    }
    out
}

/// Warn about a `disable` entry that no rule answers to.
///
/// Not an error: a project may share one configuration between `gdck` and
/// `gdlint`, or across versions, and a rule that does not exist here is
/// already switched off. But a typo silently leaving a rule on is worth a word.
fn report_unknown_rules(loaded: &Loaded) {
    let source = loaded.files.last().map_or_else(
        || "configuration".to_string(),
        |path| path.display().to_string(),
    );
    for name in &loaded.config.lint.disabled {
        if gdck_lint::rule(name).is_none() {
            eprintln!("gdck: {source}: no rule is named `{name}`; it is still on");
        }
    }
}

/// Print the settings this run would use, as a `gdck.toml`.
///
/// Which doubles as a way to write one: `gdck config > gdck.toml` produces a
/// file that says exactly what the defaults already do.
fn run_config(loaded: &Loaded) -> ExitCode {
    print!("{}", loaded.config.to_toml());
    if loaded.files.is_empty() {
        eprintln!("No configuration file found; these are the style guide's defaults.");
    } else {
        let files: Vec<String> = loaded
            .files
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        eprintln!("Read {}.", files.join(", "));
    }
    for note in &loaded.notes {
        eprintln!("gdck: {note}");
    }
    report_unknown_rules(loaded);
    ExitCode::SUCCESS
}

fn run_format(args: &FormatArgs, config: &Config) -> Result<ExitCode> {
    let mut format_config = config.format.clone();
    if let Some(line_length) = args.line_length {
        format_config.line_length = line_length;
    }
    if args.fast {
        format_config.safety_checks = false;
    }

    let paths = files::collect(&args.common.paths, config)?;
    if paths.is_empty() {
        eprintln!("No .gd files found.");
        return Ok(ExitCode::SUCCESS);
    }

    let mut changed = Vec::new();
    let mut failed = 0usize;

    for path in &paths {
        let source = match files::read(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("gdck: {error:#}");
                failed += 1;
                continue;
            }
        };

        let formatted = match gdck_format::format_source(&source.text, &format_config) {
            Ok(formatted) => formatted,
            Err(error) => {
                eprintln!("gdck: {}: {error}", source.name);
                failed += 1;
                continue;
            }
        };

        // Reading from standard input makes this a filter: there is no file to
        // write back to, so the formatted text *is* the output, with or without
        // --fix. Anything else and `gdck format - < in.gd > out.gd` writes the
        // literal string `-` over the file, or empties an already-clean one.
        let is_stdin = source.name == files::STDIN;

        if formatted == source.text {
            if is_stdin && !args.diff {
                print!("{formatted}");
            }
            continue;
        }
        changed.push(source.name.clone());

        if args.diff {
            print_diff(&source.name, &source.text, &formatted);
        } else if is_stdin {
            print!("{formatted}");
        } else if args.fix {
            std::fs::write(path, &formatted)?;
        } else {
            println!("{}", source.name);
        }
    }

    if failed > 0 {
        return Ok(ExitCode::from(EXIT_ERROR));
    }

    // Summaries go to standard error so that standard output carries only
    // content: the formatted file when reading from `-`, the diff under
    // --diff, and otherwise the list of paths that would change, which is
    // what makes `gdck format` usable in a pipeline.
    if changed.is_empty() {
        eprintln!(
            "{} {} already formatted.",
            paths.len(),
            plural(paths.len(), "file is", "files are")
        );
        return Ok(ExitCode::SUCCESS);
    }

    if args.fix {
        eprintln!(
            "Formatted {} {}.",
            changed.len(),
            plural(changed.len(), "file", "files")
        );
        return Ok(ExitCode::SUCCESS);
    }

    eprintln!(
        "{} {} would be reformatted. Run with --fix to apply.",
        changed.len(),
        plural(changed.len(), "file", "files")
    );
    Ok(ExitCode::from(EXIT_PROBLEMS))
}

/// A unified diff of what `--fix` would change.
///
/// Hunks matter here rather than being a nicety. Two one-line changes at
/// opposite ends of a file are two hunks; without them the whole span between
/// gets printed as removed and re-added, which on a real script means printing
/// the file twice.
fn print_diff(name: &str, before: &str, after: &str) {
    print!("{}", diff(name, before, after));
}

fn diff(name: &str, before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(CONTEXT_LINES)
        .header(name, &format!("{name} (formatted)"))
        .to_string()
}

/// Lines of unchanged context around each hunk, as `diff -u` uses.
const CONTEXT_LINES: usize = 3;

// -- lint -------------------------------------------------------------------

fn run_lint(args: &LintArgs, config: &Config) -> Result<ExitCode> {
    let paths = files::collect(&args.common.paths, config)?;
    if paths.is_empty() {
        eprintln!("No .gd files found.");
        return Ok(ExitCode::SUCCESS);
    }

    let mut reported = 0usize;
    let mut fixed_files = 0usize;
    let mut failed = 0usize;

    for path in &paths {
        let source = match files::read(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("gdck: {error:#}");
                failed += 1;
                continue;
            }
        };
        let name = file_name(path);
        let mut text = source.text.clone();

        if args.fix {
            let fixed = gdck_lint::fix_source(&text, &config.lint, name);
            if fixed != text {
                fixed_files += 1;
                text = fixed;
                if source.name != files::STDIN {
                    std::fs::write(path, &text)?;
                }
            }
            // Standard input has nowhere to write back to, and has to come
            // back out even when nothing changed or `gdck lint --fix - < in >
            // out` would empty a clean file.
            if source.name == files::STDIN {
                print!("{text}");
            }
        }

        let tree = gdck_syntax::parse(&text);
        let diagnostics = gdck_lint::lint_file(&tree, &config.lint, name);
        reported += diagnostics.len();
        // When the fixed file owns standard output, what is left to report
        // goes beside it rather than into it.
        let to_stderr = args.fix && source.name == files::STDIN;
        print_diagnostics(&source.name, &text, &diagnostics, to_stderr);
    }

    if failed > 0 {
        return Ok(ExitCode::from(EXIT_ERROR));
    }
    if args.fix {
        eprintln!(
            "Fixed {} {}.",
            fixed_files,
            plural(fixed_files, "file", "files")
        );
    }
    if reported == 0 {
        eprintln!(
            "{} {} clean.",
            paths.len(),
            plural(paths.len(), "file is", "files are")
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "Found {reported} {}.",
        plural(reported, "problem", "problems")
    );
    Ok(ExitCode::from(EXIT_PROBLEMS))
}

/// One line per diagnostic: `file:line:col: severity: message [rule]`.
///
/// The rule name is on every line so that the fix — adding it to `disable` or
/// to a `# gdlint: ignore=` comment — can be copied straight out of the report.
fn print_diagnostics(
    name: &str,
    source: &str,
    diagnostics: &[gdck_lint::Diagnostic],
    to_stderr: bool,
) {
    if diagnostics.is_empty() {
        return;
    }
    let index = LineIndex::new(source);
    for diagnostic in diagnostics {
        let at = index.line_col(diagnostic.range.start());
        let line = format!(
            "{name}:{at}: {}: {} [{}]",
            diagnostic.severity, diagnostic.message, diagnostic.rule
        );
        if to_stderr {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    }
}

/// The final path component, which is what the `file-name` rule is about.
fn file_name(path: &std::path::Path) -> Option<&str> {
    if path.as_os_str() == files::STDIN {
        // There is no file, so there is no file name to hold to a convention.
        return None;
    }
    path.file_name().and_then(|name| name.to_str())
}

// -- check and fix ----------------------------------------------------------

fn run_check(args: &CheckArgs, config: &Config) -> Result<ExitCode> {
    let paths = files::collect(&args.common.paths, config)?;
    if paths.is_empty() {
        eprintln!("No .gd files found.");
        return Ok(ExitCode::SUCCESS);
    }

    let mut unformatted = 0usize;
    let mut reported = 0usize;
    let mut failed = 0usize;

    for path in &paths {
        let source = match files::read(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("gdck: {error:#}");
                failed += 1;
                continue;
            }
        };

        let tree = gdck_syntax::parse(&source.text);
        let diagnostics = gdck_lint::lint_file(&tree, &config.lint, file_name(path));
        reported += diagnostics.len();
        print_diagnostics(&source.name, &source.text, &diagnostics, false);

        match gdck_format::format(&tree, &config.format) {
            Ok(formatted) if formatted == source.text => {}
            Ok(formatted) => {
                unformatted += 1;
                if args.diff {
                    print_diff(&source.name, &source.text, &formatted);
                } else {
                    println!("{}: would be reformatted", source.name);
                }
            }
            // A file that does not parse has already been reported on by the
            // linter, which does not need it to.
            Err(gdck_format::FormatError::Unparseable) => {}
            Err(error) => {
                eprintln!("gdck: {}: {error}", source.name);
                failed += 1;
            }
        }
    }

    if failed > 0 {
        return Ok(ExitCode::from(EXIT_ERROR));
    }
    if reported == 0 && unformatted == 0 {
        eprintln!(
            "{} {} clean.",
            paths.len(),
            plural(paths.len(), "file is", "files are")
        );
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "Found {reported} lint {} and {unformatted} {} to reformat. Run `gdck fix` to apply.",
        plural(reported, "problem", "problems"),
        plural(unformatted, "file", "files")
    );
    Ok(ExitCode::from(EXIT_PROBLEMS))
}

fn run_fix(args: &FixArgs, config: &Config) -> Result<ExitCode> {
    let mut format_config = config.format.clone();
    if args.fast {
        format_config.safety_checks = false;
    }
    let paths = files::collect(&args.common.paths, config)?;
    if paths.is_empty() {
        eprintln!("No .gd files found.");
        return Ok(ExitCode::SUCCESS);
    }

    let mut changed = 0usize;
    let mut remaining = 0usize;
    let mut failed = 0usize;

    for path in &paths {
        let source = match files::read(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("gdck: {error:#}");
                failed += 1;
                continue;
            }
        };
        let name = file_name(path);

        // Reordering, then lint fixes, then formatting. Each stage leaves
        // something for the next to settle: a moved declaration brings its old
        // blank lines with it, and a rewritten operator or a dropped
        // parenthesis leaves spacing behind. Any other order would leave that
        // in the file.
        let mut text = source.text.clone();
        if args.fix_order {
            match gdck_lint::reorder(&text, &config.lint) {
                gdck_lint::Reorder::Reordered(reordered) => text = reordered,
                gdck_lint::Reorder::Unchanged => {}
                gdck_lint::Reorder::Blocked(reason) => {
                    // Not a failure. The file is left exactly as it was, which
                    // is the documented outcome, and the reason is what the
                    // author needs in order to decide what to do about it.
                    eprintln!("gdck: {}: not reordered: {reason}", source.name);
                }
            }
        }
        text = gdck_lint::fix_source(&text, &config.lint, name);
        match gdck_format::format_source(&text, &format_config) {
            Ok(formatted) => text = formatted,
            Err(gdck_format::FormatError::Unparseable) => {}
            Err(error) => {
                eprintln!("gdck: {}: {error}", source.name);
                failed += 1;
                continue;
            }
        }

        if text != source.text {
            changed += 1;
            if source.name != files::STDIN {
                std::fs::write(path, &text)?;
            }
        }
        if source.name == files::STDIN {
            print!("{text}");
        }

        let tree = gdck_syntax::parse(&text);
        let diagnostics = gdck_lint::lint_file(&tree, &config.lint, name);
        remaining += diagnostics.len();
        print_diagnostics(
            &source.name,
            &text,
            &diagnostics,
            source.name == files::STDIN,
        );
    }

    if failed > 0 {
        return Ok(ExitCode::from(EXIT_ERROR));
    }
    eprintln!("Fixed {changed} {}.", plural(changed, "file", "files"));
    if remaining == 0 {
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "{remaining} {} left that no fix can resolve.",
        plural(remaining, "problem", "problems")
    );
    Ok(ExitCode::from(EXIT_PROBLEMS))
}

fn run_parse(args: &ParseArgs, config: &Config) -> Result<ExitCode> {
    let paths = files::collect(&args.common.paths, config)?;
    if paths.is_empty() {
        eprintln!("No .gd files found.");
        return Ok(ExitCode::SUCCESS);
    }

    let mut problem_count = 0usize;
    let mut failed_files = 0usize;

    for path in &paths {
        let source = match files::read(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("gdck: {error:#}");
                failed_files += 1;
                continue;
            }
        };

        if args.tokens {
            print_tokens(&source);
        }

        let tree = gdck_syntax::parse(&source.text);

        if args.tree {
            print!("{tree}");
        }

        let index = LineIndex::new(&source.text);
        for error in tree.errors() {
            println!("{}:{}", source.name, error.display_with(&index));
            problem_count += 1;
        }
    }

    if failed_files > 0 {
        return Ok(ExitCode::from(EXIT_ERROR));
    }

    if problem_count > 0 {
        eprintln!(
            "Found {problem_count} syntax {} in {} {}.",
            plural(problem_count, "error", "errors"),
            paths.len(),
            plural(paths.len(), "file", "files"),
        );
        return Ok(ExitCode::from(EXIT_PROBLEMS));
    }

    if !args.tree && !args.tokens {
        eprintln!(
            "Parsed {} {} with no syntax errors.",
            paths.len(),
            plural(paths.len(), "file", "files")
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn print_tokens(source: &files::SourceFile) {
    let lexed = gdck_syntax::tokenize(&source.text);
    for token in &lexed.tokens {
        let text = token.text(&source.text);
        if text.is_empty() {
            println!("{:?}@{}", token.kind, token.range);
        } else {
            println!("{:?}@{} {text:?}", token.kind, token.range);
        }
    }
}

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn nothing_writes_without_an_explicit_fix_flag() {
        // The one rule the whole interface is built on. `check` and `parse`
        // have no way to write at all; `format` and `lint` need --fix.
        let format = Cli::try_parse_from(["gdck", "format", "src/"]).expect("should parse");
        match format.command {
            Command::Format(args) => assert!(!args.fix),
            other => panic!("expected format, got {other:?}"),
        }

        let lint = Cli::try_parse_from(["gdck", "lint", "src/"]).expect("should parse");
        match lint.command {
            Command::Lint(args) => assert!(!args.fix),
            other => panic!("expected lint, got {other:?}"),
        }
    }

    #[test]
    fn check_flag_is_accepted_for_familiarity() {
        // `gdformat --check` and `black --check` users will type this.
        let cli = Cli::try_parse_from(["gdck", "format", "--check", "src/"])
            .expect("--check should be accepted");
        match cli.command {
            Command::Format(args) => {
                assert!(args.check);
                assert!(!args.fix, "--check must not imply writing");
            }
            other => panic!("expected format, got {other:?}"),
        }
    }

    #[test]
    fn paths_default_to_the_working_directory() {
        let cli = Cli::try_parse_from(["gdck", "check"]).expect("should parse");
        match cli.command {
            Command::Check(args) => assert_eq!(args.common.paths, vec![PathBuf::from(".")]),
            other => panic!("expected check, got {other:?}"),
        }
    }

    #[test]
    fn fix_order_is_opt_in() {
        let cli = Cli::try_parse_from(["gdck", "fix", "."]).expect("should parse");
        match cli.command {
            Command::Fix(args) => assert!(!args.fix_order),
            other => panic!("expected fix, got {other:?}"),
        }
    }

    #[test]
    fn a_config_file_can_be_named_or_refused() {
        let cli = Cli::try_parse_from(["gdck", "check", "--config", "ci.toml", "src/"])
            .expect("should parse");
        match cli.command {
            Command::Check(args) => {
                assert_eq!(args.common.config, Some(PathBuf::from("ci.toml")));
                assert!(!args.common.no_config);
            }
            other => panic!("expected check, got {other:?}"),
        }

        let cli = Cli::try_parse_from(["gdck", "lint", "--no-config"]).expect("should parse");
        match cli.command {
            Command::Lint(args) => assert!(args.common.no_config),
            other => panic!("expected lint, got {other:?}"),
        }

        // Naming a file and refusing to read one cannot both be meant.
        Cli::try_parse_from(["gdck", "lint", "--no-config", "--config", "ci.toml"])
            .expect_err("should conflict");
    }

    #[test]
    fn the_config_search_starts_where_the_paths_agree() {
        let base = base_dir(&[
            PathBuf::from("game/scenes/player.gd"),
            PathBuf::from("game/scripts"),
        ]);
        let expected = std::path::absolute("game").expect("should be absolute");
        assert_eq!(base, expected);

        // Standard input is not anywhere, so it says nothing about where to
        // look and the working directory is what is left.
        let cwd = std::env::current_dir().expect("should have a working directory");
        assert_eq!(base_dir(&[PathBuf::from("-")]), cwd);
    }

    #[test]
    fn distant_changes_are_separate_hunks() {
        // The whole point of using a real diff. An earlier hand-written one
        // trimmed the common prefix and suffix and printed everything between,
        // so these two one-line changes came out as the entire file, twice.
        let before = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let after = "A\nb\nc\nd\ne\nf\ng\nh\ni\nj\nK\n";
        let diff = diff("x.gd", before, after);

        assert_eq!(diff.matches("@@").count(), 4, "two hunks:\n{diff}");
        assert!(diff.contains("-a\n") && diff.contains("+A\n"), "{diff}");
        assert!(diff.contains("-k\n") && diff.contains("+K\n"), "{diff}");
        // The untouched middle appears once as context, not twice as a change.
        assert!(!diff.contains("-e\n"), "{diff}");
        assert!(!diff.contains("+e\n"), "{diff}");
    }

    #[test]
    fn pluralisation_reads_correctly() {
        assert_eq!(plural(1, "file", "files"), "file");
        assert_eq!(plural(0, "file", "files"), "files");
        assert_eq!(plural(2, "file", "files"), "files");
    }
}
