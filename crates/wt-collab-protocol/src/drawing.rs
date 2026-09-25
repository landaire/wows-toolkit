//! Making an annotation with a tool: what a pointer does turns into a shape.
//!
//! The rules are the same wherever the drawing happens -- a line needs two
//! points far enough apart to mean anything, a freehand stroke is tidied
//! before it is kept, an arrow drawn with shift held is straight -- so they
//! are here rather than in a front end. A front end decides what a press and
//! a drag are; this decides what they draw.

use crate::geometry::smooth_freehand;
use crate::types::Annotation;

/// How far a drag has to travel before it has drawn anything, in minimap
/// space. Below this it is a click that wobbled.
const MINIMUM_DRAG: f32 = 1.0;

/// How near a click has to be to rub something out.
const ERASE_REACH: f32 = 15.0;

/// A run of this many points or fewer is a straight line already, and
/// smoothing it would only pull its ends about.
const SMOOTHABLE: usize = 2;

/// Which tool is in hand.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Tool {
    /// Nothing is being drawn. Pointer events pass through to the map.
    #[default]
    None,
    Ship {
        species: String,
        friendly: bool,
        yaw: f32,
    },
    Freehand,
    Eraser,
    Line,
    Circle {
        filled: bool,
    },
    Rectangle {
        filled: bool,
    },
    Triangle {
        filled: bool,
    },
    Arrow,
    Measurement,
}

/// What the pointer did, in minimap space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stroke {
    /// A press that began a drag.
    Began { at: [f32; 2] },
    /// The pointer moved while held. `straight` is whether the reader is
    /// asking for a straight line, which an arrow honours by keeping only
    /// where it started and where the pointer is now.
    Moved { at: [f32; 2], straight: bool },
    /// The drag ended here.
    Ended { at: [f32; 2] },
    /// A press and release in one place, which is what places a ship and
    /// what rubs one out.
    Clicked { at: [f32; 2] },
}

/// What a stroke came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    /// Keep this, and tell the session about it.
    Added(Annotation),
    /// Rub out the annotation at this index.
    Erased(usize),
}

/// The tool in hand and whatever it has drawn so far.
#[derive(Clone, Debug, Default)]
pub struct Drawing {
    tool: Tool,
    color: [u8; 4],
    width: f32,
    /// Where a two-point tool was anchored.
    from: Option<[f32; 2]>,
    /// The run a freehand stroke or an arrow has built up.
    points: Vec<[f32; 2]>,
    /// Whether the reader last asked for a straight line, which decides what
    /// the end of an arrow's drag keeps.
    straight: bool,
}

impl Drawing {
    pub fn new(color: [u8; 4], width: f32) -> Self {
        Self { tool: Tool::None, color, width, from: None, points: Vec::new(), straight: false }
    }

    pub fn tool(&self) -> &Tool {
        &self.tool
    }

    pub fn color(&self) -> [u8; 4] {
        self.color
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    /// Takes up a different tool, dropping anything half-drawn.
    ///
    /// Half a rectangle is not the beginning of a circle, so the shape in
    /// progress goes rather than turning into one of the new tool's.
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.cancel();
    }

    pub fn set_color(&mut self, color: [u8; 4]) {
        self.color = color;
    }

    pub fn set_width(&mut self, width: f32) {
        self.width = width;
    }

    /// Abandons whatever is half-drawn, keeping the tool.
    pub fn cancel(&mut self) {
        self.from = None;
        self.points.clear();
        self.straight = false;
    }

    /// Whether a shape is part-drawn, which is what a caller draws a preview
    /// of.
    pub fn is_drawing(&self) -> bool {
        self.from.is_some() || !self.points.is_empty()
    }

    /// The shape as it stands with the pointer at `at`, for a preview drawn
    /// the same way the finished one will be.
    ///
    /// `None` when nothing is part-drawn, or when what is there is still too
    /// small to mean anything -- the same judgement that decides whether the
    /// stroke keeps it.
    pub fn in_progress(&self, at: [f32; 2]) -> Option<Annotation> {
        match &self.tool {
            Tool::Freehand => self.built(&self.points, false),
            Tool::Arrow => self.built(&self.arrow_run(at), true),
            _ => self.finished(at),
        }
    }

