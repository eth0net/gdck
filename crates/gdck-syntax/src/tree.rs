//! The concrete syntax tree.
//!
//! Every byte of the source lives in exactly one token, and every token lives
//! in the tree, so [`SyntaxTree::text`] reproduces the input exactly. That is
//! what lets a formatter rewrite one construct while leaving the surrounding
//! comments and blank lines untouched.
//!
//! Nodes live in a flat arena and refer to each other by index. Compared with
//! `Rc`-based trees this keeps children contiguous and traversal
//! cache-friendly, at the cost of needing the tree around to interpret a
//! [`NodeId`].

use std::fmt::{self, Write as _};

use crate::error::SyntaxError;
use crate::kind::SyntaxKind;
use crate::lexer::Token;
use crate::text::TextRange;

/// An index into a [`SyntaxTree`]'s node arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

/// A child of a node: either a nested node or a leaf token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    Node(NodeId),
    Token(Token),
}

#[derive(Debug, Clone)]
struct NodeData {
    kind: SyntaxKind,
    range: TextRange,
    children: Vec<Element>,
}

/// A parsed GDScript file: the source text plus its tree and diagnostics.
#[derive(Debug, Clone)]
pub struct SyntaxTree {
    source: String,
    nodes: Vec<NodeData>,
    root: NodeId,
    errors: Vec<SyntaxError>,
}

impl SyntaxTree {
    /// The source text this tree was built from.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.source
    }

    /// The outermost node, always [`SyntaxKind::SourceFile`].
    #[must_use]
    pub fn root(&self) -> SyntaxNode<'_> {
        SyntaxNode {
            tree: self,
            id: self.root,
        }
    }

    /// Diagnostics collected while lexing and parsing.
    ///
    /// A non-empty list does not mean the tree is unusable — it is still
    /// complete and lossless, with the unparseable regions wrapped in
    /// [`SyntaxKind::Error`] nodes.
    #[must_use]
    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    /// Whether parsing found any problems.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Resolve a node id obtained from this tree.
    #[must_use]
    pub fn node(&self, id: NodeId) -> SyntaxNode<'_> {
        SyntaxNode { tree: self, id }
    }

    fn data(&self, id: NodeId) -> &NodeData {
        &self.nodes[id.0 as usize]
    }
}

/// A borrowed handle to one node in a [`SyntaxTree`].
#[derive(Debug, Clone, Copy)]
pub struct SyntaxNode<'a> {
    tree: &'a SyntaxTree,
    id: NodeId,
}

impl<'a> SyntaxNode<'a> {
    #[must_use]
    pub fn id(self) -> NodeId {
        self.id
    }

    /// The tree this node belongs to.
    ///
    /// Needed to resolve the [`NodeId`]s handed out by [`Self::children`],
    /// which is how a caller walks nodes and tokens together in source order.
    #[must_use]
    pub fn tree(self) -> &'a SyntaxTree {
        self.tree
    }

    #[must_use]
    pub fn kind(self) -> SyntaxKind {
        self.tree.data(self.id).kind
    }

    #[must_use]
    pub fn range(self) -> TextRange {
        self.tree.data(self.id).range
    }

    /// The exact source text this node covers, trivia included.
    #[must_use]
    pub fn text(self) -> &'a str {
        self.range().slice(self.tree.text())
    }

    /// Direct children, nodes and tokens interleaved in source order.
    pub fn children(self) -> impl Iterator<Item = Element> + 'a {
        self.tree.data(self.id).children.iter().copied()
    }

    /// Direct child nodes, skipping tokens.
    pub fn child_nodes(self) -> impl Iterator<Item = SyntaxNode<'a>> + 'a {
        let tree = self.tree;
        self.children().filter_map(move |element| match element {
            Element::Node(id) => Some(SyntaxNode { tree, id }),
            Element::Token(_) => None,
        })
    }

    /// Direct child tokens, skipping nodes.
    pub fn child_tokens(self) -> impl Iterator<Item = Token> + 'a {
        self.children().filter_map(|element| match element {
            Element::Token(token) => Some(token),
            Element::Node(_) => None,
        })
    }

    /// The first direct child node of the given kind.
    #[must_use]
    pub fn child_node_of(self, kind: SyntaxKind) -> Option<SyntaxNode<'a>> {
        self.child_nodes().find(|node| node.kind() == kind)
    }

    /// The first direct child token of the given kind.
    #[must_use]
    pub fn child_token_of(self, kind: SyntaxKind) -> Option<Token> {
        self.child_tokens().find(|token| token.kind == kind)
    }

    /// Every node in this subtree, parents before children.
    #[must_use]
    pub fn descendants(self) -> Descendants<'a> {
        Descendants { stack: vec![self] }
    }
}

