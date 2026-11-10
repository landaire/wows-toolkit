//! The game GUI icons the player table shows, and where each is read from in
//! a build's files. Decoding and display are the front end's.

use std::collections::HashSet;

use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::types::CrewSkillName;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::model::PlayerRow;

/// One icon a row shows besides its ship class.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowIcon {
    /// By the achievement's icon key.
    Achievement(String),
    /// By the ribbon's icon key.
    Ribbon(String),
    /// By the subribbon's icon key, without the `sub` prefix its file has.
    SubRibbon(String),
    /// By the consumable's icon key.
    Consumable(String),
    /// By the modernization's game-params name.
    Modernization(String),
    /// By the signal's game-params name.
    Signal(String),
    Skill(CrewSkillName),
}

impl RowIcon {
    pub fn achievement(icon_key: &str) -> Self {
        Self::Achievement(icon_key.to_owned())
    }

    /// A ribbon or subribbon, as the result says which.
    pub fn ribbon(is_subribbon: bool, icon_key: &str) -> Self {
        if is_subribbon { Self::SubRibbon(icon_key.to_owned()) } else { Self::Ribbon(icon_key.to_owned()) }
    }

    /// The icon's name in a front end's cache: `"<kind>:<id>"`.
    pub fn cache_key(&self) -> String {
        match self {
            Self::Achievement(key) => format!("achievement:{key}"),
            Self::Ribbon(key) => format!("ribbon:{key}"),
            Self::SubRibbon(key) => format!("subribbon:{key}"),
            Self::Consumable(key) => format!("consumable:{key}"),
            Self::Modernization(name) => format!("modernization:{name}"),
            Self::Signal(name) => format!("signal:{name}"),
            Self::Skill(name) => format!("skill:{}", name.as_str()),
        }
    }

    /// The icon's PNG bytes from `vfs`, or `None` when this build has no such
    /// file (an older client, or a key with no asset).
    pub fn read(&self, vfs: &VfsPath) -> Option<Vec<u8>> {
        match self {
            Self::Achievement(key) => GuiAsset::Achievement(key).read(vfs, None),
            Self::Ribbon(key) => GuiAsset::Ribbon(key).read(vfs, None),
            // The game stores subribbon icons under a `sub`-prefixed name
            // (`subribbons/submain_caliber.png`).
            Self::SubRibbon(key) => GuiAsset::SubRibbon(&format!("sub{key}")).read(vfs, None),
            Self::Consumable(key) => GuiAsset::Consumable(key).read(vfs, None),
            Self::Modernization(name) => GuiAsset::Modernization(name).read(vfs, None),
            Self::Signal(name) => GuiAsset::SignalFlag(name).read(vfs, None),
            Self::Skill(name) => GuiAsset::CrewSkill { name }.read(vfs, None),
        }
    }
}

/// Every icon `rows` show, each once, in the order the rows first name them.
pub fn row_icons(rows: &[PlayerRow]) -> Vec<RowIcon> {
    let mut seen = HashSet::new();
    let mut icons = Vec::new();
    let mut add = |icon: RowIcon| {
        if seen.insert(icon.clone()) {
            icons.push(icon);
        }
    };

    for row in rows {
        for achievement in &row.achievements {
            add(RowIcon::achievement(&achievement.icon_key));
        }
        for ribbon in &row.ribbons {
            add(RowIcon::ribbon(ribbon.is_subribbon, &ribbon.icon_key));
        }
        for consumable in &row.consumables {
            add(RowIcon::Consumable(consumable.icon_key.clone()));
        }

        let Some(build) = row.translated_build.as_ref() else {
            continue;
        };
        for slot in build.modernization_slots.iter().flatten() {
            add(RowIcon::Modernization(slot.game_params_name.clone()));
        }
        for signal in &build.signals {
            add(RowIcon::Signal(signal.game_params_name.clone()));
        }
        for skill in build.captain_skills.iter().flatten().flat_map(|tier| &tier.skills) {
            add(RowIcon::Skill(skill.internal_name.clone()));
        }
    }

    icons
}

/// The minimap class icon of a live ship, as SVG. The GUI draws it rotated a
/// quarter turn clockwise and tinted in the player's color.
pub fn ship_class_svg(species: Species, vfs: &VfsPath) -> Option<Vec<u8>> {
    GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive }.read(vfs, None)
}

#[cfg(test)]
mod tests {
    use wows_replay_insights::battle_report::RibbonResult;
    use wows_replays::types::Relation;
    use wowsunpack::vfs::PhysicalFS;

    use super::*;
    use crate::test_support::base_row;

    fn subribbon_row() -> PlayerRow {
        let mut row = base_row(1, Relation::new(0), true);
        row.ribbons.push(RibbonResult {
            name: "main_caliber".to_owned(),
            display_name: "Main Caliber Hit".to_owned(),
            description: String::new(),
            icon_key: "main_caliber".to_owned(),
            is_subribbon: true,
            count: 1,
        });
        row
    }

    #[test]
    fn a_row_with_nothing_to_show_asks_for_no_icons() {
        assert!(row_icons(&[base_row(1, Relation::new(0), true)]).is_empty());
    }

    #[test]
    fn an_icon_shared_by_two_rows_is_asked_for_once() {
        let rows = [subribbon_row(), subribbon_row()];
        assert_eq!(row_icons(&rows), vec![RowIcon::SubRibbon("main_caliber".to_owned())]);
    }

    /// A regression to the bare `icon_key` file name reads nothing here.
    #[test]
    fn a_subribbon_icon_is_read_from_its_sub_prefixed_file() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let subribbons = dir.path().join("gui").join("ribbons").join("subribbons");
        std::fs::create_dir_all(&subribbons).expect("the subribbon directory");
        std::fs::write(subribbons.join("submain_caliber.png"), b"icon").expect("the icon file");
        let vfs: VfsPath = PhysicalFS::new(dir.path()).into();

        let icons = row_icons(&[subribbon_row()]);

        assert_eq!(icons[0].cache_key(), "subribbon:main_caliber");
        assert_eq!(icons[0].read(&vfs).as_deref(), Some(b"icon".as_slice()));
    }
}