    /// Takes a pointer event and says what it drew, if anything.
    ///
    /// `existing` is what is already on the map, which only the eraser reads.
    pub fn handle(&mut self, stroke: Stroke, existing: &[Annotation]) -> Option<Drawn> {
        match self.tool {
            Tool::None => None,
            Tool::Ship { .. } => match stroke {
                Stroke::Clicked { at } => self.finished(at).map(Drawn::Added),
                _ => None,
            },
            Tool::Eraser => match stroke {
                Stroke::Clicked { at } => nearest_within(existing, at, ERASE_REACH).map(Drawn::Erased),
                _ => None,
            },
            Tool::Freehand | Tool::Arrow => self.run(stroke),
            _ => self.anchored(stroke),
        }
    }

    /// A tool that builds a run of points as the pointer is dragged.
    fn run(&mut self, stroke: Stroke) -> Option<Drawn> {
        match stroke {
            Stroke::Began { at } => {
                self.cancel();
                self.points.push(at);
                None
            }
            Stroke::Moved { at, straight } => {
                self.straight = straight;
                self.points.push(at);
                None
            }
            Stroke::Ended { at } => {
                let arrow = matches!(self.tool, Tool::Arrow);
                let points = if arrow {
                    self.arrow_run(at)
                } else {
                    let mut points = std::mem::take(&mut self.points);
                    points.push(at);
                    points
                };
                self.cancel();
                self.built(&points, arrow).map(Drawn::Added)
            }
            Stroke::Clicked { .. } => {
                self.cancel();
                None
            }
        }
    }

    /// An arrow's points with the pointer at `at`.
    ///
    /// Asked to keep it straight, the shaft is where it began and where the
    /// pointer is and nothing between, so the wandering that got there is
    /// forgotten rather than smoothed into a curve.
    fn arrow_run(&self, at: [f32; 2]) -> Vec<[f32; 2]> {
        let Some(began) = self.points.first().copied() else { return Vec::new() };
        if self.straight {
            return vec![began, at];
        }
        let mut points = self.points.clone();
        points.push(at);
        points
    }

    /// A tool anchored by where the drag began and finished by where it
    /// ended.
    fn anchored(&mut self, stroke: Stroke) -> Option<Drawn> {
        match stroke {
            Stroke::Began { at } => {
                self.from = Some(at);
                None
            }
            Stroke::Moved { .. } => None,
            Stroke::Ended { at } => {
                let drawn = self.finished(at);
                self.from = None;
                drawn.map(Drawn::Added)
            }
            Stroke::Clicked { .. } => {
                self.cancel();
                None
            }
        }
    }

    /// What an anchored tool has drawn with the pointer at `at`, or `None`
    /// when it is too small to mean anything.
    fn finished(&self, at: [f32; 2]) -> Option<Annotation> {
        let (color, width) = (self.color, self.width);
        match &self.tool {
            Tool::Ship { species, friendly, yaw } => Some(Annotation::Ship {
                pos: at,
                yaw: *yaw,
                species: species.clone(),
                friendly: *friendly,
                config: None,
            }),
            Tool::Line => {
                let from = self.from?;
                (distance(from, at) > MINIMUM_DRAG).then_some(Annotation::Line { start: from, end: at, color, width })
            }
            Tool::Measurement => {
                let from = self.from?;
                (distance(from, at) > MINIMUM_DRAG).then_some(Annotation::Measurement {
                    start: from,
                    end: at,
                    color,
                    width,
                })
            }
            Tool::Circle { filled } => {
                let center = self.from?;
                let radius = distance(center, at);
                (radius > MINIMUM_DRAG).then_some(Annotation::Circle { center, radius, color, width, filled: *filled })
            }
            Tool::Triangle { filled } => {
                let center = self.from?;
                let radius = distance(center, at);
                (radius > MINIMUM_DRAG).then_some(Annotation::Triangle {
                    center,
                    radius,
                    rotation: 0.0,
                    color,
                    width,
                    filled: *filled,
                })
            }
            Tool::Rectangle { filled } => {
                let corner = self.from?;
                let center = [(corner[0] + at[0]) / 2.0, (corner[1] + at[1]) / 2.0];
                let half_size = [(at[0] - corner[0]).abs() / 2.0, (at[1] - corner[1]).abs() / 2.0];
                (half_size[0] > MINIMUM_DRAG && half_size[1] > MINIMUM_DRAG).then_some(Annotation::Rectangle {
                    center,
                    half_size,
                    rotation: 0.0,
                    color,
                    width,
                    filled: *filled,
                })
            }
            Tool::None | Tool::Freehand | Tool::Eraser | Tool::Arrow => None,
        }
    }

