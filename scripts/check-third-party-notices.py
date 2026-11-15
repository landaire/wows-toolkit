#!/usr/bin/env python3
"""Fails when THIRD_PARTY_NOTICES.txt is stale.

scripts/update_third_party_notices.nu records a digest of its inputs in the
file: every third-party package Cargo.lock pins, the Cargo manifests that
select their features, the configuration under third-party/licenses, the
component license files notices.toml reads from this repository, and the
generator itself. Regenerating needs cargo-about and the network; this check
needs neither.

`--digest` prints the digest of the current inputs, which is how the generator
obtains it.
"""

import hashlib
import json
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


def manifest_bytes(path: Path) -> bytes:
    # Releases bump the versions alone; they do not change the dependency graph.
    with path.open("rb") as f:
        manifest = tomllib.load(f)
    manifest.get("package", {}).pop("version", None)
    manifest.get("workspace", {}).get("package", {}).pop("version", None)
    return json.dumps(manifest, sort_keys=True).encode()


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

    with (root / "Cargo.toml").open("rb") as f:
        members = tomllib.load(f)["workspace"]["members"]
    manifests = [root / "Cargo.toml"] + sorted(p / "Cargo.toml" for m in members for p in root.glob(m))
    for path in manifests:
        digest.update(f"{path.relative_to(root).as_posix()}\0".encode())
        digest.update(hashlib.sha256(manifest_bytes(path)).hexdigest().encode() + b"\n")

    with (root / CONFIG_DIR / "notices.toml").open("rb") as f:
        components = tomllib.load(f).get("component", [])
    component_files = sorted({root / f["path"] for c in components for f in c["files"] if "crate" not in f})

    files = sorted(p for p in (root / CONFIG_DIR).rglob("*") if p.is_file())
    for path in files + component_files + [root / GENERATOR]:
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
            f"{NOTICES} is stale: its inputs changed since it was generated.\n"
            "Run `nu scripts/update_third_party_notices.nu` and commit the result.",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
