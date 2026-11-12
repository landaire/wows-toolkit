//! What the pointer, the wheel and the keyboard do to the viewport: orbit, pan and zoom the camera,
//! snap it from the gizmo, move it with held keys, hover and pick, and click to hide, cast or burst.

use std::time::Duration;

use wows_toolkit_armor::viewport::camera;
use wows_toolkit_armor::viewport::camera::Axis;
use wows_toolkit_armor::viewport::gizmo;
use wows_toolkit_armor::viewport::types::Vec2;
use wows_toolkit_armor::viewport::types::ViewRect;
use zgui::vocab::PointerButton;
use zgui::vocab::Timestamp;

use super::model::DragKind;
use super::model::DragState;
use super::model::MoveKey;
use super::model::ViewportModel;
use super::state::ViewCx;
use super::types::ViewportChange;

/// How long a gizmo snap takes, in seconds.
const GIZMO_SNAP_DURATION_SECS: f32 = 0.35;

/// How far the pointer moves, in pixels, between a press and its release before the press is a
/// drag rather than a click, as egui classifies it.
const DRAG_THRESHOLD_PX: f32 = 4.0;

/// How close in time and place a second primary press must follow the first to be a
/// double-click.
const DOUBLE_CLICK_TIME: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_SLOP_PX: f32 = 4.0;

/// The step a frame of held-key motion takes when the previous frame's time is unknown, and the
/// most a slow frame may take, so a stall does not throw the camera across the scene.
const FRAME_STEP: Duration = Duration::from_micros(16_667);
const LONGEST_FRAME: Duration = Duration::from_millis(100);

/// What releasing a button over the viewport does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClickAction {
    SnapGizmo,
    Cast,
    Burst,
    TogglePlate,
    Nothing,
}

/// What a key press asked of the pane rather than of the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyOutcome {
    /// The viewport took the key.
    Handled,
    /// Ctrl+S: open or close the pane's options rail.
    ToggleOptions,
    /// The key means nothing here.
    Ignored,
}

/// A key as the viewport reads it: the lowercase character, or the arrow's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ViewKey<'a> {
    Character(&'a str),
    Up,
    Down,
    Left,
    Right,
}

/// Which modifiers were held with a key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct KeyModifiers {
    /// Ctrl, or Cmd on macOS.
    pub(super) primary: bool,
    pub(super) shift: bool,
}

/// The primary press before this one, so the next can be told a double-click.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LastPress {
    pub(super) at: Vec2,
    pub(super) when: Timestamp,
}

impl MoveKey {
    /// Shift turns the arrows from a vertical move and an orbit into a pan.
    pub(super) fn from_key(key: ViewKey<'_>, shift: bool) -> Option<Self> {
        match (key, shift) {
            (ViewKey::Character("w"), _) => Some(Self::Forward),
            (ViewKey::Character("s"), _) => Some(Self::Back),
            (ViewKey::Character("a"), _) => Some(Self::Left),
            (ViewKey::Character("d"), _) => Some(Self::Right),
            (ViewKey::Up, false) => Some(Self::Up),
            (ViewKey::Down, false) => Some(Self::Down),
            (ViewKey::Left, false) => Some(Self::RotateLeft),
            (ViewKey::Right, false) => Some(Self::RotateRight),
            (ViewKey::Up, true) => Some(Self::PanUp),
            (ViewKey::Down, true) => Some(Self::PanDown),
            (ViewKey::Left, true) => Some(Self::PanLeft),
            (ViewKey::Right, true) => Some(Self::PanRight),
            _ => None,
        }
    }

    /// Every motion a released key may have started: Shift can be let go before the arrow it
    /// modified.
    pub(super) fn released_by(key: ViewKey<'_>) -> impl Iterator<Item = Self> {
        [Self::from_key(key, false), Self::from_key(key, true)].into_iter().flatten()
    }
}

