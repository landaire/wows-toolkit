use crate::viewport_3d::camera::ArcballCamera;
use crate::viewport_3d::camera::Axis;

pub(crate) const BOX_SIZE: f32 = 64.0;
pub(crate) const ARM_LEN: f32 = 22.0;
pub(crate) const BALL_R: f32 = 7.0;
const MARGIN: f32 = 8.0;

#[derive(Default)]
pub struct NavGizmo {
    hovered: Option<(Axis, bool)>,
    dragging: bool,
}

fn axis_color(axis: Axis) -> egui::Color32 {
    match axis {
        Axis::X => egui::Color32::from_rgb(220, 70, 70),
        Axis::Y => egui::Color32::from_rgb(90, 200, 90),
        Axis::Z => egui::Color32::from_rgb(80, 130, 230),
    }
}

fn axis_label(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "X",
        Axis::Y => "Y",
        Axis::Z => "Z",
    }
}

/// Top-right corner box for the gizmo, inset from the viewport edges.
pub(crate) fn gizmo_rect(viewport: egui::Rect) -> egui::Rect {
    let max = egui::pos2(viewport.right() - MARGIN, viewport.top() + MARGIN);
    egui::Rect::from_min_max(egui::pos2(max.x - BOX_SIZE, max.y), egui::pos2(max.x, max.y + BOX_SIZE))
}

fn axis_unit(axis: Axis, positive: bool) -> [f32; 3] {
    let s = if positive { 1.0 } else { -1.0 };
    match axis {
        Axis::X => [s, 0.0, 0.0],
        Axis::Y => [0.0, s, 0.0],
        Axis::Z => [0.0, 0.0, s],
    }
}

/// Foreshortened screen-space projection of a signed world axis under the camera view, plus view-space depth.
///
/// `view` is column-major: `view[col][row]`. Transforms the axis as a direction (w=0).
/// Returns the raw (un-normalized) egui Vec2 in screen space (Y-down) with magnitude in [0, 1]:
/// ~1 when the axis lies in the screen plane, ~0 when it points toward/away from the camera.
/// Arms drawn at `center + dir * ARM_LEN` foreshorten naturally instead of collapsing to full length.
pub(crate) fn axis_screen_dir(view: &[[f32; 4]; 4], axis: Axis, positive: bool) -> (egui::Vec2, f32) {
    let a = axis_unit(axis, positive);
    let mut out = [0.0f32; 3];
    for c in 0..3 {
        for r in 0..3 {
            out[r] += view[c][r] * a[c];
        }
    }
    // egui Y is down, so negate the view-space Y to get screen-up = negative Y
    (egui::vec2(out[0], -out[1]), out[2])
}

/// The six signed axes paired with screen direction, ordered so the ball nearest
/// the camera (largest view-space depth) comes LAST and draws on top.
pub(crate) fn ball_draw_order(view: &[[f32; 4]; 4]) -> Vec<(Axis, bool, egui::Vec2, f32)> {
    let mut balls: Vec<(Axis, bool, egui::Vec2, f32)> = Vec::with_capacity(6);
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        for positive in [true, false] {
            let (dir, depth) = axis_screen_dir(view, axis, positive);
            balls.push((axis, positive, dir, depth));
        }
    }
    balls.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));
    balls
}

/// Which signed axis ball (if any) is nearest to `pointer` within BALL_R.
pub(crate) fn hit_test(rect: egui::Rect, camera: &ArcballCamera, pointer: egui::Pos2) -> Option<(Axis, bool)> {
    let view = camera.view_matrix();
    let center = rect.center();
    let mut best: Option<(Axis, bool, f32)> = None;
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        for positive in [true, false] {
            let (dir, _depth) = axis_screen_dir(&view, axis, positive);
            let p = center + dir * ARM_LEN;
            let d = p.distance(pointer);
            if d <= BALL_R && best.is_none_or(|(_, _, bd)| d < bd) {
                best = Some((axis, positive, d));
            }
        }
    }
    best.map(|(a, p, _)| (a, p))
}

