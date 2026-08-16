# Lint rules

Every rule `gdck lint` can report. A rule is named the same way everywhere: in
the report, in a `# gdlint:` comment, and in a `disable` list.

Rules marked **fixable** are applied by `gdck lint --fix` and `gdck fix`.
Nothing else is written to disk without one of those.

`gdtoolkit`'s names for the same rules are accepted as aliases, so an existing
`gdlintrc` and existing suppression comments keep working.

## Turning a rule off

```gdscript
var BadName = 1  # gdlint: ignore=variable-name

# gdlint: disable=variable-name
var AlsoBad = 2
var StillBad = 3
# gdlint: enable=variable-name
```

`ignore=` covers its own line and the one below, so it works both as a trailing
comment and as a comment written above the line it excuses. `disable=` runs to
the matching `enable=` or to the end of the file. Both take a comma-separated
list.

Project-wide, list them under `disable` in `gdck.toml`:

```toml
[lint]
disable = ["max-returns", "line-too-long"]
```

The thresholds the design rules report against are configurable there too. See
[CONFIG.md](CONFIG.md).

## Naming conventions

From the style guide's [naming conventions][naming] table. None of these are
fixable: a name is reached from places one file cannot see — other scripts,
scene files, `call()` with a string, signals connected in the editor — so a
rename is a decision for a person with a project-wide search, not a mechanical
edit.

Two things soften the table, both from the guide itself. A single leading
underscore marks a member private and is accepted wherever a name is checked.
And a name that holds a *class* may be `PascalCase` whatever else it is:
"Also use PascalCase when loading a class into a constant or a variable".

| Rule | Applies to | Wants | Aliases |
|---|---|---|---|
| `class-name` | `class_name X` | `PascalCase` | |
| `sub-class-name` | `class X:` | `PascalCase` | |
| `function-name` | `func f()` | `snake_case` | |
| `variable-name` | `var x`, `for x in` | `snake_case` | `class-variable-name`, `function-variable-name`, `loop-variable-name`, `class-load-variable-name`, `function-preload-variable-name` |
| `argument-name` | parameters, including a setter's | `snake_case` | `function-argument-name` |
| `constant-name` | `const X` | `CONSTANT_CASE` | `load-constant-name` |
| `signal-name` | `signal x` | `snake_case` | |
| `enum-name` | `enum X` | `PascalCase` | |
| `enum-member-name` | enum members | `CONSTANT_CASE` | `enum-element-name` |
| `file-name` | the file itself | `snake_case`, or `PascalCase` — see below | |

[naming]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html#naming-conventions

### Choosing the file naming convention

