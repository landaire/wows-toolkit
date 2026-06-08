use crate::viewport_3d::camera::{ArcballCamera, Axis};

pub(crate) const BOX_SIZE: f32 = 64.0;
pub(crate) const ARM_LEN: f32 = 22.0;
pub(crate) const BALL_R: f32 = 7.0;
const MARGIN: f32 = 8.0;

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

/// Screen-space direction of a signed world axis under the camera view, plus view-space depth.
///
/// `view` is column-major: `view[col][row]`. Transforms the axis as a direction (w=0).
/// Returns a unit egui Vec2 in screen space (Y-down) and the view-space z.
pub(crate) fn axis_screen_dir(view: &[[f32; 4]; 4], axis: Axis, positive: bool) -> (egui::Vec2, f32) {
    let a = axis_unit(axis, positive);
    let mut out = [0.0f32; 3];
    for c in 0..3 {
        for r in 0..3 {
            out[r] += view[c][r] * a[c];
        }
    }
    // egui Y is down, so negate the view-space Y to get screen-up = negative Y
    let dir = egui::vec2(out[0], -out[1]);
    let n = dir.length();
    let dir = if n > 1e-6 { dir / n } else { egui::Vec2::ZERO };
    (dir, out[2])
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_3d::camera::{ArcballCamera, Axis};
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
}
