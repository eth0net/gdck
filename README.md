# gdck

A fast GDScript formatter and linter, faithful to the [official GDScript style
guide][styleguide].

> [!WARNING]
> **Work in progress.** The lexer and parser are done and tested; the formatter
> and linter are not written yet. Only `gdck parse` does anything useful today.
> See [Status](#status).

`gdck` is a ground-up reimplementation of [`godot-gdscript-toolkit`][gdtoolkit]
in Rust. It is designed to install alongside the original rather than replace it
in place, so you can try it on a project without giving up a working setup.

[styleguide]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html
[gdtoolkit]: https://github.com/Scony/godot-gdscript-toolkit

## Why

**Speed.** Formatters and linters run on every save and every commit, so startup
cost is most of what you actually feel. Parsing a 324-file, 7,800-line corpus:

| | wall clock |
|---|---|
| `gdck parse` | ~4 ms |
| `gdparse` | ~70 ms |

Most of that gap is Python interpreter startup rather than parsing throughput —
which is the point, since that cost is paid on every invocation. Measured on an
M-series Mac over 10 runs; reproduce with the corpus test described below.

**Style-guide fidelity.** The style guide says more than `gdtoolkit` enforces —
notably [code order][order], quote style, number formatting, and when to use
`:=` against an explicit type. `gdck` aims to cover the guide, and to auto-fix
what can be fixed safely.

**A reusable parser.** [`gdck-syntax`](crates/gdck-syntax) is a standalone
lossless GDScript parser. Editors, doc generators and static analysers need one
too, and there is no reason for each to write its own.

[order]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html#code-order

## Install

```sh
cargo install --git https://github.com/eth0net/gdck gdck-cli
```

The binary is `gdck`. It does not install `gdformat`, `gdlint` or `gdparse`, so
it will never collide with `gdtoolkit` on your `PATH`.

## Usage

One rule governs the whole interface:

> **Nothing is written to disk unless you pass `--fix`.**

```sh
gdck check .              # report everything, read-only
gdck fix .                # apply everything

gdck format src/          # report formatting differences
gdck format --fix src/    # apply them

gdck lint src/            # report lint problems
gdck lint --fix src/      # apply the fixable ones

gdck parse src/           # report syntax errors
gdck parse --tree a.gd    # dump the syntax tree
gdck parse --tokens a.gd  # dump the token stream
```

`check` and `fix` are the everything-at-once verbs; `format` and `lint` are the
narrower ones. `--check` is accepted everywhere as a no-op, because people
coming from `gdformat` and `black` will type it out of habit.

Exit codes: `0` clean, `1` problems found, `2` the run could not complete.

### Reading from standard input

```sh
echo 'pass' | gdck parse -
```

## Design decisions

A few choices worth stating up front, because they differ from what you might
expect.

### Reading is the default, writing is opt-in

Formatters conventionally write by default and linters conventionally report by
default. One tool doing both jobs cannot follow both conventions without
becoming a per-command fact you have to memorise, so `gdck` follows neither:
`--fix` writes, nothing else does. The cost is that `gdck format a.gd` does not
reformat `a.gd` — it tells you it would, and `gdck fix .` is the short way to
mean it.

### Code order is fixed for a whole file or not at all

Class-level initialisers run in declaration order, so reordering is not purely
cosmetic:

```gdscript
var _config := preload("res://config.tres")
var speed := _config.default_speed
```

The style guide puts public variables before private ones, so a naive reorder
hoists `speed` above `_config` and it silently initialises from `null`.

`gdck` treats each file as all-or-nothing: either every required move is
provably safe and the file is reordered, or the file is left exactly as it was
and the problem is reported. A partially reordered file would satisfy neither
the original layout nor the style guide, and would still be flagged by the next
`gdck check`.

Safety is judged conservatively. Any initialiser that is not self-contained — it
calls a function defined in the file, touches `self`, or is otherwise opaque —
is treated as potentially reading every member above it. That occasionally
blocks a file that would have been fine, but it cannot be wrong in the direction
that breaks your game. Signals, enums, constants and functions carry no ordering
semantics at all, so files needing only those moved always sort cleanly.

Reordering is opt-in (`--fix-order`) until the analysis has more mileage.

### The tree keeps every byte

Whitespace, comments and blank lines are all nodes in the syntax tree, and the
source is always exactly recoverable from it. This is what lets the formatter
rewrite one declaration while leaving the comments around it untouched, and it
holds even for files with syntax errors — a formatter must never damage a file
it merely failed to understand.

## Status

| Component | State |
|---|---|
| `gdck-syntax` — lexer | Done. Indentation, all literal forms, multi-line lambdas |
| `gdck-syntax` — parser | Done. Full declaration and expression grammar, error recovery |
| `gdck-config` | Types and file discovery done; reading `gdck.toml` not wired up |
| `gdck-format` | Not started. Design notes in the crate docs |
| `gdck-lint` | Not started. Rule catalogue in the crate docs |
| `gdck parse` | Works |
| `gdck check` / `fix` / `format` / `lint` | Interface defined, exits 2 |

The parser handles every file in `gdtoolkit`'s corpus of valid GDScript — 324
files across its parser, formatter and `gd2py` test suites — and round-trips all
353 files there, including the deliberately invalid ones.

### Known gaps

- Configuration files are not read yet; every run uses style-guide defaults.
- No `.gitignore` awareness when walking directories; only a fixed exclusion
  list (`.git`, `.godot`, `.import`, `addons`).
- The parser is more permissive than Godot in places. It is built to understand
  well-formed code, not to reject every invalid program — Godot is the authority
  on what compiles.
- Three files in `gdtoolkit`'s `potential-godot-bugs` corpus do not parse. They
  document cases where Godot's own behaviour is in question.

## Development

```sh
cargo test                 # unit and integration tests
cargo clippy --all-targets # lints, warning-free
cargo fmt --all
```

The parser is validated against an external corpus when you point it at one:

```sh
GDCK_CORPUS=../godot-gdscript-toolkit/tests \
  cargo test -p gdck-syntax --test corpus -- --nocapture
```

This checks that every `.gd` file under that directory round-trips through the
tree byte for byte. It is skipped when the variable is unset.

See [docs/DESIGN.md](docs/DESIGN.md) for architecture, and
[CONTRIBUTING.md](CONTRIBUTING.md) to get started.

## Credit

[`godot-gdscript-toolkit`][gdtoolkit] by Paweł Lampe is the reference this
project measures itself against, and its test corpus has been invaluable for
finding the corners of the grammar. It is MIT licensed; any vendored fixtures
keep that license and attribution in [`licenses/`](licenses).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
