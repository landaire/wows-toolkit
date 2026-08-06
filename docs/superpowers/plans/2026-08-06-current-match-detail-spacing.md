# Current Match Detail Spacing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Detailed second-line label and spacing while painting roster bands through inter-row padding.

**Architecture:** Keep the fixed Taffy grid. Turn the Detailed Player cell into a two-line stack matching WR/Battles, and extend each non-final painted row rectangle by `ROW_GAP` instead of exposing the parent surface.

**Tech Stack:** Rust, egui, egui_taffy, egui_kittest, rust-i18n, jj

## Tasks

- [ ] Add failing kitty assertions for the exact 2px shared Player/WR/Battles line gap and second-line alignment, plus paint-geometry assertions.
- [ ] Confirm RED.
- [ ] Add localized `Ship Stats`, 2px detail-line spacing, and gap-inclusive row paint.
- [ ] Update and inspect snapshots; rerun kitty without updates.
- [ ] Run format, clippy, and full library tests.
- [ ] Obtain fresh adversarial review and commit focused changes.
