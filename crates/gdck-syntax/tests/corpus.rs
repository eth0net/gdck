//! Conformance test against an external corpus of GDScript files.
//!
//! Point `GDCK_CORPUS` at a directory of `.gd` files and this walks all of them,
//! checking the two invariants that matter:
//!
//! * every file round-trips through the tree byte for byte, and
//! * no file makes the lexer or parser panic or hang.
//!
//! Parse *errors* are not failures here — the corpus may contain deliberately
//! invalid scripts. Losslessness must hold regardless, since a formatter must
//! never damage a file it merely failed to understand.
//!
//! ```sh
//! GDCK_CORPUS=../godot-gdscript-toolkit/tests cargo test -p gdck-syntax --test corpus
//! ```
//!
//! The test is skipped when the variable is unset, so it never breaks a clean
//! checkout or CI.

use std::path::{Path, PathBuf};

fn collect_gd_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_gd_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "gd") {
            out.push(path);
        }
    }
}

#[test]
fn corpus_round_trips() {
    let Ok(root) = std::env::var("GDCK_CORPUS") else {
        eprintln!("GDCK_CORPUS not set; skipping corpus conformance test");
        return;
    };

    let mut paths = Vec::new();
    collect_gd_files(Path::new(&root), &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "no .gd files found under {root}");

    let mut checked = 0;
    let mut with_errors = Vec::new();

    for path in &paths {
        // Some corpora carry deliberately broken encodings; skip what is not
        // valid UTF-8 rather than failing.
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };

        let tree = gdck_syntax::parse(&source);
        assert_eq!(
            tree.text(),
            source,
            "{} did not round-trip through the tree",
            path.display()
        );

        // The root must cover the whole file, or something was dropped.
        let covered = tree.root().range().end() as usize;
        assert_eq!(
            covered,
            source.len(),
            "{} left bytes outside the tree",
            path.display()
        );

        if tree.has_errors() {
            with_errors.push(path.clone());
        }
        checked += 1;
    }

    eprintln!(
        "checked {checked} files, {} parsed with errors",
        with_errors.len()
    );
}
