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
# Commands the core carries for a front end this repository does not have.
#
# A separate list from BUILT_IN_CORE: those are reachable and the sweep simply
# cannot see it. These genuinely are not reachable, on purpose - the core is
# meant to serve a future mobile GUI too, and a core that only serves the two
# clients here is one that has to be reopened later. Each entry says which
# front end would use it, so an entry that stops being true is a wrong sentence
# somebody can read rather than a silent exemption.
MOBILE_ONLY = {
    "SendPhoneCode": "phone as a credential - mobile",
    "SendPhoneCodeAgain": "phone as a credential - mobile",
    "AttachPhone": "phone as a credential - mobile",
    "ReverifyPhone": "phone as a credential - mobile",
    "RemovePhone": "phone as a credential - mobile",
    "SetSmsMfa": "SMS two-factor, needs an attached phone - mobile",
}

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


def orphaned_senders(client_path: str) -> list[str]:
    """Functions that build a command but that nothing calls.

    The variant check above cannot see these: the command is still mentioned,
    so the sweep reports the client as reaching it. Clippy cannot either, since
    these functions are `pub` and so never dead code. Both `mark_event_interest`
    and `move_role` sat orphaned this way after a button was rewired.
    """
    orphans = []
    for path in subprocess.run(
        f"grep -rl 'AppCommand::' {client_path} --include=*.rs",
        shell=True, capture_output=True, text=True,
    ).stdout.split():
        # Tests build commands to assert on them; that is not a send path.
        if "demo.rs" in path or "/tests/" in path or path.endswith("tests.rs"):
            continue
        source = open(path).read()
        # Everything from the first #[cfg(test)] onward is test code.
        source = source.split("#[cfg(test)]")[0]
        # Only functions that name a command; the rest are not senders.
        for name, body in re.findall(
            r"\n    (?:pub )?fn ([a-z_]+)\s*\([^)]*\)[^{]*\{(.*?)\n    \}",
            source, re.S,
        ):
            if "AppCommand::" not in body:
                continue
            # A call, or a bare reference - a function passed as a pointer is
            # reached just as surely as one that is called outright.
            calls = subprocess.run(
                f"grep -rn '[.:]{name}\\b' {client_path} --include=*.rs"
                f" | grep -v ' fn {name}'",
                shell=True, capture_output=True, text=True,
            ).stdout.strip()
            if not calls:
                orphans.append(f"{path.split('/')[-1]}::{name}")
    return orphans


def main() -> int:
    problems = []
    for variant in variants():
        if variant in BUILT_IN_CORE or variant in MOBILE_ONLY:
            continue
        reached = {name: reaches(path, variant) for name, path in CLIENTS.items()}
        if not any(reached.values()):
            problems.append(f"unreachable from both: {variant}")
        elif not all(reached.values()):
            only = next(name for name, ok in reached.items() if ok)
            problems.append(f"{only} only: {variant}")

    for client, path in CLIENTS.items():
        for orphan in orphaned_senders(path):
            problems.append(f"{client} builds a command nowhere calls: {orphan}")

    for problem in problems:
        print(problem)
    # Named, so an allow-list entry cannot quietly outlive its reason.
    for variant, why in sorted(BUILT_IN_CORE.items()):
        print(f"allowed: {variant} - {why}")
    for variant, why in sorted(MOBILE_ONLY.items()):
        print(f"core only: {variant} - {why}")
    print(f"-- {len(variants())} commands checked, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
