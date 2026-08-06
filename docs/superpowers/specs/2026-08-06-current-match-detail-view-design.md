# Current Match Compact and Detailed View Design

## Goal

Preserve the current roster as Compact view and add a Detailed view that exposes overall and ship scope stats in every row.

## State and Controls

- Add a persisted `CurrentMatchViewMode` with `Compact` and `Detailed` variants.
- New and existing saved state defaults to Detailed through serde defaulting.
- Add localized Compact and Detailed selectable controls beside the existing Overall and Ship scope controls, separated as a distinct toggle group.
- Changing either toggle applies after rendering, following the existing lock-safe action pattern.

## Compact Rows

Compact is the current one-line layout without visual or behavioral changes. WR and Battles follow the selected Overall or Ship scope, PR remains account PR, and the selected WR controls row coloring.

## Detailed Rows

- Identity, PR, Seen, and Actions remain single entries within a taller row.
- The WR column renders overall WR on the first line and ship WR on the second line.
- The Battles column renders overall battles on the first line and ship battles on the second line.
- PR renders once on the first line because the service provides account PR only.
- Missing values render the same weak dash used by Compact rows, preserving the two-line structure.
- The Overall or Ship scope toggle does not hide either line. It controls row coloring and the team Avg WR summary.
- Overall values always occupy the first line and ship values always occupy the second line, independent of the selected coloring scope.
- WR hovers retain the existing localized Overall and Ship breakdown. Battle-line hovers identify their scope with the existing localized Overall and Ship labels.

## Layout

Detailed rows retain the fixed Taffy columns and horizontal padding. WR and Battles cells become two-line vertical stacks; the other cells align with the first line. The selected scope alone determines the band, so a missing selected-scope WR produces no band even when the other WR exists. The row background spans the full two-line row height. Team side-by-side and stacked breakpoints remain unchanged.

Compact retains the existing no-scroll target. Detailed intentionally trades vertical density for visible data and may require vertical scrolling when the viewport cannot contain the taller roster.

## Localization

Add `ui.player_tracker.view_compact` and `ui.player_tracker.view_detailed` labels to the English fallback catalog. Other locales use the established English fallback until translated.

## Verification

- Keep the existing Compact production-path kitty snapshot as a regression baseline.
- Add a Detailed production-path kitty snapshot with distinct overall and ship WR/battle fixtures.
- Kitty geometry asserts overall values are above ship values, ship values remain on the second line, and fixed columns align across rows.
- Separate Overall and Ship Detailed renders assert identical displayed values and verify the selected row paint color through snapshot or inspected paint-shape color evidence.
- A full twelve-player-per-team Compact fixture at a representative side-by-side viewport protects the Compact no-scroll target. Detailed overflow is permitted.
- Persistence tests deserialize legacy `PlayerTracker` data with the view field absent as Detailed and round-trip both enum variants. Unit tests also cover Detailed stat-line selection.
