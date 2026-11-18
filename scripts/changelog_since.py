#!/usr/bin/env python3
"""Prints the CHANGELOG.md [Unreleased] entries added since the release before a tag.

Usage: changelog_since.py <tag>

The release before <tag> is the highest-versioned v* tag below it, wherever it sits in the
history: a pre-release can be tagged on a commit off main. The release workflow uses the
output as the notes of a pre-release that RELEASE_NOTES.md has no section for.
"""

import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

TAG = re.compile(r"v(\d+)\.(\d+)\.(\d+)(?:-(.+))?")


def version_key(tag: str) -> tuple | None:
    """Orders release tags; a suffixed version comes before its plain one, beta2 before beta10."""
    match = TAG.fullmatch(tag)
    if match is None:
        return None
    major, minor, patch, suffix = match.groups()
    numbers = (int(major), int(minor), int(patch))
    if suffix is None:
        return numbers + (1, ())
    parts = tuple((0, int(part), "") if part.isdigit() else (1, 0, part) for part in re.findall(r"\d+|\D+", suffix))
    return numbers + (0, parts)


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def previous_release(tag: str) -> str:
    current = version_key(tag)
    if current is None:
        raise SystemExit(f"{tag} is not a vX.Y.Z or vX.Y.Z-suffix tag")
    earlier = [(key, name) for name in git("tag", "--list", "v[0-9]*").split() if (key := version_key(name)) and key < current]
    if not earlier:
        raise SystemExit(f"no release tag comes before {tag}")
    return max(earlier)[1]


def unreleased(text: str) -> list[tuple[str, str]]:
    """The [Unreleased] entries of a changelog, each with its heading, in order."""
    entries = []
    inside = False
    heading = ""
    for line in text.splitlines():
        if line.startswith("## "):
            inside = line.strip() == "## [Unreleased]"
        elif inside and line.startswith("### "):
            heading = line.strip()
        elif inside and line.startswith("- "):
            entries.append((heading, line.rstrip()))

    return entries


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    previous = previous_release(sys.argv[1])
    # Counted, so a second copy of an entry added since still shows.
    known = Counter(unreleased(git("show", f"{previous}:CHANGELOG.md")))

    added: dict[str, list[str]] = {}
    for heading, entry in unreleased(Path("CHANGELOG.md").read_text(encoding="utf-8")):
        if known[(heading, entry)] > 0:
            known[(heading, entry)] -= 1
        else:
            added.setdefault(heading, []).append(entry)

    blocks = ["\n".join(([heading, ""] if heading else []) + entries) for heading, entries in added.items()]
    print("\n\n".join(blocks))
    return 0


if __name__ == "__main__":
    sys.exit(main())
