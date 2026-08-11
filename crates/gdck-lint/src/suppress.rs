//! Turning off rules from within the file.
//!
//! The syntax is `gdtoolkit`'s, because GDScript projects already have these
//! comments in them and a linter that ignored them would report a wall of
//! problems the author had already decided about:
//!
//! * `# gdlint: ignore=a,b` silences those rules on its own line and the one
//!   below, so it works both as a trailing comment and as a comment written
//!   above the line it excuses.
//! * `# gdlint: disable=a` silences `a` from there to the matching
//!   `# gdlint: enable=a`, or to the end of the file. A directive that trails
//!   code takes effect on the *next* line, since the line it sits on has
//!   already been written.
//!
//! Directives are read from the comment tokens in the tree rather than by
//! scanning lines, so a `#` inside a string literal cannot be mistaken for one.

use std::collections::HashMap;

use gdck_syntax::{Element, LineIndex, SyntaxKind, SyntaxNode, SyntaxTree};

/// Which rules are switched off on which lines.
#[derive(Debug, Default)]
pub(crate) struct Suppressions {
    /// Canonical rule name to the inclusive 1-based line ranges it is off in.
    off: HashMap<&'static str, Vec<(u32, u32)>>,
}

impl Suppressions {
    pub(crate) fn collect(tree: &SyntaxTree) -> Self {
        let source = tree.text();
        let lines = LineIndex::new(source);
        let last_line = lines.line_count() as u32;

        let mut suppressions = Self::default();
        // Rules disabled but not yet re-enabled, and where each started.
        let mut open: HashMap<&'static str, u32> = HashMap::new();

        for token in comment_tokens(tree.root()) {
            let text = token.text(source);
            let Some(directive) = parse_directive(text) else {
                continue;
            };
            let line = lines.line_col(token.range.start()).line;
            // Whether anything but whitespace precedes the comment on its line.
            let line_start = source[..token.range.start() as usize]
                .rfind('\n')
                .map_or(0, |index| index + 1);
            let trails_code = !source[line_start..token.range.start() as usize]
                .trim()
                .is_empty();

            for name in directive.rules {
                let Some(rule) = crate::rule(name) else {
                    // A name no rule answers to can never match a diagnostic,
                    // so there is nothing to record. Reporting it as a mistake
                    // would fight with projects that share one `gdlintrc`
                    // across both linters.
                    continue;
                };
                match directive.kind {
                    Kind::Ignore => suppressions.add(rule.name, line, line + 1),
                    Kind::Disable => {
                        let from = if trails_code { line + 1 } else { line };
                        open.entry(rule.name).or_insert(from);
                    }
                    Kind::Enable => {
                        if let Some(from) = open.remove(rule.name) {
                            suppressions.add(rule.name, from, line);
                        }
                    }
                }
            }
        }

        for (rule, from) in open {
            suppressions.add(rule, from, last_line);
        }
        suppressions
    }

    fn add(&mut self, rule: &'static str, from: u32, to: u32) {
        self.off.entry(rule).or_default().push((from, to));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.off.is_empty()
    }

    pub(crate) fn suppresses(&self, rule: &str, line: u32) -> bool {
        self.off
            .get(rule)
            .is_some_and(|ranges| ranges.iter().any(|(from, to)| line >= *from && line <= *to))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ignore,
    Disable,
    Enable,
}

#[derive(Debug)]
struct Directive<'a> {
    kind: Kind,
    rules: Vec<&'a str>,
}

const KINDS: &[(&str, Kind)] = &[
    ("ignore", Kind::Ignore),
    ("disable", Kind::Disable),
    ("enable", Kind::Enable),
];

/// Read `# gdlint: <kind>=<names>` out of one comment's text.
///
/// Whitespace is allowed anywhere `gdtoolkit` allows it, since the point is to
/// accept the comments already written rather than to define a new syntax.
fn parse_directive(comment: &str) -> Option<Directive<'_>> {
    let rest = comment.trim_start_matches('#').trim_start();
    let rest = rest.strip_prefix("gdlint")?.trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();

    let (kind, rest) = KINDS
        .iter()
        .find_map(|(word, kind)| rest.strip_prefix(word).map(|rest| (*kind, rest)))?;

    let rest = rest.trim_start().strip_prefix('=')?;
    let rules: Vec<&str> = rest
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    if rules.is_empty() {
        return None;
    }
    Some(Directive { kind, rules })
}

fn comment_tokens(node: SyntaxNode<'_>) -> Vec<gdck_syntax::Token> {
    let mut out = Vec::new();
    push_comments(node, &mut out);
    out
}

fn push_comments(node: SyntaxNode<'_>, out: &mut Vec<gdck_syntax::Token>) {
    for element in node.children() {
        match element {
            Element::Token(token) if token.kind == SyntaxKind::Comment => out.push(token),
            Element::Token(_) => {}
            Element::Node(id) => push_comments(node.tree().node(id), out),
        }
    }
}

#[cfg(test)]
mod tests {
    use gdck_config::LintConfig;

    fn fired(source: &str) -> Vec<&'static str> {
        crate::lint(&gdck_syntax::parse(source), &LintConfig::default())
            .into_iter()
            .map(|diagnostic| diagnostic.rule)
            .collect()
    }

    #[test]
    fn ignore_silences_its_own_line() {
        assert_eq!(fired("var BadName = 1\n"), ["variable-name"]);
        assert_eq!(
            fired("var BadName = 1 # gdlint: ignore=variable-name\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn ignore_also_silences_the_line_below() {
        // So the directive can be written above what it excuses, which is what
        // you need when the line is already long.
        assert_eq!(
            fired("# gdlint: ignore=variable-name\nvar BadName = 1\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn ignore_does_not_reach_further_than_one_line() {
        assert_eq!(
            fired("# gdlint: ignore=variable-name\nvar BadName = 1\nvar AlsoBad = 2\n"),
            ["variable-name"]
        );
    }

    #[test]
    fn disable_and_enable_bound_a_region() {
        assert_eq!(
            fired(
                "# gdlint: disable=variable-name\nvar BadName = 1\nvar AlsoBad = 2\n# gdlint: enable=variable-name\nvar StillBad = 3\n"
            ),
            ["variable-name"]
        );
    }

    #[test]
    fn disable_without_enable_runs_to_the_end_of_the_file() {
        assert_eq!(
            fired("# gdlint: disable=variable-name\nvar BadName = 1\nvar AlsoBad = 2\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_disable_trailing_code_starts_on_the_next_line() {
        // The line it sits on has already been written, so the author is
        // talking about what follows.
        assert_eq!(
            fired("var BadName = 1 # gdlint: disable=variable-name\nvar AlsoBad = 2\n"),
            ["variable-name"]
        );
    }

    #[test]
    fn several_rules_can_be_named_at_once() {
        assert_eq!(
            fired("var BadName = 1   # gdlint: ignore=variable-name, trailing-whitespace\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn gdtoolkit_rule_names_are_accepted() {
        assert_eq!(
            fired("var BadName = 1 # gdlint: ignore=class-variable-name\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_directive() {
        // Reading comments from the tree rather than scanning lines is what
        // makes this work.
        assert_eq!(
            fired("var BadName = \"# gdlint: ignore=variable-name\"\n"),
            ["variable-name"]
        );
    }

    #[test]
    fn an_unrelated_comment_is_left_alone() {
        assert_eq!(
            fired("# gdlint is a good tool\nvar BadName = 1\n"),
            ["variable-name"]
        );
    }
}
