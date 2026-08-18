# Configuration

`gdck` needs no configuration. Every default is the [style guide's][styleguide],
so a project with no configuration file gets style-guide behaviour, and this
page is only about departing from it.

[styleguide]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html

To see what a run is actually using, and to write it down:

```sh
gdck config          # the effective settings, as a gdck.toml
gdck init            # write a gdck.toml holding them
```

Do not use `gdck config > gdck.toml`. Earlier versions of this page suggested
it, and it does the opposite of what it looks like: the shell creates the file
before `gdck` starts, `gdck` then finds an empty `gdck.toml` — which
[wins outright](#where-settings-come-from) over a `gdlintrc` — and writes the
defaults into it. A project doing that to migrate loses every setting it had,
and is left with a file that looks deliberate. `gdck init` writes the file
itself, so there is nothing to race.

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

Winning outright is not the same as winning silently. If a `gdformatrc` or
`gdlintrc` is there and *disagrees* with the `gdck.toml`, the run says so once
and names the settings:

```
gdck: .gdlintrc:1: not applied, because the gdck.toml takes precedence.
      It disagrees about lint.max-line-length. Copy those into the gdck.toml,
      or delete this file
```

Only a real disagreement is reported, so once the two agree — which is what
[`gdck init`](#migrating) leaves behind — nothing is said and the warning does
not become permanent furniture. A shadowed file that cannot be parsed is passed
over without comment: it is not governing the run, so it cannot mislead you
about it.

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
indent = "tabs"              # or "spaces"
class-declaration = "multi-line"  # or "single-line"
safety-checks = true

[lint]
max-line-length = 100          # not moved by a gdlintrc; see below
max-file-lines = 1000
max-public-methods = 20
max-returns = 6
max-arguments = 10
code-order = "report"    # "report" | "fix-when-safe" | "off"
file-name = "snake-case" # "snake-case" | "pascal-case"
disable = []
# declaration-order is omitted unless a project sets one; see below.

[files]
exclude = [".git", ".godot", ".import", "addons"]
respect-gitignore = true
```

### `[format]`

| Key | Meaning |
|---|---|
| `line-length` | Where the formatter wraps. The guide says to keep lines under 100 characters. |
| `indent` | `"tabs"`, which the guide mandates, or `"spaces"` for a project already committed to them. |
| `indent-width` | How many spaces one level is, 1 to 16. Only with `indent = "spaces"`; a tab is four columns and is not adjustable. |
| `class-declaration` | Whether a file-level `class_name` keeps its `extends` on the same line. `"multi-line"` is the guide's answer; `"single-line"` suits a project already written that way. |
| `safety-checks` | Re-parse the output and check it still parses, means the same thing, kept every comment, and is stable. `--fast` turns them off for one run. |

Setting `line-length` moves `lint.max-line-length` with it unless that is set
too. Otherwise the linter would report exactly the lines the formatter just
produced.

#### Choosing the class-declaration shape

The guide asks for two lines and says so three ways: the prose introduces them
in sequence, every example writes them apart, and inner classes are singled out
for the opposite treatment — "For inner classes, use single-line declarations".
That contrast only means something if file-level declarations are not
single-line, so `"multi-line"` is the default:

```gdscript
class_name Player
extends Node
```

`gdformat` enforces neither shape. Its grammar has a separate rule for the
joined form and it preserves whichever one it is given, so a project can be
uniformly on `class_name Player extends Node` without ever having chosen it —
and the first `gdck format` would rewrite most of its files. `"single-line"` is
for a project that has looked at that diff and prefers what it had.

Either way the result is canonical: both shapes converge on the one configured,
so this decides a layout rather than preserving whatever was written. The one
exception is a comment between the two lines, which has nowhere to go on a
joined line and so keeps them apart whatever the setting says.

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
| `file-name` | Which convention `file-name` holds a file to. `"snake-case"` is the style guide's answer; `"pascal-case"` suits a project that names files after the classes in them. |
| `declaration-order` | The order `code-order` checks, as a list of group names. Omitted means the style guide's. |
| `disable` | Rule names switched off for the whole project. `gdtoolkit`'s names work here too. |

`declaration-order` takes `gdtoolkit`'s fourteen group names, in the order the
project wants them. Left alone, that order is the style guide's:

```toml
[lint]
declaration-order = [
  "tools", "classnames", "extends", "docstrings", "signals", "enums", "consts", "staticvars",
  "exports", "pubvars", "prvvars", "onreadypubvars", "onreadyprvvars", "others",
]
```

Writing that out changes nothing — it is what `gdck` already checks. `gdck
config` prints it, so the order a run is holding your code to is always
something you can read rather than infer, and `gdck init` writes it commented
out like every other untouched setting.

Note where `staticvars` falls: eighth, directly after `consts` and before
`exports`, which is [where the guide puts static variables][order]. An earlier
version of this page showed it last instead. Nothing in `gdck` behaved that
way, but a project that set out to correct the difference would have written a
fourteen-line block that did nothing.

[order]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html#code-order

A `gdlintrc` with `class-definitions-order` is read into this, so a project
that already pinned an order keeps it without writing anything new.

One thing to know before expecting it to change much. `gdck` sorts methods
more finely than these names can express: `gdtoolkit` has one `others` bucket
for every function and inner class, where `gdck` separates `_init()`,
`_enter_tree()`, `_ready()`, `_process()`, `_physics_process()`, static
functions and inner classes. Giving `others` a position says where that whole
run goes; the order *within* it stays the guide's, and nothing in
`class-definitions-order` can say otherwise. A `code-order` report naming two
methods is therefore about an order `gdlint` never checked, not one your
configuration disagrees with.

`file-name` is the only naming rule that takes a setting, and it is worth
saying why the others do not. Every other name the rules check is an
identifier, which the language and the style guide both have a settled view
about. A file name is neither — Godot does not care, and a project that names
`PlayerController.gd` after the class inside it is keeping a convention rather
than failing to keep one.

Naming the convention is better than switching the rule off with `disable`,
which is the other way to stop 55 reports: the rule carries on working, so a
file that follows neither convention is still caught.

`disable` naming a rule that does not exist is a warning rather than an error,
so one file can be shared between `gdck` versions — but it does say so, because
a typo silently leaving a rule on is worth a word.

To switch a rule off for one line or one region instead, see
[RULES.md](RULES.md#turning-a-rule-off).

### `[files]`

| Key | Meaning |
|---|---|
| `exclude` | Directory *names* skipped when walking, replacing the defaults. |
| `respect-gitignore` | Skip what a `.gitignore` covers. On by default. |

`exclude` **replaces** the defaults rather than adding to them, so name the ones
you still want:

```toml
[files]
exclude = [".git", ".godot", ".import", "addons", "vendor"]
```

Replacing is what makes it possible to *narrow* the list, which some projects
need: an addon's own source lives in `addons`, which the defaults skip, and
there has to be a way to say so.

```toml
[files]
exclude = [".git"]   # an addon linting its own source
```

Easiest is not to write it by hand. `gdck init` puts the effective list in the
file, so you edit one array that already names everything:

```toml
[files]
# exclude = [".git", ".godot", ".import", "addons"]
```

Most of what a project would otherwise add here — `build`, `export`, `.godot`
itself — is already covered by `respect-gitignore` below.

#### `respect-gitignore`

On by default. A `.gd` file git has been told to ignore is almost always
generated or vendored, so reporting on it is noise about code nobody is going
to edit.

```toml
[files]
respect-gitignore = false
```

`--no-gitignore` turns it off for one run.

Every mechanism git itself has is honoured, so what `gdck` skips is what
`git status` leaves out: the nearest `.gitignore` and every one above it,
nested files deeper in the tree, `!` negations, `.git/info/exclude`, and your
global ignore file. A directory with a `.gitignore` but no `.git` is still
read, since an export or a vendored copy meant what its file says.

This is a second filter rather than a replacement for `exclude`, and a file has
to pass both. Neither covers the other: `addons` is committed, so no
`.gitignore` mentions it, and a `build/` directory is ignored without being a
name anybody listed.

A path named directly on the command line is processed whatever the ignore
files say — `gdck format build/generated.gd` does what you asked — on the same
principle as the exclusions.

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

### Migrating

`gdck` keeps reading those files indefinitely, so there is no deadline. When you
do want a `gdck.toml` — to set something `gdtoolkit` has no equivalent for, such
as [`format.class-declaration`](#choosing-the-class-declaration-shape) or
[`lint.file-name`](#lint) — write it with `gdck init` rather than by hand:

```sh
gdck init
```

It reads whatever is in force, which in a `gdtoolkit` project is your
`gdformatrc` and `gdlintrc`, and writes the equivalent `gdck.toml`. Settings you
chose come out live; settings still at the style guide's default come out
commented, so the file reads as a list of your decisions and you keep inheriting
later changes to the rest. Anything `gdtoolkit` had that `gdck` has no setting
for is named on the way past, before the file is written.

The result is lossless: `gdck config` prints the same settings before and after.
`gdck init` refuses to overwrite an existing `gdck.toml` without `--force`, and
`gdck init --no-config` starts from the style guide's defaults instead, ignoring
whatever files are there.

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

The two line lengths are deliberately not the same setting, which surprises
people often enough to be worth spelling out. `gdformatrc`'s `line_length` moves
both, because a formatter wrapping wider than the linter allows would report its
own output. A `gdlintrc`'s `max-line-length` moves **only the linter**, and the
formatter keeps wrapping at 100.

That is what `gdtoolkit` does — `gdformat` reads `line_length` from a
`gdformatrc` and never looks at a `gdlintrc` — so a project with only a
`gdlintrc` has been formatting at 100 and linting at its own width all along,
whether or not it meant to. Making `max-line-length` widen the formatter here
would quietly reflow files `gdformat` had left alone, which is the one thing
compatibility is for. Say `format.line-length` in a `gdck.toml` to widen the
formatter; `gdck init` writes both, so the difference is visible rather than
inferred.

### What is not applied, and why you hear about it

One setting has no equivalent, because it configures machinery `gdck` does not
have:

- **Naming patterns** — `class-name`, `function-name` and the eleven others.
  `gdck` checks the guide's conventions directly rather than with a regular
  expression, so it has nowhere to put a pattern. A pattern left at
  `gdtoolkit`'s default changes nothing and passes without comment; a
  customised one is reported.
That is the only one. `class-definitions-order` used to be listed here too, and
is now [read into `lint.declaration-order`](#lint).

Anything else in the file that `gdck` has no setting for is reported too, once,
before the run it would otherwise silently affect:

```
gdck: gdlintrc:2: gdck has no setting matching `max-locals`; it is ignored
```

A key written as a bare `name:` with nothing after it is how `gdtoolkit` says a
rule is on, which is the default, so those pass without comment. `gdlint
--dump-default-config` and `gdformat --dump-default-config` output are both read
in full without a word.

A *setting* in a `gdtoolkit` file is never fatal — the file is allowed to hold
ones that mean nothing here, and what matters is that none of them silently
fails to apply. A file that is not valid YAML **is** fatal, on the same footing
as a broken `gdck.toml`: none of its settings would apply, and being formatted
by rules you had written down and rejected is worse than not running.

## `--config`

```sh
gdck check --config ci/strict.toml .
```

The kind of file is decided by its name, so `--config path/to/gdlintrc` is read
as a `gdlintrc`. Any other name is read as a `gdck.toml`.
