//! A Wadler-style document IR and its renderer.
//!
//! Lowering produces a [`Doc`] describing *where* a line could break rather
//! than where it does. The renderer then decides, outermost group first,
//! whether each group fits on the remaining width. This is the standard
//! arrangement (Wadler's "A prettier printer", by way of Prettier), and it is
//! what keeps the layout rules in one place instead of scattered through the
//! lowering of every construct.
//!
//! The one GDScript-specific wrinkle is indentation width. The style guide
//! mandates tabs, so the rendered text contains tabs, but line length has to be
//! measured in display columns. [`TAB_WIDTH`] resolves that, and matches the
//! value the lexer uses when comparing indentation depth.

use gdck_config::IndentStyle;

/// Display width of a tab when measuring line length.
///
/// Matches the lexer's value, so what the parser considers one indent level and
/// what the formatter counts as four columns cannot drift apart.
pub(crate) const TAB_WIDTH: usize = 4;

/// A document: text with the possible line breaks marked.
#[derive(Debug, Clone)]
pub(crate) struct Doc {
    kind: DocKind,
    /// Whether this document contains a break that cannot be flattened.
    ///
    /// Cached at construction rather than computed on demand, which keeps
    /// building a document linear in its size instead of quadratic.
    hard: bool,
}

#[derive(Debug, Clone)]
enum DocKind {
    Nil,
    /// Literal text. Must not contain a newline.
    Text(String),
    Concat(Vec<Doc>),
    /// A space when flat, a newline when broken.
    Line,
    /// Nothing when flat, a newline when broken.
    SoftLine,
    /// Always a newline, and forces every enclosing group to break.
    HardLine,
    /// A break candidate: rendered flat if it fits, broken otherwise.
    Group(Box<Doc>),
    /// Adds `levels` indent levels to everything inside.
    Indent(u8, Box<Doc>),
    /// Chooses between two documents based on the enclosing group's mode.
    IfBreak(Box<Doc>, Box<Doc>),
    /// Renders its contents on one line whatever the width.
    Flat(Box<Doc>),
    /// Text that may itself span lines, written out exactly as given.
    Verbatim(String),
}

impl Doc {
    pub(crate) fn nil() -> Self {
        Self {
            kind: DocKind::Nil,
            hard: false,
        }
    }

    /// # Panics
    ///
    /// Panics in debug builds if `text` contains a newline. Line breaks must be
    /// expressed as [`Doc::hard_line`] and friends, or the renderer's column
    /// tracking silently goes wrong.
    pub(crate) fn text(text: impl Into<String>) -> Self {
        let text = text.into();
        debug_assert!(
            !text.contains('\n'),
            "Doc::text must not contain a newline: {text:?}"
        );
        Self {
            kind: DocKind::Text(text),
            hard: false,
        }
    }

    /// Text that may contain newlines of its own, such as a triple-quoted
    /// string.
    ///
    /// The renderer writes it unchanged and resumes counting columns from its
    /// last line, since the earlier ones are no longer the current line.
    pub(crate) fn verbatim(text: impl Into<String>) -> Self {
        Self {
            kind: DocKind::Verbatim(text.into()),
            hard: false,
        }
    }

    /// A literal's text, whichever form it takes.
    pub(crate) fn literal(text: impl Into<String>) -> Self {
        let text = text.into();
        if text.contains('\n') {
            Self::verbatim(text)
        } else {
            Self::text(text)
        }
    }

    pub(crate) fn concat(parts: Vec<Doc>) -> Self {
        let hard = parts.iter().any(|part| part.hard);
        Self {
            kind: DocKind::Concat(parts),
            hard,
        }
    }

    pub(crate) fn line() -> Self {
        Self {
            kind: DocKind::Line,
            hard: false,
        }
    }

    pub(crate) fn soft_line() -> Self {
        Self {
            kind: DocKind::SoftLine,
            hard: false,
        }
    }

    pub(crate) fn hard_line() -> Self {
        Self {
            kind: DocKind::HardLine,
            hard: true,
        }
    }

    /// Emits nothing, but forces every group containing it to break.
    ///
    /// Used where the author's own line breaks are being honoured: the style
    /// guide shows both `var array = [1, 2, 3]` and the same array spread over
    /// four lines as good, so which one a file gets is the author's call and
    /// not something the column limit should overrule.
    pub(crate) fn break_parent() -> Self {
        Self {
            kind: DocKind::Nil,
            hard: true,
        }
    }

