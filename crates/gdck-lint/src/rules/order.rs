//! The style guide's declaration order.
//!
//! The guide gives a numbered list, and this turns it into buckets a
//! declaration can be sorted into. Everything in the list that a linter can
//! actually tell apart is here; what is left out is left out because no amount
//! of care would make it reliable.
//!
//! The list separates "overridden built-in virtual methods" from "overridden
//! custom methods" from "remaining methods". Deciding which of those a
//! function is means knowing the whole inheritance chain, including the parent
//! script and Godot's own API. `gdck` orders the five virtual callbacks the
//! guide names by name — `_init`, `_enter_tree`, `_ready`, `_process`,
//! `_physics_process` — and treats every other function as one bucket. That
//! reproduces the guide's own worked example exactly, and never claims a
//! function is misplaced on a guess about what it overrides.
//!
//! The guide also asks for public before private. That is applied to variables,
//! where it is unambiguous, and not to methods, where it would contradict the
//! rule above it: a virtual callback is private by name and still comes first.

use std::collections::HashSet;

use gdck_config::{DeclarationGroup, LintConfig};
use gdck_syntax::{SyntaxKind, SyntaxNode, TextRange};

use super::{Context, Sink, class_bodies, name_token, significant_range, significant_tokens};
use crate::Reorder;

/// Where a declaration belongs, in the order the guide gives.
///
/// The enum's own order is the file's order, which is what makes the check a
/// comparison rather than a table lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Bucket {
    /// `@tool`, `@icon`, `@static_unload`, `@abstract`.
    FileAnnotation,
    ClassName,
    Extends,
    /// A bare string used as documentation.
    Docstring,
    Signal,
    Enum,
    Constant,
    StaticVar,
    ExportVar,
    PublicVar,
    PrivateVar,
    OnreadyPublicVar,
    OnreadyPrivateVar,
    StaticInit,
    StaticFunc,
    Init,
    EnterTree,
    Ready,
    Process,
    PhysicsProcess,
    Func,
    InnerClass,
}

impl Bucket {
    /// How a message names a group of these.
    fn label(self) -> &'static str {
        match self {
            Self::FileAnnotation => "file annotations",
            Self::ClassName => "`class_name`",
            Self::Extends => "`extends`",
            Self::Docstring => "the class docstring",
            Self::Signal => "signals",
            Self::Enum => "enums",
            Self::Constant => "constants",
            Self::StaticVar => "static variables",
            Self::ExportVar => "`@export` variables",
            Self::PublicVar => "public variables",
            Self::PrivateVar => "private variables",
            Self::OnreadyPublicVar => "public `@onready` variables",
            Self::OnreadyPrivateVar => "private `@onready` variables",
            Self::StaticInit => "`_static_init()`",
            Self::StaticFunc => "static functions",
            Self::Init => "`_init()`",
            Self::EnterTree => "`_enter_tree()`",
            Self::Ready => "`_ready()`",
            Self::Process => "`_process()`",
            Self::PhysicsProcess => "`_physics_process()`",
            Self::Func => "functions",
            Self::InnerClass => "inner classes",
        }
    }
}

