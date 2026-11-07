# Developing

## Prerequisites

### Windows

Everything runs natively; Nix and WSL are not needed. You need:

- [Rust](https://rustup.rs/) (1.98+, from `rust-toolchain.toml`)
- Visual Studio 2022 with the **C++ Clang Compiler for Windows** component.
  `ring` hardcodes clang when targeting wasm32 and `cc` has no MSVC fallback
  there, so `cargo check -p wt-web --target wasm32-unknown-unknown` cannot run
  without it. Standalone LLVM (`winget install -e --id LLVM.LLVM`) works too.
- [mise](https://mise.jdx.dev/), then `mise run setup`. That downloads the
  NASM pinned by `toolchains/windows/toolchain-manifest.json` (rav1e builds its
  AV1 SIMD with it), locates clang and llvm-ar, and writes `.tooling/dev.env`
  with the `CC_wasm32_unknown_unknown` and `AR_wasm32_unknown_unknown` that
  mise loads into every shell.
- Buck needs nothing installed by hand: `bootstrap.ps1` fetches DotSlash and
  the pinned Buck2 (see [Setup](#setup)).
- `mise run install-reindeer`, only if you need to regenerate
  `third-party/rust/BUCK` after changing a dependency.

`mise run setup` exits non-zero and tells you what to install if anything is
missing, so run it first and again after installing.

### Recommended on Linux and macOS: Nix

[Nix](https://nixos.org/download/) is the recommended way to set up a development environment on Linux and macOS. Running `nix develop` gives you everything you need in a single command:

- The exact Rust toolchain version from `rust-toolchain` (with all required components)
- [DepotDownloader](https://github.com/SteamRE/DepotDownloader) for downloading game data (no separate .NET install needed)
- `openssl` and `pkg-config`
- All Linux GUI libraries (X11, Wayland, Vulkan, fontconfig) — these are often the most painful to set up manually

This means you can skip the manual Rust installation, skip the DepotDownloader installation, and skip hunting down system libraries. Just:

```bash
nix develop
cargo build -p wows_toolkit --release
```

On Linux, start the app through `bin/with-host-graphics`. A binary built in the dev shell cannot find your system's graphics drivers on its own. The wrapper adds their directory to `LD_LIBRARY_PATH` for that one command. The dev shell does not add it for everything, because Nix-built tools, `nix` included, break when they load the system's copies of their libraries.

```bash
bin/with-host-graphics target/release/wows_toolkit
```

### Manual setup (without Nix)

If you prefer not to use Nix on Linux or macOS:

- [Rust](https://rustup.rs/) (1.98+)
- [DepotDownloader](https://github.com/SteamRE/DepotDownloader) (only needed for downloading game data; requires .NET)
- `openssl` and `pkg-config` development headers
- On Linux: X11/Wayland/Vulkan/fontconfig development libraries

## Building

```bash
cargo build -p wows_toolkit --release
```

## Buck

Buck2 builds every binary from pinned sources and a pinned toolchain. No Buck action runs Cargo or resolves a tool through PATH, and every download is checked against a pinned digest.

**Cargo is still supported and is fine for day-to-day work.** `cargo build`, `cargo test`, `cargo clippy`, and running against local game data all work as they always have, and Cargo remains the only way to author dependencies. What changed is the gate: **CI builds every alias with Buck, and every release ships from CI.** A change that builds under Cargo but not under Buck does not land. Run the Buck build yourself before pushing anything that touches dependencies, build scripts, or a crate's platform-specific deps, since those are where the two disagree.

Supported target platforms are `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`.

### Setup

One command, once per machine:

```bash
./bootstrap.sh    # macOS and Linux, needs nix
```

```powershell
powershell -ExecutionPolicy Bypass -File bootstrap.ps1    # Windows, from an elevated shell
```

It installs [DotSlash](https://dotslash-cli.com) into `.tools`, fetches the Buck2 release `toolchains/buck2` pins, and provisions the pinned toolchain. It reports any host tool it needs and does not install, such as Nix or git. Re-run it after the toolchain pin moves; it is cheap and skips whatever is already in place.

Run Buck through `bin/buck2` (`bin\buck2.cmd` on Windows), or put `bin` on PATH; `mise` does that for you. `bin/buck2` runs the bootstrap's Buck2 half by itself the first time, so a checkout that only needs Buck2 can skip the bootstrap.

On Windows provisioning needs elevation because the Visual Studio and Windows SDK installers do, and it caches several GB of archives in `%LOCALAPPDATA%\wows-toolkit-buck-offline` by default. Pass `-OfflineRoot` to put them elsewhere.

Provisioning runs the platform toolchain bootstrap (`scripts/refresh-buck-toolchain.nu` or `toolchains/windows/provision-toolchain.ps1`), which writes the machine-local `.buckconfig.local`. Buck fails with a message naming it if `.buckconfig.local` is missing.

### Bumping Buck2

1. Replace `toolchains/buck2` with the DotSlash file from the new release:
   `curl -fsSL https://github.com/facebook/buck2/releases/download/<tag>/buck2 -o toolchains/buck2`
2. Re-vendor the prelude so it matches: `nu scripts/vendor-prelude.nu`
3. Update the `buck2` archive in `toolchains/windows/toolchain-manifest.json`,
   which is what CI's offline Windows toolchain installs from.

`buck2 test //ci:buck2_pins` fails if step 2 or 3 is skipped.

### Crate sources

No crate source is committed. Buck downloads each registry crate and checks it against the SHA-256 that `Cargo.lock` pins for it, and fetches each git dependency at the commit `Cargo.lock` locks it to.

### Building

Builds default to debug. The `release` configuration modifier builds release:

```bash
buck2 build //:wows_toolkit_egui
buck2 build //:wows_toolkit_egui?release
```

`-m release` does the same for every target on the command line, which is the form queries take (`buck2 aquery -m release ...`).

Aliases: `wows_toolkit_egui`, `wows_toolkit_gpui`, `wowsunpack`, `wows_data_mgr`, `replayshark`, `minimap_renderer`, `minimap_renderer_cpu`, `wgcheck`, `dhat_load`, `profile_replay`, `dhat_parse`, `wt_web_site`. `buck2 build root//:` builds all of them.

The alias is not always the shipped filename. Buck names a binary after its Rust crate, and a crate name cannot contain a hyphen, so `//:wows_data_mgr` ships as `wows-data-mgr` (which is the name Cargo produced, and the name it shipped under before the Buck cutover). The desktop app aliases name their front end, so `//:wows_toolkit_egui` builds `wows_toolkit`. `build-support/release-tools.json` holds the CLI tool mapping and `scripts/package-tools.*` checks the finished archive against it.

The host platform is selected automatically. Pass `--target-platforms` only to be explicit:

```bash
buck2 build --target-platforms toolchains//platforms:linux_x86_64 //:wgcheck
```

Platforms are `toolchains//platforms:linux_x86_64`, `:macos_arm64`, and `:windows_x86_64_msvc`.

### The tactics web client

`wt-web` builds for `wasm32-unknown-unknown` whatever platform you ask for, so it needs a Linux or macOS host with the Nix toolchain. `//:wt_web_site` is the directory GitHub Pages serves under `/wows-toolkit/tactics/`:

```bash
buck2 build -m release //:wt_web_site --out /tmp/site/wows-toolkit/tactics
python3 -m http.server -d /tmp/site
```

Then open <http://localhost:8000/wows-toolkit/tactics/>. Asset names carry a hash of their contents, so a deploy never serves a stale module. A debug build skips `wasm-opt`.

### Desktop release channels

Suffixed versions such as `1.1.0-beta1` ship the GPUI frontend. Stable versions
ship egui. The release tag must match `[workspace.package].version` in
`Cargo.toml`. Manual and `ci/**` builds select the frontend from that version.

Both frontends ship as `wows_toolkit.exe` on Windows and `wows_toolkit` on Linux
and macOS. The installer retains the same application identity and replaces the
existing installation. Windows version resources retain the full beta version;
MSI ProductVersion uses the numeric core, such as `1.1.0`. Installers with the
same numeric version replace each other in either direction, including beta
and stable builds.

For a local Buck beta installer, use
`buck2 build -m release -c release.frontend=gpui toolchains//windows:wows_toolkit_msi`.
The Cargo installer script, `scripts/build-msi.ps1`, selects the frontend from
the workspace version and stages it under the installed executable name.

### Remote action cache

Actions can read and write a remote action cache, which is off unless one is configured. `root//platforms:default` turns it on only when `[buck2_re_client]` names an engine address, because CI has no cache and enabling it without one fails every action rather than falling back to local execution. Point it at a cache by writing the section into `~/.buckconfig.d/`:

```ini
[buck2_re_client]
action_cache_address = grpc://cache.example:50051
cas_address = grpc://cache.example:50051
engine_address = grpc://cache.example:50051
tls = false

[buck2]
default_allow_cache_upload = true
```

Buck reads every file in that directory, so renaming one does not disable it; move it out instead. There is no remote execution, only the cache: the platform sets `remote_enabled = False`.

### Hermeticity

The point of the Buck build is that its actions cannot reach outside their declared inputs. This check fails if any action in a target's graph invokes Cargo, reads a cache directory, or names a tool by bare name or by a system path such as `/bin/sh`. It runs over every alias in both modes:

```bash
buck2 run //ci:hermetic
```

`scripts/test-*.nu` are the build-graph tests: toolchain configuration, build-script boundaries, build modes, workspace targets, and the hermeticity check against fixtures that must be rejected. Run them all after touching anything under `toolchains/`, `build-support/buck/`, or `third-party/rust/fixups/`:

```bash
buck2 run //ci:graph_tests
```

### Updating Rust dependencies

Edit `Cargo.toml` and `Cargo.lock` as usual and confirm Cargo is happy, then regenerate the Buck rules:

```bash
nu scripts/update-buck-deps.nu
```

This regenerates `third-party/rust/BUCK` and flattens multi-line values in the generated environment. Commit `Cargo.toml`, `Cargo.lock`, the generated BUCK file, and any fixups together.

It needs `reindeer` and `rustc` on PATH; run `mise run install-reindeer` once. That applies to regeneration only. Building needs neither Nix nor Cargo.

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

The third-party graph resolves features once per platform across the whole workspace, so on `wasm32` it inherits what the desktop crates ask for, not only what `wt-web` needs. When that pulls in something that cannot build for the browser, drop the feature for wasm32 only with a `['cfg(target_arch = "wasm32")']` section holding `omit_features`. See `tokio` and `hyper-util`.

### First-party platform-specific dependencies

Cargo resolves `[target.'cfg(...)'.dependencies]` on its own. Buck does not. A `cfg(windows)` or `cfg(target_os = "macos")` dependency section in a crate's `Cargo.toml` has to be mirrored in its `BUCK` file with `os_select`, or the crate builds everywhere except the platform that needs it:

```python
] + os_select(
    macos = [],
    linux = [],
    windows = ["//third-party/rust:windows-sys"],
)
```

The same applies to features that are only valid on one platform. `crates/minimap-renderer/BUCK` and `crates/wows-toolkit/BUCK` are the examples. A library that `wt-web` reaches carries the features Cargo resolves for `wasm32` through `wasm_select`; see `crates/wowsunpack/BUCK`.

### Re-vendoring the prelude

The vendored prelude only loads under the Buck2 release it was expanded from, so step 2 of [Bumping Buck2](#bumping-buck2) is never optional. `nu scripts/vendor-prelude.nu` expands the prelude bundled in the pinned binary and reapplies the patches this repo depends on: Nix-rooted `bash` in generated shell wrappers, pinned `tar`/`unzip`/`mkdir` and decompressors for archive extraction, and inline linker arguments on Windows. A patch upstream has since made unnecessary is skipped, but only when the result it was written to produce is already present; anything else fails and needs the patch rewritten by hand.

Expect the prelude to have renamed things. Rebuild everything on every platform afterwards, and never hand-edit files under `prelude/`.

### Troubleshooting

**`Access is denied (os error 5)` on Windows, reported as `Binary being executed, please close the process first`.** Antivirus software interfering with a file Buck has newly written. A different file is affected on each run, usually within the first second of a cold build and often several files in one run, and each is an ordinary writable file again shortly afterward. No process is executing the file: the message is a fixed string Buck attaches to any permission error, and the file named is normally a `.json` or `.bat`. Exclude the build output directory, from an elevated shell:

```powershell
Add-MpPreference -ExclusionPath 'G:\dev\wows-toolkit\buck-out'
```

A scanner rather than an ordinary lock is indicated: `DeleteFileW` reports a file that another process holds open as `os error 32`, and deletes a memory-mapped file without error. A filesystem filter driver is not subject to either rule and can reject the deletion directly.

Repeating the build also succeeds, because everything already built is cached, but a cold build continues to encounter the error.

**`Missing [hermetic_tools] <name>`** or **`Missing [nix_toolchain] root`.** `.buckconfig.local` is absent or stale. Re-run the bootstrap.

**`NASM build failed`, or `Unable to run <path>\nasm.exe: The system cannot find the file specified`, from rav1e's build script.** The `[hermetic_tools] nasm` entry names a file that is absent, usually because it referred to a location under `%TEMP%` that Windows has since deleted. Re-running the bootstrap corrects it, or on Windows repair that single entry:

```powershell
.\toolchains\windows\install-nasm.ps1
```

That installs the NASM pinned by the manifest to `.tooling\nasm`, which neither `buck2 clean` nor temporary-file cleanup removes, and rewrites the entry accordingly. It requires a `.buckconfig.local` produced by the bootstrap and terminates with an error without one. Specify `-Force` to reinstall.

**`Spawning executable <path>\python.exe failed`, or `can't find crate for <name>` naming a dependency that builds on its own.** The `[hermetic_tools] python` entry names an interpreter that is absent or unsuitable. A path under `%TEMP%` is removed by Windows cleanup. MSYS2's `mingw64` Python is present but wrong: it runs `rustc_action.py` far enough to invoke rustc, whose `--extern` paths then do not resolve, so the error names the dependency rather than the interpreter. Point the entry at a native CPython, which is what the manifest pins:

```ini
python = C:\Users\<user>\AppData\Local\Programs\Python\Python313\python.exe
```

**`Failed to query capabilities of remote`, or `dns error: No such host is known`.** The cache named by `[buck2_re_client]` is unreachable. Buck treats that as fatal, not as a reason to build locally, so every action fails. Build without it:

```bash
BUCK_OFFLINE_BUILD=1 buck2 build //:wgcheck
```

That sets both `--local-only` and `--no-remote-cache`. Removing the `[buck2_re_client]` file has the same effect permanently.

**`buck2 daemon constraint mismatch`.** The Buck2 binary changed. It restarts itself; no action needed.

## Running Tests

Replay parser tests run against committed fixture replays and require no external data:

```bash
buck2 test root//crates/...
```

Buck runs the same unit and integration tests. Doctests run under Cargo only.

```bash
buck2 test root//crates/...
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

The `game_versions.toml` file at the repo root tracks known game versions and their Steam depot manifest IDs. When a new game version ships, `update-versions` pins it from the Steam public branch:

```bash
cargo run -p wows-data-mgr -- update-versions
```

`dump-renderer-data` runs the same refresh for the build it dumped, so dumping a new build from a local install also records its pins. Pass `--no-manifest-update` to skip it.

Both need the [`steamroom`](https://github.com/landaire/steamroom) CLI on PATH:

```bash
cargo install steamroom-cli
```

The depot manifest IDs come from public app info, but reading the build number back out of the client depot needs a Steam account that owns the game. It reuses the account in `.steam-user`. Without that verification nothing is written, since the client depot also retains the previous build's directory and pinning that one would file the current release's manifest IDs under the previous release's build number.

Failing that, entries can still be written by hand from [SteamDB](https://steamdb.info/app/552990/depots/).

#### Client scripts

`dump-renderer-data` also stores the build's client `scripts.zip` (`bin/<build>/res/scripts.zip` in the install) at `<output>/scripts/<build>.zip`. Nothing is uploaded unless you ask. `--publish-scripts` uploads it as `<build>.zip` of the [`scripts` release](https://github.com/landaire/wows-replay-data/releases/tag/scripts) of wows-replay-data with [`gh`](https://cli.github.com/) and updates the release's `index.toml`. `publish-scripts --output <dir>` uploads every stored build the release does not already hold unchanged. `extract-scripts` stores the scripts of a build that is already dumped:

```bash
cargo run -p wows-data-mgr -- extract-scripts --build 13533378 --game-dir /path/to/World_of_Warships --output /path/to/dumps --publish-scripts
```

The client obfuscates its bytecode differently in every build, so no file is shared between builds and each zip is kept whole. Programs that need scripts fetch them per build with `wows_data_mgr::scripts::fetch`. Downloading a dump does not fetch them.

#### Environment variable

Set `WOWS_GAME_DATA` to override the default `game_data/` directory:

```bash
WOWS_GAME_DATA=/path/to/game_data cargo test --workspace
```

Buck tests see game data only when you name it, as an absolute path:

```bash
bin/buck2 test //crates/wows-query/... -c wows.game_data=/path/to/game_data
```

To keep it on one machine, add it to `.buckconfig.local`:

```ini
[wows]
game_data = /path/to/game_data
```

The build scripts then read the dump, so those actions are no longer hermetic. Buck reruns them when the path changes but not when the dump's contents change. Run `bin/buck2 clean` after you replace the dump in place.

## Test Fixtures

Replay fixtures live in `tests/fixtures/replays/` and are committed to the repo. They span multiple game versions (12.3 through 15.1) and ship types (DD, CA, BB, SS, CV) to provide broad parser coverage.

To add a new fixture, drop a `.wowsreplay` file into the directory and add a corresponding test in `crates/wows-replays/tests/replay_parsing.rs`.

## CI

CI runs only buck2 commands (`.github/workflows/buck.yml`), after `./bootstrap.sh` sets up Buck2 and the toolchain:

- `buck2 build root//:` and `buck2 build -m release root//:`: every alias in both modes, including the `wt-web` site.
- `buck2 run //ci:hermetic`: the hermeticity check over every alias.
- `buck2 run //ci:lint`: clippy and rustfmt over every first-party Rust target, and clippy over `wt-web` and the crates it uses as built for `wasm32`.
- `buck2 test root//crates/... root//ci/...`: the crates' tests, plus the checks that the Buck2 pins agree and that no publishable crate depends on an unpublished one.
- `buck2 run //ci:graph_tests`: the build-graph tests.

Each run checks only the first-party targets a change affects. `buck2 run //ci:affected -- --base <commit>` compares every first-party target's recursive target hash against the last successful run on `main`. The hash covers the target's sources, attributes and dependencies, so a target is selected when its own code changes or when a crate or toolchain it depends on does. Third-party targets are never checked directly. A change to Buck configuration, `.bzl` rules, toolchains, the flake, or the checks themselves (`ci/`, `scripts/`) checks everything, because no target hash covers those. So does a manual run of the workflow. The build-graph tests run when everything is checked or a `BUCK` file or manifest changed. The `root//ci/...` checks always run.

Windows is not one of those jobs: provisioning mirrors a multi-gigabyte Visual Studio layout and nothing caches it yet, so Windows-only graph breakage surfaces in `build.yml` at tag time instead.

`.github/workflows/pages.yml` deploys `//crates/wt-web:site` with the rest of `site/` on each stable tag.

Release builds (`.github/workflows/build.yml`) run on GitHub release creation and produce:

- **Windows**: Signed `.exe` + `.pdb` in a zip
- **Linux**: Flatpak bundle
- **macOS**: Apple Silicon (aarch64) binary in a `.dmg`
