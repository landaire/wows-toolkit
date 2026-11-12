//! Hover picking over plates, meshes, orbits and hardpoints, the highlights drawn for them, and
//! the part and plate visibility changes with their undo history.

use wows_toolkit_armor::camera_rings;
use wows_toolkit_armor::hull_picking;
use wows_toolkit_armor::hull_picking::HighlightChange;
use wows_toolkit_armor::hull_picking::HoverTarget;
use wows_toolkit_armor::hull_picking::MeshHit;
use wows_toolkit_armor::hull_picking::MeshTooltip;
use wows_toolkit_armor::load_ship::ArmorTriangleTooltip;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::plate_picking;
use wows_toolkit_armor::plate_picking::HighlightOrigin;
use wows_toolkit_armor::viewport::types::Vec2;
use wows_toolkit_armor::viewport::types::Vec3;
use wows_toolkit_armor::viewport::types::ViewRect;
use wows_toolkit_armor::visibility;
use wows_toolkit_armor::visibility::SidebarHighlightKey;
use wows_toolkit_armor::visibility::VisibilityFilter;
use wows_toolkit_armor::visibility::VisibilitySnapshot;

use super::model::HoverInfo;
use super::model::MeshHover;
use super::model::ViewportModel;
use super::state::ArmorViewportState;
use super::state::ViewCx;
use super::types::ViewportChange;

impl ViewportModel {
    /// CPU-picks at `position` and updates `self.hovered` or, when no plate is hit,
    /// `self.hovered_mesh` to match. Rebuilds a highlight overlay mesh only when the hovered plate
    /// or mesh name changed; the cursor is refreshed on every call so the tooltip tracks the
    /// pointer. Returns whether the caller should `cx.notify()`.
    pub(super) fn update_plate_hover(&mut self, position: Vec2) -> bool {
        let Some(rect) = self.last_bounds else { return self.clear_plate_hover() | self.clear_mesh_hover() };
        let Some(armor) = self.current_armor.clone() else {
            return self.clear_plate_hover() | self.clear_mesh_hover();
        };
        let pointer = position;
        let hit = self.viewport.pick(pointer, rect);
        let plate =
            hit.as_ref().and_then(|h| plate_picking::tooltip_for_hit(h, &self.mesh_triangle_info)).map(|tooltip| {
                let key = plate_picking::plate_key_of(tooltip);
                (key, tooltip.clone())
            });
        let (key, tooltip) = match HoverTarget::resolve(plate, || self.pick_mesh(&armor, pointer, rect)) {
            HoverTarget::Plate(plate) => plate,
            HoverTarget::Mesh(hit) => {
                self.clear_plate_hover();
                return self.set_mesh_hover(&armor, hit, position);
            }
            HoverTarget::Nothing => return self.clear_plate_hover() | self.clear_mesh_hover(),
        };
        self.clear_mesh_hover();

        let key_changed = self.hovered.as_ref().map(|h| h.key != key).unwrap_or(true);
        // Remembered past the hover itself: the penetration checker is opened
        // from a button, which the pointer has to leave the plate to reach.
        self.last_plate = Some((tooltip.zone.clone(), tooltip.thickness_mm));
        self.hovered = Some(HoverInfo { key: key.clone(), tooltip, cursor: position });
        if key_changed {
            self.rebuild_hover_highlight(&key);
        }
        true
    }

    /// The nearest shown mesh along the ray through `pointer`.
    fn pick_mesh(&self, armor: &LoadedShipArmor, pointer: Vec2, rect: ViewRect) -> Option<MeshHit> {
        let (origin, direction) = self.viewport.screen_to_model_ray(pointer, rect)?;

        armor.hull_pick_index()?.pick(origin, direction, &self.hull_shown)
    }

