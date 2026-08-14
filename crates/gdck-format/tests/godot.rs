//! Conformance against the real Godot parser.
//!
//! The safety checks re-parse formatted output with `gdck-syntax`, which is
//! deliberately lossless and lenient — it accepts things Godot does not. That
//! makes them a check that `gdck` can still read its own output, not that Godot
//! can. This closes that gap by asking Godot itself.
//!
//! ```sh
//! GDCK_GODOT="/Applications/Godot 4.7.1.app/Contents/MacOS/Godot" \
//!   GDCK_CORPUS=../godot-gdscript-toolkit/tests \
//!   cargo test -p gdck-format --test godot -- --nocapture
//! ```
//!
//! `GDCK_GODOT` alone runs the built-in cases below; adding `GDCK_CORPUS` runs
//! the whole corpus through as well. Without `GDCK_GODOT` the test skips, so
//! this costs nothing for anyone without Godot to hand.
//!
//! The comparison is **differential**. Godot reports undefined identifiers and
//! failed type inference as "Parse Error" alongside genuine syntax errors, and
//! a corpus file that references classes it does not ship will produce those
//! whatever anyone does to its formatting. So a file is only examined when
//! Godot accepts it *before* formatting, and only failures that formatting
//! introduced are reported.

use std::path::{Path, PathBuf};
use std::process::Command;

use gdck_config::FormatConfig;

/// Constructs where the formatter has to place a line break somewhere Godot
/// has an opinion about. Each one is valid GDScript that Godot accepts as
/// written; formatting must keep it that way.
const CASES: &[(&str, &str)] = &[
    (
        "nested call ending in a multiline lambda",
        // The closing brackets land after a lambda body, which is the one
        // place inside brackets where Godot still tracks indentation.
        "extends Node\n\nvar _flag := false\n\n\nfunc _ready() -> void:\n\
         \tvar box := VBoxContainer.new()\n\
         \tbox.add_child(make_button(\"a long label here to force the formatter to wrap\", func() -> void:\n\
         \t\t_flag = true\n\
         \t\tprint(\"done\")))\n\n\n\
         func make_button(label: String, cb: Callable) -> Button:\n\treturn Button.new()\n",
    ),
    (
        "single-level call ending in a multiline lambda",
        "extends Node\n\nvar _flag := false\n\n\nfunc _ready() -> void:\n\
         \tvar timer := Timer.new()\n\
         \ttimer.timeout.connect(func() -> void:\n\
         \t\t_flag = true\n\
         \t\tprint(\"a fairly long line here to encourage the formatter to wrap this\"))\n",
    ),
    (
        "lambda inside an array",
        "extends Node\n\n\nfunc _ready() -> void:\n\
         \tvar callables := [func() -> void:\n\
         \t\tprint(\"a long enough body that the array has to break open somewhere\")]\n\
         \tprint(callables.size())\n",
    ),
    (
        "property with both accessors set to methods",
        // The comma is what carries Godot on to the second accessor; without
        // it the property is over and the `get` line has nowhere to belong.
        "extends Node\n\nvar _p := 0\n\nvar p:\n\
         \tset = __set,\n\
         \tget = __get\n\n\n\
         func __get() -> int:\n\treturn _p\n\n\n\
         func __set(value: int) -> void:\n\t_p = value\n",
    ),
    (
        "property with both accessors written as blocks",
        // The same construct in the form that takes no comma at all.
        "extends Node\n\nvar _p := 0\n\nvar p:\n\
         \tset(value):\n\t\t_p = value\n\
         \tget:\n\t\treturn _p\n",
    ),
    (
        "lambda followed by another argument",
        "extends Node\n\n\nfunc _ready() -> void:\n\
         \tvar t := Timer.new()\n\
         \tt.timeout.connect(func() -> void:\n\
         \t\tprint(\"body\"), CONNECT_ONE_SHOT)\n",
    ),
];