pub(crate) fn draw(painter: &egui::Painter, rect: egui::Rect, camera: &ArcballCamera, state: &NavGizmo) {
    let center = rect.center();
    for (axis, positive, dir, _depth) in ball_draw_order(&camera.view_matrix()) {
        let color = axis_color(axis);
        let tip = center + dir * ARM_LEN;
        if positive {
            painter.line_segment([center, tip], egui::Stroke::new(2.0, color));
            painter.circle_filled(tip, BALL_R, color);
            painter.text(
                tip,
                egui::Align2::CENTER_CENTER,
                axis_label(axis),
                egui::FontId::proportional(10.0),
                egui::Color32::BLACK,
            );
        } else {
            painter.circle_stroke(tip, BALL_R, egui::Stroke::new(1.5, color));
        }
        if state.hovered == Some((axis, positive)) {
            painter.circle_stroke(tip, BALL_R + 2.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
        }
    }
}

/// Returns true if the gizmo consumed the pointer (caller skips its own orbit when true).
pub(crate) fn interact(
    state: &mut NavGizmo,
    response: &egui::Response,
    rect: egui::Rect,
    camera: &mut ArcballCamera,
) -> bool {
    let pointer = response.hover_pos().or_else(|| response.interact_pointer_pos());
    state.hovered = pointer.and_then(|p| hit_test(rect, camera, p));

    let inside = pointer.is_some_and(|p| rect.contains(p));

    if response.drag_started() && inside {
        state.dragging = true;
    }
    if state.dragging {
        if response.dragged() {
            camera.orbit(response.drag_delta(), response.rect.size());
        }
        if response.drag_stopped() {
            state.dragging = false;
        }
        return true;
    }

    if response.clicked()
        && let Some(p) = pointer
        && let Some((axis, positive)) = hit_test(rect, camera, p)
    {
        let (az, el) = crate::viewport_3d::camera::ortho_view(axis, positive, camera.azimuth);
        camera.animate_to(az, el, 0.35);
        return true;
    }

    // Hovering over the gizmo box consumes so the underlying orbit does not fire.
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_3d::camera::ArcballCamera;
    use crate::viewport_3d::camera::Axis;
    use crate::viewport_3d::types::Vec3;

    fn cam_facing_neg_z() -> ArcballCamera {
        let mut c = ArcballCamera::from_bounds(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        c.azimuth = 0.0;
        c.elevation = 0.0;
        c
    }

    #[test]
    fn world_up_points_up_on_screen() {
        let c = cam_facing_neg_z();
        let (dir, _depth) = axis_screen_dir(&c.view_matrix(), Axis::Y, true);
        assert!(dir.y < -0.5, "dir={:?}", dir);
        assert!(dir.x.abs() < 0.3, "dir={:?}", dir);
    }

    #[test]
    fn hit_test_center_of_a_ball_hits_it() {
        let c = cam_facing_neg_z();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
        let g = gizmo_rect(rect);
        let (dir, _) = axis_screen_dir(&c.view_matrix(), Axis::Y, true);
        let ball = g.center() + dir * ARM_LEN;
        assert_eq!(hit_test(g, &c, ball), Some((Axis::Y, true)));
        assert_eq!(hit_test(g, &c, g.center() + egui::vec2(1000.0, 1000.0)), None);
    }

    #[test]
    fn toward_camera_axis_has_larger_depth() {
        let mut c = ArcballCamera::from_bounds(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        c.azimuth = 0.0;
        c.elevation = 0.0;
        let view = c.view_matrix();
        let (_d_pos, depth_toward) = axis_screen_dir(&view, Axis::Z, true); // +Z toward camera (eye on +Z)
        let (_d_neg, depth_away) = axis_screen_dir(&view, Axis::Z, false); // -Z away
        assert!(depth_toward != depth_away);
        // Toward-camera axis maps to larger view-space z; ascending sort places it last (on top).
        assert!(depth_toward > depth_away, "toward={depth_toward} away={depth_away}");
    }

    #[test]
    fn nearest_ball_draws_last() {
        let mut c = ArcballCamera::from_bounds(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        c.azimuth = 0.0;
        c.elevation = 0.0; // eye on +Z, so +Z axis points toward camera
        let order = ball_draw_order(&c.view_matrix());
        let last = order.last().unwrap();
        assert_eq!((last.0, last.1), (Axis::Z, true), "nearest (+Z toward camera) must draw last");
        let max_depth = order.iter().map(|b| b.3).fold(f32::MIN, f32::max);
        assert!((last.3 - max_depth).abs() < 1e-6);
    }

    #[test]
    fn axis_pointing_at_camera_foreshortens() {
        let mut c = ArcballCamera::from_bounds(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        c.azimuth = 0.0;
        c.elevation = 0.0;
        let view = c.view_matrix();
        let (toward, _) = axis_screen_dir(&view, Axis::Z, true); // +Z points at camera
        let (in_plane, _) = axis_screen_dir(&view, Axis::X, true); // +X in screen plane
        assert!(toward.length() < 0.1, "toward len={}", toward.length());
        assert!(in_plane.length() > 0.9, "in_plane len={}", in_plane.length());
    }
}
