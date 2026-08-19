# Editors

`gdck lsp` runs `gdck` as a language server over stdin and stdout. It is meant
to be started by an editor rather than by hand.

It offers three things, which are the three `gdck` already does:

- **Problems as you type** — every lint rule and every syntax error, on the
  buffer rather than on what was last saved.
- **Format document** — the same formatter `gdck format` runs, with the same
  safety checks. A file that does not parse, or that a safety check refuses, is
  left exactly as it is.
- **Quick fixes** — a fixable rule offers its own fix as a code action.

Settings are found per file, exactly as they are on the command line: the
search starts from the file's own directory, so a repository holding more than
one Godot project gets each project's own `gdck.toml`.

## Run it alongside Godot's language server

Godot ships its own GDScript server, and this does not replace it. That one
knows the project — completion, go-to-definition, the class reference — because
it is the engine. This one knows the style guide, and deliberately offers
nothing the engine already does.

Running both is the intended arrangement, and the one `ruff` and a type checker
share in Python. Every editor below supports more than one server for a
language.

## Neovim

With the built-in client, no plugin needed:

```lua
vim.lsp.config.gdck = {
  cmd = { "gdck", "lsp" },
  filetypes = { "gdscript" },
  root_markers = { "gdck.toml", "project.godot", ".git" },
}
vim.lsp.enable("gdck")
```

To format on save:

```lua
vim.api.nvim_create_autocmd("BufWritePre", {
  pattern = "*.gd",
  callback = function() vim.lsp.buf.format() end,
})
```

If more than one server attaches and both offer formatting, name the one you
want: `vim.lsp.buf.format({ name = "gdck" })`.

## Helix

In `languages.toml`:

```toml
[language-server.gdck]
command = "gdck"
args = ["lsp"]

[[language]]
name = "gdscript"
language-servers = ["godot", "gdck"]
```

## VS Code

There is no extension yet. Any generic LSP client extension can start
`gdck lsp` for `gdscript` files in the meantime.

## Zed

```json
{
  "lsp": {
    "gdck": { "binary": { "path": "gdck", "arguments": ["lsp"] } }
  }
}
```

## Without a language server

An editor that has no LSP client, or a setup already built around external
linters, can use `--output json` instead:

```sh
gdck check --output json .
```

One object per line, carrying the rule, the message, the position three ways
and — for a fixable rule — the edits. That is what `null-ls`/`none-ls`, ALE,
efm-langserver and a VS Code problem matcher want. See the README for the
shape.

## What it does not do

Completion, hover, go-to-definition, rename, symbols. All of them want a
resolved view of the project rather than one file's syntax, and Godot's own
server already provides them.

Incremental document sync is not implemented either, and that is a choice
rather than an omission: `gdck` re-reads the whole buffer on every keystroke
because parsing a GDScript file costs a fraction of a millisecond, which is
cheaper than the bookkeeping incremental sync needs and cannot drift out of
step with the editor's copy.

## Position encoding

The protocol counts UTF-16 code units unless the client asks for something
else. `gdck` advertises UTF-8 when a client offers it — its offsets are already
bytes — and falls back to UTF-16 otherwise. Both are implemented and tested; a
comment with an accent or an emoji in it lands where you would expect either
way.
