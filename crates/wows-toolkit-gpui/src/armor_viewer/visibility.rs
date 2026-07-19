//! Visibility override state shared by the armor-visibility popover
//! (`popover.rs`) and the mesh uploaders (`upload.rs`, `picking_ui.rs`):
//! the two override maps threaded through a triangle-visibility check
//! (`VisibilityFilter`), the undo/redo snapshot type (`VisibilitySnapshot`,
//! `VisibilityUndoStack` -- ports `armor_viewer::state.rs:115-159` verbatim),
//! the sidebar-hover highlight key (`SidebarHighlightKey` -- ports the
//! Zone/Part/Plate variants of `armor_viewer::state::SidebarHighlightKey`;
//! `HullMeshes`/`SplashBoxes` are M4/deferred and intentionally omitted),
//! and the pure tri-state derivation the popover's tree renders from.
//!
//! **Convention note.** `part_visibility` uses the same boolean sense the
//! egui app does: a present value is "is this part visible", absent means
//! visible (default `true`). `plate_visibility` keeps this port's own Task 6
//! sense instead: a present value is "is this plate explicitly HIDDEN",
//! absent means not hidden (still default-visible). `upload.rs`'s
//! `plate_is_visible` already committed to that shape, and changing it would
//! flip the meaning of every existing `#[test]` there. The two maps are
//! therefore mirror images of each other's boolean sense; the tri-state
//! helpers below (ported from `armor_viewer::ui::tab::draw_armor_visibility_popover`,
//! `tab.rs:4403-4611`) translate egui's `plate_visibility.get(pk) == Some(false)`
//! (explicitly hidden, in egui's sense) to this port's `== Some(true)`
//! (explicitly hidden, in this port's sense) -- same meaning, flipped literal.

use std::collections::HashMap;

use super::load_ship::ArmorZone;
use super::load_ship::PlateKey;
use super::load_ship::ZonePart;

/// Borrowed view over the two visibility override maps, threaded through
/// mesh uploads (`upload.rs`, `picking_ui.rs`) so a triangle's visibility can
/// be resolved without cloning either map.
#[derive(Clone, Copy)]
pub struct VisibilityFilter<'a> {
    pub part: &'a HashMap<(String, String), bool>,
    pub plate: &'a HashMap<PlateKey, bool>,
}

/// Snapshot of visibility state for undo/redo. Ports
/// `armor_viewer::state::VisibilitySnapshot` verbatim.
#[derive(Clone, Default)]
pub struct VisibilitySnapshot {
    pub part_visibility: HashMap<(String, String), bool>,
    pub plate_visibility: HashMap<PlateKey, bool>,
}

/// Simple undo/redo stack for visibility changes. Ports
/// `armor_viewer::state::VisibilityUndoStack` verbatim.
#[derive(Default)]
pub struct VisibilityUndoStack {
    undo: Vec<VisibilitySnapshot>,
    redo: Vec<VisibilitySnapshot>,
}

impl VisibilityUndoStack {
    const MAX_ENTRIES: usize = 50;

