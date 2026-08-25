# gdck

[![crates.io](https://img.shields.io/crates/v/gdck.svg)](https://crates.io/crates/gdck)
[![CI](https://github.com/eth0net/gdck/actions/workflows/ci.yml/badge.svg)](https://github.com/eth0net/gdck/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/gdck.svg)](#license)
[![prek](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/j178/prek/master/docs/assets/badge-v0.json)](https://github.com/j178/prek)

A fast GDScript formatter, linter and language server, faithful to the
[official GDScript style guide][styleguide]. One binary, no Python, no runtime.

A ground-up reimplementation of [`godot-gdscript-toolkit`][gdtoolkit] in Rust.
It installs beside the original rather than over it, so you can try it without
giving up a working setup.

| | `gdck` | `gdtoolkit` | |
|---|---|---|---|
| parse | 10 ms | 433 ms | 44× |
| format | 32 ms | 683 ms | 22× |
| lint | 20 ms | 657 ms | 33× |

<sub>327 files, 7,846 lines — `gdtoolkit`'s own corpus, counting only files both
tools accept. Whole processes, startup included. Reproduce with
[`tools/benchmark.py`](tools/benchmark.py).</sub>

> [!NOTE]
> **Complete but young.** Parser, formatter and linter are done and tested, and
> one Godot project uses it daily on every commit and in CI. Worth running
> alongside your existing setup before you trust it with `--fix`.
> See [docs/STATUS.md](docs/STATUS.md).

[styleguide]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html
[gdtoolkit]: https://github.com/Scony/godot-gdscript-toolkit

## Install

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

**Cargo**, if you have a Rust toolchain:

```sh
cargo install gdck
```

The installers put `gdck` in `~/.local/bin` and add it to your `PATH`. Set
`GDCK_INSTALL_DIR` to put it elsewhere. Per-platform archives, each with a
`.sha256`, are on
[the latest release](https://github.com/eth0net/gdck/releases/latest).

Nothing here installs `gdformat`, `gdlint` or `gdparse`, so it will not collide
with `gdtoolkit` on your `PATH`.

## Usage

> **Nothing is written to disk unless you pass `--fix`.**

```sh
gdck check .              # report everything, read-only
gdck fix .                # apply everything
gdck fix --fix-order .    # and reorder declarations where that is provably safe

gdck format src/          # report formatting differences
gdck lint src/            # report lint problems
gdck parse src/           # report syntax errors

gdck config               # what settings this run would use
gdck init                 # write them to a gdck.toml
gdck lsp                  # run as a language server
```

`check` and `fix` are the everything-at-once verbs; `format` and `lint` are the
narrower ones. Each takes `--fix`, `--diff` and `--output json`, and reading
from `-` reads standard input.

Exit codes: `0` clean, `1` problems found, `2` the run could not complete.

No configuration is needed — every default is the style guide's. A project with
a `gdformatrc` or `gdlintrc` keeps its settings without writing anything new.
→ [docs/CONFIG.md](docs/CONFIG.md)

## Where it fits

**In an editor.** `gdck lsp` gives diagnostics as you type, format on save, and
each fixable rule's own fix as a quick action. It runs alongside Godot's own
language server rather than replacing it: that one knows the project, this one
knows the style guide. → [docs/EDITORS.md](docs/EDITORS.md)

**As a git hook.** Four hooks for [`prek`](https://prek.j178.dev) and
[`pre-commit`](https://pre-commit.com), including drop-in replacements for
`gdformat` and `gdlint`. They fetch the prebuilt binary for the revision you
pin, so there is no Rust toolchain and no Python to install.
→ [docs/HOOKS.md](docs/HOOKS.md), [`hooks/examples/`](hooks/examples)

```toml
# prek.toml
[[repos]]
repo = "https://github.com/eth0net/gdck"
rev = "v0.8.0"
hooks = [{ id = "gdck-fix" }]
```

**In CI.** No runtime to set up, so it is one step:

```yaml
- run: curl --proto '=https' --tlsv1.2 -LsSf https://github.com/eth0net/gdck/releases/latest/download/gdck-installer.sh | sh
- run: gdck check .
```

**In another tool.** `--output json` writes one object per line, carrying the
rule, the message, the position as line, column and byte offset, and — for a
fixable rule — the edits to apply. Summaries stay on standard error, so
standard output is nothing but records.

## Why not `gdtoolkit`

**Speed**, as above. It runs on every save and every commit, so startup cost
and throughput are both things you feel.

**Style-guide fidelity.** The guide asks for more than `gdtoolkit` enforces —
[code order][order], quote style, number formatting, comment spacing, and when
to use `:=` against an explicit type. `gdck` covers the guide and auto-fixes
what can be fixed safely. → [docs/RULES.md](docs/RULES.md)

**A reusable parser.** [`gdck-syntax`](crates/gdck-syntax) is a standalone
lossless GDScript parser. Editors, doc generators and static analysers all need
one, and there is no reason for each to write its own.

[order]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html#code-order

## Documentation

| | |
|---|---|
| [RULES.md](docs/RULES.md) | Every rule, which are fixable, how to switch one off |
| [CONFIG.md](docs/CONFIG.md) | Every setting, and migrating from `gdtoolkit` |
| [HOOKS.md](docs/HOOKS.md) | Git hooks for `prek` and `pre-commit` |
| [EDITORS.md](docs/EDITORS.md) | The language server, and editor configurations |
| [STATUS.md](docs/STATUS.md) | What is done, what it is tested against, known gaps |
| [DESIGN.md](docs/DESIGN.md) | Why it behaves as it does, and how it is built |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Getting set up, and what CI runs |

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
