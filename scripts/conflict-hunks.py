#!/usr/bin/env python3
"""Show or resolve conflict hunks one at a time.

    conflict-hunks.py show <file>
    conflict-hunks.py take <file> <choice> [<choice> ...]

A choice per hunk, in order: ours, theirs, both, none.

Written during the v2.5.10 merge, where 38 files carried 54 hunks and almost
every one wanted a different side. Opening each file in an editor to pick
between two blocks is the slowest possible way to do that, and `git checkout
--ours` is the wrong tool because it takes the whole file.

`both` concatenates ours then theirs, which is what an import block that gained
a line on each side wants. `none` drops the hunk entirely - upstream deleting
something we also changed usually ends there.
"""

import pathlib
import re
import sys

START = re.compile(r"^<<<<<<< ")
MID = re.compile(r"^=======\s*$")
END = re.compile(r"^>>>>>>> ")


def split(lines):
    """Into ('text', lines) and ('conflict', ours, theirs) runs."""
    out, i = [], 0
    while i < len(lines):
        if START.match(lines[i]):
            ours, theirs, i = [], [], i + 1
            while i < len(lines) and not MID.match(lines[i]):
                ours.append(lines[i])
                i += 1
            i += 1
            while i < len(lines) and not END.match(lines[i]):
                theirs.append(lines[i])
                i += 1
            i += 1
            out.append(("conflict", ours, theirs))
        elif out and out[-1][0] == "text":
            out[-1][1].append(lines[i])
            i += 1
        else:
            out.append(("text", [lines[i]]))
            i += 1
    return out


def show(conflicts):
    for n, (_, ours, theirs) in enumerate(conflicts, 1):
        print(f"───── hunk {n} ── OURS ({len(ours)} lines)")
        print("".join(ours[:40]), end="")
        print(f"───── hunk {n} ── THEIRS ({len(theirs)} lines)")
        print("".join(theirs[:40]), end="")
        print()


def take(path, parts, conflicts, choices):
    if len(choices) != len(conflicts):
        print(
            f"{path}: {len(conflicts)} hunks, {len(choices)} choices given",
            file=sys.stderr,
        )
        return 1

    out, k = [], 0
    for kind, *rest in parts:
        if kind == "text":
            out.extend(rest[0])
            continue
        ours, theirs = rest
        choice = choices[k]
        k += 1
        if choice == "ours":
            out.extend(ours)
        elif choice == "theirs":
            out.extend(theirs)
        elif choice == "both":
            out.extend(ours)
            out.extend(theirs)
        elif choice == "none":
            pass
        else:
            print(f"bad choice: {choice}", file=sys.stderr)
            return 1
    path.write_text("".join(out))
    print(f'{path}: {len(conflicts)} hunk(s) -> {" ".join(choices)}')
    return 0


def main():
    if len(sys.argv) < 3 or sys.argv[1] not in {"show", "take"}:
        print(__doc__)
        return 1
    mode, path = sys.argv[1], pathlib.Path(sys.argv[2])
    parts = split(path.read_text().splitlines(keepends=True))
    conflicts = [p for p in parts if p[0] == "conflict"]
    if not conflicts:
        print(f"{path}: no conflict markers", file=sys.stderr)
        return 1
    if mode == "show":
        show(conflicts)
        return 0
    return take(path, parts, conflicts, sys.argv[3:])


sys.exit(main())
