//! The armor and hull visibility trees as the options rail shows them, with
//! the tri-state each row's checkbox reads.

use std::collections::HashMap;

use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use wows_toolkit_armor::load_ship::ArmorZone;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::load_ship::ZonePart;
use wows_toolkit_armor::visibility::PartKey;
use wows_toolkit_armor::visibility::TriState;
use wows_toolkit_armor::visibility::hull_group_all_on;
use wows_toolkit_armor::visibility::hull_group_any_on;
use wows_toolkit_armor::visibility::part_any_plate_hidden;
use wows_toolkit_armor::visibility::part_on;
use wows_toolkit_armor::visibility::plate_explicitly_hidden;
use wows_toolkit_armor::visibility::zone_all_on;
use wows_toolkit_armor::visibility::zone_any_on;
use wows_toolkit_viewmodel::ship_catalog::translate_material_key;
use wows_toolkit_viewmodel::ship_catalog::translate_name;
use wowsunpack::export::gltf_export::thickness_to_color;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Species;

/// A tree checkbox: off, on, or on for some of what it covers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Check {
    #[default]
    Off,
    Partial,
    On,
}

impl From<TriState> for Check {
    fn from(state: TriState) -> Self {
        match state {
            TriState::Off => Self::Off,
            TriState::Partial => Self::Partial,
            TriState::On => Self::On,
        }
    }
}

/// One plate thickness of a part.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PlateRow {
    pub tenths: i32,
    /// `"25 mm"`.
    pub label: String,
    pub color: String,
    pub checked: bool,
}

/// One material of a zone.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PartRow {
    pub name: String,
    pub label: String,
    pub state: Check,
    /// At most one thickness is shown, so the row is a plain checkbox.
    pub single: bool,
    pub plates: Vec<PlateRow>,
}

/// One armor zone.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ZoneRow {
    pub name: String,
    pub label: String,
    pub state: Check,
    pub parts: Vec<PartRow>,
}

/// One hull mesh.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct HullMeshRow {
    pub name: String,
    pub visible: bool,
}

/// One hull part group.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct HullGroupRow {
    pub name: String,
    pub state: Check,
    pub meshes: Vec<HullMeshRow>,
}

/// How the tree names zones and materials: the game's own names where the
/// build is at hand, the raw keys where it is not.
#[derive(Clone, Copy)]
pub(super) struct TreeNames<'a> {
    pub metadata: Option<&'a GameMetadataProvider>,
    pub species: Option<Species>,
}

impl TreeNames<'_> {
    pub fn zone(&self, name: &str) -> String {
        match self.metadata {
            Some(metadata) => translate_name(metadata, name),
            None => name.to_owned(),
        }
    }

    pub fn part(&self, key: &str) -> String {
        match self.metadata {
            Some(metadata) => translate_material_key(metadata, key, self.species),
            None => key.to_owned(),
        }
    }
}

/// The armor tree. 0 mm plates are left out unless `show_zero_mm`.
pub(super) fn zone_rows(
    zones: &[ArmorZone],
    part_visibility: &HashMap<PartKey, bool>,
    plate_visibility: &HashMap<PlateKey, bool>,
    show_zero_mm: bool,
    names: TreeNames<'_>,
) -> Vec<ZoneRow> {
    zones
        .iter()
        .map(|zone| {
            let all_on = zone_all_on(zone, part_visibility, plate_visibility, show_zero_mm);
            let any_on = zone_any_on(zone, part_visibility);
            ZoneRow {
                name: zone.name.clone(),
                label: names.zone(&zone.name),
                state: TriState::from_all_any(all_on, any_on).into(),
                parts: zone
                    .parts
                    .iter()
                    .map(|part| part_row(&zone.name, part, part_visibility, plate_visibility, show_zero_mm, names))
                    .collect(),
            }
        })
        .collect()
}

fn part_row(
    zone: &str,
    part: &ZonePart,
    part_visibility: &HashMap<PartKey, bool>,
    plate_visibility: &HashMap<PlateKey, bool>,
    show_zero_mm: bool,
    names: TreeNames<'_>,
) -> PartRow {
    let visible = part_on(part_visibility, &PartKey { zone: zone.to_owned(), part: part.name.clone() });
    let any_plate_hidden = part_any_plate_hidden(zone, part, plate_visibility, show_zero_mm);
    let thicknesses = shown_thicknesses(part, show_zero_mm);
    let single = thicknesses.len() <= 1;
    // A single checkbox is either fully on or off; a header is partial while
    // the part is on with some plate hidden.
    let state = if single {
        let checked = visible && !any_plate_hidden;
        TriState::from_all_any(checked, checked)
    } else {
        TriState::from_all_any(visible && !any_plate_hidden, visible)
    };
    let plates = thicknesses
        .into_iter()
        .map(|tenths| {
            let key = PlateKey { zone: zone.to_owned(), material_name: part.name.clone(), thickness_tenths: tenths };
            let mm = tenths as f32 / 10.0;
            PlateRow {
                tenths,
                label: format!("{mm:.0} mm"),
                color: hex_color(thickness_to_color(mm)),
                checked: visible && !plate_explicitly_hidden(plate_visibility, &key),
            }
        })
        .collect();

    PartRow { name: part.name.clone(), label: names.part(&part.name), state: state.into(), single, plates }
}

