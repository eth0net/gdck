# Design

How `gdck` is put together, and why. This documents decisions that are not
obvious from the code, so that revisiting them later is a choice rather than an
accident.

## Crate layout

| Crate | Responsibility |
|---|---|
| `gdck-syntax` | Lexer, lossless CST, parser. No knowledge of formatting or rules. |
| `gdck-config` | Configuration types, defaults, and reading `gdck.toml`. |
| `gdck-format` | Tree → formatted text. |
| `gdck-lint` | Tree → diagnostics, some carrying fixes. Uses `gdck-format` for literal rewrites. |
| `gdck` | The `gdck` binary. Argument parsing, file walking, reporting. |

The split exists so `gdck-syntax` can be published and depended on by itself. A
fast GDScript parser is useful to editors, documentation generators and static
analysers, none of which want a linter attached.

`gdck-config` sits underneath both `gdck-format` and `gdck-lint` because both
need the same thresholds, and neither should have to ask the other for them.

## Dependencies

The deliverable is a single self-contained binary — no interpreter, no runtime,
nothing to install alongside it. That is the reason the project is in Rust at
all, and it is a statement about what ships, not about what builds.

Build-time crates are chosen on merit, and a proven one is preferred to an
imitation of it every time. `gdck.toml` is read by `toml`, which is what Cargo
parses manifests with. `gdlintrc` and `gdformatrc` are read by `yaml_serde`,
the YAML Organization's continuation of the deprecated `serde_yaml`. Diffs come
from `similar`.

Two of those replaced code written here, and both replacements found a defect
the hand-written version had. The diff trimmed the common prefix and suffix and
printed everything between, so two one-line changes at opposite ends of a file
printed the whole file twice. The YAML reader required block sequences to be
indented, which PyYAML does not do, so the canonical output of
`gdlint --dump-default-config` produced fourteen spurious problems — under a
test that claimed to prove the opposite, because the fixture had been written by
hand rather than by PyYAML.

That is the general shape of the argument. A hand-written reader's risk is not
failing to parse, it is accepting something and *misreading* it, and the files
here belong to other tools and other people. Something is written here only when
no crate does the job — which is why the case predicates in `gdck-lint::names`
stay. A project chooses between named conventions rather than supplying a
pattern, so there is never a regular expression to compile: `lint.file-name`
picks one of two spellings, and every other name the rules check is an
identifier the guide has a settled view about. `gdtoolkit` takes a regex for
each, which is how a project ends up with its own slightly different idea of
what snake_case means.

## The syntax tree

### Losslessness is the load-bearing constraint

Every byte of the source lives in exactly one token, and every token lives in
the tree. `SyntaxTree::text()` returns the original input, and this holds even
when parsing produced errors.

Everything else follows from this:

- The formatter can rewrite one declaration and leave surrounding comments and
  blank lines exactly where they were.
- Auto-fixing code order can move a declaration together with its doc comment
  and its annotations.
- A file that fails to parse can still be reported on without risk of damage.

The corpus test (`crates/gdck-syntax/tests/corpus.rs`) asserts this over an
arbitrary directory of `.gd` files.

### Arena, not reference counting

Nodes live in a flat `Vec` and refer to each other by index. Children of a node
are contiguous, which keeps traversal cache-friendly, and the whole tree is one
allocation-dense structure rather than a graph of `Rc`s. The cost is that a
`NodeId` is meaningless without the tree that owns it, which has not been a
problem in practice.

### Checkpoints

The builder keeps children in one flat working buffer with a stack of open
nodes. `checkpoint()` records a position in that buffer; `start_node_at()` opens
a node that retroactively contains everything added since. Both are O(1).

This matters because expression parsing needs it constantly — you only discover
that `a + b` is a binary expression after `a` has already been emitted. The same
mechanism lets an `ExprStmt` be reclassified as an `AssignStmt` when an `=`
turns up, and lets annotations become children of the declaration they modify.

The approach is borrowed from `rowan`, which `rust-analyzer` uses. `gdck` does
not depend on `rowan` because the red-green tree's incrementality is not needed
here and the arena above is simpler to reason about.

## The lexer

### Indentation

GDScript delimits blocks by indentation, so the lexer emits zero-width `Indent`
and `Dedent` tokens the way a Python tokenizer does. This keeps the parser a
plain recursive-descent affair with no whitespace sensitivity of its own.