/// One frame's camera motion from the held keys, each axis summed over the keys that drive it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct HeldMotion {
    pub(super) forward: f32,
    pub(super) right: f32,
    pub(super) vertical: f32,
    pub(super) rotate: f32,

    /// Screen space, +x right and +y up.
    pub(super) pan_x: f32,
    pub(super) pan_y: f32,
}

impl HeldMotion {
    pub(super) fn from_keys<'a>(keys: impl IntoIterator<Item = &'a MoveKey>) -> Self {
        let mut motion = Self::default();
        for key in keys {
            match key {
                MoveKey::Forward => motion.forward += 1.0,
                MoveKey::Back => motion.forward -= 1.0,
                MoveKey::Left => motion.right -= 1.0,
                MoveKey::Right => motion.right += 1.0,
                MoveKey::Up => motion.vertical += 1.0,
                MoveKey::Down => motion.vertical -= 1.0,
                MoveKey::RotateLeft => motion.rotate += 1.0,
                MoveKey::RotateRight => motion.rotate -= 1.0,
                MoveKey::PanLeft => motion.pan_x -= 1.0,
                MoveKey::PanRight => motion.pan_x += 1.0,
                MoveKey::PanUp => motion.pan_y += 1.0,
                MoveKey::PanDown => motion.pan_y -= 1.0,
            }
        }

        motion
    }
}

/// What releasing `button` does. A press that moved past the drag threshold was an orbit or a
/// pan, and does nothing more.
///
/// Shift-click casts: a plain primary click hides a plate, a primary drag orbits, a middle drag
/// pans, a secondary click opens the menu, and Ctrl-click is a secondary click on macOS, so Shift
/// is the modifier no other gesture holds. It is read before splash mode, which owns the plain
/// click only.
pub(super) fn click_action(
    button: PointerButton,
    plain_click: bool,
    press_in_gizmo: bool,
    splash_mode: bool,
    shift: bool,
) -> ClickAction {
    if !plain_click {
        return ClickAction::Nothing;
    }

    match button {
        PointerButton::Primary if press_in_gizmo => ClickAction::SnapGizmo,
        PointerButton::Primary if shift => ClickAction::Cast,
        PointerButton::Primary if splash_mode => ClickAction::Burst,
        PointerButton::Primary => ClickAction::TogglePlate,
        _ => ClickAction::Nothing,
    }
}

impl ViewportModel {
    /// Where the navigation gizmo sits, or `None` while there is no scene for it to orient or the
    /// camera lock owns the camera: it is not drawn then, so nothing may be aimed at it either.
    pub(super) fn gizmo_rect(&self) -> Option<ViewRect> {
        self.current_armor.as_ref()?;
        self.camera_is_free().then_some(())?;
        self.last_bounds.map(gizmo::gizmo_rect)
    }

    /// A press over the viewport. Says whether the press starts a drag, which the element then
    /// captures the pointer for.
    pub(super) fn pointer_down(&mut self, at: Vec2, button: PointerButton, when: Timestamp, cx: &mut ViewCx) -> bool {
        if !self.is_gpu_ready() {
            return false;
        }
        match button {
            PointerButton::Primary => {
                let in_gizmo = self.gizmo_rect().is_some_and(|rect| rect.contains(at));
                self.gizmo_press_in_box = in_gizmo;
                let previous = self.last_press.replace(LastPress { at, when });
                let double = previous.is_some_and(|previous| {
                    when.saturating_since(previous.when) <= DOUBLE_CLICK_TIME
                        && (at - previous.at).norm() <= DOUBLE_CLICK_SLOP_PX
                });
                if !in_gizmo && double && !self.perspective.enabled {
                    // A third press starts counting again.
                    self.last_press = None;
                    if let Some(home) = self.home_camera.clone() {
                        self.viewport.camera = home;
                        self.viewport.mark_dirty();
                        cx.emit(ViewportChange::Camera);
                    }
                    self.drag = None;
                    return false;
                }
                self.drag =
                    Some(DragState { kind: DragKind::Orbit, last_position: at, start_position: at, moved: false });
                true
            }
            PointerButton::Middle => {
                self.gizmo_press_in_box = false;
                self.drag =
                    Some(DragState { kind: DragKind::Pan, last_position: at, start_position: at, moved: false });
                true
            }
            _ => false,
        }
    }

