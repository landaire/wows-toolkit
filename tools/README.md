# Pinned tools

`buck2` here is the DotSlash file published by the Buck2 release the vendored
prelude was expanded from (see `prelude/VENDORED_FROM`). DotSlash downloads and
verifies the right binary for the host platform on first use and caches it, so
the prelude and the binary cannot drift apart, and no one has to install Buck2
by hand.

`mise.toml` puts this directory on PATH, so `buck2 ...` works once DotSlash
itself is installed:

    winget install -e --id Facebook.DotSlash   # Windows
    brew install dotslash                      # macOS
    cargo install dotslash                     # anywhere

Windows has no shebang, so `buck2.bat` sits beside the DotSlash file and
invokes it. `%~dpn0` strips the `.bat`, which is what makes the pair work.

## Bumping Buck2

1. Replace `tools/buck2` with the DotSlash file from the new release:
   `curl -fsSL https://github.com/facebook/buck2/releases/download/<tag>/buck2 -o tools/buck2`
2. Re-vendor the prelude so it matches: `nu scripts/vendor-prelude.nu`
3. Update the `buck2` archive in `toolchains/windows/toolchain-manifest.json`,
   which is what CI's offline Windows toolchain installs from.

`build-support/buck/check-prelude-version.sh` fails the build if step 2 is
skipped.
