//! Rules about the shape of the lines rather than the code on them.
//!
//! `line-too-long`, `trailing-whitespace`, `mixed-indentation`,
//! `tab-indentation`, `line-ending` and `final-newline`. The formatter
//! produces all of this correctly; these rules exist so that a project can be
//! told what is wrong without having its files rewritten.
//!
//! # Multi-line strings
//!
//! The bytes inside a triple-quoted string are a value, not layout. Trailing
//! spaces there are part of what the program means, and the indentation of a
//! continuation line is text the author chose. Every rule here skips lines
//! that fall inside one, which is why they read the tree instead of only the
//! source text.

use gdck_syntax::{SyntaxKind, TextRange, Token};

use super::{Context, Sink, all_tokens, significant_tokens};
use crate::{Edit, Fix};

/// Columns a tab advances by. Matches the lexer, so a line this rule calls 100
/// columns wide is the same line the formatter would.
const TAB_WIDTH: usize = 4;

pub(crate) fn check(context: &Context<'_>, sink: &mut Sink) {
    let protected = multi_line_strings(context);
    check_lines(context, sink, &protected);
    check_line_endings(context, sink);
    check_final_newline(context, sink);
    check_byte_order_mark(context, sink);
}

/// A leading byte-order mark, which the guide asks files not to carry.
///
/// > Use **UTF-8** encoding without a byte order mark.
///
/// It sits with `line-ending` and `final-newline` because all three come from
/// the same four bullets about encoding, and like them it is unambiguous: the
/// guide admits no exception, so the mark can simply go.
///
/// The lexer takes it as trivia rather than as the first character of the
/// first identifier — which it once was, leaving `extends` unrecognised and
/// the file unparseable — so by the time this runs the file has been read
/// normally and this is the only thing left to say about it.
fn check_byte_order_mark(context: &Context<'_>, sink: &mut Sink) {
    const MARK: char = '\u{feff}';
    if !context.source.starts_with(MARK) {
        return;
    }
    let range = TextRange::new(0, MARK.len_utf8() as u32);
    sink.report_with_fix(
        "byte-order-mark",
        range,
        "file starts with a byte-order mark; the guide asks for UTF-8 without one",
        Fix::new(vec![Edit::delete(range)]),
    );
}

/// The ranges of every string literal that spans more than one line.
fn multi_line_strings(context: &Context<'_>) -> Vec<TextRange> {
    significant_tokens(context.root())
        .into_iter()
        .filter(|token| {
            matches!(token.kind, SyntaxKind::Str) && context.token_text(*token).contains('\n')
        })
        .map(|token| token.range)
        .collect()
}

/// Whether an offset falls strictly within one of the protected ranges.
///
/// Strictly, so that the code before a multi-line string opens and after it
/// closes is still checked.
fn is_protected(protected: &[TextRange], offset: u32) -> bool {
    protected
        .iter()
        .any(|range| offset > range.start() && offset < range.end())
}

fn check_lines(context: &Context<'_>, sink: &mut Sink, protected: &[TextRange]) {
    let max_width = context.config.max_line_length as usize;
    let mut start = 0u32;

    for line in context.source.split_inclusive('\n') {
        let terminated = line.len() - line.trim_end_matches(['\n', '\r']).len();
        let body = &line[..line.len() - terminated];
        let end = start + body.len() as u32;

        // Each check asks about the offset it would report, not about the
        // line as a whole. The closing `"""` of a multi-line string shares a
        // line with the string's last byte and with whatever follows it.
        if !is_protected(protected, start) && !is_protected(protected, end) {
            check_length(sink, body, start, end, max_width);
        }
        if !is_protected(protected, end) {
            check_trailing_whitespace(sink, body, start, end);
        }
        if !is_protected(protected, start) {
            check_indentation(sink, body, start);
        }

        start += line.len() as u32;
    }
}

fn check_length(sink: &mut Sink, content: &str, start: u32, end: u32, max_width: usize) {
    let width = display_width(content);
    if width <= max_width {
        return;
    }
    sink.report(
        "line-too-long",
        TextRange::new(start, end),
        format!("line is {width} columns wide, over the limit of {max_width}"),
    );
}

fn check_trailing_whitespace(sink: &mut Sink, content: &str, start: u32, end: u32) {
    let trimmed = content.trim_end_matches([' ', '\t']);
    if trimmed.len() == content.len() {
        return;
    }
    let range = TextRange::new(start + trimmed.len() as u32, end);
    sink.report_with_fix(
        "trailing-whitespace",
        range,
        "trailing whitespace",
        Fix::new(vec![Edit::delete(range)]),
    );
}

