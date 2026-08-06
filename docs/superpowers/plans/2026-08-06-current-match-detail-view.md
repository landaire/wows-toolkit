# Current Match Detailed View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve the current roster as Compact and add a persisted Detailed view with overall stats first and ship stats second.

**Architecture:** Add a serde-defaulted-to-Detailed view enum to `PlayerTracker`, carry it through the lock-safe UI action/context path, and switch only the WR/Battles cell renderers between one-line selected-scope and two-line fixed-scope output. Keep row banding tied to the existing scope toggle.

**Tech Stack:** Rust 1.92, serde, egui, egui_taffy, egui_kittest, rust-i18n, jj

## Global Constraints

- Compact behavior and snapshot remain unchanged.
- Detailed always shows overall first and ship second.
- PR remains account-level and appears once.
- The selected scope controls row banding and team Avg WR only.
- Existing fixed horizontal columns remain aligned.

---

### Task 1: Persisted view mode and controls

**Files:**
- Modify: `crates/wows-toolkit/src/ui/player_tracker/mod.rs`
- Modify: `crates/wows-toolkit/src/ui/player_tracker/current_match.rs`
- Modify: `crates/wt-translations/translations/en.toml`

- [ ] Write failing default, serde round-trip, and control-action tests.
- [ ] Run focused tests and confirm RED.
- [ ] Add `CurrentMatchViewMode`, persisted field, localized controls, and lock-safe action application.
- [ ] Run focused tests and confirm GREEN.

### Task 2: Detailed row rendering

**Files:**
- Modify: `crates/wows-toolkit/src/ui/player_tracker/current_match.rs`
- Create: `crates/wows-toolkit/tests/snapshots/current_match_two_teams_detailed_overall.png`
- Create: `crates/wows-toolkit/tests/snapshots/current_match_two_teams_detailed_ship.png`

- [ ] Add failing kitty assertions for overall-first/ship-second WR and battles, unchanged values across scope renders, aligned columns, and scope-driven row paint.
- [ ] Add a failing 12-player-per-team Compact no-overflow assertion.
- [ ] Run kitty tests and confirm RED.
- [ ] Implement two-line WR/Battles cells and full-height row paint while leaving Compact unchanged.
- [ ] Update snapshots, inspect them, and rerun kitty without updates.

### Task 3: Verification and integration

- [ ] Run formatting, clippy, and the full library suite.
- [ ] Obtain fresh adversarial review and address Critical/Important findings.
- [ ] Commit logical implementation milestones with focused `jj` commits.
