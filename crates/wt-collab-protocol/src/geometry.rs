//! The maths behind an annotation: how near a point is to one, which way an
//! arrow points, and how a freehand stroke is tidied.
//!
//! Here rather than in a front end because both of them need it and neither
//! owns it. The egui app keeps its own mirror of [`Annotation`] in egui's
//! vector types and dispatches on that; a front end with no painter holds the
//! wire type directly. Both reach the same formulas through this module, so a
//! stroke cannot be hit-tested one way in one app and another way in the
//! other.

use crate::types::Annotation;
use wows_minimap_renderer::draw_command::AnnotationShape;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipVisibility;
use wows_minimap_renderer::map_data::MinimapPos;
use wowsunpack::game_types::EntityId;

/// Below this a segment is a point, and its own start is the nearest thing on
/// it.
const DEGENERATE: f32 = 0.001;

fn length(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// How far `at` is from the segment `a`-`b`.
pub fn distance_to_segment(at: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = sub(b, a);
    let ap = sub(at, a);
    let len_sq = ab[0] * ab[0] + ab[1] * ab[1];
    if len_sq < DEGENERATE {
        return length(ap);
    }
    let travel = ((ap[0] * ab[0] + ap[1] * ab[1]) / len_sq).clamp(0.0, 1.0);
    length(sub(at, [a[0] + ab[0] * travel, a[1] + ab[1] * travel]))
}

/// How far `at` is from the nearest part of a run of points.
///
/// Takes the points as an iterator so a caller holding them in its own vector
/// type passes them without building another.
pub fn distance_to_polyline(points: impl IntoIterator<Item = [f32; 2]>, at: [f32; 2]) -> f32 {
    let mut nearest = f32::MAX;
    let mut previous: Option<[f32; 2]> = None;
    for point in points {
        if let Some(from) = previous {
            nearest = nearest.min(distance_to_segment(at, from, point));
        }
        previous = Some(point);
    }
    nearest
}

/// How far `at` is from a circle, which is nothing at all when inside it.
pub fn distance_to_circle(center: [f32; 2], radius: f32, at: [f32; 2]) -> f32 {
    let from_center = length(sub(at, center));
    if from_center <= radius { 0.0 } else { from_center - radius }
}

/// How far `at` is from a rectangle turned by `rotation`.
pub fn distance_to_rotated_rect(center: [f32; 2], half_size: [f32; 2], rotation: f32, at: [f32; 2]) -> f32 {
    let offset = sub(at, center);
    let (sin, cos) = rotation.sin_cos();
    // Into the rectangle's own frame, where its sides are the axes.
    let local = [offset[0] * cos + offset[1] * sin, -offset[0] * sin + offset[1] * cos];
    let dx = (local[0].abs() - half_size[0]).max(0.0);
    let dy = (local[1].abs() - half_size[1]).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// The corners of an equilateral triangle of `radius` about `center`, the
/// first pointing up before `rotation` turns it.
pub fn triangle_corners(center: [f32; 2], radius: f32, rotation: f32) -> [[f32; 2]; 3] {
    std::array::from_fn(|corner| {
        let angle = rotation + corner as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
        [center[0] + radius * angle.cos(), center[1] + radius * angle.sin()]
    })
}

/// How far `at` is from a triangle, which is nothing at all well inside it.
pub fn distance_to_triangle(center: [f32; 2], radius: f32, rotation: f32, at: [f32; 2]) -> f32 {
    // The inscribed circle is entirely inside the triangle whatever its
    // rotation, so a point within it needs no edge test.
    if length(sub(at, center)) <= radius * 0.5 {
        return 0.0;
    }
    let corners = triangle_corners(center, radius, rotation);
    (0..3).map(|edge| distance_to_segment(at, corners[edge], corners[(edge + 1) % 3])).fold(f32::MAX, f32::min)
}

/// How far `at` is from `annotation`, in minimap space.
pub fn annotation_distance(annotation: &Annotation, at: [f32; 2]) -> f32 {
    match annotation {
        Annotation::Ship { pos, .. } => length(sub(*pos, at)),
        Annotation::FreehandStroke { points, .. } | Annotation::Arrow { points, .. } => {
            distance_to_polyline(points.iter().copied(), at)
        }
        Annotation::Line { start, end, .. } | Annotation::Measurement { start, end, .. } => {
            distance_to_segment(at, *start, *end)
        }
        Annotation::Circle { center, radius, .. } => distance_to_circle(*center, *radius, at),
        Annotation::Rectangle { center, half_size, rotation, .. } => {
            distance_to_rotated_rect(*center, *half_size, *rotation, at)
        }
        Annotation::Triangle { center, radius, rotation, .. } => distance_to_triangle(*center, *radius, *rotation, at),
    }
}

/// How far back along a drawn arrow its head takes its bearing from.
const ARROW_TRAILING_DISTANCE: f32 = 30.0;

/// At most this many points back, however short they are.
const MAX_TRAILING_POINTS: usize = 10;

/// Which way the head of an arrow drawn through `points` should face.
///
/// The last stretch of the stroke rather than its final segment: a hand
/// wobbles at the end of a drag, and a head taken from the last two points
/// alone jitters with it. Segments count for less the further back they are.
pub fn arrow_direction(points: &[[f32; 2]]) -> [f32; 2] {
    let count = points.len();
    if count < 2 {
        return [1.0, 0.0];
    }

    let mut accumulated = [0.0_f32; 2];
    let mut total_weight = 0.0_f32;
    let mut walked = 0.0_f32;
    for back in 1..count.min(MAX_TRAILING_POINTS + 1) {
        let at = count - 1 - back;
        let segment = sub(points[at + 1], points[at]);
        let len = length(segment);
        if len < DEGENERATE {
            continue;
        }
        let weight = 1.0 / back as f32;
        accumulated[0] += segment[0] / len * weight;
        accumulated[1] += segment[1] / len * weight;
        total_weight += weight;
        walked += len;
        if walked >= ARROW_TRAILING_DISTANCE {
            break;
        }
    }

    if total_weight > 0.0 {
        let averaged = [accumulated[0] / total_weight, accumulated[1] / total_weight];
        let len = length(averaged);
        if len > DEGENERATE {
            return [averaged[0] / len, averaged[1] / len];
        }
    }

    // A stroke that doubled back on itself averages to nothing, so fall back
    // to where it ended up relative to where it began.
    let overall = sub(points[count - 1], points[0]);
    let len = length(overall);
    if len > DEGENERATE { [overall[0] / len, overall[1] / len] } else { [1.0, 0.0] }
}

/// Tidies a freehand stroke: drops the points a hand put in without meaning
/// to, then rounds off what is left.
pub fn smooth_freehand(points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    if points.len() <= 2 {
        return points;
    }

    // How much to simplify by follows the size of the stroke, so a small one
    // is not flattened into a line and a large one is not left ragged.
    let mut low = points[0];
    let mut high = points[0];
    for point in &points {
        low[0] = low[0].min(point[0]);
        low[1] = low[1].min(point[1]);
        high[0] = high[0].max(point[0]);
        high[1] = high[1].max(point[1]);
    }
    let epsilon = (length(sub(high, low)) * 0.012).max(0.3);

    let mut result = simplify(&points, epsilon);
    for _ in 0..2 {
        result = subdivide(&result);
    }
    result
}

/// Ramer-Douglas-Peucker: keeps the points that carry the shape.
///
/// A stretch shorter than [`DEGENERATE`] is measured from its start rather
/// than projected onto, which is what [`distance_to_segment`] does for every
/// other caller; at that length the two answers differ by far less than a
/// pixel.
fn simplify(points: &[[f32; 2]], epsilon: f32) -> Vec<[f32; 2]> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let first = points[0];
    let last = *points.last().expect("a run of more than two points has a last");

    let mut furthest = 0.0_f32;
    let mut at = 0;
    for (index, point) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let distance = distance_to_segment(*point, first, last);
        if distance > furthest {
            furthest = distance;
            at = index;
        }
    }

    if furthest > epsilon {
        let mut kept = simplify(&points[..=at], epsilon);
        let rest = simplify(&points[at..], epsilon);
        // The split point ends one half and starts the other.
        kept.pop();
        kept.extend(rest);
        kept
    } else {
        vec![first, last]
    }
}