`Indent` and `Dedent` are explicitly **not** trivia — the parser consumes them
to delimit blocks. Whitespace, comments, newlines and line continuations are.

A tab advances to the next multiple of 4 columns, used only to compare depth
between lines. The raw indentation text stays in the whitespace token, so the
linter can still tell tabs from spaces and report mixing.

Blank lines and comment-only lines carry no indentation information and do not
touch the indent stack. A consequence worth knowing: a comment at column 0
between two indented lines lands inside the enclosing block in the tree, because
the dedent does not happen until the next line with real content. Python
tokenizes the same way. The formatter reassigns such comments by their own
indentation.

### Value position

`%` is modulo after a value and a unique-node path before one. `&` and `^` are
bitwise operators after a value and `StringName` / `NodePath` sigils before one.
The lexer resolves this by asking whether the previous meaningful token could
end an expression — the same trick that tells regex from division in a
JavaScript lexer.

A newline resets the answer, since the previous line's last token says nothing
about a token starting a fresh statement. Without that, a line beginning `^"a/b"`
lexes as bitwise-xor whenever the line above happens to end in a value.

### Node paths

`$Node/Path` cannot be lexed as `$`, identifier, `/`, identifier — the slash is
not division. The whole path becomes one token. A `.` is consumed only as part
of `..`, so `$Sprite2D.position` still splits into a path and an attribute
access.

### Multi-line lambdas

Indentation means nothing inside `()`, `[]` and `{}` — except inside a lambda
body, which is a real block:

```gdscript
button.pressed.connect(
    func() -> void:
        do_something()
        do_more()
)
```

The lexer handles this the way Godot's own tokenizer does. A `func` seen inside
brackets arms lambda detection; the next `:` at that same bracket depth opens a
lambda context. Indentation becomes significant again, but only at exactly that
depth, so a multi-line array *inside* the lambda body still suppresses it.

A lambda body ends at whichever comes first:

- a line indented less than the body's first line,
- a `,` at the lambda's own bracket depth, separating it from the next argument,
- the bracket enclosing it closing,
- end of file.

The dedents a body owes are emitted before the token that closed it, so the
parser sees a properly terminated block.

The body's extent is derived from the **first body line**, not from the line the
lambda opened on. A lambda can open mid-line, after a comma, with its body at
that same column:

```gdscript
bar(func():
    var x = 1
    if x > 0:
        print(x), func():
        var y = 1
        return y)
```

Here the second lambda opens at the end of a line indented 12, and its body is
also indented 12. Comparing against the opening line would end the body
immediately.

## The parser

### Never fails

Malformed input produces `Error` nodes and diagnostics; parsing always returns a
complete tree. Editors need this because most keystrokes leave a buffer
temporarily unparseable, and it lets one run report several problems instead of
stopping at the first.

Every loop routes through `ensure_progress`, which forces a token to be consumed
if a rule returned without consuming anything. A grammar bug surfaces as one
stray `Error` node rather than a hang.

Errors at the same offset are collapsed, since cascades from a single mistake
are noise.

### Statement boundaries

Newlines are trivia, which means the expression parser would happily run past
the end of a statement — reading the `if` that opens the next line as a ternary
belonging to the previous expression. The parser tracks bracket depth and stops
at a newline when outside brackets.

Entering an indented block resets that depth, because a block is its own
line-oriented world even when it sits inside brackets. That is exactly the
multi-line-lambda-as-argument case.

### Precedence

Binding powers follow the GDScript reference. Comparisons bind tighter than
`in`, which binds tighter than `is`, which binds tighter than `not`. `**` is
right-associative and binds tighter than unary minus, so `-2 ** 2` is `-(2 ** 2)`
as in Godot.

`not in` is one infix operator spelled as two words, matched before `not` is
considered as anything else.

`as` gets its own `CastExpr` node rather than being a plain binary operator, so
the static-typing lint rules can find casts directly.

### Contextual keywords

`set` and `get` are ordinary identifiers everywhere except in a property
accessor clause, so they are matched by text. This is also why `var p: set = f`
has to be told apart from a type annotation: the parser refuses to read `set` or
`get` as a type name.

