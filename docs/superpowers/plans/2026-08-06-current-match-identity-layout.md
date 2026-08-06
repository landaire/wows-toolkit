# Current Match Identity Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reorder Current Match roster identities to Class, combined Player, then Ship, and show a bare Seen count.

**Architecture:** Keep the existing eight-track Taffy grid and fixed statistic positions, but replace the three identity tracks with a fixed class-icon track, flexible combined player track, and fixed ship-name track. Exercise the exact production render path with egui_kittest geometry, accessibility text, and snapshot assertions.

**Tech Stack:** Rust 1.92, egui, egui_taffy, egui_kittest, rust-i18n, jj

## Global Constraints

- Code and UI literals are ASCII only.
- All visible column headings use existing localization keys.
- WR, PR, Battles, Seen, and Actions remain fixed across rows.
- The test must fail for the old layout before production code changes.

---

### Task 1: Current Match identity grid

**Files:**
- Modify: `crates/wows-toolkit/src/ui/player_tracker/current_match.rs`
- Modify: `crates/wows-toolkit/tests/snapshots/current_match_two_teams.png`

**Interfaces:**
- Consumes: `render_rosters`, `render_team`, `LiveRosterRow`, and `TeamContext`
- Produces: production rows ordered as Class, Player, Ship, WR, PR, Battles, Seen, Actions

- [ ] **Step 1: Write failing kitty assertions**

Update `kittest_current_match_columns_are_visually_aligned` so it asserts the localized Class, Player, Ship, and Seen headers; class-before-clan-before-name-before-ship ordering; aligned player, ship, and WR columns; and an exact bare Seen value without `Seen` or `x`. Add tracked fixture data so Seen renders.

- [ ] **Step 2: Verify RED**

Run `cargo test -p wows_toolkit --lib ui::player_tracker::current_match::tests::kittest_current_match_columns_are_visually_aligned -- --exact` and confirm it fails because the old grid order and `Seen xN` text do not satisfy the assertions.

- [ ] **Step 3: Implement the layout**

Change the identity grid tracks and render calls to Class, combined Player, and Ship. Render clan plus player name in the same cell, keep identity adornments after them, use `ui.search.field.class`, `ui.search.field.player_present`, and `ui.replay.column.ship_name` headers, and render `separate_number(total, Some(ctx.locale))` as the Seen cell text.

- [ ] **Step 4: Verify GREEN and update snapshot**

Run the exact kitty test with `UPDATE_SNAPSHOTS=1`, inspect `crates/wows-toolkit/tests/snapshots/current_match_two_teams.png`, then rerun without snapshot updates and require PASS.

- [ ] **Step 5: Run regression verification**

Run `cargo fmt --all -- --check`, `cargo clippy -p wows_toolkit --all-targets`, and `cargo test -p wows_toolkit --lib`.

- [ ] **Step 6: Review and commit**

Obtain a fresh adversarial review, address Critical and Important findings, then commit the implementation and snapshot with `jj commit` using message `fix(player-tracker): reorder current-match identities`.
