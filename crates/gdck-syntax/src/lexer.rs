//! A hand-written lexer for GDScript.
//!
//! Two properties matter here and are enforced by tests:
//!
//! 1. **Lossless.** Concatenating the source text of every token in order
//!    reproduces the input byte for byte. Whitespace and comments are tokens,
//!    not skipped input.
//! 2. **Block-aware.** GDScript delimits blocks by indentation, so the lexer
//!    emits zero-width [`Indent`](SyntaxKind::Indent) and
//!    [`Dedent`](SyntaxKind::Dedent) tokens the way a Python tokenizer does,
//!    which lets the parser stay a plain recursive-descent affair.

use std::cmp::Ordering;

use crate::error::SyntaxError;
use crate::kind::SyntaxKind;
use crate::text::TextRange;

/// Column width of a tab when measuring indentation.
///
/// Only ever used to *compare* indentation depth between lines. The raw
/// indentation text is preserved in the whitespace token, so a linter can still
/// see whether a file mixes tabs and spaces.
const TAB_WIDTH: u32 = 4;

/// A lexed token: a kind plus the span of source it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub range: TextRange,
}

impl Token {
    #[must_use]
    pub fn text(self, source: &str) -> &str {
        self.range.slice(source)
    }
}

/// The result of lexing a source file.
#[derive(Debug, Clone)]
pub struct LexResult {
    pub tokens: Vec<Token>,
    pub errors: Vec<SyntaxError>,
}

/// A multi-line lambda body opened inside brackets.
///
/// Indentation is normally meaningless inside `()`, `[]` and `{}`, but a lambda
/// written across several lines needs it back:
///
/// ```gdscript
/// button.pressed.connect(
///     func() -> void:
///         do_something()
///         do_more()
/// )
/// ```
///
/// Godot's own tokenizer handles this the same way, by tracking where such a
/// body starts and re-enabling indentation for exactly its extent.
#[derive(Debug, Clone, Copy)]
struct LambdaContext {
    /// Bracket depth the lambda's `:` was seen at. Indentation is significant
    /// again only at exactly this depth — nested brackets suppress it as usual.
    bracket_depth: u32,
    /// Indentation of the body's first line, once one has been seen.
    ///
    /// The body then runs until a line indented less than that, the same rule
    /// Python uses for a suite. Deriving it from the body rather than from the
    /// opening line matters because a lambda can open mid-line — after a comma
    /// separating it from a previous argument — with its body at that same
    /// column.
    body_column: Option<u32>,
    /// Height of the indent stack when the lambda opened, so closing it emits
    /// exactly the dedents its body pushed.
    indent_len: usize,
    /// Whether the body started on the same line as the `:`.
    ///
    /// `func(): return 1` is a complete lambda, so it ends where its line does.
    /// Without this the next line's indentation would be taken for the start
    /// of the body, and an argument list broken across lines —
    ///
    /// ```gdscript
    /// connect(
    ///     func(): return 1
    /// )
    /// ```
    ///
    /// — would swallow its own closing bracket.
    inline: bool,
}

