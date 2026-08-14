#!/usr/bin/env bash
# One-time setup for building with Buck on macOS and Linux.
#
# Everything after this is offline: the toolchain and every crate source are
# pinned, and no Buck action downloads anything.
#
# Windows: run setup.ps1 instead.
set -euo pipefail

cd "$(dirname "$0")"

missing=0

if ! command -v nix >/dev/null 2>&1; then
    echo "nix is required and was not found." >&2
    echo "  Install it from https://nixos.org/download/, then re-run this script." >&2
    missing=1
fi

# Everything else comes from the flake, so nix is the only hard prerequisite.
if [ "$missing" -ne 0 ]; then
    exit 1
fi

echo "==> Toolchain"
nix develop --command nu scripts/refresh-buck-toolchain.nu

echo
echo "Setup complete. Build with:"
echo "  buck2 build //:wows_toolkit"
