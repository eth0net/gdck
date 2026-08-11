//! Formatting conformance against an external corpus of GDScript files.
//!
//! Point `GDCK_CORPUS` at a directory of `.gd` files and this formats every
//! one of them with the safety checks on. A file that formats at all has
//! therefore been checked to still parse, to mean the same thing, to have kept
//! every comment, and to be stable under a second pass.
//!
//! ```sh
//! GDCK_CORPUS=../godot-gdscript-toolkit/tests cargo test -p gdck-format --test corpus
//! ```
//!
//! A relative path is taken as relative to the workspace root, since cargo
//! runs integration tests with the *crate* directory as the working directory.
//!
//! Files that do not parse are skipped rather than failed: a corpus may hold
//! deliberately invalid scripts, and refusing to format those is correct.

use std::path::{Path, PathBuf};

use gdck_config::FormatConfig;

/// Resolve a possibly-relative corpus path against the workspace root.
fn resolve_from_workspace_root(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    // CARGO_MANIFEST_DIR is `<workspace>/crates/gdck-format`.
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

#[test]
fn corpus_formats_and_survives_the_safety_checks() {
    let Ok(root) = std::env::var("GDCK_CORPUS") else {
        eprintln!("GDCK_CORPUS not set; skipping formatter conformance test");
        return;
    };
    let root = resolve_from_workspace_root(&root);

    let mut paths = Vec::new();
    collect_gd_files(&root, &mut paths);
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no .gd files found under {}",
        root.display()
    );

    let config = FormatConfig::default();
    let mut formatted = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();

    for path in &paths {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        if gdck_syntax::parse(&source).has_errors() {
            // Refusing to format a file with syntax errors is the intended
            // behaviour, and a corpus may carry such files on purpose.
            skipped += 1;
            continue;
        }
        match gdck_format::format_source(&source, &config) {
            Ok(_) => formatted += 1,
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }

    eprintln!("formatted {formatted} files, skipped {skipped} that do not parse");
    assert!(
        failures.is_empty(),
        "{} of {} files failed a safety check:\n{}",
        failures.len(),
        formatted + failures.len(),
        failures.join("\n")
    );
}
