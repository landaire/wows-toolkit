//! What a pointer resting on a frame reads: a roster's consumable icon, or
//! the advantage label.

use rust_i18n::t;
use wows_minimap_renderer::draw_command::ConsumableLines;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::consumable_lines;
use wows_minimap_renderer::drawing::DrawnRegion;
use wows_minimap_renderer::drawing::RegionKind;

/// What a hover over a roster's consumable icon reads, and whose it is.
pub struct ConsumableHover {
    pub player: String,
    pub lines: ConsumableLines,
}

impl ConsumableHover {
    /// Whether two hovers read the same icon, which is when a front end can
    /// leave the reading on screen as it is.
    pub fn is_same_icon(&self, other: &Self) -> bool {
        self.player == other.player && self.lines.name == other.lines.name
    }
}

/// What the roster icon at `at`, a canvas point, reads as, if it is on one.
///
/// `regions` are what the frame showing `commands` drew that a pointer can
/// rest on. The region says which row and which of its icons; the rest is read
/// off the roster command the frame already holds, so a reading cannot drift
/// from what was drawn.
pub fn consumable_at(regions: &[DrawnRegion], commands: &[DrawCommand], at: (f32, f32)) -> Option<ConsumableHover> {
    let region = regions.iter().find(|region| region.contains(at))?;
    let RegionKind::RosterConsumable { entity_id, index } = region.kind;
    for command in commands {
        let DrawCommand::TeamRoster { rows, .. } = command else { continue };
        let Some(row) = rows.iter().find(|row| row.entity_id == entity_id) else { continue };
        let Some(consumable) = row.consumables.get(index) else { continue };
        return Some(ConsumableHover { player: row.player_name.clone(), lines: consumable_lines(consumable) });
    }

    None
}

/// Whether the consumable reading under the pointer changed.
pub fn hover_changed(was: Option<&ConsumableHover>, now: Option<&ConsumableHover>) -> bool {
    match (was, now) {
        (Some(was), Some(now)) => !was.is_same_icon(now),
        (None, None) => false,
        _ => true,
    }
}

/// What the advantage reads, one line each, for a frame's commands.
///
/// The label itself is drawn into the frame by the renderer, so there is no
/// element of its own to rest a pointer on; the strip it sits in is what the
/// reader hovers instead. `None` when the frame carries no advantage.
pub fn advantage_hover(commands: &[DrawCommand]) -> Option<Vec<String>> {
    let breakdown = commands.iter().find_map(|command| match command {
        DrawCommand::TeamAdvantage { level: Some(_), breakdown, .. } => Some(breakdown),
        _ => None,
    })?;

    let mut lines = vec![t!("ui.renderer.advantage.breakdown").into_owned()];
    match wows_minimap_renderer::advantage::breakdown_rows(breakdown) {
        None => lines.push(t!("ui.renderer.advantage.team_eliminated").into_owned()),
        Some(rows) => {
            for row in rows {
                lines.push(format!("{}  {}", t!(row.label_key), row.contribution));
            }
            if !breakdown.hp_data_reliable {
                lines.push(t!("ui.renderer.advantage.hp_incomplete").into_owned());
            }
        }
    }

    Some(lines)
}
