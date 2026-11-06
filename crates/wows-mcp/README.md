# wows-mcp

- Answers World of Warships game data questions for any [MCP](https://modelcontextprotocol.io) client.
- Looks up ships (including ships in closed testing), armor, aircraft and their ordnance, shells, penetration and overmatch, concealment, stat cards with captain skills and upgrades applied, captain skills, upgrades, consumables, ship prices and research tree links, and replay headers.
- Gives build-dependent stats the way players ask for them: the base value, the best-fit build with the skills and upgrades it assumes, and common alternatives such as the same build without Concealment Expert.
- Understands community names: abbreviations such as SE, SI, IFHE, DM and GK, initialisms of any multi-word name, former skill names such as BFT, and names typed without the German sharp s. Every name read as something else is listed under `interpretations`.
- Answers anything else in GameParams with a [jq](https://jqlang.org) filter over the raw entries.
- Finds ships whose stats meet your conditions, with the fewest modules, upgrades and captain skills they need.
- Reads one game build from a [wows-replay-data](https://github.com/landaire/wows-replay-data) dump, or downloads the newest one.

## Usage

Download the newest game data build and serve it on stdio:

```bash
wows-mcp --data ~/wows-game-data --download
```

Later runs can skip `--download` and start from what is already on disk:

```bash
wows-mcp --data ~/wows-game-data
```

`--data` takes a dump root (the directory with `builds.toml`) or a single build directory (the one with `metadata.toml`).

If the dump's GameParams cache was written by another version of the toolkit, the server parses `GameParams.data` itself, which takes a few seconds. Pass `--cache-dir` to keep the rebuilt cache so the next start is fast:

```bash
wows-mcp --data ~/wows-game-data --cache-dir ~/.cache/wows-mcp
```

`gameparams_query` needs `--cache-dir`. Its first call writes the raw GameParams of the build there, about 40 MB, which takes about ten seconds.

To serve streamable HTTP instead of stdio, give an address:

```bash
wows-mcp --data ~/wows-game-data --http 127.0.0.1:8080
```

### Client configuration

For a client that launches stdio servers from a JSON config:

```json
{
  "mcpServers": {
    "wows": {
      "command": "wows-mcp",
      "args": ["--data", "/home/you/wows-game-data", "--cache-dir", "/home/you/.cache/wows-mcp"]
    }
  }
}
```

## Tools

`find_ships`, `ship_overview`, `tech_tree_lines`, `ship_armor`, `ship_aircraft`, `ship_shells`, `ship_concealment`, `penetration_search`, `armor_trace`, `citadel_profile`, `armor_assessment`, `turtleback_search`, `ship_stats`, `ship_firing_arcs`, `stat_names`, `stat_search`, `captain_skill`, `skill_tree`, `upgrade`, `ship_upgrades`, `ship_consumables`, `gameparams_query`, `replay_summary`.

`armor_trace`, `citadel_profile`, `armor_assessment`, `turtleback_search` and `ship_firing_arcs` read the ship armor models in the game data dump. `ship_firing_arcs` takes the mount facings from them. These tools need a dump that carries the armor extract. `wows-data-mgr dump-renderer-data` writes it unless you pass `--no-armor`.

Each tool returns compact JSON. The query logic lives in the `wows-query` crate.

## Building

```bash
cargo build -p wows-mcp --release
buck2 build //crates/wows-mcp:wows_mcp_bin
```
