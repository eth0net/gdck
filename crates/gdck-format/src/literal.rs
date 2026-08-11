//! Normalising the text of literal tokens.
//!
//! These are the style-guide rules that no amount of clever line breaking
//! produces on its own, because they are about the spelling of a token rather
//! than its placement.
//!
//! This module is public so the linter can report a badly spelled literal and
//! offer the formatter's own rewrite as the fix. Two implementations that
//! disagreed would show up as `gdck lint --fix` producing something
//! `gdck format` then changed again.

/// Rewrite a number literal to the style guide's spelling.
///
/// Two rules apply: hexadecimal letters are lowercase, and a float always has
/// a digit on each side of the point. Digit separators are deliberately left
/// alone — the guide suggests them for large numbers but calls the threshold a
/// generality, so inserting or removing them is a judgement a formatter should
/// not make.
#[must_use]
pub fn normalize_number(text: &str) -> String {
    if let Some(rest) = strip_radix_prefix(text) {
        let (prefix, digits) = text.split_at(text.len() - rest.len());
        return format!(
            "{}{}",
            prefix.to_ascii_lowercase(),
            digits.to_ascii_lowercase()
        );
    }

    // Decimal or float. Split off any exponent before touching the point, so
    // that `1.e5` gets the same treatment as `1.`.
    // A radix prefix has already been handled, so any `e` here is an exponent.
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(index) => text.split_at(index),
        None => (text, ""),
    };

    let mut mantissa = mantissa.to_string();
    if mantissa.starts_with('.') {
        mantissa.insert(0, '0');
    }
    if mantissa.ends_with('.') {
        mantissa.push('0');
    }
    format!("{mantissa}{exponent}")
}

fn strip_radix_prefix(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'0' {
        return None;
    }
    match bytes[1] {
        b'x' | b'X' | b'b' | b'B' => Some(&text[2..]),
        _ => None,
    }
}

/// Rewrite a string literal to use the quote style that needs fewer escapes.
///
/// The guide prefers double quotes, allows single quotes when they avoid
/// escapes, and prefers double quotes on a tie. Applies to plain strings and
/// to the `&` and `^` prefixed `StringName` and `NodePath` forms, whose quoted
/// part follows the same rules.
///
/// Raw and triple-quoted strings are returned unchanged: in a raw string the
/// backslash is not reliably an escape, and a triple-quoted string may contain
/// bare quotes whose meaning depends on position.
#[must_use]
pub fn normalize_string(text: &str) -> String {
    let Some(quote_at) = text.find(['"', '\'']) else {
        // `$Node/Path` and `%Unique` have no quoted part.
        return text.to_string();
    };
    let (prefix, quoted) = text.split_at(quote_at);

    if prefix.contains(['r', 'R']) {
        return text.to_string();
    }

    let quote = quoted.as_bytes()[0] as char;
    let triple = [quote; 3].iter().collect::<String>();
    if quoted.starts_with(&triple) {
        return text.to_string();
    }

    // An unterminated literal cannot appear in a tree the formatter accepts,
    // but leaving it alone is cheaper than proving that here.
    if quoted.len() < 2 || !quoted.ends_with(quote) {
        return text.to_string();
    }
    let body = &quoted[1..quoted.len() - 1];

    let units = split_units(body);
    let doubles = units.iter().filter(|unit| represents(unit, '"')).count();
    let singles = units.iter().filter(|unit| represents(unit, '\'')).count();

    // Ties go to double quotes, which is what the guide asks for.
    let target = if doubles <= singles { '"' } else { '\'' };

    let mut out = String::with_capacity(text.len());
    out.push_str(prefix);
    out.push(target);
    for unit in &units {
        if represents(unit, target) {
            out.push('\\');
            out.push(target);
        } else if represents(unit, '"') {
            out.push('"');
        } else if represents(unit, '\'') {
            out.push('\'');
        } else {
            out.push_str(unit);
        }
    }
    out.push(target);
    out
}

