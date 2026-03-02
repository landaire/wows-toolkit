# Minimap Renderer

Generates minimap timelapse videos from World of Warships replay files (`.wowsreplay`).

## Quick Start

There are two ways to provide game data to the renderer:

1. **Direct game install** (`--game`) -- point at a WoWs installation directory
2. **Pre-extracted data** (`--extracted-dir`) -- point at a lightweight dump of only the files the renderer needs

Option 2 is useful when you don't have the full game installed (e.g. CI, a server, or a friend's machine).

## Usage

### Rendering from a game installation

```sh
minimap_renderer \
  --game "C:\Games\World_of_Warships" \
  -o output.mp4 \
  replay.wowsreplay
```

### Rendering from pre-extracted data

```sh
minimap_renderer \
  --extracted-dir /path/to/extracted_data \
  -o output.mp4 \
  replay.wowsreplay
```

The `--extracted-dir` can point to either:
- The version directory itself (e.g. `extracted/15.1.0_11965230/`)
- A parent directory containing version subdirectories -- the renderer will auto-detect and match the replay's build number

## Creating Extracted Data with `wows-data-mgr`

The `wows-data-mgr` CLI manages game data downloads and can dump the subset of files the renderer needs.

### Step 1: Register your game installation

If you have WoWs installed locally, register it so `wows-data-mgr` knows where to find game files:

```sh
# Register as "latest" -- always uses whatever build is currently installed
wows-data-mgr register --latest --path "C:\Games\World_of_Warships"
```

You can also register a specific version:

```sh
wows-data-mgr register --version 15.1 --path "C:\Games\World_of_Warships"
```

Verify it was registered:

```sh
wows-data-mgr list
```

### Step 2: Dump renderer data

Extract only the files the renderer needs into a portable directory:

```sh
# Dump the latest available build
wows-data-mgr dump-renderer-data --latest -o ./extracted

# Or dump a specific version
wows-data-mgr dump-renderer-data --version 15.1 -o ./extracted

# Or dump by build number
wows-data-mgr dump-renderer-data --build 11965230 -o ./extracted
```

This creates a directory like `./extracted/15.1.0_11965230/` containing:

```
15.1.0_11965230/
  metadata.toml              # version + build info
  game_params.rkyv           # serialized game parameters
  translations/en/LC_MESSAGES/
    global.mo                # ship name translations
  vfs/                       # extracted game files
    spaces/*/minimap.png     # map images (land layer)
    spaces/*/minimap_water.png
    spaces/*/space.settings  # map metadata
    content/gameplay/*/space.settings
    content/GameParams.data
    gui/fla/minimap/         # ship icons
    gui/battle_hud/          # plane, death, capture icons
    gui/consumables/         # consumable icons
    gui/powerups/drops/      # powerup icons
    gui/fonts/               # rendering fonts
    gui/data/constants/      # game constants
    scripts/entities.xml     # entity definitions
    scripts/entity_defs/     # entity def files
```

Only the files the renderer actually reads are extracted -- no map geometry, models, textures, or other large assets.

### Step 3: Render

```sh
minimap_renderer \
  --extracted-dir ./extracted \
  -o output.mp4 \
  replay.wowsreplay
```

## Renderer Options

| Flag | Description |
|------|-------------|
| `--no-player-names` | Hide player names |
| `--no-ship-names` | Hide ship names |
| `--no-capture-points` | Hide capture zones |
| `--no-buildings` | Hide building markers |
| `--no-turret-direction` | Hide turret direction indicators |
| `--no-armament` | Hide selected armament/ammo type |
| `--show-trails` | Show position trail heatmap |
| `--no-dead-trails` | Hide trails for dead ships |
| `--show-speed-trails` | Show speed-based trails (blue=slow, red=fast) |
| `--show-ship-config` | Show detection/battery range circles |
| `--config <path>` | Load render settings from a TOML config file |
| `--dump-frame <n\|mid\|last>` | Dump a single frame as PNG instead of video |
| `--cpu` | Use CPU encoder (openh264) instead of GPU |
| `--check-encoder` | Check encoder availability and exit |
| `--generate-config` | Print default TOML config to stdout |
