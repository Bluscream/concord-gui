#!/usr/bin/env python3
"""Remove `use` lines a merge duplicated.

    dedupe-imports.py <file> [<file> ...]

Taking upstream's side of an import hunk very often re-adds names the fork
already imports a few lines down, because the fork's copy was rewritten
(`crate::` became `concord::`) and no longer looks like the same line to git.
The result is E0252 - "defined multiple times" - across a dozen files, which is
tedious and completely mechanical to fix.

This walks a file's top-level `use` statements, records every name each one
binds, and drops any whole statement whose names are all already bound by an
earlier one. A statement that binds anything new is left alone: merging two
partially-overlapping lists is a judgement call, and a wrong guess here removes
an import that was doing work.
"""

import pathlib
import re
import sys

USE = re.compile(r"^use\s+(.*?);\s*$", re.S)


def statements(text):
    """Yield (start, end, body) for each top-level `use ...;`, braces included."""
    lines = text.splitlines(keepends=True)
    i = 0
    while i < len(lines):
        if lines[i].startswith("use "):
            start = i
            depth = lines[i].count("{") - lines[i].count("}")
            while depth > 0 or not lines[i].rstrip().endswith(";"):
                i += 1
                if i >= len(lines):
                    return
                depth += lines[i].count("{") - lines[i].count("}")
            yield start, i + 1, "".join(lines[start : i + 1])
        i += 1


def bound_names(body):
    """The identifiers a `use` statement brings into scope."""
    match = USE.match(body.strip())
    if not match:
        return set()
    path = match.group(1)
    if path.endswith("*"):
        return set()
    if " as " in path:
        return {path.rsplit(" as ", 1)[1].strip()}
    if "{" in path:
        if "}" not in path:
            # A `use` whose braces the merge left unbalanced. Nothing here can
            # be said about what it binds, so say nothing.
            return set()
        inner = path[path.index("{") + 1 : path.rindex("}")]
    else:
        inner = path
    names = set()
    for piece in re.split(r",(?![^{]*})", inner):
        piece = piece.strip()
        if not piece or piece.endswith("*"):
            continue
        if "{" in piece:
            if "}" not in piece:
                continue
            head = piece[: piece.index("{")].rstrip(": ")
            for nested in piece[piece.index("{") + 1 : piece.rindex("}")].split(","):
                nested = nested.strip()
                if nested and nested != "self":
                    names.add(nested.split("::")[-1])
            if head:
                continue
            continue
        names.add(piece.split("::")[-1].strip())
    return {n for n in names if n and n != "self"}


def dedupe(path):
    text = path.read_text()
    lines = text.splitlines(keepends=True)
    seen, drop = set(), []
    for start, end, body in statements(text):
        names = bound_names(body)
        if names and names <= seen:
            drop.append((start, end))
        else:
            seen |= names
    if not drop:
        return 0
    for start, end in reversed(drop):
        del lines[start:end]
    path.write_text("".join(lines))
    return len(drop)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    total = 0
    for name in sys.argv[1:]:
        path = pathlib.Path(name)
        # Per file, deliberately: one unparseable file used to raise and take
        # the whole run with it, so a single oddity silently disabled the pass
        # for every other file on the command line.
        try:
            dropped = dedupe(path)
        except (OSError, ValueError) as error:
            print(f"{path}: skipped ({error})", file=sys.stderr)
            continue
        if dropped:
            print(f"{path}: dropped {dropped} duplicate use statement(s)")
        total += dropped
    print(f"{total} removed")
    return 0


sys.exit(main())
