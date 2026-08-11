# Contributing

Thanks for looking. This is an early-stage project, so the most useful
contributions right now are the formatter and the linter — see
[docs/DESIGN.md](docs/DESIGN.md) for what each is meant to become.

## Getting set up

Rust 1.85 or newer (the crates use edition 2024).

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

If you have a Godot project to hand, pointing `GDCK_CORPUS` at it is a good way
to find grammar gaps. Please do report any file that fails to round-trip.

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
