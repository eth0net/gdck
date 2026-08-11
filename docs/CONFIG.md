# Configuration

`gdck` needs no configuration. Every default is the [style guide's][styleguide],
so a project with no configuration file gets style-guide behaviour, and this
page is only about departing from it.

[styleguide]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html

To see what a run is actually using:

```sh
gdck config          # the effective settings, as a gdck.toml
gdck config > gdck.toml   # and that is a valid starting point
```

## Where settings come from

`gdck` searches upwards from the directory the paths on the command line have in
common — so `gdck check ../game` reads `../game/gdck.toml`, not whatever sits
above your shell's working directory — and takes the first of these it finds:

1. `--config <path>`, which skips the search entirely.
2. The nearest `gdck.toml` or `.gdck.toml`.
3. `gdformatrc` and `gdlintrc`, [`gdtoolkit`'s own files](#gdtoolkit-compatibility).
4. Nothing, which is the style guide's defaults.

A `gdck.toml` wins **outright**, not key by key: a project that has written one
has said what it wants, and quietly mixing in a `gdlintrc` from three
directories further up would make the result impossible to predict. `--no-config`
ignores all of them.

One configuration governs a whole run. A repository holding several Godot
projects with different settings needs one invocation per project.

A configuration file that cannot be read **stops the run** with exit code 2,
rather than falling back to the defaults. Formatting a project by rules it
explicitly rejected, and not saying so, is worse than not running.

## `gdck.toml`

Every setting, at its default:

```toml
[format]
line-length = 100
indent = "tabs"          # or "spaces"
safety-checks = true

[lint]
max-line-length = 100
max-file-lines = 1000
max-public-methods = 20
max-returns = 6
max-arguments = 10
code-order = "report"    # "report" | "fix-when-safe" | "off"
disable = []

[files]
exclude = [".git", ".godot", ".import", "addons"]
```

### `[format]`

| Key | Meaning |
|---|---|
| `line-length` | Where the formatter wraps. The guide says to keep lines under 100 characters. |
| `indent` | `"tabs"`, which the guide mandates, or `"spaces"` for a project already committed to them. |
| `indent-width` | How many spaces one level is, 1 to 16. Only with `indent = "spaces"`; a tab is four columns and is not adjustable. |
| `safety-checks` | Re-parse the output and check it still parses, means the same thing, kept every comment, and is stable. `--fast` turns them off for one run. |

Setting `line-length` moves `lint.max-line-length` with it unless that is set
too. Otherwise the linter would report exactly the lines the formatter just
produced.

### `[lint]`

Each threshold is named after the rule it configures, so `max-returns` is the
limit the `max-returns` rule reports on. The rules themselves are in
[RULES.md](RULES.md).

| Key | Meaning |
|---|---|
| `max-line-length` | The width `line-too-long` reports past, measured in display columns with a tab counting as four. |
| `max-file-lines` | The limit `max-file-lines` reports past. |
| `max-public-methods` | The limit `max-public-methods` reports past, counting methods that do not begin with `_`. |
| `max-returns` | How many `return` statements one function may hold. |
| `max-arguments` | How many parameters one function may take. |
| `code-order` | `"report"` names ordering problems and leaves `--fix-order` to apply them; `"fix-when-safe"` reorders whenever `gdck fix` runs and the moves are provably safe; `"off"` says nothing about order at all. |
| `disable` | Rule names switched off for the whole project. `gdtoolkit`'s names work here too. |

`disable` naming a rule that does not exist is a warning rather than an error,
so one file can be shared between `gdck` versions — but it does say so, because
a typo silently leaving a rule on is worth a word.

To switch a rule off for one line or one region instead, see
[RULES.md](RULES.md#turning-a-rule-off).

### `[files]`

`exclude` is a list of directory names skipped when walking a directory. It
**replaces** the defaults rather than adding to them, so include the ones you
still want:

```toml
[files]
exclude = [".git", ".godot", ".import", "addons", "vendor"]
```

A path named directly on the command line is always processed, whatever this
says — `gdck format addons/thing.gd` does what you asked.

### Spelling

It is ordinary TOML, read by the `toml` crate — anything that is valid there is
valid here. Keys are kebab-case, and `_` is accepted in place of `-`, so
`line_length` and `line-length` are the same setting. `[table]` headers and
dotted keys are interchangeable:

```toml
format.line-length = 100    # the same as [format] / line-length = 100
```

An unknown key is an error, not something ignored, and the message names the
nearest key that exists:

```
gdck: gdck.toml:3: unknown setting `line-lenght`; did you mean `format.line-length`?
gdck: gdck.toml:7: unknown setting `max-returns`; did you mean `lint.max-returns`?
```

The second is the more common mistake — the right key under the wrong table —
so it is checked before spelling.

## `gdtoolkit` compatibility

A project already using `gdformat` and `gdlint` has decided its line length and
which rules it does not want. Making it restate all of that before `gdck` will
behave is a reason not to try `gdck` at all, so with no `gdck.toml` present the
existing files are read.

Read from `gdformatrc`:

| `gdformat` | Becomes |
|---|---|
| `line_length` | `format.line-length`, and `lint.max-line-length` with it |
| `use_spaces` | `format.indent` and `format.indent-width` |
| `safety_checks` | `format.safety-checks` |
| `excluded_directories` | `files.exclude` |

Read from `gdlintrc`:

| `gdlint` | Becomes |
|---|---|
| `disable` | `lint.disable` |
| `max-line-length` | `lint.max-line-length` |
| `max-file-lines` | `lint.max-file-lines` |
| `max-public-methods` | `lint.max-public-methods` |
| `max-returns` | `lint.max-returns` |
| `function-arguments-number` | `lint.max-arguments` |
| `excluded_directories` | `files.exclude` |

Both are read when both exist, formatting first, so a `gdlintrc` naming its own
line length has the last word on what the linter reports.

### What is not applied, and why you hear about it

Two settings have no equivalent, because they configure machinery `gdck` does
not have:

- **Naming patterns** — `class-name`, `function-name` and the eleven others.
  `gdck` checks the guide's conventions directly rather than with a regular
  expression, so it has nowhere to put a pattern. A pattern left at
  `gdtoolkit`'s default changes nothing and passes without comment; a
  customised one is reported.
- **`class-definitions-order`** — `gdck` orders declarations the way the style
  guide does. The default order is the one it implements, so only a reordered
  list is reported.

Anything else in the file that `gdck` has no setting for is reported too, once,
before the run it would otherwise silently affect:

```
gdck: gdlintrc:2: gdck has no setting matching `max-locals`; it is ignored
```

A key written as a bare `name:` with nothing after it is how `gdtoolkit` says a
rule is on, which is the default, so those pass without comment. `gdlint
--dump-default-config` and `gdformat --dump-default-config` output are both read
in full without a word.

Nothing in a `gdtoolkit` file is ever fatal. It is allowed to hold settings that
mean nothing here; what it must not do is have one silently not apply.

## `--config`

```sh
gdck check --config ci/strict.toml .
```

The kind of file is decided by its name, so `--config path/to/gdlintrc` is read
as a `gdlintrc`. Any other name is read as a `gdck.toml`.