pub(crate) fn check(context: &Context<'_>, sink: &mut Sink) {
    if context.config.code_order == gdck_config::CodeOrderFix::Off {
        return;
    }

    for body in class_bodies(context.root()) {
        let members: Vec<(SyntaxNode<'_>, Bucket)> = body
            .child_nodes()
            .filter_map(|node| Some((node, bucket_of(context, node)?)))
            .collect();

        // Seeded from the first member rather than from the first bucket:
        // under a project's own order, the group `gdck` happens to list first
        // may sort anywhere, and starting from it would report every file.
        let mut highest: Option<Bucket> = None;
        for (node, bucket) in members {
            let Some(top) = highest else {
                highest = Some(bucket);
                continue;
            };
            if rank(bucket, context.config) >= rank(top, context.config) {
                highest = Some(bucket);
                continue;
            }
            sink.report(
                "code-order",
                significant_range(node),
                format!("{} come before {}", bucket.label(), top.label()),
            );
        }
    }
}

/// Which group of `gdtoolkit`'s vocabulary this bucket falls into.
///
/// `gdck` sorts more finely than those fourteen names can say, and everything
/// it knows that they do not — `_ready()` apart from `_process()` apart from a
/// static function — is `Others` to them.
fn group_of(bucket: Bucket) -> DeclarationGroup {
    match bucket {
        Bucket::FileAnnotation => DeclarationGroup::Tools,
        Bucket::ClassName => DeclarationGroup::ClassNames,
        Bucket::Extends => DeclarationGroup::Extends,
        Bucket::Docstring => DeclarationGroup::Docstrings,
        Bucket::Signal => DeclarationGroup::Signals,
        Bucket::Enum => DeclarationGroup::Enums,
        Bucket::Constant => DeclarationGroup::Consts,
        Bucket::StaticVar => DeclarationGroup::StaticVars,
        Bucket::ExportVar => DeclarationGroup::Exports,
        Bucket::PublicVar => DeclarationGroup::PubVars,
        Bucket::PrivateVar => DeclarationGroup::PrvVars,
        Bucket::OnreadyPublicVar => DeclarationGroup::OnreadyPubVars,
        Bucket::OnreadyPrivateVar => DeclarationGroup::OnreadyPrvVars,
        Bucket::StaticInit
        | Bucket::StaticFunc
        | Bucket::Init
        | Bucket::EnterTree
        | Bucket::Ready
        | Bucket::Process
        | Bucket::PhysicsProcess
        | Bucket::Func
        | Bucket::InnerClass => DeclarationGroup::Others,
    }
}

/// Where a bucket sorts, which is the guide's order unless the project stated
/// one of its own.
///
/// The second half of the key is the bucket itself, so the finer order `gdck`
/// knows survives inside whatever position the project gave the group. Two
/// buckets in the same group stay in the guide's relative order rather than
/// becoming interchangeable.
pub(crate) fn rank(bucket: Bucket, config: &LintConfig) -> (usize, Bucket) {
    let Some(order) = &config.declaration_order else {
        return (0, bucket);
    };
    let group = group_of(bucket);
    // A group the project left out sorts after everything it named, rather
    // than silently taking position zero.
    let position = order
        .iter()
        .position(|named| *named == group)
        .unwrap_or(order.len());
    (position, bucket)
}

/// Which bucket a class member belongs to, or `None` for something the order
/// says nothing about.
pub(crate) fn bucket_of(context: &Context<'_>, node: SyntaxNode<'_>) -> Option<Bucket> {
    match node.kind() {
        SyntaxKind::Annotation => Some(Bucket::FileAnnotation),
        SyntaxKind::ClassNameDecl => Some(Bucket::ClassName),
        SyntaxKind::ExtendsDecl => Some(Bucket::Extends),
        SyntaxKind::SignalDecl => Some(Bucket::Signal),
        SyntaxKind::EnumDecl => Some(Bucket::Enum),
        SyntaxKind::ConstDecl => Some(Bucket::Constant),
        SyntaxKind::ClassDecl => Some(Bucket::InnerClass),
        SyntaxKind::VarDecl => Some(var_bucket(context, node)),
        SyntaxKind::FuncDecl => Some(func_bucket(context, node)),
        // A bare string at class level is the docstring the guide puts fourth.
        SyntaxKind::ExprStmt => significant_tokens(node)
            .first()
            .filter(|token| token.kind == SyntaxKind::Str)
            .map(|_| Bucket::Docstring),
        // `pass`, an error node, or a statement in a function body. None of
        // them are declarations with a place in the order.
        _ => None,
    }
}

fn var_bucket(context: &Context<'_>, node: SyntaxNode<'_>) -> Bucket {
    let private = name_token(node).is_some_and(|token| context.token_text(token).starts_with('_'));

    if has_annotation(context, node, "onready") {
        return if private {
            Bucket::OnreadyPrivateVar
        } else {
            Bucket::OnreadyPublicVar
        };
    }
    // Every `@export`, `@export_range`, `@export_enum` and the rest.
    if annotations(context, node).any(|name| name.starts_with("export")) {
        return Bucket::ExportVar;
    }
    if node.child_token_of(SyntaxKind::StaticKw).is_some() {
        return Bucket::StaticVar;
    }
    if private {
        Bucket::PrivateVar
    } else {
        Bucket::PublicVar
    }
}

fn func_bucket(context: &Context<'_>, node: SyntaxNode<'_>) -> Bucket {
    let name = name_token(node).map(|token| context.token_text(token));
    match name {
        Some("_static_init") => Bucket::StaticInit,
        Some("_init") => Bucket::Init,
        Some("_enter_tree") => Bucket::EnterTree,
        Some("_ready") => Bucket::Ready,
        Some("_process") => Bucket::Process,
        Some("_physics_process") => Bucket::PhysicsProcess,
        _ if node.child_token_of(SyntaxKind::StaticKw).is_some() => Bucket::StaticFunc,
        _ => Bucket::Func,
    }
}

// -- Reordering -------------------------------------------------------------

/// A class body's members, each with the exact text that has to move with it.
///
/// Chunks tile the region between the first and last member exactly, so a
/// reorder is a permutation of the source rather than a re-rendering of it. A
/// declaration therefore takes its comments, its annotations and the blank
/// lines above it wherever it goes, byte for byte.
#[derive(Debug)]
struct Chunk {
    range: TextRange,
    bucket: Bucket,
    /// Index into the body's member list, for the dependency analysis.
    member: usize,
}

/// One pass of [`reorder`], which handles a single class body.
#[derive(Debug)]
enum Step {
    /// Every class body in the file is already in order.
    Done,
    Rewrote(String),
    Blocked(String),
}

/// Put a file's declarations in the style guide's order, or explain why not.
///
/// All or nothing for the whole file. A file that is reordered halfway
/// satisfies neither its original layout nor the guide, and would be reported
/// again by the next run; a file left alone at least still works.
///
/// The result is a permutation of the source and nothing else, so its blank
/// lines follow the declarations they used to sit above. Run the formatter
/// afterwards to settle those — `gdck fix --fix-order` does.
pub(crate) fn reorder(source: &str, config: &gdck_config::LintConfig) -> Reorder {
    let mut text = source.to_string();

    // One body per pass, since rewriting an inner class moves every offset
    // computed for the file around it. Each pass sorts one body, so the bound
    // is the number of bodies; anything past that is a bug rather than a file.
    for _ in 0..MAX_REORDER_PASSES {
        let tree = gdck_syntax::parse(&text);
        if tree.has_errors() {
            return Reorder::Blocked("the file has syntax errors".to_string());
        }
        let context = Context {
            tree: &tree,
            source: tree.text(),
            config,
            file_name: None,
        };
        match reorder_pass(&context) {
            Step::Done => {
                return if text == source {
                    Reorder::Unchanged
                } else {
                    Reorder::Reordered(text)
                };
            }
            Step::Rewrote(next) => text = next,
            Step::Blocked(reason) => return Reorder::Blocked(reason),
        }
    }
    Reorder::Blocked("the file did not settle into a stable order".to_string())
}

const MAX_REORDER_PASSES: usize = 64;

fn reorder_pass(context: &Context<'_>) -> Step {
    for body in class_bodies(context.root()) {
        let members: Vec<SyntaxNode<'_>> = body.child_nodes().collect();
        let buckets: Vec<Option<Bucket>> = members
            .iter()
            .map(|node| bucket_of(context, *node))
            .collect();

        let placed: Vec<(usize, Bucket)> = buckets
            .iter()
            .flatten()
            .map(|bucket| rank(*bucket, context.config))
            .collect();
        if placed.windows(2).all(|pair| pair[0] <= pair[1]) {
            continue;
        }

        // Out of order, so this body has to be rewritten. Everything in it
        // needs somewhere to go first.
        let chunks = match chunks_of(context, &members, &buckets) {
            Ok(chunks) => chunks,
            Err(reason) => return Step::Blocked(reason),
        };

        // A stable sort, so declarations that belong to the same group keep
        // the order the author put them in.
        let mut target: Vec<usize> = (0..chunks.len()).collect();
        target.sort_by_key(|index| rank(chunks[*index].bucket, context.config));

        if let Some(blocker) = unsafe_move(context, &members, &chunks, &target) {
            return Step::Blocked(blocker);
        }

        let start = chunks[0].range.start() as usize;
        let end = chunks[chunks.len() - 1].range.end() as usize;
        let mut rewritten = String::with_capacity(context.source.len());
        rewritten.push_str(&context.source[..start]);
        for index in target {
            rewritten.push_str(chunks[index].range.slice(context.source));
        }
        rewritten.push_str(&context.source[end..]);
        return Step::Rewrote(rewritten);
    }
    Step::Done
}

/// Cut a body into one chunk per member, covering the region exactly.
///
/// Fails when the members do not tile cleanly, which is what makes moving them
/// a permutation rather than a rewrite.
fn chunks_of(
    context: &Context<'_>,
    members: &[SyntaxNode<'_>],
    buckets: &[Option<Bucket>],
) -> Result<Vec<Chunk>, String> {
    let source = context.source;
    let mut chunks: Vec<Chunk> = Vec::with_capacity(members.len());
    let mut previous_end: Option<usize> = None;

    for (index, node) in members.iter().enumerate() {
        let Some(bucket) = buckets[index] else {
            return Err(format!(
                "a `{}` here is not a declaration the order has a place for",
                describe(context, *node)
            ));
        };
        let (Some(head), Some(tail)) = (
            own_start(source, *node),
            significant_tokens(*node)
                .last()
                .map(|token| token.range.end()),
        ) else {
            return Err("a declaration has no text to move".to_string());
        };
        let (head, tail) = (head as usize, tail as usize);

        let line_start = source[..head].rfind('\n').map_or(0, |at| at + 1);
        let line_end = source[tail..]
            .find('\n')
            .map_or(source.len(), |at| tail + at + 1);

        let start = match previous_end {
            None => line_start,
            Some(end) if end > line_start => {
                return Err(format!(
                    "`{}` shares a line with the declaration above it",
                    describe(context, *node)
                ));
            }
            Some(end) => end,
        };
        previous_end = Some(line_end);
        chunks.push(Chunk {
            range: TextRange::new(start as u32, line_end as u32),
            bucket,
            member: index,
        });
    }
    if chunks.is_empty() {
        return Err("the class body is empty".to_string());
    }
    Ok(chunks)
}

/// How a message refers to a member: its name if it has one, else its first
/// word.
fn describe(context: &Context<'_>, node: SyntaxNode<'_>) -> String {
    if let Some(token) = name_token(node) {
        return context.token_text(token).to_string();
    }
    significant_tokens(node).first().map_or_else(
        || "declaration".to_string(),
        |token| context.token_text(*token).to_string(),
    )
}

/// Where a declaration begins on the page: its first token, or the first
/// comment written above it, whichever comes first.
fn own_start(source: &str, node: SyntaxNode<'_>) -> Option<u32> {
    let first_code = significant_tokens(node)
        .first()
        .map(|token| token.range.start())?;
    // The parser attaches a comment to the token that follows it, so the
    // trailing comment of the declaration *above* arrives here as this one's
    // leading trivia. Taking it would drag this chunk's start back onto the
    // previous line, where it looks like the two share one. A comment with
    // nothing but whitespace before it on its line is genuinely this
    // declaration's; one sharing a line with code belongs to that code.
    let first_comment = super::all_tokens(node)
        .into_iter()
        .filter(|token| token.kind.is_comment())
        .map(|token| token.range.start())
        .filter(|start| *start < first_code)
        .find(|start| starts_its_line(source, *start));
    Some(first_comment.unwrap_or(first_code))
}

/// Whether only whitespace sits between `offset` and the line it is on.
fn starts_its_line(source: &str, offset: u32) -> bool {
    let before = &source[..offset as usize];
    let line_start = before.rfind('\n').map_or(0, |at| at + 1);
    before[line_start..].trim().is_empty()
}

/// The name of a move that would change what the program does, if there is one.
///
/// Class-level variable initialisers run in declaration order, so hoisting a
/// variable above one it reads leaves it initialised from `null`. Nothing else
/// in a class body carries order: constants and enums are resolved when the
/// script is compiled, and signals and functions are declarations rather than
/// steps.
fn unsafe_move(
    context: &Context<'_>,
    members: &[SyntaxNode<'_>],
    chunks: &[Chunk],
    target: &[usize],
) -> Option<String> {
    let local_functions: HashSet<&str> = context
        .root()
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::FuncDecl)
        .filter_map(name_token)
        .map(|token| context.token_text(token))
        .collect();

    // Where each chunk ends up.
    let mut position = vec![0usize; chunks.len()];
    for (place, index) in target.iter().enumerate() {
        position[*index] = place;
    }

    // The variables, in the order they are written and so in the order they
    // are initialised.
    let variables: Vec<(usize, &str)> = chunks
        .iter()
        .enumerate()
        .filter(|(_, chunk)| members[chunk.member].kind() == SyntaxKind::VarDecl)
        .filter_map(|(index, chunk)| {
            name_token(members[chunk.member]).map(|token| (index, context.token_text(token)))
        })
        .collect();

    for (order, (index, name)) in variables.iter().enumerate() {
        let declaration = members[chunks[*index].member];
        let earlier = &variables[..order];
        let reads = reads_of(context, declaration, &local_functions);

        for (other, other_name) in earlier {
            let depends = match &reads {
                Reads::Everything => true,
                Reads::Names(names) => names.contains(*other_name),
            };
            if depends && position[*other] > position[*index] {
                return Some(format!(
                    "`{name}` is initialised from `{other_name}`, which the guide's order would \
                     move below it"
                ));
            }
        }
    }
    None
}

/// What a variable's initialiser may read from the object being built.
#[derive(Debug)]
enum Reads {
    /// Named members only.
    Names(HashSet<String>),
    /// Something opaque: every member declared above it has to stay there.
    Everything,
}

fn reads_of(
    context: &Context<'_>,
    declaration: SyntaxNode<'_>,
    local_functions: &HashSet<&str>,
) -> Reads {
    // A setter runs when the initialiser assigns, and its body can read
    // anything at all.
    if declaration
        .child_node_of(SyntaxKind::Accessors)
        .is_some_and(|accessors| {
            accessors
                .descendants()
                .any(|node| node.kind() == SyntaxKind::Block)
        })
    {
        return Reads::Everything;
    }
    let Some(initializer) = declaration.child_node_of(SyntaxKind::Initializer) else {
        return Reads::Names(HashSet::new());
    };

    let mut names = HashSet::new();
    let tokens = significant_tokens(initializer);
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            // `self.x` and `super` reach the object directly, and a node path
            // can reach a script that reaches back.
            SyntaxKind::SelfKw
            | SyntaxKind::SuperKw
            | SyntaxKind::GetNode
            | SyntaxKind::UniqueNode => return Reads::Everything,
            SyntaxKind::Ident => {
                let name = context.token_text(*token);
                // A call to a function defined in this file can read whatever
                // it likes. A call to anything else — a constructor, a global,
                // an autoload — cannot see this object's members unless it was
                // handed them, in which case they are named here anyway.
                let called = tokens
                    .get(index + 1)
                    .is_some_and(|next| next.kind == SyntaxKind::LParen);
                if called && local_functions.contains(name) {
                    return Reads::Everything;
                }
                names.insert(name.to_string());
            }
            _ => {}
        }
    }
    Reads::Names(names)
}