/// Pre-order iterator over a subtree. See [`SyntaxNode::descendants`].
#[derive(Debug)]
pub struct Descendants<'a> {
    stack: Vec<SyntaxNode<'a>>,
}

impl<'a> Iterator for Descendants<'a> {
    type Item = SyntaxNode<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        // Push in reverse so children come back out in source order.
        let children: Vec<_> = node.child_nodes().collect();
        self.stack.extend(children.into_iter().rev());
        Some(node)
    }
}

impl fmt::Display for SyntaxTree {
    /// Renders the tree in the indented form used by `gdck parse --tree`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        write_node(&mut out, self.root(), 0)?;
        f.write_str(&out)
    }
}

fn write_node(out: &mut String, node: SyntaxNode<'_>, depth: usize) -> fmt::Result {
    let indent = "  ".repeat(depth);
    writeln!(out, "{indent}{:?}@{}", node.kind(), node.range())?;
    for element in node.children() {
        match element {
            Element::Node(id) => write_node(out, node.tree.node(id), depth + 1)?,
            Element::Token(token) => {
                let indent = "  ".repeat(depth + 1);
                let text = token.text(node.tree.text());
                if text.is_empty() {
                    writeln!(out, "{indent}{:?}@{}", token.kind, token.range)?;
                } else {
                    writeln!(out, "{indent}{:?}@{} {:?}", token.kind, token.range, text)?;
                }
            }
        }
    }
    Ok(())
}

// -- Builder ----------------------------------------------------------------

/// A position in the child buffer that a node can later be opened at.
///
/// Needed because an expression parser only discovers it is looking at a binary
/// expression *after* parsing the left operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint(usize);

/// Builds a [`SyntaxTree`] as the parser walks the token stream.
///
/// Children accumulate in one flat buffer; closing a node drains its slice out
/// of that buffer into the arena. This keeps [`TreeBuilder::checkpoint`] O(1),
/// which matters because the Pratt expression parser takes one per operand.
#[derive(Debug)]
pub struct TreeBuilder {
    nodes: Vec<NodeData>,
    children: Vec<Element>,
    stack: Vec<(SyntaxKind, usize)>,
    /// Where the last token ended, so empty nodes still get a sensible position.
    last_offset: u32,
}

impl TreeBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            children: Vec::new(),
            stack: Vec::new(),
            last_offset: 0,
        }
    }

    pub fn start_node(&mut self, kind: SyntaxKind) {
        debug_assert!(kind.is_node(), "{kind:?} is a token, not a node");
        self.stack.push((kind, self.children.len()));
    }

    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint(self.children.len())
    }

    /// Open a node that retroactively contains everything added since
    /// `checkpoint`.
    pub fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        debug_assert!(kind.is_node(), "{kind:?} is a token, not a node");
        debug_assert!(
            checkpoint.0 <= self.children.len(),
            "checkpoint outlived its buffer"
        );
        self.stack.push((kind, checkpoint.0));
    }

    pub fn token(&mut self, token: Token) {
        debug_assert!(token.kind.is_token(), "{:?} is a node", token.kind);
        self.last_offset = token.range.end();
        self.children.push(Element::Token(token));
    }

    pub fn finish_node(&mut self) {
        let (kind, start) = self.stack.pop().expect("finish_node without start_node");
        let children: Vec<Element> = self.children.drain(start..).collect();

        let range = children
            .iter()
            .map(|element| match element {
                Element::Token(token) => token.range,
                Element::Node(id) => self.nodes[id.0 as usize].range,
            })
            .reduce(TextRange::cover)
            .unwrap_or_else(|| TextRange::empty(self.last_offset));

        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(NodeData {
            kind,
            range,
            children,
        });
        self.children.push(Element::Node(id));
    }

    /// Close the builder, producing the finished tree.
    ///
    /// # Panics
    ///
    /// Panics if any node is still open, or if the builder produced anything
    /// other than exactly one root node.
    #[must_use]
    pub fn finish(mut self, source: String, errors: Vec<SyntaxError>) -> SyntaxTree {
        assert!(self.stack.is_empty(), "unclosed nodes remain");
        assert_eq!(self.children.len(), 1, "expected exactly one root node");

        let root = match self.children.pop().expect("checked non-empty") {
            Element::Node(id) => id,
            Element::Token(_) => panic!("root must be a node"),
        };

        SyntaxTree {
            source,
            nodes: self.nodes,
            root,
            errors,
        }
    }
}

