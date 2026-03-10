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

Release builds (`.github/workflows/build.yml`) run on GitHub release creation and produce:

- **Windows**: Signed `.exe` + `.pdb` in a zip
- **Linux**: Flatpak bundle
- **macOS**: Universal binary (aarch64 + x86_64) in a `.dmg`
