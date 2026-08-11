//! Where comments and blank lines attach.
//!
//! The parser keeps every comment in the tree, but it keeps them as trivia
//! sitting in front of whichever token follows. That is the right thing for a
//! lossless tree and the wrong shape for a formatter, which needs to ask "what
//! comments belong to this statement?" rather than "what trivia precedes this
//! token?".
//!
//! This module answers that by walking the token stream once and building two
//! indexes:
//!
//! * **Leading** comments — those on a line of their own — attach to the next
//!   significant token, together with how many blank lines separate them.
//! * **Trailing** comments — those following code on the same line — attach to
//!   the last significant token before them.
//!
//! Both are keyed by source offset, which is stable regardless of how the tree
//! happens to be nested.

use std::collections::HashMap;

use gdck_syntax::{Element, SyntaxKind, SyntaxNode, SyntaxTree};

/// A comment written on its own line.
#[derive(Debug, Clone)]
pub(crate) struct LeadingComment {
    pub(crate) text: String,
    /// Blank lines between this comment and whatever came before it.
    pub(crate) blank_lines_before: usize,
}

/// Everything attached in front of one significant token.
#[derive(Debug, Clone, Default)]
pub(crate) struct Leading {
    pub(crate) comments: Vec<LeadingComment>,
    /// Blank lines between the last leading comment (or the previous code, if
    /// there are none) and the token itself.
    pub(crate) blank_lines_before: usize,
}

/// Comment and blank-line indexes for one file.
#[derive(Debug, Default)]
pub(crate) struct Trivia {
    leading: HashMap<u32, Leading>,
    trailing: HashMap<u32, String>,
    /// Every comment in the file, in source order, for the safety check.
    all_comments: Vec<String>,
}

impl Trivia {
    /// Build the indexes for a parsed file.
    pub(crate) fn collect(tree: &SyntaxTree) -> Self {
        let mut trivia = Self::default();
        let source = tree.text();

        let mut pending: Vec<LeadingComment> = Vec::new();
        let mut blank_run = 0usize;
        // Whether the current line has produced a significant token yet, which
        // is what distinguishes a trailing comment from a leading one.
        let mut line_has_code = false;
        // Whether the current line is entirely empty so far, so that a newline
        // ending it counts as a blank line.
        let mut line_is_empty = true;
        let mut last_significant: Option<u32> = None;

        for token in flat_tokens(tree.root()) {
            let kind = token.kind;
            let start = token.range.start();

            match kind {
                // Neither code nor comment: the zero-width block markers sit
                // between the indentation and the first token of a line, and
                // whitespace never starts or ends one.
                SyntaxKind::Indent
                | SyntaxKind::Dedent
                | SyntaxKind::Whitespace
                | SyntaxKind::LineContinuation => {}
                SyntaxKind::Newline => {
                    if line_is_empty {
                        blank_run += 1;
                    }
                    line_is_empty = true;
                    line_has_code = false;
                }
                SyntaxKind::Comment | SyntaxKind::DocComment => {
                    let text = token.text(source).trim_end().to_string();
                    trivia.all_comments.push(text.clone());
                    if line_has_code {
                        if let Some(anchor) = last_significant {
                            trivia.trailing.insert(anchor, text);
                        }
                    } else {
                        pending.push(LeadingComment {
                            text,
                            blank_lines_before: blank_run,
                        });
                        blank_run = 0;
                    }
                    line_is_empty = false;
                }
                _ => {
                    if !pending.is_empty() || blank_run > 0 {
                        trivia.leading.insert(
                            start,
                            Leading {
                                comments: std::mem::take(&mut pending),
                                blank_lines_before: blank_run,
                            },
                        );
                    }
                    blank_run = 0;
                    line_is_empty = false;
                    line_has_code = true;
                    // Separators are not anchors. A comment after `1,` belongs
                    // to the `1`: the comma is the list's punctuation, and the
                    // formatter may move or remove it, which would strand the
                    // comment. The same goes for a semicolon, which is dropped
                    // outright.
                    if !matches!(kind, SyntaxKind::Comma | SyntaxKind::Semicolon) {
                        last_significant = Some(start);
                    }
                }
            }
        }

        trivia
    }

    /// What attaches in front of the token starting at `offset`.
    pub(crate) fn leading_at(&self, offset: u32) -> Leading {
        self.leading.get(&offset).cloned().unwrap_or_default()
    }

