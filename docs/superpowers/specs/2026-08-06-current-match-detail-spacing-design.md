# Current Match Detail Spacing Design

## Goal

Improve Detailed row readability while removing exposed parent-background gaps between roster rows.

## Behavior

- Detailed rows use one `DETAIL_LINE_GAP` constant of 2px between lines in the Player, WR, and Battles cells.
- The combined Player cell renders a small, muted, localized `Ship Stats` label at the beginning of line two.
- The label uses `ui.player_tracker.ship_stats` from the English fallback catalog.
- The existing inter-row spacing remains 2px, but belongs to the preceding row's painted band so adjacent rows have continuous background coverage.
- Compact retains its one-line content and also paints through the existing inter-row spacing.
- The final row does not paint beyond its content because there is no following-row spacing.

## Verification

- Detailed kitty geometry asserts the `Ship Stats` label aligns with the Player column, its second-line top aligns with ship WR and ship battles within 0.1px, and each cell has at least the explicit 2px gap below line-one content.
- A paint-geometry unit test asserts non-final row backgrounds include `ROW_GAP` and final rows do not.
- Updated Detailed and Compact snapshots are visually inspected for continuous row bands and readable line spacing.
