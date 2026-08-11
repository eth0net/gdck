//! Diagnostics produced while lexing and parsing.

use std::fmt;

use crate::text::{LineIndex, TextRange};

/// A problem found while turning source text into a tree.
///
/// Errors never stop the lexer or parser — both always produce a complete,
/// lossless tree. An error means part of that tree is wrapped in
/// [`SyntaxKind::Error`](crate::SyntaxKind::Error), not that parsing gave up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    range: TextRange,
    message: String,
}

impl SyntaxError {
    #[must_use]
    pub fn new(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn range(&self) -> TextRange {
        self.range
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Render as `line:col: message`, the shape most editors and CI log
    /// scrapers expect.
    #[must_use]
    pub fn display_with(&self, index: &LineIndex) -> String {
        format!("{}: {}", index.line_col(self.range.start()), self.message)
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.range, self.message)
    }
}

impl std::error::Error for SyntaxError {}