    pub(super) fn pointer_move(&mut self, at: Vec2, cx: &mut ViewCx) {
        if !self.is_gpu_ready() {
            return;
        }
        self.last_pointer = Some(at);
        self.update_gizmo_hover(at);

        if let Some(drag) = self.drag {
            let (dx, dy) = (at.x - drag.last_position.x, at.y - drag.last_position.y);
            if dx != 0.0 || dy != 0.0 {
                let size = self.viewport_size();
                // A locked camera is steered on its orbit instead: orbiting the model would fight
                // the lock, which puts the camera back every upload.
                if self.perspective.enabled {
                    if self.steer_perspective(dx, dy) {
                        self.refresh_perspective(cx);
                    }
                } else {
                    match drag.kind {
                        DragKind::Orbit => self.viewport.camera.orbit((dx, dy), size),
                        DragKind::Pan => self.viewport.camera.pan((dx, dy), size),
                    }
                }
                let moved = drag.moved || (at - drag.start_position).norm() > DRAG_THRESHOLD_PX;
                self.drag = Some(DragState { last_position: at, moved, ..drag });
                self.viewport.mark_dirty();
                cx.emit(ViewportChange::Camera);
            }
        }

        self.pick_hovers(at);
    }

    /// The pointer came over the view, where it may rest without moving after an overlay over it
    /// closed: picks at once.
    pub(super) fn pointer_entered(&mut self, at: Vec2) {
        if !self.is_gpu_ready() {
            return;
        }
        self.last_pointer = Some(at);
        self.update_gizmo_hover(at);
        self.pick_hovers(at);
    }

    /// The pointer left the view. Every hover goes, since no move will clear them; a drag carries
    /// on, since its pointer is captured.
    pub(super) fn pointer_left(&mut self) {
        // A picking index landing now has no pointer over the view to pick at.
        self.last_pointer = None;
        self.gizmo_hover = None;
        self.clear_pointer_hovers();
    }

    /// Reads the gizmo axis under `at`. Returns whether it changed.
    pub(super) fn update_gizmo_hover(&mut self, at: Vec2) -> bool {
        let hover = self.gizmo_rect().and_then(|rect| gizmo::hit_test(rect, &self.viewport.camera, at));
        let changed = hover != self.gizmo_hover;
        self.gizmo_hover = hover;

        changed
    }

    /// Picks the orbit, hardpoint, plate or mesh under `at`. Returns whether any hover changed.
    ///
    /// Not while a drag moves the camera, which would leave a tooltip sitting stale over a moving
    /// model, and not while the camera is locked, which is a view rather than a place to edit
    /// from.
    pub(super) fn pick_hovers(&mut self, at: Vec2) -> bool {
        if self.drag.is_some() || !self.camera_is_free() {
            return self.clear_pointer_hovers();
        }

        let ring_changed = self.update_ring_hover(at);
        let hardpoint_changed = self.update_hardpoint_hover(at);

        self.update_plate_hover(at) || ring_changed || hardpoint_changed
    }

    /// Clears the plate, mesh, orbit and hardpoint hovers. Returns whether any was set.
    pub(super) fn clear_pointer_hovers(&mut self) -> bool {
        let ring_changed = self.clear_ring_hover();
        let hardpoint_changed = self.hovered_hardpoint.take().is_some();
        let plate_changed = self.clear_plate_hover();

        self.clear_mesh_hover() || plate_changed || ring_changed || hardpoint_changed
    }

