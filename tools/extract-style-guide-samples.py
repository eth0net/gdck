#!/usr/bin/env python3
"""Extract the GDScript style guide's code samples into test fixtures.

The style guide is the specification `gdck` is written against, and it carries
worked examples of nearly every rule it states. Turning those into fixtures
makes the guide itself the oracle, rather than someone's reading of it.

Usage:

    tools/extract-style-guide-samples.py path/to/godot-docs

Writes `crates/gdck-format/tests/style-guide/*.gd`, overwriting what is there.
Expected-output files (`*.expected.gd`) are curated by hand and left alone.

Two adjustments are made to the samples as written:

* The documentation renders indentation as four spaces; the guide itself
  mandates tabs, so leading four-space groups become tabs.
* Some samples are statement fragments rather than whole files. Those are
  wrapped in a function so they parse, and a bodyless signature gets a `pass`.
  Each sample is tried as written first, and only adjusted if it does not
  parse on its own.

The samples are part of the Godot documentation and are licensed CC BY 3.0.
See `licenses/README.md`.
"""

import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FIXTURES = REPO / "crates" / "gdck-format" / "tests" / "style-guide"
GUIDE = Path("tutorials/scripting/gdscript/gdscript_styleguide.rst")

# Samples that are prose formatted as a literal block rather than GDScript:
# the code-order list, and the public/private sub-list under it.
NOT_CODE = {"code_order__neutral", "code_order__neutral_2"}


def sections(text):
    """Yield (section, kind, body) for every literal block in the guide."""
    lines = text.split("\n")
    section = "intro"
    pending = None
    index = 0
    while index < len(lines):
        line = lines[index]

        underlined = (
            index + 1 < len(lines)
            and re.fullmatch(r"[~^-]{3,}", lines[index + 1].strip())
            and line.strip()
        )
        if underlined:
            section = re.sub(r"[^a-z0-9]+", "_", line.strip().lower()).strip("_")
            index += 2
            continue

        marker = re.match(r"\.\.\s+rst-class::\s+code-example-(good|bad)", line)
        if marker:
            pending = marker.group(1)
            index += 1
            continue

        if line.strip() == "::":
            index += 1
            while index < len(lines) and not lines[index].strip():
                index += 1
            body = []
            while index < len(lines):
                current = lines[index]
                if current.strip() and not current.startswith("    "):
                    break
                body.append(current[4:] if current.startswith("    ") else "")
                index += 1
            while body and not body[-1].strip():
                body.pop()
            yield section, pending or "neutral", "\n".join(body) + "\n"
            pending = None
            continue

        index += 1


def to_tabs(text):
    """Convert four-space indent levels to tabs, or return None if impossible.

    A sample indented by something that is not a whole number of levels is
    demonstrating indent *width*, which is the one thing that cannot survive
    the conversion — the guide's bad example of a two-space indent becomes
    either no indent or a correct one, and in both cases stops being the thing
    it was written to illustrate. Those are skipped rather than mangled.
    """
    out = []
    for line in text.split("\n"):
        stripped = line.lstrip(" ")
        spaces = len(line) - len(stripped)
        if spaces % 4 != 0:
            return None
        out.append("\t" * (spaces // 4) + stripped)
    return "\n".join(out)


def wrapped(text):
    body = "\n".join(
        ("\t" + line) if line.strip() else ""
        for line in text.rstrip("\n").split("\n")
    )
    return f"func _ready():\n{body}\n"


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    guide = Path(sys.argv[1]) / GUIDE
    if not guide.is_file():
        sys.exit(f"not found: {guide}")

    gdck = REPO / "target" / "debug" / "gdck"
    if not gdck.is_file():
        sys.exit("build gdck first: cargo build -p gdck-cli")

    with tempfile.TemporaryDirectory() as tmp:
        probe = Path(tmp) / "probe.gd"

        def parses(text):
            probe.write_text(text)
            # A non-zero exit is the answer being looked for, not an error.
            done = subprocess.run(
                [str(gdck), "parse", str(probe)],
                capture_output=True,
                text=True,
                check=False,
            )
            return done.returncode == 0

        FIXTURES.mkdir(parents=True, exist_ok=True)
        for stale in FIXTURES.glob("*.gd"):
            if not stale.name.endswith(".expected.gd"):
                stale.unlink()

        counts = {}
        written = 0
        skipped = []
        for section, kind, body in sections(guide.read_text()):
            key = (section, kind)
            counts[key] = counts.get(key, 0) + 1
            suffix = "" if counts[key] == 1 else f"_{counts[key]}"
            name = f"{section}__{kind}{suffix}"
            if name in NOT_CODE:
                continue

            text = to_tabs(body)
            if text is None:
                skipped.append(f"{name} (indent width has no tab equivalent)")
                continue
            for candidate in (text, wrapped(text), text.rstrip("\n") + "\n\tpass\n"):
                if parses(candidate):
                    (FIXTURES / f"{name}.gd").write_text(candidate)
                    written += 1
                    break
            else:
                skipped.append(f"{name} (does not parse)")

        print(f"wrote {written} fixtures to {FIXTURES.relative_to(REPO)}")
        for name in skipped:
            print(f"  skipped: {name}")


if __name__ == "__main__":
    main()
