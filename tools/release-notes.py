#!/usr/bin/env python3
"""Turn a CHANGELOG section into notes that render correctly on GitHub.

    tools/release-notes.py 0.8.0 > notes.md

GitHub renders a release body with GitHub Flavored Markdown, which treats a
single newline as a line break. A repository's `.md` files are rendered without
that, so the same paragraph reads as a paragraph in `CHANGELOG.md` and as a
ragged column of 79-character lines in the release notes. Two fixes are
possible: stop wrapping the changelog, which makes it worse to read and to
diff, or unwrap it on the way out. This is the second.

It also carries the link definitions across. A section referring to `[#9]`
keeps its definition at the foot of `CHANGELOG.md`, and lifting the section out
without it leaves a literal `[#9]` in the notes.

Code fences, tables, headings and list markers are left exactly as they are;
only the continuation lines of paragraphs and list items are joined.
"""

import argparse
import re
import sys

FENCE = re.compile(r"^\s*```")
TABLE = re.compile(r"^\s*\|")
HEADING = re.compile(r"^\s*#")
# `- `, `* `, `1. ` — the start of a new item, which must not be joined to the
# one above it.
BULLET = re.compile(r"^(\s*)([-*+]|\d+\.)\s")
# `[#9]: https://...` at the foot of the file.
DEFINITION = re.compile(r"^\[([^\]]+)\]:\s")


def section(text, version):
    """The body of one version's section, without its heading."""
    lines = text.split("\n")
    start = None
    for i, line in enumerate(lines):
        if re.match(rf"^## \[{re.escape(version)}\]", line):
            start = i + 1
            break
    if start is None:
        sys.exit(f"no section for {version} in the changelog")
    end = len(lines)
    for i in range(start, len(lines)):
        if lines[i].startswith("## "):
            end = i
            break
    return lines[start:end]


def definitions(text):
    found = {}
    for line in text.split("\n"):
        match = DEFINITION.match(line)
        if match:
            found[match.group(1)] = line
    return found


def unwrap(lines):
    """Join the continuation lines of paragraphs and list items."""
    out = []
    in_fence = False
    for line in lines:
        if FENCE.match(line):
            in_fence = not in_fence
            out.append(line)
            continue
        # Inside a fence every line is content, including blank ones.
        if in_fence or not line.strip():
            out.append(line)
            continue
        # These own their line: joining them would break the construct.
        if TABLE.match(line) or HEADING.match(line) or DEFINITION.match(line):
            out.append(line)
            continue
        # A new list item starts a new line; anything else may continue the
        # previous one.
        starts_item = bool(BULLET.match(line))
        joinable = (
            out
            and out[-1].strip()
            and not FENCE.match(out[-1])
            and not TABLE.match(out[-1])
            and not HEADING.match(out[-1])
            and not DEFINITION.match(out[-1])
        )
        if joinable and not starts_item:
            out[-1] = out[-1].rstrip() + " " + line.strip()
        else:
            out.append(line)
    return out


def render(text, version):
    body = unwrap(section(text, version))

    # Only the definitions this section actually refers to, so the notes do not
    # carry the whole changelog's worth.
    joined = "\n".join(body)
    used = []
    for label, definition in definitions(text).items():
        if re.search(rf"\[{re.escape(label)}\](?!:)", joined):
            used.append(definition)
    if used:
        while body and not body[-1].strip():
            body.pop()
        body += [""] + sorted(used)

    return "\n".join(body).strip() + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="the version to extract, e.g. 0.8.0")
    parser.add_argument("--changelog", default="CHANGELOG.md")
    args = parser.parse_args()
    sys.stdout.write(render(open(args.changelog).read(), args.version))


if __name__ == "__main__":
    main()
