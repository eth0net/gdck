# Contributing

Thanks for looking. The parser, formatter and linter are all working, so the
most useful contributions now are rules the style guide asks for and `gdck`
does not check yet, and any file that it gets wrong — see
[docs/DESIGN.md](docs/DESIGN.md) for how the pieces fit together.

## Getting set up

Rust 1.88 or newer.

```sh
git clone https://github.com/eth0net/gdck
cd gdck
cargo test
```

## Before opening a pull request

```sh
cargo fmt --all
cargo clippy --all-targets    # must be warning-free
cargo test
```

CI runs the same three on Linux, macOS and Windows.

### Or let the hooks run them

[`prek`][prek] runs the same checks from `prek.toml`, split so that committing
stays quick and the compiling happens on push:

```sh
brew install prek     # or: cargo install --git https://github.com/j178/prek
prek install          # installs both the pre-commit and pre-push hooks
```

Committing runs the file-hygiene checks, `cargo fmt` and `cargo clippy`.
Pushing adds `cargo test`, the doctests and the MSRV check — which is
everything CI runs except the corpus jobs, since those need
`godot-gdscript-toolkit` checked out. `prek run --all-files` runs the lot on
demand, and `git commit --no-verify` skips them when you know better.

The hygiene hooks are prek's own builtins rather than hooks from a remote
repository, so there is nothing to clone and no Python to install: a fresh
checkout can run them offline.

The fixture directories are excluded from the whitespace hooks. Their value is
being byte-for-byte what `gdtoolkit` and the style guide produced, so nothing
may tidy them — least of all a whitespace fixer, when one of the rules under
test is `trailing-whitespace`.

[prek]: https://prek.j178.dev

### The hooks this project ships

Separate from the hooks used to develop it, `gdck` publishes hook definitions
for other projects to consume — `.pre-commit-hooks.yaml` and the `hooks/gdck`
script that fetches the binary. [docs/HOOKS.md](docs/HOOKS.md) documents them.

None of it is reachable from `cargo test`, so it has its own check:

```sh
cargo build -p gdck
tools/test-hooks.sh
```

That runs the sample configurations from `hooks/examples/` under both `prek`
and `pre-commit` against a throwaway project, checks every hook id in
`.pre-commit-hooks.yaml` resolves, then separately downloads the newest
published release to prove the fetch path still works. It needs `uv` and a
network connection. CI runs it on every push, along with `shellcheck` over both
scripts.

A new hook in `.pre-commit-hooks.yaml` is picked up by the id check on its own,
since that reads the ids from the file. Mentioning it in the two samples is the
part nothing checks for you.

If you edit `hooks/gdck` or anything in `tools/`, the commit hooks run
`shellcheck` over it, which means having it installed:

```sh
brew install shellcheck
```

## Adding a dependency

The deliverable is a single self-contained binary, which is a statement about
what ships rather than about what builds — build-time crates are chosen on
merit, and a proven one is preferred to an imitation of it every time. See the
Dependencies section of [docs/DESIGN.md](docs/DESIGN.md).

What a new dependency does have to clear is `deny.toml`:

```sh
cargo install cargo-deny
cargo deny check
```

Four checks: security advisories, licences, duplicate versions, and where each
crate came from. CI runs the same thing. A dependency whose licence is not
already in the allow list is a licensing decision rather than a build failure —
the binary is distributed under `MIT OR Apache-2.0`, so anything linked into it
has to be compatible with both.

It is not in the hook set, because it needs the network to fetch the advisory
database and pushing offline should still work.

## Testing against a real corpus

The parser is validated against an external directory of `.gd` files:

```sh
git clone https://github.com/Scony/godot-gdscript-toolkit ../godot-gdscript-toolkit
GDCK_CORPUS=../godot-gdscript-toolkit/tests \
  cargo test -p gdck-syntax --test corpus -- --nocapture
```

Every file must round-trip through the tree byte for byte. Parse *errors* are
not failures there — that corpus contains deliberately invalid scripts — but
losslessness has to hold for all of them.

The formatter and the linter have matching corpus tests, run the same way:

```sh
GDCK_CORPUS=../godot-gdscript-toolkit/tests \
  cargo test -p gdck-format --test corpus -- --nocapture
GDCK_CORPUS=../godot-gdscript-toolkit/tests \
  cargo test -p gdck-lint --test corpus -- --nocapture
```

All three are skipped when the variable is unset.

If you have a Godot project to hand, pointing `GDCK_CORPUS` at it is a good way
to find grammar gaps. Please do report any file that fails to round-trip.

The formatter's other fixtures are the style guide's own code samples,
regenerated from a checkout of the Godot documentation:

```sh
cargo build -p gdck
tools/extract-style-guide-samples.py ../godot-docs
```

## Working on the parser

A few things worth knowing before you start:

- **Losslessness is not negotiable.** Every byte of input must end up in exactly
  one token, and every token in the tree. If you add a token kind, add it to a
  round-trip test.
- **The parser must not fail.** Unparseable input becomes an `Error` node and a
  diagnostic. Every loop goes through `ensure_progress` so a rule that consumes
  nothing cannot hang the parser.
- **New grammar needs a regression test in `crates/gdck-syntax/src/lib.rs`.**
  The corpus test is a safety net, not a substitute — it only runs when someone
  has a corpus checked out.

The indentation and multi-line-lambda handling in the lexer is the subtlest part
of the codebase. `docs/DESIGN.md` explains the rules; please read that section
before changing it, and add a case to `parses_multiline_lambdas_inside_brackets`
for anything new.

## Adding a lint rule

1. Add it to `PLANNED_RULES` in `crates/gdck-lint/src/lib.rs` if it is not there.
2. Name it in kebab-case, matching the style guide's wording where one applies.
3. Rules that can be fixed mechanically should carry an edit. Rules that cannot
   be fixed *safely* should not — see the code-order discussion in the design
   doc for what "safely" is doing there.
4. Anything configurable belongs in `gdck-config`, with a style-guide-derived
   default.

## Commit and PR style

Small, focused commits with a short imperative subject. Explain *why* in the
body when it is not obvious; the code already says what.

## Licensing

Contributions are dual licensed under MIT and Apache-2.0, matching the project.
By submitting a pull request you agree to that.
