//! Lowering the concrete syntax tree to a [`Doc`].
//!
//! Everything the style guide says about *layout* is decided here, in terms of
//! where a break may happen and how far continuation lines indent. What the
//! renderer does with those choices is its own business.
//!
//! Two of the guide's rules shape most of this module:
//!
//! * Continuation lines take **two** indent levels, so they cannot be mistaken
//!   for the block that follows. Arrays, dictionaries and enums are the stated
//!   exception and take one.
//! * There is **one statement per line**, so a body written as `if x: pass` is
//!   expanded. Lambdas are the exception, being expressions rather than
//!   statements.

use std::cell::RefCell;
use std::collections::HashSet;

#[allow(clippy::enum_glob_use)]
use gdck_syntax::SyntaxKind::*;
use gdck_syntax::{SyntaxKind, SyntaxNode, SyntaxTree, Token};

use crate::doc::{Doc, join};
use crate::literal::{normalize_number, normalize_string};
use crate::trivia::{Leading, Trivia, first_significant, last_significant};

/// Indent levels for a continuation line, per the style guide.
const CONTINUATION_INDENT: u8 = 2;
/// Indent levels inside an array, dictionary or enum, the stated exception.
const COLLECTION_INDENT: u8 = 1;
/// How many `.name` steps a chain needs before breaking it is worth doing.
///
/// One or two are a plain member access, which reads worse split across lines
/// than left long. Beyond that the chain is doing enough to be worth the
/// parentheses that breaking it requires.
const MIN_CHAIN_SEGMENTS: usize = 3;

/// Where a run of members or statements sits, which decides how many blank
/// lines surround a definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The file itself. Definitions get two blank lines around them.
    File,
    /// An inner class body. Definitions get one.
    Class,
    /// A function body.
    Function,
}

/// Lowers a syntax tree to a [`Doc`].
///
/// Deliberately unaware of the format settings: lowering says where a break
/// *may* go, and rendering decides which ones are taken, from the line length
/// and the indent style. Keeping the settings out of here is what stops a
/// width-dependent decision from being baked into the document.
pub(crate) struct Lowerer<'a> {
    tree: &'a SyntaxTree,
    trivia: &'a Trivia,
    /// Trailing comments already emitted, by anchor offset.
    ///
    /// A comment on the last line of a block is anchored on a token that is
    /// also the last token of the block, of the function holding it, and of
    /// every construct up to the file. Without this, each of them would emit
    /// it again.
    emitted_trailing: RefCell<HashSet<u32>>,
}

impl<'a> Lowerer<'a> {
    pub(crate) fn new(tree: &'a SyntaxTree, trivia: &'a Trivia) -> Self {
        Self {
            tree,
            trivia,
            emitted_trailing: RefCell::new(HashSet::new()),
        }
    }

