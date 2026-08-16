# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `gdck init` writes a `gdck.toml`. In a project with a `gdformatrc` or
  `gdlintrc` it is also the migration path: the settings are read by the same
  code that reads them for a real run, so what lands in the file is exactly what
  `gdck` was already doing — `gdck config` prints the same thing before and
  after — and anything `gdtoolkit` had that has no equivalent here is named
  before the file is written rather than dropped in silence.
  Settings still at the style guide's default are written commented out. The
  live lines are then the project's own decisions, and a later change to a
  default still reaches it rather than being pinned by accident.
  It refuses to overwrite an existing `gdck.toml` without `--force`.
  `--no-config` starts from the defaults instead of what is there, and
  `--config <path>` takes a named file as the source.

- `format.class-declaration` chooses whether a file-level `class_name` keeps
  its `extends` on the same line. `"multi-line"` is the default and the style
  guide's answer — its prose introduces the two in sequence, its examples write
  them apart, and it singles inner classes out for the opposite treatment ("For
  inner classes, use single-line declarations"), which only means something if
  file-level declarations are not single-line.
  `"single-line"` exists because `gdformat` enforces neither shape: its grammar
  has a separate rule for the joined form and it preserves whichever it was
  given. A project migrating can therefore be uniformly on
  `class_name Player extends Node` without ever having chosen it, and would
  meet that as a diff across most of its files. Either setting is canonical —
  both shapes converge on the one configured, so this decides a layout rather
  than preserving what was written.

### Fixed

- A comment written between `class_name` and `extends` was dropped by the
  formatter. The safety checks caught it, so nothing was ever written to disk
  and the file was reported as rejected rather than silently mangled — but the
  file could not be formatted at all until the comment was moved. It is now
  kept, on its own line, and its presence holds the two lines apart even under
  `class-declaration = "single-line"`, where a joined line would leave it
  nowhere to go.

### Documentation

- `docs/CONFIG.md` recommended `gdck config > gdck.toml` as the way to start a
  configuration file. It does the opposite of what it looks like: the shell
  creates the file before `gdck` starts, `gdck` finds an empty `gdck.toml`,
  which beats a `gdlintrc` outright, and writes the defaults into it. A project
  following that advice to migrate lost every setting it had and was left with
  a file that looked deliberate. The advice is retracted in place, and
  `gdck init` does the job properly.
- `docs/CONFIG.md` listed `class-definitions-order` among the `gdlintrc`
  settings that are reported and not applied. It has been read into
  `lint.declaration-order` since 0.3.0; the page now says so.

## [0.3.0] - 2026-08-16

Two trials against real projects drove most of this: a parser bug that
rejected code Godot compiles, and two rules that told a project its own
settled conventions were mistakes.

### Fixed

- `gdck-syntax`: `when` and `match` were refused as names, so real code Godot
  compiles was reported as a syntax error — `var when: int`, a parameter called
  `when`, and `text.match("*.gd")` among them. Both are keywords Godot still
  accepts as identifiers: `match` because `String.match()` was on the engine's
  API first, and `when` because it arrived as a `match` guard long after code
  was using it as a name. The list is `Token::is_identifier` in
  `gdscript_tokenizer.cpp`, whose comment on `when` reads "New keyword, avoid
  breaking existing code".
  A file this hit could not be formatted at all, and the mis-parse produced
  phantom lint findings around it. Both are gone: on the project that reported
  it, all 243 files now parse and the 7 spurious `expression-not-assigned`
  findings disappeared. A `match` guard still reads `when` as the keyword, so
  `1 when when > 0` parses as a guard testing a variable of that name.

### Added

- `lint.declaration-order` sets the order `code-order` checks, and a
  `gdlintrc`'s `class-definitions-order` is read into it rather than reported
  as something `gdck` will not apply. A project that pinned an order keeps it.
  What no such setting can change is the order *within* a group: `gdtoolkit`
  has one `others` bucket for every method and inner class, where `gdck`
  separates `_init()`, `_ready()`, `_process()`, static functions and the rest.
  Giving `others` a position says where that run goes; inside it the guide's
  order stands.
- `lint.file-name` chooses which convention the `file-name` rule holds a file
  to: `"snake-case"`, as the style guide says and as before, or
  `"pascal-case"` for a project that names files after the classes in them.
  It is the only naming rule that takes a setting, because it is the only one
  whose subject is not an identifier — Godot has no opinion about what a
  script is called. Naming the convention keeps the rule working, where
  `disable = ["file-name"]` stops it noticing anything: a file following
  neither convention is still reported.

## [0.2.0] - 2026-08-15

The first release with a working formatter and linter. `0.1.0` was the parser
and a command-line skeleton, and was never published anywhere.

### Added

- Prebuilt binaries for macOS, Linux and Windows on every release, with a
  one-line installer for each platform and a Homebrew formula in
  [eth0net/homebrew-tap](https://github.com/eth0net/homebrew-tap). `gdck` is
  for people writing Godot games, most of whom have no Rust toolchain, so
  `cargo install` was a filter rather than an invitation. Linux is statically
  linked against musl and runs on any distribution. Every archive carries a
  SHA-256 beside it.
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

- `gdck`: `gdck format -` wrote the literal string `-` to standard output
  instead of the formatted file, so `gdck format - < in.gd > out.gd` destroyed
  the file. Reading from `-` is now a filter with or without `--fix`, which is
  what the README always said it was: there is no file to write back to, so
  the formatted text is the output.
- `gdck`: `gdck parse` wrote its summary to standard output while every
  other subcommand wrote it to standard error, so `gdck parse --tree` mixed a
  sentence of prose into the tree.
- `gdck`: `--diff` printed the whole span between the first and last change
  as removed and re-added, so two one-line changes at opposite ends of a file
  printed the file twice. Diffs now come from `similar` and have hunks.
- `gdck-syntax`: a single-line lambda inside a bracketed construct written
  across several lines (`connect(\n\tfunc(): return 1\n)`) took the closing
  bracket's line as the start of its body, swallowing the bracket.
- `gdck-format`: a function's annotations were pulled up onto its declaration
  line. The Godot documentation writes `@rpc("any_peer")` above the `func` and
  `@export_range(0, 10) var lives` beside the `var`, so the two are now
  distinguished. `@abstract` stays inline either way, as a modifier.
- `gdck-format`: an argument list ending in a multi-line lambda produced a file
  Godot rejects with "Unindent doesn't match the previous indentation level".
  A lambda body is the one place inside brackets where Godot still tracks
  indentation, and the first line after it has to sit at the enclosing
  statement's indent — which the closing brackets of a wrapped call do not. A
  trailing comma ends the lambda on the body's own last line, so no line is
  left to check; collections already emitted one and argument lists now do too.
- `gdck-format`: a standalone annotation was moved up beside the declaration
  under it, producing a line Godot rejects with "Expected newline after a
  standalone annotation". `@warning_ignore_start`, `@warning_ignore_restore`,
  `@export_category`, `@export_group` and `@export_subgroup` open or close a
  region rather than describing what follows them, so there is nothing for them
  to sit beside and they now keep their own line. The annotations that do
  describe the declaration below, like `@export_range`, still move up onto it.
- `gdck-format`: parentheses around a multi-line lambda were closed on a line
  of their own, so `assert((func() -> bool: ...).call())` came back as
  something Godot rejects with "Unindent doesn't match the previous
  indentation level". Until a lambda ends, a dedent has to land on one of the
  enclosing statement's own indentation levels, and a continuation's is not one
  of them. The closing parenthesis is what ends this lambda, so it now stays on
  the body's last line — the same shape the comma gives an argument list.
- `gdck-format`: a property whose accessors are both set to methods lost the
  comma between them, so `var p:\n\tset = __set,\n\tget = __get` came back
  without one and Godot rejected it with "Expected end of indented block for
  property". Godot has two property forms and decides which it is reading from
  the first accessor: the comma is what carries it on to the second, and only
  that form has one. Accessors written as blocks are still separated by nothing
  but a newline, since a comma there is a syntax error rather than a redundancy.

### Changed

- `gdck format` writes summaries to standard error, so standard output carries
  only content: the formatted file when reading from `-`, the diff under
  `--diff`, and otherwise the list of paths that would change.
- The crate holding the binary is now `gdck` rather than `gdck-cli`, so
  installing it is `cargo install gdck`. The binary was always called `gdck`
  and is unchanged. The name had to be settled before the first publish, since
  a crate name cannot be moved afterwards.
- Publishing is opt-in per crate, and every crate here opts in. `gdck-syntax`
  is a library in its own right — no dependencies, and a tree that reproduces
  its input byte for byte. `gdck-config`, `gdck-format` and `gdck-lint` are
  published only because the binary depends on them; their APIs move with
  `gdck` and carry no stability promise.

### Internal

- End-to-end tests that run the binary, covering what only a real process can
  answer: that nothing reaches the disk without `--fix`, which stream each kind
  of output goes to, and which exit code comes back. Both of the `gdck`
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
- Conformance against the real Godot parser, in `crates/gdck-format/tests/`.
  The existing safety checks re-parse formatted output with `gdck-syntax`,
  which is lossless and lenient by design and so accepts things Godot does
  not; this asks Godot itself. The comparison is differential — Godot reports
  undefined identifiers as parse errors too, so only files it accepted before
  formatting are examined, and only failures formatting introduced are
  reported. Set `GDCK_GODOT` to a Godot binary to run it.

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

[Unreleased]: https://github.com/eth0net/gdck/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/eth0net/gdck/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/eth0net/gdck/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/eth0net/gdck/releases/tag/v0.1.0
