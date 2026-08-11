# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
