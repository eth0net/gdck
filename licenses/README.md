# Third-party licenses

`gdck` itself is dual licensed MIT OR Apache-2.0; see `LICENSE-MIT` and
`LICENSE-APACHE` at the repository root.

This directory holds the licenses of any third-party material vendored into the
repository. Nothing is vendored at present.

## Note on the reference implementation

[`godot-gdscript-toolkit`](https://github.com/Scony/godot-gdscript-toolkit) is
MIT licensed. `gdck` is an independent reimplementation and contains none of its
code. Its test corpus is used for conformance checking by pointing `GDCK_CORPUS`
at a separate checkout, so no fixtures are copied into this repository.

If any of that corpus is ever vendored here, it keeps its own MIT license and
attribution, recorded in this directory.
