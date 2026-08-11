//! The single kind enum covering both tokens (leaves) and nodes (interior).
//!
//! Keeping tokens and nodes in one enum is the usual arrangement for a lossless
//! tree: it lets a child be described by one `SyntaxKind` regardless of whether
//! it turned out to be a leaf or a subtree.

/// A syntactic category. Values below [`SyntaxKind::FIRST_NODE`] are tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(clippy::upper_case_acronyms)]
pub enum SyntaxKind {
    // ---- Trivia -----------------------------------------------------------
    // Trivia carries no meaning to the grammar but every byte of it lives in
    // the tree, which is what makes formatting round-trips exact.
    /// Spaces and tabs. Leading indentation is *also* whitespace; `Indent` and
    /// `Dedent` are emitted alongside it, not instead of it.
    Whitespace,
    /// `# comment`
    Comment,
    /// `## documentation comment`
    DocComment,
    /// `\n` or `\r\n`
    Newline,
    /// A backslash followed by a newline, joining two physical lines.
    LineContinuation,

    // ---- Synthetic --------------------------------------------------------
    // Zero-width markers produced by the lexer's indentation tracking.
    Indent,
    Dedent,
    Eof,
    /// A byte the lexer could not classify.
    Unknown,

    // ---- Literals ---------------------------------------------------------
    Int,
    Float,
    /// `"..."`, `'...'`, triple-quoted, and `r`-prefixed raw variants.
    Str,
    /// `&"name"` — a `StringName` literal.
    StringName,
    /// `^"path"` — a `NodePath` literal.
    NodePath,
    /// `$Node/Path` or `$"Node/Path"`.
    GetNode,
    /// `%UniqueName` or `%"UniqueName"`.
    UniqueNode,
    Ident,

    // ---- Keywords ---------------------------------------------------------
    // Kept contiguous so `is_keyword` can be a range check. `abstract` is
    // deliberately absent: Godot 4.5 spells it `@abstract`, an annotation, so
    // treating it as a keyword would break `var abstract = 1`.
    AndKw,
    AsKw,
    AssertKw,
    AwaitKw,
    BreakKw,
    BreakpointKw,
    ClassKw,
    ClassNameKw,
    ConstKw,
    ContinueKw,
    ElifKw,
    ElseKw,
    EnumKw,
    ExtendsKw,
    FalseKw,
    ForKw,
    FuncKw,
    IfKw,
    InKw,
    IsKw,
    MatchKw,
    NamespaceKw,
    NotKw,
    NullKw,
    OrKw,
    PassKw,
    PreloadKw,
    ReturnKw,
    SelfKw,
    SignalKw,
    StaticKw,
    SuperKw,
    TraitKw,
    TrueKw,
    VarKw,
    VoidKw,
    WhenKw,
    WhileKw,
    YieldKw,

    // ---- Punctuation and operators ---------------------------------------
    Plus,
    Minus,
    Star,
    StarStar,
    Slash,
    Percent,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    StarStarEq,
    SlashEq,
    PercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
    EqEq,
    Bang,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Amp,
    AmpAmp,
    Pipe,
    PipePipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    Arrow,
    ColonEq,
    Colon,
    Semicolon,
    Comma,
    Dot,
    DotDot,
    /// `...`, introducing a variadic parameter.
    Ellipsis,
    At,
    Dollar,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    // ---- Nodes ------------------------------------------------------------
    /// The root node. Always the outermost node of a parse.
    SourceFile,

    // Class-level declarations
    Annotation,
    ClassNameDecl,
    ExtendsDecl,
    SignalDecl,
    EnumDecl,
    EnumBody,
    EnumVariant,
    ConstDecl,
    VarDecl,
    FuncDecl,
    ClassDecl,

    // Declaration pieces
    ParamList,
    Param,
    ArgList,
    TypeHint,
    ReturnType,
    Initializer,
    /// The `set`/`get` clause block hanging off a `var`.
    Accessors,
    Setter,
    Getter,
    Block,

    // Statements
    ExprStmt,
    AssignStmt,
    IfStmt,
    ElifClause,
    ElseClause,
    WhileStmt,
    ForStmt,
    MatchStmt,
    MatchArm,
    MatchGuard,
    ReturnStmt,
    PassStmt,
    BreakStmt,
    ContinueStmt,
    BreakpointStmt,
    AssertStmt,

    // Expressions
    BinaryExpr,
    UnaryExpr,
    TernaryExpr,
    CastExpr,
    AwaitExpr,
    CallExpr,
    SubscriptExpr,
    /// `a.b` — attribute access.
    AttributeExpr,
    ParenExpr,
    ArrayExpr,
    DictExpr,
    DictEntry,
    LambdaExpr,
    PreloadExpr,
    /// A bare identifier used as a value.
    NameRef,
    Literal,

    /// Wraps tokens the parser could not fit into the grammar. Its presence is
    /// what keeps the tree lossless in the face of a syntax error.
    Error,
}