    pub(crate) fn group(inner: Doc) -> Self {
        let hard = inner.hard;
        Self {
            kind: DocKind::Group(Box::new(inner)),
            hard,
        }
    }

    pub(crate) fn indent(inner: Doc) -> Self {
        Self::indent_by(1, inner)
    }

    /// Indent by several levels at once.
    ///
    /// The style guide asks for two levels on continuation lines, to tell them
    /// apart from the block that follows, so this is not just `indent` twice
    /// for convenience — it is a rule with its own call sites.
    pub(crate) fn indent_by(levels: u8, inner: Doc) -> Self {
        let hard = inner.hard;
        Self {
            kind: DocKind::Indent(levels, Box::new(inner)),
            hard,
        }
    }

    /// Render `inner` on one line, ignoring the width.
    ///
    /// For the places GDScript gives no legal way to break: a `match` pattern
    /// is not inside brackets, so a line break in one is a syntax error rather
    /// than a long line.
    pub(crate) fn flat(inner: Doc) -> Self {
        Self {
            kind: DocKind::Flat(Box::new(inner)),
            hard: false,
        }
    }

    /// Whether this is the single space used to separate adjacent pieces.
    ///
    /// Lets a caller take a separator back when it turns out a line break is
    /// needed there instead.
    pub(crate) fn is_space(&self) -> bool {
        matches!(&self.kind, DocKind::Text(text) if text == " ")
    }

    pub(crate) fn if_break(broken: Doc, flat: Doc) -> Self {
        // Deliberately not inheriting `hard` from either branch: a document
        // that only appears when its group is already broken cannot itself be
        // the reason for breaking.
        Self {
            kind: DocKind::IfBreak(Box::new(broken), Box::new(flat)),
            hard: false,
        }
    }
}

/// Join `parts` with `separator` between each pair.
pub(crate) fn join(parts: Vec<Doc>, separator: &Doc) -> Doc {
    let mut out = Vec::with_capacity(parts.len().saturating_mul(2));
    for (index, part) in parts.into_iter().enumerate() {
        if index > 0 {
            out.push(separator.clone());
        }
        out.push(part);
    }
    Doc::concat(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Flat,
    Break,
}

/// One unit of pending work: what to render, at what indent, in which mode.
type Cmd<'a> = (u8, Mode, &'a Doc);