    /// What a run of points has drawn, tidied unless it is already straight.
    fn built(&self, points: &[[f32; 2]], arrow: bool) -> Option<Annotation> {
        if points.len() < 2 {
            return None;
        }
        let points = if points.len() > SMOOTHABLE { smooth_freehand(points.to_vec()) } else { points.to_vec() };
        let (color, width) = (self.color, self.width);
        Some(if arrow {
            Annotation::Arrow { points, color, width }
        } else {
            Annotation::FreehandStroke { points, color, width }
        })
    }
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Which of `annotations` is nearest `at`, when one is within `reach`.
pub fn nearest_within(annotations: &[Annotation], at: [f32; 2], reach: f32) -> Option<usize> {
    annotations
        .iter()
        .enumerate()
        .map(|(index, annotation)| (index, crate::geometry::annotation_distance(annotation, at)))
        .filter(|(_, distance)| *distance < reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// How near a click has to be to pick something out.
const PICK_REACH: f32 = 15.0;

/// Moves `annotation` by `delta`, in minimap space.
pub fn move_annotation(annotation: &mut Annotation, delta: [f32; 2]) {
    let shift = |point: &mut [f32; 2]| {
        point[0] += delta[0];
        point[1] += delta[1];
    };
    match annotation {
        Annotation::Ship { pos, .. } => shift(pos),
        Annotation::FreehandStroke { points, .. } | Annotation::Arrow { points, .. } => {
            points.iter_mut().for_each(shift)
        }
        Annotation::Line { start, end, .. } | Annotation::Measurement { start, end, .. } => {
            shift(start);
            shift(end);
        }
        Annotation::Circle { center, .. }
        | Annotation::Rectangle { center, .. }
        | Annotation::Triangle { center, .. } => shift(center),
    }
}

/// Whether `annotation` has a bearing that can be turned.
///
/// A circle looks the same at every angle and a stroke carries its own
/// bearing in its points, so neither takes a rotation.
pub fn can_rotate(annotation: &Annotation) -> bool {
    matches!(annotation, Annotation::Ship { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. })
}

/// Turns `annotation` to face `angle`, if it is one that faces anywhere.
pub fn rotate_annotation(annotation: &mut Annotation, angle: f32) {
    match annotation {
        Annotation::Ship { yaw, .. } => *yaw = angle,
        Annotation::Rectangle { rotation, .. } | Annotation::Triangle { rotation, .. } => *rotation = angle,
        _ => {}
    }
}

/// How big a ship annotation's icon is drawn, in minimap space.
///
/// The renderer's `assets::ICON_SIZE` is the same figure, but it is behind
/// that crate's rendering feature and this one does not rasterise anything.
const SHIP_ICON: f32 = (wows_minimap_renderer::MINIMAP_SIZE * 3 / 128) as f32;

/// The rectangle `annotation` occupies in minimap space, as left, top, right
/// and bottom.
///
/// Around the drawn shape rather than around its anchor: a rectangle turned
/// on its corner reaches further than its own half size, and a handle placed
/// over its middle would sit inside it.
pub fn annotation_bounds(annotation: &Annotation) -> [f32; 4] {
    fn around(points: impl IntoIterator<Item = [f32; 2]>) -> [f32; 4] {
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for point in points {
            bounds[0] = bounds[0].min(point[0]);
            bounds[1] = bounds[1].min(point[1]);
            bounds[2] = bounds[2].max(point[0]);
            bounds[3] = bounds[3].max(point[1]);
        }
        bounds
    }
    fn square(center: [f32; 2], half: f32) -> [f32; 4] {
        [center[0] - half, center[1] - half, center[0] + half, center[1] + half]
    }

    match annotation {
        Annotation::Ship { pos, .. } => square(*pos, SHIP_ICON / 2.0),
        Annotation::FreehandStroke { points, .. } | Annotation::Arrow { points, .. } => around(points.iter().copied()),
        Annotation::Line { start, end, .. } | Annotation::Measurement { start, end, .. } => around([*start, *end]),
        Annotation::Circle { center, radius, .. } => square(*center, *radius),
        Annotation::Rectangle { center, half_size, rotation, .. } => {
            let (sin, cos) = rotation.sin_cos();
            around([[-1.0_f32, -1.0_f32], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].into_iter().map(|[sx, sy]| {
                let (x, y) = (sx * half_size[0], sy * half_size[1]);
                [center[0] + x * cos - y * sin, center[1] + x * sin + y * cos]
            }))
        }
        Annotation::Triangle { center, radius, rotation, .. } => {
            around(crate::geometry::triangle_corners(*center, *radius, *rotation))
        }
    }
}

/// Which way `at` lies from `center`, as the angle a turned shape takes.
///
/// Zero points up the map and the angle grows anticlockwise, so west is a
/// quarter turn and east is minus one. That is what the egui renderer
/// measures and what a stored rotation already means, so the convention is
/// its own rather than a nicer one.
pub fn bearing(center: [f32; 2], at: [f32; 2]) -> f32 {
    -(at[0] - center[0]).atan2(-(at[1] - center[1]))
}

/// What the reader has picked out to move or turn.
///
/// Indices into the session's own list, so a caller reads the annotations
/// themselves from wherever it holds them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    picked: Vec<usize>,
}

impl Selection {
    /// Picks out whatever is under `at`, if anything is near enough.
    ///
    /// `add` is whether the reader is building a selection up rather than
    /// replacing it, which is ctrl-click in both front ends. A click on
    /// nothing clears the selection, unless they were adding to it.
    pub fn click(&mut self, annotations: &[Annotation], at: [f32; 2], add: bool) {
        let Some(found) = nearest_within(annotations, at, PICK_REACH) else {
            if !add {
                self.clear();
            }
            return;
        };
        if !add {
            self.picked = vec![found];
            return;
        }
        match self.picked.iter().position(|picked| *picked == found) {
            Some(at) => {
                self.picked.remove(at);
            }
            None => self.picked.push(found),
        }
    }

    /// What is picked out, in the order it was picked.
    pub fn picked(&self) -> &[usize] {
        &self.picked
    }

    pub fn is_empty(&self) -> bool {
        self.picked.is_empty()
    }

    /// The one thing picked out, when exactly one is.
    ///
    /// A rotation handle belongs to a single shape: turning several at once
    /// about their own middles is not what a handle on one of them means.
    pub fn single(&self) -> Option<usize> {
        match self.picked.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    pub fn clear(&mut self) {
        self.picked.clear();
    }

    /// Forgets anything picked out that is past the end of `annotations`.
    ///
    /// The session's list is shared, so another peer rubbing something out
    /// can leave a selection pointing past the end of it.
    pub fn retain_within(&mut self, annotations: &[Annotation]) {
        self.picked.retain(|picked| *picked < annotations.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn drawing(tool: Tool) -> Drawing {
        let mut drawing = Drawing::new(WHITE, 2.0);
        drawing.set_tool(tool);
        drawing
    }

    fn drag(drawing: &mut Drawing, from: [f32; 2], to: [f32; 2]) -> Option<Drawn> {
        drawing.handle(Stroke::Began { at: from }, &[]);
        drawing.handle(Stroke::Moved { at: to, straight: false }, &[]);
        drawing.handle(Stroke::Ended { at: to }, &[])
    }

    fn line(start: [f32; 2], end: [f32; 2]) -> Annotation {
        Annotation::Line { start, end, color: WHITE, width: 2.0 }
    }

    /// Moving a shape moves every part of it, not just where it is anchored.
    #[test]
    fn moving_a_shape_moves_the_whole_of_it() {
        let mut stroke =
            Annotation::FreehandStroke { points: vec![[0.0, 0.0], [10.0, 10.0]], color: WHITE, width: 2.0 };
        move_annotation(&mut stroke, [5.0, -5.0]);
        let Annotation::FreehandStroke { points, .. } = &stroke else { panic!("still a stroke") };
        assert_eq!(points, &vec![[5.0, -5.0], [15.0, 5.0]]);

        let mut measurement = Annotation::Measurement { start: [0.0, 0.0], end: [10.0, 0.0], color: WHITE, width: 2.0 };
        move_annotation(&mut measurement, [1.0, 2.0]);
        let Annotation::Measurement { start, end, .. } = &measurement else { panic!("still a measurement") };
        assert_eq!((*start, *end), ([1.0, 2.0], [11.0, 2.0]), "both ends, not just the first");
    }

    /// Only a shape with a bearing can be turned; the rest are left alone
    /// rather than quietly gaining a rotation nothing draws.
    #[test]
    fn only_a_shape_with_a_bearing_turns() {
        let mut rect = Annotation::Rectangle {
            center: [0.0, 0.0],
            half_size: [5.0, 5.0],
            rotation: 0.0,
            color: WHITE,
            width: 2.0,
            filled: false,
        };
        assert!(can_rotate(&rect));
        rotate_annotation(&mut rect, 1.25);
        let Annotation::Rectangle { rotation, .. } = &rect else { panic!("still a rectangle") };
        assert_eq!(*rotation, 1.25);

        let mut circle =
            Annotation::Circle { center: [0.0, 0.0], radius: 5.0, color: WHITE, width: 2.0, filled: false };
        assert!(!can_rotate(&circle));
        rotate_annotation(&mut circle, 1.25);
        assert!(matches!(circle, Annotation::Circle { .. }), "and it is untouched");
    }

    /// A shape's bounds go round what is drawn, so a turned rectangle
    /// reaches past its own half size.
    #[test]
    fn bounds_go_round_the_drawn_shape() {
        let rect = |rotation: f32| Annotation::Rectangle {
            center: [100.0, 100.0],
            half_size: [20.0, 10.0],
            rotation,
            color: WHITE,
            width: 2.0,
            filled: false,
        };

        let [left, top, right, bottom] = annotation_bounds(&rect(0.0));
        assert!((left - 80.0).abs() < 1e-3 && (right - 120.0).abs() < 1e-3);
        assert!((top - 90.0).abs() < 1e-3 && (bottom - 110.0).abs() < 1e-3);

        let [left, _, right, _] = annotation_bounds(&rect(std::f32::consts::FRAC_PI_4));
        assert!(right - left > 40.0, "turned on its corner it reaches further: {}", right - left);
    }

    /// A stroke's bounds go round every point of it.
    #[test]
    fn a_strokes_bounds_hold_all_of_it() {
        let stroke = Annotation::FreehandStroke {
            points: vec![[10.0, 50.0], [30.0, 10.0], [20.0, 90.0]],
            color: WHITE,
            width: 2.0,
        };
        assert_eq!(annotation_bounds(&stroke), [10.0, 10.0, 30.0, 90.0]);
    }

    /// A bearing is measured from up the map, growing anticlockwise, which
    /// is the convention a stored rotation already carries.
    #[test]
    fn a_bearing_starts_up_the_map_and_grows_clockwise() {
        let center = [100.0, 100.0];
        let quarter = std::f32::consts::FRAC_PI_2;
        assert!(bearing(center, [100.0, 50.0]).abs() < 1e-4, "straight up is zero");
        assert!((bearing(center, [50.0, 100.0]) - quarter).abs() < 1e-4, "west is a quarter turn");
        assert!((bearing(center, [150.0, 100.0]) + quarter).abs() < 1e-4, "and east is minus one");
        assert!(
            (bearing(center, [100.0, 150.0]).abs() - std::f32::consts::PI).abs() < 1e-4,
            "straight down is half a turn, either way round"
        );
    }

    /// A click picks out what it landed on, and a click on open water lets
    /// go of what was picked.
    #[test]
    fn a_click_picks_out_what_it_landed_on() {
        let annotations = vec![line([0.0, 0.0], [100.0, 0.0]), line([0.0, 100.0], [100.0, 100.0])];
        let mut picked = Selection::default();

        picked.click(&annotations, [50.0, 2.0], false);
        assert_eq!(picked.picked(), [0]);

        picked.click(&annotations, [50.0, 98.0], false);
        assert_eq!(picked.picked(), [1], "a plain click replaces rather than adds");

        picked.click(&annotations, [50.0, 50.0], false);
        assert!(picked.is_empty(), "and open water lets go");
    }

    /// Adding to a selection takes one in and out again, and a click on open
    /// water while adding leaves what is picked alone.
    #[test]
    fn adding_to_a_selection_takes_one_in_and_out() {
        let annotations = vec![line([0.0, 0.0], [100.0, 0.0]), line([0.0, 100.0], [100.0, 100.0])];
        let mut picked = Selection::default();

        picked.click(&annotations, [50.0, 2.0], true);
        picked.click(&annotations, [50.0, 98.0], true);
        assert_eq!(picked.picked(), [0, 1]);
        assert_eq!(picked.single(), None, "two is not one");

        picked.click(&annotations, [50.0, 2.0], true);
        assert_eq!(picked.picked(), [1], "the second click lets that one go");
        assert_eq!(picked.single(), Some(1));

        picked.click(&annotations, [50.0, 50.0], true);
        assert_eq!(picked.picked(), [1], "open water while adding changes nothing");
    }

    /// A peer rubbing something out cannot leave a selection pointing past
    /// the end of the list.
    #[test]
    fn a_selection_does_not_outlive_what_it_picked() {
        let annotations = vec![line([0.0, 0.0], [100.0, 0.0]), line([0.0, 100.0], [100.0, 100.0])];
        let mut picked = Selection::default();
        picked.click(&annotations, [50.0, 2.0], true);
        picked.click(&annotations, [50.0, 98.0], true);

        picked.retain_within(&annotations[..1]);
        assert_eq!(picked.picked(), [0]);
    }

    /// A drag that went nowhere draws nothing, so a click with the line tool
    /// does not leave a dot on the map.
    #[test]
    fn a_drag_that_went_nowhere_draws_nothing() {
        let mut line = drawing(Tool::Line);
        assert_eq!(drag(&mut line, [50.0, 50.0], [50.2, 50.2]), None);

        let mut circle = drawing(Tool::Circle { filled: false });
        assert_eq!(drag(&mut circle, [50.0, 50.0], [50.5, 50.0]), None);

        // And the tool is left ready rather than half-anchored.
        assert!(!circle.is_drawing());
    }

    /// A rectangle is drawn corner to corner but kept as a centre and a half
    /// size, whichever way the drag went.
    #[test]
    fn a_rectangle_is_kept_by_its_middle_however_it_was_dragged() {
        for (from, to) in [([10.0, 10.0], [50.0, 30.0]), ([50.0, 30.0], [10.0, 10.0])] {
            let mut rect = drawing(Tool::Rectangle { filled: true });
            let Some(Drawn::Added(Annotation::Rectangle { center, half_size, filled, .. })) = drag(&mut rect, from, to)
            else {
                panic!("a rectangle tool draws a rectangle");
            };
            assert_eq!(center, [30.0, 20.0]);
            assert_eq!(half_size, [20.0, 10.0]);
            assert!(filled);
        }
    }

    /// A rectangle needs width in both directions: a drag along one axis is
    /// a line, not a rectangle.
    #[test]
    fn a_rectangle_flat_in_one_direction_is_not_drawn() {
        let mut rect = drawing(Tool::Rectangle { filled: false });
        assert_eq!(drag(&mut rect, [10.0, 10.0], [50.0, 10.0]), None);
    }

    /// A freehand stroke is tidied before it is kept, and keeps its ends.
    #[test]
    fn a_freehand_stroke_is_tidied_but_starts_and_ends_where_the_hand_did() {
        let mut freehand = drawing(Tool::Freehand);
        freehand.handle(Stroke::Began { at: [0.0, 0.0] }, &[]);
        for step in 1..20 {
            let along = step as f32 * 3.0;
            let wobble = if step % 2 == 0 { 0.4 } else { -0.4 };
            freehand.handle(Stroke::Moved { at: [along, wobble], straight: false }, &[]);
        }
        let Some(Drawn::Added(Annotation::FreehandStroke { points, .. })) =
            freehand.handle(Stroke::Ended { at: [60.0, 0.0] }, &[])
        else {
            panic!("a freehand tool draws a stroke");
        };

        assert_eq!(points[0], [0.0, 0.0]);
        assert_eq!(*points.last().expect("a stroke has an end"), [60.0, 0.0]);
        assert!(points.len() < 21, "the wobble was taken out: {} points", points.len());
    }

    /// Holding straight while drawing an arrow keeps only where it began and
    /// where the pointer is, however the hand wandered between.
    #[test]
    fn a_straight_arrow_forgets_where_the_hand_wandered() {
        let mut arrow = drawing(Tool::Arrow);
        arrow.handle(Stroke::Began { at: [0.0, 0.0] }, &[]);
        arrow.handle(Stroke::Moved { at: [10.0, 30.0], straight: false }, &[]);
        arrow.handle(Stroke::Moved { at: [20.0, 40.0], straight: true }, &[]);
        let Some(Drawn::Added(Annotation::Arrow { points, .. })) = arrow.handle(Stroke::Ended { at: [60.0, 0.0] }, &[])
        else {
            panic!("an arrow tool draws an arrow");
        };

        assert_eq!(points, vec![[0.0, 0.0], [60.0, 0.0]], "where it began and where it ended, nothing between");
    }

    /// The eraser rubs out the nearest thing within reach, and nothing at all
    /// when the click was nowhere near one.
    #[test]
    fn the_eraser_takes_the_nearest_thing_within_reach() {
        let annotations = vec![
            Annotation::Line { start: [0.0, 0.0], end: [100.0, 0.0], color: WHITE, width: 2.0 },
            Annotation::Line { start: [0.0, 50.0], end: [100.0, 50.0], color: WHITE, width: 2.0 },
        ];
        let mut eraser = drawing(Tool::Eraser);

        assert_eq!(eraser.handle(Stroke::Clicked { at: [50.0, 3.0] }, &annotations), Some(Drawn::Erased(0)));
        assert_eq!(eraser.handle(Stroke::Clicked { at: [50.0, 47.0] }, &annotations), Some(Drawn::Erased(1)));
        assert_eq!(eraser.handle(Stroke::Clicked { at: [50.0, 25.0] }, &annotations), None, "between the two");
    }

    /// Taking up another tool drops what was half-drawn, rather than letting
    /// it finish as a shape nobody asked for.
    #[test]
    fn changing_tool_drops_what_was_half_drawn() {
        let mut drawing = drawing(Tool::Rectangle { filled: false });
        drawing.handle(Stroke::Began { at: [10.0, 10.0] }, &[]);
        assert!(drawing.is_drawing());

        drawing.set_tool(Tool::Circle { filled: false });
        assert!(!drawing.is_drawing());
        assert_eq!(drawing.handle(Stroke::Ended { at: [50.0, 50.0] }, &[]), None, "no circle from a rectangle");
    }

    /// With no tool in hand nothing is drawn, whatever the pointer does.
    #[test]
    fn nothing_is_drawn_without_a_tool() {
        let mut idle = Drawing::new(WHITE, 2.0);
        assert_eq!(drag(&mut idle, [0.0, 0.0], [50.0, 50.0]), None);
        assert_eq!(idle.handle(Stroke::Clicked { at: [0.0, 0.0] }, &[]), None);
    }

    /// A shape part-drawn previews as the shape it will be, so what a reader
    /// sees while dragging is what they get.
    #[test]
    fn a_part_drawn_shape_previews_as_what_it_will_become() {
        let mut circle = drawing(Tool::Circle { filled: true });
        assert!(circle.in_progress([50.0, 50.0]).is_none(), "before it is anchored");

        circle.handle(Stroke::Began { at: [50.0, 50.0] }, &[]);
        let Some(Annotation::Circle { center, radius, filled, .. }) = circle.in_progress([50.0, 80.0]) else {
            panic!("a circle previews as a circle");
        };
        assert_eq!(center, [50.0, 50.0]);
        assert!((radius - 30.0).abs() < 1e-4);
        assert!(filled);

        // And what it previews is what it keeps.
        let Some(Drawn::Added(Annotation::Circle { radius: kept, .. })) =
            circle.handle(Stroke::Ended { at: [50.0, 80.0] }, &[])
        else {
            panic!("and it draws one");
        };
        assert!((kept - radius).abs() < 1e-4);
    }

    /// A ship is placed by a click rather than a drag, since it has a size of
    /// its own.
    #[test]
    fn a_ship_is_placed_by_a_click() {
        let mut ship = drawing(Tool::Ship { species: "Cruiser".to_string(), friendly: true, yaw: 1.5 });

        assert_eq!(drag(&mut ship, [0.0, 0.0], [50.0, 50.0]), None, "a drag places nothing");

        let Some(Drawn::Added(Annotation::Ship { pos, yaw, species, friendly, .. })) =
            ship.handle(Stroke::Clicked { at: [30.0, 40.0] }, &[])
        else {
            panic!("a ship tool places a ship");
        };
        assert_eq!(pos, [30.0, 40.0]);
        assert_eq!(yaw, 1.5);
        assert_eq!(species, "Cruiser");
        assert!(friendly);
    }
}
