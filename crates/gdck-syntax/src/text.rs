//! Byte-oriented text primitives shared by the lexer, parser and tree.

use std::fmt;

/// A half-open byte range into the source text.
///
/// Ranges are byte offsets, not char offsets. GDScript source is UTF-8 and the
/// lexer only ever splits on ASCII boundaries, so every range produced here is
/// guaranteed to land on a char boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TextRange {
    start: u32,
    end: u32,
}

impl TextRange {
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    /// A zero-width range at `offset`, used for synthetic tokens such as
    /// `Indent`, `Dedent` and `Eof`.
    #[must_use]
    pub const fn empty(offset: u32) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    #[must_use]
    pub const fn start(self) -> u32 {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> u32 {
        self.end
    }

    #[must_use]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// The smallest range covering both `self` and `other`.
    #[must_use]
    pub fn cover(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Slice `text` with this range.
    #[must_use]
    pub fn slice(self, text: &str) -> &str {
        &text[self.start as usize..self.end as usize]
    }
}

impl fmt::Display for TextRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// A 1-based line and column pair, for human-facing diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineCol {
    pub line: u32,
    /// 1-based column counted in UTF-8 bytes from the start of the line.
    pub col: u32,
}

impl fmt::Display for LineCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Maps byte offsets to line/column positions.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the first character of each line.
    line_starts: Vec<u32>,
}

impl LineIndex {
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset as u32 + 1);
            }
        }
        Self { line_starts }
    }

    /// Resolve a byte offset to a 1-based line and column.
    #[must_use]
    pub fn line_col(&self, offset: u32) -> LineCol {
        // partition_point gives the number of line starts <= offset, which is
        // exactly the 1-based line number.
        let line = self.line_starts.partition_point(|&start| start <= offset);
        let line_start = self.line_starts[line - 1];
        LineCol {
            line: line as u32,
            col: offset - line_start + 1,
        }
    }

    /// Number of lines in the indexed text.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_basics() {
        let range = TextRange::new(2, 5);
        assert_eq!(range.len(), 3);
        assert!(!range.is_empty());
        assert_eq!(range.slice("abcdefg"), "cde");
        assert!(TextRange::empty(4).is_empty());
        assert_eq!(TextRange::new(1, 3).cover(TextRange::new(7, 9)).len(), 8);
    }

    #[test]
    fn line_col_resolves_positions() {
        let index = LineIndex::new("ab\ncd\n\nef");
        assert_eq!(index.line_col(0), LineCol { line: 1, col: 1 });
        assert_eq!(index.line_col(1), LineCol { line: 1, col: 2 });
        // The newline itself still belongs to the line it terminates.
        assert_eq!(index.line_col(2), LineCol { line: 1, col: 3 });
        assert_eq!(index.line_col(3), LineCol { line: 2, col: 1 });
        assert_eq!(index.line_col(6), LineCol { line: 3, col: 1 });
        assert_eq!(index.line_col(7), LineCol { line: 4, col: 1 });
        assert_eq!(index.line_count(), 4);
    }

    #[test]
    fn line_col_counts_utf8_bytes() {
        // "é" is two bytes, so the following char sits at byte column 3.
        let index = LineIndex::new("é!");
        assert_eq!(index.line_col(2), LineCol { line: 1, col: 3 });
    }
}
