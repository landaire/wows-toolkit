#!/usr/bin/env python3
"""Fails when the Buck2 pins disagree, or the prelude was not re-vendored.

The vendored prelude only loads under the Buck2 release it was expanded from,
and that release is pinned twice: in the DotSlash file bin/buck2 runs, and in
the Windows toolchain manifest. scripts/vendor-prelude.nu records the release it
expanded the prelude from in prelude/VENDORED_FROM.
"""

import json
import re
import sys
from pathlib import Path

RELEASE_URL = re.compile(r"https://github\.com/facebook/buck2/releases/download/([^/]+)/")


def tags_in(text: str) -> set[str]:
    return set(RELEASE_URL.findall(text))


def main() -> int:
    dotslash = Path("toolchains/buck2").read_text(encoding="utf-8")
    # The DotSlash file is JSON after its shebang line.
    pinned = json.loads(dotslash.split("\n", 1)[1])
    pins = {
        "toolchains/buck2": {
            tag
            for platform in pinned["platforms"].values()
            for provider in platform["providers"]
            for tag in tags_in(provider["url"])
        },
        "toolchains/windows/toolchain-manifest.json": tags_in(
            Path("toolchains/windows/toolchain-manifest.json").read_text(encoding="utf-8")
        ),
    }
    vendored = re.findall(r"^Buck2 release: (\S+)$", Path("prelude/VENDORED_FROM").read_text(encoding="utf-8"), re.M)
    pins["prelude/VENDORED_FROM"] = set(vendored)

    failed = False
    for source, tags in pins.items():
        if len(tags) != 1:
            print(f"{source} names Buck2 releases {sorted(tags)}; it must name exactly one.", file=sys.stderr)
            failed = True
    if failed:
        return 1

    releases = {source: next(iter(tags)) for source, tags in pins.items()}
    if len(set(releases.values())) != 1:
        for source, tag in releases.items():
            print(f"{source}: {tag}", file=sys.stderr)
        print("The Buck2 pins disagree. After moving one, move the others and re-vendor with nu scripts/vendor-prelude.nu.", file=sys.stderr)
        return 1

    print(f"Every Buck2 pin and the vendored prelude name release {releases['toolchains/buck2']}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