    /// Hovers the mesh `hit` names, rebuilding the highlight only when the hovered mesh name
    /// changed. Returns whether the caller should `cx.notify()`.
    fn set_mesh_hover(&mut self, armor: &LoadedShipArmor, hit: MeshHit, cursor: Vec2) -> bool {
        let (Some(mesh), Some(source)) =
            (armor.hull_meshes.get(hit.mesh.get()), armor.hull_pick_index().and_then(|pick| pick.source(hit.mesh)))
        else {
            return self.clear_mesh_hover();
        };

        let tooltip = match self.hovered_mesh.take() {
            Some(hover) if hover.mesh == hit.mesh => hover.tooltip,
            _ => MeshTooltip::new(mesh, source),
        };
        self.hovered_mesh = Some(MeshHover { mesh: hit.mesh, tooltip, cursor });
        let highlighted = self.mesh_highlight.as_ref().map(|(name, _)| name.as_str());
        match HighlightChange::between(highlighted, Some(&mesh.name)) {
            HighlightChange::Keep => {}
            HighlightChange::Show(name) => self.show_mesh_highlight(name),
            HighlightChange::Remove => self.remove_mesh_highlight(),
        }

        true
    }

    /// Clears any hovered mesh, its tooltip and its highlight. Returns whether there was one.
    pub(super) fn clear_mesh_hover(&mut self) -> bool {
        self.remove_mesh_highlight();

        self.hovered_mesh.take().is_some()
    }

    /// Replaces the mesh highlight with one over every mesh named `name`.
    fn show_mesh_highlight(&mut self, name: String) {
        self.remove_mesh_highlight();
        let Some(scene) = self.scene() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let names = [name];
        let mesh_id = hull_picking::upload_hull_highlight(
            &mut self.viewport,
            scene.device(),
            &armor.hull_meshes,
            &names,
            HighlightOrigin::ViewHover,
        );
        let [name] = names;
        self.mesh_highlight = Some((name, mesh_id));
        self.viewport.mark_dirty();
    }

    fn remove_mesh_highlight(&mut self) {
        if let Some((_, old_id)) = self.mesh_highlight.take() {
            self.viewport.remove_mesh(old_id);
            self.viewport.mark_dirty();
        }
    }

    /// Keeps the hovered mesh after its meshes were uploaded again: drops it once hidden, else
    /// draws its highlight again if the upload removed it.
    pub(super) fn revalidate_mesh_hover(&mut self) {
        let Some(armor) = self.current_armor.clone() else { return };
        let shown = self
            .hovered_mesh
            .as_ref()
            .filter(|hover| self.hull_shown.get(hover.mesh.get()) == Some(&true))
            .and_then(|hover| armor.hull_meshes.get(hover.mesh.get()));
        match shown {
            Some(mesh) if self.mesh_highlight.is_none() => self.show_mesh_highlight(mesh.name.clone()),
            Some(_) => {}
            None => {
                self.clear_mesh_hover();
            }
        }
    }

    /// Reads the camera orbit under the pointer, if it is resting on one.
    ///
    /// Screen-space rather than a pick: an orbit is drawn a few centimetres wide, and a ray would
    /// miss it at every angle a ship is looked at from. Returns whether what it reads changed.
    pub(super) fn update_ring_hover(&mut self, position: Vec2) -> bool {
        if self.ring_hovers.is_empty() {
            return self.clear_ring_hover();
        }
        let Some(rect) = self.last_bounds else { return self.clear_ring_hover() };
        let camera = &self.viewport.camera;
        let viewport = &self.viewport;
        let found = camera_rings::nearest_ring_label(
            &self.ring_hovers,
            |point| camera.project_to_screen(viewport.pos_to_world_space(point), rect).map(|at| [at.x, at.y]),
            [position.x, position.y],
        )
        .map(str::to_string);

        let changed = found.as_ref() != self.hovered_ring.as_ref().map(|(label, _)| label);
        self.hovered_ring = found.map(|label| (label, position));
        changed
    }

    /// Forgets the orbit under the pointer. Returns whether there was one.
    pub(super) fn clear_ring_hover(&mut self) -> bool {
        self.hovered_ring.take().is_some()
    }

