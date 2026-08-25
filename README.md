# gdck

[![crates.io](https://img.shields.io/crates/v/gdck.svg)](https://crates.io/crates/gdck)
[![CI](https://github.com/eth0net/gdck/actions/workflows/ci.yml/badge.svg)](https://github.com/eth0net/gdck/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/gdck.svg)](#license)

A fast GDScript formatter and linter, faithful to the [official GDScript style
guide][styleguide].

> [!NOTE]
> **Complete but young.** The parser, formatter and linter are done and tested,
> every subcommand works, there are git hooks and a language server, and one
> Godot project uses it daily on every commit and in CI. It has also been run
> against `gdtoolkit`'s corpus and against real Godot builds. It is still new
> enough to be worth running alongside your existing setup before you trust it
> with `--fix`. See [Status](#status).

`gdck` is a ground-up reimplementation of [`godot-gdscript-toolkit`][gdtoolkit]
in Rust. It is designed to install alongside the original rather than replace it
in place, so you can try it on a project without giving up a working setup.

[styleguide]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html
[gdtoolkit]: https://github.com/Scony/godot-gdscript-toolkit

## Why

**Speed.** Formatters and linters run on every save and every commit, so both
startup cost and throughput are things you actually feel. Over `gdtoolkit`'s
own corpus — 327 files and 7,846 lines, counting only the files both tools
accept:

| | `gdck` | `gdtoolkit` | |
|---|---|---|---|
| parse | 10 ms | 433 ms | 44× |
| format | 32 ms | 683 ms | 22× |
| format, safety checks off | 20 ms | — | |
| lint | 20 ms | 657 ms | 33× |

Whole processes, interpreter startup included, because that is what you wait
for and it is paid on every invocation. Both tools re-parse their own output to
verify it, which is most of what formatting costs; `--fast` turns that off in
`gdck` and is the row to compare against a formatter that does not do it.

Measured on an M-series Mac by [`tools/benchmark.py`](tools/benchmark.py),
which is in the repository so this is a claim you can check rather than one you
have to take on trust:

```sh
cargo build --release
tools/benchmark.py ../godot-gdscript-toolkit
```

**Style-guide fidelity.** The style guide says more than `gdtoolkit` enforces —
notably [code order][order], quote style, number formatting, comment spacing,
and when to use `:=` against an explicit type. `gdck` covers the guide, and
auto-fixes what can be fixed safely. The rule catalogue is in
[docs/RULES.md](docs/RULES.md).

**One binary, everywhere it is wanted.** The same `gdck` is a command-line
tool, a [git hook](docs/HOOKS.md) for `prek` and `pre-commit`, and a
[language server](docs/EDITORS.md) for your editor. No Python, no runtime, no
toolchain — and it installs beside `gdtoolkit` rather than over it.

**A reusable parser.** [`gdck-syntax`](crates/gdck-syntax) is a standalone
lossless GDScript parser. Editors, doc generators and static analysers need one
too, and there is no reason for each to write its own.

[order]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html#code-order

## Install

Every route below installs one binary, `gdck`, and needs nothing else present
— no Python, no runtime, no toolchain. It does not install `gdformat`,
`gdlint` or `gdparse`, so it will never collide with `gdtoolkit` on your
`PATH`.

**macOS and Linux**

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/eth0net/gdck/releases/latest/download/gdck-installer.sh | sh
```

**Windows**

```powershell
irm https://github.com/eth0net/gdck/releases/latest/download/gdck-installer.ps1 | iex
```

**Homebrew**

```sh
brew install eth0net/tap/gdck
```

The installers put `gdck` in `~/.local/bin` and add that to your `PATH` if it
is not there already. Set `GDCK_INSTALL_DIR` to put it somewhere else.

**Cargo**, if you already have a Rust toolchain:

```sh
cargo install gdck
```

Or take an archive for your platform from
[the latest release](https://github.com/eth0net/gdck/releases/latest). Each one
carries a `.sha256` beside it. Downloading through a browser is the one route
where macOS and Windows will ask whether you trust an unsigned binary; the
installers above are fetched rather than downloaded and are not marked that
way.

### In CI

There is no runtime to set up, so a Godot project's workflow can have the
linter in one step:

```yaml
- run: curl --proto '=https' --tlsv1.2 -LsSf https://github.com/eth0net/gdck/releases/latest/download/gdck-installer.sh | sh
- run: gdck check .
```

### As a git hook

`gdck` ships hook definitions for [`prek`](https://prek.j178.dev) and
[`pre-commit`](https://pre-commit.com). Both read the same file from this
repository, so either tool works from the same definitions.

```toml
# prek.toml
[[repos]]
repo = "https://github.com/eth0net/gdck"
rev = "v0.7.0"
hooks = [{ id = "gdck-fix" }]
```

```yaml
# .pre-commit-config.yaml
repos:
  - repo: https://github.com/eth0net/gdck
    rev: v0.7.0
    hooks:
      - id: gdck-fix
```

The hooks fetch the prebuilt binary for the revision you pinned, so they need
no Rust toolchain and no Python. `gdck-format` and `gdck-lint` are also
available as drop-in replacements for `gdformat` and `gdlint`.

Both files above are in [`hooks/examples/`](hooks/examples) in full, with the
alternatives commented, ready to copy.
[docs/HOOKS.md](docs/HOOKS.md) covers all four hooks, migrating an existing
`gdtoolkit` config, and why `exclude` behaves differently under a hook.

## Usage

One rule governs the whole interface:

> **Nothing is written to disk unless you pass `--fix`.**

```sh
gdck check .              # report everything, read-only
gdck fix .                # apply everything
gdck fix --fix-order .    # and reorder declarations where that is provably safe

gdck format src/          # report formatting differences
gdck format --fix src/    # apply them

gdck lint src/            # report lint problems
gdck lint --fix src/      # apply the fixable ones

gdck parse src/           # report syntax errors
gdck parse --tree a.gd    # dump the syntax tree
gdck parse --tokens a.gd  # dump the token stream

gdck config               # what settings this run would use
gdck init                 # write them to a gdck.toml

gdck lsp                  # run as a language server, for an editor to start
```

`check` and `fix` are the everything-at-once verbs; `format` and `lint` are the
narrower ones. `--check` is accepted everywhere as a no-op, because people
coming from `gdformat` and `black` will type it out of habit.

Reports go to standard output and summaries to standard error, so standard
output carries only content — the diagnostics, the diff under `--diff`, or the
file itself when reading from `-`.

Exit codes: `0` clean, `1` problems found, `2` the run could not complete.

### In an editor

`gdck lsp` runs as a language server over stdin and stdout: problems as you
type, format document, and each fixable rule's own fix as a quick action.

```lua
-- Neovim, with the built-in client
vim.lsp.config.gdck = { cmd = { "gdck", "lsp" }, filetypes = { "gdscript" } }
vim.lsp.enable("gdck")
```

It runs alongside Godot's own language server rather than replacing it: that
one knows the project, this one knows the style guide.
[docs/EDITORS.md](docs/EDITORS.md) has configurations for Neovim, Helix, Zed
and VS Code, and what the server deliberately does not offer.

### Machine-readable output

`--output json` reports one JSON object per line, for an editor or a script
rather than a person:

```sh
gdck check --output json .
```

```json
{"file":"a.gd","rule":"quote-style","severity":"warning","message":"write `'x'` as `\"x\"`","range":{"start":{"line":2,"column":9,"offset":21},"end":{"line":2,"column":12,"offset":24}},"fix":{"edits":[{"range":{"start":{"line":2,"column":9,"offset":21},"end":{"line":2,"column":12,"offset":24}},"text":"\"x\""}]}}
```

Every record carries the same fields whatever produced it — a lint rule, a
syntax error (`"rule":"syntax-error"`) or a file the formatter would rewrite
(`"rule":"format"`) — so a consumer reads one list rather than merging three.
Positions come as line, column *and* byte offset, because editors disagree
about which they want and all three are already known. A fixable rule carries
the edits, so a tool can apply them without running `gdck` again.

Summaries stay on standard error, so standard output is nothing but records.
One object per line rather than one array: a run is a stream, so output arrives
as it is produced and survives being cut off with every completed record still
valid. `jq -s .` collects it into an array.

Every rule is listed in [docs/RULES.md](docs/RULES.md), along with how to turn
one off for a line, a region or the project.

### Configuration

None is needed — every default is the style guide's. To depart from it, write a
`gdck.toml`:

```toml
[format]
line-length = 100

[lint]
max-returns = 6
disable = ["max-public-methods"]
```

`gdck` searches upwards from the paths you gave it. With no `gdck.toml` it falls
back to `gdtoolkit`'s own `gdformatrc` and `gdlintrc`, so a project already set
up for `gdformat` and `gdlint` keeps its settings without writing anything new.
`gdck config` prints what a run would actually use, and
[docs/CONFIG.md](docs/CONFIG.md) documents every setting.

`gdck init` writes that `gdck.toml` for you, carrying a `gdformatrc` or
`gdlintrc` across if there is one — losslessly, and naming anything it could not
carry. Settings left at the style guide's default are written commented out, so
the file says what your project decided rather than pinning today's defaults.

Walking a directory skips `.git`, `.godot`, `.import` and `addons`, and anything
your `.gitignore` covers — a `.gd` file git ignores is nearly always generated.
Change the first with `files.exclude`; turn the second off with `--no-gitignore`.
A file you name on the command line is always processed.

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

What a reorder produces is a *permutation of the source* — the same bytes in a
different order — rather than a re-rendering of it. A declaration therefore
takes its comments and annotations with it exactly as written, and nothing can
be lost on the way. Its blank lines travel with it too, which the formatter then
settles; `gdck fix --fix-order` runs both.

Reordering is opt-in (`--fix-order`) until the analysis has more mileage.

### The linter agrees with `gdtoolkit` where it should

Existing GDScript projects have already triaged `gdlint`'s findings, so a
reimplementation that quietly widened a rule would present old code as newly
broken. Over `gdtoolkit`'s own test corpus the two agree exactly on
`unused-argument` (270 findings), `constant-name`, `trailing-whitespace`,
`function-name`, `enum-member-name`, `argument-name`, `enum-name` and
`max-arguments`.

Where they differ, they differ on purpose, and
[docs/RULES.md](docs/RULES.md#differences-from-gdtoolkit) says why. The shortest
version: `gdck` measures line length in columns rather than characters, so a
tab-indented line counts the same to the linter as to the formatter; it reports
a `pass` beside a `func` that `gdlint`'s statement test does not see; and it
checks a good deal more of the guide's declaration order.

`gdtoolkit`'s rule names are accepted as aliases everywhere a rule is named, so
an existing `gdlintrc` and existing `# gdlint: ignore=` comments keep working.

### Formatting you asked for is formatting you keep

The style guide shows `var array = [1, 2, 3]` and the same array spread over
four lines as *both* good, and marks an 83-column `if` as bad for not being
wrapped. Neither follows from a column limit, so `gdck` treats the author's own
line breaks as the deciding vote: a bracketed construct written across several
lines stays that way and gains its trailing comma, and one written on a single
line stays there if it fits. To collapse a construct, delete the newlines.

### Nothing is written until the output has been checked

Before returning, the formatter re-parses its own output and verifies that it
parses, that the tree still means the same thing, that no comment was lost, and
that a second pass changes nothing. If any of those fail, the file is left alone
and the reason is reported. `--fast` turns them off.

These are not decoration. Every one of them caught real bugs while the formatter
was being written, including a lexer bug where a single-line lambda inside a
wrapped argument list swallowed its own closing bracket. A formatter that
silently eats code is far worse than one that refuses to run.

The equivalence used is tree shape rather than token stream, since the guide
asks for rewrites that move tokens: hoisting an inner class's `extends` onto the
declaration line, and dropping redundant parentheses. Comparing shape is weaker
in exactly those places and stronger where it matters — grouping is encoded by
the nesting, so a parenthesis that actually mattered shows up as a differently
shaped expression rather than as two missing tokens.

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
| `gdck-config` | Done. `gdck.toml`, plus `gdformatrc` and `gdlintrc` for compatibility |
| `gdck-format` | Done. Wadler pretty printer with safety checks |
| `gdck-lint` | Done. 36 rules, 12 of them fixable. See [docs/RULES.md](docs/RULES.md) |
| Every subcommand | Works |
| Git hooks | Done. `prek` and `pre-commit`. See [docs/HOOKS.md](docs/HOOKS.md) |
| Language server | Diagnostics, formatting and quick fixes. See [docs/EDITORS.md](docs/EDITORS.md) |

The parser handles every file in `gdtoolkit`'s corpus of valid GDScript — 324
files across its parser, formatter and `gd2py` test suites — and round-trips all
353 files there, including the deliberately invalid ones. The formatter formats
every one of those valid files with its safety checks passing.

Beyond that corpus it has been run against two real Godot projects, a 243-file
game and a 488-file addon. Every formatted file in the corpus is fed to a real
Godot build in CI and has to be accepted by the engine's own parser, so
"formats correctly" means Godot agrees rather than that `gdck` agrees with
itself. Reordering is held to a stronger promise still: it may only ever permute
a file's declarations, checked against both of those projects as well as the
corpus.

The formatter is also tested against the style guide's own worked examples: all
56 usable code samples are extracted from the documentation and asserted on
directly, so the guide decides what correct output is. See
[`crates/gdck-format/tests/style_guide.rs`](crates/gdck-format/tests/style_guide.rs)
for the classification of every sample, including the three the formatter
knowingly does not reproduce and why.

The linter is held to the same corpus, on what it must never do rather than on
what it finds: `--fix` never turns a file that parsed into one that does not,
settles after one more pass, and never introduces a kind of problem the file did
not already have. `--fix-order` only ever permutes a file's bytes.

### Known gaps

- One configuration governs a whole run, found from the directory the given
  paths have in common. A repository holding several Godot projects with
  different settings needs one invocation per project.
- Naming conventions are not configurable. `gdck` checks the style guide's
  conventions directly rather than with a regular-expression engine, so a
  `gdlintrc` that customised one is reported as not applied rather than
  honoured. See [docs/CONFIG.md](docs/CONFIG.md#gdtoolkit-compatibility).
- `code-order` cannot tell an overridden virtual method from a custom one, since
  that needs the whole inheritance chain. It orders the callbacks the guide names
  and leaves the rest as one group, rather than guessing. See
  [docs/RULES.md](docs/RULES.md#what-code-order-does-and-does-not-check).
- Naming rules never offer a rename. A name is reached from scene files, from
  `call()` with a string, and from signals connected in the editor, none of which
  one file can see.
- CRLF and CR line endings are rewritten to LF, which the style guide mandates.
  There is no option to keep them.
- The formatter does not fill lines. Where the style guide hand-wraps a call or
  a boolean chain several items per line, `gdck` puts one per line. Both of the
  guide's samples of that are hand-formatted rather than derived from a column
  limit, so no deterministic rule reproduces them.
- The guide calls itself advice rather than a rulebook, and `gdck` turns it into
  exit codes. Everywhere that needed a decision the guide did not make — a
  trailing comma Godot requires, an order it stopped asking for, a length
  measured in columns — is listed in
  [docs/DESIGN.md](docs/DESIGN.md#known-differences-from-the-guide).
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
tree byte for byte. The formatter has a matching one:

```sh
GDCK_CORPUS=../godot-gdscript-toolkit/tests \
  cargo test -p gdck-format --test corpus -- --nocapture
```

And so does the linter:

```sh
GDCK_CORPUS=../godot-gdscript-toolkit/tests \
  cargo test -p gdck-lint --test corpus -- --nocapture
```

All three are skipped when the variable is unset.

The style-guide fixtures are regenerated from a checkout of the documentation:

```sh
cargo build -p gdck
tools/extract-style-guide-samples.py ../godot-docs
```

See [docs/DESIGN.md](docs/DESIGN.md) for architecture, and
[CONTRIBUTING.md](CONTRIBUTING.md) to get started.

## Credit

[`godot-gdscript-toolkit`][gdtoolkit] by Paweł Lampe is the reference this
project measures itself against, and its test corpus has been invaluable for
finding the corners of the grammar. It is MIT licensed; any vendored fixtures
keep that license and attribution in [`licenses/`](licenses).

The formatter's test fixtures are the code samples from the GDScript style
guide, part of the Godot documentation and licensed CC BY 3.0. Attribution is
in [`licenses/`](licenses).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
