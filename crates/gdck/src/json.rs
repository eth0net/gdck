//! Diagnostics as JSON, for something other than a person to read.
//!
//! The records here are declared rather than reused from `gdck-lint`, so the
//! output is a contract of its own: renaming a field inside the linter cannot
//! quietly change what an editor plugin receives, and what a consumer sees is
//! written out in one place where it can be read.
//!
//! One object per line rather than one array per run. A run is a stream — each
//! file is reported as it is read — so newline-delimited output arrives as it
//! is produced instead of waiting for the last file, and survives being cut
//! off mid-run with every completed record still valid. That is what an editor
//! plugin reading incrementally wants; `jq -s .` collects it into an array for
//! anyone who would rather have one.

use gdck_lint::{Diagnostic, Severity};
use gdck_syntax::{LineIndex, TextRange};
use serde::Serialize;

/// A position given three ways, because consumers disagree about which they
/// want and all three are already known.
///
/// `line` and `column` are 1-based, matching what the human output prints and
/// what an editor shows. `offset` is a 0-based byte offset, which is what a
/// program applying an edit needs.
#[derive(Serialize)]
struct Position {
    line: u32,
    column: u32,
    offset: u32,
}

#[derive(Serialize)]
struct Span {
    start: Position,
    end: Position,
}

#[derive(Serialize)]
struct Replacement {
    range: Span,
    text: String,
}

#[derive(Serialize)]
struct Rewrite {
    edits: Vec<Replacement>,
}

/// One reported problem, whatever produced it.
///
/// Syntax errors and files needing reformatting come through here too, under
/// their own `rule` names. A consumer wants one list of everything wrong with
/// a file, not three that have to be merged.
#[derive(Serialize)]
struct Record<'a> {
    file: &'a str,
    /// A rule name, or `syntax-error`, or `format`.
    rule: &'a str,
    severity: &'a str,
    message: &'a str,
    range: Span,
    /// The rewrite `--fix` would apply, so a consumer can apply it without
    /// running `gdck` over the file a second time. Null when there is none.
    fix: Option<Rewrite>,
}

fn position(index: &LineIndex, offset: u32) -> Position {
    let at = index.line_col(offset);
    Position {
        line: at.line,
        column: at.col,
        offset,
    }
}

fn span(index: &LineIndex, range: TextRange) -> Span {
    Span {
        start: position(index, range.start()),
        end: position(index, range.end()),
    }
}

/// Serialising a `Record` cannot fail: every field is a string, a number or a
/// sequence of them, and none of the cases `serde_json` reports an error for —
/// a map with non-string keys, a float that is not a number — can arise.
fn line(record: &Record<'_>) -> String {
    serde_json::to_string(record).expect("a Record is always serialisable")
}

/// One lint diagnostic.
pub(crate) fn diagnostic(file: &str, index: &LineIndex, diagnostic: &Diagnostic) -> String {
    let severity = diagnostic.severity.to_string();
    line(&Record {
        file,
        rule: diagnostic.rule,
        severity: &severity,
        message: &diagnostic.message,
        range: span(index, diagnostic.range),
        fix: diagnostic.fix.as_ref().map(|fix| Rewrite {
            edits: fix
                .edits
                .iter()
                .map(|edit| Replacement {
                    range: span(index, edit.range),
                    text: edit.text.clone(),
                })
                .collect(),
        }),
    })
}

/// A syntax error.
///
/// No fix: the tree could not be read, so there is nothing to propose.
pub(crate) fn syntax_error(file: &str, index: &LineIndex, at: TextRange, message: &str) -> String {
    line(&Record {
        file,
        rule: "syntax-error",
        severity: "error",
        message,
        range: span(index, at),
        fix: None,
    })
}

/// A file the formatter would rewrite.
///
/// The range covers the whole file, because that is what is being reported:
/// not a position to jump to, but a file that does not match its own
/// formatting.
pub(crate) fn unformatted(file: &str, index: &LineIndex, length: u32) -> String {
    let severity = Severity::Warning.to_string();
    line(&Record {
        file,
        rule: "format",
        severity: &severity,
        message: "would be reformatted",
        range: span(index, TextRange::new(0, length)),
        fix: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> serde_json::Value {
        serde_json::from_str(text).expect("output should be valid JSON")
    }

    #[test]
    fn a_diagnostic_carries_its_position_three_ways() {
        let index = LineIndex::new("extends Node\nvar x = 1\n");
        let text = diagnostic(
            "a.gd",
            &index,
            &Diagnostic {
                rule: "quote-style",
                severity: Severity::Warning,
                range: TextRange::new(13, 16),
                message: "an example".to_string(),
                fix: None,
            },
        );
        // One line, so it can be read a record at a time.
        assert!(!text.contains('\n'), "{text}");

        let value = parsed(&text);
        assert_eq!(value["file"], "a.gd");
        assert_eq!(value["rule"], "quote-style");
        assert_eq!(value["severity"], "warning");
        assert_eq!(value["fix"], serde_json::Value::Null);
        // Byte 13 is the start of the second line, all three ways.
        assert_eq!(value["range"]["start"]["line"], 2);
        assert_eq!(value["range"]["start"]["column"], 1);
        assert_eq!(value["range"]["start"]["offset"], 13);
    }

    #[test]
    fn a_fix_carries_the_edits_a_consumer_would_apply() {
        let index = LineIndex::new("var a = 'x'\n");
        let text = diagnostic(
            "a.gd",
            &index,
            &Diagnostic {
                rule: "quote-style",
                severity: Severity::Warning,
                range: TextRange::new(8, 11),
                message: "prefer double quotes".to_string(),
                fix: Some(gdck_lint::Fix::new(vec![gdck_lint::Edit::replace(
                    TextRange::new(8, 11),
                    "\"x\"",
                )])),
            },
        );
        let value = parsed(&text);
        assert_eq!(value["fix"]["edits"][0]["text"], "\"x\"");
        assert_eq!(value["fix"]["edits"][0]["range"]["start"]["offset"], 8);
        assert_eq!(value["fix"]["edits"][0]["range"]["end"]["offset"], 11);
    }

    #[test]
    fn awkward_text_survives_the_round_trip() {
        // The reason this is serialised rather than written by hand: a message
        // or a file name carrying a quote, a backslash or a control character
        // has to come back out exactly as it went in.
        let index = LineIndex::new("\n");
        let text = syntax_error(
            "a\"b\\c.gd",
            &index,
            TextRange::empty(0),
            "a quote \" a backslash \\ a tab \t and a bell \u{7}",
        );
        let value = parsed(&text);
        assert_eq!(value["file"], "a\"b\\c.gd");
        assert_eq!(
            value["message"],
            "a quote \" a backslash \\ a tab \t and a bell \u{7}"
        );
    }

    #[test]
    fn a_file_needing_reformatting_reports_through_the_same_shape() {
        let source = "extends Node\n";
        let index = LineIndex::new(source);
        let value = parsed(&unformatted("a.gd", &index, source.len() as u32));
        assert_eq!(value["rule"], "format");
        assert_eq!(value["severity"], "warning");
        assert_eq!(value["range"]["start"]["offset"], 0);
        assert_eq!(value["range"]["end"]["offset"], 13);
    }
}
