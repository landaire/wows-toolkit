use crate::viewport::camera::ArcballCamera;
use crate::viewport::camera::Axis;
use crate::viewport::types::Vec2;
use crate::viewport::types::ViewRect;

pub const BOX_SIZE: f32 = 64.0;
pub const ARM_LEN: f32 = 22.0;
pub const BALL_R: f32 = 7.0;
const MARGIN: f32 = 8.0;

/// Top-right corner box for the gizmo, inset from the viewport edges.
pub fn gizmo_rect(viewport: ViewRect) -> ViewRect {
    let max_x = viewport.right() - MARGIN;
    let max_y = viewport.top() + MARGIN;
    ViewRect::from_min_size(Vec2::new(max_x - BOX_SIZE, max_y), Vec2::new(BOX_SIZE, BOX_SIZE))
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
/// Returns the raw (un-normalized) screen-space vector (Y-down) with magnitude in [0, 1]:
/// ~1 when the axis lies in the screen plane, ~0 when it points toward/away from the camera.
/// Arms drawn at `center + dir * ARM_LEN` foreshorten naturally instead of collapsing to full length.
pub fn axis_screen_dir(view: &[[f32; 4]; 4], axis: Axis, positive: bool) -> (Vec2, f32) {
    let a = axis_unit(axis, positive);
    let mut out = [0.0f32; 3];
    for c in 0..3 {
        for r in 0..3 {
            out[r] += view[c][r] * a[c];
        }
    }
    // Screen Y is down, so negate the view-space Y to get screen-up = negative Y
    (Vec2::new(out[0], -out[1]), out[2])
}

/// One signed axis as the gizmo draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisProjection {
    pub axis: Axis,
    pub positive: bool,
    /// Foreshortened screen direction; see [`axis_screen_dir`].
    pub dir: Vec2,
    /// View-space depth; larger is nearer the camera.
    pub depth: f32,
}

/// The six signed axes in a fixed order.
fn axis_projections(view: &[[f32; 4]; 4]) -> Vec<AxisProjection> {
    let mut balls = Vec::with_capacity(6);
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        for positive in [true, false] {
            let (dir, depth) = axis_screen_dir(view, axis, positive);
            balls.push(AxisProjection { axis, positive, dir, depth });
        }
    }
    balls
}

/// The six signed axes ordered so the ball nearest the camera (largest
/// view-space depth) comes LAST and draws on top.
pub fn ball_draw_order(view: &[[f32; 4]; 4]) -> Vec<AxisProjection> {
    let mut balls = axis_projections(view);
    // Depths come from a finite view matrix; an incomparable pair keeps its order.
    balls.sort_by(|a, b| a.depth.partial_cmp(&b.depth).unwrap_or(std::cmp::Ordering::Equal));
    balls
}

/// Which signed axis ball (if any) is nearest to `pointer` within BALL_R.
pub fn hit_test(rect: ViewRect, camera: &ArcballCamera, pointer: Vec2) -> Option<(Axis, bool)> {
    let center = rect.center();
    let mut best: Option<(AxisProjection, f32)> = None;
    for ball in axis_projections(&camera.view_matrix()) {
        let d = (center + ball.dir * ARM_LEN - pointer).norm();
        if d <= BALL_R && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((ball, d));
        }
    }
    best.map(|(ball, _)| (ball.axis, ball.positive))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::camera::ArcballCamera;
    use crate::viewport::camera::Axis;
    use crate::viewport::types::Bounds;
    use crate::viewport::types::Vec3;

    fn unit_box() -> Bounds {
        Bounds { min: Vec3::new(-1.0, -1.0, -1.0), max: Vec3::new(1.0, 1.0, 1.0) }
    }

    fn cam_facing_neg_z() -> ArcballCamera {
        let mut c = ArcballCamera::from_bounds(unit_box());
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
        let rect = ViewRect::from_min_size(Vec2::new(0.0, 0.0), Vec2::new(400.0, 300.0));
        let g = gizmo_rect(rect);
        let (dir, _) = axis_screen_dir(&c.view_matrix(), Axis::Y, true);
        let ball = g.center() + dir * ARM_LEN;
        assert_eq!(hit_test(g, &c, ball), Some((Axis::Y, true)));
        assert_eq!(hit_test(g, &c, g.center() + Vec2::new(1000.0, 1000.0)), None);
    }

    #[test]
    fn toward_camera_axis_has_larger_depth() {
        let mut c = ArcballCamera::from_bounds(unit_box());
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
        let mut c = ArcballCamera::from_bounds(unit_box());
        c.azimuth = 0.0;
        c.elevation = 0.0; // eye on +Z, so +Z axis points toward camera
        let order = ball_draw_order(&c.view_matrix());
        let last = order.last().unwrap();
        assert_eq!((last.axis, last.positive), (Axis::Z, true), "nearest (+Z toward camera) must draw last");
        let max_depth = order.iter().map(|b| b.depth).fold(f32::MIN, f32::max);
        assert!((last.depth - max_depth).abs() < 1e-6);
    }

    #[test]
    fn axis_pointing_at_camera_foreshortens() {
        let mut c = ArcballCamera::from_bounds(unit_box());
        c.azimuth = 0.0;
        c.elevation = 0.0;
        let view = c.view_matrix();
        let (toward, _) = axis_screen_dir(&view, Axis::Z, true); // +Z points at camera
        let (in_plane, _) = axis_screen_dir(&view, Axis::X, true); // +X in screen plane
        assert!(toward.norm() < 0.1, "toward len={}", toward.norm());
        assert!(in_plane.norm() > 0.9, "in_plane len={}", in_plane.norm());
    }
}