fn check_indentation(sink: &mut Sink, content: &str, start: u32) {
    let indent = &content[..content.len() - content.trim_start_matches([' ', '\t']).len()];
    if indent.is_empty() || indent.len() == content.len() {
        // Nothing indented, or a whitespace-only line, whose whitespace is
        // already `trailing-whitespace`'s to report.
        return;
    }
    let range = TextRange::new(start, start + indent.len() as u32);
    let tabs = indent.contains('\t');
    let spaces = indent.contains(' ');

    // Exactly one of these fires, so a file indented with spaces is not
    // reported twice for the same line.
    if tabs && spaces {
        sink.report(
            "mixed-indentation",
            range,
            "indentation mixes tabs and spaces",
        );
    } else if spaces {
        sink.report("tab-indentation", range, "indent with tabs, not spaces");
    }
}

/// The width of a line in columns, with tabs advancing to the next stop.
fn display_width(text: &str) -> usize {
    let mut width = 0;
    for c in text.chars() {
        if c == '\t' {
            width += TAB_WIDTH - width % TAB_WIDTH;
        } else {
            width += 1;
        }
    }
    width
}

/// Line endings are read from the newline *tokens* rather than by looking for
/// carriage returns in the text, so a `\r` inside a string literal is left
/// alone: there it is data the program depends on, not a line ending.
fn check_line_endings(context: &Context<'_>, sink: &mut Sink) {
    let carriage_returns: Vec<Token> = all_tokens(context.root())
        .into_iter()
        .filter(|token| token.kind == SyntaxKind::Newline && context.token_text(*token) != "\n")
        .collect();
    let Some(first) = carriage_returns.first() else {
        return;
    };

    // One diagnostic for the file. Every line of a CRLF file is wrong, and a
    // report that says so once is more use than one that says so a thousand
    // times.
    let edits = carriage_returns
        .iter()
        .map(|token| {
            let text = context.token_text(*token);
            let feed = text.find('\n').unwrap_or(text.len()) as u32;
            Edit::delete(TextRange::new(
                token.range.start(),
                token.range.start() + feed,
            ))
        })
        .collect();
    sink.report_with_fix(
        "line-ending",
        first.range,
        format!(
            "{} {} end with a carriage return; the style guide asks for line feeds",
            carriage_returns.len(),
            if carriage_returns.len() == 1 {
                "line"
            } else {
                "lines"
            }
        ),
        Fix::new(edits),
    );
}

/// A file ends with exactly one line terminator: no missing one, no run of
/// blank lines.
///
/// A trailing `\r\n` is one terminator and passes here. That it is the wrong
/// terminator is `line-ending`'s to say, and reporting it twice would be
/// reporting one problem twice.
fn check_final_newline(context: &Context<'_>, sink: &mut Sink) {
    let source = context.source;
    if source.is_empty() {
        return;
    }
    let body = source.trim_end_matches(['\n', '\r']).len();
    let tail = &source[body..];
    if tail == "\n" || tail == "\r\n" {
        return;
    }

    // A file that is nothing but blank lines has no last line to terminate.
    let (range, message) = if body == 0 {
        (
            TextRange::new(0, source.len() as u32),
            "file contains nothing but blank lines",
        )
    } else if tail.is_empty() {
        (
            TextRange::empty(source.len() as u32),
            "file does not end with a line feed",
        )
    } else {
        (
            TextRange::new(body as u32, source.len() as u32),
            "file ends with more than one line feed",
        )
    };
    let replacement = if body == 0 { "" } else { "\n" };

    sink.report_with_fix(
        "final-newline",
        range,
        message,
        Fix::new(vec![Edit::replace(range, replacement)]),
    );
}

#[cfg(test)]
mod tests {
    use gdck_config::LintConfig;

    use crate::Diagnostic;

    fn diagnostics(source: &str) -> Vec<Diagnostic> {
        crate::lint(&gdck_syntax::parse(source), &LintConfig::default())
    }

