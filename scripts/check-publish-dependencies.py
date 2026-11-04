#!/usr/bin/env python3
"""Fails when a publishable workspace crate depends on an unpublished one.

crates.io rejects such a crate at publish time, long after the dependency was
added. Reads the manifests directly, so it needs no Cargo.
"""

import sys
import tomllib
from pathlib import Path

DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def load(path: Path) -> dict:
    with path.open("rb") as f:
        return tomllib.load(f)


def dependency_tables(manifest: dict):
    for table in DEPENDENCY_TABLES:
        yield manifest.get(table, {})
    for target in manifest.get("target", {}).values():
        for table in DEPENDENCY_TABLES:
            yield target.get(table, {})


def main() -> int:
    root = Path.cwd()
    workspace = load(root / "Cargo.toml")["workspace"]
    shared = workspace.get("dependencies", {})

    members = {}
    for member in workspace["members"]:
        manifest_path = (root / member / "Cargo.toml").resolve()
        members[manifest_path.parent] = load(manifest_path)

    def is_private(manifest: dict) -> bool:
        publish = manifest.get("package", {}).get("publish")
        return publish is False or publish == []

    violations = []
    for directory, manifest in members.items():
        if is_private(manifest):
            continue
        for table in dependency_tables(manifest):
            for name, spec in table.items():
                if not isinstance(spec, dict):
                    continue
                if spec.get("workspace"):
                    shared_spec = shared.get(name, {})
                    path = shared_spec.get("path") if isinstance(shared_spec, dict) else None
                    base = root
                else:
                    path = spec.get("path")
                    base = directory
                if path is None:
                    continue
                target = members.get((base / path).resolve())
                if target is not None and is_private(target):
                    violations.append((manifest["package"]["name"], target["package"]["name"]))

    if violations:
        for source, target in sorted(set(violations)):
            print(f"{source} depends on unpublished workspace crate {target}", file=sys.stderr)
        return 1

    print("No publishable workspace crate depends on an unpublished workspace crate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