/// One pass of Chaikin corner cutting.
fn subdivide(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 2);
    out.push(points[0]);
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let along = sub(b, a);
        out.push([a[0] + along[0] * 0.25, a[1] + along[1] * 0.25]);
        out.push([a[0] + along[0] * 0.75, a[1] + along[1] * 0.75]);
    }
    out.push(*points.last().expect("a run of more than two points has a last"));
    out
}

/// How many metres one unit of the game's own world space is.
const BW_TO_METERS: f32 = 30.0;

/// A minimap distance in kilometres, for a map `space_size` wide.
pub fn minimap_distance_to_km(distance: f32, space_size: f32) -> f32 {
    let world = distance / wows_minimap_renderer::MINIMAP_SIZE as f32 * space_size;
    world * BW_TO_METERS / 1000.0
}

/// The inverse: kilometres as a minimap distance.
pub fn km_to_minimap_distance(km: f32, space_size: f32) -> f32 {
    let world = km * 1000.0 / BW_TO_METERS;
    world / space_size * wows_minimap_renderer::MINIMAP_SIZE as f32
}

/// What a placed ship is tinted, as the egui toolbar tints it.
const FRIENDLY: [u8; 3] = [76, 232, 170];
const ENEMY: [u8; 3] = [254, 77, 42];

