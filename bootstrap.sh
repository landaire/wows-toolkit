#!/bin/sh
# Sets up a Linux or macOS machine to build with Buck2: installs DotSlash into
# .tools, fetches the Buck2 release that toolchains/buck2 pins, and provisions
# the pinned toolchain through Nix. Windows uses bootstrap.ps1.
#
# bin/buck2 runs this with --buck2-only on first use, which stops after Buck2.
# Re-run it after the toolchain pin moves; it skips whatever is in place.
set -eu

DOTSLASH_VERSION=v0.5.9

root=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
tools=$root/.tools

buck2_only=
for arg in "$@"; do
    case $arg in
    --buck2-only) buck2_only=1 ;;
    *)
        printf 'bootstrap: unknown argument %s\n' "$arg" >&2
        exit 1
        ;;
    esac
done

say() {
    printf '%s\n' "$*" >&2
}

fail() {
    say "bootstrap: $*"
    exit 1
}

have() {
    command -v "$1" >/dev/null 2>&1
}

sha256_of() {
    if have sha256sum; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

install_dotslash() {
    for tool in curl tar; do
        have "$tool" || fail "$tool is required to download DotSlash"
    done

    mkdir -p "$tools/bin"
    scratch=$(mktemp -d "$tools/download.XXXXXX")
    trap 'rm -rf "$scratch"' EXIT

    say "Downloading DotSlash $DOTSLASH_VERSION"
    curl -fsSL --retry 3 -o "$scratch/$asset" \
        "https://github.com/facebook/dotslash/releases/download/$DOTSLASH_VERSION/$asset"

    actual=$(sha256_of "$scratch/$asset")
    [ "$actual" = "$sha256" ] || fail "$asset has SHA-256 $actual, not $sha256"

    tar -xzf "$scratch/$asset" -C "$scratch" dotslash
    mv -f "$scratch/dotslash" "$tools/bin/dotslash"
    printf '%s\n' "$sha256" >"$tools/bin/dotslash.sha256"
}

missing_required() {
    if [ -z "${required_reported:-}" ]; then
        say "Missing required host tools:"
        required_reported=1
    fi

    required_missing=1
    say "  $1: $2"
}

missing_optional() {
    if [ -z "${optional_reported:-}" ]; then
        say "Optional capabilities unavailable:"
        optional_reported=1
    fi

    say "  $1: $2"
}

check_host_tools() {
    have nix || missing_required nix "required. The pinned toolchain comes from the flake. Install it from https://nixos.org/download/."
    have git || missing_required git "required. Buck2 fetches git dependencies with it."

    have reindeer ||
        missing_optional reindeer "regenerating third-party/rust/BUCK after a dependency change needs it. Run mise run install-reindeer."
    have cargo ||
        missing_optional cargo "the wasm32 check of wt-web needs it. The Nix devShell provides it."

    if [ -n "${required_missing:-}" ]; then
        fail "install the required host tools above and run bootstrap.sh again"
    fi
}

case "$(uname -s)-$(uname -m)" in
Linux-x86_64)
    asset=dotslash-linux-musl.x86_64.$DOTSLASH_VERSION.tar.gz
    sha256=4c75c6eb7890ae35993b962073f6d9bbe78b42b81a5691303ad70f63bfbf7196
    ;;
Linux-aarch64 | Linux-arm64)
    asset=dotslash-linux-musl.arm64.$DOTSLASH_VERSION.tar.gz
    sha256=e58374446dacd4a228388cd275d974bbd8f946916b513cf0eb86bdff5d9d675e
    ;;
Darwin-x86_64 | Darwin-arm64)
    asset=dotslash-macos.$DOTSLASH_VERSION.tar.gz
    sha256=67963df97f5ce512f900a85950add6e883722c3460ee5d752a6e62665fd2f986
    ;;
*)
    fail "no DotSlash release for $(uname -s) $(uname -m)"
    ;;
esac

installed=$(cat "$tools/bin/dotslash.sha256" 2>/dev/null || true)
if [ ! -x "$tools/bin/dotslash" ] || [ "$installed" != "$sha256" ]; then
    install_dotslash
fi

say "Fetching Buck2"
DOTSLASH_CACHE=$tools/dotslash "$tools/bin/dotslash" "$root/toolchains/buck2" --version >&2

if [ -n "$buck2_only" ]; then
    exit 0
fi

check_host_tools

say "Provisioning the toolchain"
cd "$root"
# Run from inside a devShell, LD_LIBRARY_PATH can carry the host's driver
# directory, and nix crashes loading host libraries from it.
env -u LD_LIBRARY_PATH nix develop --command nu scripts/refresh-buck-toolchain.nu

say "Run bin/buck2 in place of buck2, or add $root/bin to PATH. Then:"
say "  bin/buck2 build //:wows_toolkit"