    pub(super) fn pointer_up(&mut self, at: Vec2, button: PointerButton, shift: bool, cx: &mut ViewCx) {
        let drag = self.drag.take();
        if !self.is_gpu_ready() {
            self.gizmo_press_in_box = false;
            return;
        }
        let plain_click = drag.is_some_and(|drag| !drag.moved);
        match click_action(button, plain_click, self.gizmo_press_in_box, self.splash_mode, shift) {
            ClickAction::SnapGizmo => {
                if let Some(rect) = self.gizmo_rect()
                    && let Some((axis, positive)) = gizmo::hit_test(rect, &self.viewport.camera, at)
                {
                    self.snap_camera(axis, positive, cx);
                }
            }
            ClickAction::Cast => self.cast_at(at, cx),
            ClickAction::Burst => self.burst_at(at, cx),
            ClickAction::TogglePlate => {
                // A plain click on the model, outside the gizmo box, toggles the hovered plate,
                // as egui's click-to-hide does.
                if self.camera_is_free()
                    && let Some(key) = self.hovered.as_ref().map(|hover| hover.key.clone())
                {
                    self.toggle_plate(key, cx);
                }
            }
            ClickAction::Nothing => {}
        }
        self.gizmo_press_in_box = false;
        // The release lands where the pointer is: pick again now the drag is over.
        if drag.is_some() && self.last_pointer.is_some() {
            self.pick_hovers(at);
        }
    }

    /// The press ended somewhere the viewport did not see its release.
    pub(super) fn pointer_cancelled(&mut self) {
        self.gizmo_press_in_box = false;
        self.drag = None;
    }

    /// A wheel turn of `notches` pixels, positive toward the scene.
    pub(super) fn wheel(&mut self, notches: f32, cx: &mut ViewCx) {
        if !self.is_gpu_ready() || notches == 0.0 {
            return;
        }
        // The wheel moves the locked camera between the inner and outer orbit rather than pulling
        // it back off them.
        if self.perspective.enabled {
            if self.zoom_perspective(notches) {
                self.refresh_perspective(cx);
            }
        } else {
            self.viewport.camera.zoom(notches);
        }
        self.viewport.mark_dirty();
        cx.emit(ViewportChange::Camera);
    }

    /// Ctrl+S opens and closes the options rail. Ctrl+Z undoes; Ctrl+Shift+Z or Ctrl+R redoes.
    /// Otherwise WASD moves the camera target, the arrows move it vertically and orbit, and
    /// Shift+arrows pan, for as long as the key is held.
    pub(super) fn key_down(
        &mut self,
        key: ViewKey<'_>,
        modifiers: KeyModifiers,
        repeat: bool,
        cx: &mut ViewCx,
    ) -> KeyOutcome {
        if !self.is_gpu_ready() {
            return KeyOutcome::Ignored;
        }
        if !repeat && modifiers.primary {
            match key {
                ViewKey::Character("s") => return KeyOutcome::ToggleOptions,
                ViewKey::Character("z") if !modifiers.shift => {
                    self.undo_visibility(cx);
                    return KeyOutcome::Handled;
                }
                ViewKey::Character("r") | ViewKey::Character("z") => {
                    self.redo_visibility(cx);
                    return KeyOutcome::Handled;
                }
                _ => {}
            }
        }
        let Some(key) = MoveKey::from_key(key, modifiers.shift) else { return KeyOutcome::Ignored };
        if self.held_keys.insert(key) {
            // The first frame of the motion is drawn by the renderer, which keeps asking for
            // frames while a key is held.
            self.viewport.mark_dirty();
        }

        KeyOutcome::Handled
    }

    /// Lets go of a released movement key. The motion stops on the next frame with nothing held.
    pub(super) fn key_up(&mut self, key: ViewKey<'_>) {
        for key in MoveKey::released_by(key) {
            self.held_keys.remove(&key);
        }
    }

    /// Focus left the view, which hears no release for the keys held: they all stop.
    pub(super) fn release_keys(&mut self) {
        self.held_keys.clear();
    }