    /// Reads the hardpoint marker nearest the pointer, within a few pixels. Returns whether it
    /// changed.
    pub(super) fn update_hardpoint_hover(&mut self, position: Vec2) -> bool {
        const REACH_PX: f32 = 10.0;

        let found = self.last_bounds.and_then(|rect| {
            let pointer = position;
            self.hardpoint_labels
                .iter()
                .enumerate()
                .filter_map(|(index, label)| {
                    let anchor = self.viewport.pos_to_world_space(Vec3::from(label.position));
                    let at = self.viewport.camera.project_to_screen(anchor, rect)?;
                    let distance = ((at.x - pointer.x).powi(2) + (at.y - pointer.y).powi(2)).sqrt();
                    (distance <= REACH_PX).then_some((index, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(index, _)| index)
        });

        let changed = found != self.hovered_hardpoint;
        self.hovered_hardpoint = found;
        changed
    }

    /// Clears any hovered plate, tooltip, and highlight overlay. Returns whether anything changed,
    /// so callers only notify when needed.
    pub(super) fn clear_plate_hover(&mut self) -> bool {
        let had_hover = self.hovered.take().is_some();
        if let Some((_, old_id)) = self.hover_highlight.take() {
            self.viewport.remove_mesh(old_id);
            self.viewport.mark_dirty();
        }
        had_hover
    }

    /// Replaces the hover-highlight overlay mesh with one for `key`. Leaves no highlight if there
    /// is no device or no armor loaded.
    pub(super) fn rebuild_hover_highlight(&mut self, key: &PlateKey) {
        if let Some((_, old_id)) = self.hover_highlight.take() {
            self.viewport.remove_mesh(old_id);
        }
        let Some(scene) = self.scene() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let show_zero_mm = self.show_zero_mm();
        let visibility = VisibilityFilter {
            part: &self.part_visibility,
            plate: &self.plate_visibility,
            show_hidden_only: self.show_hidden_only,
        };
        let mesh_id = plate_picking::upload_plate_highlight(
            &mut self.viewport,
            scene.device(),
            &armor,
            key,
            visibility,
            show_zero_mm,
            HighlightOrigin::ViewHover,
        );
        self.hover_highlight = Some((key.clone(), mesh_id));
        self.viewport.mark_dirty();
    }

    /// Replaces the tree-row highlight overlay mesh with one for `key`. Leaves no highlight if
    /// there is no device or no armor loaded. Also rebuilds an active highlight against fresh
    /// geometry after a visibility change.
    pub(super) fn rebuild_sidebar_highlight(&mut self, key: SidebarHighlightKey) {
        if let Some((_, old_id)) = self.sidebar_highlight.take() {
            self.viewport.remove_mesh(old_id);
        }
        let Some(scene) = self.scene() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let visibility = VisibilityFilter {
            part: &self.part_visibility,
            plate: &self.plate_visibility,
            show_hidden_only: self.show_hidden_only,
        };
        let show_zero_mm = self.show_zero_mm();
        let mesh_id = match &key {
            SidebarHighlightKey::Zone(zone) => plate_picking::upload_zone_highlight(
                &mut self.viewport,
                scene.device(),
                &armor,
                zone,
                visibility,
                show_zero_mm,
            ),
            SidebarHighlightKey::Part(zone, part) => plate_picking::upload_part_highlight(
                &mut self.viewport,
                scene.device(),
                &armor,
                zone,
                part,
                visibility,
                show_zero_mm,
            ),
            SidebarHighlightKey::Plate(pk) => plate_picking::upload_plate_highlight(
                &mut self.viewport,
                scene.device(),
                &armor,
                pk,
                visibility,
                show_zero_mm,
                HighlightOrigin::Row,
            ),
            SidebarHighlightKey::HullMeshes(names) => hull_picking::upload_hull_highlight(
                &mut self.viewport,
                scene.device(),
                &armor.hull_meshes,
                names,
                HighlightOrigin::Row,
            ),
        };
        self.sidebar_highlight = Some((key, mesh_id));
        self.viewport.mark_dirty();
    }

    /// Sets the tree-row highlight to `key`, rebuilding the overlay mesh only when the key
    /// changed.
    pub(super) fn set_sidebar_hover(&mut self, key: SidebarHighlightKey, cx: &mut ViewCx) {
        if self.sidebar_highlight.as_ref().map(|(k, _)| k) == Some(&key) {
            return;
        }
        self.rebuild_sidebar_highlight(key);
        cx.notify();
    }

    /// Clears the tree-row highlight unconditionally: the tree closed.
    pub(super) fn clear_sidebar_hover(&mut self, cx: &mut ViewCx) {
        if let Some((_, old_id)) = self.sidebar_highlight.take() {
            self.viewport.remove_mesh(old_id);
            self.viewport.mark_dirty();
            cx.notify();
        }
    }

    /// Clears the tree-row highlight only if it matches `key`: a row's leave firing after another
    /// row's enter already switched the highlight must not clear the new one.
    pub(super) fn clear_sidebar_hover_if(&mut self, key: &SidebarHighlightKey, cx: &mut ViewCx) {
        if self.sidebar_highlight.as_ref().map(|(k, _)| k) == Some(key) {
            self.clear_sidebar_hover(cx);
        }
    }

    /// Records an undo step, runs `mutate`, then uploads again. Every visibility change goes
    /// through here so none can skip the undo step or the upload.
    fn snapshot_and_mutate(&mut self, cx: &mut ViewCx, mutate: impl FnOnce(&mut Self)) {
        self.undo_stack.push(VisibilitySnapshot {
            part_visibility: self.part_visibility.clone(),
            plate_visibility: self.plate_visibility.clone(),
        });
        mutate(self);
        self.mesh_seed.armor = false;
        self.reupload_current_armor(cx);
        cx.emit(ViewportChange::Settings);
    }

    /// Toggles a single plate's visibility.
    pub(super) fn toggle_plate(&mut self, key: PlateKey, cx: &mut ViewCx) {
        self.snapshot_and_mutate(cx, |this| {
            let hidden = this.plate_visibility.get(&key).copied().unwrap_or(false);
            this.set_plate_hidden_raw(key, !hidden);
        });
    }

    /// Sets one plate's hidden state with no undo step or upload of its own. `plate_visibility`
    /// holds only hidden keys, so showing a plate removes its entry.
    fn set_plate_hidden_raw(&mut self, key: PlateKey, hidden: bool) {
        if hidden {
            self.plate_visibility.insert(key, true);
        } else {
            self.plate_visibility.remove(&key);
        }
    }

    /// Clears every explicitly hidden plate.
    pub(super) fn show_all_plates(&mut self, cx: &mut ViewCx) {
        if self.plate_visibility.is_empty() {
            return;
        }
        self.snapshot_and_mutate(cx, |this| this.plate_visibility.clear());
    }

    /// Turns `(zone, material)` off at the part level.
    pub(super) fn disable_material(&mut self, zone: &str, material: &str, cx: &mut ViewCx) {
        let key = (zone.to_string(), material.to_string());
        self.snapshot_and_mutate(cx, |this| {
            this.part_visibility.insert(key, false);
        });
    }

    /// Every `(zone, part)` on, every plate override cleared.
    pub(super) fn set_all_parts_visible(&mut self, cx: &mut ViewCx) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(cx, |this| {
            for (zone, parts) in &armor.zone_parts {
                for part in parts {
                    this.part_visibility.insert((zone.clone(), part.clone()), true);
                }
            }
            this.plate_visibility.clear();
        });
    }

    /// Every `(zone, part)` off.
    pub(super) fn set_all_parts_hidden(&mut self, cx: &mut ViewCx) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(cx, |this| {
            for (zone, parts) in &armor.zone_parts {
                for part in parts {
                    this.part_visibility.insert((zone.clone(), part.clone()), false);
                }
            }
        });
    }

    /// Clears plate-level overrides only, leaving `part_visibility` untouched.
    pub(super) fn reset_plate_overrides(&mut self, cx: &mut ViewCx) {
        if self.plate_visibility.is_empty() {
            return;
        }
        self.snapshot_and_mutate(cx, |this| this.plate_visibility.clear());
    }

    /// Sets `(zone, part)` to `checked` and clears the part's plate overrides whatever `checked`
    /// is, unlike the zone toggle, which clears them only when turning on.
    pub(super) fn toggle_part(
        &mut self,
        zone: String,
        part: String,
        plate_thicknesses: Vec<i32>,
        checked: bool,
        cx: &mut ViewCx,
    ) {
        self.snapshot_and_mutate(cx, move |this| {
            visibility::clear_plate_overrides(&mut this.plate_visibility, &zone, &part, &plate_thicknesses);
            this.part_visibility.insert((zone, part), checked);
        });
    }

    /// Sets a zone's `parts` to `checked`, or with `solo` turns those parts on, every other part
    /// off and clears every zone's plate overrides. `parts` is every part of the zone, or while
    /// the tree is filtered, the ones it lists.
    pub(super) fn toggle_zone(&mut self, zone: String, parts: Vec<String>, checked: bool, solo: bool, cx: &mut ViewCx) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(cx, move |this| {
            if solo {
                for z in &armor.zone_part_plates {
                    for p in &z.parts {
                        let on = z.name == zone && parts.contains(&p.name);
                        this.part_visibility.insert((z.name.clone(), p.name.clone()), on);
                        for &t in &p.plates {
                            this.plate_visibility.remove(&PlateKey {
                                zone: z.name.clone(),
                                material_name: p.name.clone(),
                                thickness_tenths: t,
                            });
                        }
                    }
                }
                return;
            }
            let Some(z) = armor.zone_part_plates.iter().find(|z| z.name == zone) else { return };
            for p in z.parts.iter().filter(|p| parts.contains(&p.name)) {
                this.part_visibility.insert((zone.clone(), p.name.clone()), checked);
                if checked {
                    for &t in &p.plates {
                        this.plate_visibility.remove(&PlateKey {
                            zone: zone.clone(),
                            material_name: p.name.clone(),
                            thickness_tenths: t,
                        });
                    }
                }
            }
        });
    }