impl Default for TreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(kind: SyntaxKind, start: u32, end: u32) -> Token {
        Token {
            kind,
            range: TextRange::new(start, end),
        }
    }

    #[test]
    fn builds_a_nested_tree() {
        let mut builder = TreeBuilder::new();
        builder.start_node(SyntaxKind::SourceFile);
        builder.start_node(SyntaxKind::PassStmt);
        builder.token(token(SyntaxKind::PassKw, 0, 4));
        builder.finish_node();
        builder.token(token(SyntaxKind::Newline, 4, 5));
        builder.finish_node();

        let tree = builder.finish("pass\n".to_string(), Vec::new());
        assert_eq!(tree.root().kind(), SyntaxKind::SourceFile);
        assert_eq!(tree.root().range(), TextRange::new(0, 5));
        assert_eq!(tree.text(), "pass\n");

        let stmt = tree.root().child_nodes().next().expect("one child node");
        assert_eq!(stmt.kind(), SyntaxKind::PassStmt);
        assert_eq!(stmt.text(), "pass");
    }

    #[test]
    fn checkpoint_wraps_already_added_children() {
        // Mirrors how the Pratt parser turns `a + b` into a BinaryExpr only
        // after `a` has already been emitted.
        let mut builder = TreeBuilder::new();
        builder.start_node(SyntaxKind::SourceFile);
        let checkpoint = builder.checkpoint();
        builder.token(token(SyntaxKind::Ident, 0, 1));
        builder.start_node_at(checkpoint, SyntaxKind::BinaryExpr);
        builder.token(token(SyntaxKind::Plus, 2, 3));
        builder.token(token(SyntaxKind::Ident, 4, 5));
        builder.finish_node();
        builder.finish_node();

        let tree = builder.finish("a + b".to_string(), Vec::new());
        let binary = tree.root().child_nodes().next().expect("binary expr");
        assert_eq!(binary.kind(), SyntaxKind::BinaryExpr);
        assert_eq!(binary.range(), TextRange::new(0, 5));
        assert_eq!(binary.child_tokens().count(), 3);
    }

    #[test]
    fn empty_nodes_get_a_position() {
        let mut builder = TreeBuilder::new();
        builder.start_node(SyntaxKind::SourceFile);
        builder.token(token(SyntaxKind::PassKw, 0, 4));
        builder.start_node(SyntaxKind::Block);
        builder.finish_node();
        builder.finish_node();

        let tree = builder.finish("pass".to_string(), Vec::new());
        let block = tree.root().child_nodes().next().expect("block");
        assert_eq!(block.range(), TextRange::empty(4));
        assert_eq!(block.text(), "");
    }

    #[test]
    fn descendants_walk_in_source_order() {
        let mut builder = TreeBuilder::new();
        builder.start_node(SyntaxKind::SourceFile);
        builder.start_node(SyntaxKind::VarDecl);
        builder.token(token(SyntaxKind::VarKw, 0, 3));
        builder.finish_node();
        builder.start_node(SyntaxKind::PassStmt);
        builder.token(token(SyntaxKind::PassKw, 4, 8));
        builder.finish_node();
        builder.finish_node();

        let tree = builder.finish("var pass".to_string(), Vec::new());
        let kinds: Vec<_> = tree.root().descendants().map(SyntaxNode::kind).collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::SourceFile,
                SyntaxKind::VarDecl,
                SyntaxKind::PassStmt
            ]
        );
    }
}