/// Render a document to text.
// One dispatch over the document kinds. Splitting it would mean passing the
// output buffer, column and pending indent between helpers, which reads worse
// than the loop does.
#[allow(clippy::too_many_lines)]
pub(crate) fn render(doc: &Doc, width: usize, style: IndentStyle) -> String {
    let unit = match style {
        IndentStyle::Tabs => "\t".to_string(),
        IndentStyle::Spaces(n) => " ".repeat(n as usize),
    };
    let unit_width = match style {
        IndentStyle::Tabs => TAB_WIDTH,
        IndentStyle::Spaces(n) => n as usize,
    };

    let mut out = String::new();
    let mut column = 0usize;
    // Indentation is written just before the next piece of text rather than
    // straight after the newline, so a line with nothing on it stays empty
    // instead of collecting trailing whitespace.
    let mut pending_indent: Option<u8> = None;
    let mut stack: Vec<Cmd<'_>> = vec![(0, Mode::Break, doc)];

    while let Some((indent, mode, doc)) = stack.pop() {
        match &doc.kind {
            DocKind::Nil => {}
            DocKind::Text(text) => {
                if let Some(level) = pending_indent.take() {
                    for _ in 0..level {
                        out.push_str(&unit);
                    }
                }
                out.push_str(text);
                column += display_width(text);
            }
            DocKind::Concat(parts) => {
                for part in parts.iter().rev() {
                    stack.push((indent, mode, part));
                }
            }
            DocKind::Indent(levels, inner) => {
                stack.push((indent.saturating_add(*levels), mode, inner));
            }
            DocKind::Group(inner) => {
                let inner_mode = if mode == Mode::Flat {
                    // Already inside something rendering on one line, so this
                    // group has no say. That is what makes `Doc::flat` a
                    // guarantee rather than a preference.
                    Mode::Flat
                } else if doc.hard {
                    Mode::Break
                } else if fits(
                    width.saturating_sub(column),
                    (indent, Mode::Flat, inner),
                    &stack,
                ) {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                stack.push((indent, inner_mode, inner));
            }
            DocKind::Line => match mode {
                Mode::Flat => {
                    if let Some(level) = pending_indent.take() {
                        for _ in 0..level {
                            out.push_str(&unit);
                        }
                    }
                    out.push(' ');
                    column += 1;
                }
                Mode::Break => {
                    new_line(
                        &mut out,
                        &mut column,
                        &mut pending_indent,
                        indent,
                        unit_width,
                    );
                }
            },
            DocKind::SoftLine => {
                if mode == Mode::Break {
                    new_line(
                        &mut out,
                        &mut column,
                        &mut pending_indent,
                        indent,
                        unit_width,
                    );
                }
            }
            DocKind::HardLine => {
                new_line(
                    &mut out,
                    &mut column,
                    &mut pending_indent,
                    indent,
                    unit_width,
                );
            }
            DocKind::IfBreak(broken, flat) => {
                let chosen = if mode == Mode::Break { broken } else { flat };
                stack.push((indent, mode, chosen));
            }
            DocKind::Flat(inner) => stack.push((indent, Mode::Flat, inner)),
            DocKind::Verbatim(text) => {
                if let Some(level) = pending_indent.take() {
                    for _ in 0..level {
                        out.push_str(&unit);
                    }
                }
                out.push_str(text);
                // Columns resume from whatever follows the last newline; the
                // lines before it are behind us.
                column = match text.rsplit_once('\n') {
                    Some((_, last)) => display_width(last),
                    None => column + display_width(text),
                };
            }
        }
    }

    // A pending indent at the end means the document finished with a line
    // break, which is exactly what we want: no trailing whitespace.
    out
}

fn new_line(
    out: &mut String,
    column: &mut usize,
    pending_indent: &mut Option<u8>,
    indent: u8,
    unit_width: usize,
) {
    out.push('\n');
    *pending_indent = Some(indent);
    *column = indent as usize * unit_width;
}

/// Whether `first`, followed by whatever is already queued, reaches a line
/// break within `remaining` columns.
///
/// Looking past the group being measured matters: `foo(bar)` only fits if the
/// closing parenthesis and everything trailing it fit too.
#[allow(clippy::cast_possible_wrap)]
fn fits(remaining: usize, first: Cmd<'_>, rest: &[Cmd<'_>]) -> bool {
    // The caller passes a saturating subtraction of two column counts, so this
    // is far from the wrapping range; it goes signed so overflow is detectable.
    let mut remaining = remaining as isize;
    let mut queue: Vec<Cmd<'_>> = vec![first];
    // `rest` is a stack, so its last element is the next one to be processed.
    let mut rest_index = rest.len();

    loop {
        if remaining < 0 {
            return false;
        }

        let (indent, mode, doc) = if let Some(cmd) = queue.pop() {
            cmd
        } else {
            // Nothing left of the group being measured, so carry on through
            // what was already queued. `rest` is a stack, so it is read from
            // the end.
            if rest_index == 0 {
                return true;
            }
            rest_index -= 1;
            rest[rest_index]
        };

        match &doc.kind {
            DocKind::Nil => {}
            DocKind::Text(text) => remaining -= display_width(text) as isize,
            DocKind::Concat(parts) => {
                for part in parts.iter().rev() {
                    queue.push((indent, mode, part));
                }
            }
            DocKind::Indent(levels, inner) => {
                queue.push((indent.saturating_add(*levels), mode, inner));
            }
            DocKind::Group(inner) => {
                // A group that must break ends the line, so measuring stops
                // there rather than pretending its contents are flat.
                let inner_mode = if doc.hard { Mode::Break } else { Mode::Flat };
                queue.push((indent, inner_mode, inner));
            }
            DocKind::Line => match mode {
                Mode::Flat => remaining -= 1,
                // Reaching a break means the rest goes on another line.
                Mode::Break => return true,
            },
            DocKind::SoftLine => {
                if mode == Mode::Break {
                    return true;
                }
            }
            DocKind::HardLine => return true,
            DocKind::IfBreak(broken, flat) => {
                let chosen = if mode == Mode::Break { broken } else { flat };
                queue.push((indent, mode, chosen));
            }
            DocKind::Flat(inner) => queue.push((indent, Mode::Flat, inner)),
            DocKind::Verbatim(text) => {
                // A literal that breaks its own line ends the measurement, the
                // same as reaching any other break.
                if text.contains('\n') {
                    return true;
                }
                remaining -= display_width(text) as isize;
            }
        }
    }
}