fn has_annotation(context: &Context<'_>, node: SyntaxNode<'_>, wanted: &str) -> bool {
    annotations(context, node).any(|name| name == wanted)
}

fn annotations<'a>(
    context: &'a Context<'a>,
    node: SyntaxNode<'a>,
) -> impl Iterator<Item = &'a str> + 'a {
    node.child_nodes()
        .filter(|child| child.kind() == SyntaxKind::Annotation)
        .filter_map(move |child| {
            // `@` then the name; the arguments are a child node.
            child
                .child_tokens()
                .find(|token| token.kind.is_ident_like())
                .map(|token| context.token_text(token))
        })
}

#[cfg(test)]
mod tests {
    use gdck_config::{CodeOrderFix, LintConfig};

    use crate::Diagnostic;

    /// Only `code-order`, so a fixture written to test ordering does not also
    /// have to satisfy every naming and spacing rule.
    fn config() -> LintConfig {
        let mut config = LintConfig::default();
        config.disabled = crate::RULES
            .iter()
            .map(|rule| rule.name.to_string())
            .filter(|name| name != "code-order")
            .collect();
        config
    }

    fn diagnostics(source: &str) -> Vec<Diagnostic> {
        crate::lint(&gdck_syntax::parse(source), &config())
    }

    fn messages(source: &str) -> Vec<String> {
        diagnostics(source)
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn the_guides_own_example_is_in_order() {
        // Straight from "Signals and properties", then "Methods and static
        // functions".
        let source = "signal player_spawned(position)\n\nenum Job {\n\tKNIGHT,\n\tWIZARD,\n}\n\nconst MAX_LIVES = 3\n\n@export var job: Job = Job.KNIGHT\n@export var max_health = 50\n\nvar health = max_health\n\nvar _speed = 300.0\n\n@onready var sword = get_node(\"Sword\")\n\n\nfunc _init():\n\tadd_to_group(\"state_machine\")\n\n\nfunc _ready():\n\tpass\n\n\nfunc _unhandled_input(event):\n\tprint(event)\n\n\nfunc transition_to(target):\n\tprint(target)\n\n\nfunc _on_state_changed(previous):\n\tprint(previous)\n";
        assert_eq!(messages(source), Vec::<String>::new());
    }

    #[test]
    fn the_class_declaration_comes_first() {
        assert_eq!(
            messages("extends Node\n\n@tool\n"),
            ["file annotations come before `extends`"]
        );
        assert_eq!(
            messages("@tool\nclass_name Player\nextends Node\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn signals_come_before_variables() {
        assert_eq!(
            messages("var health = 1\n\nsignal died\n"),
            ["signals come before public variables"]
        );
    }

    #[test]
    fn variables_are_grouped_by_kind_then_by_visibility() {
        assert_eq!(
            messages("var _speed = 1\nvar health = 2\n"),
            ["public variables come before private variables"]
        );
        assert_eq!(
            messages("@onready var sword = 1\n@export var health = 2\n"),
            ["`@export` variables come before public `@onready` variables"]
        );
        assert_eq!(
            messages("var health = 1\n\nconst MAX = 2\n"),
            ["constants come before public variables"]
        );
    }

    #[test]
    fn the_named_virtual_callbacks_are_ordered() {
        assert_eq!(
            messages("func _ready():\n\tpass\n\n\nfunc _init():\n\tpass\n"),
            ["`_init()` come before `_ready()`"]
        );
        assert_eq!(
            messages("func _init():\n\tpass\n\n\nfunc _ready():\n\tpass\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_function_is_never_called_misplaced_on_a_guess() {
        // Neither of these is a callback `gdck` can recognise, so their order
        // relative to each other is not something it will claim to know.
        assert_eq!(
            messages(
                "func _unhandled_input(e):\n\tprint(e)\n\n\nfunc transition_to(t):\n\tprint(t)\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            messages(
                "func transition_to(t):\n\tprint(t)\n\n\nfunc _unhandled_input(e):\n\tprint(e)\n"
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn inner_classes_come_last() {
        assert_eq!(
            messages("class Inner:\n\tpass\n\n\nfunc f():\n\tpass\n"),
            ["functions come before inner classes"]
        );
    }

    #[test]
    fn an_inner_class_has_its_own_order() {
        assert_eq!(
            messages("class Inner:\n\tvar health = 1\n\n\tsignal died\n"),
            ["signals come before public variables"]
        );
    }

    // -- Reordering ---------------------------------------------------------

    /// A project's own order is honoured, both when reporting and when
    /// reordering. `gdtoolkit` lets a project pin `class-definitions-order`,
    /// and a project that did so had a reason.
    #[test]
    fn a_projects_own_declaration_order_is_the_one_checked() {
        use gdck_config::DeclarationGroup as Group;
        let source = "enum State { IDLE }\nsignal died\n";

        // The guide puts signals above enums, so this is out of order.
        let guide = crate::lint(&gdck_syntax::parse(source), &LintConfig::default());
        assert_eq!(guide.len(), 1);
        assert_eq!(guide[0].rule, "code-order");

        // A project that asked for enums first is not.
        let mut config = LintConfig::default();
        config.declaration_order = Some(vec![Group::Enums, Group::Signals, Group::Others]);
        assert!(crate::lint(&gdck_syntax::parse(source), &config).is_empty());

        // And reordering follows the same order rather than the guide's.
        let backwards = "signal died\nenum State { IDLE }\n";
        match crate::reorder(backwards, &config) {
            crate::Reorder::Reordered(text) => assert_eq!(text, source),
            other => panic!("expected a reorder, got {other:?}"),
        }
    }

    /// `gdtoolkit` has one bucket for every method; `gdck` has nine. Giving
    /// `others` a position says where they all go, not that they stop being
    /// ordered among themselves.
    #[test]
    fn the_finer_order_survives_inside_others() {
        use gdck_config::DeclarationGroup as Group;
        let mut config = LintConfig::default();
        config.declaration_order = Some(vec![Group::Others, Group::Signals]);
        // `_ready` before `_init` is wrong however `others` is placed.
        let found = crate::lint(
            &gdck_syntax::parse("func _ready():\n\tpass\n\n\nfunc _init():\n\tpass\n"),
            &config,
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].rule, "code-order");
    }

    fn reordered(source: &str) -> String {
        match crate::reorder(source, &LintConfig::default()) {
            crate::Reorder::Reordered(text) => text,
            crate::Reorder::Unchanged => source.to_string(),
            crate::Reorder::Blocked(reason) => panic!("blocked: {reason}"),
        }
    }

    fn blocked(source: &str) -> String {
        match crate::reorder(source, &LintConfig::default()) {
            crate::Reorder::Blocked(reason) => reason,
            other => panic!("expected the reorder to be refused, got {other:?}"),
        }
    }

    #[test]
    fn a_file_already_in_order_is_untouched() {
        let source = "signal died\n\nconst MAX = 3\n\nvar health = 1\n";
        assert_eq!(
            crate::reorder(source, &LintConfig::default()),
            crate::Reorder::Unchanged
        );
    }

    #[test]
    fn declarations_move_into_the_guides_order() {
        assert_eq!(
            reordered("var health = 1\nconst MAX = 3\nsignal died\n"),
            "signal died\nconst MAX = 3\nvar health = 1\n"
        );
    }

    #[test]
    fn a_declaration_takes_its_comments_and_blank_lines_with_it() {
        // The reorder is a permutation of the source, so nothing is re-rendered
        // and nothing can be dropped on the way.
        assert_eq!(
            reordered("# Documents health.\nvar health = 1\n\n## The signal.\nsignal died\n"),
            "\n## The signal.\nsignal died\n# Documents health.\nvar health = 1\n"
        );
    }

    #[test]
    fn a_trailing_comment_is_not_mistaken_for_the_next_declarations_own() {
        // The parser hands a comment to the token that follows it, so the
        // trailing comment of one declaration arrives as the leading trivia of
        // the next. Treating it as the next one's start dragged that chunk
        // back onto the previous line, where it looked like the two shared
        // one, and the whole file was refused. On the project this was found
        // on it blocked 13 files holding 105 of the 171 order reports.
        assert_eq!(
            reordered("var health = 1 # how much\nsignal died # when it runs out\n"),
            "signal died # when it runs out\nvar health = 1 # how much\n"
        );
    }

    #[test]
    fn a_comment_on_its_own_line_still_belongs_to_what_follows_it() {
        // The other half of the same rule: this one really is the signal's, so
        // it has to travel with the signal rather than stay behind.
        assert_eq!(
            reordered("var health = 1 # how much\n# Documents the signal.\nsignal died\n"),
            "# Documents the signal.\nsignal died\nvar health = 1 # how much\n"
        );
    }

    #[test]
    fn an_annotation_moves_with_the_declaration_it_modifies() {
        assert_eq!(
            reordered("var health = 1\n@export var damage = 2\n"),
            "@export var damage = 2\nvar health = 1\n"
        );
    }

    #[test]
    fn an_initialiser_dependency_stops_the_whole_file() {
        // The guide puts public variables above private ones, which would
        // hoist `speed` above the `_config` it reads.
        let source = "var _config = 3\nvar speed = _config\n";
        assert!(blocked(source).contains("`speed` is initialised from `_config`"));
    }

    #[test]
    fn a_dependency_the_order_keeps_intact_is_not_a_problem() {
        // Both are private, so the sort is stable and neither moves past the
        // other. The signal is what is out of order.
        assert_eq!(
            reordered("var _config = 3\nvar _speed = _config\nsignal died\n"),
            "signal died\nvar _config = 3\nvar _speed = _config\n"
        );
    }

    #[test]
    fn an_opaque_initialiser_is_assumed_to_read_everything_above_it() {
        // `_compute` could read `_base` without naming it here.
        let source =
            "var _base = 1\n\n\nfunc _compute():\n\treturn _base\n\n\nvar total = _compute()\n";
        assert!(blocked(source).contains("`total`"));
    }

    #[test]
    fn a_constructor_call_is_not_opaque() {
        // `Vector2` is not defined in this file, so it cannot read what has
        // not been set yet.
        assert_eq!(
            reordered("var _origin = 1\nvar position = Vector2(0, 0)\n"),
            "var position = Vector2(0, 0)\nvar _origin = 1\n"
        );
    }

    #[test]
    fn a_setter_makes_a_variable_opaque() {
        let source =
            "var _limit = 1\n\nvar health = 5:\n\tset(value):\n\t\thealth = mini(value, _limit)\n";
        assert!(blocked(source).contains("`health`"));
    }

    #[test]
    fn two_declarations_on_one_line_cannot_be_moved_apart() {
        assert!(blocked("var health = 1; signal died\n").contains("`died` shares a line"));
    }

    #[test]
    fn a_statement_with_no_place_in_the_order_stops_the_file() {
        assert!(
            blocked("var health = 1\nsignal died\npass\n")
                .contains("`pass` here is not a declaration")
        );
    }

    #[test]
    fn a_file_that_does_not_parse_is_left_alone() {
        assert!(blocked("func f(:\n\tpass\n").contains("syntax errors"));
    }

    #[test]
    fn an_inner_class_is_reordered_too() {
        assert_eq!(
            reordered("class Inner:\n\tvar health = 1\n\tsignal died\n"),
            "class Inner:\n\tsignal died\n\tvar health = 1\n"
        );
    }

    #[test]
    fn reordering_is_a_permutation_of_the_source() {
        // Every byte that was there is still there, which is what makes this
        // safe to do without re-rendering the file.
        let source = "@onready var sprite = 1\nvar health = 2\nconst MAX = 3\n# why\nsignal died\n";
        let after = reordered(source);
        let mut before_bytes: Vec<u8> = source.bytes().collect();
        let mut after_bytes: Vec<u8> = after.bytes().collect();
        before_bytes.sort_unstable();
        after_bytes.sort_unstable();
        assert_eq!(before_bytes, after_bytes);
    }

    #[test]
    fn the_rule_can_be_switched_off_entirely() {
        let mut config = config();
        config.code_order = CodeOrderFix::Off;
        let tree = gdck_syntax::parse("var health = 1\n\nsignal died\n");
        assert!(crate::lint(&tree, &config).is_empty());
    }
}
