//! Lint conformance against an external corpus of GDScript files.
//!
//! Point `GDCK_CORPUS` at a directory of `.gd` files:
//!
//! ```sh
//! GDCK_CORPUS=../godot-gdscript-toolkit/tests cargo test -p gdck-lint --test corpus
//! ```
//!
//! A relative path is taken as relative to the workspace root, since cargo
//! runs integration tests with the *crate* directory as the working directory.
//!
//! What is asserted is not which diagnostics come out — that is the unit
//! tests' job — but that nothing the linter does to a file can damage it:
//!
//! * `--fix` never turns a file that parsed into one that does not.
//! * `--fix` settles: running it twice changes nothing the first run left.
//! * `--fix` never introduces a problem that was not there before.
//! * `--fix-order` only ever permutes the file's bytes, and what comes out
//!   still parses.

use std::path::{Path, PathBuf};

use gdck_config::LintConfig;
use gdck_lint::Reorder;

/// Resolve a possibly-relative corpus path against the workspace root.
fn resolve_from_workspace_root(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    // CARGO_MANIFEST_DIR is `<workspace>/crates/gdck-lint`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(path)
}

fn collect_gd_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_gd_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "gd") {
            out.push(path);
        }
    }
}

fn corpus() -> Option<Vec<PathBuf>> {
    let root = resolve_from_workspace_root(&std::env::var("GDCK_CORPUS").ok()?);
    let mut paths = Vec::new();
    collect_gd_files(&root, &mut paths);
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no .gd files found under {}",
        root.display()
    );
    Some(paths)
}

/// A multiset of the bytes in a string, for asserting a permutation.
fn byte_census(text: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = text.bytes().collect();
    bytes.sort_unstable();
    bytes
}

#[test]
fn fixing_the_corpus_never_damages_a_file() {
    let Some(paths) = corpus() else {
        eprintln!("GDCK_CORPUS not set; skipping lint conformance test");
        return;
    };
    let config = LintConfig::default();
    let mut fixed = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();

    for path in &paths {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let name = path.file_name().and_then(|name| name.to_str());
        if gdck_syntax::parse(&source).has_errors() {
            // Linting a broken file is allowed and useful, but the fixes are
            // not expected to survive a file the parser only half understood.
            skipped += 1;
            continue;
        }

        let before = gdck_lint::lint_file(&gdck_syntax::parse(&source), &config, name);
        let once = gdck_lint::fix_source(&source, &config, name);
        let tree = gdck_syntax::parse(&once);

        if tree.has_errors() {
            failures.push(format!("{}: fixing broke the parse", path.display()));
            continue;
        }
        let twice = gdck_lint::fix_source(&once, &config, name);
        if twice != once {
            failures.push(format!("{}: fixing did not settle", path.display()));
            continue;
        }

        // Rules, not counts: a fix legitimately removes several diagnostics at
        // once, and just as legitimately reveals a line that is now too long.
        // What it must never do is invent a kind of problem the file did not
        // have.
        let after = gdck_lint::lint_file(&tree, &config, name);
        let new_rules: Vec<&str> = after
            .iter()
            .map(|diagnostic| diagnostic.rule)
            .filter(|rule| !before.iter().any(|other| other.rule == *rule))
            .collect();
        if !new_rules.is_empty() {
            failures.push(format!(
                "{}: fixing introduced {new_rules:?}",
                path.display()
            ));
            continue;
        }
        if once != source {
            fixed += 1;
        }
    }

    eprintln!("fixed {fixed} files, skipped {skipped} that do not parse");
    assert!(
        failures.is_empty(),
        "{} files did not survive `--fix`:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn reordering_the_corpus_only_ever_moves_declarations() {
    let Some(paths) = corpus() else {
        eprintln!("GDCK_CORPUS not set; skipping reorder conformance test");
        return;
    };
    let config = LintConfig::default();
    let mut reordered = 0;
    let mut blocked = 0;
    let mut unchanged = 0;
    let mut failures = Vec::new();

    for path in &paths {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        match gdck_lint::reorder(&source, &config) {
            Reorder::Unchanged => unchanged += 1,
            Reorder::Blocked(_) => blocked += 1,
            Reorder::Reordered(after) => {
                reordered += 1;
                if byte_census(&after) != byte_census(&source) {
                    failures.push(format!("{}: not a permutation", path.display()));
                } else if gdck_syntax::parse(&after).has_errors() {
                    failures.push(format!("{}: reordering broke the parse", path.display()));
                } else if !matches!(gdck_lint::reorder(&after, &config), Reorder::Unchanged) {
                    failures.push(format!("{}: reordering did not settle", path.display()));
                }
            }
        }
    }

    eprintln!("reordered {reordered} files, left {unchanged} alone, refused {blocked}");
    assert!(
        failures.is_empty(),
        "{} files were damaged by reordering:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