`abstract` is *not* a keyword. Godot 4.5 spells it `@abstract`, an annotation, so
treating it as reserved would break `var abstract = 1`.

### Annotation attachment

`@export var health := 100` puts the annotation *inside* the `VarDecl`, so
moving the declaration moves its annotations. `@tool` and `@icon` describe the
file rather than the `class_name` that follows, so they stay as siblings. The
distinction is by target: declarations that can be annotated adopt them,
`class_name` and `extends` do not.

## The formatter

### Two stages

`lower` turns the syntax tree into a document IR describing where a line *may*
break; the renderer decides where it *does*, given the width. Keeping those
apart means the style-guide rules live in one place rather than being spread
through string concatenation.

The IR is the usual Wadler set — text, soft and hard breaks, groups, indent,
and a break-dependent choice for trailing commas — plus two additions that
GDScript needs:

- `break_parent` emits nothing but forces every enclosing group open. It carries
  "the author wrapped this" and "there is a comment here" outwards.
- `flat` renders its contents on one line whatever the width, for `match`
  patterns. A pattern sits between `match` and `:` with no brackets around it,
  so a line break in one is a syntax error rather than a long line.

### Where the guide is the specification

Every code sample in the style guide is extracted into a test fixture by
`tools/extract-style-guide-samples.py` and classified in
`crates/gdck-format/tests/style_guide.rs`. Doing that settled several questions
that reading the prose did not:

- **Blank lines inside a class.** The guide says to surround definitions with
  two blank lines, but its own worked example ends with an inner class whose
  `func` has one. Two at file level, one inside a class.
- **Parameter defaults.** The whitespace rule says one space around operators,
  yet two separate samples write `func take_damage(amount, effect=null)`. A
  default with no type hint is written tight; one with a type hint is spaced,
  which is the surrounding convention and contradicts no sample.
- **Inline comment spacing.** The guide states no rule, but writes
  `print("Example") # Short comment.` — one space, where `gdformat` uses two.
- **Enums.** "Write enums with each item on its own line" is explicit, so an
  enum body always breaks regardless of width.

### Known differences from the guide

The guide describes itself as advice: "Style guides aren't meant as hard
rulebooks", and keeping a project consistent matters more than "following this
guide to a tee". `gdck` turns that advice into exit codes, and that conversion
needs decisions the guide did not make. They are collected here so that adopting
`gdck` is an informed choice, and so that changing one is deliberate.

They are not all the same kind of thing, and the difference matters.

**Godot requires it and the guide is silent.** Not style. The alternative does
not compile.

- A breaking argument list whose last item is a lambda block gains a trailing
  comma. The guide scopes trailing commas to "arrays, dictionaries, and enums",
  so an argument list is outside what it describes at all. Godot goes back to
  ignoring a lambda's indentation at whatever ends the lambda, and until then a
  dedent has to land on one of the enclosing statement's own levels — where the
  closing bracket of a wrapped call does not sit. The comma ends the lambda on
  the body's last line, leaving no line to measure. `gdck` emits it whenever
  such a list breaks, rather than only where the nesting would otherwise fail,
  so that whether the output compiles does not depend on how deeply the call
  happens to be nested.
- Parentheses around a lambda block close on the body's last line, for the same
  reason and because nothing else is available to end that lambda. Putting the
  closer at the enclosing statement's own indent is equally valid GDScript and
  was the other candidate; it would need the renderer to anchor a line to
  something other than the current indent, which nothing else in the formatter
  does.

**Stricter than the guide asks.**