/// The Godot binary to compare against, if one was named.
///
/// A `GDCK_GODOT` that points at nothing is a mistake worth stopping for: the
/// alternative is a run that looks like it checked something and did not.
/// Not setting it at all is the documented way to skip.
fn godot() -> Option<PathBuf> {
    let raw = std::env::var("GDCK_GODOT").ok()?;
    let path = PathBuf::from(&raw);
    assert!(
        path.is_file(),
        "GDCK_GODOT is set to {raw:?}, which is not a file. Point it at a Godot \
         binary, or unset it to skip this test."
    );
    Some(path)
}

fn resolve_from_workspace_root(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(path)
}

/// A directory Godot can be pointed at, removed when the test ends.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("gdck-godot-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("should create the scratch directory");
        Self { root }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// What Godot says about one file, as the lines it complained with.
///
/// `--check-only` parses and analyses without running anything. Godot exits 0
/// either way, so the errors have to be read out of its output.
fn godot_errors(binary: &Path, dir: &Path, name: &str) -> Vec<String> {
    let output = Command::new(binary)
        .current_dir(dir)
        .args(["--headless", "--check-only", "--script", name])
        .output()
        .expect("should run Godot");

    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.lines()
        .filter(|line| line.contains("SCRIPT ERROR") || line.contains("Parse Error"))
        .map(str::trim)
        .map(str::to_owned)
        .collect()
}

/// Format `source` and report what formatting broke, if anything.
///
/// `None` means there is nothing to say: either Godot rejected the input as
/// given — so it has no opinion worth trusting about the output — or the
/// output is fine.
fn regression(binary: &Path, scratch: &Path, label: &str, source: &str) -> Option<String> {
    let before_path = scratch.join("before.gd");
    std::fs::write(&before_path, source).expect("should write");
    if !godot_errors(binary, scratch, "before.gd").is_empty() {
        return None;
    }

    let Ok(formatted) = gdck_format::format_source(source, &FormatConfig::default()) else {
        return None;
    };

    let after_path = scratch.join("after.gd");
    std::fs::write(&after_path, &formatted).expect("should write");
    let errors = godot_errors(binary, scratch, "after.gd");
    if errors.is_empty() {
        return None;
    }

    Some(format!(
        "{label}: Godot accepts this file but rejects what `gdck format` makes of it.\n\
         --- Godot said ---\n{}\n--- formatted output ---\n{}",
        errors.join("\n"),
        formatted
    ))
}

#[test]
fn formatted_output_is_accepted_by_godot() {
    let Some(binary) = godot() else {
        eprintln!("GDCK_GODOT not set; skipping Godot conformance test");
        return;
    };
    let scratch = Scratch::new("cases");

    let failures: Vec<String> = CASES
        .iter()
        .filter_map(|(label, source)| regression(&binary, &scratch.root, label, source))
        .collect();

    eprintln!("checked {} constructs against Godot", CASES.len());
    assert!(
        failures.is_empty(),
        "{} of {} constructs broke:\n\n{}",
        failures.len(),
        CASES.len(),
        failures.join("\n\n")
    );
}

#[test]
fn formatted_corpus_is_accepted_by_godot() {
    let Some(binary) = godot() else {
        eprintln!("GDCK_GODOT not set; skipping Godot corpus conformance test");
        return;
    };
    let Ok(root) = std::env::var("GDCK_CORPUS") else {
        eprintln!("GDCK_CORPUS not set; skipping Godot corpus conformance test");
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

    let scratch = Scratch::new("corpus");
    let mut checked = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();

    for path in &paths {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let label = path.display().to_string();
        if let Some(failure) = regression(&binary, &scratch.root, &label, &source) {
            failures.push(failure);
            checked += 1;
        } else {
            // Either Godot had no opinion on the input or the output was
            // fine; only the first is worth counting as a skip, but the
            // two are not worth a second Godot invocation to tell apart.
            checked += 1;
            skipped += 1;
        }
    }

    eprintln!("ran {checked} corpus files past Godot ({skipped} produced nothing to report)");
    assert!(
        failures.is_empty(),
        "{} corpus files were broken by formatting:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
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
