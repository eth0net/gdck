//! GDScript formatter.
//!
//! # Status
//!
//! Not implemented. [`format`] currently returns [`FormatError::Unimplemented`]
//! for every input. The parser it builds on is complete; this crate is the next
//! piece of work.
//!
//! # Planned approach
//!
//! A Wadler-style pretty printer over the CST, in two stages:
//!
//! 1. **Lower** the tree to a document IR of `Text`, `Line`, `Group` and
//!    `Indent` nodes. Comments and blank-line runs are carried through as
//!    attachments on the construct they precede or trail, which is why the
//!    parser keeps them in the tree rather than discarding them.
//! 2. **Render** the IR at the configured width, breaking the outermost group
//!    that does not fit and recurring inwards.
//!
//! Style-guide rules that fall out of the IR rather than needing special
//! cases: the 100-character wrap, one space around operators and after commas,
//! two blank lines around top-level definitions and one inside classes,
//! trailing commas on anything that breaks across lines, and single-indent
//! continuations for collections against double-indent for wrapped
//! expressions.
//!
//! Rules needing explicit handling: preferring double quotes unless that adds
//! escapes, lowercase hexadecimal, leading and trailing zeros on floats, and
//! dropping redundant parentheses.
//!
//! # Safety checks
//!
//! Before writing anything, the formatter re-parses its own output and
//! verifies that the token stream is equivalent modulo trivia, that a second
//! formatting pass is a no-op, and that no comment was dropped. A formatter
//! that silently eats code is far worse than one that refuses to run, so these
//! are on by default and `--fast` turns them off.

use std::fmt;

use gdck_config::FormatConfig;
use gdck_syntax::SyntaxTree;

/// Why formatting could not be completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The formatter is not written yet.
    Unimplemented,
    /// The input could not be parsed, so there is nothing safe to rewrite.
    Unparseable,
    /// Formatting changed the meaning of the code. Always a bug in `gdck`.
    SafetyCheckFailed(&'static str),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unimplemented => f.write_str("the formatter is not implemented yet"),
            Self::Unparseable => f.write_str("cannot format a file with syntax errors"),
            Self::SafetyCheckFailed(what) => {
                write!(f, "formatting was rejected by a safety check: {what}")
            }
        }
    }
}

impl std::error::Error for FormatError {}

/// Format a parsed GDScript file.
pub fn format(tree: &SyntaxTree, _config: &FormatConfig) -> Result<String, FormatError> {
    if tree.has_errors() {
        return Err(FormatError::Unparseable);
    }
    Err(FormatError::Unimplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_format_unparseable_input() {
        let tree = gdck_syntax::parse("func f(:\n");
        assert_eq!(
            format(&tree, &FormatConfig::default()),
            Err(FormatError::Unparseable)
        );
    }

    #[test]
    fn reports_that_it_is_unimplemented() {
        let tree = gdck_syntax::parse("pass\n");
        assert_eq!(
            format(&tree, &FormatConfig::default()),
            Err(FormatError::Unimplemented)
        );
    }
}
