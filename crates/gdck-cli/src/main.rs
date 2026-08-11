//! The `gdck` command-line interface.
//!
//! One rule governs the whole surface: **nothing is written to disk without
//! `--fix`**. `check` and `fix` are the everything-at-once verbs; `format` and
//! `lint` are the narrower ones. `--check` is accepted everywhere as a no-op so
//! that muscle memory from `gdformat` and `black` does not produce an error.

mod files;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use gdck_config::Config;
use gdck_syntax::LineIndex;

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
}

/// Paths shared by every subcommand.
#[derive(Debug, Args)]
struct PathArgs {
    /// Files or directories to process. Use `-` to read standard input
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,
}

#[derive(Debug, Args)]
struct CheckArgs {
    #[command(flatten)]
    paths: PathArgs,
    /// Show a unified diff of the changes that would be made
    #[arg(short, long)]
    diff: bool,
}

#[derive(Debug, Args)]
struct FixArgs {
    #[command(flatten)]
    paths: PathArgs,
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
    paths: PathArgs,
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
    paths: PathArgs,
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
    paths: PathArgs,
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
    // Reading configuration from disk is not wired up yet, so every run uses
    // the style-guide defaults.
    let config = Config::default();

    match &cli.command {
        Command::Parse(args) => run_parse(args, &config),
        Command::Check(_) => Ok(unimplemented_command(
            "check",
            "It will run the linter and report formatting differences.",
        )),
        Command::Fix(_) => Ok(unimplemented_command(
            "fix",
            "It will format files and apply fixable lint rules.",
        )),
        Command::Format(args) => run_format(args, &config),
        Command::Lint(_) => Ok(unimplemented_command(
            "lint",
            "See docs/RULES.md for the rules it will ship with.",
        )),
    }
}

/// Report a subcommand that exists in the interface but has no implementation.
///
/// The surface is wired up in full deliberately: it pins the design down and
/// means `gdck --help` documents where the project is going.
fn unimplemented_command(name: &str, note: &str) -> ExitCode {
    eprintln!("gdck: `{name}` is not implemented yet.");
    eprintln!("      {note}");
    eprintln!("      `gdck parse` works today. Progress: https://github.com/eth0net/gdck");
    ExitCode::from(EXIT_ERROR)
}

fn run_format(args: &FormatArgs, config: &Config) -> Result<ExitCode> {
    let mut format_config = config.format.clone();
    if let Some(line_length) = args.line_length {
        format_config.line_length = line_length;
    }
    if args.fast {
        format_config.safety_checks = false;
    }

    let paths = files::collect(&args.paths.paths, config)?;
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

        if formatted == source.text {
            // Standard input has to come back out either way, or `gdck format
            // --fix - < in.gd > out.gd` would empty an already-clean file.
            if args.fix && source.name == files::STDIN {
                print!("{formatted}");
            }
            continue;
        }
        changed.push(source.name.clone());

        if args.diff {
            print_diff(&source.name, &source.text, &formatted);
        }

        if args.fix {
            // Reading from standard input has nowhere to write back to, so the
            // result goes to standard output instead.
            if source.name == files::STDIN {
                print!("{formatted}");
            } else {
                std::fs::write(path, &formatted)?;
            }
        } else if !args.diff {
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

/// A minimal unified diff, enough to see what would change.
///
/// Written by hand rather than pulled in as a dependency: the output is only
/// ever read by a person deciding whether to run `--fix`, so the cost of a
/// crate that computes a minimal edit script is not worth paying.
fn print_diff(name: &str, before: &str, after: &str) {
    println!("--- {name}");
    println!("+++ {name} (formatted)");

    let before: Vec<&str> = before.lines().collect();
    let after: Vec<&str> = after.lines().collect();

    // Trim the common prefix and suffix so the report is about what changed.
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    let remaining = before.len().min(after.len()) - prefix;
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(remaining)
        .take_while(|(a, b)| a == b)
        .count();

    for line in &before[prefix..before.len() - suffix] {
        println!("-{line}");
    }
    for line in &after[prefix..after.len() - suffix] {
        println!("+{line}");
    }
}

fn run_parse(args: &ParseArgs, config: &Config) -> Result<ExitCode> {
    let paths = files::collect(&args.paths.paths, config)?;
    if paths.is_empty() {
        println!("No .gd files found.");
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
        println!(
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
            Command::Check(args) => assert_eq!(args.paths.paths, vec![PathBuf::from(".")]),
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
    fn pluralisation_reads_correctly() {
        assert_eq!(plural(1, "file", "files"), "file");
        assert_eq!(plural(0, "file", "files"), "files");
        assert_eq!(plural(2, "file", "files"), "files");
    }
}
