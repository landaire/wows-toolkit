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

/// Clears any `plate_visibility` override for `(zone, part, thickness)` for
/// each of `thicknesses`, on a part checkbox toggle. Callers pass only the
/// `show_zero_mm`-filtered thickness set (not every plate on the part), so an
/// override on a thickness currently filtered out by `show_zero_mm` (e.g. a
/// hidden 0mm plate while the filter is off) survives the toggle. Ports the
/// egui original's clear scope (`tab.rs:4538-4540`, `4565-4567`).
pub(crate) fn clear_plate_overrides(
    plate_visibility: &mut HashMap<PlateKey, bool>,
    zone: &str,
    part: &str,
    thicknesses: &[i32],
) {
    for &t in thicknesses {
        plate_visibility.remove(&(zone.to_string(), part.to_string(), t));
    }
}

/// Whether every named hull mesh in `names` is visible per `hull_visibility`
/// (absent/false = hidden, matching `upload_hull::upload_hull_meshes`'s own
/// default). Drives the hull popover's group-checkbox tri-state
/// (`popover.rs`); ports the `group_all_on`/`group_any_on` locals from
/// `draw_hull_visibility_popover` (`tab.rs:3273-3274`).
pub(crate) fn hull_group_all_on(hull_visibility: &HashMap<String, bool>, names: &[String]) -> bool {
    names.iter().all(|n| hull_visibility.get(n).copied().unwrap_or(false))
}

/// Whether any named hull mesh in `names` is visible per `hull_visibility`.
/// See [`hull_group_all_on`]'s doc.
pub(crate) fn hull_group_any_on(hull_visibility: &HashMap<String, bool>, names: &[String]) -> bool {
    names.iter().any(|n| hull_visibility.get(n).copied().unwrap_or(false))
}

/// Drops `hull_visibility` entries for mesh names no longer present in
/// `hull_part_groups` (a Milestone 4 Task 8c hull-upgrade/LOD reload changed
/// the mesh set), then inserts a default entry for every mesh name that
/// doesn't already have one. The default is "on" if any surviving entry was
/// on, else "off" -- matching the egui app's own `apply_hull_reload`/
/// `apply_upgrade_reload` retain-plus-default-new-parts logic
/// (`armor_viewer/ui/tab.rs:2547-2554`, `2737-2743`). The default-fill (not
/// just a retain) is necessary here specifically because `hull_visibility`'s
/// convention is absent-means-HIDDEN (see the module doc's convention note,
/// which is about `part_visibility`/`plate_visibility`, but `hull_visibility`
/// carries the same opposite-of-`part_visibility` default too -- a newly
/// appeared hull mesh name would otherwise silently stay hidden even when
/// every other hull part is on).
pub(crate) fn retain_hull_visibility(
    hull_visibility: &mut HashMap<String, bool>,
    hull_part_groups: &[(String, Vec<String>)],
) {
    let default_on = hull_visibility.values().any(|&v| v);
    hull_visibility.retain(|name, _| hull_part_groups.iter().any(|(_, names)| names.contains(name)));
    for (_group, names) in hull_part_groups {
        for name in names {
            hull_visibility.entry(name.clone()).or_insert(default_on);
        }
    }
}

/// Drops `part_visibility` entries for `(zone, material)` pairs no longer
/// present in `zone_parts` (post-reload). No default-fill needed here, unlike
/// [`retain_hull_visibility`]: `part_visibility`'s absent-means-visible
/// convention already makes a newly appeared part visible with no entry at
/// all, matching the egui app's own `apply_upgrade_reload`
/// (`tab.rs:2719-2723`), whose explicit `or_insert(true)` fill is a no-op
/// under this port's opposite default-boolean convention (see the module
/// doc's convention note).
pub(crate) fn retain_part_visibility(
    part_visibility: &mut HashMap<(String, String), bool>,
    zone_parts: &[(String, Vec<String>)],
) {
    part_visibility.retain(|(zone, part), _| zone_parts.iter().any(|(z, parts)| z == zone && parts.contains(part)));
}

