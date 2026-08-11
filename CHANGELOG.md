# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `gdck-config`: configuration files are read, and `gdck config` prints the
  settings a run would use.
  - `gdck.toml` or `.gdck.toml`, found by walking up from the directory the
    given paths have in common. Every setting is documented in
    [docs/CONFIG.md](docs/CONFIG.md).
  - `gdformatrc` and `gdlintrc` are read when there is no `gdck.toml`, so a
    project already set up for `gdtoolkit` keeps its line length, its
    thresholds and its disabled rules without writing anything new. What
    `gdck` cannot honour — a customised naming pattern, a reordered
    `class-definitions-order` — is reported rather than silently dropped.
  - `--config` to name a file and `--no-config` to ignore all of them.
  - `gdck.toml` is deserialised by `toml` and `serde`, with validation above
    it for what a type cannot say: ranges, and settings that only mean
    something alongside another. An unknown key is an error naming the nearest
    key that exists, including when the mistake was the right key under the
    wrong table. `gdlintrc` and `gdformatrc` are read by `yaml_serde` into an
    untyped mapping, so a setting `gdck` has no equivalent for is skipped with
    a note rather than failing the file — but a file that cannot be parsed at
    all is an error, since none of its settings would apply.
  - Setting `format.line-length` moves `lint.max-line-length` with it unless
    that is set too, so the linter cannot report the lines the formatter just
    produced.
- `gdck-lint`: a working linter, and `gdck lint`, `gdck check` and `gdck fix`.
  - 33 rules covering the style guide's naming conventions, its formatting
    rules, and the declaration order it prescribes; the design thresholds
    inherited from `gdtoolkit`; and five correctness smells. The catalogue is
    documented in [docs/RULES.md](docs/RULES.md), and a test fails if a rule
    ships undocumented.
  - Fixes for 10 of them, applied back to front so earlier offsets stay valid.
    `quote-style` and `number-format` call into the formatter for their
    rewrite, so the two cannot disagree about what the right spelling is.
  - `code-order`, with `gdck fix --fix-order` to apply it. Reordering is
    all-or-nothing per file, and is refused when an initialiser might read a
    member the move would put below it. What it does produce is a permutation
    of the source, so a declaration takes its comments and annotations with it
    byte for byte.
  - Suppression with `# gdlint: ignore=`, `disable=` and `enable=`, matching
    `gdtoolkit`'s syntax and semantics. `gdtoolkit`'s rule names are accepted
    as aliases wherever a rule is named, so an existing `gdlintrc` and existing
    suppression comments keep working.
  - Agreement with `gdlint` checked rule by rule over its own test corpus:
    identical findings for `unused-argument`, `constant-name`,
    `trailing-whitespace`, `function-name`, `enum-member-name`,
    `argument-name`, `enum-name` and `max-arguments`, and every difference
    elsewhere accounted for.
- `gdck-format`: a working formatter, and `gdck format`.
  - A Wadler-style pretty printer over the syntax tree: two indent levels on
    continuation lines and one inside arrays, dictionaries and enums; trailing
    commas on collections that break; one statement per line; two blank lines
    around top-level definitions and one inside a class.
  - Literal normalisation: quote style chosen to minimise escapes, lowercase
    hexadecimal, a digit either side of a float's point.
  - Single-line inner class declarations (`class Child extends Parent:`), with
    a body-level `extends` hoisted onto the declaration line, and the file-level
    `class_name` / `extends` pair split across two lines. Both are what the
    style guide's "Class declaration" section shows.
  - Redundant parentheses dropped; parentheses added when an expression has to
    wrap and has no brackets of its own.
  - Line breaks the author wrote in a bracketed construct are preserved, since
    the guide presents both the wrapped and unwrapped forms as good.
  - Safety checks, on by default: the output is re-parsed and checked to still
    parse, to mean the same thing, to have kept every comment, and to be stable
    under a second pass. `--fast` turns them off.
  - Conformance tests against all 56 usable code samples in the GDScript style
    guide, extracted from the documentation by
    `tools/extract-style-guide-samples.py`.

