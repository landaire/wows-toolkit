# Developing

## Prerequisites

### Recommended: Nix

[Nix](https://nixos.org/download/) is the recommended way to set up a development environment (even on Windows if you're going to be touching `wows-data-mgr`). Running `nix develop` gives you everything you need in a single command:

- The exact Rust toolchain version from `rust-toolchain` (with all required components)
- [DepotDownloader](https://github.com/SteamRE/DepotDownloader) for downloading game data (no separate .NET install needed)
- `openssl` and `pkg-config`
- All Linux GUI libraries (X11, Wayland, Vulkan, fontconfig) — these are often the most painful to set up manually

This means you can skip the manual Rust installation, skip the DepotDownloader installation, and skip hunting down system libraries. Just:

```bash
nix develop
cargo build -p wows_toolkit --release
```

### Manual setup (without Nix)

If you prefer not to use Nix:

- [Rust](https://rustup.rs/) (1.92+)
- [DepotDownloader](https://github.com/SteamRE/DepotDownloader) (only needed for downloading game data; requires .NET)
- `openssl` and `pkg-config` development headers
- On Linux: X11/Wayland/Vulkan/fontconfig development libraries

## Building

```bash
cargo build -p wows_toolkit --release
```

## Buck

Buck2 builds every binary from pinned sources and a pinned toolchain. No Buck action runs Cargo, downloads anything, or resolves a tool through PATH.

**Cargo is still supported and is fine for day-to-day work.** `cargo build`, `cargo test`, `cargo clippy`, and running against local game data all work as they always have, and Cargo remains the only way to author dependencies. What changed is the gate: **CI builds every alias with Buck, and every release ships from CI.** A change that builds under Cargo but not under Buck does not land. Run the Buck build yourself before pushing anything that touches dependencies, build scripts, or a crate's platform-specific deps, since those are where the two disagree.

Supported target platforms are `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`.

### Setup

One command, once per machine:

```bash
./setup.sh     # macOS and Linux, needs nix
```

```powershell
.\setup.ps1    # Windows, from an elevated shell
```

It provisions the pinned toolchain and fetches the pinned crate sources. Everything after it is offline. Re-run it after the toolchain pin moves or `Cargo.lock` changes; it is cheap and skips whatever is already in place.

On Windows it needs elevation because the Visual Studio and Windows SDK installers do, and it caches several GB of archives in `%LOCALAPPDATA%\wows-toolkit-buck-offline` by default. Pass `-OfflineRoot` to put them elsewhere.

Under the hood setup runs the platform bootstrap (`scripts/refresh-buck-toolchain.nu` or `toolchains/windows/provision-toolchain.ps1`), which writes the machine-local `.buckconfig.local` and then runs `scripts/fetch-buck-deps.py`. You can run those directly; setup exists so you do not have to know which. Buck fails with a message naming the bootstrap if `.buckconfig.local` is missing.

### Crate sources

`third-party/rust/vendor/*.crate` is not committed. Every registry crate is pinned by version and SHA-256 in `Cargo.lock`, so `scripts/fetch-buck-deps.py` downloads them and verifies each against the lock. A cold fetch is about 150 MB and takes seconds; after that the build is offline.

Archives that the lock cannot verify are committed instead, because there is nothing to check a download against. Right now that is one git dependency. If you add another, the fetch script fails and names the file to commit.

### Building

Builds default to debug.

```bash
buck2 build //:wows_toolkit
buck2 build -c native_build.mode=release //:wows_toolkit
```

Aliases: `wows_toolkit`, `wowsunpack`, `wows_data_mgr`, `replayshark`, `minimap_renderer`, `wgcheck`, `dhat_load`, `profile_replay`, `dhat_parse`.

The host platform is selected automatically. Pass `--target-platforms` only to be explicit:

```bash
buck2 build --target-platforms toolchains//platforms:linux_x86_64 //:wgcheck
```

Platforms are `toolchains//platforms:linux_x86_64`, `:macos_arm64`, and `:windows_x86_64_msvc`.

### Hermeticity

The point of the Buck build is that its actions cannot reach outside their declared inputs. This check fails if any action in a target's graph invokes Cargo, downloads, reads a cache directory, or names a tool by bare name or by a system path such as `/bin/sh`:

```bash
nu scripts/check-buck-hermetic.nu //:wows_toolkit
nu scripts/check-buck-hermetic.nu //:wows_toolkit release
```

`scripts/test-*.nu` are the build-graph tests: toolchain configuration, build-script boundaries, build modes, workspace targets, and the hermeticity check against fixtures that must be rejected. Run them all after touching anything under `toolchains/`, `build-support/buck/`, or `third-party/rust/fixups/`:

```bash
for t in scripts/test-*.nu; do nu "$t"; done
```

### Updating Rust dependencies

Edit `Cargo.toml` and `Cargo.lock` as usual and confirm Cargo is happy, then regenerate the Buck rules:

```bash
nu scripts/update-buck-deps.nu
```

This vendors every crate source, regenerates `third-party/rust/BUCK`, flattens multi-line values in the generated environment, and fails if Reindeer emitted a network download rule. Commit `Cargo.toml`, `Cargo.lock`, the generated BUCK file, and any fixups together. The vendored `.crate` archives are not committed; other people get them from `Cargo.lock` on their next setup.

It needs Nix and only runs on macOS or Linux; on Windows use WSL2. That applies to regeneration only. Building needs neither Nix nor Cargo.

Build the affected targets with Buck before pushing. A new dependency that pulls in a build script or native code usually needs a fixup, and CI will not let it through without one.

Do not hand-edit `third-party/rust/BUCK`. Everything in it comes from `reindeer.toml` and the fixups, and the next regeneration discards anything else.

### Fixups

A crate needs a fixup in `third-party/rust/fixups/<crate>/fixups.toml` when its build script does something a hermetic action cannot. In rough order of preference:

- The build script reads host state (probes pkg-config, searches for a library, inspects the compiler). Set `buildscript.run = false` and declare what it would have produced. For generated sources, check in the file and point `OUT_DIR` at a `filegroup`; `x11-dl` and `libsqlite3-sys` are the worked examples.
- The crate compiles C or C++. Declare it as `[[cxx_library]]` with explicit `srcs`, `headers`, and `include_paths` rather than letting the build script drive `cc`. See `vk-mem`.
- The build script only emits link flags for code it built. Keep it running and add `rustc_link_lib` / `rustc_link_search`. See `rav1e` and `openh264-sys2`.

Two things that will bite:

- A `cfg(...)` section applies per crate version. `windows_x86_64_msvc` vendors four versions and each needs its own entry.
- Two `cxx_library` entries under different `cfg(...)` predicates must have different `name` values. Reindeer merges same-named entries and the flags from both end up on one rule.

A build script's `rustc_link_search` does not reach the final link under Buck, only that crate's own compile. If a crate links a bundled import library by bare name, the search path will not be enough; `windows-targets` is handled by building it with `windows_raw_dylib` instead.

### First-party platform-specific dependencies

Cargo resolves `[target.'cfg(...)'.dependencies]` on its own. Buck does not. A `cfg(windows)` or `cfg(target_os = "macos")` dependency section in a crate's `Cargo.toml` has to be mirrored in its `BUCK` file with `os_select`, or the crate builds everywhere except the platform that needs it:

```python
] + os_select(
    macos = [],
    linux = [],
    windows = ["//third-party/rust:windows-sys"],
)
```

The same applies to features that are only valid on one platform. `crates/minimap-renderer/BUCK` and `crates/wows-toolkit/BUCK` are the examples.

### Updating the Buck2 pin

Buck2 is pinned to a specific release in three places that must agree, because the vendored prelude only loads under the release it was expanded from:

- `buck2Release` in `flake.nix` (macOS and Linux; fetched from the release, not from nixpkgs, which tracks a different one)
- the `buck2` entry in `toolchains/windows/toolchain-manifest.json`
- the release recorded in `prelude/VENDORED_FROM`

To move the pin: update the URLs and SHA-256s in the first two, install the new binary, then re-vendor the prelude against it:

```bash
nu scripts/vendor-prelude.nu
```

That expands the prelude bundled in the pinned binary and reapplies the patches this repo depends on: Nix-rooted `bash` in generated shell wrappers, pinned `tar`/`unzip`/`mkdir` and decompressors for archive extraction, and inline linker arguments on Windows. A patch upstream has since made unnecessary is skipped, but only when the result it was written to produce is already present; anything else fails and needs the patch rewritten by hand.

Expect the prelude to have renamed things. Rebuild everything on every platform afterwards, and never hand-edit files under `prelude/`.

### Troubleshooting

**`Access is denied (os error 5)` on Windows, reported as `Binary being executed, please close the process first`.** Antivirus holding a lock on a file Buck just wrote. It hits a different file each time, usually in the first second of a cold build, and the file is unlocked again moments later. Nothing is executing. Exclude the build output, from an elevated shell:

```powershell
Add-MpPreference -ExclusionPath 'G:\dev\wows-toolkit\buck-out'
```

Retrying also works, since everything already built is cached, but a cold build will keep tripping it.

**`Missing [hermetic_tools] <name>`** or **`Missing [nix_toolchain] root`.** `.buckconfig.local` is absent or stale. Re-run the bootstrap.

**`buck2 daemon constraint mismatch`.** The Buck2 binary changed. It restarts itself; no action needed.

## Running Tests

Replay parser tests run against committed fixture replays and require no external data:

```bash
cargo test --workspace
```

### Game Data Tests

Some tests exercise game file parsing (VFS, PKG, MFM, GameParams) and require a local copy of World of Warships. These tests are skipped when game data is not available.

#### Using `wows-data-mgr`

The `wows-data-mgr` CLI tool manages game data downloads and version tracking.

**If using Nix**, DepotDownloader is already available — skip straight to the download command.

**Without Nix**, install [DepotDownloader](https://github.com/SteamRE/DepotDownloader) first:

```bash
dotnet tool install -g DepotDownloader
```

Then download the latest game version:

```bash
cargo run -p wows-data-mgr -- download --latest
```

Or register an existing WoWs installation (no download needed):

```bash
cargo run -p wows-data-mgr -- register --path /path/to/World_of_Warships
```

List known versions and their availability:

```bash
cargo run -p wows-data-mgr -- list
```

The tool saves your Steam username to `.steam-user` (gitignored) and uses DepotDownloader's `-remember-password` flag so subsequent runs are non-interactive.

#### Known versions

The `game_versions.toml` file at the repo root tracks known game versions and their Steam depot manifest IDs. When a new game version ships, add an entry with the manifest ID from [SteamDB](https://steamdb.info/app/552990/depots/).

#### Environment variable

Set `WOWS_GAME_DATA` to override the default `game_data/` directory:

```bash
WOWS_GAME_DATA=/path/to/game_data cargo test --workspace
```

## Test Fixtures

Replay fixtures live in `tests/fixtures/replays/` and are committed to the repo. They span multiple game versions (12.3 through 15.1) and ship types (DD, CA, BB, SS, CV) to provide broad parser coverage.

To add a new fixture, drop a `.wowsreplay` file into the directory and add a corresponding test in `crates/wows-replays/tests/replay_parsing.rs`.

## CI

The CI pipeline (`.github/workflows/rust.yml`) runs on every push and PR:

- **Check**: `cargo check --workspace --all-features`
- **Rustfmt**: `cargo fmt --all -- --check`
- **Clippy**: `cargo clippy --workspace --all-features -- -D warnings`
- **Test**: `cargo test --workspace`

`.github/workflows/buck.yml` builds every alias through Buck on Linux, macOS, and Windows. Each job provisions only its pinned toolchain, drops network access before the first Buck action, builds every alias, and runs the hermeticity check. The Windows job publishes the unsigned `.exe`, `.pdb`, and `.msi`; signing is a separate workflow that consumes those artifacts and cannot rebuild them.

Release builds (`.github/workflows/build.yml`) run on GitHub release creation and produce:

- **Windows**: Signed `.exe` + `.pdb` in a zip
- **Linux**: Flatpak bundle
- **macOS**: Apple Silicon (aarch64) binary in a `.dmg`