/// Drops `plate_visibility` entries for plate keys no longer present in
/// `zone_part_plates` (post-reload). Same no-default-fill rationale as
/// [`retain_part_visibility`]; matches the egui app's own `apply_upgrade_reload`
/// plate retain (`tab.rs:2725-2729`), which likewise has no default-fill.
pub(crate) fn retain_plate_visibility(plate_visibility: &mut HashMap<PlateKey, bool>, zone_part_plates: &[ArmorZone]) {
    plate_visibility.retain(|(zone, part, thickness), _| {
        zone_part_plates
            .iter()
            .any(|z| z.name == *zone && z.parts.iter().any(|p| p.name == *part && p.plates.contains(thickness)))
    });
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
    fn clear_plate_overrides_leaves_thicknesses_outside_the_given_set_untouched() {
        let mut plate_visibility = HashMap::new();
        plate_visibility.insert(("Citadel".to_string(), "Cit_Belt".to_string(), 0), true);
        plate_visibility.insert(("Citadel".to_string(), "Cit_Belt".to_string(), 320), true);

        // Toggling the part's checkbox while `show_zero_mm` is off passes only
        // the filtered (non-zero) thickness set, matching `render_part_row`'s
        // `visible_plates`.
        clear_plate_overrides(&mut plate_visibility, "Citadel", "Cit_Belt", &[320]);

        assert!(!plate_visibility.contains_key(&("Citadel".to_string(), "Cit_Belt".to_string(), 320)));
        assert!(
            plate_visibility.contains_key(&("Citadel".to_string(), "Cit_Belt".to_string(), 0)),
            "a 0mm override should survive a show_zero_mm-filtered toggle"
        );
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

    #[test]
    fn hull_group_all_on_requires_every_named_mesh_visible() {
        let mut vis = HashMap::new();
        vis.insert("Hull_A".to_string(), true);
        vis.insert("Hull_B".to_string(), false);
        let names = vec!["Hull_A".to_string(), "Hull_B".to_string()];
        assert!(!hull_group_all_on(&vis, &names));
        assert!(hull_group_any_on(&vis, &names));
    }

    #[test]
    fn hull_group_all_on_is_true_when_every_named_mesh_is_visible() {
        let mut vis = HashMap::new();
        vis.insert("Hull_A".to_string(), true);
        vis.insert("Hull_B".to_string(), true);
        let names = vec!["Hull_A".to_string(), "Hull_B".to_string()];
        assert!(hull_group_all_on(&vis, &names));
        assert!(hull_group_any_on(&vis, &names));
    }

    #[test]
    fn hull_group_any_on_is_false_when_every_named_mesh_is_hidden_or_absent() {
        let vis = HashMap::new();
        let names = vec!["Hull_A".to_string(), "Hull_B".to_string()];
        assert!(!hull_group_all_on(&vis, &names));
        assert!(!hull_group_any_on(&vis, &names));
    }

    #[test]
    fn retain_hull_visibility_drops_meshes_no_longer_present() {
        let mut vis = HashMap::new();
        vis.insert("Turret_A".to_string(), true);
        vis.insert("Turret_B_stale".to_string(), true);
        let groups = vec![("Main Battery".to_string(), vec!["Turret_A".to_string()])];
        retain_hull_visibility(&mut vis, &groups);
        assert!(!vis.contains_key("Turret_B_stale"), "a mesh not in the new hull's groups must be dropped");
        assert_eq!(vis.get("Turret_A"), Some(&true), "a surviving mesh's existing value must be preserved");
    }

    #[test]
    fn retain_hull_visibility_defaults_new_meshes_on_when_any_surviving_entry_was_on() {
        let mut vis = HashMap::new();
        vis.insert("Turret_A".to_string(), true);
        let groups = vec![("Main Battery".to_string(), vec!["Turret_A".to_string(), "Turret_B_new".to_string()])];
        retain_hull_visibility(&mut vis, &groups);
        assert_eq!(vis.get("Turret_B_new"), Some(&true), "a new mesh should default on when the user had any on");
    }

    #[test]
    fn retain_hull_visibility_defaults_new_meshes_off_when_nothing_was_on() {
        let mut vis = HashMap::new();
        vis.insert("Turret_A".to_string(), false);
        let groups = vec![("Main Battery".to_string(), vec!["Turret_A".to_string(), "Turret_B_new".to_string()])];
        retain_hull_visibility(&mut vis, &groups);
        assert_eq!(vis.get("Turret_B_new"), Some(&false), "a new mesh should default off when nothing was on");
    }

    #[test]
    fn retain_part_visibility_drops_parts_no_longer_present_and_keeps_survivors() {
        let mut vis = HashMap::new();
        vis.insert(("Citadel".to_string(), "Cit_Belt".to_string()), false);
        vis.insert(("Bow".to_string(), "Bow_Stale".to_string()), false);
        let zone_parts = vec![("Citadel".to_string(), vec!["Cit_Belt".to_string()])];
        retain_part_visibility(&mut vis, &zone_parts);
        assert!(!vis.contains_key(&("Bow".to_string(), "Bow_Stale".to_string())));
        assert_eq!(vis.get(&("Citadel".to_string(), "Cit_Belt".to_string())), Some(&false));
    }

    #[test]
    fn retain_plate_visibility_drops_plates_no_longer_present_and_keeps_survivors() {
        let mut vis = HashMap::new();
        vis.insert(("Citadel".to_string(), "Cit_Belt".to_string(), 320), true);
        vis.insert(("Citadel".to_string(), "Cit_Belt".to_string(), 999), true);
        let zone_part_plates = vec![zone("Citadel", vec![("Cit_Belt", vec![320])])];
        retain_plate_visibility(&mut vis, &zone_part_plates);
        assert!(!vis.contains_key(&("Citadel".to_string(), "Cit_Belt".to_string(), 999)));
        assert_eq!(vis.get(&("Citadel".to_string(), "Cit_Belt".to_string(), 320)), Some(&true));
    }
}
