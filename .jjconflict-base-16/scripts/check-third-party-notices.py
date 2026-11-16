#!/usr/bin/env python3
"""Fails when THIRD_PARTY_NOTICES.txt is stale.

scripts/update_third_party_notices.nu records a digest of its inputs in the
file: every third-party package Cargo.lock pins, the configuration under
third-party/licenses, and the generator itself. Regenerating needs cargo-about
and the network; this check needs neither.

`--digest` prints the digest of the current inputs, which is how the generator
obtains it.
"""

import hashlib
import re
import sys
import tomllib
from pathlib import Path

NOTICES = Path("THIRD_PARTY_NOTICES.txt")
CONFIG_DIR = Path("third-party/licenses")
GENERATOR = Path("scripts/update_third_party_notices.nu")
DIGEST_LINE = re.compile(r"^Input digest: sha256:([0-9a-f]{64})$", re.MULTILINE)


def text_bytes(path: Path) -> bytes:
    # A Windows checkout may convert line endings; the digest must not change.
    return path.read_bytes().replace(b"\r\n", b"\n")


def input_digest(root: Path) -> str:
    digest = hashlib.sha256()

    with (root / "Cargo.lock").open("rb") as f:
        packages = tomllib.load(f)["package"]
    # Workspace members carry no source. Their versions change on every release
    # without changing what the notices cover.
    for package in sorted(packages, key=lambda p: (p["name"], p["version"], p.get("source", ""))):
        if "source" not in package:
            continue
        fields = (package["name"], package["version"], package["source"], package.get("checksum", ""))
        digest.update(("\0".join(fields) + "\n").encode())

    files = sorted(p for p in (root / CONFIG_DIR).rglob("*") if p.is_file())
    for path in files + [root / GENERATOR]:
        relative = path.relative_to(root).as_posix()
        digest.update(f"{relative}\0".encode())
        digest.update(hashlib.sha256(text_bytes(path)).hexdigest().encode() + b"\n")

    return digest.hexdigest()


def main() -> int:
    root = Path.cwd()
    current = input_digest(root)

    if sys.argv[1:] == ["--digest"]:
        print(current)
        return 0

    notices = root / NOTICES
    if not notices.is_file():
        print(f"{NOTICES} is missing. Run `nu scripts/update_third_party_notices.nu`.", file=sys.stderr)
        return 1

    recorded = DIGEST_LINE.search(text_bytes(notices).decode("utf-8"))
    if recorded is None:
        print(f"{NOTICES} records no input digest. Run `nu scripts/update_third_party_notices.nu`.", file=sys.stderr)
        return 1

    if recorded.group(1) != current:
        print(
            f"{NOTICES} is stale: Cargo.lock, {CONFIG_DIR} or {GENERATOR} changed since it was generated.\n"
            "Run `nu scripts/update_third_party_notices.nu` and commit the result.",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
