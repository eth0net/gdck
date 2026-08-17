# Git hooks

`gdck` ships hook definitions for [`prek`][prek] and [`pre-commit`][pre-commit].
Both read the same `.pre-commit-hooks.yaml` from this repository, so one set of
definitions serves either tool and you can switch between them without changing
what runs.

Nothing here needs Python, and nothing here needs a Rust toolchain.

Ready-to-copy configurations for both are in
[`hooks/examples/`](../hooks/examples): [`prek.toml`](../hooks/examples/prek.toml)
and
[`.pre-commit-config.yaml`](../hooks/examples/.pre-commit-config.yaml).
Take the one for your tool, set `rev`, and you are done. Both are run by this
project's own test suite, so neither can drift from the hooks it names.

[prek]: https://prek.j178.dev
[pre-commit]: https://pre-commit.com

## The hooks

| Hook | Runs | Writes to your files | Replaces |
|---|---|---|---|
| `gdck-format` | `gdck format --fix` | yes | `gdformat` |
| `gdck-lint` | `gdck lint` | no | `gdlint` |
| `gdck-fix` | `gdck fix` | yes | both, with fixes |
| `gdck-check` | `gdck check` | no | both, reporting only |

`gdck-format` and `gdck-lint` exist to be dropped in where `gdformat` and
`gdlint` were. If you are not migrating an existing config, prefer one of the
combined hooks: `gdck-fix` does in a single pass what `gdck-format` and
`gdck-lint --fix` would do in two, and a file is only read once.

Pick one writing hook. Running `gdck-format` and `gdck-fix` together means the
second one re-reads what the first just wrote, for no benefit.

## prek

`prek` is the one to reach for: it is a single binary with no Python to
install, and it reads its own `prek.toml`, which is the format this project
documents first.

```toml
[[repos]]
repo = "https://github.com/eth0net/gdck"
rev = "v0.6.0"
hooks = [
  { id = "gdck-fix" },
]
```

Then:

```sh
prek install
```

`prek` also reads `.pre-commit-config.yaml` if you have one, so an existing
config keeps working as-is — see the next section for its shape.

## pre-commit

```yaml
repos:
  - repo: https://github.com/eth0net/gdck
    rev: v0.6.0
    hooks:
      - id: gdck-fix
```

Then:

```sh
pre-commit install
```

## Migrating from gdtoolkit

Replace the `gdtoolkit` block with the `gdck` one. In `prek.toml`:

```toml
# before
[[repos]]
repo = "https://github.com/Scony/godot-gdscript-toolkit"
rev = "4.5.0"
hooks = [
  { id = "gdformat" },
  { id = "gdlint" },
]

# after
[[repos]]
repo = "https://github.com/eth0net/gdck"
rev = "v0.6.0"
hooks = [
  { id = "gdck-format" },
  { id = "gdck-lint" },
]
```

Or in `.pre-commit-config.yaml`:

```yaml
# before
repos:
  - repo: https://github.com/Scony/godot-gdscript-toolkit
    rev: 4.5.0
    hooks:
      - id: gdformat
      - id: gdlint

# after
repos:
  - repo: https://github.com/eth0net/gdck
    rev: v0.6.0
    hooks:
      - id: gdck-format
      - id: gdck-lint
```

Two things to expect on the first run.

`gdck` reports more than `gdlint` did, because it also checks formatting and
because `code-order` resolves cases `gdlint` left alone. That is not a config
you are missing; [docs/RULES.md](RULES.md#differences-from-gdtoolkit) explains
each difference and how to switch any of them off.

If the project has a `gdlintrc` or `.gdformatrc`, run `gdck init` **before** the
first hook run. It reads those and writes an equivalent `gdck.toml`, so your
settings carry over instead of being silently replaced by defaults. See
[docs/CONFIG.md](CONFIG.md#migrating).

## Which files the hooks see

All four hooks are declared `types: [gdscript]`, so the tool that runs them
selects `.gd` files and passes them in explicitly.

That matters more than it sounds, because **`files.exclude` in your `gdck.toml`
does not apply to a hook run.** It governs which files `gdck` finds when it
walks a directory, and a hook run never walks anything — the file list arrives
already chosen. A file named outright is always checked:

```sh
$ gdck check .              # addons is excluded by default
No .gd files found.

$ gdck check addons/a.gd    # named outright, so it is checked
addons/a.gd: would be reformatted
```

Since `addons` is in the default `exclude`, the practical effect is that a hook
run reports on addon code that `gdck check .` stays quiet about. That is the
same thing `gdformat` and `gdlint` do as hooks, so a migrated config behaves as
it did — but it will not match what you see running `gdck` yourself.

Use the hook runner's own `exclude` to keep files out:

```toml
hooks = [
  { id = "gdck-fix", exclude = '^addons/' },
]
```

```yaml
- id: gdck-fix
  exclude: ^addons/
```

`.gitignore` awareness needs no such care. An ignored file is not staged, so it
never reaches a hook to begin with.

## How the binary gets there

`hooks/gdck` fetches the prebuilt binary for whichever `rev` you pinned and
caches it, then hands off to it. The version is read from the `Cargo.toml` in
the checked-out revision, so the binary always matches the tag you asked for
and there is no version string to keep in step by hand.

Both `prek` and `pre-commit` can bootstrap a Rust toolchain and build a hook
from source, and neither is asked to here. Doing so would mean roughly a 300MB
toolchain download and a full compile of the workspace, on every machine and
every cold CI runner, to produce a binary already built and published for seven
targets. Fetching that archive instead takes seconds, which is the standard the
tool being replaced sets: `gdformat` arrives with `pip install`.

The cache lives in `${XDG_CACHE_HOME:-~/.cache}/gdck-hook/<version>/`, keyed by
version so that bumping `rev` fetches the new binary and leaves the old one
alone. Set `GDCK_HOOK_CACHE` to move it.

To run a build of your own instead — a local checkout, or a version you patched
— point `GDCK_HOOK_BINARY` at it and no download happens:

```sh
GDCK_HOOK_BINARY=$PWD/target/release/gdck prek run --all-files
```

The fetch needs `curl` or `wget`, and runs over HTTPS with a pinned minimum TLS
version. If a machine has neither, the hook fails with that as the reason
rather than falling back to something less safe.