### Fixed

- `gdck-cli`: `gdck format -` wrote the literal string `-` to standard output
  instead of the formatted file, so `gdck format - < in.gd > out.gd` destroyed
  the file. Reading from `-` is now a filter with or without `--fix`, which is
  what the README always said it was: there is no file to write back to, so
  the formatted text is the output.
- `gdck-cli`: `gdck parse` wrote its summary to standard output while every
  other subcommand wrote it to standard error, so `gdck parse --tree` mixed a
  sentence of prose into the tree.
- `gdck-cli`: `--diff` printed the whole span between the first and last change
  as removed and re-added, so two one-line changes at opposite ends of a file
  printed the file twice. Diffs now come from `similar` and have hunks.
- `gdck-syntax`: a single-line lambda inside a bracketed construct written
  across several lines (`connect(\n\tfunc(): return 1\n)`) took the closing
  bracket's line as the start of its body, swallowing the bracket.
- `gdck-format`: a function's annotations were pulled up onto its declaration
  line. The Godot documentation writes `@rpc("any_peer")` above the `func` and
  `@export_range(0, 10) var lives` beside the `var`, so the two are now
  distinguished. `@abstract` stays inline either way, as a modifier.

### Changed

- `gdck format` writes summaries to standard error, so standard output carries
  only content: the formatted file when reading from `-`, the diff under
  `--diff`, and otherwise the list of paths that would change.

### Internal

- End-to-end tests that run the binary, covering what only a real process can
  answer: that nothing reaches the disk without `--fix`, which stream each kind
  of output goes to, and which exit code comes back. Both of the `gdck-cli`
  fixes above were found by writing them.
- `prek` hooks in `prek.toml`, running the file-hygiene checks plus `cargo fmt`
  and `cargo clippy` on commit, and the tests, doctests and MSRV check on push.
  The hygiene hooks are prek's builtins, so there is no repository to clone and
  no Python to install. See [CONTRIBUTING.md](CONTRIBUTING.md).
- `cargo deny` in `deny.toml`, checked in CI: security advisories, licences,
  duplicate versions and crate provenance. The licence allow list is exactly
  what the tree needs — `MIT`, `Apache-2.0` and `Unicode-3.0` — so anything
  arriving with a fourth is a decision to be made rather than a default to be
  inherited.

## [0.1.0] - 2026-08-11

Initial commit. The parser works; the formatter and linter do not exist yet.

### Added

- `gdck-syntax`: a lossless lexer and parser for GDScript 4.
  - Zero-width `Indent` / `Dedent` tokens for indentation-delimited blocks.
  - Multi-line lambda bodies inside brackets, where indentation becomes
    significant again.
  - All literal forms: `$Node/Path`, `%Unique`, `&"StringName"`, `^"NodePath"`,
    raw and triple-quoted strings, every number base.
  - Full declaration and expression grammar, including annotations, property
    accessors, `match` patterns with bindings and rest markers, variadic
    parameters and `@abstract` members.
  - Error recovery: parsing never fails, and the tree always reproduces its
    input byte for byte.
  - Round-trips all 353 `.gd` files in `godot-gdscript-toolkit`'s test suites,
    and parses all 324 valid ones without error.
- `gdck-config`: configuration types and defaults taken from the GDScript style
  guide, plus config-file discovery.
- `gdck-cli`: the `gdck` binary, with a working `parse` subcommand and the full
  `check` / `fix` / `format` / `lint` interface defined.
- `gdck-format` and `gdck-lint`: crate skeletons carrying the design notes and
  the planned rule catalogue.

[Unreleased]: https://github.com/eth0net/gdck/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/eth0net/gdck/releases/tag/v0.1.0
