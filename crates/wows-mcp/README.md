# wows-mcp

- Answers World of Warships game data questions for any [MCP](https://modelcontextprotocol.io) client.
- Looks up ships, armor, shells, penetration and overmatch, stat cards with captain skills and upgrades applied, captain skills, upgrades, consumables, ship prices and research tree links, and replay headers.
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

`find_ships`, `ship_overview`, `ship_armor`, `ship_shells`, `penetration_search`, `ship_stats`, `captain_skill`, `skill_tree`, `upgrade`, `ship_upgrades`, `ship_consumables`, `replay_summary`.

Each tool returns compact JSON. The query logic lives in the `wows-query` crate.

## Building

```bash
cargo build -p wows-mcp --release
buck2 build //crates/wows-mcp:wows_mcp_bin
```
