#!/usr/bin/env python3
"""Survey the reference clients for advertised features.

Scans every repository in .references/ for feature keywords in its README and
other top-level markdown, and prints a matrix of feature -> clients.

This measures what a project *claims*, not what it does, and keyword matching
over-counts: "tabs" matches any mention of the word. Treat the output as a
popularity signal, not a census. Regenerates the table in docs/FEATURES.md.
"""

import json
import pathlib
import re
import sys

FEATURES = {
    "Voice / calls": r"voice (support|chat|call)|\bvoice\b.*\bcall|audio call",
    "Video / camera": r"\bwebcam\b|\bcamera\b|video call",
    "Screenshare": r"screen ?share|screen ?sharing|go live",
    "E2EE voice (DAVE)": r"\bdave\b|e2ee|end-to-end",
    "Noise suppression": r"noise suppression|krisp|rnnoise",
    "Multi-account": r"multi[- ]account|multiple accounts",
    "Browser impersonation": r"impersonat|super ?properties|identifies? .*web client|user ?agent",
    "Per-channel tabs": r"\btabs?\b",
    "Markdown parsing": r"markdown",
    "Embeds": r"\bembed",
    "File upload": r"file upload|upload file|attachment",
    "Image viewer": r"image viewer|lightbox|view images",
    "Inline images": r"inline image|image preview|show images",
    "Animated emoji": r"animated (emoji|avatar|icon)|\bapng\b|\bgif\b",
    "Custom emoji": r"custom emoji|emoji support|\bemojis?\b",
    "Stickers": r"\bsticker",
    "Reactions": r"\breact(ion|ing)?\b",
    "Edit / delete": r"\bedit\b.*\bdelete\b|delete message|edit message",
    "Reply": r"\breply|replies",
    "Pins": r"\bpin(ned|ning|s)?\b",
    "Threads": r"\bthread",
    "Forums": r"\bforum",
    "DMs / group DMs": r"\bdms?\b|direct message|group dm",
    "Typing indicators": r"typing indicator|is typing",
    "Unread / mentions": r"unread|mention indicator|ping",
    "Guild folders": r"guild folder|server folder|folders",
    "Notifications": r"notification",
    "Sounds": r"\bsounds?\b|audio cue",
    "Search": r"\bsearch\b",
    "QR login": r"\bqr\b",
    "Token login": r"\btoken\b.*login|login.*token",
    "Moderation (kick/ban)": r"\bkick\b|\bban\b",
    "Role management": r"manage roles|modify roles|role management",
    "Invite management": r"manage invites|invite management|create invite",
    "Emoji management": r"manage emoji",
    "Audit log": r"audit log",
    "Server management": r"server management|guild management|manage (server|guild)",
    "Rich presence / activity": r"rich presence|\bactivity\b|playing status",
    "Custom status": r"custom status|set status",
    "Theming": r"theme|styleable|customizable|css",
    "Keybindings / vim": r"keybind|vim|shortcut",
    "Cross-platform": r"cross[- ]platform|windows.*linux|linux.*macos",
    "Not Electron": r"not electron|without electron|no electron",
    "Slash commands": r"slash command|application command",
    "Polls": r"\bpoll",
    "Friends list": r"friend",
    "User profiles": r"user profile|view profile",
    "Read receipts / ack": r"mark as read|read state",
}


def survey(root: pathlib.Path) -> dict[str, list[str]]:
    rows: dict[str, list[str]] = {}
    for repo in sorted(p for p in root.glob("*/*__*") if p.is_dir()):
        text = ""
        for readme in (list(repo.glob("README*")) + list(repo.glob("*.md")))[:4]:
            try:
                text += readme.read_text(errors="ignore").lower()
            except OSError:
                continue
        if not text:
            continue
        name = repo.name.split("__")[0]
        for feature, pattern in FEATURES.items():
            if re.search(pattern, text):
                rows.setdefault(feature, []).append(name)
    return rows


def main() -> int:
    root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "../.references")
    if not root.is_dir():
        print(f"no reference tree at {root}", file=sys.stderr)
        return 1

    rows = survey(root)
    for feature, repos in sorted(rows.items(), key=lambda item: -len(item[1])):
        print(f"{len(repos):3d}  {feature}")
        print(f"     {', '.join(sorted(repos))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
