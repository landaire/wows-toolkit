"""Populate third-party/rust/vendor from Cargo.lock.

The Buck build reads crate sources from `third-party/rust/vendor/*.crate`. Those
are not committed: every registry crate is pinned by version and SHA-256 in
Cargo.lock, so fetching them is reproducible and the archives are verified
against the lock rather than trusted.

Crates that do not come from a registry have no checksum to verify against and
cannot be reconstructed from the lock, so those stay committed. This script
fails if one is missing rather than fetching something unverifiable.

Runs on the interpreter pinned by the platform bootstrap; stdlib only.
"""

import concurrent.futures
import hashlib
import os
import re
import sys
import urllib.error
import urllib.request

REGISTRY_URL = "https://static.crates.io/crates/{name}/{name}-{version}.crate"
RETRIES = 3

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOCK = os.path.join(REPO, "Cargo.lock")
VENDOR = os.path.join(REPO, "third-party", "rust", "vendor")


class Package:
    def __init__(self, name, version, source, checksum):
        self.name = name
        self.version = version
        self.source = source
        self.checksum = checksum

    @property
    def filename(self):
        return "{}-{}.crate".format(self.name, self.version)

    @property
    def path(self):
        return os.path.join(VENDOR, self.filename)


def read_lock():
    """Split Cargo.lock into the packages Buck needs a .crate archive for.

    A package with no source is a workspace member and is built from the tree.
    """
    text = open(LOCK, encoding="utf-8").read()
    registry, unverifiable = [], []
    for block in text.split("[[package]]")[1:]:
        name = re.search(r'^name = "([^"]+)"', block, re.M)
        version = re.search(r'^version = "([^"]+)"', block, re.M)
        source = re.search(r'^source = "([^"]+)"', block, re.M)
        checksum = re.search(r'^checksum = "([0-9a-f]{64})"', block, re.M)
        if not (name and version and source):
            continue
        package = Package(
            name.group(1),
            version.group(1),
            source.group(1),
            checksum.group(1) if checksum else None,
        )
        (registry if package.checksum else unverifiable).append(package)
    return registry, unverifiable


def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fetch(package):
    """Return None once the archive is present and matches the lock."""
    if os.path.exists(package.path) and digest(package.path) == package.checksum:
        return None

    url = REGISTRY_URL.format(name=package.name, version=package.version)
    for attempt in range(RETRIES):
        try:
            with urllib.request.urlopen(url, timeout=120) as response:
                data = response.read()
        except (urllib.error.URLError, TimeoutError) as err:
            if attempt == RETRIES - 1:
                return "{}: {}".format(package.filename, err)
            continue

        got = hashlib.sha256(data).hexdigest()
        if got != package.checksum:
            return "{}: sha256 {} does not match Cargo.lock {}".format(
                package.filename, got, package.checksum
            )

        # Write through a temporary so an interrupted run cannot leave a
        # truncated archive that later looks like a checksum mismatch.
        temp = package.path + ".part"
        with open(temp, "wb") as f:
            f.write(data)
        os.replace(temp, package.path)
        return None
    return None


def main():
    registry, unverifiable = read_lock()
    os.makedirs(VENDOR, exist_ok=True)

    missing = [p for p in unverifiable if not os.path.exists(p.path)]
    if missing:
        sys.stderr.write(
            "Not fetchable and not committed:\n"
            + "".join("  {}  ({})\n".format(p.filename, p.source) for p in missing)
            + "Cargo.lock records no checksum for these, so they cannot be verified.\n"
            "Run `nu scripts/update-buck-deps.nu` and commit the resulting archives.\n"
        )
        return 1

    errors = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
        for i, error in enumerate(pool.map(fetch, registry), 1):
            if error:
                errors.append(error)
            if i % 100 == 0 or i == len(registry):
                print("{}/{}".format(i, len(registry)), flush=True)

    if errors:
        sys.stderr.write("Failed to vendor {} crates:\n".format(len(errors)))
        sys.stderr.writelines("  " + e + "\n" for e in errors)
        return 1

    # A crate left behind by an older lock is not referenced by the build graph,
    # but it does make the tree diverge from Cargo.lock.
    expected = {p.filename for p in registry} | {p.filename for p in unverifiable}
    for stale in sorted(set(os.listdir(VENDOR)) - expected - {"index"}):
        if stale.endswith(".crate") or stale.endswith(".part"):
            os.remove(os.path.join(VENDOR, stale))
            print("removed stale " + stale)

    print("{} crates vendored, {} committed".format(len(registry), len(unverifiable)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