/// Tokenize `source`.
///
/// Always succeeds. Malformed input produces [`SyntaxKind::Unknown`] tokens and
/// entries in [`LexResult::errors`], never a hard failure.
#[must_use]
pub fn tokenize(source: &str) -> LexResult {
    Lexer::new(source).run()
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: u32,
    tokens: Vec<Token>,
    errors: Vec<SyntaxError>,
    /// Indentation columns of the currently open blocks. Always starts with 0.
    indents: Vec<u32>,
    /// Nesting depth of `()`, `[]` and `{}`. Inside brackets, newlines are
    /// trivia and indentation is not significant.
    bracket_depth: u32,
    /// Multi-line lambda bodies currently open inside brackets, innermost last.
    lambda_stack: Vec<LambdaContext>,
    /// Bracket depth at which a `func` was seen, arming lambda detection. The
    /// next `:` at that same depth opens a lambda body.
    pending_lambda: Option<u32>,
    /// Set after a newline, cleared once the line is under way.
    at_line_start: bool,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            // A typical GDScript file lands near one token per 4 bytes.
            tokens: Vec::with_capacity(source.len() / 4),
            errors: Vec::new(),
            indents: vec![0],
            bracket_depth: 0,
            lambda_stack: Vec::new(),
            pending_lambda: None,
            at_line_start: true,
        }
    }

    fn run(mut self) -> LexResult {
        while !self.at_eof() {
            if self.at_line_start && self.indent_significant() {
                self.lex_line_start();
            } else {
                self.lex_token();
            }
        }

        // Close any blocks still open at end of file.
        let end = self.pos;
        while !self.lambda_stack.is_empty() {
            self.close_top_lambda(end);
        }
        while self.indents.len() > 1 {
            self.indents.pop();
            self.push(SyntaxKind::Dedent, TextRange::empty(end));
        }
        self.push(SyntaxKind::Eof, TextRange::empty(end));

        LexResult {
            tokens: self.tokens,
            errors: self.errors,
        }
    }

    // -- Indentation --------------------------------------------------------

    /// Handle the start of a physical line: measure indentation and emit the
    /// indent/dedent markers implied by it.
    ///
    /// Blank lines and comment-only lines carry no indentation information, so
    /// they are passed through as trivia without touching the indent stack.
    fn lex_line_start(&mut self) {
        let start = self.pos;
        let mut column = 0;
        let mut cursor = self.pos;
        while let Some(byte) = self.byte_at(cursor) {
            match byte {
                b' ' => column += 1,
                // A tab advances to the next tab stop.
                b'\t' => column = (column / TAB_WIDTH + 1) * TAB_WIDTH,
                _ => break,
            }
            cursor += 1;
        }

        let indent_range = TextRange::new(start, cursor);
        // End of file, a line break, a comment, or a line continuation on an
        // otherwise empty line: none of these say anything about indentation.
        let blank_line = matches!(
            self.byte_at(cursor),
            None | Some(b'\n' | b'\r' | b'#' | b'\\')
        );

        if blank_line {
            // No indent bookkeeping; just emit the whitespace and carry on
            // lexing the comment or newline that follows.
            if !indent_range.is_empty() {
                self.pos = cursor;
                self.push(SyntaxKind::Whitespace, indent_range);
            }
            self.at_line_start = false;
            // Re-arm at_line_start when the newline is consumed by lex_token.
            self.lex_token();
            return;
        }

        // The first line of a lambda body fixes that body's indentation; a
        // later line indented less than it ends the lambda, however many
        // brackets deep it sits.
        while let Some(context) = self.lambda_stack.last() {
            match context.body_column {
                // A body written on the `:` line is already complete.
                None if context.inline => self.close_top_lambda(start),
                None => {
                    self.lambda_stack
                        .last_mut()
                        .expect("checked immediately above")
                        .body_column = Some(column);
                    break;
                }
                Some(body_column) if column < body_column => self.close_top_lambda(start),
                Some(_) => break,
            }
        }

        // Closing a lambda can hand control back to a bracket, where
        // indentation means nothing again.
        if !self.indent_significant() {
            if !indent_range.is_empty() {
                self.pos = cursor;
                self.push(SyntaxKind::Whitespace, indent_range);
            }
            self.at_line_start = false;
            return;
        }

        // Dedents close blocks, so they belong before this line's indentation
        // whitespace; an indent opens a block containing it, so it goes after.
        let current = *self.indents.last().expect("indent stack is never empty");
        match column.cmp(&current) {
            Ordering::Greater => {
                self.pos = cursor;
                self.emit_indent_whitespace(indent_range);
                self.indents.push(column);
                self.push(SyntaxKind::Indent, TextRange::empty(cursor));
            }
            Ordering::Less => {
                while *self.indents.last().expect("indent stack is never empty") > column {
                    self.indents.pop();
                    self.push(SyntaxKind::Dedent, TextRange::empty(start));
                }
                if *self.indents.last().expect("indent stack is never empty") != column {
                    self.errors.push(SyntaxError::new(
                        indent_range,
                        "unindent does not match any outer indentation level",
                    ));
                    // Accept the column so one bad line does not cascade.
                    self.indents.push(column);
                }
                self.pos = cursor;
                self.emit_indent_whitespace(indent_range);
            }
            Ordering::Equal => {
                self.pos = cursor;
                self.emit_indent_whitespace(indent_range);
            }
        }

        self.at_line_start = false;
    }

    /// Emit a line's leading whitespace, if it has any.
    fn emit_indent_whitespace(&mut self, indent_range: TextRange) {
        if !indent_range.is_empty() {
            self.push(SyntaxKind::Whitespace, indent_range);
        }
    }

    // -- Token dispatch -----------------------------------------------------

    fn lex_token(&mut self) {
        let start = self.pos;
        let Some(byte) = self.byte_at(self.pos) else {
            return;
        };

        let kind = match byte {
            b' ' | b'\t' => {
                self.bump_while(|b| b == b' ' || b == b'\t');
                SyntaxKind::Whitespace
            }
            b'\n' => {
                self.pos += 1;
                self.finish_line();
                SyntaxKind::Newline
            }
            b'\r' => {
                self.pos += 1;
                if self.byte_at(self.pos) == Some(b'\n') {
                    self.pos += 1;
                }
                self.finish_line();
                SyntaxKind::Newline
            }
            b'\\' => self.lex_backslash(),
            b'#' => self.lex_comment(),
            b'0'..=b'9' => self.lex_number(),
            b'.' if matches!(self.byte_at(self.pos + 1), Some(b'0'..=b'9')) => self.lex_number(),
            b'"' | b'\'' => self.lex_string(),
            b'$' => self.lex_node_path(SyntaxKind::GetNode),
            b'&' | b'^' if self.starts_annotated_string() => self.lex_annotated_string(byte),
            b'%' if self.starts_unique_node() => self.lex_node_path(SyntaxKind::UniqueNode),
            b if is_ident_start(b) => self.lex_ident_or_keyword(),
            _ => self.lex_operator(),
        };

        // lex_operator already emitted its own token when it recovered.
        if self.pos == start && kind == SyntaxKind::Unknown {
            self.pos += 1;
        }

        // Arm and fire lambda detection. A `func` inside brackets is always the
        // start of a lambda, and the next `:` at that same depth opens its body.
        if kind == SyntaxKind::FuncKw && self.bracket_depth > 0 {
            self.pending_lambda = Some(self.bracket_depth);
        } else if kind == SyntaxKind::Colon
            && self.bracket_depth > 0
            && self.pending_lambda == Some(self.bracket_depth)
        {
            self.pending_lambda = None;
            self.lambda_stack.push(LambdaContext {
                bracket_depth: self.bracket_depth,
                body_column: None,
                indent_len: self.indents.len(),
                inline: false,
            });
        } else if kind != SyntaxKind::Newline {
            if let Some(context) = self.lambda_stack.last_mut() {
                if context.body_column.is_none() {
                    // A token after the `:` and before any newline means the
                    // body is on this line, so the lambda ends with it.
                    context.inline = true;
                }
            }
        }

        if kind != SyntaxKind::Newline {
            self.at_line_start = false;
        }

        self.push(kind, TextRange::new(start, self.pos));
    }

    /// Whether indentation carries meaning at the current position.
    ///
    /// Outside brackets it always does. Inside them it does only within the
    /// body of a multi-line lambda, and only at that lambda's own depth.
    fn indent_significant(&self) -> bool {
        match self.lambda_stack.last() {
            Some(context) => self.bracket_depth == context.bracket_depth,
            None => self.bracket_depth == 0,
        }
    }

    /// Close the innermost lambda body, emitting the dedents it owes.
    fn close_top_lambda(&mut self, offset: u32) {
        let Some(context) = self.lambda_stack.pop() else {
            return;
        };
        while self.indents.len() > context.indent_len {
            self.indents.pop();
            self.push(SyntaxKind::Dedent, TextRange::empty(offset));
        }
    }

    /// Called after consuming a newline.
    fn finish_line(&mut self) {
        self.at_line_start = true;
    }

    fn lex_backslash(&mut self) -> SyntaxKind {
        let start = self.pos;
        let mut cursor = self.pos + 1;
        // Tolerate trailing whitespace between the backslash and the newline;
        // it is a common and otherwise invisible mistake.
        while matches!(self.byte_at(cursor), Some(b' ' | b'\t')) {
            cursor += 1;
        }
        match self.byte_at(cursor) {
            Some(b'\n') => {
                self.pos = cursor + 1;
                SyntaxKind::LineContinuation
            }
            Some(b'\r') => {
                cursor += 1;
                if self.byte_at(cursor) == Some(b'\n') {
                    cursor += 1;
                }
                self.pos = cursor;
                SyntaxKind::LineContinuation
            }
            _ => {
                self.pos = start + 1;
                self.errors.push(SyntaxError::new(
                    TextRange::new(start, self.pos),
                    "stray `\\` outside a line continuation",
                ));
                SyntaxKind::Unknown
            }
        }
    }

    fn lex_comment(&mut self) -> SyntaxKind {
        let doc = self.byte_at(self.pos + 1) == Some(b'#');
        self.bump_while(|b| b != b'\n' && b != b'\r');
        if doc {
            SyntaxKind::DocComment
        } else {
            SyntaxKind::Comment
        }
    }

    // -- Literals -----------------------------------------------------------

    fn lex_number(&mut self) -> SyntaxKind {
        let mut is_float = false;

        if self.byte_at(self.pos) == Some(b'0')
            && matches!(self.byte_at(self.pos + 1), Some(b'x' | b'X' | b'b' | b'B'))
        {
            let radix_marker = self.byte_at(self.pos + 1).expect("checked above");
            self.pos += 2;
            if radix_marker == b'x' || radix_marker == b'X' {
                self.bump_while(|b| b.is_ascii_hexdigit() || b == b'_');
            } else {
                self.bump_while(|b| matches!(b, b'0' | b'1' | b'_'));
            }
            return SyntaxKind::Int;
        }

        self.bump_while(|b| b.is_ascii_digit() || b == b'_');

        // A `.` is only part of the number when a digit follows, so `1..2` and
        // `1.foo()` still lex as a range and a method call.
        if self.byte_at(self.pos) == Some(b'.')
            && matches!(self.byte_at(self.pos + 1), Some(b'0'..=b'9'))
        {
            is_float = true;
            self.pos += 1;
            self.bump_while(|b| b.is_ascii_digit() || b == b'_');
        } else if self.byte_at(self.pos) == Some(b'.')
            && !matches!(self.byte_at(self.pos + 1), Some(b'.'))
            && !matches!(self.byte_at(self.pos + 1), Some(b) if is_ident_start(b))
        {
            // Trailing-dot form: `1.`
            is_float = true;
            self.pos += 1;
        }

        if matches!(self.byte_at(self.pos), Some(b'e' | b'E')) {
            let mut cursor = self.pos + 1;
            if matches!(self.byte_at(cursor), Some(b'+' | b'-')) {
                cursor += 1;
            }
            if matches!(self.byte_at(cursor), Some(b'0'..=b'9')) {
                is_float = true;
                self.pos = cursor;
                self.bump_while(|b| b.is_ascii_digit() || b == b'_');
            }
        }

        if is_float {
            SyntaxKind::Float
        } else {
            SyntaxKind::Int
        }
    }

    /// Whether an `&` or `^` at the cursor introduces a `StringName` or
    /// `NodePath` literal rather than a bitwise operator.
    fn starts_annotated_string(&self) -> bool {
        matches!(self.byte_at(self.pos + 1), Some(b'"' | b'\'')) && !self.prev_can_end_expr()
    }

    fn lex_annotated_string(&mut self, sigil: u8) -> SyntaxKind {
        self.pos += 1;
        self.lex_string();
        if sigil == b'&' {
            SyntaxKind::StringName
        } else {
            SyntaxKind::NodePath
        }
    }

    /// Whether a `%` at the cursor introduces a unique-node path rather than
    /// the modulo operator.
    fn starts_unique_node(&self) -> bool {
        if self.prev_can_end_expr() {
            return false;
        }
        matches!(self.byte_at(self.pos + 1), Some(b'"' | b'\''))
            || matches!(self.byte_at(self.pos + 1), Some(b) if is_ident_start(b))
    }

    /// Lex `$Node/Path`, `$"quoted/path"` or `%UniqueName`.
    ///
    /// Node paths contain `/` and `..`, neither of which can be lexed as an
    /// operator here, so the whole path becomes one token.
    fn lex_node_path(&mut self, kind: SyntaxKind) -> SyntaxKind {
        let sigil_start = self.pos;
        self.pos += 1;

        if matches!(self.byte_at(self.pos), Some(b'"' | b'\'')) {
            self.lex_string();
            return kind;
        }

        let mut matched_any = false;

        // An absolute path such as `$/root` starts with the separator.
        if self.byte_at(self.pos) == Some(b'/') {
            self.pos += 1;
            matched_any = true;
        }

        loop {
            // A segment is `..`, an optional `%` prefix, or a name.
            if self.byte_at(self.pos) == Some(b'.') && self.byte_at(self.pos + 1) == Some(b'.') {
                self.pos += 2;
                matched_any = true;
            } else {
                if self.byte_at(self.pos) == Some(b'%') {
                    self.pos += 1;
                    matched_any = true;
                }
                if matches!(self.byte_at(self.pos), Some(b) if is_ident_start(b)) {
                    self.bump_while(is_ident_continue);
                    matched_any = true;
                } else {
                    break;
                }
            }

            if self.byte_at(self.pos) == Some(b'/') {
                self.pos += 1;
            } else {
                break;
            }
        }

        if !matched_any {
            self.errors.push(SyntaxError::new(
                TextRange::new(sigil_start, self.pos),
                "expected a node path after the sigil",
            ));
        }
        kind
    }

    fn lex_string(&mut self) -> SyntaxKind {
        let start = self.pos;
        let quote = self.byte_at(self.pos).expect("caller checked for a quote");

        // Triple-quoted strings span lines and end only on a matching triple.
        let triple =
            self.byte_at(self.pos + 1) == Some(quote) && self.byte_at(self.pos + 2) == Some(quote);
        let delim_len = if triple { 3 } else { 1 };
        self.pos += delim_len;

        loop {
            let Some(byte) = self.byte_at(self.pos) else {
                self.errors.push(SyntaxError::new(
                    TextRange::new(start, self.pos),
                    "unterminated string literal",
                ));
                break;
            };

            // A backslash escapes the next byte even in raw strings: it stops
            // the quote from terminating, it just stays in the value.
            if byte == b'\\' {
                self.pos += 1;
                if self.pos < self.len() {
                    self.pos += 1;
                }
                continue;
            }

            if !triple && matches!(byte, b'\n' | b'\r') {
                self.errors.push(SyntaxError::new(
                    TextRange::new(start, self.pos),
                    "unterminated string literal",
                ));
                break;
            }

            if byte == quote {
                if triple {
                    if self.byte_at(self.pos + 1) == Some(quote)
                        && self.byte_at(self.pos + 2) == Some(quote)
                    {
                        self.pos += 3;
                        break;
                    }
                    self.pos += 1;
                    continue;
                }
                self.pos += 1;
                break;
            }

            self.pos += 1;
        }

        SyntaxKind::Str
    }

    fn lex_ident_or_keyword(&mut self) -> SyntaxKind {
        let start = self.pos;
        self.bump_while(is_ident_continue);
        let text = TextRange::new(start, self.pos).slice(self.source);

        // `r"..."` is a raw string, not the identifier `r`.
        if text == "r" && matches!(self.byte_at(self.pos), Some(b'"' | b'\'')) {
            self.lex_string();
            return SyntaxKind::Str;
        }

        SyntaxKind::from_keyword(text).unwrap_or(SyntaxKind::Ident)
    }

    // -- Operators ----------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn lex_operator(&mut self) -> SyntaxKind {
        let byte = self.byte_at(self.pos).expect("caller checked for a byte");
        let next = self.byte_at(self.pos + 1);
        let after = self.byte_at(self.pos + 2);

        // A lambda body inside brackets also ends at the comma separating it
        // from the next element, or at the bracket that encloses it. The
        // dedents must land before this token, so close before consuming it.
        match byte {
            b')' | b']' | b'}' => {
                while self
                    .lambda_stack
                    .last()
                    .is_some_and(|context| context.bracket_depth >= self.bracket_depth)
                {
                    self.close_top_lambda(self.pos);
                }
            }
            b',' => {
                while self
                    .lambda_stack
                    .last()
                    .is_some_and(|context| context.bracket_depth == self.bracket_depth)
                {
                    self.close_top_lambda(self.pos);
                }
            }
            _ => {}
        }

        self.pos += 1;

        macro_rules! two {
            ($kind:expr) => {{
                self.pos += 1;
                $kind
            }};
        }
        macro_rules! three {
            ($kind:expr) => {{
                self.pos += 2;
                $kind
            }};
        }

        match byte {
            b'+' if next == Some(b'=') => two!(SyntaxKind::PlusEq),
            b'+' => SyntaxKind::Plus,
            b'-' if next == Some(b'=') => two!(SyntaxKind::MinusEq),
            b'-' if next == Some(b'>') => two!(SyntaxKind::Arrow),
            b'-' => SyntaxKind::Minus,
            b'*' if next == Some(b'*') && after == Some(b'=') => three!(SyntaxKind::StarStarEq),
            b'*' if next == Some(b'*') => two!(SyntaxKind::StarStar),
            b'*' if next == Some(b'=') => two!(SyntaxKind::StarEq),
            b'*' => SyntaxKind::Star,
            b'/' if next == Some(b'=') => two!(SyntaxKind::SlashEq),
            b'/' => SyntaxKind::Slash,
            b'%' if next == Some(b'=') => two!(SyntaxKind::PercentEq),
            b'%' => SyntaxKind::Percent,
            b'=' if next == Some(b'=') => two!(SyntaxKind::EqEq),
            b'=' => SyntaxKind::Eq,
            b'!' if next == Some(b'=') => two!(SyntaxKind::BangEq),
            b'!' => SyntaxKind::Bang,
            b'<' if next == Some(b'<') && after == Some(b'=') => three!(SyntaxKind::ShlEq),
            b'<' if next == Some(b'<') => two!(SyntaxKind::Shl),
            b'<' if next == Some(b'=') => two!(SyntaxKind::LtEq),
            b'<' => SyntaxKind::Lt,
            b'>' if next == Some(b'>') && after == Some(b'=') => three!(SyntaxKind::ShrEq),
            b'>' if next == Some(b'>') => two!(SyntaxKind::Shr),
            b'>' if next == Some(b'=') => two!(SyntaxKind::GtEq),
            b'>' => SyntaxKind::Gt,
            b'&' if next == Some(b'&') => two!(SyntaxKind::AmpAmp),
            b'&' if next == Some(b'=') => two!(SyntaxKind::AmpEq),
            b'&' => SyntaxKind::Amp,
            b'|' if next == Some(b'|') => two!(SyntaxKind::PipePipe),
            b'|' if next == Some(b'=') => two!(SyntaxKind::PipeEq),
            b'|' => SyntaxKind::Pipe,
            b'^' if next == Some(b'=') => two!(SyntaxKind::CaretEq),
            b'^' => SyntaxKind::Caret,
            b'~' => SyntaxKind::Tilde,
            b':' if next == Some(b'=') => two!(SyntaxKind::ColonEq),
            b':' => SyntaxKind::Colon,
            b';' => SyntaxKind::Semicolon,
            b',' => SyntaxKind::Comma,
            b'.' if next == Some(b'.') && after == Some(b'.') => three!(SyntaxKind::Ellipsis),
            b'.' if next == Some(b'.') => two!(SyntaxKind::DotDot),
            b'.' => SyntaxKind::Dot,
            b'@' => SyntaxKind::At,
            b'$' => SyntaxKind::Dollar,
            b'(' => {
                self.bracket_depth += 1;
                SyntaxKind::LParen
            }
            b')' => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
                SyntaxKind::RParen
            }
            b'[' => {
                self.bracket_depth += 1;
                SyntaxKind::LBracket
            }
            b']' => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
                SyntaxKind::RBracket
            }
            b'{' => {
                self.bracket_depth += 1;
                SyntaxKind::LBrace
            }
            b'}' => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
                SyntaxKind::RBrace
            }
            _ => {
                // Consume the whole UTF-8 sequence so spans stay on char
                // boundaries.
                while self.pos < self.len() && !self.source.is_char_boundary(self.pos as usize) {
                    self.pos += 1;
                }
                self.errors.push(SyntaxError::new(
                    TextRange::new(self.pos - 1, self.pos),
                    "unexpected character",
                ));
                SyntaxKind::Unknown
            }
        }
    }

    // -- Helpers ------------------------------------------------------------

    /// Whether the previous meaningful token could end an expression.
    ///
    /// This is the classic disambiguation trick: `%` after a value is modulo,
    /// but `%` in value position starts a unique-node path. Same idea as
    /// telling regex from division in a JavaScript lexer.
    ///
    /// A newline resets the answer, because the previous line's last token says
    /// nothing about a token starting a fresh statement. Without that, a line
    /// beginning `^"path"` would lex as bitwise-xor whenever the line above it
    /// happened to end in a value.
    fn prev_can_end_expr(&self) -> bool {
        for token in self.tokens.iter().rev() {
            // A line continuation is a distinct kind, so it correctly does not
            // reset this.
            if token.kind == SyntaxKind::Newline {
                return false;
            }
            if token.kind.is_trivia() {
                continue;
            }
            return matches!(
                token.kind,
                SyntaxKind::Ident
                    | SyntaxKind::Int
                    | SyntaxKind::Float
                    | SyntaxKind::Str
                    | SyntaxKind::StringName
                    | SyntaxKind::NodePath
                    | SyntaxKind::GetNode
                    | SyntaxKind::UniqueNode
                    | SyntaxKind::RParen
                    | SyntaxKind::RBracket
                    | SyntaxKind::RBrace
                    | SyntaxKind::SelfKw
                    | SyntaxKind::SuperKw
                    | SyntaxKind::TrueKw
                    | SyntaxKind::FalseKw
                    | SyntaxKind::NullKw
            );
        }
        false
    }

    fn push(&mut self, kind: SyntaxKind, range: TextRange) {
        self.tokens.push(Token { kind, range });
    }

    fn bump_while(&mut self, predicate: impl Fn(u8) -> bool) {
        while let Some(byte) = self.byte_at(self.pos) {
            if !predicate(byte) {
                break;
            }
            self.pos += 1;
        }
    }

    fn byte_at(&self, pos: u32) -> Option<u8> {
        self.bytes.get(pos as usize).copied()
    }

    fn len(&self) -> u32 {
        self.bytes.len() as u32
    }

    fn at_eof(&self) -> bool {
        self.pos >= self.len()
    }
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant the whole formatter depends on.
    fn assert_lossless(source: &str) {
        let lexed = tokenize(source);
        let rebuilt: String = lexed
            .tokens
            .iter()
            .map(|token| token.text(source))
            .collect();
        assert_eq!(rebuilt, source, "token spans must cover the source exactly");
    }

    fn kinds(source: &str) -> Vec<SyntaxKind> {
        tokenize(source)
            .tokens
            .into_iter()
            .map(|token| token.kind)
            .filter(|kind| !kind.is_trivia() && *kind != SyntaxKind::Eof)
            .collect()
    }

    #[test]
    fn round_trips_a_realistic_script() {
        assert_lossless(
            "@tool\nclass_name Player\nextends CharacterBody2D\n\n## Docs.\nsignal died\n\nconst MAX := 100\n\n\nfunc _ready() -> void:\n\tvar x := [1, 2, {\"a\": 1}]  # trailing\n\tif x and true:\n\t\tprint($Sprite2D/Label)\n",
        );
    }

    #[test]
    fn round_trips_edge_cases() {
        for source in [
            "",
            "\n",
            "\n\n\n",
            "pass",
            "\tpass\n",
            "a\r\nb\r\n",
            "var s = \"unterminated\n",
            "if a \\\n\tand b:\n\tpass\n",
            "x = 1 § 2\n",
            "func f():\n\t\t\tpass\n\treturn\n",
        ] {
            assert_lossless(source);
        }
    }

    #[test]
    fn emits_indent_and_dedent_around_blocks() {
        let kinds = kinds("func f():\n\tpass\nvar x = 1\n");
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::FuncKw,
                SyntaxKind::Ident,
                SyntaxKind::LParen,
                SyntaxKind::RParen,
                SyntaxKind::Colon,
                SyntaxKind::Indent,
                SyntaxKind::PassKw,
                SyntaxKind::Dedent,
                SyntaxKind::VarKw,
                SyntaxKind::Ident,
                SyntaxKind::Eq,
                SyntaxKind::Int,
            ]
        );
    }

    #[test]
    fn blank_and_comment_lines_do_not_shift_indentation() {
        // The comment sits at column 0 but must not close the function block.
        let kinds = kinds("func f():\n\tvar a = 1\n\n# note\n\tvar b = 2\n");
        assert_eq!(
            kinds.iter().filter(|k| **k == SyntaxKind::Indent).count(),
            1
        );
        assert_eq!(
            kinds.iter().filter(|k| **k == SyntaxKind::Dedent).count(),
            1
        );
    }

    #[test]
    fn newlines_inside_brackets_are_not_line_breaks() {
        // No indent/dedent should be produced by the wrapped array.
        let kinds = kinds("var a = [\n\t1,\n\t2,\n]\n");
        assert!(!kinds.contains(&SyntaxKind::Indent));
        assert!(!kinds.contains(&SyntaxKind::Dedent));
    }

    #[test]
    fn distinguishes_modulo_from_unique_node() {
        assert_eq!(
            kinds("var a = b % c\n"),
            vec![
                SyntaxKind::VarKw,
                SyntaxKind::Ident,
                SyntaxKind::Eq,
                SyntaxKind::Ident,
                SyntaxKind::Percent,
                SyntaxKind::Ident,
            ]
        );
        assert_eq!(
            kinds("var a = %HealthBar\n"),
            vec![
                SyntaxKind::VarKw,
                SyntaxKind::Ident,
                SyntaxKind::Eq,
                SyntaxKind::UniqueNode,
            ]
        );
        assert_eq!(
            kinds("print(%Bar, a % 2)\n"),
            vec![
                SyntaxKind::Ident,
                SyntaxKind::LParen,
                SyntaxKind::UniqueNode,
                SyntaxKind::Comma,
                SyntaxKind::Ident,
                SyntaxKind::Percent,
                SyntaxKind::Int,
                SyntaxKind::RParen,
            ]
        );
    }

    #[test]
    fn lexes_node_paths_as_single_tokens() {
        assert_eq!(kinds("$Sprite2D\n"), vec![SyntaxKind::GetNode]);
        assert_eq!(kinds("$../Sibling/%Unique\n"), vec![SyntaxKind::GetNode]);
        assert_eq!(kinds("$\"quoted/path\"\n"), vec![SyntaxKind::GetNode]);
        // Attribute access after a path is not part of the path.
        assert_eq!(
            kinds("$Sprite2D.position\n"),
            vec![SyntaxKind::GetNode, SyntaxKind::Dot, SyntaxKind::Ident]
        );
    }

    #[test]
    fn lexes_string_name_and_node_path_literals() {
        assert_eq!(kinds("emit(&\"died\")\n")[2], SyntaxKind::StringName);
        assert_eq!(kinds("var p = ^\"a/b\"\n")[3], SyntaxKind::NodePath);
        // With a value to its left, `&` is still bitwise-and.
        assert_eq!(kinds("var x = a & b\n")[4], SyntaxKind::Amp);
    }

    #[test]
    fn lexes_number_forms() {
        assert_eq!(kinds("1_000_000"), vec![SyntaxKind::Int]);
        assert_eq!(kinds("0xfb8c0b"), vec![SyntaxKind::Int]);
        assert_eq!(kinds("0b1010_1010"), vec![SyntaxKind::Int]);
        assert_eq!(kinds("0.234"), vec![SyntaxKind::Float]);
        assert_eq!(kinds("1e-5"), vec![SyntaxKind::Float]);
        assert_eq!(kinds("1.5e10"), vec![SyntaxKind::Float]);
        // A range, not a float followed by a number.
        assert_eq!(
            kinds("1..2"),
            vec![SyntaxKind::Int, SyntaxKind::DotDot, SyntaxKind::Int]
        );
        // Method call on an integer literal.
        assert_eq!(
            kinds("1.max(2)"),
            vec![
                SyntaxKind::Int,
                SyntaxKind::Dot,
                SyntaxKind::Ident,
                SyntaxKind::LParen,
                SyntaxKind::Int,
                SyntaxKind::RParen
            ]
        );
    }

    #[test]
    fn lexes_string_forms() {
        assert_eq!(kinds("\"double\""), vec![SyntaxKind::Str]);
        assert_eq!(kinds("'single'"), vec![SyntaxKind::Str]);
        assert_eq!(kinds("\"\"\"triple\nspanning\"\"\""), vec![SyntaxKind::Str]);
        assert_eq!(kinds("r\"raw\\n\""), vec![SyntaxKind::Str]);
        assert_eq!(kinds("\"esc\\\"aped\""), vec![SyntaxKind::Str]);
    }

    #[test]
    fn separates_doc_comments_from_plain_comments() {
        let lexed = tokenize("## doc\n# plain\n");
        let comments: Vec<_> = lexed
            .tokens
            .iter()
            .filter(|t| t.kind.is_comment())
            .map(|t| t.kind)
            .collect();
        assert_eq!(comments, vec![SyntaxKind::DocComment, SyntaxKind::Comment]);
    }

    #[test]
    fn line_continuation_joins_lines() {
        let kinds = kinds("var a = 1 + \\\n\t2\n");
        assert!(!kinds.contains(&SyntaxKind::Indent));
        assert_eq!(kinds.last(), Some(&SyntaxKind::Int));
    }

    #[test]
    fn reports_inconsistent_dedent() {
        let lexed = tokenize("func f():\n\t\tpass\n\treturn\n");
        assert!(
            lexed
                .errors
                .iter()
                .any(|e| e.message().contains("unindent")),
            "expected an unindent diagnostic, got {:?}",
            lexed.errors
        );
    }

    #[test]
    fn reports_unterminated_string() {
        let lexed = tokenize("var s = \"oops\n");
        assert!(
            lexed
                .errors
                .iter()
                .any(|e| e.message().contains("unterminated"))
        );
    }

    #[test]
    fn closes_open_blocks_at_eof() {
        let lexed = tokenize("func f():\n\tif a:\n\t\tpass");
        let dedents = lexed
            .tokens
            .iter()
            .filter(|t| t.kind == SyntaxKind::Dedent)
            .count();
        assert_eq!(dedents, 2);
        assert_eq!(lexed.tokens.last().map(|t| t.kind), Some(SyntaxKind::Eof));
    }
}
