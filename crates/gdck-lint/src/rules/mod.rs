//! The rule implementations, grouped by what they look at.
//!
//! Each group walks the parts of the tree it cares about rather than sharing
//! one dispatching traversal. A `.gd` file is small and a traversal is a walk
//! over a contiguous arena, so the saving from a single pass would not pay for
//! the coupling: a group can be read, tested and changed without knowing what
//! the others match on.

mod correctness;
mod design;
mod naming;
mod order;
mod style;
mod whitespace;

use std::collections::HashSet;

use gdck_config::LintConfig;
use gdck_syntax::{SyntaxKind, SyntaxNode, SyntaxTree, TextRange, Token};

use crate::{Diagnostic, Fix};

/// What every rule is given.
#[derive(Debug)]
pub(crate) struct Context<'a> {
    pub(crate) tree: &'a SyntaxTree,
    pub(crate) source: &'a str,
    pub(crate) config: &'a LintConfig,
    /// The final path component, e.g. `player.gd`, when it is known.
    pub(crate) file_name: Option<&'a str>,
}

impl<'a> Context<'a> {
    pub(crate) fn root(&self) -> SyntaxNode<'a> {
        self.tree.root()
    }

    pub(crate) fn token_text(&self, token: Token) -> &'a str {
        token.text(self.source)
    }
}

/// Where diagnostics accumulate.
///
/// Holding the disabled set here rather than checking it in each rule means a
/// rule cannot forget to, at the cost of doing the analysis for a rule nobody
/// will see. That analysis is a tree walk over a file, which is not worth
/// arranging the code around.
#[derive(Debug)]
pub(crate) struct Sink {
    disabled: HashSet<&'static str>,
    out: Vec<Diagnostic>,
}

impl Sink {
    pub(crate) fn new(disabled: HashSet<&'static str>) -> Self {
        Self {
            disabled,
            out: Vec::new(),
        }
    }

    pub(crate) fn report(
        &mut self,
        rule: &'static str,
        range: TextRange,
        message: impl Into<String>,
    ) {
        self.push(rule, range, message.into(), None);
    }

    pub(crate) fn report_with_fix(
        &mut self,
        rule: &'static str,
        range: TextRange,
        message: impl Into<String>,
        fix: Fix,
    ) {
        self.push(rule, range, message.into(), Some(fix));
    }

    fn push(&mut self, rule: &'static str, range: TextRange, message: String, fix: Option<Fix>) {
        let entry = crate::rule(rule);
        debug_assert!(entry.is_some(), "{rule} is not in the rule catalogue");
        let Some(entry) = entry else { return };
        if self.disabled.contains(entry.name) {
            return;
        }
        debug_assert!(
            fix.is_none() || entry.fixable,
            "{rule} produced a fix but is not marked fixable"
        );
        self.out.push(Diagnostic {
            rule: entry.name,
            severity: entry.severity,
            range,
            message,
            fix,
        });
    }

    pub(crate) fn finish(self) -> Vec<Diagnostic> {
        self.out
    }
}

pub(crate) use order::reorder;

pub(crate) fn run(context: &Context<'_>, sink: &mut Sink) {
    naming::check(context, sink);
    whitespace::check(context, sink);
    style::check(context, sink);
    design::check(context, sink);
    correctness::check(context, sink);
    order::check(context, sink);
}

// -- Shared tree helpers ----------------------------------------------------

/// The name a declaration introduces.
///
/// Every declaration puts its name in the first `Ident` token it owns
/// directly: annotations, type hints and initialisers are all child *nodes*,
/// so they cannot be mistaken for it.
pub(crate) fn name_token(node: SyntaxNode<'_>) -> Option<Token> {
    node.child_token_of(SyntaxKind::Ident)
}

/// Every class body in the file: the file itself, then each inner class.
///
/// A GDScript file *is* a class, which is why its declarations obey the same
/// ordering and counting rules as an inner class's.
pub(crate) fn class_bodies(root: SyntaxNode<'_>) -> Vec<SyntaxNode<'_>> {
    let mut bodies = vec![root];
    for node in root.descendants() {
        if node.kind() == SyntaxKind::ClassDecl {
            if let Some(block) = node.child_node_of(SyntaxKind::Block) {
                bodies.push(block);
            }
        }
    }
    bodies
}

/// Strip any layers of parentheses, giving the expression they group.
pub(crate) fn unwrap_parens(mut node: SyntaxNode<'_>) -> SyntaxNode<'_> {
    while node.kind() == SyntaxKind::ParenExpr {
        match node.child_nodes().next() {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}

/// The callee name of a call, when it is a plain identifier.
///
/// `load("x")` gives `load`; `resources.load("x")` gives nothing, since the
/// method being called is not the global function.
pub(crate) fn callee_name<'a>(call: SyntaxNode<'a>, source: &'a str) -> Option<&'a str> {
    let callee = call.child_nodes().next()?;
    if callee.kind() != SyntaxKind::NameRef {
        return None;
    }
    Some(callee.child_token_of(SyntaxKind::Ident)?.text(source))
}

/// Every token in a subtree, trivia included, in source order.
pub(crate) fn all_tokens(node: SyntaxNode<'_>) -> Vec<Token> {
    let mut out = Vec::new();
    collect_tokens(node, &mut out);
    out
}

/// The tokens of a subtree that carry meaning, with trivia and the zero-width
/// block markers dropped.
pub(crate) fn significant_tokens(node: SyntaxNode<'_>) -> Vec<Token> {
    let mut out = all_tokens(node);
    out.retain(|token| {
        !token.kind.is_trivia()
            && !matches!(
                token.kind,
                SyntaxKind::Indent | SyntaxKind::Dedent | SyntaxKind::Eof
            )
    });
    out
}

fn collect_tokens(node: SyntaxNode<'_>, out: &mut Vec<Token>) {
    for element in node.children() {
        match element {
            gdck_syntax::Element::Token(token) => out.push(token),
            gdck_syntax::Element::Node(id) => collect_tokens(node.tree().node(id), out),
        }
    }
}

/// The span a declaration occupies on the page, ignoring the trivia the parser
/// attached in front of it.
///
/// A node's own range starts at the newline that ended the previous line, so
/// pointing a diagnostic at it would point at the wrong line.
pub(crate) fn significant_range(node: SyntaxNode<'_>) -> TextRange {
    let tokens = significant_tokens(node);
    match (tokens.first(), tokens.last()) {
        (Some(first), Some(last)) => first.range.cover(last.range),
        _ => node.range(),
    }
}