    /// Expands or collapses a zone header in the armor tree. Not undo-tracked.
    pub(super) fn toggle_zone_expanded(&mut self, zone: String, cx: &mut ViewCx) {
        if !self.expanded_zones.remove(&zone) {
            self.expanded_zones.insert(zone);
        }
        cx.notify();
    }

    /// Expands or collapses a material header in the armor tree. Not undo-tracked.
    pub(super) fn toggle_part_expanded(&mut self, key: (String, String), cx: &mut ViewCx) {
        if !self.expanded_parts.remove(&key) {
            self.expanded_parts.insert(key);
        }
        cx.notify();
    }

    /// Expands or collapses a hull-part-group header in the hull tree. Not undo-tracked.
    pub(super) fn toggle_hull_group_expanded(&mut self, group: String, cx: &mut ViewCx) {
        if !self.expanded_hull_groups.remove(&group) {
            self.expanded_hull_groups.insert(group);
        }
        cx.notify();
    }

    fn current_visibility_snapshot(&self) -> VisibilitySnapshot {
        VisibilitySnapshot {
            part_visibility: self.part_visibility.clone(),
            plate_visibility: self.plate_visibility.clone(),
        }
    }

    /// Restores the previous visibility snapshot.
    pub(super) fn undo_visibility(&mut self, cx: &mut ViewCx) {
        let current = self.current_visibility_snapshot();
        let Some(prev) = self.undo_stack.undo(current) else { return };
        self.part_visibility = prev.part_visibility;
        self.mesh_seed.armor = false;
        self.plate_visibility = prev.plate_visibility;
        self.reupload_current_armor(cx);
        cx.emit(ViewportChange::Settings);
    }

