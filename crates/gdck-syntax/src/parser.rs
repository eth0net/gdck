//! A recursive-descent parser with a Pratt loop for expressions.
//!
//! The parser never fails. Input it cannot fit into the grammar is wrapped in
//! [`SyntaxKind::Error`] nodes and recorded as a diagnostic, so the resulting
//! tree always covers the whole file. That matters for editor use, where most
//! keystrokes leave the buffer temporarily unparseable, and it is what lets
//! `gdck` report several problems in one pass instead of stopping at the first.
//!
//! Trivia is emitted into whichever node is open when the parser looks ahead,
//! so a comment before a declaration becomes a sibling preceding it rather than
//! disappearing.

use crate::error::SyntaxError;
// The grammar rules below read far better as `self.at(FuncKw)` than as
// `self.at(SyntaxKind::FuncKw)`, and this module does nothing but grammar.
#[allow(clippy::enum_glob_use)]
use crate::kind::SyntaxKind::{self, *};
use crate::lexer::{Token, tokenize};
use crate::text::TextRange;
use crate::tree::{Checkpoint, SyntaxTree, TreeBuilder};

/// Parse GDScript source into a lossless tree.
///
/// Always returns a tree. Check [`SyntaxTree::errors`] for problems.
#[must_use]
pub fn parse(source: &str) -> SyntaxTree {
    let lexed = tokenize(source);
    Parser {
        source,
        tokens: lexed.tokens,
        pos: 0,
        builder: TreeBuilder::new(),
        errors: lexed.errors,
        bracket_depth: 0,
        in_pattern: false,
        fuel: 0,
    }
    .run()
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    builder: TreeBuilder,
    errors: Vec<SyntaxError>,
    /// Nesting depth of `()`, `[]` and `{}`.
    ///
    /// Outside brackets a newline ends the statement, so the expression parser
    /// must stop at one; inside them a newline is just formatting and an
    /// expression may span as many lines as it likes.
    bracket_depth: u32,
    /// Set while parsing a `match` pattern, where `var name` bindings and `..`
    /// rest markers are legal in places an ordinary expression forbids them.
    in_pattern: bool,
    /// Guards against a rule that loops without consuming input.
    fuel: u32,
}

/// Tokens that can begin a class-level declaration; used to resynchronise.
const CLASS_MEMBER_START: &[SyntaxKind] = &[
    At,
    VarKw,
    ConstKw,
    FuncKw,
    ClassKw,
    ClassNameKw,
    ExtendsKw,
    SignalKw,
    EnumKw,
    StaticKw,
];

/// Tokens that can begin a statement; used to resynchronise inside a block.
const STATEMENT_START: &[SyntaxKind] = &[
    VarKw,
    ConstKw,
    IfKw,
    ElifKw,
    ElseKw,
    ForKw,
    WhileKw,
    MatchKw,
    ReturnKw,
    PassKw,
    BreakKw,
    ContinueKw,
    BreakpointKw,
    AssertKw,
    AwaitKw,
    FuncKw,
    ClassKw,
    SignalKw,
    EnumKw,
    StaticKw,
];

