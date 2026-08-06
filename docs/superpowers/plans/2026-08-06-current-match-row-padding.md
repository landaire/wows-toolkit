# Current Match Row Padding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Increase colored row padding and remove vertical gaps.

**Architecture:** Adjust shared row inset constants, render the row loop inside a child UI whose egui vertical item spacing is zero, remove trailing-gap paint extension, and retain the fixed Taffy grid and Detailed line spacing.

**Tech Stack:** Rust, egui, egui_kittest, jj

## Tasks

- [ ] Add a failing assertion over actual consecutive Frame response rectangles.
- [ ] Change horizontal padding to 10px, vertical padding to 3px, and row gap to zero.
- [ ] Update and visually inspect Compact and Detailed snapshots.
- [ ] Run format, clippy, and full library tests.
- [ ] Obtain fresh adversarial review and commit.