`file-name` is the one naming rule that takes a setting, because it is the one
whose subject is not an identifier. Godot has no opinion about what a script is
called, and a project that names `PlayerController.gd` after the class inside it
is keeping a convention rather than missing one. Say which with
`lint.file-name`, documented in [CONFIG.md](CONFIG.md#lint):

```toml
[lint]
file-name = "pascal-case"
```

This is better than `disable = ["file-name"]`, which is the other way to stop
the reports. The rule keeps working, so a file named like neither convention is
still caught. `gdtoolkit` has no equivalent rule at all, so a project arriving
from it sees these reports for the first time and is choosing between two
conventions rather than losing one.

### Differences from `gdtoolkit`

- `gdtoolkit` additionally accepts `_on_PascalCase_signal_name` as a function
  name, which was how Godot 3 generated signal callbacks. Godot 4 generates
  `_on_button_pressed`, and the guide asks for `snake_case`, so `gdck` does not.
- `gdtoolkit` forbids a leading underscore on a *local* variable. `gdck` allows
  it, because `var _unused = f()` is how you discard a return value.

## Formatting

The formatter produces all of this correctly. These rules exist so a project can
be told what is wrong without having its files rewritten.

| Rule | Fixable | Notes | Aliases |
|---|---|---|---|
| `line-too-long` | | Width in columns, with a tab worth 4. Configurable. | `max-line-length` |
| `trailing-whitespace` | ✓ | | |
| `mixed-indentation` | | Tabs and spaces in one line's indentation. | `mixed-tabs-and-spaces` |
| `tab-indentation` | | Indentation made of spaces. | `tab-characters` |
| `line-ending` | ✓ | CRLF or CR. Reported once for the file. | |
| `final-newline` | ✓ | Exactly one line feed at the end. | |

Every one of these skips lines inside a triple-quoted string, where the spaces
are part of a value rather than layout.

`line-too-long` counts columns rather than characters, so a tab-indented line
counts the same here as it does to the formatter. `gdtoolkit` counts characters,
which is why it reports fewer.

## Style-guide rules the formatter cannot infer

| Rule | Fixable | Wants |
|---|---|---|
| `boolean-operators` | ✓ | `and`, `or` and `not` rather than `&&`, `\|\|` and `!` |
| `unnecessary-parens` | ✓ | No parentheses around a bare condition |
| `comment-space` | ✓ | A comment starts with a space; commented-out code does not |
| `quote-style` | ✓ | Double quotes, unless single quotes need fewer escapes |
| `number-format` | ✓ | Lowercase hexadecimal, a digit either side of a float's point |
| `redundant-type-hint` | ✓ | `:=` when the type is already written on the line |
| `ambiguous-inferred-type` | | An explicit type where `:=` cannot supply one |
| `code-order` | | The guide's [declaration order][order], aliased `class-definitions-order` |

`quote-style` and `number-format` offer the formatter's own rewrite as the fix,
by calling into it. Two implementations that disagreed would show up as
`gdck lint --fix` producing something `gdck format` then changed again.

`comment-space` has to tell a comment from disabled code, which the guide draws
by intent rather than by syntax:

```gdscript
# This is a comment.
#print("This is disabled code")
```

The test is whether the text after the hash *is* GDScript, which `gdck` has a
parser to answer. Parsing alone is too generous — a single English word parses
as an expression — so the text must also look like code: hold a bracket or an
`=`, or open with a keyword. The bias is deliberate. Reporting a comment that
was disabled code would be telling the author to break the thing the guide asked
them to do; missing one that was prose costs a space. `#region` and `#endregion`
are exempt, as the guide requires.

`ambiguous-inferred-type` covers the two shapes the guide marks bad: an integer
literal, which "could be that float was intended", and a node lookup, which
"can't infer the exact type and will use Node". A cast answers the question, and
the guide endorses that spelling, so `:= get_node("UI/Bar") as ProgressBar` is
not reported.

[order]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html#code-order

### What `code-order` does and does not check

The guide separates "overridden built-in virtual methods" from "overridden
custom methods" from "remaining methods". Deciding which of those a function is
means knowing the whole inheritance chain, including the parent script and
Godot's own API.

So `gdck` orders the five virtual callbacks the guide names by name — `_init`,
`_enter_tree`, `_ready`, `_process`, `_physics_process` — along with
`_static_init` and static functions, and treats every other function as one
bucket. That reproduces the guide's own worked example exactly, and never claims
a function is misplaced on a guess about what it overrides.

A project that pinned `class-definitions-order` in a `gdlintrc`, or set
`lint.declaration-order` in a `gdck.toml`, gets the order it asked for rather
than the guide's. What it cannot change is the order *inside* a group: the
fourteen names `gdtoolkit` uses have one `others` bucket for every method and
inner class, so a report naming two methods is about an order `gdlint` never
checked rather than one the configuration disagrees with. See
[CONFIG.md](CONFIG.md#lint).

#### Why a project moving from `gdlint` sees new order reports

`gdlint` has a token for static *variables* and none for static *methods*. In
`gdtoolkit`'s `linter/class_checks.py`, `static_func_def` and `abstract_func_def`
both return `"others"` — the same bucket as an ordinary `func_def` — while
`static_class_var_stmt` returns `"staticvars"`. So `class-definitions-order` can
place static variables, which are position 08 in the guide, and can say nothing
at all about `_static_init()` and "remaining static methods", which are 12 and
13 and belong *before* the virtual callbacks.

Inner classes are the same story from the other end: the guide puts them last,
at 17, and `gdlint` has no token to say so.

`gdck` checks both, so adopting it on a `gdlint`-clean project surfaces
orderings nothing had been checking. On the project this was measured against,
that was 115 reports of a function after an inner class and 32 of a static
function after an ordinary one — none of them a regression, and none of them
something a `gdlintrc` could have caught.

Public-before-private is applied to variables, where it is unambiguous, and not
to methods, where it would contradict the rule above it: a virtual callback is
private by name and still comes first. The guide itself asked for public before
private until 4.4, which merged both into one group; `gdck` keeping the older
order for variables is one of the departures listed in
[DESIGN.md](DESIGN.md#known-differences-from-the-guide).

Reordering is opt-in and all-or-nothing per file. See
[Code order](../README.md#code-order-is-fixed-for-a-whole-file-or-not-at-all).

## Design thresholds

None of these come from the style guide. They are `gdtoolkit`'s defaults, kept
to the number so a project already configured against that linter gets the same
answers from this one. All four are configurable.

| Rule | Default | Aliases |
|---|---|---|
| `max-file-lines` | 1000 | |
| `max-public-methods` | 20, counted per class | |
| `max-returns` | 6, counted per function body | |
| `max-arguments` | 10 | `function-arguments-number` |

A lambda's `return`s belong to the lambda, not to the function holding it.

## Correctness

Not style. Each of these is a line that could be deleted, or one that was meant
to say something else.

| Rule | Fixable | Reports |
|---|---|---|
| `unused-argument` | | An argument the body never mentions |
| `duplicated-load` | | The same path given to `load` or `preload` twice |
| `expression-not-assigned` | | An expression statement with no effect |
| `comparison-with-itself` | | Both sides of a comparison being the same expression |
| `unnecessary-pass` | ✓ | `pass` in a block that has other statements |

`unused-argument` skips names with a leading underscore, which is the convention
for an argument that is deliberately unused — an overridden virtual method has
to take what it is given. That is also the fix, which is why none is offered:
renaming is a decision about the interface.

`comparison-with-itself` ignores comparisons involving a call, since
`randi() == randi()` is a question rather than a tautology.

`unnecessary-pass` offers its fix only when the `pass` is alone on its line.
Sharing one, removing it is an edit to a line rather than the deletion of one,
and one-statement-per-line is the formatter's business.
