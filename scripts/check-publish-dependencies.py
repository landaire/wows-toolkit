#!/usr/bin/env python3

import json
import subprocess
import sys
from pathlib import Path


def main() -> int:
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
    )
    packages = json.loads(result.stdout)["packages"]
    by_manifest = {Path(package["manifest_path"]).resolve(): package for package in packages}
    private = {
        Path(package["manifest_path"]).resolve()
        for package in packages
        if package.get("publish") is False or package.get("publish") == []
    }

    violations = []
    for package in packages:
        if Path(package["manifest_path"]).resolve() in private:
            continue
        for dependency in package["dependencies"]:
            path = dependency.get("path")
            if path is None:
                continue
            manifest = (Path(path) / "Cargo.toml").resolve()
            target = by_manifest.get(manifest)
            if target is not None and manifest in private:
                violations.append((package["name"], target["name"]))

    if violations:
        for source, target in violations:
            print(f"{source} depends on unpublished workspace crate {target}", file=sys.stderr)
        return 1

    print("No publishable workspace crate depends on an unpublished workspace crate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