    /// The same-line comment after the token starting at `offset`, if any.
    pub(crate) fn trailing_at(&self, offset: u32) -> Option<&str> {
        self.trailing.get(&offset).map(String::as_str)
    }

    /// Every comment in the file, for checking none was dropped.
    pub(crate) fn all_comments(&self) -> &[String] {
        &self.all_comments
    }
}

/// Every token in the subtree, in source order.
pub(crate) fn flat_tokens(node: SyntaxNode<'_>) -> Vec<gdck_syntax::Token> {
    let mut out = Vec::new();
    push_tokens(node, &mut out);
    out
}

fn push_tokens(node: SyntaxNode<'_>, out: &mut Vec<gdck_syntax::Token>) {
    for element in node.children() {
        match element {
            Element::Token(token) => out.push(token),
            Element::Node(id) => push_tokens(node.tree().node(id), out),
        }
    }
}

/// The first token in this subtree that carries meaning, skipping trivia and
/// the zero-width block markers.
pub(crate) fn first_significant(node: SyntaxNode<'_>) -> Option<gdck_syntax::Token> {
    flat_tokens(node).into_iter().find(|token| {
        !token.kind.is_trivia()
            && !matches!(
                token.kind,
                SyntaxKind::Indent | SyntaxKind::Dedent | SyntaxKind::Eof
            )
    })
}

/// The last token in this subtree that carries meaning.
pub(crate) fn last_significant(node: SyntaxNode<'_>) -> Option<gdck_syntax::Token> {
    flat_tokens(node).into_iter().rev().find(|token| {
        !token.kind.is_trivia()
            && !matches!(
                token.kind,
                SyntaxKind::Indent | SyntaxKind::Dedent | SyntaxKind::Eof
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trivia_of(source: &str) -> (SyntaxTree, Trivia) {
        let tree = gdck_syntax::parse(source);
        let trivia = Trivia::collect(&tree);
        (tree, trivia)
    }

    #[test]
    fn an_own_line_comment_leads_the_next_token() {
        let source = "# documents f\nfunc f():\n\tpass\n";
        let (_tree, trivia) = trivia_of(source);
        let leading = trivia.leading_at(source.find("func").unwrap() as u32);
        assert_eq!(leading.comments.len(), 1);
        assert_eq!(leading.comments[0].text, "# documents f");
    }

    #[test]
    fn a_same_line_comment_trails_the_code_before_it() {
        let source = "var x = 1 # why\n";
        let (_tree, trivia) = trivia_of(source);
        // Anchored on `1`, the last significant token on the line.
        let anchor = source.find('1').unwrap() as u32;
        assert_eq!(trivia.trailing_at(anchor), Some("# why"));
    }

    #[test]
    fn blank_lines_are_counted_not_collapsed() {
        let source = "var a = 1\n\n\nvar b = 2\n";
        let (_tree, trivia) = trivia_of(source);
        let leading = trivia.leading_at(source.find("var b").unwrap() as u32);
        assert_eq!(leading.blank_lines_before, 2);
        assert!(leading.comments.is_empty());
    }

    #[test]
    fn blank_lines_around_a_comment_are_recorded_separately() {
        let source = "var a = 1\n\n# note\nvar b = 2\n";
        let (_tree, trivia) = trivia_of(source);
        let leading = trivia.leading_at(source.find("var b").unwrap() as u32);
        assert_eq!(leading.comments.len(), 1);
        assert_eq!(leading.comments[0].blank_lines_before, 1);
        // No blank line between the comment and what it documents.
        assert_eq!(leading.blank_lines_before, 0);
    }

    #[test]
    fn every_comment_is_recorded_for_the_safety_check() {
        let source = "# one\nvar x = 1 # two\n## three\nfunc f():\n\tpass\n";
        let (_tree, trivia) = trivia_of(source);
        assert_eq!(trivia.all_comments(), ["# one", "# two", "## three"]);
    }

    #[test]
    fn indentation_markers_do_not_count_as_code() {
        // The Indent token sits between the tab and `pass`; if it were treated
        // as code, the comment below would attach as a trailing comment.
        let source = "func f():\n\t# leading\n\tpass\n";
        let (_tree, trivia) = trivia_of(source);
        let leading = trivia.leading_at(source.find("pass").unwrap() as u32);
        assert_eq!(leading.comments.len(), 1);
        assert_eq!(leading.comments[0].text, "# leading");
    }
}
