#!/usr/bin/env python3
"""Split `use crate::{…}` groups that straddle the workspace boundary.

    split-crate-imports.py <file> [<file> ...]

Upstream is one crate, so it writes a single grouped import:

    use crate::{discord::{A, B}, tui::state::DashboardState};

Here `discord` lives in the `concord` crate and `tui` is local, so that one
statement has to become two. The flat form (`use crate::discord::A;`) is a
plain textual substitution and `upstream.sh` already does it; this grouped
form is not, which is why it kept arriving as a compile error instead.

Each top-level entry of the group is re-emitted under `concord::` or `crate::`
depending on its first segment. Anything whose shape is not understood is left
untouched: a wrong guess here silently repoints code at a different module.
"""

import pathlib
import re
import sys

# Modules that moved to the `concord` crate. Must match REWRITE_RE in
# upstream.sh - and, like it, must not list `app` or `tui`, which are valid on
# both sides and mean different things.
FOREIGN = {"discord", "config", "logging", "support", "risk", "translation"}

START = re.compile(r"^use crate::\{\s*$")
# The same statement written on one line. Upstream's rustfmt settings keep a
# short group inline, so this form arrives too and used to be skipped.
ONE_LINE = re.compile(r"^use crate::\{(.*)\};\s*$")


def split_entries(body):
    """Top-level comma-separated entries of a brace group, braces respected."""
    entries, depth, current = [], 0, []
    for char in body:
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
        if char == "," and depth == 0:
            entries.append("".join(current).strip())
            current = []
            continue
        current.append(char)
    tail = "".join(current).strip()
    if tail:
        entries.append(tail)
    return [entry for entry in entries if entry]


def emit(entries):
    """The replacement statements, or None when the group needs no splitting."""
    heads = {entry.split("::", 1)[0].split("{", 1)[0].strip() for entry in entries}
    if not heads & FOREIGN:
        # Entirely local: the group stays as it is.
        return None
    if heads <= FOREIGN:
        # Entirely foreign, so the group needs no splitting - only its root
        # renamed. `rewrite_imports` cannot do it either: its substitution
        # matches `crate::discord`, and here the two halves sit on different
        # lines. Nothing handled this shape until it appeared in v2.6.0.
        out = ["use concord::{\n"]
        out.extend(f"    {entry},\n" for entry in entries)
        out.append("};\n")
        return out
    out = []
    for crate in ("concord", "crate"):
        picked = [
            entry
            for entry in entries
            if (entry.split("::", 1)[0].split("{", 1)[0].strip() in FOREIGN)
            == (crate == "concord")
        ]
        if not picked:
            continue
        if len(picked) == 1:
            out.append(f"use {crate}::{picked[0]};\n")
        else:
            out.append(f"use {crate}::{{\n")
            out.extend(f"    {entry},\n" for entry in picked)
            out.append("};\n")
    return out


def rewrite(path):
    text = path.read_text()
    # A conflict marker sits inside the very brace group this rewrites, so it
    # would be read as one more import and emitted into the middle of a
    # statement. Refuse the file; it is not resolved yet.
    if "\n<<<<<<<" in text or text.startswith("<<<<<<<"):
        raise ValueError("file still has conflict markers")
    lines = text.splitlines(keepends=True)
    out, i, changed = [], 0, 0
    while i < len(lines):
        inline = ONE_LINE.match(lines[i])
        if inline:
            emitted = emit(split_entries(inline.group(1)))
            if emitted is None:
                out.append(lines[i])
            else:
                out.extend(emitted)
                changed += 1
            i += 1
            continue
        if not START.match(lines[i]):
            out.append(lines[i])
            i += 1
            continue
        depth, j = 1, i + 1
        while j < len(lines) and depth > 0:
            depth += lines[j].count("{") - lines[j].count("}")
            j += 1
        if depth != 0 or not lines[j - 1].strip().startswith("};"):
            out.append(lines[i])
            i += 1
            continue
        emitted = emit(split_entries("".join(lines[i + 1 : j - 1])))
        if emitted is None:
            out.append(lines[i])
            i += 1
            continue
        out.extend(emitted)
        changed += 1
        i = j
    if changed:
        path.write_text("".join(out))
    return changed


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    total = 0
    for name in sys.argv[1:]:
        path = pathlib.Path(name)
        if not path.is_file():
            continue
        try:
            split = rewrite(path)
        except (OSError, ValueError) as error:
            print(f"{path}: skipped ({error})", file=sys.stderr)
            continue
        if split:
            print(f"{path}: split {split} grouped import(s)")
        total += split
    print(f"{total} split")
    return 0


sys.exit(main())
