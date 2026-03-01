# Developing

## Prerequisites

- [Rust](https://rustup.rs/) (1.92+)
- [Nix](https://nixos.org/download/) (optional, for reproducible builds)

## Building

```bash
cargo build -p wows_toolkit --release
```

Or with Nix:

```bash
nix develop
cargo build -p wows_toolkit --release
```

## Running Tests

Replay parser tests run against committed fixture replays and require no external data:

```bash
cargo test --workspace
```

### Game Data Tests

Some tests exercise game file parsing (VFS, PKG, MFM, GameParams) and require a local copy of World of Warships. These tests are skipped when game data is not available.

#### Using `wows-game-data-dl`

The `wows-game-data-dl` CLI tool manages game data downloads and version tracking. Install [DepotDownloader](https://github.com/SteamRE/DepotDownloader) first:

```bash
dotnet tool install -g DepotDownloader
```

Then download the latest game version:

```bash
cargo run -p wows-game-data-dl -- download --latest
```

Or register an existing WoWs installation (no download needed):

```bash
cargo run -p wows-game-data-dl -- register --path /path/to/World_of_Warships
```

List known versions and their availability:

```bash
cargo run -p wows-game-data-dl -- list
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
- **Linux**: AppImage
- **macOS**: Universal binary (aarch64 + x86_64) in a `.dmg`
