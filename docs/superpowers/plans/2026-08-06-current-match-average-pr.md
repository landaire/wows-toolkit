# Current Match Average PR Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render localized team average PR beside team average WR in Current Match headings.

**Architecture:** Add a pure PR aggregate over resolved row stats and a small heading renderer that independently emits WR and PR summaries. Extend the production-path kitty snapshot plus focused unit tests before changing production behavior.

**Tech Stack:** Rust 1.92, egui, egui_kittest, rust-i18n, jj

## Global Constraints

- Average PR uses account PR in both Overall and Ship modes.
- Missing PR rows are skipped, never treated as zero.
- WR and PR summary availability are independent.
- Visible labels are localized and ASCII-only in source.

---

### Task 1: Team average PR heading

**Files:**
- Modify: `crates/wows-toolkit/src/ui/player_tracker/current_match.rs`
- Modify: `crates/wows-toolkit/tests/snapshots/current_match_two_teams.png`

**Interfaces:**
- Consumes: `row_stats`, `PlayerStatsOut`, `PersonalRatingCategory`, `TeamContext`
- Produces: `team_average_personal_rating(...) -> Option<f64>` and heading UI containing independent Avg WR and Avg PR labels

- [ ] **Step 1: Write failing tests**

Add unit tests for averaging only resolved PR values, returning `None`, and mode invariance. Extend the kitty test to require `Avg PR: 1800` after each team's Avg WR label.

- [ ] **Step 2: Verify RED**

Run the focused unit and kitty tests and confirm failure because the PR aggregate and heading label do not exist.

- [ ] **Step 3: Implement minimal behavior**

Add the aggregate, render `{t!("stat.avg_pr")}: {average:.0}`, color it with `PersonalRatingCategory::from_pr(average).swatch(...)`, and keep WR/PR rendering in separate `if let` branches.

- [ ] **Step 4: Verify GREEN and inspect snapshot**

Update the kitty snapshot, inspect it visually, then rerun the exact kitty test without snapshot updates.

- [ ] **Step 5: Verify, review, and commit**

Run format, clippy, and the full library suite. Obtain a fresh adversarial review, address Critical and Important findings, then commit with `fix(player-tracker): show team average PR`.
