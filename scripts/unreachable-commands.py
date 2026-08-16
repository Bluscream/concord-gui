#!/usr/bin/env python3
"""Every AppCommand no front end constructs, and every one only half of them do.

Clippy cannot find these: a command is public API, so it is never dead code.
The earlier shell version of this matched only struct variants, so a unit
variant like `LoadConnections` was invisible to it - which is how three
connection commands sat unreachable without the check noticing.
"""

import re
import subprocess
import sys

SOURCE = "src/discord/commands.rs"
CLIENTS = {"tui": "src/tui", "gui": "crates/gui/src"}

# Commands the core builds on a client's behalf, so no client names the variant.
#
# An allow-list rather than silence: each entry has to say who does build it and
# who sends it, and a wrong entry is then a wrong sentence somebody can read -
# which is better than a check that quietly skips things.
BUILT_IN_CORE = {
    # AccountForm::submit() builds it; both panels call that and send the
    # result. Naming the variant in a client would mean duplicating the
    # validation that decides whether it may be sent at all.
    "ModifyAccount": "AccountForm::submit, sent by both account panels",
}


def variants() -> list[str]:
    src = open(SOURCE).read()
    start = src.index("pub enum AppCommand {")
    body = src[start : src.index("\n}", start)]
    return re.findall(r"^    ([A-Z][A-Za-z0-9]*)\s*(?:\{|,|\()", body, re.M)


def reaches(client_path: str, variant: str) -> bool:
    # demo.rs answers commands rather than sending them, and tests are not a
    # way for a user to reach anything.
    found = subprocess.run(
        f'grep -rn "AppCommand::{variant}" {client_path} --include=*.rs 2>/dev/null'
        " | grep -v demo.rs | grep -v test",
        shell=True, capture_output=True, text=True,
    ).stdout.strip()
    return bool(found)


def main() -> int:
    problems = []
    for variant in variants():
        if variant in BUILT_IN_CORE:
            continue
        reached = {name: reaches(path, variant) for name, path in CLIENTS.items()}
        if not any(reached.values()):
            problems.append(f"unreachable from both: {variant}")
        elif not all(reached.values()):
            only = next(name for name, ok in reached.items() if ok)
            problems.append(f"{only} only: {variant}")

    for problem in problems:
        print(problem)
    # Named, so an allow-list entry cannot quietly outlive its reason.
    for variant, why in sorted(BUILT_IN_CORE.items()):
        print(f"allowed: {variant} - {why}")
    print(f"-- {len(variants())} commands checked, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