/// How long an arrow's head is, relative to the line's width.
const ARROW_HEAD_LENGTH: f32 = 6.0;

/// How wide it is, relative to its length.
const ARROW_HEAD_SPREAD: f32 = 0.6;

fn at(point: [f32; 2]) -> MinimapPos {
    MinimapPos { x: point[0], y: point[1] }
}

fn run(points: &[[f32; 2]]) -> Vec<MinimapPos> {
    points.iter().map(|point| at(*point)).collect()
}

/// What to draw for `annotation`, in the order it should be drawn.
///
/// The target knows three shapes, so the drawing tools turn into those here
/// rather than every renderer learning each tool. An arrow is two commands:
/// its shaft and its filled head. A ship is the renderer's own ship command,
/// so a placed one is drawn from the same icons as a real one; its range
/// circles are not here, because a range is read out of the ship's game
/// data, which this layer does not carry.
pub fn annotation_commands(annotation: &Annotation) -> Vec<DrawCommand> {
    match annotation {
        Annotation::Ship { pos, yaw, species, friendly, config } => {
            vec![DrawCommand::Ship {
                // Not a ship in the battle, so it answers to no entity. The
                // viewport picks ships out of the baked frame rather than
                // out of what is drawn over it, so nothing looks this up.
                entity_id: EntityId::from(0_u32),
                pos: at(*pos),
                yaw: *yaw,
                species: Some(species.clone()),
                color: Some(if *friendly { FRIENDLY } else { ENEMY }),
                visibility: ShipVisibility::Visible,
                opacity: 1.0,
                is_self: false,
                // A placed ship belongs to nobody, which is also what keeps
                // it out of anything keyed by player.
                player_name: None,
                ship_name: config.as_ref().map(|config| config.ship_name.clone()).filter(|name| !name.is_empty()),
                is_detected_teammate: false,
                is_disconnected: false,
                name_color: None,
            }]
        }
        Annotation::FreehandStroke { points, color, width } => {
            vec![DrawCommand::Annotation {
                shape: AnnotationShape::Polyline { points: run(points) },
                color: *color,
                width: *width,
            }]
        }
        Annotation::Line { start, end, color, width } | Annotation::Measurement { start, end, color, width } => {
            vec![DrawCommand::Annotation {
                shape: AnnotationShape::Polyline { points: vec![at(*start), at(*end)] },
                color: *color,
                width: *width,
            }]
        }
        Annotation::Circle { center, radius, color, width, filled } => {
            vec![DrawCommand::Annotation {
                shape: AnnotationShape::Circle { center: at(*center), radius: *radius, filled: *filled },
                color: *color,
                width: *width,
            }]
        }
        Annotation::Rectangle { center, half_size, rotation, color, width, filled } => {
            let (sin, cos) = rotation.sin_cos();
            let corners: Vec<[f32; 2]> = [[-1.0_f32, -1.0_f32], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
                .into_iter()
                .map(|[sx, sy]| {
                    let (x, y) = (sx * half_size[0], sy * half_size[1]);
                    [center[0] + x * cos - y * sin, center[1] + x * sin + y * cos]
                })
                .collect();
            vec![DrawCommand::Annotation {
                shape: AnnotationShape::Polygon { points: run(&corners), filled: *filled },
                color: *color,
                width: *width,
            }]
        }
        Annotation::Triangle { center, radius, rotation, color, width, filled } => {
            let corners = triangle_corners(*center, *radius, *rotation);
            vec![DrawCommand::Annotation {
                shape: AnnotationShape::Polygon { points: run(&corners), filled: *filled },
                color: *color,
                width: *width,
            }]
        }
        Annotation::Arrow { points, color, width } => {
            let mut commands = vec![DrawCommand::Annotation {
                shape: AnnotationShape::Polyline { points: run(points) },
                color: *color,
                width: *width,
            }];
            if let Some(head) = arrow_head(points, *width) {
                commands.push(DrawCommand::Annotation {
                    shape: AnnotationShape::Polygon { points: run(&head), filled: true },
                    color: *color,
                    width: *width,
                });
            }
            commands
        }
    }
}

/// The three corners of an arrow's head, or `None` for a stroke with no tip
/// to put one on.
fn arrow_head(points: &[[f32; 2]], width: f32) -> Option<[[f32; 2]; 3]> {
    let tip = *points.last()?;
    if points.len() < 2 {
        return None;
    }
    let forward = arrow_direction(points);
    let length = width * ARROW_HEAD_LENGTH;
    let across = [-forward[1], forward[0]];
    let base = [tip[0] - forward[0] * length, tip[1] - forward[1] * length];
    let spread = length * ARROW_HEAD_SPREAD;
    Some([
        tip,
        [base[0] + across[0] * spread, base[1] + across[1] * spread],
        [base[0] - across[0] * spread, base[1] - across[1] * spread],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEAR: f32 = 1e-4;

    fn stroke(points: &[[f32; 2]]) -> Annotation {
        Annotation::FreehandStroke { points: points.to_vec(), color: [255, 0, 0, 255], width: 2.0 }
    }

    /// A point beside a segment measures to the nearest place on it, and one
    /// beyond an end measures to that end rather than to the infinite line.
    #[test]
    fn a_segment_is_measured_to_its_nearest_point_not_its_line() {
        assert!((distance_to_segment([5.0, 3.0], [0.0, 0.0], [10.0, 0.0]) - 3.0).abs() < NEAR);
        assert!((distance_to_segment([13.0, 4.0], [0.0, 0.0], [10.0, 0.0]) - 5.0).abs() < NEAR, "past the end");
        assert!((distance_to_segment([-3.0, 4.0], [0.0, 0.0], [10.0, 0.0]) - 5.0).abs() < NEAR, "before the start");
    }

    /// A segment of no length is a point.
    #[test]
    fn a_segment_of_no_length_is_a_point() {
        assert!((distance_to_segment([3.0, 4.0], [0.0, 0.0], [0.0, 0.0]) - 5.0).abs() < NEAR);
    }

    /// A stroke is measured to whichever of its parts is nearest, not to its
    /// ends.
    #[test]
    fn a_stroke_is_measured_to_its_nearest_part() {
        let zigzag = stroke(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]);
        assert!((annotation_distance(&zigzag, [5.0, 2.0]) - 2.0).abs() < NEAR, "beside the first leg");
        assert!((annotation_distance(&zigzag, [12.0, 5.0]) - 2.0).abs() < NEAR, "beside the second");
        // The corner is on the stroke, and the far side of the bend is not.
        assert!(annotation_distance(&zigzag, [10.0, 0.0]) < NEAR);
        assert!((annotation_distance(&zigzag, [0.0, 10.0]) - 10.0).abs() < NEAR, "across the bend");
    }

    /// Inside a shape is no distance at all, which is what makes a filled
    /// shape selectable by clicking its middle.
    #[test]
    fn the_inside_of_a_shape_is_no_distance_from_it() {
        assert_eq!(distance_to_circle([0.0, 0.0], 10.0, [3.0, 3.0]), 0.0);
        assert!((distance_to_circle([0.0, 0.0], 10.0, [0.0, 14.0]) - 4.0).abs() < NEAR);

        assert_eq!(distance_to_rotated_rect([0.0, 0.0], [10.0, 4.0], 0.0, [5.0, 2.0]), 0.0);
        assert_eq!(distance_to_triangle([0.0, 0.0], 10.0, 0.0, [0.0, 0.0]), 0.0);
    }

    /// A rectangle's rotation turns the shape, not the measurement: a point
    /// off its long side is outside until the rectangle turns to face it.
    #[test]
    fn a_rectangle_is_measured_in_its_own_frame() {
        let half = [10.0, 2.0];
        let at = [0.0, 6.0];
        assert!((distance_to_rotated_rect([0.0, 0.0], half, 0.0, at) - 4.0).abs() < NEAR, "off the long side");

        let quarter = std::f32::consts::FRAC_PI_2;
        assert_eq!(distance_to_rotated_rect([0.0, 0.0], half, quarter, at), 0.0, "and inside once it is turned");
    }

    /// An arrow's head follows the run-up to the tip rather than its last
    /// segment, so a wobble at the end does not swing it.
    #[test]
    fn an_arrow_takes_its_bearing_from_the_run_up_to_its_tip() {
        let mut points: Vec<[f32; 2]> = (0..20).map(|step| [step as f32 * 5.0, 0.0]).collect();
        let straight = arrow_direction(&points);
        assert!((straight[0] - 1.0).abs() < 0.01 && straight[1].abs() < 0.01, "{straight:?}");

        // A single short jink at the very end.
        points.push([96.0, 1.0]);
        let jinked = arrow_direction(&points);
        assert!(jinked[1].abs() < 0.5, "the head barely moves: {jinked:?}");
    }

    /// A stroke with no length at all still points somewhere, rather than
    /// handing back a direction of zero that would collapse the head.
    #[test]
    fn an_arrow_that_went_nowhere_still_points_somewhere() {
        assert_eq!(arrow_direction(&[]), [1.0, 0.0]);
        assert_eq!(arrow_direction(&[[4.0, 4.0]]), [1.0, 0.0]);
        assert_eq!(arrow_direction(&[[4.0, 4.0], [4.0, 4.0]]), [1.0, 0.0]);
    }

    /// Smoothing keeps the shape: the tidied stroke stays near the drawn one
    /// and keeps its ends exactly.
    #[test]
    fn smoothing_keeps_the_ends_and_the_shape() {
        let drawn: Vec<[f32; 2]> = (0..40)
            .map(|step| {
                let along = step as f32 * 2.0;
                // A gentle curve with a hand's jitter on it.
                [along, (along * 0.05).sin() * 20.0 + if step % 2 == 0 { 0.3 } else { -0.3 }]
            })
            .collect();
        let smoothed = smooth_freehand(drawn.clone());

        assert!(smoothed.len() >= 2);
        assert_eq!(smoothed[0], drawn[0], "it starts where the hand did");
        assert_eq!(*smoothed.last().expect("smoothing keeps both ends"), *drawn.last().expect("the drawn end"));

        for point in &drawn {
            assert!(
                distance_to_polyline(smoothed.iter().copied(), *point) < 3.0,
                "the tidied stroke stays under the drawn one at {point:?}"
            );
        }
    }

    /// Too short a stroke to tidy is handed back untouched.
    #[test]
    fn a_stroke_of_two_points_is_left_alone() {
        let drawn = vec![[0.0, 0.0], [10.0, 10.0]];
        assert_eq!(smooth_freehand(drawn.clone()), drawn);
    }

    fn shapes(annotation: &Annotation) -> Vec<AnnotationShape> {
        annotation_commands(annotation)
            .into_iter()
            .map(|command| match command {
                DrawCommand::Annotation { shape, .. } => shape,
                other => panic!("an annotation draws annotations, got {other:?}"),
            })
            .collect()
    }

    /// A rectangle is drawn as its four turned corners, and those corners are
    /// where hit testing says the rectangle is.
    #[test]
    fn a_rectangle_is_drawn_where_it_is_measured() {
        let rect = Annotation::Rectangle {
            center: [100.0, 100.0],
            half_size: [20.0, 10.0],
            rotation: std::f32::consts::FRAC_PI_4,
            color: [255, 255, 255, 255],
            width: 2.0,
            filled: false,
        };

        let drawn = shapes(&rect);
        let [AnnotationShape::Polygon { points, filled }] = &drawn[..] else {
            panic!("a rectangle draws one polygon, got {drawn:?}");
        };
        assert!(!filled);
        assert_eq!(points.len(), 4);
        for corner in points {
            assert!(
                annotation_distance(&rect, [corner.x, corner.y]) < 0.01,
                "a drawn corner is on the rectangle: {corner:?}"
            );
        }
        // And the corners are actually turned, not axis-aligned.
        assert!(points.iter().all(|corner| (corner.x - 100.0).abs() > 0.01));
    }

    /// A triangle draws the same three corners its hit test measures to.
    #[test]
    fn a_triangle_draws_the_corners_it_is_measured_by() {
        let triangle = Annotation::Triangle {
            center: [50.0, 50.0],
            radius: 12.0,
            rotation: 0.3,
            color: [0, 255, 0, 200],
            width: 1.5,
            filled: true,
        };

        let drawn = shapes(&triangle);
        let [AnnotationShape::Polygon { points, filled }] = &drawn[..] else {
            panic!("a triangle draws one polygon, got {drawn:?}");
        };
        assert!(filled);
        let corners = triangle_corners([50.0, 50.0], 12.0, 0.3);
        for (drawn, expected) in points.iter().zip(corners) {
            assert!((drawn.x - expected[0]).abs() < 0.01 && (drawn.y - expected[1]).abs() < 0.01);
        }
    }

    /// An arrow is its shaft and a filled head at the tip, pointing the way
    /// the stroke was going.
    #[test]
    fn an_arrow_draws_a_shaft_and_a_head_at_its_tip() {
        let arrow = Annotation::Arrow {
            points: vec![[0.0, 0.0], [30.0, 0.0], [60.0, 0.0]],
            color: [255, 0, 0, 255],
            width: 2.0,
        };

        let drawn = shapes(&arrow);
        let [AnnotationShape::Polyline { points: shaft }, AnnotationShape::Polygon { points: head, filled }] =
            &drawn[..]
        else {
            panic!("an arrow draws a shaft and a head, got {drawn:?}");
        };
        assert!(filled, "the head is solid");
        assert_eq!(shaft.len(), 3);
        assert_eq!(head.len(), 3);

        // The head starts at the tip and lies behind it, along the stroke.
        assert!((head[0].x - 60.0).abs() < 0.01 && head[0].y.abs() < 0.01);
        assert!(head[1].x < 60.0 && head[2].x < 60.0, "the barbs trail the tip");
        assert!(head[1].y * head[2].y < 0.0, "one either side of the shaft");
    }

    /// A stroke of one point has no direction and so grows no head, rather
    /// than drawing a spike in an arbitrary direction.
    #[test]
    fn an_arrow_of_one_point_grows_no_head() {
        let stub = Annotation::Arrow { points: vec![[5.0, 5.0]], color: [255, 0, 0, 255], width: 2.0 };
        assert_eq!(shapes(&stub).len(), 1, "the shaft alone");
    }

    /// A placed ship is drawn from the renderer's own ship icons, tinted by
    /// which side it is on, and belongs to no player.
    #[test]
    fn a_placed_ship_is_drawn_as_a_ship() {
        let ship = |friendly: bool| Annotation::Ship {
            pos: [10.0, 20.0],
            yaw: 1.25,
            species: "Cruiser".to_string(),
            friendly,
            config: None,
        };

        let drawn = annotation_commands(&ship(true));
        let [DrawCommand::Ship { pos, yaw, species, color, player_name, ship_name, .. }] = &drawn[..] else {
            panic!("a placed ship draws a ship, got {drawn:?}");
        };
        assert_eq!((pos.x, pos.y), (10.0, 20.0));
        assert_eq!(*yaw, 1.25);
        assert_eq!(species.as_deref(), Some("Cruiser"));
        assert_eq!(*color, Some([76, 232, 170]), "tinted as friendly");
        assert!(player_name.is_none(), "it belongs to nobody");
        assert!(ship_name.is_none(), "and is unnamed until one is chosen");

        let enemy = annotation_commands(&ship(false));
        let [DrawCommand::Ship { color, .. }] = &enemy[..] else { panic!("still a ship") };
        assert_eq!(*color, Some([254, 77, 42]), "and tinted as the enemy otherwise");
    }

    /// A ship that has been given a name carries it, which is what the
    /// renderer draws above the icon.
    #[test]
    fn a_named_ship_carries_its_name() {
        let named = Annotation::Ship {
            pos: [10.0, 20.0],
            yaw: 0.0,
            species: "Cruiser".to_string(),
            friendly: true,
            config: Some(crate::types::AnnotationShipConfig { ship_name: "Moskva".to_string(), ..Default::default() }),
        };
        let drawn = annotation_commands(&named);
        let [DrawCommand::Ship { ship_name, .. }] = &drawn[..] else { panic!("a ship") };
        assert_eq!(ship_name.as_deref(), Some("Moskva"));
    }

    /// Minimap distance and kilometres convert back to each other.
    #[test]
    fn a_distance_converts_to_kilometres_and_back() {
        let space_size = 1400.0;
        let there = minimap_distance_to_km(300.0, space_size);
        let back = km_to_minimap_distance(there, space_size);
        assert!((back - 300.0).abs() < NEAR, "{there} km, back to {back}");
    }
}