    /// Applies the next visibility snapshot again.
    pub(super) fn redo_visibility(&mut self, cx: &mut ViewCx) {
        let current = self.current_visibility_snapshot();
        let Some(next) = self.undo_stack.redo(current) else { return };
        self.part_visibility = next.part_visibility;
        self.mesh_seed.armor = false;
        self.plate_visibility = next.plate_visibility;
        self.reupload_current_armor(cx);
        cx.emit(ViewportChange::Settings);
    }

    /// The hovered plate's tooltip and the pointer it follows.
    pub(super) fn plate_hover(&self) -> Option<(ArmorTriangleTooltip, Vec2)> {
        self.hovered.as_ref().map(|hover| (hover.tooltip.clone(), hover.cursor))
    }

    /// The hovered mesh's tooltip and the pointer it follows.
    pub(super) fn mesh_hover(&self) -> Option<(MeshTooltip, Vec2)> {
        self.hovered_mesh.as_ref().map(|hover| (hover.tooltip.clone(), hover.cursor))
    }

    /// The hovered orbit's label and the pointer it follows.
    pub(super) fn ring_hover(&self) -> Option<(String, Vec2)> {
        self.hovered_ring.clone()
    }
}

impl ArmorViewportState {
    pub fn set_sidebar_hover(self, key: SidebarHighlightKey) {
        self.update(|this, cx| this.set_sidebar_hover(key, cx));
    }