/// Split a string body into escape sequences and single characters.
///
/// Keeping `\n` and friends as one unit is what lets the quote style change
/// without disturbing any other escape.
fn split_units(body: &str) -> Vec<&str> {
    let mut units = Vec::new();
    let mut chars = body.char_indices();
    while let Some((start, c)) = chars.next() {
        if c == '\\' {
            if let Some((next_start, next)) = chars.next() {
                units.push(&body[start..next_start + next.len_utf8()]);
                continue;
            }
        }
        units.push(&body[start..start + c.len_utf8()]);
    }
    units
}

/// Whether a unit is the given quote character, escaped or not.
fn represents(unit: &str, quote: char) -> bool {
    let bare = unit.len() == quote.len_utf8() && unit.starts_with(quote);
    let escaped = unit.starts_with('\\') && unit.ends_with(quote) && unit.chars().count() == 2;
    bare || escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_get_a_digit_on_both_sides() {
        // The style guide's own good/bad pair.
        assert_eq!(normalize_number(".234"), "0.234");
        assert_eq!(normalize_number("13."), "13.0");
        assert_eq!(normalize_number("0.234"), "0.234");
        assert_eq!(normalize_number("13.0"), "13.0");
    }

    #[test]
    fn hexadecimal_letters_are_lowercased() {
        assert_eq!(normalize_number("0xFB8C0B"), "0xfb8c0b");
        assert_eq!(normalize_number("0Xfb8c0b"), "0xfb8c0b");
        assert_eq!(normalize_number("0xffff_f8f8_0000"), "0xffff_f8f8_0000");
    }

    #[test]
    fn digit_separators_are_left_alone() {
        assert_eq!(normalize_number("1_234_567_890"), "1_234_567_890");
        assert_eq!(normalize_number("12345"), "12345");
        assert_eq!(normalize_number("12_345"), "12_345");
    }

    #[test]
    fn an_exponent_does_not_confuse_the_point_rules() {
        assert_eq!(normalize_number("1.e5"), "1.0e5");
        assert_eq!(normalize_number("1.5e-3"), "1.5e-3");
    }

    #[test]
    fn binary_literals_keep_their_separators() {
        assert_eq!(normalize_number("0b1101_0010_1010"), "0b1101_0010_1010");
    }

    #[test]
    fn quote_choice_follows_the_style_guide_samples() {
        // Every case in the guide's "Quotes" example.
        assert_eq!(normalize_string(r#""hello world""#), r#""hello world""#);
        assert_eq!(normalize_string(r#""hello 'world'""#), r#""hello 'world'""#);
        assert_eq!(normalize_string(r#"'hello "world"'"#), r#"'hello "world"'"#);
        assert_eq!(
            normalize_string(r#""'hello' \"world\"""#),
            r#""'hello' \"world\"""#
        );
    }

    #[test]
    fn single_quotes_become_double_when_that_costs_nothing() {
        assert_eq!(normalize_string("'plain'"), r#""plain""#);
        assert_eq!(normalize_string(r"'it\'s'"), r#""it's""#);
    }

    #[test]
    fn double_quotes_become_single_when_that_removes_escapes() {
        assert_eq!(normalize_string(r#""say \"hi\"""#), r#"'say "hi"'"#);
    }

    #[test]
    fn a_tie_prefers_double_quotes() {
        assert_eq!(normalize_string(r#"'\'a\' "b"'"#), r#""'a' \"b\"""#);
    }

    #[test]
    fn other_escapes_survive_a_quote_change() {
        assert_eq!(normalize_string(r"'a\nb\tc'"), r#""a\nb\tc""#);
        assert_eq!(normalize_string(r"'\\'"), r#""\\""#);
    }

    #[test]
    fn raw_and_triple_quoted_strings_are_left_alone() {
        assert_eq!(normalize_string(r"r'raw'"), r"r'raw'");
        assert_eq!(normalize_string(r"'''triple'''"), r"'''triple'''");
        assert_eq!(normalize_string(r#""""triple""""#), r#""""triple""""#);
    }

    #[test]
    fn prefixed_string_forms_keep_their_sigil() {
        assert_eq!(normalize_string("&'name'"), r#"&"name""#);
        assert_eq!(normalize_string("^'path'"), r#"^"path""#);
        // A node path written without quotes has nothing to normalise.
        assert_eq!(normalize_string("$Node/Path"), "$Node/Path");
    }
}
