# Design

How `gdck` is put together, and why. This documents decisions that are not
obvious from the code, so that revisiting them later is a choice rather than an
accident.

## Crate layout

| Crate | Responsibility |
|---|---|
| `gdck-syntax` | Lexer, lossless CST, parser. No knowledge of formatting or rules. |
| `gdck-config` | Configuration types, defaults, file discovery. Dependency-free. |
| `gdck-format` | Tree → formatted text. |
| `gdck-lint` | Tree → diagnostics, some carrying fixes. Uses `gdck-format` for literal rewrites. |
| `gdck-cli` | The `gdck` binary. Argument parsing, file walking, reporting. |

The split exists so `gdck-syntax` can be published and depended on by itself. A
fast GDScript parser is useful to editors, documentation generators and static
analysers, none of which want a linter attached.

`gdck-config` deliberately has no dependencies. It holds the naming patterns and
thresholds that both `gdck-format` and `gdck-lint` need, and keeping it inert
means neither picks up a serialisation stack it does not use.

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

Three samples are not reproducible by any deterministic rule, because they are
hand-formatted rather than derived from the column limit: two wrap a call by
filling several arguments per line, and one wraps a boolean chain two
comparisons per line. In each case the guide's break points do not match where a
real fill at 100 columns would land. `gdck` puts one item per line, which is
deterministic and produces better diffs. They are listed as exceptions in the
conformance test so the deviation cannot become accidental.

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

## Planned: configuration

`gdck.toml` or `.gdck.toml`, discovered by walking up from the working
directory. Sketch:

```toml
[format]
line-length = 100
indent = "tabs"      # or { spaces = 4 }
safety-checks = true

[lint]
max-line-length = 100
max-file-lines = 1000
code-order = "report"   # "report" | "fix-when-safe" | "off"
disable = ["max-returns"]

[files]
exclude = [".git", ".godot", ".import", "addons"]
```

A compatibility shim for `gdlintrc` and `gdformatrc` is worth having so existing
projects work unchanged, but `gdck.toml` should win where both are present.