/// The thicknesses a part row lists, which are also the plates its checkbox
/// clears overrides for.
pub(super) fn shown_thicknesses(part: &ZonePart, show_zero_mm: bool) -> Vec<i32> {
    part.plates.iter().copied().filter(|&tenths| show_zero_mm || tenths != 0).collect()
}

/// The hull tree.
pub(super) fn hull_rows(
    groups: &[(String, Vec<String>)],
    hull_visibility: &HashMap<String, bool>,
) -> Vec<HullGroupRow> {
    groups
        .iter()
        .map(|(group, meshes)| {
            let all_on = hull_group_all_on(hull_visibility, meshes);
            let any_on = hull_group_any_on(hull_visibility, meshes);
            HullGroupRow {
                name: group.clone(),
                state: TriState::from_all_any(all_on, any_on).into(),
                meshes: meshes
                    .iter()
                    .map(|name| HullMeshRow {
                        name: name.clone(),
                        visible: hull_visibility.get(name).copied().unwrap_or(false),
                    })
                    .collect(),
            }
        })
        .collect()
}

/// `"#rrggbb"` of a linear 0..1 RGBA colour, alpha dropped.
pub(super) fn hex_color(color: [f32; 4]) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(color[0]), channel(color[1]), channel(color[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: TreeNames<'static> = TreeNames { metadata: None, species: None };

    fn zone() -> ArmorZone {
        ArmorZone {
            name: "Citadel".to_owned(),
            parts: vec![
                ZonePart { name: "Belt".to_owned(), plates: vec![0, 250, 4100] },
                ZonePart { name: "Deck".to_owned(), plates: vec![600] },
            ],
        }
    }

    fn plate(material: &str, tenths: i32) -> PlateKey {
        PlateKey { zone: "Citadel".to_owned(), material_name: material.to_owned(), thickness_tenths: tenths }
    }

    fn rows(parts: &[(&str, bool)], hidden: &[PlateKey], show_zero_mm: bool) -> Vec<ZoneRow> {
        let parts: HashMap<PartKey, bool> = parts
            .iter()
            .map(|(part, on)| (PartKey { zone: "Citadel".to_owned(), part: (*part).to_owned() }, *on))
            .collect();
        let plates: HashMap<PlateKey, bool> = hidden.iter().map(|key| (key.clone(), true)).collect();
        zone_rows(&[zone()], &parts, &plates, show_zero_mm, NAMES)
    }

    #[test]
    fn nothing_hidden_reads_on_throughout() {
        let rows = rows(&[], &[], false);
        let zone = &rows[0];

        assert_eq!(zone.state, Check::On);
        assert_eq!(zone.parts[0].state, Check::On);
        assert!(!zone.parts[0].single);
        assert_eq!(zone.parts[1].state, Check::On);
        assert!(zone.parts[1].single);
        assert!(zone.parts[0].plates.iter().all(|plate| plate.checked));
    }

    #[test]
    fn a_hidden_plate_makes_its_part_and_zone_partial() {
        let rows = rows(&[], &[plate("Belt", 250)], false);
        let belt = &rows[0].parts[0];

        assert_eq!(rows[0].state, Check::Partial);
        assert_eq!(belt.state, Check::Partial);
        assert_eq!(belt.plates.iter().map(|plate| plate.checked).collect::<Vec<_>>(), [false, true]);
    }

    #[test]
    fn a_part_turned_off_is_off_and_its_zone_partial() {
        let rows = rows(&[("Deck", false)], &[], false);

        assert_eq!(rows[0].state, Check::Partial);
        assert_eq!(rows[0].parts[1].state, Check::Off);
    }

    #[test]
    fn every_part_off_turns_the_zone_off() {
        let rows = rows(&[("Belt", false), ("Deck", false)], &[], false);

        assert_eq!(rows[0].state, Check::Off);
        assert!(rows[0].parts[0].plates.iter().all(|plate| !plate.checked));
    }

    #[test]
    fn zero_mm_plates_are_listed_only_when_asked_for() {
        let hidden = rows(&[], &[], false);
        let shown = rows(&[], &[], true);

        assert_eq!(hidden[0].parts[0].plates.iter().map(|plate| plate.tenths).collect::<Vec<_>>(), [250, 4100]);
        assert_eq!(shown[0].parts[0].plates.iter().map(|plate| plate.tenths).collect::<Vec<_>>(), [0, 250, 4100]);
        assert_eq!(shown[0].parts[0].plates[2].label, "410 mm");
    }

    #[test]
    fn a_hull_group_is_partial_when_some_meshes_show() {
        let groups = vec![("Hull".to_owned(), vec!["a".to_owned(), "b".to_owned()])];
        let none = hull_rows(&groups, &HashMap::new());
        let some = hull_rows(&groups, &HashMap::from([("a".to_owned(), true)]));
        let all = hull_rows(&groups, &HashMap::from([("a".to_owned(), true), ("b".to_owned(), true)]));

        assert_eq!(none[0].state, Check::Off);
        assert_eq!(some[0].state, Check::Partial);
        assert_eq!(some[0].meshes.iter().map(|mesh| mesh.visible).collect::<Vec<_>>(), [true, false]);
        assert_eq!(all[0].state, Check::On);
    }

    #[test]
    fn a_colour_is_written_as_hex() {
        assert_eq!(hex_color([1.0, 0.5, 0.0, 0.3]), "#ff8000");
    }
}
