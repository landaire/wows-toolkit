# WoWs Toolkit

This is a toolkit for interacting with World of Warships files

![WoWs Toolkit Replay Tab](assets/wows_toolkit_replay_screenshot.png)

![WoWs Toolkit Unpacker Tab](assets/wows_toolkit_unpacker_screenshot.png)

![WoWs Toolkit Unpacker Tab With Filtering](assets/wows_toolkit_unpacker_filtering.png)

## Usage

1. Run the application
2. Set the World of Warships directory in the settings tab (defaults to `C:\Games\World_of_Warships` if it exists)
3. ???
4. Do things

The application will automatically check for updates on startup and, if available, will present update details in-app.

## Pre-built Binaries

Pre-built binaries for Windows are provided at https://github.com/landaire/wows-toolkit/releases. For all other platforms you will have to compile yourself.

## Building Locally

Make sure you are using the latest version of stable rust by running `rustup update`.

`cargo run --release`

On Linux you need to first run:

`sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev libgtk-3-dev`

On Fedora Rawhide you need to run:

`dnf install clang clang-devel clang-tools-extra libxkbcommon-devel pkg-config openssl-devel libxcb-devel gtk3-devel atk fontconfig-devel`