    pub fn clear_sidebar_hover(self) {
        self.update(|this, cx| this.clear_sidebar_hover(cx));
    }

    pub fn clear_sidebar_hover_if(self, key: &SidebarHighlightKey) {
        self.update(|this, cx| this.clear_sidebar_hover_if(key, cx));
    }

    pub fn toggle_plate(self, key: PlateKey) {
        self.update(|this, cx| this.toggle_plate(key, cx));
    }

    pub fn show_all_plates(self) {
        self.update(|this, cx| this.show_all_plates(cx));
    }

    pub fn disable_material(self, zone: &str, material: &str) {
        self.update(|this, cx| this.disable_material(zone, material, cx));
    }

    pub fn set_all_parts_visible(self) {
        self.update(|this, cx| this.set_all_parts_visible(cx));
    }

    pub fn set_all_parts_hidden(self) {
        self.update(|this, cx| this.set_all_parts_hidden(cx));
    }

    pub fn reset_plate_overrides(self) {
        self.update(|this, cx| this.reset_plate_overrides(cx));
    }

    pub fn toggle_part(self, zone: String, part: String, plate_thicknesses: Vec<i32>, checked: bool) {
        self.update(|this, cx| this.toggle_part(zone, part, plate_thicknesses, checked, cx));
    }

    pub fn toggle_zone(self, zone: String, parts: Vec<String>, checked: bool, solo: bool) {
        self.update(|this, cx| this.toggle_zone(zone, parts, checked, solo, cx));
    }

    pub fn toggle_zone_expanded(self, zone: String) {
        self.update(|this, cx| this.toggle_zone_expanded(zone, cx));
    }

    pub fn toggle_part_expanded(self, key: (String, String)) {
        self.update(|this, cx| this.toggle_part_expanded(key, cx));
    }

    pub fn toggle_hull_group_expanded(self, group: String) {
        self.update(|this, cx| this.toggle_hull_group_expanded(group, cx));
    }

    pub fn undo_visibility(self) {
        self.update(|this, cx| this.undo_visibility(cx));
    }

