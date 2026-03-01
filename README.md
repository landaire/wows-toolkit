# WoWs Toolkit

A monorepo of tools for interacting with World of Warships game data, replays, and assets.

<p>
  <img src="assets/replay_inspector.png" alt="Replay Inspector" width="800">
</p>
<p>
  <img src="assets/armor_viewer.png" alt="Armor Viewer" width="800">
</p>
<p>
  <img src="assets/armor_trajectory.png" alt="Ballistic Trajectory Analysis" width="800">
</p>
<p>
  <img src="assets/replay_renderer.png" alt="Replay Renderer" width="500">
</p>
<p>
  <img src="assets/resource_browser.png" alt="Resource Browser" width="800">
</p>

## Crates

| Crate | Description | CLI Binary |
|-------|-------------|------------|
| [`wows-toolkit`](crates/wows-toolkit) | GUI application for browsing replays, extracting game files, and viewing armor models | `wows_toolkit` |
| [`wowsunpack`](crates/wowsunpack) | Library and CLI for unpacking World of Warships game assets (IDX/PKG files, GameParams) | `wowsunpack` |
| [`wows-replays`](crates/wows-replays) | Core replay file parser library (`wows_replays`) | - |
| [`minimap-renderer`](crates/minimap-renderer) | Library and CLI for rendering replay minimaps as images or video (`wows_minimap_renderer`) | `minimap_renderer` |
| [`replayshark`](crates/replayshark) | CLI tool for dumping and analyzing replay files | `replayshark` |

## Documentation

Reverse engineering notes and format specifications live in [`docs/`](docs/):

- [BALLISTICS.md](docs/BALLISTICS.md) - Trajectory simulation, penetration, and splash mechanics
- [MODELS.md](docs/MODELS.md) - `.geometry` file format specification
- [TEAM_ADVANTAGE_SCORING.md](docs/TEAM_ADVANTAGE_SCORING.md) - Team advantage calculation algorithm
- [format_templates/](docs/format_templates/) - Binary format templates for 010 Editor

## Community Discussion

If you'd like to discuss the toolkit features, bugs, or whatever, please feel free to open an issue here on GitHub or join the Discord server: https://discord.gg/SpmXzfSdux.

## Pre-Built Application Binaries

Pre-built binaries for Windows are provided at https://github.com/landaire/wows-toolkit/releases/latest Download the `
wows-toolkit_v(VERSION)_x86_64-pc-windows-gnu.zip` file, extract the application somewhere, and you're good to go! For all other platforms you will have to compile yourself.

## Usage

1. Run the application
2. Set the World of Warships directory in the settings tab (defaults to `C:\Games\World_of_Warships` if it exists)
3. ???
4. Do things

The application will automatically check for updates on startup and, if available, will present update details in-app.

This is not considered a World of Warships mod and does not modify your World of Warships install at all. It passively reads game files required for parsing replays, and parses replay files directly.

## Features

- Can read replay files and display statistics such as damage dealt, time lived, spotting damage, and potential damage.
- Can view player builds by clicking the "Actions" button on a player row and choosing to open the build in your web browser.
- Can browse and extract packed game files.
- Automatically sends **builds** (not raw replays) to shipbuilds.com for build statistic gathering from **Randoms** and **Ranked** games. Training rooms are not sent. Sending replay data can be disabled in the application settings tab.

## For Developers

If you do not want to compile the application yourself or make changes to WoWs Toolkit please ignore this section!

To build yourself, make sure you are using the latest version of stable rust by running `rustup update`. Next, simply run `cargo run --release -p wows_toolkit` from the source code directory.

To build the CLI tools:

```
cargo build --release -p wowsunpack
cargo build --release -p wows_minimap_renderer --features bin
cargo build --release -p replayshark
```

On Linux you need to first run:

`sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev libgtk-3-dev`

On Fedora Rawhide you need to run:

`dnf install clang clang-devel clang-tools-extra libxkbcommon-devel pkg-config openssl-devel libxcb-devel gtk3-devel atk fontconfig-devel`

### Nix

A Nix flake is provided with a devShell and packages for the CLI tools:

```
nix develop          # Enter dev shell
nix build .#wowsunpack
nix build .#minimap-renderer
nix build .#replayshark
```
