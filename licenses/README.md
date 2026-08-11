# Third-party licenses

`gdck` itself is dual licensed MIT OR Apache-2.0; see `LICENSE-MIT` and
`LICENSE-APACHE` at the repository root.

This directory holds the licenses of third-party material vendored into the
repository.

## Godot documentation

`crates/gdck-format/tests/style-guide/*.gd` are the code samples from the
[GDScript style guide][guide], extracted verbatim by
`tools/extract-style-guide-samples.py`. They are used as test fixtures so that
the guide itself, rather than a reading of it, decides what the formatter must
produce.

[guide]: https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_styleguide.html

- **Source**: [godotengine/godot-docs](https://github.com/godotengine/godot-docs),
  `tutorials/scripting/gdscript/gdscript_styleguide.rst`
- **Revision**: `97184e42c7930e8b46293bd746f304d29290b44f` (2026-08-10)
- **Copyright**: Juan Linietsky, Ariel Manzur and the Godot community
- **License**: Creative Commons Attribution 3.0 Unported (CC BY 3.0), a copy of
  which is in `godot-docs/LICENSE.txt`

Two mechanical changes are applied on extraction and are described in the tool:
indentation is converted from the four spaces the documentation renders to the
tabs the guide itself mandates, and samples that are statement fragments are
wrapped in a function so they form valid files.

## Note on the reference implementation

[`godot-gdscript-toolkit`](https://github.com/Scony/godot-gdscript-toolkit) is
MIT licensed. `gdck` is an independent reimplementation and contains none of its
code. Its test corpus is used for conformance checking by pointing `GDCK_CORPUS`
at a separate checkout, so no fixtures are copied into this repository.

If any of that corpus is ever vendored here, it keeps its own MIT license and
attribution, recorded in this directory.