    /// Push current state before a mutation. Clears the redo stack.
    pub fn push(&mut self, snapshot: VisibilitySnapshot) {
        self.undo.push(snapshot);
        if self.undo.len() > Self::MAX_ENTRIES {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Undo: returns the previous snapshot, pushing current state onto redo.
    pub fn undo(&mut self, current: VisibilitySnapshot) -> Option<VisibilitySnapshot> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    /// Redo: returns the next snapshot, pushing current state onto undo.
    pub fn redo(&mut self, current: VisibilitySnapshot) -> Option<VisibilitySnapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

/// Identifies what the user is hovering in the visibility popover, for the
/// sidebar-hover highlight overlay (`popover.rs`). Ports the Zone/Part/Plate
/// variants of `armor_viewer::state::SidebarHighlightKey`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarHighlightKey {
    /// All visible armor triangles in a zone.
    Zone(String),
    /// All visible armor triangles for a (zone, material/part).
    Part(String, String),
    /// A specific plate by (zone, material, thickness_i32).
    Plate(PlateKey),
}

/// Whether `(zone, part)` is visible per `part_visibility` (absent = visible).
pub(crate) fn part_on(part_visibility: &HashMap<(String, String), bool>, zone: &str, part: &str) -> bool {
    part_visibility.get(&(zone.to_string(), part.to_string())).copied().unwrap_or(true)
}

/// Whether `key` is explicitly hidden in `plate_visibility` (this port's
/// sense: present + `true` = hidden, absent = visible). See the module doc's
/// convention note.
pub(crate) fn plate_explicitly_hidden(plate_visibility: &HashMap<PlateKey, bool>, key: &PlateKey) -> bool {
    plate_visibility.get(key).copied().unwrap_or(false)
}

/// Visible plate thicknesses for `part` (tenths of mm), honoring `show_zero_mm`.
fn visible_thicknesses(part: &ZonePart, show_zero_mm: bool) -> impl Iterator<Item = i32> + '_ {
    part.plates.iter().copied().filter(move |&t| show_zero_mm || t != 0)
}

/// Whether every part in `zone` is on, AND no plate in the zone is explicitly
/// hidden. Ports the `zone_all_on` local from `draw_armor_visibility_popover`
/// (`tab.rs:4455-4464`).
pub(crate) fn zone_all_on(
    zone: &ArmorZone,
    part_visibility: &HashMap<(String, String), bool>,
    plate_visibility: &HashMap<PlateKey, bool>,
    show_zero_mm: bool,
) -> bool {
    zone.parts.iter().all(|p| {
        if !part_on(part_visibility, &zone.name, &p.name) {
            return false;
        }
        !visible_thicknesses(p, show_zero_mm)
            .any(|t| plate_explicitly_hidden(plate_visibility, &(zone.name.clone(), p.name.clone(), t)))
    })
}

/// Whether any part in `zone` is on. Ports the `zone_any_on` local from
/// `draw_armor_visibility_popover` (`tab.rs:4465-4468`).
pub(crate) fn zone_any_on(zone: &ArmorZone, part_visibility: &HashMap<(String, String), bool>) -> bool {
    zone.parts.iter().any(|p| part_on(part_visibility, &zone.name, &p.name))
}

/// Whether any of `part`'s visible-thickness plates are explicitly hidden.
/// Ports the `any_plate_hidden` local from `draw_armor_visibility_popover`
/// (`tab.rs:4523-4526`).
pub(crate) fn part_any_plate_hidden(
    zone: &str,
    part: &ZonePart,
    plate_visibility: &HashMap<PlateKey, bool>,
    show_zero_mm: bool,
) -> bool {
    visible_thicknesses(part, show_zero_mm)
        .any(|t| plate_explicitly_hidden(plate_visibility, &(zone.to_string(), part.name.clone(), t)))
}

/// A checkbox's on/off/indeterminate state, driving both the checked value
/// and whether the popover draws the partial-state dash.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TriState {
    Off,
    Partial,
    On,
}

impl TriState {
    pub(crate) fn from_all_any(all_on: bool, any_on: bool) -> Self {
        if all_on {
            TriState::On
        } else if any_on {
            TriState::Partial
        } else {
            TriState::Off
        }
    }

    pub(crate) fn checked(self) -> bool {
        self == TriState::On
    }

