# gdck documentation

[`gdck`](https://github.com/eth0net/gdck) is a fast GDScript formatter, linter
and language server, faithful to the [official GDScript style guide][guide].
The [main README](../README.md) covers installing it and the shape of the
command line; everything longer than a paragraph lives here.

[guide]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html

## For using it

- **[RULES.md](RULES.md)** — every rule, what it wants, which are fixable, and
  how to switch one off for a line, a region or the project. Also where `gdck`
  reports more than `gdlint` did, and why.
- **[CONFIG.md](CONFIG.md)** — every setting in a `gdck.toml`, what `gdck init`
  does with an existing `gdformatrc` or `gdlintrc`, and what happens when a
  `gdtoolkit` setting has no equivalent here.
- **[HOOKS.md](HOOKS.md)** — the four git hooks for `prek` and `pre-commit`,
  migrating a `gdtoolkit` configuration, and why `exclude` behaves differently
  under a hook than on the command line.
- **[EDITORS.md](EDITORS.md)** — `gdck lsp`, with configurations for Neovim,
  Helix, Zed and VS Code, and why it is meant to run alongside Godot's own
  language server rather than instead of it.

## For working on it

- **[DESIGN.md](DESIGN.md)** — how the crates fit together, why the formatter
  is a Wadler pretty printer, what the safety checks promise, and the places
  `gdck` knowingly departs from the guide.
- **[CONTRIBUTING.md](../CONTRIBUTING.md)** — getting set up, what CI runs, and
  how to add a rule.
- **[CHANGELOG.md](../CHANGELOG.md)** — what changed, and in most cases why.