    /// Applies one frame's camera movement for every held movement key. `dt` is the time since the
    /// previous frame, which the pan is scaled by. Says whether the camera moved.
    pub(super) fn apply_held_keys(&mut self, dt: Duration) -> bool {
        if self.held_keys.is_empty() || !self.camera_is_free() {
            return false;
        }

        let motion = HeldMotion::from_keys(&self.held_keys);
        if motion.forward != 0.0 || motion.right != 0.0 {
            self.viewport.camera.wasd(motion.forward, motion.right);
        }
        if motion.vertical != 0.0 {
            self.viewport.camera.move_vertical(motion.vertical);
        }
        if motion.rotate != 0.0 {
            self.viewport.camera.rotate_horizontal(motion.rotate);
        }
        if motion.pan_x != 0.0 || motion.pan_y != 0.0 {
            let size = self.viewport_size();
            self.viewport.camera.pan_keys((motion.pan_x, motion.pan_y), dt.as_secs_f32(), size);
        }

        true
    }

    /// Turns the camera to look along a signed axis, eased over a short animation.
    pub(super) fn snap_camera(&mut self, axis: Axis, positive: bool, cx: &mut ViewCx) {
        let (azimuth, elevation) = camera::ortho_view(axis, positive, self.viewport.camera.azimuth);
        self.viewport.camera.animate_to(azimuth, elevation, GIZMO_SNAP_DURATION_SECS);
        self.viewport.mark_dirty();
        cx.emit(ViewportChange::Camera);
    }

    /// Advances what moves on its own for the frame drawn at `now`: the held keys, a gizmo snap
    /// in flight and the hit rings. Says whether another frame is wanted.
    ///
    /// Every frame of a snap is a camera change, so a mirrored pane follows the whole eased move.
    pub(super) fn advance_frame(&mut self, now: Timestamp, cx: &mut ViewCx) -> bool {
        let dt = self.last_frame.map_or(FRAME_STEP, |last| now.saturating_since(last).min(LONGEST_FRAME));
        if self.apply_held_keys(dt) {
            self.viewport.mark_dirty();
            cx.emit(ViewportChange::Camera);
        }
        if self.viewport.camera.animation.is_some() {
            self.viewport.camera.update_animation(dt.as_secs_f32());
            self.viewport.mark_dirty();
            cx.emit(ViewportChange::Camera);
        }

        let moving = !self.held_keys.is_empty() || self.viewport.camera.animation.is_some() || self.prune_pulses(cx);
        self.last_frame = moving.then_some(now);
        moving
    }

