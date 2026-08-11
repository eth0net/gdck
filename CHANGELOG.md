# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing yet.

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
