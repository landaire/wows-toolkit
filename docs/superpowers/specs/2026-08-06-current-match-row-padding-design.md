# Current Match Row Padding Design

## Goal

Give roster content more colored breathing room while making adjacent row bands vertically continuous.

## Layout

- Row horizontal inner padding is 10px on both edges.
- Row vertical inner padding is 3px above and below content.
- Inter-row spacing is zero, so adjacent painted bands touch. The row-loop child UI sets egui `item_spacing.y` to zero; changing only the explicit row-gap constant is insufficient because egui otherwise inserts implicit widget spacing.
- The 2px Detailed line gap is unchanged.
- Header horizontal padding follows the row horizontal inset so columns remain aligned.
- Compact and Detailed use the same outer row padding.

## Verification

The production-path Compact and Detailed kitty snapshots must change for the new geometry, then pass after deliberate baseline updates. A production row-layout helper returns the actual consecutive Frame response rectangles so a geometry test can assert `first.max.y == second.min.y`; fixed columns must remain aligned.
