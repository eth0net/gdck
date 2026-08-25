#!/usr/bin/env python3
"""Time `gdck` against `gdtoolkit` on a corpus both tools accept.

The numbers in the README come from here, so that a claim about speed is one
anyone can check rather than one they have to take on trust.

    cargo build --release
    tools/benchmark.py ../godot-gdscript-toolkit

Both tools have to be on PATH: `gdck` is taken from `target/release` unless
`--gdck` says otherwise, and `gdparse`, `gdformat` and `gdlint` come from a
`gdtoolkit` install (`pip install gdtoolkit`, or `brew install gdtoolkit`).

Only files *both* tools parse are timed. The corpus carries deliberately
invalid scripts, and the two disagree about a handful of valid ones; timing a
file one of them rejects would measure how quickly it gave up rather than how
quickly it worked.

Each command runs in its own process, so what is measured is what a person
waits for — interpreter startup included, since that is paid on every
invocation and is a real part of why one of these feels slower than the other.
"""

import argparse
import glob
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

# gdck is quick enough that one run is mostly noise; gdtoolkit is slow enough
# that three is plenty and ten would only make this tedious to run.
FAST_RUNS = 10
SLOW_RUNS = 3


def time_ms(command, runs):
    """Mean wall-clock milliseconds over `runs`, after one warm-up."""
    subprocess.run(command, capture_output=True)
    samples = []
    for _ in range(runs):
        start = time.perf_counter()
        subprocess.run(command, capture_output=True)
        samples.append((time.perf_counter() - start) * 1000)
    return statistics.mean(samples)


def parses(command, path):
    return subprocess.run(command + [path], capture_output=True).returncode == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", help="a godot-gdscript-toolkit checkout")
    parser.add_argument(
        "--gdck",
        default=os.path.join("target", "release", "gdck"),
        help="the gdck binary to time (default: target/release/gdck)",
    )
    args = parser.parse_args()

    gdck = os.path.abspath(args.gdck)
    if not os.path.exists(gdck):
        sys.exit(f"no gdck at {gdck}; run `cargo build --release` first")
    for tool in ("gdparse", "gdformat", "gdlint"):
        if shutil.which(tool) is None:
            sys.exit(f"{tool} is not on PATH; install gdtoolkit to compare")

    found = glob.glob(os.path.join(args.corpus, "tests", "**", "*.gd"), recursive=True)
    if not found:
        sys.exit(f"no .gd files under {args.corpus}/tests")

    with tempfile.TemporaryDirectory() as work:
        # A copy, because `gdformat` writes in place and the corpus is someone
        # else's checkout. Only `--check` is used below, but a benchmark is not
        # the place to rely on that.
        kept = 0
        for path in sorted(found):
            if parses([gdck, "parse"], path) and parses(["gdparse"], path):
                shutil.copy(path, work)
                kept += 1
        files = sorted(glob.glob(os.path.join(work, "*.gd")))
        lines = sum(
            open(f, errors="ignore").read().count("\n") for f in files
        )
        skipped = len(found) - kept
        print(f"{kept} files, {lines} lines ({skipped} skipped: one tool or the other rejects them)\n")

        rows = [
            ("parse", [gdck, "parse", work], FAST_RUNS, ["gdparse"] + files, SLOW_RUNS),
            ("format", [gdck, "format", work], FAST_RUNS, ["gdformat", "--check"] + files, SLOW_RUNS),
            ("format, safety checks off", [gdck, "format", "--fast", work], FAST_RUNS, None, 0),
            ("lint", [gdck, "lint", work], FAST_RUNS, ["gdlint"] + files, SLOW_RUNS),
        ]

        print(f"| {'':26} | {'gdck':>8} | {'gdtoolkit':>10} | {'':>6} |")
        print(f"|{'-' * 28}|{'-' * 10}|{'-' * 12}|{'-' * 8}|")
        for label, ours, our_runs, theirs, their_runs in rows:
            mine = time_ms(ours, our_runs)
            if theirs is None:
                print(f"| {label:26} | {mine:6.0f} ms | {'—':>10} | {'':>6} |")
                continue
            other = time_ms(theirs, their_runs)
            print(
                f"| {label:26} | {mine:6.0f} ms | {other:7.0f} ms | {other / mine:5.0f}× |"
            )


if __name__ == "__main__":
    main()
