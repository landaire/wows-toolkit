//! Version-aware GUI asset resolution.
//!
//! Consumers (the renderer, the replay inspector) request an asset by what it
//! *is* — e.g. [`GuiAsset::Consumable`] for `"PCY009_CrashCrewPremium"` — rather
//! than by where it lives. The resolver maps each request to candidate VFS
//! paths (newest layout first) for the build's version and returns the first
//! that exists, so callers never track how WoWs has moved assets between game
//! versions. Assets that simply don't exist in a build (e.g. the Flash-era
//! GUI, which has no per-file ribbon/ship icons) resolve to `None` and let the
//! caller degrade gracefully.

use std::io::Read;

use vfs::VfsPath;

use crate::game_params::types::Species;

/// Minimap ship-icon state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShipIconState {
    Alive,
    Dead,
    Invisible,
    LastVisible,
}

impl ShipIconState {
    fn suffix(self) -> &'static str {
        match self {
            ShipIconState::Alive => "",
            ShipIconState::Dead => "_dead",
            ShipIconState::Invisible => "_invisible",
            ShipIconState::LastVisible => "_last_visible",
        }
    }
}

/// Team relation for relation-keyed icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Ally,
    Enemy,
    Neutral,
}

impl Relation {
    fn as_str(self) -> &'static str {
        match self {
            Relation::Ally => "ally",
            Relation::Enemy => "enemy",
            Relation::Neutral => "neutral",
        }
    }
}

/// A GUI asset identified by what it represents, not by its file path.
#[derive(Clone, Copy, Debug)]
pub enum GuiAsset<'a> {
    /// Minimap class icon for another player's ship.
    ShipClassIcon { species: Species, state: ShipIconState },
    /// Minimap icon for the viewing player's own ship.
    SelfShipIcon { species: Species, alive: bool },
    /// Consumable icon by PCY identifier (e.g. `"PCY009_CrashCrewPremium"`).
    Consumable(&'a str),
    /// Achievement icon by lowercase key.
    Achievement(&'a str),
    /// Ribbon icon by name.
    Ribbon(&'a str),
    /// Sub-ribbon icon by name.
    SubRibbon(&'a str),
    /// Upgrade/modernization icon by file stem.
    Modernization(&'a str),
    /// Signal flag icon by file stem.
    SignalFlag(&'a str),
    /// Small nation flag by nation name.
    NationFlag(&'a str),
    /// Ship silhouette by ship index (e.g. `"PJSB018"`).
    ShipSilhouette(&'a str),
    /// Capture-point base flag by team relation.
    CapturePointFlag(Relation),
}

impl GuiAsset<'_> {
    /// Candidate VFS paths for this asset, highest-priority (newest layout)
    /// first. `build` lets the mapping branch when a path genuinely moved
    /// between game versions; today most assets share one path plus a fallback.
    pub fn candidate_paths(&self, build: u32) -> Vec<String> {
        // Reserved for build-gated path overrides, e.g.
        // `if build < N { vec![old_path] } else { vec![new_path] }`. Today every
        // asset resolves the same across modern builds (with in-order fallbacks),
        // so the parameter is not yet branched on.
        let _ = build;
        match *self {
            GuiAsset::ShipClassIcon { species, state } => {
                let s = species.name().to_ascii_lowercase();
                vec![format!("gui/fla/minimap/ship_icons/minimap_{s}{}.svg", state.suffix())]
            }
            GuiAsset::SelfShipIcon { species, alive } => {
                let s = species.name().to_ascii_lowercase();
                let life = if alive { "alive" } else { "dead" };
                vec![
                    format!("gui/fla/minimap/ship_icons_self/minimap_self_{life}_{s}.svg"),
                    format!("gui/fla/minimap/ship_icons_self/minimap_self_{life}.svg"),
                ]
            }
            GuiAsset::Consumable(pcy) => vec![format!("gui/consumables/consumable_{pcy}.png")],
            GuiAsset::Achievement(key) => vec![format!("gui/achievements/icon_achievement_{key}.png")],
            GuiAsset::Ribbon(name) => vec![format!("gui/ribbons/{name}.png")],
            GuiAsset::SubRibbon(name) => vec![format!("gui/ribbons/subribbons/{name}.png")],
            GuiAsset::Modernization(name) => vec![format!("gui/modernization_icons/{name}.png")],
            GuiAsset::SignalFlag(name) => vec![format!("gui/signal_flags/{name}.png")],
            GuiAsset::NationFlag(nation) => vec![format!("gui/nation_flags/tiny/flag_{nation}.png")],
            GuiAsset::ShipSilhouette(index) => vec![format!("gui/ships_silhouettes/{index}.png")],
            GuiAsset::CapturePointFlag(rel) => {
                vec![format!("gui/battle_hud/markers/capture_point/icon_base_{}_flag.png", rel.as_str())]
            }
        }
    }

    /// Resolve to the first candidate path that exists in `vfs`, or `None` when
    /// the asset isn't present in this build.
    pub fn resolve(&self, vfs: &VfsPath, build: u32) -> Option<VfsPath> {
        for path in self.candidate_paths(build) {
            if let Ok(entry) = vfs.join(&path)
                && entry.exists().unwrap_or(false)
            {
                return Some(entry);
            }
        }
        None
    }

    /// Read the asset's bytes from the first candidate path that exists.
    pub fn read(&self, vfs: &VfsPath, build: u32) -> Option<Vec<u8>> {
        let entry = self.resolve(vfs, build)?;
        let mut buf = Vec::new();
        entry.open_file().ok()?.read_to_end(&mut buf).ok()?;
        (!buf.is_empty()).then_some(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOME_BUILD: u32 = 11791718;

    #[test]
    fn self_icon_falls_back_to_generic() {
        let paths = GuiAsset::SelfShipIcon { species: Species::Destroyer, alive: true }.candidate_paths(SOME_BUILD);
        assert_eq!(paths[0], "gui/fla/minimap/ship_icons_self/minimap_self_alive_destroyer.svg");
        assert_eq!(paths[1], "gui/fla/minimap/ship_icons_self/minimap_self_alive.svg");
    }

    #[test]
    fn consumable_and_capture_point_paths() {
        assert_eq!(
            GuiAsset::Consumable("PCY009_CrashCrewPremium").candidate_paths(SOME_BUILD)[0],
            "gui/consumables/consumable_PCY009_CrashCrewPremium.png"
        );
        assert_eq!(
            GuiAsset::CapturePointFlag(Relation::Enemy).candidate_paths(SOME_BUILD)[0],
            "gui/battle_hud/markers/capture_point/icon_base_enemy_flag.png"
        );
    }
}