    fn fired(source: &str) -> Vec<&'static str> {
        diagnostics(source)
            .into_iter()
            .map(|diagnostic| diagnostic.rule)
            .collect()
    }

    fn fixed(source: &str) -> String {
        crate::apply_fixes(source, &diagnostics(source))
    }

    #[test]
    fn trailing_whitespace_is_reported_and_removed() {
        assert_eq!(fired("var a = 1   \n"), ["trailing-whitespace"]);
        assert_eq!(fixed("var a = 1   \n"), "var a = 1\n");
        // A blank line carrying indentation counts.
        assert_eq!(
            fixed("func f():\n\tpass\n\t\n\tpass\n"),
            "func f():\n\tpass\n\n\tpass\n"
        );
    }

    #[test]
    fn a_long_line_is_reported_with_its_width() {
        let long = format!("var x = \"{}\"\n", "a".repeat(100));
        let found = diagnostics(&long);
        assert_eq!(found[0].rule, "line-too-long");
        assert!(
            found[0].message.contains("110 columns"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn tabs_count_as_four_columns_so_the_formatter_agrees() {
        // 24 tabs is 96 columns, plus `pass` makes exactly 100.
        let at_limit = format!("{}pass\n", "\t".repeat(24));
        assert!(!fired(&at_limit).contains(&"line-too-long"));
        let over = format!("{}passx\n", "\t".repeat(24));
        assert!(fired(&over).contains(&"line-too-long"));
    }

    #[test]
    fn space_indentation_is_reported_once() {
        assert_eq!(
            fired("func f():\n    pass\n"),
            ["tab-indentation"],
            "a space-indented line is not also mixed indentation"
        );
        assert_eq!(fired("func f():\n\t pass\n"), ["mixed-indentation"]);
        assert_eq!(fired("func f():\n\tpass\n"), Vec::<&str>::new());
    }

    #[test]
    fn a_missing_final_newline_is_added() {
        assert_eq!(fired("var a = 1"), ["final-newline"]);
        assert_eq!(fixed("var a = 1"), "var a = 1\n");
    }

    #[test]
    fn extra_blank_lines_at_the_end_are_removed() {
        assert_eq!(fired("var a = 1\n\n\n"), ["final-newline"]);
        assert_eq!(fixed("var a = 1\n\n\n"), "var a = 1\n");
        assert_eq!(fired("var a = 1\n"), Vec::<&str>::new());
        // An empty file has no final line to end.
        assert_eq!(fired(""), Vec::<&str>::new());
    }

    #[test]
    fn carriage_returns_are_reported_once_for_the_file() {
        let found = diagnostics("var a = 1\r\nvar b = 2\r\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "line-ending");
        assert!(found[0].message.contains("2 lines"));
        assert_eq!(
            fixed("var a = 1\r\nvar b = 2\r\n"),
            "var a = 1\nvar b = 2\n"
        );
    }

    #[test]
    fn a_multi_line_string_keeps_its_own_layout() {
        // The spaces and the indentation inside are part of the value.
        let source = "var text = \"\"\"\n    padded   \n\"\"\"\n";
        assert_eq!(fired(source), Vec::<&str>::new());
    }

    #[test]
    fn code_around_a_multi_line_string_is_still_checked() {
        let source = "var text = \"\"\"\nbody\n\"\"\"   \n";
        assert_eq!(fired(source), ["trailing-whitespace"]);
    }

    #[test]
    fn the_configured_width_is_respected() {
        let mut config = LintConfig::default();
        config.max_line_length = 20;
        let tree = gdck_syntax::parse("var some_name_here = 1234\n");
        let found = crate::lint(&tree, &config);
        assert_eq!(found[0].rule, "line-too-long");
    }

    #[test]
    fn a_byte_order_mark_is_reported_and_removed() {
        let source = "\u{feff}extends Node\n";
        assert_eq!(fired(source), ["byte-order-mark"]);
        assert_eq!(fixed(source), "extends Node\n");
    }

    #[test]
    fn a_byte_order_mark_no_longer_stops_the_file_parsing() {
        // It used to be lexed into the first identifier, so `extends` was not
        // a keyword and nothing after it was understood. The rest of the rules
        // have to see a normal file.
        let tree = gdck_syntax::parse("\u{feff}extends Node\n");
        assert!(!tree.has_errors(), "should parse cleanly");
        // And the tree still reproduces its input exactly.
        assert_eq!(tree.text(), "\u{feff}extends Node\n");
    }

    #[test]
    fn a_mark_that_is_not_at_the_start_is_not_this_rule() {
        // U+FEFF inside a string is data, not an encoding artefact.
        let source = "var a = \"x\u{feff}y\"\n";
        assert_eq!(fired(source), Vec::<&str>::new());
    }
}
