# AGENTS.md

Guidance for agents working in the wows-toolkit repository. These rules override default behavior and apply to every crate unless a crate-local AGENTS.md says otherwise.

## Repository

Cargo workspace, edition 2024, rust 1.92. Crates under `crates/`:

- `wows-toolkit` main desktop app (egui + eframe + glow)
- `wowsunpack` game data unpacking, models, game params
- `wows-replays` replay file parsing and packet decode
- `minimap-renderer` minimap draw command generation
- `replayshark` CLI replay analysis
- `wows-data-mgr` game data management
- `wt-collab-protocol`, `wt-collab-egui` shared collab protocol and UI
- `wt-web` WASM client
- `wt-translations` translations
- `wows-replay-insights` derived replay insights
- `wgcheck` WGCheck .gch report parsing

## Version control

- The repo is jj-colocated. Use `jj`, not `git`, as the authoritative interface.
- Never append `Co-Authored-By` or any AI attribution to commit messages.
- Make focused commits, one per logical change or milestone.

## Types and data modeling

- Prefer newtypes over raw primitives for domain values, even when the value arrives as a primitive. Wrap identifiers and any quantities that could be confused with each other (angles together with their unit, bitflags, durations, indices, weapon groups) in distinct newtypes so the type system rejects mixing them. Reuse existing newtypes (`wowsunpack` `TeamId`, `GameParamId`, `EntityId`, etc.) instead of storing their raw inner value.
- Model "absent" or "unlimited" with `Option` or an enum, never sentinel values like `-1`, `0`, or empty string.
- Bubble `Option` and `Result` up as far as practical. Resolve them at the boundary where there is enough context to handle them correctly.

## Defaults and missing data

- Scrutinize every `.unwrap_or`, `.unwrap_or_default`, `.unwrap_or_else`, and `Default` applied to parsed or possibly-missing data.
- Do not paper over malformed or absent input with a default unless that default is genuinely correct. When it is, document why at the call site. Otherwise propagate the error or the option.

## Errors

- Use strong `thiserror` enums with structured fields.
- Never match on an error's `Display` or `Debug` string to recover data. If meaningful data is only reachable by parsing a formatted string, the error type is wrong; add a field.
- Use `rootcause` to attach context as errors cross boundaries.

## Comments

- Comments explain non-obvious intent only. Keep them terse and DRY.
- No salesmanship or filler wording.
- No historical framing ("now X", "was Y, now Z"). Describe current behavior.
- No numbered step-recaps of the implementation. Short WHY lines only.

## Text and formatting

- ASCII only in code, comments, UI strings, and commit messages. No emdash, endash, ellipsis, arrows, or other unicode symbols.
- No separator or banner comments (`// ---`, `// ===`, long dashed/equals rules, section dividers). Structure code with modules, functions, and blank lines, not comment dividers.

## Compatibility

- Changes must work across old (0.6.x) and current game versions. Packet layout differences are version-gated; see `MODERN_PACKET_LAYOUT_MIN_VERSION` in `wows-replays` `packet2.rs`.

## Review

- At the end of each milestone, run an adversarial code review with a fresh subagent before committing. For ECS work the reviewer must be framed as a bevy_ecs/ECS expert and instructed to challenge the design itself (component vs resource boundaries, query patterns, archetype/iteration-order determinism, entity lifecycle), not just surface code quality.

## Active major effort

Reimplementing `BattleController` (in `wows-replays`) on `bevy_ecs` as a new `wows-battle-world` crate. Design spec: `docs/superpowers/specs/2026-06-04-battle-world-ecs-design.md`.
