#!/usr/bin/env bash
# Verify the vendored Buck2 prelude was expanded from the pinned buck2.
#
# The hermetic build breaks when the prelude and the buck2 binary disagree: a
# prelude newer than the binary uses rule parameters the binary rejects at load
# time. scripts/vendor-prelude.nu records the exact buck2 the prelude was
# expanded from in prelude/VENDORED_FROM; this asserts that matches the pinned
# buck2 (buck2Pinned in flake.nix). It fails when buck2 is bumped without
# re-vendoring.
#
# Usage: check-prelude-version.sh BUCK2_VERSION_LINE VENDORED_FROM
#
# BUCK2_VERSION_LINE  the `buck2 --version` output, e.g. "buck2 2026-07-31-<hash>"
# VENDORED_FROM       path to prelude/VENDORED_FROM
set -euo pipefail

version_line=$1
vendored_from=$2

# `buck2 --version` prints "buck2 <version>"; VENDORED_FROM records "<version>".
version=${version_line#buck2 }
if [ "$version" = "$version_line" ] || [ -z "$version" ]; then
    echo "unexpected buck2 --version output: $version_line" >&2
    exit 1
fi

if ! grep -qF -- "$version" "$vendored_from"; then
    echo "prelude/VENDORED_FROM does not record buck2 $version" >&2
    echo "The vendored prelude was expanded from a different buck2 than the pinned one." >&2
    echo "Re-vendor with: nu scripts/vendor-prelude.nu" >&2
    exit 1
fi
echo "vendored prelude matches pinned buck2 $version"