    pub fn redo_visibility(self) {
        self.update(|this, cx| this.redo_visibility(cx));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wows_toolkit_armor::load_model::fixtures;
    use wows_toolkit_armor::load_ship::ArmorZone;
    use wows_toolkit_armor::load_ship::PlateKey;
    use wows_toolkit_armor::load_ship::ZonePart;
    use wowsunpack::export::model_catalog::ModelPath;

    use super::ViewCx;
    use super::ViewportChange;
    use super::ViewportModel;

    fn plate(zone: &str, part: &str, thickness_tenths: i32) -> PlateKey {
        PlateKey { zone: zone.to_string(), material_name: part.to_string(), thickness_tenths }
    }

    fn part_key(zone: &str, part: &str) -> (String, String) {
        (zone.to_string(), part.to_string())
    }

    /// A model holding two zones: Bow with Belt and Deck, Stern with Belt.
    fn model_with_armor() -> ViewportModel {
        let path = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");
        let mut armor = fixtures::armored_model_scene(path);
        armor.zone_parts = vec![
            ("Bow".to_string(), vec!["Belt".to_string(), "Deck".to_string()]),
            ("Stern".to_string(), vec!["Belt".to_string()]),
        ];
        armor.zone_part_plates = vec![
            ArmorZone {
                name: "Bow".to_string(),
                parts: vec![
                    ZonePart { name: "Belt".to_string(), plates: vec![250, 500] },
                    ZonePart { name: "Deck".to_string(), plates: vec![190] },
                ],
            },
            ArmorZone {
                name: "Stern".to_string(),
                parts: vec![ZonePart { name: "Belt".to_string(), plates: vec![250] }],
            },
        ];
        let mut model = ViewportModel::new();
        model.current_armor = Some(Arc::new(armor));
        model
    }

    #[test]
    fn toggling_a_plate_hides_then_shows_it() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();
        let key = plate("Bow", "Belt", 250);

        model.toggle_plate(key.clone(), &mut cx);
        assert_eq!(model.plate_visibility.get(&key), Some(&true));
        assert_eq!(cx.events(), &[ViewportChange::Settings]);

        model.toggle_plate(key.clone(), &mut cx);
        assert!(model.plate_visibility.is_empty());
    }

    #[test]
    fn undo_and_redo_walk_the_visibility_history() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();
        let key = plate("Bow", "Belt", 250);

        model.toggle_plate(key.clone(), &mut cx);
        model.disable_material("Bow", "Deck", &mut cx);

        model.undo_visibility(&mut cx);
        assert!(model.part_visibility.is_empty());
        assert_eq!(model.plate_visibility.get(&key), Some(&true));

        model.undo_visibility(&mut cx);
        assert!(model.plate_visibility.is_empty());

        model.undo_visibility(&mut cx);
        assert!(model.plate_visibility.is_empty());