    pub(crate) fn partial(self) -> bool {
        self == TriState::Partial
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(name: &str, parts: Vec<(&str, Vec<i32>)>) -> ArmorZone {
        ArmorZone {
            name: name.to_string(),
            parts: parts.into_iter().map(|(n, plates)| ZonePart { name: n.to_string(), plates }).collect(),
        }
    }

    #[test]
    fn undo_stack_round_trips_a_single_mutation() {
        let mut stack = VisibilityUndoStack::default();
        let before = VisibilitySnapshot::default();
        let mut after = VisibilitySnapshot::default();
        after.part_visibility.insert(("Hull".to_string(), "Belt".to_string()), false);

        stack.push(before.clone());
        let undone = stack.undo(after.clone()).expect("expected an undo entry");
        assert!(undone.part_visibility.is_empty());

        let redone = stack.redo(undone).expect("expected a redo entry");
        assert_eq!(redone.part_visibility, after.part_visibility);
    }

    #[test]
    fn undo_stack_caps_at_max_entries() {
        let mut stack = VisibilityUndoStack::default();
        for i in 0..(VisibilityUndoStack::MAX_ENTRIES + 10) {
            let mut snap = VisibilitySnapshot::default();
            snap.part_visibility.insert(("Zone".to_string(), format!("P{i}")), true);
            stack.push(snap);
        }
        assert_eq!(stack.undo.len(), VisibilityUndoStack::MAX_ENTRIES);
        // The oldest entries should have been dropped: the remaining stack's
        // bottom entry is the 11th pushed (index 10), not the 1st.
        assert_eq!(stack.undo[0].part_visibility.len(), 1);
        assert!(stack.undo[0].part_visibility.contains_key(&("Zone".to_string(), "P10".to_string())));
    }

    #[test]
    fn undo_returns_none_when_stack_is_empty() {
        let mut stack = VisibilityUndoStack::default();
        assert!(stack.undo(VisibilitySnapshot::default()).is_none());
    }

    #[test]
    fn push_clears_the_redo_stack() {
        let mut stack = VisibilityUndoStack::default();
        stack.push(VisibilitySnapshot::default());
        let mut later = VisibilitySnapshot::default();
        later.part_visibility.insert(("A".to_string(), "B".to_string()), false);
        stack.undo(later).unwrap();
        assert!(!stack.redo.is_empty());

        stack.push(VisibilitySnapshot::default());
        assert!(stack.redo.is_empty(), "pushing a new mutation should clear any pending redo");
    }

    #[test]
    fn part_on_defaults_to_visible_when_absent() {
        let map = HashMap::new();
        assert!(part_on(&map, "Citadel", "Cit_Belt"));
    }

    #[test]
    fn part_on_respects_an_explicit_false() {
        let mut map = HashMap::new();
        map.insert(("Citadel".to_string(), "Cit_Belt".to_string()), false);
        assert!(!part_on(&map, "Citadel", "Cit_Belt"));
    }

    #[test]
    fn zone_all_on_is_true_with_no_overrides() {
        let z = zone("Citadel", vec![("Cit_Belt", vec![320]), ("Cit_Deck", vec![200])]);
        assert!(zone_all_on(&z, &HashMap::new(), &HashMap::new(), false));
        assert!(zone_any_on(&z, &HashMap::new()));
    }

    #[test]
    fn zone_all_on_is_false_when_a_part_is_off() {
        let z = zone("Citadel", vec![("Cit_Belt", vec![320]), ("Cit_Deck", vec![200])]);
        let mut part_visibility = HashMap::new();
        part_visibility.insert(("Citadel".to_string(), "Cit_Deck".to_string()), false);
        assert!(!zone_all_on(&z, &part_visibility, &HashMap::new(), false));
        // The other part is still on, so the zone isn't fully off either.
        assert!(zone_any_on(&z, &part_visibility));
    }

    #[test]
    fn zone_all_on_is_false_when_a_plate_is_explicitly_hidden() {
        let z = zone("Citadel", vec![("Cit_Belt", vec![320])]);
        let mut plate_visibility = HashMap::new();
        plate_visibility.insert(("Citadel".to_string(), "Cit_Belt".to_string(), 320), true);
        assert!(!zone_all_on(&z, &HashMap::new(), &plate_visibility, false));
        // The part itself is still on (no part-level override), so any_on stays true.
        assert!(zone_any_on(&z, &HashMap::new()));
    }

    #[test]
    fn zone_any_on_is_false_only_when_every_part_is_off() {
        let z = zone("Citadel", vec![("Cit_Belt", vec![320]), ("Cit_Deck", vec![200])]);
        let mut part_visibility = HashMap::new();
        part_visibility.insert(("Citadel".to_string(), "Cit_Belt".to_string()), false);
        part_visibility.insert(("Citadel".to_string(), "Cit_Deck".to_string()), false);
        assert!(!zone_any_on(&z, &part_visibility));
    }

    #[test]
    fn zone_all_on_ignores_zero_mm_plates_unless_shown() {
        let z = zone("Hull", vec![("Trans", vec![0])]);
        let mut plate_visibility = HashMap::new();
        plate_visibility.insert(("Hull".to_string(), "Trans".to_string(), 0), true);
        // The only plate is 0mm and hidden by default, so it's excluded from
        // the "any plate hidden" check regardless of its own override.
        assert!(zone_all_on(&z, &HashMap::new(), &plate_visibility, false));
    }

    #[test]
    fn part_any_plate_hidden_detects_one_hidden_layer() {
        let part = ZonePart { name: "Cit_Belt".to_string(), plates: vec![320, 200] };
        let mut plate_visibility = HashMap::new();
        plate_visibility.insert(("Citadel".to_string(), "Cit_Belt".to_string(), 200), true);
        assert!(part_any_plate_hidden("Citadel", &part, &plate_visibility, false));
    }

    #[test]
    fn part_any_plate_hidden_is_false_with_no_overrides() {
        let part = ZonePart { name: "Cit_Belt".to_string(), plates: vec![320] };
        assert!(!part_any_plate_hidden("Citadel", &part, &HashMap::new(), false));
    }

    #[test]
    fn tri_state_derives_correctly() {
        assert_eq!(TriState::from_all_any(true, true), TriState::On);
        assert_eq!(TriState::from_all_any(false, true), TriState::Partial);
        assert_eq!(TriState::from_all_any(false, false), TriState::Off);
        assert!(TriState::from_all_any(true, true).checked());
        assert!(TriState::from_all_any(false, true).partial());
        assert!(!TriState::from_all_any(true, true).partial());
    }
}
