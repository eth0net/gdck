# Status

What is done, what it has been run against, and what it cannot do yet.
Summarised in the [README](../README.md); this is the detail.

| Component | State |
|---|---|
| `gdck-syntax` — lexer | Done. Indentation, all literal forms, multi-line lambdas |
| `gdck-syntax` — parser | Done. Full declaration and expression grammar, error recovery |
| `gdck-config` | Done. `gdck.toml`, plus `gdformatrc` and `gdlintrc` for compatibility |
| `gdck-format` | Done. Wadler pretty printer with safety checks |
| `gdck-lint` | Done. 36 rules, 12 of them fixable. See [docs/RULES.md](RULES.md) |
| Every subcommand | Works |
| Git hooks | Done. `prek` and `pre-commit`. See [docs/HOOKS.md](HOOKS.md) |
| Language server | Diagnostics, formatting and quick fixes. See [docs/EDITORS.md](EDITORS.md) |

The parser handles every file in `gdtoolkit`'s corpus of valid GDScript — 324
files across its parser, formatter and `gd2py` test suites — and round-trips all
353 files there, including the deliberately invalid ones. The formatter formats
every one of those valid files with its safety checks passing.

Beyond that corpus it has been run against two real Godot projects, a 243-file
game and a 488-file addon. Every formatted file in the corpus is fed to a real
Godot build in CI and has to be accepted by the engine's own parser, so
"formats correctly" means Godot agrees rather than that `gdck` agrees with
itself. Reordering is held to a stronger promise still: it may only ever permute
a file's declarations, checked against both of those projects as well as the
corpus.

The formatter is also tested against the style guide's own worked examples: all
56 usable code samples are extracted from the documentation and asserted on
directly, so the guide decides what correct output is. See
[`crates/gdck-format/tests/style_guide.rs`](../crates/gdck-format/tests/style_guide.rs)
for the classification of every sample, including the three the formatter
knowingly does not reproduce and why.

The linter is held to the same corpus, on what it must never do rather than on
what it finds: `--fix` never turns a file that parsed into one that does not,
settles after one more pass, and never introduces a kind of problem the file did
not already have. `--fix-order` only ever permutes a file's bytes.

### Known gaps

- One configuration governs a whole run, found from the directory the given
  paths have in common. A repository holding several Godot projects with
  different settings needs one invocation per project.
- Naming conventions are not configurable. `gdck` checks the style guide's
  conventions directly rather than with a regular-expression engine, so a
  `gdlintrc` that customised one is reported as not applied rather than
  honoured. See [docs/CONFIG.md](CONFIG.md#gdtoolkit-compatibility).
- `code-order` cannot tell an overridden virtual method from a custom one, since
  that needs the whole inheritance chain. It orders the callbacks the guide names
  and leaves the rest as one group, rather than guessing. See
  [docs/RULES.md](RULES.md#what-code-order-does-and-does-not-check).
- Naming rules never offer a rename. A name is reached from scene files, from
  `call()` with a string, and from signals connected in the editor, none of which
  one file can see.
- CRLF and CR line endings are rewritten to LF, which the style guide mandates.
  There is no option to keep them.
- The formatter does not fill lines. Where the style guide hand-wraps a call or
  a boolean chain several items per line, `gdck` puts one per line. Both of the
  guide's samples of that are hand-formatted rather than derived from a column
  limit, so no deterministic rule reproduces them.
- The guide calls itself advice rather than a rulebook, and `gdck` turns it into
  exit codes. Everywhere that needed a decision the guide did not make — a
  trailing comma Godot requires, an order it stopped asking for, a length
  measured in columns — is listed in
  [docs/DESIGN.md](DESIGN.md#known-differences-from-the-guide).
- No `.gitignore` awareness when walking directories; only a fixed exclusion
  list (`.git`, `.godot`, `.import`, `addons`).
- The parser is more permissive than Godot in places. It is built to understand
  well-formed code, not to reject every invalid program — Godot is the authority
  on what compiles.
- Three files in `gdtoolkit`'s `potential-godot-bugs` corpus do not parse. They
  document cases where Godot's own behaviour is in question.