impl Parser<'_> {
    fn run(mut self) -> SyntaxTree {
        self.builder.start_node(SourceFile);
        while !self.at(Eof) {
            let before = self.pos;
            self.parse_class_member();
            while self.eat(Semicolon) {}
            self.ensure_progress(before, CLASS_MEMBER_START);
        }
        // Trailing trivia and the Eof marker still belong in the tree.
        self.skip_trivia();
        self.bump_raw();
        self.builder.finish_node();
        self.builder.finish(self.source.to_string(), self.errors)
    }

    // -- Class level --------------------------------------------------------

    fn parse_class_member(&mut self) {
        let checkpoint = self.builder.checkpoint();

        // Annotations bind to the declaration that follows, so parse them
        // first and let the declaration retroactively adopt them.
        let mut is_abstract = false;
        while self.at(At) {
            is_abstract |= self.parse_annotation();
        }

        match self.current() {
            // File-level annotations such as `@tool` and `@icon` precede these
            // but do not modify them, so they stay as siblings.
            ClassNameKw => self.parse_class_name(),
            ExtendsKw => self.parse_extends(),

            VarKw => self.parse_var_decl(checkpoint),
            ConstKw => self.parse_const_decl(checkpoint),
            SignalKw => self.parse_signal_decl(checkpoint),
            EnumKw => self.parse_enum_decl(checkpoint),
            FuncKw => self.parse_func_decl(checkpoint, is_abstract),
            ClassKw => self.parse_inner_class(checkpoint),

            StaticKw => match self.nth(1) {
                VarKw => self.parse_var_decl(checkpoint),
                FuncKw => self.parse_func_decl(checkpoint, is_abstract),
                _ => self.error_and_recover(
                    "expected `var` or `func` after `static`",
                    CLASS_MEMBER_START,
                ),
            },

            // A bare `pass` is a legal class body, standing in for members that
            // are not there yet.
            PassKw => self.simple_statement(PassStmt),

            // A bare string at class level is a docstring.
            Str => self.parse_expr_statement(),

            // Annotations with nothing to modify are legal on their own, as
            // `@tool` and `@icon` are.
            At | Eof | Dedent => {}

            _ => self.error_and_recover("expected a declaration", CLASS_MEMBER_START),
        }
    }

    /// `@name` or `@name(arg, ...)`. Returns whether this was `@abstract`.
    fn parse_annotation(&mut self) -> bool {
        self.builder.start_node(Annotation);
        self.bump(); // @
        let mut is_abstract = false;
        if self.current().is_ident_like() {
            is_abstract = self.current_text() == "abstract";
            self.bump();
        } else {
            self.error("expected an annotation name after `@`");
        }
        if self.at(LParen) {
            self.parse_arg_list();
        }
        self.builder.finish_node();
        is_abstract
    }

    /// `class_name Name [extends Base]`
    fn parse_class_name(&mut self) {
        self.builder.start_node(ClassNameDecl);
        self.bump(); // class_name
        self.expect(Ident, "expected a class name");
        if self.at(ExtendsKw) {
            self.parse_extends();
        }
        self.builder.finish_node();
    }

    /// `extends Base` or `extends "res://base.gd"`
    fn parse_extends(&mut self) {
        self.builder.start_node(ExtendsDecl);
        self.bump(); // extends
        if self.at(Str) {
            self.bump();
            // `extends "path.gd".Inner`
            while self.at(Dot) {
                self.bump();
                self.expect(Ident, "expected a name after `.`");
            }
        } else {
            self.parse_type();
        }
        self.builder.finish_node();
    }

    /// `signal name` or `signal name(a, b: int)`
    fn parse_signal_decl(&mut self, checkpoint: Checkpoint) {
        self.builder.start_node_at(checkpoint, SignalDecl);
        self.bump(); // signal
        self.expect(Ident, "expected a signal name");
        if self.at(LParen) {
            self.parse_param_list();
        }
        self.builder.finish_node();
    }

    /// `enum [Name] { A, B = 2, }`
    fn parse_enum_decl(&mut self, checkpoint: Checkpoint) {
        self.builder.start_node_at(checkpoint, EnumDecl);
        self.bump(); // enum
        if self.at(Ident) {
            self.bump();
        }
        if self.at(LBrace) {
            self.builder.start_node(EnumBody);
            self.bump(); // {
            self.enter_brackets();
            while !self.at(RBrace) && !self.at(Eof) {
                let before = self.pos;
                self.builder.start_node(EnumVariant);
                self.expect(Ident, "expected an enum member name");
                if self.eat(Eq) {
                    self.parse_expr();
                }
                self.builder.finish_node();
                if !self.eat(Comma) {
                    break;
                }
                self.ensure_progress(before, &[RBrace]);
            }
            self.expect(RBrace, "expected `}` to close the enum");
            self.leave_brackets();
            self.builder.finish_node();
        } else {
            self.error("expected `{` to open the enum body");
        }
        self.builder.finish_node();
    }

    /// `const NAME [: Type] = value`
    fn parse_const_decl(&mut self, checkpoint: Checkpoint) {
        self.builder.start_node_at(checkpoint, ConstDecl);
        self.bump(); // const
        self.expect(Ident, "expected a constant name");
        if !self.parse_type_and_initializer() {
            self.error("a constant must be initialised");
        }
        self.builder.finish_node();
    }

    /// `[static] var name [: Type] [= value] [: set/get]`
    fn parse_var_decl(&mut self, checkpoint: Checkpoint) {
        self.builder.start_node_at(checkpoint, VarDecl);
        self.eat(StaticKw);
        self.bump(); // var
        self.expect(Ident, "expected a variable name");
        self.parse_type_and_initializer();
        // Property accessors: `var x: set = f, get = g` or an indented block.
        if self.at(Colon) {
            self.parse_accessors();
        }
        self.builder.finish_node();
    }

    /// Parse `: Type`, `:= value`, `: Type = value`, `= value`, or nothing.
    ///
    /// Returns whether an initializer was present. The `:=` form is recorded as
    /// an [`Initializer`] holding the `:=` token, which is what lets the
    /// static-typing lint rules tell inferred from explicit declarations.
    fn parse_type_and_initializer(&mut self) -> bool {
        // `:=` and the equivalent `: =` written with a space.
        if self.at(ColonEq) || (self.at(Colon) && self.nth(1) == Eq) {
            self.builder.start_node(Initializer);
            self.bump(); // `:=` or `:`
            self.eat(Eq); // the `=` of a spaced `: =`
            self.parse_expr();
            self.builder.finish_node();
            return true;
        }

        // A bare `:` is a type hint only when a type name follows; otherwise it
        // opens an accessor clause, which is the caller's business. `set` and
        // `get` are identifiers, so `var p: set = f` needs telling apart from a
        // genuine type annotation by name.
        let names_accessor = matches!(self.nth_text(1), "set" | "get");
        if self.at(Colon) && matches!(self.nth(1), Ident | VoidKw) && !names_accessor {
            self.builder.start_node(TypeHint);
            self.bump(); // :
            self.parse_type();
            self.builder.finish_node();
        }

        if self.at(Eq) {
            self.parse_initializer();
            return true;
        }
        false
    }

    fn parse_initializer(&mut self) {
        self.builder.start_node(Initializer);
        self.bump(); // =
        self.parse_expr();
        self.builder.finish_node();
    }

    /// The `set`/`get` clauses attached to a `var`.
    fn parse_accessors(&mut self) {
        self.builder.start_node(Accessors);
        self.bump(); // :
        if self.at(Indent) {
            self.bump();
            while !self.at(Dedent) && !self.at(Eof) {
                let before = self.pos;
                self.parse_one_accessor();
                // `get = __get,` and `set = __set` may be comma-separated even
                // when written across several lines.
                self.eat(Comma);
                self.ensure_progress(before, &[Dedent]);
            }
            self.eat(Dedent);
        } else {
            loop {
                self.parse_one_accessor();
                if !self.eat(Comma) {
                    break;
                }
            }
        }
        self.builder.finish_node();
    }

    fn parse_one_accessor(&mut self) {
        // `set` and `get` are contextual keywords: everywhere else they are
        // ordinary identifiers, so they are matched by text rather than kind.
        if !self.at(Ident) {
            self.error_and_recover("expected `set` or `get`", &[Dedent, Comma]);
            return;
        }
        let node = match self.current_text() {
            "set" => Setter,
            "get" => Getter,
            _ => {
                self.error_and_recover("expected `set` or `get`", &[Dedent, Comma]);
                return;
            }
        };

        self.builder.start_node(node);
        self.bump(); // set / get

        if self.at(LParen) {
            // `set(value):` — an inline accessor body.
            self.parse_param_list();
        }
        if self.eat(Eq) {
            // `set = method_name`
            self.parse_expr();
        } else if self.eat(Colon) {
            self.parse_block();
        } else {
            self.error("expected `=` or `:` after the accessor");
        }
        self.builder.finish_node();
    }

    /// `[static] func name(params) [-> Type]: block`
    ///
    /// `allow_no_body` is set when an `@abstract` annotation preceded the
    /// declaration, since an abstract function is written without one.
    fn parse_func_decl(&mut self, checkpoint: Checkpoint, allow_no_body: bool) {
        self.builder.start_node_at(checkpoint, FuncDecl);
        self.eat(StaticKw);
        self.bump(); // func
        self.expect(Ident, "expected a function name");
        if self.at(LParen) {
            self.parse_param_list();
        } else {
            self.error("expected `(` to open the parameter list");
        }
        if self.at(Arrow) {
            self.builder.start_node(ReturnType);
            self.bump();
            self.parse_type();
            self.builder.finish_node();
        }
        if self.eat(Colon) {
            self.parse_block();
        } else if !allow_no_body {
            self.error("expected `:` to open the function body");
        }
        self.builder.finish_node();
    }

    /// `class Name [extends Base]: block`
    fn parse_inner_class(&mut self, checkpoint: Checkpoint) {
        self.builder.start_node_at(checkpoint, ClassDecl);
        self.bump(); // class
        self.expect(Ident, "expected a class name");
        if self.at(ExtendsKw) {
            self.parse_extends();
        }
        if self.eat(Colon) {
            self.parse_class_block();
        } else {
            self.error("expected `:` to open the class body");
        }
        self.builder.finish_node();
    }

    fn parse_class_block(&mut self) {
        self.builder.start_node(Block);
        if self.at(Indent) {
            self.bump();
            while !self.at(Dedent) && !self.at(Eof) {
                let before = self.pos;
                self.parse_class_member();
                while self.eat(Semicolon) {}
                self.ensure_progress(before, CLASS_MEMBER_START);
            }
            self.eat(Dedent);
        } else {
            self.parse_class_member();
        }
        self.builder.finish_node();
    }

    fn parse_param_list(&mut self) {
        self.builder.start_node(ParamList);
        self.bump(); // (
        self.enter_brackets();
        while !self.at(RParen) && !self.at(Eof) {
            let before = self.pos;
            self.builder.start_node(Param);
            // `...rest` collects the remaining arguments.
            self.eat(Ellipsis);
            self.expect(Ident, "expected a parameter name");
            self.parse_type_and_initializer();
            self.builder.finish_node();
            if !self.eat(Comma) {
                break;
            }
            self.ensure_progress(before, &[RParen]);
        }
        self.expect(RParen, "expected `)` to close the parameter list");
        self.leave_brackets();
        self.builder.finish_node();
    }

    fn parse_arg_list(&mut self) {
        self.builder.start_node(ArgList);
        self.bump(); // (
        self.enter_brackets();
        while !self.at(RParen) && !self.at(Eof) {
            let before = self.pos;
            self.parse_expr();
            if !self.eat(Comma) {
                break;
            }
            self.ensure_progress(before, &[RParen]);
        }
        self.expect(RParen, "expected `)` to close the argument list");
        self.leave_brackets();
        self.builder.finish_node();
    }

    /// `int`, `Vector2`, `A.B`, `Array[int]`, `void`
    fn parse_type(&mut self) {
        if self.at(VoidKw) {
            self.bump();
            return;
        }
        if !self.at(Ident) {
            self.error("expected a type name");
            return;
        }
        self.bump();
        while self.at(Dot) {
            self.bump();
            self.expect(Ident, "expected a name after `.`");
        }
        if self.at(LBracket) {
            self.bump();
            self.enter_brackets();
            while !self.at(RBracket) && !self.at(Eof) {
                let before = self.pos;
                self.parse_type();
                if !self.eat(Comma) {
                    break;
                }
                self.ensure_progress(before, &[RBracket]);
            }
            self.expect(RBracket, "expected `]` to close the type parameters");
            self.leave_brackets();
        }
    }

    // -- Statements ---------------------------------------------------------

    /// Parse a block body, either indented or inline after a `:`.
    fn parse_block(&mut self) {
        self.builder.start_node(Block);
        if self.at(Indent) {
            // An indented block is its own line-oriented world even when it
            // sits inside brackets, which is the case for a multi-line lambda
            // passed as an argument. Without this reset, statements in the body
            // would be glued together into one expression.
            let enclosing_brackets = std::mem::take(&mut self.bracket_depth);
            self.bump();
            while !self.at(Dedent) && !self.at(Eof) {
                let before = self.pos;
                self.parse_statement();
                // `a = 1; b = 2` on one line inside an indented block.
                while self.eat(Semicolon) {}
                self.ensure_progress(before, STATEMENT_START);
            }
            self.eat(Dedent);
            self.bracket_depth = enclosing_brackets;
        } else {
            // `if x: pass` — one or more statements on the same line.
            loop {
                self.parse_statement();
                if !self.eat(Semicolon) || self.newline_ahead() {
                    break;
                }
                if self.at(Eof) || self.at(Dedent) {
                    break;
                }
            }
        }
        self.builder.finish_node();
    }

    #[allow(clippy::too_many_lines)]
    fn parse_statement(&mut self) {
        match self.current() {
            PassKw => self.simple_statement(PassStmt),
            BreakKw => self.simple_statement(BreakStmt),
            ContinueKw => self.simple_statement(ContinueStmt),
            BreakpointKw => self.simple_statement(BreakpointStmt),

            ReturnKw => {
                self.builder.start_node(ReturnStmt);
                self.bump();
                if !self.at_statement_end() {
                    self.parse_expr();
                }
                self.builder.finish_node();
            }

            AssertKw => {
                self.builder.start_node(AssertStmt);
                self.bump();
                if self.at(LParen) {
                    self.parse_arg_list();
                } else {
                    self.error("expected `(` after `assert`");
                }
                self.builder.finish_node();
            }

            VarKw => {
                let checkpoint = self.builder.checkpoint();
                self.parse_var_decl(checkpoint);
            }
            ConstKw => {
                let checkpoint = self.builder.checkpoint();
                self.parse_const_decl(checkpoint);
            }
            StaticKw if self.nth(1) == VarKw => {
                let checkpoint = self.builder.checkpoint();
                self.parse_var_decl(checkpoint);
            }

            IfKw => self.parse_if_statement(),
            WhileKw => {
                self.builder.start_node(WhileStmt);
                self.bump();
                self.parse_expr();
                if self.eat(Colon) {
                    self.parse_block();
                } else {
                    self.error("expected `:` to open the loop body");
                }
                self.builder.finish_node();
            }
            ForKw => self.parse_for_statement(),
            MatchKw => self.parse_match_statement(),

            // Annotations such as `@warning_ignore` are legal inside a body.
            At => {
                let checkpoint = self.builder.checkpoint();
                while self.at(At) {
                    self.parse_annotation();
                }
                match self.current() {
                    VarKw => self.parse_var_decl(checkpoint),
                    ConstKw => self.parse_const_decl(checkpoint),
                    // A trailing annotation at the end of a block modifies
                    // nothing, but is not an error.
                    Dedent | Eof => {}
                    _ => self.parse_statement(),
                }
            }

            // A nested `func` is a lambda used as a statement; `class` and
            // `signal` can appear inside a class body reached from here.
            ClassKw | SignalKw | EnumKw => self.parse_class_member(),

            Eof | Dedent => {
                self.error("unexpected end of block");
            }

            _ => self.parse_expr_statement(),
        }
    }

    fn simple_statement(&mut self, kind: SyntaxKind) {
        self.builder.start_node(kind);
        self.bump();
        self.builder.finish_node();
    }

    fn parse_if_statement(&mut self) {
        self.builder.start_node(IfStmt);
        self.bump(); // if
        self.parse_expr();
        if self.eat(Colon) {
            self.parse_block();
        } else {
            self.error("expected `:` to open the branch body");
        }

        while self.at(ElifKw) {
            self.builder.start_node(ElifClause);
            self.bump();
            self.parse_expr();
            if self.eat(Colon) {
                self.parse_block();
            } else {
                self.error("expected `:` to open the branch body");
            }
            self.builder.finish_node();
        }

        if self.at(ElseKw) {
            self.builder.start_node(ElseClause);
            self.bump();
            if self.eat(Colon) {
                self.parse_block();
            } else {
                self.error("expected `:` to open the branch body");
            }
            self.builder.finish_node();
        }

        self.builder.finish_node();
    }

    fn parse_for_statement(&mut self) {
        self.builder.start_node(ForStmt);
        self.bump(); // for
        self.expect(Ident, "expected a loop variable name");
        if self.at(Colon) && matches!(self.nth(1), Ident | VoidKw) {
            self.builder.start_node(TypeHint);
            self.bump();
            self.parse_type();
            self.builder.finish_node();
        }
        // The `in` here is part of the loop, not the containment operator, so
        // the iterable is parsed separately rather than as one expression.
        if !self.eat(InKw) {
            self.error("expected `in` after the loop variable");
        }
        self.parse_expr();
        if self.eat(Colon) {
            self.parse_block();
        } else {
            self.error("expected `:` to open the loop body");
        }
        self.builder.finish_node();
    }

    fn parse_match_statement(&mut self) {
        self.builder.start_node(MatchStmt);
        self.bump(); // match
        self.parse_expr();
        if !self.eat(Colon) {
            self.error("expected `:` after the match subject");
        }
        if self.at(Indent) {
            self.bump();
            while !self.at(Dedent) && !self.at(Eof) {
                let before = self.pos;
                self.parse_match_arm();
                self.ensure_progress(before, &[Dedent]);
            }
            self.eat(Dedent);
        } else {
            self.error("expected an indented block of match arms");
        }
        self.builder.finish_node();
    }

    fn parse_match_arm(&mut self) {
        self.builder.start_node(MatchArm);
        // Patterns are comma-separated alternatives. Inside one, `var x` binds
        // a capture and `..` matches the rest, at any nesting depth — so the
        // flag stays set through nested array and dictionary patterns.
        self.in_pattern = true;
        loop {
            self.parse_expr();
            if !self.eat(Comma) {
                break;
            }
            if self.at(Colon) || self.at(WhenKw) || self.at(Eof) {
                break;
            }
        }
        self.in_pattern = false;
        if self.at(WhenKw) {
            self.builder.start_node(MatchGuard);
            self.bump();
            self.parse_expr();
            self.builder.finish_node();
        }
        if self.eat(Colon) {
            self.parse_block();
        } else {
            self.error("expected `:` after the match pattern");
        }
        self.builder.finish_node();
    }

    /// An expression, optionally followed by an assignment operator.
    fn parse_expr_statement(&mut self) {
        let checkpoint = self.builder.checkpoint();
        self.builder.start_node(ExprStmt);
        self.parse_expr();

        if is_assign_op(self.current()) {
            // Retroactively reclassify: this was an assignment all along.
            self.builder.finish_node();
            self.builder.start_node_at(checkpoint, AssignStmt);
            self.bump();
            self.parse_expr();
        }
        self.builder.finish_node();
    }

    // -- Expressions --------------------------------------------------------

    fn parse_expr(&mut self) {
        self.parse_expr_bp(0);
    }

    /// Pratt loop. `min_bp` is the binding power the caller has already claimed.
    fn parse_expr_bp(&mut self, min_bp: u8) {
        let checkpoint = self.builder.checkpoint();
        self.parse_prefix();

        loop {
            // A newline outside brackets ends the statement. Without this the
            // `if` opening the *next* line would be read as a ternary belonging
            // to this expression.
            if self.at_line_break() {
                break;
            }
            let kind = self.current();

            // Ternary `value if cond else other` binds loosest of all.
            if kind == IfKw && min_bp <= TERNARY_BP {
                self.builder.start_node_at(checkpoint, TernaryExpr);
                self.bump(); // if
                self.parse_expr_bp(TERNARY_BP + 1);
                if !self.eat(ElseKw) {
                    self.error("expected `else` to complete the conditional expression");
                }
                self.parse_expr_bp(TERNARY_BP);
                self.builder.finish_node();
                continue;
            }

            // `not in` is a single infix operator spelled as two words. It has
            // to be matched before `not` is considered as anything else.
            if kind == NotKw && self.nth(1) == InKw {
                let (left_bp, right_bp) = NOT_IN_BP;
                if left_bp < min_bp {
                    break;
                }
                self.builder.start_node_at(checkpoint, BinaryExpr);
                self.bump(); // not
                self.bump(); // in
                self.parse_expr_bp(right_bp);
                self.builder.finish_node();
                continue;
            }

            // `as` is a cast, not a plain binary operator, so it gets its own
            // node kind for the benefit of the static-typing lint rules.
            if kind == AsKw && CAST_BP >= min_bp {
                self.builder.start_node_at(checkpoint, CastExpr);
                self.bump();
                self.parse_type();
                self.builder.finish_node();
                continue;
            }

            let Some((left_bp, right_bp)) = infix_binding_power(kind) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }

            self.builder.start_node_at(checkpoint, BinaryExpr);
            self.bump();
            self.parse_expr_bp(right_bp);
            self.builder.finish_node();
        }
    }

    fn parse_prefix(&mut self) {
        match self.current() {
            NotKw | Bang => {
                self.builder.start_node(UnaryExpr);
                self.bump();
                self.parse_expr_bp(NOT_BP);
                self.builder.finish_node();
            }
            Minus | Plus | Tilde => {
                self.builder.start_node(UnaryExpr);
                self.bump();
                self.parse_expr_bp(UNARY_BP);
                self.builder.finish_node();
            }
            AwaitKw => {
                self.builder.start_node(AwaitExpr);
                self.bump();
                self.parse_expr_bp(UNARY_BP);
                self.builder.finish_node();
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) {
        let checkpoint = self.builder.checkpoint();
        self.parse_atom();

        loop {
            // Postfix operators cannot start a new line either, so `foo()`
            // followed by a line beginning `(x)` stays two statements.
            if self.at_line_break() {
                break;
            }
            match self.current() {
                LParen => {
                    self.builder.start_node_at(checkpoint, CallExpr);
                    self.parse_arg_list();
                    self.builder.finish_node();
                }
                LBracket => {
                    self.builder.start_node_at(checkpoint, SubscriptExpr);
                    self.bump();
                    self.enter_brackets();
                    self.parse_expr();
                    self.expect(RBracket, "expected `]` to close the subscript");
                    self.leave_brackets();
                    self.builder.finish_node();
                }
                Dot => {
                    self.builder.start_node_at(checkpoint, AttributeExpr);
                    self.bump();
                    // Keywords are legal member names in a few places, so accept
                    // any identifier-shaped token here.
                    if self.at(Ident) {
                        self.bump();
                    } else {
                        self.error("expected a member name after `.`");
                    }
                    self.builder.finish_node();
                }
                _ => break,
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn parse_atom(&mut self) {
        match self.current() {
            Int | Float | Str | StringName | NodePath | GetNode | UniqueNode | TrueKw | FalseKw
            | NullKw => {
                self.builder.start_node(Literal);
                self.bump();
                self.builder.finish_node();
            }

            Ident | SelfKw | SuperKw => {
                self.builder.start_node(NameRef);
                self.bump();
                self.builder.finish_node();
            }

            PreloadKw => {
                self.builder.start_node(PreloadExpr);
                self.bump();
                if self.at(LParen) {
                    self.parse_arg_list();
                } else {
                    self.error("expected `(` after `preload`");
                }
                self.builder.finish_node();
            }

            LParen => {
                self.builder.start_node(ParenExpr);
                self.bump();
                self.enter_brackets();
                if !self.at(RParen) {
                    self.parse_expr();
                }
                self.expect(RParen, "expected `)` to close the group");
                self.leave_brackets();
                self.builder.finish_node();
            }

            LBracket => {
                self.builder.start_node(ArrayExpr);
                self.bump();
                self.enter_brackets();
                while !self.at(RBracket) && !self.at(Eof) {
                    let before = self.pos;
                    self.parse_expr();
                    if !self.eat(Comma) {
                        break;
                    }
                    self.ensure_progress(before, &[RBracket]);
                }
                self.expect(RBracket, "expected `]` to close the array");
                self.leave_brackets();
                self.builder.finish_node();
            }

            LBrace => self.parse_dict(),

            // `var name` binds a capture inside a match pattern.
            VarKw if self.in_pattern => {
                self.builder.start_node(NameRef);
                self.bump();
                self.expect(Ident, "expected a name after `var` in a pattern");
                self.builder.finish_node();
            }

            // `..` matches whatever is left of an array or dictionary pattern.
            DotDot if self.in_pattern => {
                self.builder.start_node(Literal);
                self.bump();
                self.builder.finish_node();
            }

            // A lambda: `func(a): ...` or `func named(a): ...`
            FuncKw => {
                self.builder.start_node(LambdaExpr);
                self.bump();
                if self.at(Ident) {
                    self.bump();
                }
                if self.at(LParen) {
                    self.parse_param_list();
                } else {
                    self.error("expected `(` to open the lambda parameters");
                }
                if self.at(Arrow) {
                    self.builder.start_node(ReturnType);
                    self.bump();
                    self.parse_type();
                    self.builder.finish_node();
                }
                if self.eat(Colon) {
                    self.parse_block();
                } else {
                    self.error("expected `:` to open the lambda body");
                }
                self.builder.finish_node();
            }

            _ => self.error_and_recover("expected an expression", STATEMENT_START),
        }
    }

    /// `{"key": value}` and the Lua-style `{key = value}`.
    fn parse_dict(&mut self) {
        self.builder.start_node(DictExpr);
        self.bump(); // {
        self.enter_brackets();
        while !self.at(RBrace) && !self.at(Eof) {
            let before = self.pos;
            self.builder.start_node(DictEntry);
            if self.in_pattern && self.at(DotDot) {
                // A rest marker stands alone; it has no `key: value` shape.
                self.bump();
            } else if self.at(Ident) && self.nth(1) == Eq {
                self.bump(); // key
                self.bump(); // =
                self.parse_expr();
            } else {
                self.parse_expr();
                if self.eat(Colon) {
                    self.parse_expr();
                } else if !self.in_pattern {
                    // A dictionary pattern may test for a key alone, as in
                    // `{"name", "age"}`.
                    self.error("expected `:` between the key and value");
                }
            }
            self.builder.finish_node();
            if !self.eat(Comma) {
                break;
            }
            self.ensure_progress(before, &[RBrace]);
        }
        self.expect(RBrace, "expected `}` to close the dictionary");
        self.leave_brackets();
        self.builder.finish_node();
    }

    // -- Token plumbing -----------------------------------------------------

    /// The next real token, without consuming anything.
    ///
    /// Lookahead deliberately does not emit the trivia it skips over. Trivia is
    /// emitted by [`Self::bump`], which means it lands inside whichever node
    /// owns the token that *follows* it — so a comment on its own line attaches
    /// to the declaration it documents rather than to the one above it.
    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// The `n`th upcoming non-trivia token, without emitting anything.
    fn nth(&self, n: usize) -> SyntaxKind {
        self.tokens[self.pos..]
            .iter()
            .filter(|token| !token.kind.is_trivia())
            .nth(n)
            .map_or(Eof, |token| token.kind)
    }

    /// Source text of the next non-trivia token, for contextual keywords.
    fn current_text(&self) -> &str {
        self.nth_text(0)
    }

    /// Source text of the `n`th upcoming non-trivia token.
    fn nth_text(&self, n: usize) -> &str {
        self.tokens[self.pos..]
            .iter()
            .filter(|token| !token.kind.is_trivia())
            .nth(n)
            .map_or("", |token| token.text(self.source))
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: SyntaxKind, message: &str) {
        if !self.eat(kind) {
            self.error(message);
        }
    }

    /// Consume the current token into the tree.
    fn bump(&mut self) {
        self.skip_trivia();
        self.bump_raw();
    }

    fn bump_raw(&mut self) {
        let token = self.tokens[self.pos];
        self.builder.token(token);
        if token.kind != Eof {
            self.pos += 1;
        }
    }

    fn skip_trivia(&mut self) {
        while self.tokens[self.pos].kind.is_trivia() {
            self.bump_raw();
        }
    }

    /// Whether a newline separates the cursor from the next real token.
    ///
    /// A line continuation is a distinct token kind, so `a \` followed by a
    /// newline correctly reports `false` here.
    fn newline_ahead(&self) -> bool {
        self.tokens[self.pos..]
            .iter()
            .take_while(|token| token.kind.is_trivia())
            .any(|token| token.kind == Newline)
    }

    /// Whether the current position ends a logical line.
    ///
    /// Inside brackets a newline carries no meaning, which is what lets an
    /// array literal or a parenthesised expression span several lines.
    fn at_line_break(&self) -> bool {
        self.bracket_depth == 0 && self.newline_ahead()
    }

    fn enter_brackets(&mut self) {
        self.bracket_depth += 1;
    }

    fn leave_brackets(&mut self) {
        self.bracket_depth = self.bracket_depth.saturating_sub(1);
    }

    fn at_statement_end(&self) -> bool {
        self.newline_ahead() || matches!(self.current(), Semicolon | Dedent | Eof)
    }

    // -- Errors -------------------------------------------------------------

    fn error(&mut self, message: &str) {
        // Point at the offending token, not at the trivia in front of it.
        let range = self.tokens[self.pos..]
            .iter()
            .find(|token| !token.kind.is_trivia())
            .map_or_else(|| self.tokens[self.pos].range, |token| token.range);
        // Collapse runs of errors at the same spot; they are almost always
        // cascades from the first one and only add noise.
        if self
            .errors
            .last()
            .is_some_and(|last| last.range().start() == range.start())
        {
            return;
        }
        self.errors
            .push(SyntaxError::new(TextRange::empty(range.start()), message));
    }

    /// Record an error and skip tokens until something in `recovery` shows up.
    fn error_and_recover(&mut self, message: &str, recovery: &[SyntaxKind]) {
        self.error(message);
        self.builder.start_node(Error);
        // Always consume at least one token so the caller cannot spin.
        if !self.at(Eof) {
            self.bump();
        }
        while !self.at(Eof) && !recovery.contains(&self.current()) && !self.at(Dedent) {
            if self.newline_ahead() {
                break;
            }
            self.bump();
        }
        self.builder.finish_node();
    }

    /// Backstop against a rule that returned without consuming anything.
    ///
    /// Every loop in the parser routes through here, so a grammar bug shows up
    /// as one stray Error node rather than a hang.
    fn ensure_progress(&mut self, before: usize, recovery: &[SyntaxKind]) {
        if self.pos != before {
            self.fuel = 0;
            return;
        }
        self.fuel += 1;
        if self.fuel > 1 {
            self.fuel = 0;
            self.error_and_recover("unexpected token", recovery);
        }
    }
}

// -- Precedence -------------------------------------------------------------

/// Binding power of the ternary `x if c else y`, the loosest operator.
const TERNARY_BP: u8 = 1;
/// Binding power of `as`, which sits just above the boolean operators.
const CAST_BP: u8 = 9;
/// Right binding power of the `not` / `!` prefix operator.
const NOT_BP: u8 = 7;
/// Right binding power of the arithmetic prefix operators.
const UNARY_BP: u8 = 27;
/// Binding powers of `not in`, matching plain `in`.
const NOT_IN_BP: (u8, u8) = (11, 12);

/// Left and right binding powers for infix operators.
///
/// Ordering follows the GDScript reference: comparisons bind tighter than
/// `in`, which binds tighter than `is`, which binds tighter than `not`.
/// Right-associative operators get a right power below their left power.
fn infix_binding_power(kind: SyntaxKind) -> Option<(u8, u8)> {
    Some(match kind {
        OrKw | PipePipe => (3, 4),
        AndKw | AmpAmp => (5, 6),
        IsKw => (9, 10),
        InKw => (11, 12),
        Lt | LtEq | Gt | GtEq | EqEq | BangEq => (13, 14),
        Pipe => (15, 16),
        Caret => (17, 18),
        Amp => (19, 20),
        Shl | Shr => (21, 22),
        Plus | Minus => (23, 24),
        Star | Slash | Percent => (25, 26),
        // Right-associative and tighter than unary minus, so `-2 ** 2` is
        // `-(2 ** 2)`, matching Godot.
        StarStar => (30, 29),
        _ => return None,
    })
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
