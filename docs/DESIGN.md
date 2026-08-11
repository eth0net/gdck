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
| `gdck-lint` | Tree → diagnostics, some carrying fixes. |
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

## Planned: the formatter

A Wadler-style pretty printer in two stages.

1. **Lower** the CST to a document IR of `Text`, `Line`, `Group` and `Indent`.
   Comments and blank-line runs ride along as attachments on the construct they
   lead or trail.
2. **Render** at the configured width, breaking the outermost group that does
   not fit and recurring inwards.

Rules that fall out of the IR: the 100-column wrap, one space around operators
and after commas, two blank lines around top-level definitions and one inside
classes, trailing commas on anything that breaks across lines, single-indent
continuations for collections against double-indent for wrapped expressions.

Rules needing explicit handling: double quotes unless that adds escapes,
lowercase hex, leading and trailing zeros on floats, dropping redundant
parentheses.

### Safety checks

Before writing, the formatter re-parses its own output and verifies that the
token stream is equivalent modulo trivia, that a second pass is a no-op, and
that no comment was dropped. A formatter that silently eats code is much worse
than one that refuses to run. On by default; `--fast` turns them off.

## Planned: the linter

Rules are visitors over the CST run in a single pass, each producing a
diagnostic with a byte range and optionally an edit. `--fix` applies
non-overlapping edits back to front so earlier offsets stay valid.

Suppression reuses the comment syntax GDScript projects already have:
`# gdlint: ignore=rule-name` for one line, `# gdlint: disable=rule-name` and
`enable=` for a region.

The rule catalogue lives in `crates/gdck-lint/src/lib.rs` as `PLANNED_RULES`.

### Code order

The analysis behind the all-or-nothing policy is in the
[README](../README.md#code-order-is-fixed-for-a-whole-file-or-not-at-all).
Implementation notes:

- Build the target order from the style guide's sequence, keeping declarations
  within a bucket in their original relative order.
- Build dependency edges between class-level variables. A direct reference to
  another member is a precise edge. Anything opaque — a call to a file-local
  function, `self`, a subscript of something unknown — conservatively depends on
  every member declared above it.
- If the target order violates no edge, rewrite the file's order. Otherwise
  leave it untouched and report, naming the declaration that blocked it.

The over-approximation is the point. Precise interprocedural analysis would be
needed to know whether `var total := _compute()` reads `_base` inside
`_compute`, and being wrong there breaks a game at runtime with no error.

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