impl SyntaxKind {
    /// The first node kind. Everything ordered before this is a token.
    pub const FIRST_NODE: SyntaxKind = SyntaxKind::SourceFile;

    const FIRST_KEYWORD: SyntaxKind = SyntaxKind::AndKw;
    const LAST_KEYWORD: SyntaxKind = SyntaxKind::YieldKw;

    /// Whether this token is a reserved word.
    #[must_use]
    pub fn is_keyword(self) -> bool {
        self >= Self::FIRST_KEYWORD && self <= Self::LAST_KEYWORD
    }

    /// Whether this token is shaped like an identifier.
    ///
    /// Keywords count: annotation names and member names may reuse them, so
    /// `@tool` and `x.get` have to be accepted.
    #[must_use]
    pub fn is_ident_like(self) -> bool {
        self == Self::Ident || self.is_keyword()
    }

    /// Whether this kind describes a leaf produced by the lexer.
    #[must_use]
    pub fn is_token(self) -> bool {
        self < Self::FIRST_NODE
    }

    /// Whether this kind describes an interior node produced by the parser.
    #[must_use]
    pub fn is_node(self) -> bool {
        !self.is_token()
    }

    /// Trivia is skipped by the parser but retained in the tree.
    ///
    /// `Indent` and `Dedent` are deliberately *not* trivia: they are structural,
    /// and the parser consumes them to delimit blocks.
    #[must_use]
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace
                | Self::Comment
                | Self::DocComment
                | Self::Newline
                | Self::LineContinuation
        )
    }

    /// Whether this token is a comment of either flavour.
    #[must_use]
    pub fn is_comment(self) -> bool {
        matches!(self, Self::Comment | Self::DocComment)
    }

    /// Whether this token may begin a type annotation or a value expression.
    #[must_use]
    pub fn is_literal(self) -> bool {
        matches!(
            self,
            Self::Int
                | Self::Float
                | Self::Str
                | Self::StringName
                | Self::NodePath
                | Self::TrueKw
                | Self::FalseKw
                | Self::NullKw
        )
    }

    /// Map an identifier-shaped string to its keyword kind, if it is one.
    #[must_use]
    pub fn from_keyword(text: &str) -> Option<Self> {
        Some(match text {
            "and" => Self::AndKw,
            "as" => Self::AsKw,
            "assert" => Self::AssertKw,
            "await" => Self::AwaitKw,
            "break" => Self::BreakKw,
            "breakpoint" => Self::BreakpointKw,
            "class" => Self::ClassKw,
            "class_name" => Self::ClassNameKw,
            "const" => Self::ConstKw,
            "continue" => Self::ContinueKw,
            "elif" => Self::ElifKw,
            "else" => Self::ElseKw,
            "enum" => Self::EnumKw,
            "extends" => Self::ExtendsKw,
            "false" => Self::FalseKw,
            "for" => Self::ForKw,
            "func" => Self::FuncKw,
            "if" => Self::IfKw,
            "in" => Self::InKw,
            "is" => Self::IsKw,
            "match" => Self::MatchKw,
            "namespace" => Self::NamespaceKw,
            "not" => Self::NotKw,
            "null" => Self::NullKw,
            "or" => Self::OrKw,
            "pass" => Self::PassKw,
            "preload" => Self::PreloadKw,
            "return" => Self::ReturnKw,
            "self" => Self::SelfKw,
            "signal" => Self::SignalKw,
            "static" => Self::StaticKw,
            "super" => Self::SuperKw,
            "trait" => Self::TraitKw,
            "true" => Self::TrueKw,
            "var" => Self::VarKw,
            "void" => Self::VoidKw,
            "when" => Self::WhenKw,
            "while" => Self::WhileKw,
            "yield" => Self::YieldKw,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_and_node_partition() {
        assert!(SyntaxKind::Whitespace.is_token());
        assert!(SyntaxKind::RBrace.is_token());
        assert!(SyntaxKind::SourceFile.is_node());
        assert!(SyntaxKind::Error.is_node());
        assert!(!SyntaxKind::SourceFile.is_token());
    }

    #[test]
    fn indent_is_structural_not_trivia() {
        assert!(SyntaxKind::Whitespace.is_trivia());
        assert!(SyntaxKind::Comment.is_trivia());
        assert!(!SyntaxKind::Indent.is_trivia());
        assert!(!SyntaxKind::Dedent.is_trivia());
    }

    #[test]
    fn keywords_round_trip() {
        assert_eq!(SyntaxKind::from_keyword("func"), Some(SyntaxKind::FuncKw));
        assert_eq!(
            SyntaxKind::from_keyword("class_name"),
            Some(SyntaxKind::ClassNameKw)
        );
        assert_eq!(SyntaxKind::from_keyword("classname"), None);
        assert_eq!(SyntaxKind::from_keyword("Func"), None);
        assert_eq!(SyntaxKind::from_keyword("position"), None);
    }
}