    /// The comment following `token` on its own line, emitted at most once.
    ///
    /// Returns `None` when there is nothing to emit, which callers inside
    /// brackets rely on: a trailing comment there has to force the brackets
    /// open, or the comma after it would be commented out.
    fn trailing_comment_text(&self, token: Option<Token>) -> Option<&'a str> {
        let token = token?;
        let offset = token.range.start();
        let comment = self.trivia.trailing_at(offset)?;
        if !self.emitted_trailing.borrow_mut().insert(offset) {
            return None;
        }
        Some(comment)
    }

    /// A trailing comment as it appears at the end of a line.
    fn trailing_comment_of(&self, token: Option<Token>) -> Option<Doc> {
        let comment = self.trailing_comment_text(token)?;
        // One space before an inline comment. The guide states no rule, but
        // its own example writes `print("Example") # Short comment.` and the
        // guide's samples are what this project treats as normative. gdformat
        // uses two, so output differs from it here.
        Some(Doc::text(format!(" {comment}")))
    }

    /// A trailing comment, forcing every enclosing group to break.
    ///
    /// A comment runs to the end of its line, so anything that would otherwise
    /// have been rendered after it on the same line — a closing bracket, the
    /// next element — has to move down or it gets commented out.
    fn trailing_comment(&self, token: Option<Token>) -> Doc {
        self.trailing_comment_of(token)
            .map_or_else(Doc::nil, |comment| {
                Doc::concat(vec![comment, Doc::break_parent()])
            })
    }

    /// Comments written on their own lines in front of `node`.
    fn leading_comments_of(&self, node: SyntaxNode<'a>) -> Vec<String> {
        first_significant(node)
            .map(|token| self.trivia.leading_at(token.range.start()))
            .map(|leading| {
                leading
                    .comments
                    .into_iter()
                    .map(|comment| comment.text)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Comments written on their own lines in front of a token.
    fn leading_comments_at(&self, token: Option<Token>) -> Vec<String> {
        token
            .map(|token| self.trivia.leading_at(token.range.start()))
            .map(|leading| {
                leading
                    .comments
                    .into_iter()
                    .map(|comment| comment.text)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The comment after the `:` that opens a body, which belongs to the
    /// header line rather than to the first statement inside.
    fn colon_comment(&self, node: SyntaxNode<'a>) -> Doc {
        self.trailing_comment(node.child_token_of(Colon))
    }

    fn text(&self, token: Token) -> &'a str {
        token.text(self.tree.text())
    }

    // -- File ---------------------------------------------------------------

    pub(crate) fn source_file(&self, root: SyntaxNode<'a>) -> Doc {
        let members = child_nodes(root);
        let mut parts = vec![self.sequence(&members, Scope::File)];

        // Comments after the last declaration hang off the end-of-file marker.
        if let Some(eof) = node_token(root, Eof) {
            let trailing = self.trivia.leading_at(eof.range.start());
            if !trailing.comments.is_empty() {
                if !members.is_empty() {
                    parts.push(Doc::hard_line());
                    for _ in 0..blank_run(trailing.comments[0].blank_lines_before) {
                        parts.push(Doc::hard_line());
                    }
                }
                parts.push(comment_run(&trailing, false));
            }
        }

        // The guide asks for exactly one line feed at the end of the file.
        parts.push(Doc::hard_line());
        Doc::concat(parts)
    }

    /// A run of members or statements, with their comments and blank lines.
    fn sequence(&self, items: &[SyntaxNode<'a>], scope: Scope) -> Doc {
        let mut parts = Vec::new();
        let mut previous: Option<SyntaxNode<'a>> = None;

        for item in items {
            let leading = first_significant(*item)
                .map(|token| self.trivia.leading_at(token.range.start()))
                .unwrap_or_default();

            if let Some(previous) = previous {
                parts.push(Doc::hard_line());
                for _ in 0..blank_lines_before(previous, *item, &leading, scope) {
                    parts.push(Doc::hard_line());
                }
            }

            if !leading.comments.is_empty() {
                parts.push(comment_run(&leading, true));
            }
            parts.push(self.member(*item, scope));

            parts.push(self.trailing_comment(last_significant(*item)));

            previous = Some(*item);
        }

        Doc::concat(parts)
    }

    // -- Members and statements ---------------------------------------------

    fn member(&self, node: SyntaxNode<'a>, scope: Scope) -> Doc {
        match node.kind() {
            Annotation => self.annotation(node),
            ClassNameDecl => self.class_name_decl(node),
            ExtendsDecl => self.extends_decl(node),
            SignalDecl => self.signal_decl(node),
            EnumDecl => self.enum_decl(node),
            ConstDecl | VarDecl => self.var_like_decl(node),
            FuncDecl => self.func_decl(node),
            ClassDecl => self.class_decl(node),

            ExprStmt => self.optional_parens(self.first_expr(node)),
            AssignStmt => self.assign_stmt(node),
            IfStmt => self.if_stmt(node),
            WhileStmt => self.while_stmt(node),
            ForStmt => self.for_stmt(node),
            MatchStmt => self.match_stmt(node),
            ReturnStmt => self.return_stmt(node),
            AssertStmt => self.assert_stmt(node),
            PassStmt => Doc::text("pass"),
            BreakStmt => Doc::text("break"),
            ContinueStmt => Doc::text("continue"),
            BreakpointStmt => Doc::text("breakpoint"),

            // Reached only if a caller ignored `has_errors`; reproducing the
            // source verbatim is the least destructive thing available.
            Error => Doc::text(node.text().trim().to_string()),

            _ => {
                let _ = scope;
                self.expr(node)
            }
        }
    }

    /// Annotations attached to a declaration, each followed by a separator.
    ///
    /// A function's annotations take a line each and a variable's stay beside
    /// it, which is how the Godot documentation writes them throughout:
    /// `@rpc("any_peer")` above the `func`, `@export_range(0, 10) var lives`
    /// beside the `var`.
    ///
    /// `@abstract` is the exception. It reads as a modifier rather than as a
    /// declaration about the one below it, and both the style guide's
    /// `@abstract class MyNode extends Node:` and the language reference's
    /// `@abstract func draw()` write it inline.
    fn attached_annotations(&self, node: SyntaxNode<'a>) -> Doc {
        let annotations: Vec<SyntaxNode<'a>> = node
            .child_nodes()
            .filter(|child| child.kind() == Annotation)
            .collect();
        if annotations.is_empty() {
            return Doc::nil();
        }
        let mut parts = Vec::new();
        for (index, annotation) in annotations.iter().enumerate() {
            // The first annotation's own leading comments were emitted by the
            // enclosing sequence, since it is the declaration's first token.
            if index > 0 {
                for comment in self.leading_comments_of(*annotation) {
                    parts.push(Doc::text(comment));
                    parts.push(Doc::hard_line());
                }
            }
            parts.push(self.annotation(*annotation));
            let own_line =
                node.kind() == FuncDecl && self.annotation_name(*annotation) != "abstract";
            match self.trailing_comment_of(last_significant(*annotation)) {
                // `@onready # why` has to end its line, so the declaration it
                // modifies moves to the next one.
                Some(comment) => {
                    parts.push(comment);
                    parts.push(Doc::hard_line());
                }
                None if own_line => parts.push(Doc::hard_line()),
                None => parts.push(Doc::text(" ")),
            }
        }
        // Comments between the last annotation and the `var` or `func` it
        // modifies sit in front of that keyword.
        let keyword = tokens(node).first().copied();
        let between = self.leading_comments_at(keyword);
        if !between.is_empty() {
            // Undo the separating space; the comments need their own lines.
            if matches!(parts.last(), Some(last) if last.is_space()) {
                parts.pop();
                parts.push(Doc::hard_line());
            }
            for comment in between {
                parts.push(Doc::text(comment));
                parts.push(Doc::hard_line());
            }
        }
        Doc::concat(parts)
    }

    fn annotation_name(&self, node: SyntaxNode<'a>) -> &'a str {
        tokens(node)
            .iter()
            .find(|token| token.kind.is_ident_like())
            .map_or("", |token| self.text(*token))
    }

    fn annotation(&self, node: SyntaxNode<'a>) -> Doc {
        let name = self.annotation_name(node);
        let mut parts = vec![Doc::text(format!("@{name}"))];
        if let Some(args) = node.child_node_of(ArgList) {
            parts.push(self.arg_list(args));
        }
        Doc::concat(parts)
    }

    /// `class_name Name`, with any `extends` moved to its own line.
    ///
    /// The guide's class-declaration example puts `class_name` and `extends` on
    /// separate lines, and explicitly contrasts that with inner classes, which
    /// "use single-line declarations". Splitting here is what makes the two
    /// forms come out as the guide writes them.
    fn class_name_decl(&self, node: SyntaxNode<'a>) -> Doc {
        let name = tokens(node)
            .into_iter()
            .find(|token| token.kind == Ident)
            .map_or(String::new(), |token| self.text(token).to_string());
        let mut parts = vec![Doc::text(format!("class_name {name}"))];
        if let Some(extends) = node.child_node_of(ExtendsDecl) {
            parts.push(Doc::hard_line());
            parts.push(self.extends_decl(extends));
        }
        Doc::concat(parts)
    }

    fn extends_decl(&self, node: SyntaxNode<'a>) -> Doc {
        let tokens = tokens(node);
        // Skip the `extends` keyword itself; the rest is a type path.
        Doc::text(format!("extends {}", self.type_text(&tokens[1..])))
    }

    fn signal_decl(&self, node: SyntaxNode<'a>) -> Doc {
        let name = tokens(node)
            .into_iter()
            .find(|token| token.kind == Ident)
            .map_or(String::new(), |token| self.text(token).to_string());
        let mut parts = vec![
            self.attached_annotations(node),
            Doc::text(format!("signal {name}")),
        ];
        if let Some(params) = node.child_node_of(ParamList) {
            parts.push(self.param_list(params));
        }
        Doc::concat(parts)
    }

    fn enum_decl(&self, node: SyntaxNode<'a>) -> Doc {
        let name = tokens(node)
            .into_iter()
            .find(|token| token.kind == Ident)
            .map(|token| format!("{} ", self.text(token)))
            .unwrap_or_default();

        let mut parts = vec![
            self.attached_annotations(node),
            Doc::text(format!("enum {name}")),
        ];

        let Some(body) = node.child_node_of(EnumBody) else {
            return Doc::concat(parts);
        };
        let variants = child_nodes(body);
        let docs = variants
            .iter()
            .map(|variant| self.enum_variant(*variant))
            .collect();
        // The guide is explicit: "Write enums with each item on its own line."
        // So this one breaks regardless of how short it would be.
        parts.push(self.collection(
            body,
            LBrace,
            &variants,
            docs,
            CollectionStyle::collection().always_expanded(),
        ));
        Doc::concat(parts)
    }

    fn enum_variant(&self, node: SyntaxNode<'a>) -> Doc {
        let tokens = tokens(node);
        let name = tokens
            .iter()
            .find(|token| token.kind == Ident)
            .map_or("", |token| self.text(*token));
        match child_nodes(node).first() {
            Some(value) => Doc::concat(vec![Doc::text(format!("{name} = ")), self.expr(*value)]),
            None => Doc::text(name.to_string()),
        }
    }

    /// `var`, `static var` and `const`, which share a shape.
    fn var_like_decl(&self, node: SyntaxNode<'a>) -> Doc {
        let tokens = tokens(node);
        let keyword = if tokens.iter().any(|token| token.kind == ConstKw) {
            "const"
        } else if tokens.iter().any(|token| token.kind == StaticKw) {
            "static var"
        } else {
            "var"
        };
        let name = tokens
            .iter()
            .find(|token| token.kind == Ident)
            .map_or("", |token| self.text(*token));

        let mut parts = vec![
            self.attached_annotations(node),
            Doc::text(format!("{keyword} {name}")),
        ];
        if let Some(hint) = node.child_node_of(TypeHint) {
            parts.push(self.type_hint(hint));
        }
        if let Some(initializer) = node.child_node_of(Initializer) {
            parts.push(self.initializer(initializer, true));
        }
        if let Some(accessors) = node.child_node_of(Accessors) {
            parts.push(self.accessors(accessors));
        }
        Doc::concat(parts)
    }

    fn type_hint(&self, node: SyntaxNode<'a>) -> Doc {
        let tokens = tokens(node);
        Doc::text(format!(": {}", self.type_text(&tokens[1..])))
    }

    /// `= value` or `:= value`.
    ///
    /// `spaced` is false for a parameter default, which the guide's own
    /// examples write tight (`func take_damage(amount, effect=null)`).
    fn initializer(&self, node: SyntaxNode<'a>, spaced: bool) -> Doc {
        let inferred = tokens(node)
            .first()
            .is_some_and(|token| matches!(token.kind, ColonEq | Colon));
        let operator = if inferred {
            " := "
        } else if spaced {
            " = "
        } else {
            "="
        };
        let value = child_nodes(node)
            .first()
            .map_or_else(Doc::nil, |value| self.optional_parens(Some(*value)));
        Doc::concat(vec![Doc::text(operator), value])
    }

    fn accessors(&self, node: SyntaxNode<'a>) -> Doc {
        let clauses = child_nodes(node);
        // The source form is kept: an accessor written as an indented block
        // stays one, and `set = f, get = g` stays on the line.
        let block_form = node.child_tokens().any(|token| token.kind == Indent);

        // Godot has two property forms and decides which one this is from the
        // first accessor alone: `= method` makes it the setget form, anything
        // else the inline-block form. Only the setget form is comma-separated,
        // and there the comma is what lets the parser go on to the second
        // accessor at all — without it the block is over, and the second is
        // rejected with "Expected end of indented block for property". A
        // block-bodied accessor takes none, since nothing ever looks for one.
        let setget = clauses
            .first()
            .is_some_and(|clause| clause.child_node_of(Block).is_none());

        if block_form {
            let mut parts = vec![Doc::text(":"), self.colon_comment(node)];
            let mut inner = Vec::new();
            let count = clauses.len();
            for (index, clause) in clauses.iter().enumerate() {
                for comment in self.leading_comments_of(*clause) {
                    inner.push(Doc::hard_line());
                    inner.push(Doc::text(comment));
                }
                inner.push(Doc::hard_line());
                inner.push(self.accessor(*clause));
                if setget && index + 1 < count {
                    inner.push(Doc::text(","));
                }
                inner.push(self.trailing_comment(last_significant(*clause)));
            }
            parts.push(Doc::indent(Doc::concat(inner)));
            return Doc::concat(parts);
        }

        let rendered: Vec<Doc> = clauses
            .iter()
            .map(|clause| self.accessor(*clause))
            .collect();
        Doc::concat(vec![Doc::text(": "), join(rendered, &Doc::text(", "))])
    }

    fn accessor(&self, node: SyntaxNode<'a>) -> Doc {
        let keyword = if node.kind() == Setter { "set" } else { "get" };
        let mut parts = vec![Doc::text(keyword)];
        if let Some(params) = node.child_node_of(ParamList) {
            parts.push(self.param_list(params));
        }
        match node.child_node_of(Block) {
            Some(block) => {
                parts.push(Doc::text(":"));
                parts.push(self.colon_comment(node));
                parts.push(self.block(block, Scope::Function));
            }
            None => {
                // `set = method_name`
                if let Some(target) = child_nodes(node).first() {
                    parts.push(Doc::text(" = "));
                    parts.push(self.expr(*target));
                }
            }
        }
        Doc::concat(parts)
    }

    fn func_decl(&self, node: SyntaxNode<'a>) -> Doc {
        let tokens = tokens(node);
        let is_static = tokens.iter().any(|token| token.kind == StaticKw);
        let name = tokens
            .iter()
            .find(|token| token.kind == Ident)
            .map_or("", |token| self.text(*token));

        let mut parts = vec![self.attached_annotations(node)];
        if is_static {
            parts.push(Doc::text("static "));
        }
        parts.push(Doc::text(format!("func {name}")));
        if let Some(params) = node.child_node_of(ParamList) {
            parts.push(self.param_list(params));
        }
        if let Some(return_type) = node.child_node_of(ReturnType) {
            parts.push(self.return_type(return_type));
        }
        // An `@abstract` function has no body at all.
        if let Some(block) = node.child_node_of(Block) {
            parts.push(Doc::text(":"));
            parts.push(self.colon_comment(node));
            parts.push(self.block(block, Scope::Function));
        }
        Doc::concat(parts)
    }

    fn return_type(&self, node: SyntaxNode<'a>) -> Doc {
        let tokens = tokens(node);
        Doc::text(format!(" -> {}", self.type_text(&tokens[1..])))
    }

    /// `class Name extends Parent:` — one line, as the guide requires.
    ///
    /// GDScript accepts the parent either on the declaration line or as the
    /// first statement of the body, and the guide asks for the former: "For
    /// inner classes, use single-line declarations". So a body-level `extends`
    /// is hoisted into the header.
    fn class_decl(&self, node: SyntaxNode<'a>) -> Doc {
        let name = tokens(node)
            .into_iter()
            .find(|token| token.kind == Ident)
            .map_or(String::new(), |token| self.text(token).to_string());

        let block = node.child_node_of(Block);
        let mut members = block.map(|block| child_nodes(block)).unwrap_or_default();

        let mut extends = node.child_node_of(ExtendsDecl);
        // `members.len() > 1` because a class whose only member is the
        // `extends` would be left with an empty body, which does not parse.
        if extends.is_none() && members.len() > 1 {
            let body_level = members
                .iter()
                .position(|member| member.kind() == ExtendsDecl && self.has_no_comments(*member));
            if let Some(index) = body_level {
                extends = Some(members.remove(index));
            }
        }

        let mut parts = vec![
            self.attached_annotations(node),
            Doc::text(format!("class {name}")),
        ];
        if let Some(extends) = extends {
            parts.push(Doc::text(" "));
            parts.push(self.extends_decl(extends));
        }
        parts.push(Doc::text(":"));
        parts.push(self.colon_comment(node));
        parts.push(self.indented(&members, Scope::Class));
        Doc::concat(parts)
    }

    /// Whether a node has no comment attached in front of or behind it.
    ///
    /// Moving a declaration that carries a comment would move the comment too,
    /// or strand it; leaving such a case alone costs one unapplied style rule
    /// and avoids rewriting what someone wrote deliberately.
    fn has_no_comments(&self, node: SyntaxNode<'a>) -> bool {
        let leading_clean = first_significant(node).is_none_or(|token| {
            self.trivia
                .leading_at(token.range.start())
                .comments
                .is_empty()
        });
        let trailing_clean = last_significant(node)
            .is_none_or(|token| self.trivia.trailing_at(token.range.start()).is_none());
        leading_clean && trailing_clean
    }

    /// An indented body. Always expanded, since the guide asks for one
    /// statement per line.
    fn block(&self, node: SyntaxNode<'a>, scope: Scope) -> Doc {
        let statements = child_nodes(node);
        self.indented(&statements, scope)
    }

    fn indented(&self, statements: &[SyntaxNode<'a>], scope: Scope) -> Doc {
        if statements.is_empty() {
            return Doc::nil();
        }
        Doc::indent(Doc::concat(vec![
            Doc::hard_line(),
            self.sequence(statements, scope),
        ]))
    }

    fn param_list(&self, node: SyntaxNode<'a>) -> Doc {
        let params = child_nodes(node);
        let docs = params.iter().map(|param| self.param(*param)).collect();
        self.collection(
            node,
            LParen,
            &params,
            docs,
            CollectionStyle::continuation().expanded(self.expanded(node)),
        )
    }

    fn param(&self, node: SyntaxNode<'a>) -> Doc {
        let tokens = tokens(node);
        let variadic = tokens.iter().any(|token| token.kind == Ellipsis);
        let name = tokens
            .iter()
            .find(|token| token.kind == Ident)
            .map_or("", |token| self.text(*token));

        let mut parts = Vec::new();
        parts.push(Doc::text(if variadic {
            format!("...{name}")
        } else {
            name.to_string()
        }));
        let typed = node.child_node_of(TypeHint);
        if let Some(hint) = typed {
            parts.push(self.type_hint(hint));
        }
        if let Some(initializer) = node.child_node_of(Initializer) {
            // A default is written tight unless a type hint has already made
            // the declaration wordy, which is the convention the guide's
            // examples use and the one Python settled on for the same reason.
            parts.push(self.initializer(initializer, typed.is_some()));
        }
        Doc::concat(parts)
    }

    fn arg_list(&self, node: SyntaxNode<'a>) -> Doc {
        let args = child_nodes(node);
        let docs = args.iter().map(|arg| self.bracketed_expr(*arg)).collect();
        self.collection(
            node,
            LParen,
            &args,
            docs,
            CollectionStyle::continuation().expanded(self.expanded(node)),
        )
    }

    /// A bracketed, comma-separated run.
    ///
    /// `trailing_comma` marks the collection literals the guide calls out —
    /// arrays, dictionaries and enums — which also take a single indent level
    /// rather than a continuation's two.
    fn collection(
        &self,
        owner: SyntaxNode<'a>,
        open_kind: SyntaxKind,
        items: &[SyntaxNode<'a>],
        docs: Vec<Doc>,
        style: CollectionStyle,
    ) -> Doc {
        let (open, close_kind, close) = match open_kind {
            LParen => ("(", RParen, ")"),
            LBracket => ("[", RBracket, "]"),
            _ => ("{", RBrace, "}"),
        };
        let open_token = owner.child_token_of(open_kind);
        let close_token = owner.child_token_of(close_kind);

        // A comment straight after the opening bracket, and any on their own
        // lines before the closing one, belong to the brackets rather than to
        // any element.
        let after_open = self.trailing_comment_of(open_token);
        let before_close = self.leading_comments_at(close_token);

        if items.is_empty() && after_open.is_none() && before_close.is_empty() {
            return Doc::text(format!("{open}{close}"));
        }

        let indent = if style.collection_indent {
            COLLECTION_INDENT
        } else {
            CONTINUATION_INDENT
        };

        let mut body = Vec::new();
        let mut forced = style.expanded;
        if let Some(comment) = after_open {
            body.push(comment);
            forced = true;
        }

        // A lambda body is the one place inside brackets where Godot still
        // tracks indentation, and it stops doing so again at whatever ends the
        // lambda. A trailing comma ends it on the body's own last line; without
        // one, the closing bracket's line is the first line after the body, and
        // Godot requires that line to sit at the enclosing statement's indent
        // rather than at a continuation's. Rather than special-case the
        // closer's indentation, end the lambda where a collection already
        // would — with a comma.
        let trailing_comma =
            style.trailing_comma || items.last().is_some_and(|item| breaking_lambda(*item));

        let count = items.len();
        for (index, (item, doc)) in items.iter().zip(docs).enumerate() {
            // The separator comes first, then any comments on their own lines,
            // then the item. Emitting comments before the separator would run
            // the comment and the item together on one line.
            if index == 0 {
                body.push(if style.spaced {
                    Doc::if_break(Doc::soft_line(), Doc::text(" "))
                } else {
                    Doc::soft_line()
                });
            }
            for comment in self.leading_comments_of(*item) {
                body.push(Doc::text(comment));
                body.push(Doc::hard_line());
                forced = true;
            }
            body.push(doc);

            let last = index + 1 == count;
            if last {
                if trailing_comma {
                    body.push(Doc::if_break(Doc::text(","), Doc::nil()));
                }
            } else {
                body.push(Doc::text(","));
            }
            if let Some(comment) = self.trailing_comment_of(last_significant(*item)) {
                body.push(comment);
                forced = true;
            }
            if !last {
                body.push(Doc::line());
            }
        }

        for comment in before_close {
            body.push(Doc::hard_line());
            body.push(Doc::text(comment));
            forced = true;
        }

        let mut parts = vec![Doc::text(open.to_string())];
        if forced {
            parts.push(Doc::break_parent());
        }
        parts.push(Doc::indent_by(indent, Doc::concat(body)));
        parts.push(if style.spaced {
            Doc::if_break(Doc::soft_line(), Doc::text(" "))
        } else {
            Doc::soft_line()
        });
        parts.push(Doc::text(close.to_string()));
        Doc::group(Doc::concat(parts))
    }

    /// Whether the author wrote this construct across more than one line.
    ///
    /// The style guide presents `var array = [1, 2, 3]` and the same array
    /// spread over four lines as *both* good, and marks an 83-column `if` as
    /// bad for not being wrapped. Neither is derivable from a column limit, so
    /// the author's own choice is what settles it.
    fn expanded(&self, node: SyntaxNode<'a>) -> bool {
        // Measured between the node's own first and last tokens. `node.text()`
        // would include the trivia in front of it, so an element sitting on
        // its own line inside a broken list would look expanded itself.
        let (Some(first), Some(last)) = (first_significant(node), last_significant(node)) else {
            return false;
        };
        let start = first.range.start() as usize;
        let end = last.range.end() as usize;
        self.tree.text()[start..end].contains('\n')
    }

    /// Peel off parentheses that group nothing.
    ///
    /// Safe wherever brackets or a statement boundary already delimit the
    /// expression, which is every caller below: an argument, a collection
    /// element, a subscript index, or the whole right-hand side of a
    /// statement. Elsewhere — an operand, a cast, the base of an attribute —
    /// the parentheses carry precedence and are left alone.
    fn unwrap_parens(&self, node: SyntaxNode<'a>) -> SyntaxNode<'a> {
        let mut current = node;
        while current.kind() == ParenExpr {
            let Some(inner) = self.first_expr(current) else {
                break;
            };
            current = inner;
        }
        current
    }

    /// An expression in a bracket-delimited position.
    ///
    /// Breaking is always legal here, since the enclosing bracket is open, so
    /// no parentheses need adding — only redundant ones removing.
    fn bracketed_expr(&self, node: SyntaxNode<'a>) -> Doc {
        self.expr(self.unwrap_parens(node))
    }

    // -- Statements ---------------------------------------------------------

    #[allow(clippy::unused_self)]
    fn first_expr(&self, node: SyntaxNode<'a>) -> Option<SyntaxNode<'a>> {
        child_nodes(node).into_iter().next()
    }

    fn assign_stmt(&self, node: SyntaxNode<'a>) -> Doc {
        let children = child_nodes(node);
        let operator = tokens(node)
            .into_iter()
            .find(|token| is_assign_op(token.kind))
            .map_or("=", |token| self.text(token));

        let target = children
            .first()
            .map_or_else(Doc::nil, |target| self.member(*target, Scope::Function));
        let value = children
            .get(1)
            .map_or_else(Doc::nil, |value| self.optional_parens(Some(*value)));
        Doc::concat(vec![target, Doc::text(format!(" {operator} ")), value])
    }

    fn if_stmt(&self, node: SyntaxNode<'a>) -> Doc {
        let mut parts = Vec::new();
        let children = child_nodes(node);
        let mut children = children.into_iter();

        let condition = children.next();
        let block = children.next();
        parts.push(Doc::text("if "));
        parts.push(self.condition(condition));
        parts.push(Doc::text(":"));
        parts.push(self.colon_comment(node));
        if let Some(block) = block {
            parts.push(self.block(block, Scope::Function));
        }

        for clause in children {
            parts.push(Doc::hard_line());
            for comment in self.leading_comments_of(clause) {
                parts.push(Doc::text(comment));
                parts.push(Doc::hard_line());
            }
            match clause.kind() {
                ElifClause => {
                    let inner = child_nodes(clause);
                    parts.push(Doc::text("elif "));
                    parts.push(self.condition(inner.first().copied()));
                    parts.push(Doc::text(":"));
                    parts.push(self.colon_comment(clause));
                    if let Some(block) = inner.get(1) {
                        parts.push(self.block(*block, Scope::Function));
                    }
                }
                ElseClause => {
                    parts.push(Doc::text("else:"));
                    parts.push(self.colon_comment(clause));
                    if let Some(block) = child_nodes(clause).first() {
                        parts.push(self.block(*block, Scope::Function));
                    }
                }
                _ => parts.push(self.member(clause, Scope::Function)),
            }
        }
        Doc::concat(parts)
    }

    /// A condition, which may need parentheses added to wrap.
    fn condition(&self, node: Option<SyntaxNode<'a>>) -> Doc {
        self.optional_parens(node)
    }

    fn while_stmt(&self, node: SyntaxNode<'a>) -> Doc {
        let children = child_nodes(node);
        let mut parts = vec![
            Doc::text("while "),
            self.condition(children.first().copied()),
            Doc::text(":"),
            self.colon_comment(node),
        ];
        if let Some(block) = children.get(1) {
            parts.push(self.block(*block, Scope::Function));
        }
        Doc::concat(parts)
    }

    fn for_stmt(&self, node: SyntaxNode<'a>) -> Doc {
        let tokens = tokens(node);
        let name = tokens
            .iter()
            .find(|token| token.kind == Ident)
            .map_or("", |token| self.text(*token));

        let children = child_nodes(node);
        let mut parts = vec![Doc::text(format!("for {name}"))];
        let mut rest = children.as_slice();
        if let Some(hint) = children.first().filter(|node| node.kind() == TypeHint) {
            parts.push(self.type_hint(*hint));
            rest = &children[1..];
        }
        parts.push(Doc::text(" in "));
        parts.push(self.optional_parens(rest.first().copied()));
        parts.push(Doc::text(":"));
        parts.push(self.colon_comment(node));
        if let Some(block) = rest.get(1) {
            parts.push(self.block(*block, Scope::Function));
        }
        Doc::concat(parts)
    }

    fn match_stmt(&self, node: SyntaxNode<'a>) -> Doc {
        let children = child_nodes(node);
        let mut parts = vec![
            Doc::text("match "),
            self.optional_parens(children.first().copied()),
            Doc::text(":"),
            self.colon_comment(node),
        ];
        let arms: Vec<SyntaxNode<'a>> = children
            .into_iter()
            .filter(|child| child.kind() == MatchArm)
            .collect();
        if !arms.is_empty() {
            parts.push(Doc::indent(Doc::concat(vec![
                Doc::hard_line(),
                self.sequence(&arms, Scope::Function),
            ])));
        }
        Doc::concat(parts)
    }

    fn match_arm(&self, node: SyntaxNode<'a>) -> Doc {
        let children = child_nodes(node);
        let patterns: Vec<Doc> = children
            .iter()
            .filter(|child| !matches!(child.kind(), Block | MatchGuard))
            .map(|pattern| Doc::flat(self.expr(*pattern)))
            .collect();

        // Patterns sit between `match` and `:` with no brackets around them,
        // so there is nowhere a line break would be legal. A pattern too long
        // for the limit stays long, and the linter reports it.
        let mut parts = vec![Doc::flat(join(patterns, &Doc::text(", ")))];
        if let Some(guard) = children.iter().find(|child| child.kind() == MatchGuard) {
            parts.push(Doc::text(" when "));
            parts.push(self.optional_parens(self.first_expr(*guard)));
        }
        parts.push(Doc::text(":"));
        parts.push(self.colon_comment(node));
        if let Some(block) = children.iter().find(|child| child.kind() == Block) {
            parts.push(self.block(*block, Scope::Function));
        }
        Doc::concat(parts)
    }

    fn return_stmt(&self, node: SyntaxNode<'a>) -> Doc {
        match self.first_expr(node) {
            Some(value) => Doc::concat(vec![
                Doc::text("return "),
                self.optional_parens(Some(value)),
            ]),
            None => Doc::text("return"),
        }
    }

    fn assert_stmt(&self, node: SyntaxNode<'a>) -> Doc {
        let args = node
            .child_node_of(ArgList)
            .map_or_else(|| Doc::text("()"), |args| self.arg_list(args));
        Doc::concat(vec![Doc::text("assert"), args])
    }

    // -- Expressions --------------------------------------------------------

    /// Render an expression, adding parentheses if it has to wrap.
    ///
    /// GDScript has no way to continue a line without brackets, so an
    /// expression that overflows in a statement position needs a pair added.
    /// One that can already break inside itself — a call, an array, a
    /// dictionary — is left to do that instead, since wrapping those in another
    /// layer of parentheses only adds noise.
    fn optional_parens(&self, node: Option<SyntaxNode<'a>>) -> Doc {
        let Some(node) = node else {
            return Doc::nil();
        };

        // An expression already in parentheses is unwrapped here and given
        // them back only if it turns out to need them, which is what removes
        // the redundant pair from `if (is_colliding()):`.
        let inner = self.unwrap_parens(node);

        if self.breaks_on_its_own(inner) {
            return self.expr(inner);
        }

        // A condition the author already wrapped in parentheses across several
        // lines stays wrapped, however short it would be on one.
        let keep_open = if node.kind() == ParenExpr && self.expanded(node) {
            Doc::break_parent()
        } else {
            Doc::nil()
        };

        let body = self.chain_parts(inner).unwrap_or_else(|| self.expr(inner));

        Doc::group(Doc::concat(vec![
            keep_open,
            Doc::if_break(Doc::text("("), Doc::nil()),
            Doc::indent_by(
                CONTINUATION_INDENT,
                Doc::concat(vec![Doc::soft_line(), body]),
            ),
            Doc::soft_line(),
            Doc::if_break(Doc::text(")"), Doc::nil()),
        ]))
    }

    #[allow(clippy::too_many_lines)]
    fn expr(&self, node: SyntaxNode<'a>) -> Doc {
        match node.kind() {
            Literal => {
                let Some(token) = tokens(node).into_iter().next() else {
                    return Doc::nil();
                };
                // `literal` rather than `text`: a triple-quoted string carries
                // its own newlines and must be written out untouched.
                Doc::literal(match token.kind {
                    Int | Float => normalize_number(self.text(token)),
                    Str | StringName | NodePath | GetNode | UniqueNode => {
                        normalize_string(self.text(token))
                    }
                    _ => self.text(token).to_string(),
                })
            }

            NameRef => {
                let tokens = tokens(node);
                // `var name` inside a match pattern binds a capture.
                if tokens.first().is_some_and(|token| token.kind == VarKw) {
                    let name = tokens.get(1).map_or("", |token| self.text(*token));
                    return Doc::text(format!("var {name}"));
                }
                Doc::text(
                    tokens
                        .first()
                        .map_or("", |token| self.text(*token))
                        .to_string(),
                )
            }

            ParenExpr => {
                let Some(inner) = self.first_expr(node) else {
                    return Doc::text("()");
                };
                let body = self.chain_parts(inner).unwrap_or_else(|| self.expr(inner));
                // Parentheses around a lambda block have to close on the
                // body's own last line. Godot goes back to ignoring
                // indentation at whatever ends the lambda, and until then a
                // dedent has to land on one of the enclosing statement's
                // levels — which a continuation indent is not. Here the
                // closing paren is what ends it, so it cannot be given a line
                // of its own; an argument list ends it with a comma instead,
                // in `collection`.
                if breaking_lambda(inner) {
                    return Doc::concat(vec![Doc::text("("), body, Doc::text(")")]);
                }
                Doc::group(Doc::concat(vec![
                    Doc::text("("),
                    if self.expanded(node) {
                        Doc::break_parent()
                    } else {
                        Doc::nil()
                    },
                    Doc::indent_by(
                        CONTINUATION_INDENT,
                        Doc::concat(vec![Doc::soft_line(), body]),
                    ),
                    Doc::soft_line(),
                    Doc::text(")"),
                ]))
            }

            ArrayExpr => {
                let items = child_nodes(node);
                let docs = items
                    .iter()
                    .map(|item| self.bracketed_expr(*item))
                    .collect();
                self.collection(
                    node,
                    LBracket,
                    &items,
                    docs,
                    CollectionStyle::collection().expanded(self.expanded(node)),
                )
            }

            DictExpr => self.dict_expr(node),
            DictEntry => self.dict_entry(node),

            BinaryExpr => Doc::group(self.binary_expr(node)),
            TernaryExpr => Doc::group(self.ternary_expr(node)),

            UnaryExpr => {
                let operator = tokens(node)
                    .into_iter()
                    .next()
                    .map_or("", |token| self.text(token));
                // `not` is a word and needs separating; the symbols do not.
                let separator = if operator == "not" { " " } else { "" };
                let operand = self
                    .first_expr(node)
                    .map_or_else(Doc::nil, |operand| self.expr(operand));
                Doc::concat(vec![Doc::text(format!("{operator}{separator}")), operand])
            }

            AwaitExpr => Doc::concat(vec![
                Doc::text("await "),
                self.first_expr(node)
                    .map_or_else(Doc::nil, |inner| self.expr(inner)),
            ]),

            CastExpr => {
                let tokens = tokens(node);
                let type_tokens = tokens
                    .iter()
                    .position(|token| token.kind == AsKw)
                    .map_or(&tokens[..], |index| &tokens[index + 1..]);
                Doc::concat(vec![
                    self.first_expr(node)
                        .map_or_else(Doc::nil, |inner| self.expr(inner)),
                    Doc::text(format!(" as {}", self.type_text(type_tokens))),
                ])
            }

            PreloadExpr => {
                let args = node
                    .child_node_of(ArgList)
                    .map_or_else(|| Doc::text("()"), |args| self.arg_list(args));
                Doc::concat(vec![Doc::text("preload"), args])
            }

            CallExpr | SubscriptExpr | AttributeExpr => {
                let Some((base, ops)) = self.chain(node) else {
                    return Doc::text(node.text().trim().to_string());
                };
                if segment_count(&ops) >= MIN_CHAIN_SEGMENTS {
                    // Inside brackets this group may break on its own; at
                    // statement level `optional_parens` will have taken the
                    // ungrouped form instead, so the break gets parentheses.
                    Doc::group(self.render_chain(base, &ops, true))
                } else {
                    self.render_chain(base, &ops, false)
                }
            }

            LambdaExpr => self.lambda_expr(node),

            MatchArm => self.match_arm(node),

            Error => Doc::text(node.text().trim().to_string()),

            // Statements can appear where an expression is expected inside a
            // block; `member` knows how to render them.
            _ => self.member(node, Scope::Function),
        }
    }

    fn dict_expr(&self, node: SyntaxNode<'a>) -> Doc {
        let entries = child_nodes(node);
        let docs = entries
            .iter()
            .map(|entry| self.dict_entry(*entry))
            .collect();
        // A dictionary on one line gets a space inside each brace, which the
        // guide asks for so that `{ }` is not mistaken for `[ ]` at a glance.
        self.collection(
            node,
            LBrace,
            &entries,
            docs,
            CollectionStyle::collection()
                .expanded(self.expanded(node))
                .with_inner_spaces(),
        )
    }

    fn dict_entry(&self, node: SyntaxNode<'a>) -> Doc {
        let tokens = tokens(node);
        let children = child_nodes(node);

        // The Lua-style `{ key = value }` form keeps its key as a bare token.
        if tokens.iter().any(|token| token.kind == Eq) {
            let key = tokens.first().map_or("", |token| self.text(*token));
            let value = children
                .first()
                .map_or_else(Doc::nil, |value| self.bracketed_expr(*value));
            // A comment between the `=` and its value has nowhere to sit on
            // the line: putting it after the value would swallow the comma
            // that follows. It moves above the entry instead.
            // Taken as bare text, not as a trailing comment: it is going on a
            // line of its own, where the separating space would be indentation
            // that a second formatting pass would then strip.
            let stray = match self.trailing_comment_text(node.child_token_of(Eq)) {
                Some(comment) => Doc::concat(vec![Doc::text(comment), Doc::hard_line()]),
                None => Doc::nil(),
            };
            return Doc::concat(vec![stray, Doc::text(format!("{key} = ")), value]);
        }

        // A rest marker in a pattern stands alone.
        if children.is_empty() {
            let text = tokens
                .first()
                .map_or(String::new(), |token| self.text(*token).to_string());
            return Doc::text(text);
        }

        let key = self.bracketed_expr(children[0]);
        match children.get(1) {
            Some(value) => Doc::concat(vec![key, Doc::text(": "), self.bracketed_expr(*value)]),
            // A dictionary pattern may test for a key alone.
            None => key,
        }
    }

    /// A run of binary operators at the same precedence, breaking together.
    ///
    /// Flattening matters for readability: `a and b and c` should either be one
    /// line or three, never a staircase of nested groups each deciding for
    /// itself.
    fn binary_expr(&self, node: SyntaxNode<'a>) -> Doc {
        let operator_kind = tokens(node).into_iter().next().map(|token| token.kind);

        let mut operands = Vec::new();
        let mut operators = Vec::new();
        self.flatten_binary(node, operator_kind, &mut operands, &mut operators);

        let mut parts = vec![operands.remove(0)];
        for (operator, operand) in operators.into_iter().zip(operands) {
            // The break goes before the operator, so a continuation line opens
            // with `and` rather than trailing one, as the guide's example does.
            parts.push(Doc::line());
            parts.push(Doc::text(format!("{operator} ")));
            parts.push(operand);
        }
        // No indent here: the enclosing parentheses, real or added by
        // `optional_parens`, already supply the continuation's two levels.
        Doc::concat(parts)
    }

    /// An operator chain without its own group, for a caller that has one.
    ///
    /// When a wrapped expression breaks, the guide breaks it at *every*
    /// operator rather than filling lines. Sharing the enclosing parentheses'
    /// group is what produces that: one decision, applied to the whole chain.
    fn chain_parts(&self, node: SyntaxNode<'a>) -> Option<Doc> {
        match node.kind() {
            BinaryExpr => Some(self.binary_expr(node)),
            TernaryExpr => Some(self.ternary_expr(node)),
            CallExpr | SubscriptExpr | AttributeExpr => {
                let (base, ops) = self.chain(node)?;
                (segment_count(&ops) >= MIN_CHAIN_SEGMENTS)
                    .then(|| self.render_chain(base, &ops, true))
            }
            _ => None,
        }
    }

    fn flatten_binary(
        &self,
        node: SyntaxNode<'a>,
        operator_kind: Option<SyntaxKind>,
        operands: &mut Vec<Doc>,
        operators: &mut Vec<String>,
    ) {
        let children = child_nodes(node);
        let tokens = tokens(node);
        let operator = self.operator_text(&tokens);

        // Only a left operand using the *same* operator continues the chain.
        // Testing this node's operator instead would flatten `a > b and c`
        // into a single run and break it between `a` and `> b`.
        let left_continues = |left: &SyntaxNode<'a>| {
            left.kind() == BinaryExpr
                && crate::lower::tokens(*left).first().map(|token| token.kind) == operator_kind
        };

        match children.first() {
            Some(left) if left_continues(left) => {
                self.flatten_binary(*left, operator_kind, operands, operators);
            }
            Some(left) => operands.push(self.expr(*left)),
            None => {}
        }
        operators.push(operator);
        if let Some(right) = children.get(1) {
            operands.push(self.expr(*right));
        }
    }

    /// The operator's spelling, joining the two words of `not in`.
    fn operator_text(&self, tokens: &[Token]) -> String {
        let mut words: Vec<&str> = tokens
            .iter()
            .filter(|token| !token.kind.is_node())
            .map(|token| self.text(*token))
            .collect();
        words.truncate(2);
        match words.as_slice() {
            [first, second] if *first == "not" && *second == "in" => "not in".to_string(),
            [first, ..] => (*first).to_string(),
            [] => String::new(),
        }
    }

    /// A chain of `x if c else y`, breaking before every `else` together.
    fn ternary_expr(&self, node: SyntaxNode<'a>) -> Doc {
        let mut parts = Vec::new();
        let mut current = node;
        loop {
            let children = child_nodes(current);
            let (Some(value), Some(condition)) = (children.first(), children.get(1)) else {
                break;
            };
            parts.push(self.expr(*value));
            parts.push(Doc::text(" if "));
            parts.push(self.expr(*condition));

            match children.get(2) {
                // A ternary in the else position continues the same chain.
                Some(other) if other.kind() == TernaryExpr => {
                    parts.push(Doc::line());
                    parts.push(Doc::text("else "));
                    current = *other;
                }
                Some(other) => {
                    parts.push(Doc::line());
                    parts.push(Doc::text("else "));
                    parts.push(self.expr(*other));
                    break;
                }
                None => break,
            }
        }
        Doc::concat(parts)
    }

    /// Split a postfix chain into the value it starts from and the operations
    /// applied to it.
    ///
    /// The parser nests `a.b().c[0]` as a left-leaning spine of three
    /// different node kinds. Flattening it is what lets the chain break only
    /// before each `.`, with each call and subscript staying attached to the
    /// name it belongs to.
    fn chain(&self, node: SyntaxNode<'a>) -> Option<(SyntaxNode<'a>, Vec<ChainOp<'a>>)> {
        let mut ops = Vec::new();
        let mut current = node;
        loop {
            match current.kind() {
                AttributeExpr => {
                    let name = tokens(current)
                        .into_iter()
                        .find(|token| token.kind.is_ident_like())
                        .map_or(String::new(), |token| self.text(token).to_string());
                    ops.push(ChainOp::Attr(name));
                }
                CallExpr => ops.push(ChainOp::Call(current.child_node_of(ArgList)?)),
                SubscriptExpr => ops.push(ChainOp::Index(*child_nodes(current).get(1)?)),
                _ => break,
            }
            current = *child_nodes(current).first()?;
        }
        ops.reverse();
        Some((current, ops))
    }

    /// Render a chain, breaking before each `.` when `broken` is set.
    ///
    /// No indentation is added: a chain only ever breaks inside brackets,
    /// which have already supplied the continuation's two levels.
    fn render_chain(&self, base: SyntaxNode<'a>, ops: &[ChainOp<'a>], broken: bool) -> Doc {
        let mut parts = vec![self.expr(base)];
        for op in ops {
            match op {
                ChainOp::Attr(name) => {
                    if broken {
                        parts.push(Doc::soft_line());
                    }
                    parts.push(Doc::text(format!(".{name}")));
                }
                ChainOp::Call(args) => parts.push(self.arg_list(*args)),
                ChainOp::Index(index) => parts.push(Doc::concat(vec![
                    Doc::text("["),
                    self.bracketed_expr(*index),
                    Doc::text("]"),
                ])),
            }
        }
        Doc::concat(parts)
    }

    /// Whether an expression can absorb a line break inside itself, so it does
    /// not need parentheses added around it.
    fn breaks_on_its_own(&self, node: SyntaxNode<'a>) -> bool {
        match node.kind() {
            ArrayExpr | DictExpr | ParenExpr | LambdaExpr | PreloadExpr => true,
            CallExpr | SubscriptExpr | AttributeExpr => {
                let Some((base, ops)) = self.chain(node) else {
                    return false;
                };
                // A chain long enough to break needs brackets to break inside,
                // so it has to be the one thing that *does* ask for them.
                if segment_count(&ops) >= MIN_CHAIN_SEGMENTS {
                    return false;
                }
                // Otherwise it breaks wherever a call in it does, which is why
                // a trailing `.is_cool()` does not stop `foo([...]).is_cool()`
                // from breaking inside its array.
                let has_call_args = ops.iter().any(|op| match op {
                    ChainOp::Call(args) => args.child_nodes().next().is_some(),
                    _ => false,
                });
                has_call_args || self.breaks_on_its_own(base)
            }
            _ => false,
        }
    }

    fn lambda_expr(&self, node: SyntaxNode<'a>) -> Doc {
        let name = tokens(node)
            .into_iter()
            .find(|token| token.kind == Ident)
            .map(|token| format!(" {}", self.text(token)))
            .unwrap_or_default();

        let mut parts = vec![Doc::text(format!("func{name}"))];
        if let Some(params) = node.child_node_of(ParamList) {
            parts.push(self.param_list(params));
        }
        if let Some(return_type) = node.child_node_of(ReturnType) {
            parts.push(self.return_type(return_type));
        }
        parts.push(Doc::text(":"));
        parts.push(self.colon_comment(node));

        if let Some(block) = node.child_node_of(Block) {
            // A lambda is an expression, so the one-statement-per-line rule
            // does not force it open. One written on a single line stays there.
            let inline = !block.child_tokens().any(|token| token.kind == Indent);
            let statements = child_nodes(block);
            if inline && statements.len() == 1 {
                parts.push(Doc::text(" "));
                parts.push(self.member(statements[0], Scope::Function));
            } else {
                parts.push(self.block(block, Scope::Function));
            }
        }
        Doc::concat(parts)
    }

    // -- Types --------------------------------------------------------------

    /// Render a run of tokens making up a type path.
    ///
    /// Types never wrap, so this produces a plain string: `Array[int]`,
    /// `A.B.C`, `"res://base.gd".Inner`.
    fn type_text(&self, tokens: &[Token]) -> String {
        let mut out = String::new();
        for token in tokens {
            match token.kind {
                Comma => out.push_str(", "),
                _ => out.push_str(self.text(*token)),
            }
        }
        out
    }
}

/// How a bracketed run of items is laid out.
///
// Four independent layout switches; bundling them into an enum would need one
// variant per combination and say less than the names do.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy)]
struct CollectionStyle {
    /// Arrays, dictionaries and enums take one indent level and a trailing
    /// comma; everything else is a continuation and takes two with none.
    collection_indent: bool,
    trailing_comma: bool,
    /// A space inside each brace when the whole thing is on one line.
    spaced: bool,
    expanded: bool,
}

impl CollectionStyle {
    /// Arrays, dictionaries and enums: the guide's stated exception.
    fn collection() -> Self {
        Self {
            collection_indent: true,
            trailing_comma: true,
            spaced: false,
            expanded: false,
        }
    }

    /// Argument and parameter lists, which are ordinary continuation lines.
    fn continuation() -> Self {
        Self {
            collection_indent: false,
            trailing_comma: false,
            spaced: false,
            expanded: false,
        }
    }

    fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    fn always_expanded(mut self) -> Self {
        self.expanded = true;
        self
    }

    fn with_inner_spaces(mut self) -> Self {
        self.spaced = true;
        self
    }
}

/// One step of a postfix chain. See [`Lowerer::chain`].
#[derive(Debug)]
enum ChainOp<'a> {
    /// `.name`, the only place a chain may break.
    Attr(String),
    Call(SyntaxNode<'a>),
    Index(SyntaxNode<'a>),
}

/// Whether this expression is a lambda that [`Lowerer::lambda_expr`] will
/// render as an indented block rather than on one line.
///
/// The two have to agree: a comma is emitted for exactly the shape that puts a
/// block in the middle of a bracketed construct.
fn breaking_lambda(node: SyntaxNode<'_>) -> bool {
    if node.kind() != LambdaExpr {
        return false;
    }
    let Some(block) = node.child_node_of(Block) else {
        return false;
    };
    let inline = !block.child_tokens().any(|token| token.kind == Indent);
    !(inline && child_nodes(block).len() == 1)
}

/// How many `.name` steps a chain has, which is how many break points it
/// would gain.
fn segment_count(ops: &[ChainOp<'_>]) -> usize {
    ops.iter()
        .filter(|op| matches!(op, ChainOp::Attr(_)))
        .count()
}

/// Comments on their own lines, followed by a break onto the thing they
/// document when `attached` is set.
fn comment_run(leading: &Leading, attached: bool) -> Doc {
    let mut parts = Vec::new();
    for (index, comment) in leading.comments.iter().enumerate() {
        if index > 0 {
            parts.push(Doc::hard_line());
            for _ in 0..blank_run(comment.blank_lines_before) {
                parts.push(Doc::hard_line());
            }
        }
        parts.push(Doc::text(comment.text.clone()));
    }
    if attached {
        parts.push(Doc::hard_line());
        for _ in 0..blank_run(leading.blank_lines_before) {
            parts.push(Doc::hard_line());
        }
    }
    Doc::concat(parts)
}
/// How many blank lines go between two consecutive items.
fn blank_lines_before(
    previous: SyntaxNode<'_>,
    item: SyntaxNode<'_>,
    leading: &Leading,
    scope: Scope,
) -> usize {
    // A definition's leading comments belong to it, so the mandated spacing
    // goes before the comments rather than between them and the definition.
    let requested = leading
        .comments
        .first()
        .map_or(leading.blank_lines_before, |first| first.blank_lines_before);

    if is_definition(previous) || is_definition(item) {
        return match scope {
            Scope::File => 2,
            Scope::Class | Scope::Function => 1,
        };
    }
    blank_run(requested)
}

/// Significant direct child tokens, in source order.
fn tokens(node: SyntaxNode<'_>) -> Vec<Token> {
    node.child_tokens()
        .filter(|token| !token.kind.is_trivia() && !matches!(token.kind, Indent | Dedent | Eof))
        .collect()
}

fn child_nodes(node: SyntaxNode<'_>) -> Vec<SyntaxNode<'_>> {
    node.child_nodes().collect()
}

/// Definitions are the members the guide asks to surround with blank lines.
fn is_definition(node: SyntaxNode<'_>) -> bool {
    matches!(node.kind(), FuncDecl | ClassDecl)
}

/// Collapse a run of blank lines to at most one.
fn blank_run(requested: usize) -> usize {
    requested.min(1)
}

fn node_token(node: SyntaxNode<'_>, kind: SyntaxKind) -> Option<Token> {
    node.child_token_of(kind)
}

fn is_assign_op(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        Eq | PlusEq
            | MinusEq
            | StarEq
            | StarStarEq
            | SlashEq
            | PercentEq
            | AmpEq
            | PipeEq
            | CaretEq
            | ShlEq
            | ShrEq
    )
}