/// Width of a string in display columns.
///
/// Counts characters rather than grapheme clusters, so a line whose overflow
/// depends on combining marks or East Asian width may be measured slightly
/// short. Getting that exactly right needs a Unicode table, and the cost of
/// being wrong here is one line a couple of columns over the limit.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|c| if c == '\t' { TAB_WIDTH } else { 1 })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_tabs(doc: &Doc, width: usize) -> String {
        render(doc, width, IndentStyle::Tabs)
    }

    #[test]
    fn a_group_that_fits_stays_flat() {
        let doc = Doc::group(Doc::concat(vec![
            Doc::text("f("),
            Doc::soft_line(),
            Doc::text("a"),
            Doc::soft_line(),
            Doc::text(")"),
        ]));
        assert_eq!(render_tabs(&doc, 100), "f(a)");
    }

    #[test]
    fn a_group_that_does_not_fit_breaks() {
        let doc = Doc::group(Doc::concat(vec![
            Doc::text("f("),
            Doc::indent(Doc::concat(vec![Doc::soft_line(), Doc::text("argument")])),
            Doc::soft_line(),
            Doc::text(")"),
        ]));
        assert_eq!(render_tabs(&doc, 5), "f(\n\targument\n)");
    }

    #[test]
    fn a_hard_line_forces_every_enclosing_group() {
        let doc = Doc::group(Doc::concat(vec![
            Doc::text("a"),
            Doc::line(),
            Doc::hard_line(),
            Doc::text("b"),
        ]));
        // Plenty of room, but the hard line still breaks the group, so the
        // soft `line` before it breaks too.
        assert_eq!(render_tabs(&doc, 100), "a\n\nb");
    }

    #[test]
    fn blank_lines_carry_no_trailing_whitespace() {
        // Two breaks in a row leave the middle line genuinely empty, because
        // indentation is written just before text rather than after a newline.
        let doc = Doc::indent(Doc::concat(vec![
            Doc::hard_line(),
            Doc::text("a"),
            Doc::hard_line(),
            Doc::hard_line(),
            Doc::text("b"),
        ]));
        assert_eq!(render_tabs(&doc, 100), "\n\ta\n\n\tb");
    }

    #[test]
    fn if_break_picks_the_branch_matching_the_group() {
        let trailing_comma = Doc::if_break(Doc::text(","), Doc::nil());
        let build = |width| {
            let doc = Doc::group(Doc::concat(vec![
                Doc::text("["),
                Doc::indent(Doc::concat(vec![
                    Doc::soft_line(),
                    Doc::text("1"),
                    trailing_comma.clone(),
                ])),
                Doc::soft_line(),
                Doc::text("]"),
            ]));
            render_tabs(&doc, width)
        };
        assert_eq!(build(100), "[1]");
        assert_eq!(build(2), "[\n\t1,\n]");
    }

    #[test]
    fn fits_accounts_for_text_queued_after_the_group() {
        // The group alone is 5 columns and would fit in 8, but the trailing
        // text pushes the line over, so it has to break.
        let doc = Doc::concat(vec![
            Doc::group(Doc::concat(vec![
                Doc::text("("),
                Doc::soft_line(),
                Doc::text("ab"),
                Doc::soft_line(),
                Doc::text(")"),
            ])),
            Doc::text(" trailing"),
        ]);
        assert_eq!(render_tabs(&doc, 8), "(\nab\n) trailing");
    }

    #[test]
    fn indentation_follows_the_configured_style() {
        let doc = Doc::indent(Doc::concat(vec![Doc::hard_line(), Doc::text("x")]));
        assert_eq!(render(&doc, 100, IndentStyle::Tabs), "\n\tx");
        assert_eq!(render(&doc, 100, IndentStyle::Spaces(4)), "\n    x");
    }

    #[test]
    fn two_indent_levels_are_distinct_from_one() {
        let doc = Doc::indent_by(2, Doc::concat(vec![Doc::hard_line(), Doc::text("x")]));
        assert_eq!(render_tabs(&doc, 100), "\n\t\tx");
    }

    #[test]
    fn a_tab_counts_as_four_columns_when_measuring() {
        // One indent level plus "abcdef" is 4 + 6 = 10 columns, so a width of
        // 9 must break but 10 must not.
        let build = |width| {
            let doc = Doc::indent(Doc::concat(vec![
                Doc::hard_line(),
                Doc::group(Doc::concat(vec![
                    Doc::text("abcdef"),
                    Doc::soft_line(),
                    Doc::text("g"),
                ])),
            ]));
            render_tabs(&doc, width)
        };
        assert_eq!(build(11), "\n\tabcdefg");
        assert_eq!(build(10), "\n\tabcdef\n\tg");
    }
}
