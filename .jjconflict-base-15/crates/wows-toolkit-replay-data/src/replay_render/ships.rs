//! Per-ship choices: which ship a pointer meant, and whose trail and ranges
//! are drawn.

use std::collections::HashMap;
use std::collections::HashSet;

use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_replays::types::EntityId;

use crate::replay_render::view::WindowPoint;

/// How near a right-click has to land to have meant a ship, in map pixels.
/// A ship icon is about this wide.
pub const PICK_RADIUS: f32 = 14.0;

/// A ship showing none of its ranges.
pub const NO_RANGES: ShipConfigFilter = ShipConfigFilter {
    detection: false,
    main_battery: false,
    secondary_battery: false,
    torpedo: false,
    radar: false,
    hydro: false,
};

/// A ship showing all of them.
pub const ALL_RANGES: ShipConfigFilter = ShipConfigFilter {
    detection: true,
    main_battery: true,
    secondary_battery: true,
    torpedo: true,
    radar: true,
    hydro: true,
};

/// A ship a pointer picked out of a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickedShip {
    pub entity_id: EntityId,
    pub player_name: String,
}

/// The ship a context menu is open for.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipMenu {
    pub entity_id: EntityId,
    pub player_name: String,
    /// Where the right-click landed, which is where the menu opens: a menu
    /// about one ship belongs beside that ship, not in a corner.
    pub at: WindowPoint,
}

impl ShipMenu {
    pub fn open(ship: PickedShip, at: WindowPoint) -> Self {
        Self { entity_id: ship.entity_id, player_name: ship.player_name, at }
    }
}

/// Whose trails are hidden and whose ranges are drawn, by player name, which
/// is what a trail command and a range command carry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShipChoices {
    trail_hidden: HashSet<String>,
    /// A ship with no entry shows none, which is what keeps the map readable
    /// until the reader asks for one.
    ship_ranges: HashMap<String, ShipConfigFilter>,
}

impl ShipChoices {
    pub fn new() -> Self {
        Self { trail_hidden: HashSet::new(), ship_ranges: HashMap::new() }
    }

    pub fn trail_hidden(&self) -> &HashSet<String> {
        &self.trail_hidden
    }

    pub fn ship_ranges(&self) -> &HashMap<String, ShipConfigFilter> {
        &self.ship_ranges
    }

    /// Whether the reader has asked for `command`'s ship's trail or range.
    pub fn allows(&self, command: &DrawCommand) -> bool {
        per_ship_allows(command, &self.trail_hidden, &self.ship_ranges)
    }

    /// Whether `player` has a trail drawn.
    pub fn trail_shown(&self, player: &str) -> bool {
        !self.trail_hidden.contains(player)
    }

    /// Shows or hides one ship's trail.
    ///
    /// Turning one on turns the trails on, since a reader who asked for this
    /// ship's trail meant to see it; hiding the last of the `drawn` ships'
    /// turns them off again rather than leaving an empty layer switched on.
    pub fn set_trail_shown(&mut self, player: &str, shown: bool, drawn: &[String], options: &mut RenderOptions) {
        if shown {
            self.trail_hidden.remove(player);
            options.show_trails = true;
        } else {
            self.trail_hidden.insert(player.to_owned());
            if drawn.iter().all(|name| self.trail_hidden.contains(name)) {
                options.show_trails = false;
            }
        }
    }

    /// Hides every one of the `drawn` ships' trails but `player`'s.
    pub fn only_trail(&mut self, player: &str, drawn: &[String], options: &mut RenderOptions) {
        self.trail_hidden = drawn.iter().filter(|name| *name != player).cloned().collect();
        options.show_trails = true;
    }

    /// Which ranges `player` has on.
    pub fn ranges_for(&self, player: &str) -> ShipConfigFilter {
        self.ship_ranges.get(player).copied().unwrap_or(NO_RANGES)
    }

    /// Changes which ranges one ship shows.
    pub fn set_ranges_for(
        &mut self,
        player: &str,
        apply: impl FnOnce(&mut ShipConfigFilter),
        options: &mut RenderOptions,
    ) {
        let mut filter = self.ranges_for(player);
        apply(&mut filter);
        self.ship_ranges.insert(player.to_owned(), filter);

        // Ranges are drawn at all only while some ship has one.
        options.show_ship_config = self.ship_ranges.values().any(|filter| *filter != NO_RANGES);
    }

    /// Leaves only `player`'s ranges on, or all of them when it had none.
    pub fn only_ranges(&mut self, player: &str, options: &mut RenderOptions) {
        let keep = self.ranges_for(player);
        let keep = if keep == NO_RANGES { ALL_RANGES } else { keep };
        self.ship_ranges.clear();
        self.ship_ranges.insert(player.to_owned(), keep);
        options.show_ship_config = true;
    }

    /// Turns all of one ship's ranges on, or all of them off.
    ///
    /// The six switches one at a time is six clicks to ask a question that
    /// is usually all-or-nothing, which is why the egui menu offers both.
    pub fn set_every_range_for(&mut self, player: &str, on: bool, options: &mut RenderOptions) {
        self.set_ranges_for(player, |filter| *filter = if on { ALL_RANGES } else { NO_RANGES }, options);
    }

    /// Turns every one of the `drawn` ships' ranges on.
    pub fn all_ranges(&mut self, drawn: &[String], options: &mut RenderOptions) {
        self.ship_ranges = drawn.iter().map(|name| (name.clone(), ALL_RANGES)).collect();
        options.show_ship_config = true;
    }
}

/// The ship drawn nearest `at`, a map point, if one is close enough to have
/// been meant.
///
/// Reads one frame's commands rather than the whole track: a right-click
/// picks what the reader can see.
pub fn ship_at(commands: &[DrawCommand], at: (f32, f32)) -> Option<PickedShip> {
    let mut best: Option<(f32, PickedShip)> = None;
    for command in commands {
        let (pos, player_name, entity_id) = match command {
            DrawCommand::Ship { pos, player_name, entity_id, .. }
            | DrawCommand::DeadShip { pos, player_name, entity_id, .. } => (pos, player_name, entity_id),
            _ => continue,
        };
        let Some(name) = player_name.clone() else { continue };
        let away = ((pos.x - at.0).powi(2) + (pos.y - at.1).powi(2)).sqrt();
        if away > PICK_RADIUS {
            continue;
        }
        if best.as_ref().is_none_or(|(nearest, _)| away < *nearest) {
            best = Some((away, PickedShip { entity_id: *entity_id, player_name: name }));
        }
    }

    best.map(|(_, ship)| ship)
}

/// The ships drawn in a frame, by player name.
pub fn drawn_ships(commands: &[DrawCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Ship { player_name, .. } | DrawCommand::DeadShip { player_name, .. } => player_name.clone(),
            _ => None,
        })
        .collect()
}

/// Whether the reader has asked for this ship's trail or this range.
///
/// The global switches decide whether a layer is drawn at all; this decides
/// whose, which is what the per-ship menu changes.
pub fn per_ship_allows(
    command: &DrawCommand,
    trail_hidden: &HashSet<String>,
    ship_ranges: &HashMap<String, ShipConfigFilter>,
) -> bool {
    match command {
        DrawCommand::PositionTrail { player_name, .. } => {
            player_name.as_ref().is_none_or(|name| !trail_hidden.contains(name))
        }
        DrawCommand::ShipConfigCircle { player_name, kind, .. } => {
            ship_ranges.get(player_name).is_some_and(|filter| filter.is_enabled(kind))
        }
        _ => true,
    }
}
