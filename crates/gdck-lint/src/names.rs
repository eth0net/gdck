//! The style guide's naming conventions, as predicates.
//!
//! [`gdck_config::naming`] states each convention as a regular expression,
//! which is the form a user would write to override one. These functions match
//! those exact patterns directly, as predicates.
//!
//! Written rather than compiled because the conventions are fixed — `gdck`
//! checks the guide's, and offers no way to configure a different one — and a
//! dozen lines of `is_ascii_uppercase` reads better at that size than a
//! pattern does. If naming ever becomes configurable, this is the module that
//! gets replaced by a real engine rather than extended.

/// `([A-Z][a-z0-9]*)+` — a name made of capitalised words, e.g. `YAMLParser`.
///
/// Runs of capitals are allowed, which is what lets an acronym stay an
/// acronym. What is excluded is an underscore or a leading lowercase letter.
pub(crate) fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_alphanumeric())
}

/// `[a-z][a-z0-9]*(_[a-z0-9]+)*` — lowercase words joined by single
/// underscores, e.g. `load_level`.
pub(crate) fn is_snake_case(name: &str) -> bool {
    is_separated_case(name, char::is_ascii_lowercase)
}

/// `[A-Z][A-Z0-9]*(_[A-Z0-9]+)*` — the same shape in capitals, e.g.
/// `MAX_SPEED`.
pub(crate) fn is_constant_case(name: &str) -> bool {
    is_separated_case(name, char::is_ascii_uppercase)
}

/// The shared shape of `snake_case` and `CONSTANT_CASE`: a word of `letter`s
/// and digits, then any number of underscore-separated words, with no empty
/// word anywhere.
fn is_separated_case(name: &str, letter: fn(&char) -> bool) -> bool {
    let mut words = name.split('_');
    let Some(first) = words.next() else {
        return false;
    };
    // Only the first word must start with a letter. `state_2` is fine;
    // `2_state` is not an identifier in the first place.
    if !first.chars().next().is_some_and(|c| letter(&c)) {
        return false;
    }
    std::iter::once(first)
        .chain(words)
        .all(|word| !word.is_empty() && word.chars().all(|c| letter(&c) || c.is_ascii_digit()))
}

/// Strip the single leading underscore that marks a private member.
///
/// The guide asks for exactly one, so `__thing` keeps an underscore here and
/// fails whichever check follows.
pub(crate) fn without_private_prefix(name: &str) -> &str {
    name.strip_prefix('_').unwrap_or(name)
}

/// `_?[a-z][a-z0-9]*(_[a-z0-9]+)*` — `snake_case`, optionally private.
pub(crate) fn is_private_snake_case(name: &str) -> bool {
    is_snake_case(without_private_prefix(name))
}

/// `_?[A-Z][A-Z0-9]*(_[A-Z0-9]+)*` — `CONSTANT_CASE`, optionally private.
pub(crate) fn is_private_constant_case(name: &str) -> bool {
    is_constant_case(without_private_prefix(name))
}

/// `_?([A-Z][a-z0-9]*)+` — `PascalCase`, optionally private.
pub(crate) fn is_private_pascal_case(name: &str) -> bool {
    is_pascal_case(without_private_prefix(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_accepts_the_guides_examples() {
        assert!(is_pascal_case("YAMLParser"));
        assert!(is_pascal_case("Weapon"));
        assert!(is_pascal_case("CharacterBody3D"));
        assert!(is_pascal_case("Camera3D"));
    }

    #[test]
    fn pascal_case_rejects_underscores_and_lowercase_starts() {
        assert!(!is_pascal_case("yaml_parser"));
        assert!(!is_pascal_case("Yaml_Parser"));
        assert!(!is_pascal_case("weapon"));
        assert!(!is_pascal_case("_Weapon"));
        assert!(!is_pascal_case(""));
        assert!(is_private_pascal_case("_Weapon"));
    }

    #[test]
    fn snake_case_accepts_the_guides_examples() {
        assert!(is_snake_case("particle_effect"));
        assert!(is_snake_case("load_level"));
        assert!(is_snake_case("door_opened"));
        assert!(is_snake_case("state"));
        assert!(is_snake_case("vector2"));
        assert!(is_snake_case("player_2_score"));
    }

    #[test]
    fn snake_case_rejects_capitals_and_empty_words() {
        assert!(!is_snake_case("ParticleEffect"));
        assert!(!is_snake_case("particle_Effect"));
        assert!(!is_snake_case("particle__effect"));
        assert!(!is_snake_case("particle_"));
        assert!(!is_snake_case("_particle"));
        assert!(!is_snake_case(""));
    }

    #[test]
    fn one_leading_underscore_marks_a_private_name() {
        assert!(is_private_snake_case("_counter"));
        assert!(is_private_snake_case("_recalculate_path"));
        assert!(is_private_snake_case("counter"));
        // The guide asks for a single underscore.
        assert!(!is_private_snake_case("__counter"));
    }

    #[test]
    fn constant_case_is_snake_case_in_capitals() {
        assert!(is_constant_case("MAX_SPEED"));
        assert!(is_constant_case("EARTH"));
        assert!(is_constant_case("LEVEL_2"));
        assert!(!is_constant_case("MaxSpeed"));
        assert!(!is_constant_case("max_speed"));
        assert!(!is_constant_case("MAX__SPEED"));
        assert!(is_private_constant_case("_MAX_SPEED"));
    }
}
