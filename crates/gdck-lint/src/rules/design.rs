//! Size thresholds: `max-file-lines`, `max-public-methods`, `max-returns` and
//! `max-arguments`.
//!
//! None of these come from the style guide. They are `gdtoolkit`'s defaults,
//! kept to the number so that a project already configured against that linter
//! gets the same answers from this one. All four are configurable, and a
//! threshold that has to be argued about is a threshold that should be set in
//! the project rather than defended here.

use gdck_syntax::{SyntaxKind, SyntaxNode, TextRange};

use super::{Context, Sink, class_bodies, name_token, significant_range};

pub(crate) fn check(context: &Context<'_>, sink: &mut Sink) {
    check_file_lines(context, sink);
    check_public_methods(context, sink);

    for node in context.root().descendants() {
        if node.kind() == SyntaxKind::FuncDecl {
            check_returns(context, sink, node);
            check_arguments(context, sink, node);
        }
    }
}

fn check_file_lines(context: &Context<'_>, sink: &mut Sink) {
    let limit = context.config.max_file_lines as usize;
    // A file ending in a line feed does not have an empty last line.
    let lines = context.source.lines().count();
    if lines <= limit {
        return;
    }
    sink.report(
        "max-file-lines",
        TextRange::empty(0),
        format!("file is {lines} lines long, over the limit of {limit}"),
    );
}

/// Counted per class, since the file and each inner class are separate types
/// with separate interfaces.
fn check_public_methods(context: &Context<'_>, sink: &mut Sink) {
    let limit = context.config.max_public_methods as usize;

    for body in class_bodies(context.root()) {
        let public: Vec<SyntaxNode<'_>> = body
            .child_nodes()
            .filter(|node| node.kind() == SyntaxKind::FuncDecl)
            .filter(|node| {
                name_token(*node).is_some_and(|token| !context.token_text(token).starts_with('_'))
            })
            .collect();
        if public.len() <= limit {
            continue;
        }
        // Anchored on the method that crossed the line, which is the one worth
        // looking at.
        let range = significant_range(public[limit]);
        sink.report(
            "max-public-methods",
            range,
            format!(
                "class has {} public methods, over the limit of {limit}",
                public.len()
            ),
        );
    }
}

fn check_returns(context: &Context<'_>, sink: &mut Sink, func: SyntaxNode<'_>) {
    let limit = context.config.max_returns as usize;
    // Lambdas have their own returns, and counting those against the function
    // holding them would report the wrong body.
    let returns: Vec<SyntaxNode<'_>> = func
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::ReturnStmt)
        .filter(|node| enclosing_body(func, *node))
        .collect();
    if returns.len() <= limit {
        return;
    }
    sink.report(
        "max-returns",
        significant_range(returns[limit]),
        format!(
            "function has {} return statements, over the limit of {limit}",
            returns.len()
        ),
    );
}

/// Whether `func` is the nearest function or lambda enclosing `node`.
fn enclosing_body(func: SyntaxNode<'_>, node: SyntaxNode<'_>) -> bool {
    !func
        .descendants()
        .filter(|inner| inner.kind() == SyntaxKind::LambdaExpr)
        .any(|lambda| {
            lambda.range().start() <= node.range().start()
                && node.range().end() <= lambda.range().end()
        })
}

fn check_arguments(context: &Context<'_>, sink: &mut Sink, func: SyntaxNode<'_>) {
    let limit = context.config.max_function_arguments as usize;
    let Some(params) = func.child_node_of(SyntaxKind::ParamList) else {
        return;
    };
    let count = params
        .child_nodes()
        .filter(|node| node.kind() == SyntaxKind::Param)
        .count();
    if count <= limit {
        return;
    }
    let name = name_token(func).map_or(params.range(), |token| token.range);
    sink.report(
        "max-arguments",
        name,
        format!("function takes {count} arguments, over the limit of {limit}"),
    );
}

#[cfg(test)]
mod tests {
    use gdck_config::LintConfig;

    fn fired_with(config: &LintConfig, source: &str) -> Vec<&'static str> {
        crate::lint(&gdck_syntax::parse(source), config)
            .into_iter()
            .map(|diagnostic| diagnostic.rule)
            .collect()
    }

    /// Only the rule under test, so a fixture written to trip a threshold does
    /// not also have to be idiomatic.
    fn only(rule: &str) -> LintConfig {
        LintConfig {
            disabled: crate::RULES
                .iter()
                .map(|entry| entry.name.to_string())
                .filter(|name| name != rule)
                .collect(),
            ..LintConfig::default()
        }
    }

    #[test]
    fn a_long_file_is_reported_once() {
        let config = LintConfig {
            max_file_lines: 3,
            ..only("max-file-lines")
        };
        assert_eq!(fired_with(&config, "var a = 1\n"), Vec::<&str>::new());
        assert_eq!(
            fired_with(&config, "var a = 1\nvar b = 2\nvar c = 3\nvar d = 4\n"),
            ["max-file-lines"]
        );
    }

    #[test]
    fn public_methods_are_counted_and_private_ones_are_not() {
        let config = LintConfig {
            max_public_methods: 1,
            ..only("max-public-methods")
        };
        assert_eq!(
            fired_with(&config, "func a():\n\tpass\n\n\nfunc _b():\n\tpass\n"),
            Vec::<&str>::new()
        );
        assert_eq!(
            fired_with(&config, "func a():\n\tpass\n\n\nfunc b():\n\tpass\n"),
            ["max-public-methods"]
        );
    }

    #[test]
    fn an_inner_class_is_counted_separately() {
        let config = LintConfig {
            max_public_methods: 1,
            ..only("max-public-methods")
        };
        assert_eq!(
            fired_with(
                &config,
                "func a():\n\tpass\n\n\nclass Inner:\n\tfunc b():\n\t\tpass\n"
            ),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn returns_are_counted_per_function_body() {
        let config = LintConfig {
            max_returns: 1,
            ..only("max-returns")
        };
        assert_eq!(
            fired_with(&config, "func f(a):\n\tif a:\n\t\treturn 1\n\treturn 2\n"),
            ["max-returns"]
        );
        // A lambda's returns belong to the lambda.
        assert_eq!(
            fired_with(
                &config,
                "func f(a):\n\tvar g = func():\n\t\tif a:\n\t\t\treturn 1\n\t\treturn 2\n\treturn g\n"
            ),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn arguments_are_counted() {
        let config = LintConfig {
            max_function_arguments: 2,
            ..only("max-arguments")
        };
        assert_eq!(
            fired_with(&config, "func f(a, b):\n\tprint(a, b)\n"),
            Vec::<&str>::new()
        );
        assert_eq!(
            fired_with(&config, "func f(a, b, c):\n\tprint(a, b, c)\n"),
            ["max-arguments"]
        );
    }

    #[test]
    fn the_defaults_are_gdtoolkits() {
        let config = LintConfig::default();
        assert_eq!(config.max_file_lines, 1000);
        assert_eq!(config.max_public_methods, 20);
        assert_eq!(config.max_returns, 6);
        assert_eq!(config.max_function_arguments, 10);
    }
}
