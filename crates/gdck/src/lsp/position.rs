//! Byte offsets to the positions the protocol asks for.
//!
//! `gdck` counts bytes. The protocol counts UTF-16 code units by default, and
//! since 3.17 will count UTF-8 bytes instead if the client and server agree on
//! it. The three disagree for every character outside ASCII, so this is where a
//! server quietly puts a squiggle in the wrong place: a line with an accent in
//! it shifts by one, a line with an emoji by more, and nothing looks broken
//! until someone writes a comment in their own language.
//!
//! Nothing here converts the other way. Positions arriving from the client are
//! never turned back into offsets — a code action carries its edits already
//! converted, and formatting replaces the whole document — so the protocol's
//! numbers are only ever written, never read.

use gdck_syntax::{LineIndex, TextRange};

/// How the client counts along a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    /// UTF-8 code units, i.e. bytes. What `gdck` already has, so the
    /// conversion is free — but a client has to ask for it.
    Utf8,
    /// UTF-16 code units. The protocol's default, and what a client that says
    /// nothing is entitled to expect.
    Utf16,
}

/// A document's text, indexed, with the encoding the client agreed to.
pub(crate) struct Positions<'a> {
    text: &'a str,
    index: LineIndex,
    encoding: Encoding,
}

impl<'a> Positions<'a> {
    pub(crate) fn new(text: &'a str, encoding: Encoding) -> Self {
        Self {
            text,
            index: LineIndex::new(text),
            encoding,
        }
    }

    /// A byte offset as a protocol position.
    ///
    /// Both fields are 0-based here, where `LineCol` is 1-based for people.
    pub(crate) fn position(&self, offset: u32) -> lsp_types::Position {
        let at = self.index.line_col(offset);
        // `col` is 1-based bytes into the line, so this is where the line
        // starts and how far in the offset is.
        let byte_in_line = at.col - 1;
        let line_start = (offset - byte_in_line) as usize;

        let character = match self.encoding {
            Encoding::Utf8 => byte_in_line,
            Encoding::Utf16 => {
                // Slicing at `offset` is safe: an offset always lands on a
                // character boundary, since every one comes from the lexer.
                let prefix = &self.text[line_start..offset as usize];
                prefix.chars().map(|c| c.len_utf16() as u32).sum()
            }
        };

        lsp_types::Position {
            line: at.line - 1,
            character,
        }
    }

    pub(crate) fn range(&self, range: TextRange) -> lsp_types::Range {
        lsp_types::Range {
            start: self.position(range.start()),
            end: self.position(range.end()),
        }
    }

    /// The whole document, for an edit that replaces all of it.
    pub(crate) fn whole(&self) -> lsp_types::Range {
        lsp_types::Range {
            start: lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: self.position(self.text.len() as u32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, offset: u32, encoding: Encoding) -> (u32, u32) {
        let position = Positions::new(text, encoding).position(offset);
        (position.line, position.character)
    }

    #[test]
    fn ascii_agrees_whichever_way_it_is_counted() {
        let text = "extends Node\nvar x = 1\n";
        for encoding in [Encoding::Utf8, Encoding::Utf16] {
            assert_eq!(at(text, 0, encoding), (0, 0), "{encoding:?}");
            assert_eq!(at(text, 8, encoding), (0, 8), "{encoding:?}");
            // Byte 13 is the start of the second line.
            assert_eq!(at(text, 13, encoding), (1, 0), "{encoding:?}");
            assert_eq!(at(text, 17, encoding), (1, 4), "{encoding:?}");
        }
    }

    #[test]
    fn a_two_byte_character_counts_once_in_utf16() {
        // `é` is two bytes and one UTF-16 code unit, so everything after it on
        // the line sits one column earlier than its byte offset suggests.
        let text = "var café = 1\n";
        let after = text.find(" = ").unwrap() as u32;
        assert_eq!(at(text, after, Encoding::Utf8), (0, 9));
        assert_eq!(at(text, after, Encoding::Utf16), (0, 8));
    }

    #[test]
    fn a_character_outside_the_basic_plane_counts_twice_in_utf16() {
        // An emoji is four bytes, one `char`, and *two* UTF-16 code units —
        // the case that catches a server counting characters instead of code
        // units, which looks right until it is tried.
        let text = "var a = \"🙂\"\n";
        let end = text.rfind('"').unwrap() as u32;
        assert_eq!(at(text, end, Encoding::Utf8), (0, 13));
        assert_eq!(at(text, end, Encoding::Utf16), (0, 11));

        let one_emoji = "🙂";
        assert_eq!(one_emoji.len(), 4, "four bytes");
        assert_eq!(one_emoji.chars().count(), 1, "one char");
        assert_eq!(
            one_emoji.chars().map(char::len_utf16).sum::<usize>(),
            2,
            "two UTF-16 code units"
        );
    }

    #[test]
    fn a_tab_is_one_unit_however_wide_it_looks() {
        // The formatter measures a tab as four columns; the protocol does not,
        // and an editor placing a squiggle wants the unit, not the width.
        let text = "func f():\n\t\tpass\n";
        let pass = text.find("pass").unwrap() as u32;
        assert_eq!(at(text, pass, Encoding::Utf16), (1, 2));
    }

    #[test]
    fn a_range_and_the_whole_document_line_up() {
        let text = "extends Node\nvar x = 1\n";
        let positions = Positions::new(text, Encoding::Utf16);
        let range = positions.range(TextRange::new(13, 16));
        assert_eq!((range.start.line, range.start.character), (1, 0));
        assert_eq!((range.end.line, range.end.character), (1, 3));

        // The document ends after its final newline, which is the start of a
        // line that has no content — replacing that range replaces everything.
        let whole = positions.whole();
        assert_eq!((whole.start.line, whole.start.character), (0, 0));
        assert_eq!((whole.end.line, whole.end.character), (2, 0));
    }

    #[test]
    fn an_empty_document_has_an_empty_range() {
        let positions = Positions::new("", Encoding::Utf16);
        let whole = positions.whole();
        assert_eq!(whole.start, whole.end);
    }
}
