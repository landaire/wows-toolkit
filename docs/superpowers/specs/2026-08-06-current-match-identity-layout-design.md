# Current Match Identity Layout Design

## Goal

Match the replay inspector's left-to-right identity order in the Current Match roster while preserving stable statistic columns.

## Layout

Each roster row uses this column order:

`Class | Player | Ship | WR | PR | Battles | Seen | Actions`

- Class contains only the ship class icon.
- Player contains the optional clan tag immediately followed by the player name in one grid cell on both teams. The clan tag keeps its clan color. Existing note, hidden-profile, and Twitch adornments remain in this cell after the identity text; they cannot reorder the clan and name or move the statistic columns.
- Ship contains the localized ship name, with the existing species text as its fallback.
- WR, PR, Battles, Seen, and Actions retain fixed positions.
- Seen renders only the encounter count. The localized Seen header supplies the meaning; rows do not repeat a `Seen` prefix.
- Class uses `ui.search.field.class`, Player uses `ui.search.field.player_present`, Ship uses `ui.replay.column.ship_name`, and Seen uses `ui.player_tracker.column.encounters`. No literal English header is introduced.

## Responsive Behavior

The two team panels retain their current side-by-side and stacked breakpoints. Long player or ship names truncate within their assigned cells and do not move the statistic columns.

## Verification

The production-path egui_kittest test and snapshot must verify:

- the class icon precedes the combined clan and player text;
- the combined player cell precedes the ship name;
- clan and player text occupy one grid column;
- the rendered headers match the localized Class, Player, Ship, and Seen values;
- the Seen cell's accessible text is exactly the formatted encounter count and contains neither `Seen` nor `x`;
- WR and the other statistic columns remain aligned across varied content.
