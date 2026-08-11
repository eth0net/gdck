//! Reading the files `gdtoolkit` itself writes.
//!
//! The fixtures beside this file are the literal output of
//! `gdlint --dump-default-config` and `gdformat --dump-default-config`,
//! produced by PyYAML rather than written by hand. That distinction has already
//! mattered once: an earlier hand-written approximation of this file indented
//! its block sequences, which PyYAML does not do, and the reader of the day
//! passed the test while reporting fourteen spurious problems on the real
//! thing.
//!
//! Every value in these files is a `gdtoolkit` default, so none of them is an
//! override and none should draw a word of comment.

use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn gdlints_default_config_is_read_in_full_without_a_word() {
    let loaded = gdck_config::load(&fixture("gdlintrc")).expect("should load");
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);

    // Its defaults are gdck's defaults, so nothing should have moved.
    let config = &loaded.config;
    assert_eq!(config.lint.max_line_length, 100);
    assert_eq!(config.lint.max_file_lines, 1000);
    assert_eq!(config.lint.max_public_methods, 20);
    assert_eq!(config.lint.max_returns, 6);
    assert_eq!(config.lint.max_function_arguments, 10);
    assert!(config.lint.disabled.is_empty());
    assert_eq!(config.excluded_dirs, [".git"]);
}

#[test]
fn gdformats_default_config_is_read_in_full_without_a_word() {
    let loaded = gdck_config::load(&fixture("gdformatrc")).expect("should load");
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);

    let config = &loaded.config;
    assert_eq!(config.format.line_length, 100);
    // `use_spaces: null` and `safety_checks: null` mean "the default", which is
    // tabs and checks on.
    assert_eq!(config.format.indent, gdck_config::IndentStyle::Tabs);
    assert!(config.format.safety_checks);
    assert_eq!(config.excluded_dirs, [".git"]);
}

#[test]
fn a_block_sequence_written_the_way_pyyaml_writes_it_is_read() {
    // PyYAML puts sequence items at column 0 under their key, which is legal
    // YAML and is what `class-definitions-order` and `disable` look like in a
    // dumped file.
    let text = "disable:\n- max-returns\n- unused-argument\nmax-returns: 3\n";
    // The file has to be called `gdlintrc` for `load` to know what it is, so
    // it needs a directory of its own.
    let dir = std::env::temp_dir().join(format!("gdck-pyyaml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("should create");
    let path = dir.join("gdlintrc");
    std::fs::write(&path, text).expect("should write");

    let loaded = gdck_config::load(&path).expect("should load");
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        loaded.config.lint.disabled,
        ["max-returns", "unused-argument"]
    );
    assert_eq!(loaded.config.lint.max_returns, 3);
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
}
