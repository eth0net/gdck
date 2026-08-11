//! The rule catalogue and its documentation say the same thing.
//!
//! `docs/RULES.md` is what a user reads to decide whether to switch a rule off,
//! so a rule that ships undocumented is a rule nobody can turn off on purpose,
//! and a documented rule that does not exist is a promise the tool does not
//! keep.

use std::path::Path;

fn rules_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("RULES.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Every ``` `name` ``` in the document, which is how a rule is written there.
fn quoted_names(text: &str) -> Vec<&str> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
        .collect()
}

#[test]
fn every_rule_is_documented() {
    let doc = rules_doc();
    let missing: Vec<&str> = gdck_lint::RULES
        .iter()
        .map(|rule| rule.name)
        .filter(|name| !doc.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "these rules are not mentioned in docs/RULES.md: {missing:?}"
    );
}

#[test]
fn every_alias_is_documented() {
    let doc = rules_doc();
    let missing: Vec<&str> = gdck_lint::RULES
        .iter()
        .flat_map(|rule| rule.aliases)
        .copied()
        .filter(|name| !doc.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "these gdtoolkit aliases are accepted but not documented: {missing:?}"
    );
}

/// A name in the document that no rule answers to is a stale entry, or a typo
/// that would send someone to add a `disable` line that does nothing.
#[test]
fn the_document_invents_no_rules() {
    let doc = rules_doc();
    // Names that look like rules but are something else: configuration keys,
    // file names, and the `gdlint:` directives.
    let not_rules = [
        "gdlintrc", "disable", "enable", "ignore", "and", "or", "not", "pass", "class", "func",
        "var", "const", "signal", "enum", "self",
    ];
    let unknown: Vec<&str> = quoted_names(&doc)
        .into_iter()
        .filter(|name| name.contains('-'))
        .filter(|name| !not_rules.contains(name))
        .filter(|name| gdck_lint::rule(name).is_none())
        .collect();
    assert!(
        unknown.is_empty(),
        "docs/RULES.md names rules that do not exist: {unknown:?}"
    );
}