        model.redo_visibility(&mut cx);
        model.redo_visibility(&mut cx);
        assert_eq!(model.plate_visibility.get(&key), Some(&true));
        assert_eq!(model.part_visibility.get(&part_key("Bow", "Deck")), Some(&false));
    }

    #[test]
    fn a_new_change_drops_the_redo_history() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();

        model.toggle_plate(plate("Bow", "Belt", 250), &mut cx);
        model.undo_visibility(&mut cx);
        model.toggle_plate(plate("Bow", "Belt", 500), &mut cx);
        model.redo_visibility(&mut cx);

        assert_eq!(model.plate_visibility.len(), 1);
        assert!(model.plate_visibility.contains_key(&plate("Bow", "Belt", 500)));
    }

    #[test]
    fn showing_all_plates_with_none_hidden_records_no_step() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();

        model.show_all_plates(&mut cx);
        assert!(cx.events().is_empty());

        model.toggle_plate(plate("Bow", "Belt", 250), &mut cx);
        model.show_all_plates(&mut cx);
        assert!(model.plate_visibility.is_empty());

        model.undo_visibility(&mut cx);
        assert_eq!(model.plate_visibility.len(), 1);
    }

    #[test]
    fn toggling_a_part_clears_its_plate_overrides_either_way() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();
        model.plate_visibility.insert(plate("Bow", "Belt", 250), true);
        model.plate_visibility.insert(plate("Bow", "Deck", 190), true);

        model.toggle_part("Bow".to_string(), "Belt".to_string(), vec![250, 500], false, &mut cx);

        assert_eq!(model.part_visibility.get(&part_key("Bow", "Belt")), Some(&false));
        assert!(!model.plate_visibility.contains_key(&plate("Bow", "Belt", 250)));
        assert!(model.plate_visibility.contains_key(&plate("Bow", "Deck", 190)));
    }

    #[test]
    fn toggling_a_zone_sets_its_listed_parts_and_clears_overrides_only_when_on() {
        let mut model = model_with_armor();
        let mut cx = ViewCx::for_test();
        model.plate_visibility.insert(plate("Bow", "Belt", 250), true);

        model.toggle_zone("Bow".to_string(), vec!["Belt".to_string()], false, false, &mut cx);
        assert_eq!(model.part_visibility.get(&part_key("Bow", "Belt")), Some(&false));
        assert!(!model.part_visibility.contains_key(&part_key("Bow", "Deck")));
        assert!(model.plate_visibility.contains_key(&plate("Bow", "Belt", 250)));

        model.toggle_zone("Bow".to_string(), vec!["Belt".to_string()], true, false, &mut cx);
        assert_eq!(model.part_visibility.get(&part_key("Bow", "Belt")), Some(&true));
        assert!(model.plate_visibility.is_empty());
    }

    #[test]
    fn soloing_a_zone_turns_every_other_part_off() {
        let mut model = model_with_armor();
        let mut cx = ViewCx::for_test();
        model.plate_visibility.insert(plate("Stern", "Belt", 250), true);

        model.toggle_zone("Bow".to_string(), vec!["Deck".to_string()], true, true, &mut cx);

        assert_eq!(model.part_visibility.get(&part_key("Bow", "Deck")), Some(&true));
        assert_eq!(model.part_visibility.get(&part_key("Bow", "Belt")), Some(&false));
        assert_eq!(model.part_visibility.get(&part_key("Stern", "Belt")), Some(&false));
        assert!(model.plate_visibility.is_empty());
    }

    #[test]
    fn toggling_a_zone_without_armor_records_no_step() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();

        model.toggle_zone("Bow".to_string(), vec!["Belt".to_string()], true, false, &mut cx);

        assert!(cx.events().is_empty());
        assert!(model.part_visibility.is_empty());
    }

    #[test]
    fn all_and_none_cover_every_part() {
        let mut model = model_with_armor();
        let mut cx = ViewCx::for_test();
        model.plate_visibility.insert(plate("Bow", "Belt", 250), true);

        model.set_all_parts_hidden(&mut cx);
        assert_eq!(model.part_visibility.len(), 3);
        assert!(model.part_visibility.values().all(|on| !on));
        assert_eq!(model.plate_visibility.len(), 1);

        model.set_all_parts_visible(&mut cx);
        assert!(model.part_visibility.values().all(|on| *on));
        assert!(model.plate_visibility.is_empty());
    }

    #[test]
    fn clearing_hovers_reports_whether_there_was_one() {
        let mut model = ViewportModel::new();
        assert!(!model.clear_plate_hover());
        assert!(!model.clear_mesh_hover());
        assert!(!model.clear_ring_hover());

        model.hovered_ring = Some(("Orbit".to_string(), wows_toolkit_armor::viewport::types::Vec2::new(1.0, 2.0)));
        assert!(model.clear_ring_hover());
        assert!(!model.clear_ring_hover());
    }

    #[test]
    fn hovers_without_a_layout_clear_and_report_no_change() {
        let mut model = ViewportModel::new();
        let at = wows_toolkit_armor::viewport::types::Vec2::new(5.0, 5.0);
        assert!(!model.update_plate_hover(at));
        assert!(!model.update_hardpoint_hover(at));
        assert!(!model.update_ring_hover(at));
    }

    #[test]
    fn expanding_toggles_back_and_forth() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();

        model.toggle_zone_expanded("Bow".to_string(), &mut cx);
        assert!(model.expanded_zones.contains("Bow"));
        model.toggle_zone_expanded("Bow".to_string(), &mut cx);
        assert!(model.expanded_zones.is_empty());

        model.toggle_part_expanded(part_key("Bow", "Belt"), &mut cx);
        assert!(model.expanded_parts.contains(&part_key("Bow", "Belt")));

        model.toggle_hull_group_expanded("Guns".to_string(), &mut cx);
        assert!(model.expanded_hull_groups.contains("Guns"));
    }
}