- Public variables come before private ones. The guide said so until 4.4, which
  merged the two into "remaining regular variables" and stopped having an
  opinion. `gdck` kept the older, narrower order for variables. It applies
  nothing of the kind to methods, where it would contradict the virtual-callback
  order above it — see [RULES.md](RULES.md#what-code-order-does-and-does-not-check).
- Wrapping puts one item per line. Three of the guide's samples are not
  reproducible by any deterministic rule, being hand-formatted rather than
  derived from the column limit: two wrap a call by filling several arguments
  per line, and one wraps a boolean chain two comparisons per line. In each case
  the break points do not match where a real fill at 100 columns would land. One
  item per line is deterministic and diffs better. The three are listed as
  exceptions in the conformance test so the deviation cannot become accidental.

**Measured differently.**

- Line length is counted in columns with a tab worth four, where the guide says
  "characters". A tab-indented line therefore counts the same to the linter as
  it does to the formatter. `gdtoolkit` counts characters, which is why it
  reports fewer.

**Read out of the guide's examples where its prose does not say.** These are
readings rather than departures, but they are still choices, and extracting the
samples as fixtures is what settled them. They are listed under [Where the guide
is the specification](#where-the-guide-is-the-specification) above.

### Comments

The parser keeps comments as trivia in front of the token that follows, which is
right for a lossless tree and the wrong shape for a formatter. `trivia.rs`
re-indexes them into leading comments — those on a line of their own, attached
to the next significant token — and trailing ones, attached to the last
significant token before them.

Two details matter. A comment after `1,` anchors on the `1` rather than on the
comma, because the formatter may move or remove that comma and would strand the
comment. And a comment on the last line of a block is anchored on a token that
is also the last token of the block, of the function holding it, and of every
construct up to the file, so emitted anchors are tracked to stop each of them
emitting it again.

## The linter

The rule catalogue lives in `crates/gdck-lint/src/lib.rs` as `RULES` and is
documented for users in [RULES.md](RULES.md). A test fails if the two disagree.

### Grouped walks, not one pass

Each group of rules walks the parts of the tree it cares about. A single
dispatching traversal would save a few microseconds on a file of a thousand
lines and would couple every rule to one match statement; a group as it stands
can be read, tested and changed without knowing what the others match on.

### Fixes

A diagnostic carries a `Fix` of one or more `Edit`s, applied back to front so
that offsets computed against the original text stay valid. Overlap is judged
edit by edit rather than over the span a fix covers: dropping the parentheses
from `if (!a):` is two deletions with the `!` between them, and rewriting that
`!` in the same pass is not a conflict.

`fix_source` re-lints and applies again until nothing changes, because a fix
deferred for overlapping another is picked up next time round. If a pass ever
makes a file that parsed stop parsing, the result is discarded.

### Where the formatter is the authority

`quote-style` and `number-format` call `gdck_format::literal` for their
rewrites. Two implementations would eventually disagree, and that would show up
as `gdck lint --fix` producing something `gdck format` then changed again.

This is why `gdck-lint` depends on `gdck-format`, which the crate table above
does not otherwise imply.

### Telling a comment from disabled code

The guide asks for a space after the hash, "but not code that you comment out",
which is a distinction of intent rather than of syntax. `comment-space` resolves
it by asking whether the text after the hash *is* GDScript — this project has a
parser, and it is tried both at class level and inside a function body, since
`const X = 1` is only legal in one and `return null` only in the other.

Parsing alone is too generous, since a single English word parses as an
expression, so the text must also look like code: hold a bracket or an `=`, or
open with a keyword. The bias is deliberate — reporting a comment that was
disabled code would be telling the author to break the thing the guide asked
them to do.

### Suppression

Reuses the comment syntax GDScript projects already have: `# gdlint: ignore=`
for one line and the one below it, `# gdlint: disable=` / `enable=` for a
region, with `gdtoolkit`'s exact semantics. Directives are read from the comment
*tokens* in the tree rather than by scanning lines, so a `#` inside a string
literal cannot be mistaken for one.

### Code order

The analysis behind the all-or-nothing policy is in the
[README](../README.md#code-order-is-fixed-for-a-whole-file-or-not-at-all).

A reorder is a **permutation of the source**, not a re-rendering of it. The
members of a class body are cut into chunks that tile the region between the
first and last of them exactly, and the chunks are reordered. A declaration
therefore takes its comments, its annotations and the blank lines above it
wherever it goes, byte for byte, and nothing can be dropped. A corpus test
asserts that the bytes coming out are the bytes that went in.

That also sets the limits. A body is refused when two declarations share a line,
or when it holds something the order has no place for, because neither can be
moved without rewriting rather than permuting.

Dependency edges are built between class-level variables only. Constants and
enums are resolved when the script is compiled, and signals and functions are
declarations rather than steps, so none of them carry order. An initialiser is
treated as reading every variable above it when it touches `self`, `super`, a
`$` or `%` node path, a property setter, or calls a function defined in this
file. Otherwise its dependencies are exactly the member names it mentions — so
`Vector2(0, 0)` is self-contained, since a constructor defined elsewhere cannot
see members that have not been set.

The over-approximation is the point. Precise interprocedural analysis would be
needed to know whether `var total := _compute()` reads `_base` inside
`_compute`, and being wrong there breaks a game at runtime with no error.

One class body is rewritten per pass, with a re-parse between them, because
rewriting an inner class moves every offset computed for the file around it.

## Configuration

The schema and the precedence are documented for users in [CONFIG.md](CONFIG.md).
What is worth recording here is why the crate reads its own files.

### Two formats, two readers

`gdck.toml` is deserialised by `toml` and `serde`, into structs whose every
field is optional so that a setting left out stays distinguishable from one
written at its default. Values are `Spanned`, which is what lets a setting that
parsed but will not do — an `indent-width` alongside `indent = "tabs"` — be
reported at the line it is on rather than at the top of the file. Validation
above the deserialiser covers what a type cannot say: ranges, and settings that
only mean something alongside another.

The `gdtoolkit` files are read by `yaml_serde` into a `Mapping` rather than into
a struct, because `serde`'s all-or-nothing shape is wrong for them: a `gdlintrc`
legitimately holds dozens of settings `gdck` has no equivalent for, and the
right response to each is to skip that one with a note rather than to fail the
file. Iterating an untyped mapping gives that, and still gets a real YAML parser
underneath — `!!set`, block sequences at either indentation, quoting rules and
all.

The one thing it does not give is a line number for a key that parsed, since
locations live only on errors. `line_of` recovers it by finding the line the key
opens, which is unambiguous because top-level keys are unique in a mapping.

A *setting* in a `gdtoolkit` file is never fatal, which is the opposite of the
rule for `gdck.toml`. The *file* is: if the document cannot be parsed then none
of its settings apply, which is the outcome worse than not running — a project
formatted by rules it had written down and rejected, with nothing said.

### Strict about its own keys

An unknown key in `gdck.toml` is an error, and the message names the nearest key
that exists. A misspelled setting that does nothing is the classic way to spend
an afternoon wondering why a threshold had no effect, and the whole schema is
twelve keys — there is no version-skew argument for tolerating one that is not
among them.

The exception is a `disable` entry naming a rule that does not exist, which is a
warning. That list is shared with `gdlint`, whose rule set is not the same, and
a rule that does not exist here is already off.

### One configuration per run

Found by walking up from the directory the command-line paths have in common,
rather than per file. Per-file resolution is what `ruff` and `rustfmt` do and it
is better in a monorepo, but it means a config cache and a config argument
threaded through every subcommand, for a case a second invocation already
covers. The limitation is documented rather than hidden.

A `gdck.toml` wins outright over the `gdtoolkit` files rather than merging with
them key by key. Merging would make the effective settings a function of two
files that were written years apart by people solving different problems, and
`gdck config` exists precisely because that question should have a short answer.

### Writing settings, and why there is no `config set`

`gdck init` creates the file; nothing edits one in place. A `git config`-style
`gdck config set lint.max-returns 3` has been considered and is not built.

The generated file is mostly comments — the commented-out defaults, the header
explaining them, and whatever the project wrote itself. A setter implemented the
obvious way, reading into a `Config` and writing it back out, would delete all of
that. That is the same failure as `gdck config > gdck.toml`, which this project
shipped as documented advice and had to retract: a command that looks like it
edits a file while actually replacing it with a machine's idea of it. Doing it
safely means a format-preserving editor such as `toml_edit`, and that is the
condition rather than a detail.

Against that, the case is thin. `git config` earns its setters because git's
configuration is a hierarchy edited constantly across scopes; this is one
project file touched at adoption and when somebody changes their mind. No
comparable tool has one — `ruff`, `black`, `prettier`, `rustfmt` and `biome` all
offer generation or migration and no `set`.

The use that would justify it is scripted rollout across many repositories,
where `sed` is genuinely awkward because it has to *uncomment* a default line.
If that turns up, build it on `toml_edit`.

### The linter and the formatter cannot disagree about width

Setting `format.line-length` moves `lint.max-line-length` with it unless that
one is set too. Left independent, the common case — a project that wants 120
columns — produces a linter reporting exactly the lines the formatter just
made. The same rule applies when the width comes from a `gdformatrc`, which has
nowhere to say otherwise.