    /// Drops the hit rings that have finished spreading, and every ring under reduced motion.
    /// Says whether any is still spreading.
    fn prune_pulses(&mut self, cx: &ViewCx) -> bool {
        if cx.reduce_motion() {
            self.hit_pulses.clear();
        }
        let now = std::time::Instant::now();
        self.hit_pulses.retain(|pulse| now.saturating_duration_since(pulse.started) < super::depth_cues::PULSE);

        !self.hit_pulses.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use wows_toolkit_armor::viewport::camera::Axis;
    use zgui::vocab::PointerButton;
    use zgui::vocab::Timestamp;

    use super::ClickAction;
    use super::HeldMotion;
    use super::KeyModifiers;
    use super::KeyOutcome;
    use super::ViewKey;
    use super::click_action;
    use crate::tabs::armor_viewer::viewport::model::MoveKey;
    use crate::tabs::armor_viewer::viewport::model::ViewportModel;
    use crate::tabs::armor_viewer::viewport::state::ViewCx;
    use crate::tabs::armor_viewer::viewport::types::ViewportChange;

    #[test]
    fn shift_arrows_pan_instead_of_orbit_and_rise() {
        assert_eq!(MoveKey::from_key(ViewKey::Up, false), Some(MoveKey::Up));
        assert_eq!(MoveKey::from_key(ViewKey::Left, false), Some(MoveKey::RotateLeft));
        assert_eq!(MoveKey::from_key(ViewKey::Up, true), Some(MoveKey::PanUp));
        assert_eq!(MoveKey::from_key(ViewKey::Left, true), Some(MoveKey::PanLeft));
        assert_eq!(MoveKey::from_key(ViewKey::Character("w"), true), Some(MoveKey::Forward));

        let motion = HeldMotion::from_keys(&[MoveKey::PanRight, MoveKey::PanUp]);
        assert_eq!((motion.pan_x, motion.pan_y), (1.0, 1.0));
        assert_eq!(motion.rotate, 0.0);
        assert_eq!(motion.vertical, 0.0);
    }

    #[test]
    fn releasing_an_arrow_stops_its_pan_whatever_shift_is() {
        let released: Vec<MoveKey> = MoveKey::released_by(ViewKey::Left).collect();
        assert_eq!(released, vec![MoveKey::RotateLeft, MoveKey::PanLeft]);
    }

    #[test]
    fn a_shift_click_casts_and_a_plain_click_hides_a_plate() {
        assert_eq!(click_action(PointerButton::Primary, true, false, false, true), ClickAction::Cast);
        assert_eq!(click_action(PointerButton::Primary, true, false, false, false), ClickAction::TogglePlate);
        assert_eq!(click_action(PointerButton::Primary, true, false, true, false), ClickAction::Burst);
        assert_eq!(click_action(PointerButton::Primary, true, false, true, true), ClickAction::Cast);
    }

    #[test]
    fn a_shift_drag_orbits_and_the_gizmo_snaps_without_casting() {
        assert_eq!(click_action(PointerButton::Primary, false, false, false, true), ClickAction::Nothing);
        assert_eq!(click_action(PointerButton::Primary, true, true, false, true), ClickAction::SnapGizmo);
    }

    #[test]
    fn the_middle_button_never_casts() {
        assert_eq!(click_action(PointerButton::Middle, true, false, false, true), ClickAction::Nothing);
        assert_eq!(click_action(PointerButton::Secondary, true, false, true, true), ClickAction::Nothing);
    }

    #[test]
    fn a_held_key_asks_for_frames_until_it_is_released() {
        let mut model = ViewportModel::new();
        model.held_keys.insert(MoveKey::RotateLeft);
        let mut cx = ViewCx::for_test();
        let azimuth = model.viewport.camera.azimuth;

        assert!(model.advance_frame(Timestamp::from_origin(Duration::from_millis(16)), &mut cx));
        assert_ne!(model.viewport.camera.azimuth, azimuth, "the camera turned");
        assert_eq!(cx.events(), &[ViewportChange::Camera]);

        model.key_up(ViewKey::Left);
        let mut cx = ViewCx::for_test();
        assert!(!model.advance_frame(Timestamp::from_origin(Duration::from_millis(32)), &mut cx));
        assert!(cx.events().is_empty());
    }

    #[test]
    fn a_snap_animates_to_the_axis_and_then_stops_asking_for_frames() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();
        model.snap_camera(Axis::X, true, &mut cx);
        let mut at = Duration::ZERO;
        let mut frames = 0;
        while model.advance_frame(Timestamp::from_origin(at), &mut cx) {
            at += Duration::from_millis(16);
            frames += 1;
            assert!(frames < 100, "the snap ends");
        }
        let (azimuth, elevation) =
            wows_toolkit_armor::viewport::camera::ortho_view(Axis::X, true, model.viewport.camera.azimuth);
        assert!((model.viewport.camera.elevation - elevation).abs() < 1e-4);
        assert!(
            (model.viewport.camera.azimuth.rem_euclid(std::f32::consts::TAU)
                - azimuth.rem_euclid(std::f32::consts::TAU))
            .abs()
                < 1e-3
        );
    }

    #[test]
    fn keys_need_a_device() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();
        let outcome = model.key_down(ViewKey::Character("w"), KeyModifiers::default(), false, &mut cx);
        assert_eq!(outcome, KeyOutcome::Ignored);
        assert!(model.held_keys.is_empty());
    }
}
