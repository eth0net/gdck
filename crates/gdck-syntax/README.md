# gdck-syntax

A lossless lexer and parser for [GDScript](https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/).

Part of [`gdck`](https://github.com/eth0net/gdck), but usable on its own.

## What "lossless" means here

Every byte of the source lives in exactly one token, and every token lives in
the tree. The input is always exactly recoverable:

```rust
let tree = gdck_syntax::parse(source);
assert_eq!(tree.text(), source);
```

That holds for whitespace, comments and blank lines, and it holds for files with
syntax errors too. Parsing never fails — malformed input produces
`SyntaxKind::Error` nodes and entries in `tree.errors()`, so tools can report
several problems per run and editors can work with a half-typed buffer.

This is what a formatter needs (rewrite one declaration, leave the comments
around it alone) and what an editor needs (a tree for every keystroke).

## Example

```rust
use gdck_syntax::{SyntaxKind, SyntaxNode};

let tree = gdck_syntax::parse("@export var health: int = 100\n");

for node in tree.root().descendants() {
    if node.kind() == SyntaxKind::VarDecl {
        println!("{:?} at {}", node.text(), node.range());
    }
}
```

## Handled

Full GDScript 4 declaration and expression grammar: annotations, `class_name`,
`extends`, signals, enums, constants, static and `@abstract` members, inner
classes, property accessors, typed and inferred variables, variadic parameters,
lambdas including multi-line ones inside brackets, `match` patterns with
bindings and rest markers, and every literal form (`$Node/Path`, `%Unique`,
`&"StringName"`, `^"NodePath"`, raw and triple-quoted strings, all number
bases).

Validated against the 324 valid `.gd` files in `godot-gdscript-toolkit`'s test
suites, and round-trips all 353 files there.

## Not a validator

This parser is built to understand well-formed code, not to reject every invalid
program. It is more permissive than Godot in places. Godot remains the authority
on what actually compiles.

## License

MIT OR Apache-2.0
