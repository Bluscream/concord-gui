#!/usr/bin/env python3
"""Every documented Discord route this client never calls.

The command sweep asks whether both front ends can reach what the core builds.
This asks the question one level out: whether the core builds everything the
API offers. Neither finds what the other does.

Documentation comes from the community API reference in `.references/`, which
is a sibling of the repository rather than inside it - it is large, and it is
somebody else's work.
"""

import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

DOCS = Path("../.references/official-internals/userdoccers__github/pages")

# Filtered out rather than listed: selling, measuring, and the developer
# portal. `docs/FEATURE-BACKLOG.md` says why each group is out of scope.
OUT_OF_SCOPE = (
    "quests", "store", "billing", "entitlements", "promotions", "collectibles",
    "virtual-currency", "subscription-plans", "partner-sdk", "teams",
    "hypesquad", "referrals", "outbound-promotions", "bogo-promotions",
    "storefront", "companies", "company", "application-directory", "games",
    "game", "consoles", "modules", "downloads", "download", "shop",
    "social-sdk", "apex", "unverified-applications", "applications",
    "applications-with-assets", "application-identities", "subscription",
    "payment", "gifts", "widget-configs", "dev-portal-csat-survey-response",
)
OUT_OF_SCOPE_SUBSTRINGS = (
    "premium", "entitle", "monetiz", "role-subscription", "analytics",
    "billing", "gift-code", "commerce",
)


def normalise(url: str) -> str:
    """Collapse ids so `/guilds/123/bans` and `/guilds/{id}/bans` match."""
    url = re.sub(r"\{[^}]*\}", "{}", url)
    url = re.sub(r"/\d+", "/{}", url)
    return url.strip("/")


def documented() -> dict[str, set[str]]:
    routes: dict[str, set[str]] = defaultdict(set)
    pattern = re.compile(r'RouteHeader method="([A-Z]+)" url="([^"]+)"')
    for path in DOCS.rglob("*.mdx"):
        for method, url in pattern.findall(path.read_text(errors="replace")):
            routes[normalise(url)].add(method)
    return routes


def ours() -> set[str]:
    found = subprocess.run(
        r"grep -rhoE 'discord\.com/api/v9/[a-z0-9@{}/_.-]+' src crates --include=*.rs",
        shell=True, capture_output=True, text=True,
    ).stdout
    return {normalise(line.split("/api/v9/", 1)[1]) for line in found.splitlines() if "/api/v9/" in line}


def in_scope(url: str) -> bool:
    head = url.split("/")[0]
    if head in OUT_OF_SCOPE:
        return False
    return not any(word in url for word in OUT_OF_SCOPE_SUBSTRINGS)


def main() -> int:
    if not DOCS.is_dir():
        print(f"no API documentation at {DOCS} - see docs/FEATURE-BACKLOG.md")
        return 2

    called = ours()
    gaps = sorted(
        (url, methods)
        for url, methods in documented().items()
        if url not in called and in_scope(url)
    )

    by_area: dict[str, int] = defaultdict(int)
    for url, methods in gaps:
        by_area[url.split("/")[0]] += 1
        print(f"{' '.join(sorted(methods)):24} /{url}")

    print(f"\n-- {len(called)} routes called, {len(gaps)} documented and unused")
    for area, count in sorted(by_area.items(), key=lambda pair: -pair[1])[:10]:
        print(f"   {count:4}  {area}")
    # Informational: unlike the command sweep, a gap here is a feature nobody
    # has built yet rather than something broken, so this never fails a build.
    return 0


if __name__ == "__main__":
    sys.exit(main())
