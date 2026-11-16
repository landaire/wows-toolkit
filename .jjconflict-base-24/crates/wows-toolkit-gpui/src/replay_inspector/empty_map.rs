//! The faint map on the replay pane with nothing open: the minimap's grid,
//! a few islands and the capture points of a battle between them.

use gpui_kit::App;
use gpui_kit::Bounds;
use gpui_kit::Div;
use gpui_kit::Hsla;
use gpui_kit::ParentElement as _;
use gpui_kit::PathBuilder;
use gpui_kit::Pixels;
use gpui_kit::Point;
use gpui_kit::SharedString;
use gpui_kit::Styled as _;
use gpui_kit::TextAlign;
use gpui_kit::TextRun;
use gpui_kit::Window;
use gpui_kit::canvas;
use gpui_kit::div;
use gpui_kit::point;
use gpui_kit::px;
use gpui_kit::relative;

/// A place on the map in fractions of its edge, from the top left.
#[derive(Clone, Copy)]
struct MapPoint {
    x: f32,
    y: f32,
}

impl MapPoint {
    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn on(self, bounds: Bounds<Pixels>) -> Point<Pixels> {
        point(bounds.origin.x + bounds.size.width * self.x, bounds.origin.y + bounds.size.height * self.y)
    }
}

struct CapturePoint {
    letter: &'static str,
    centre: MapPoint,
}

/// The map, drawn in `outline` with its grid and land in `faint`. It shrinks
/// with a narrow pane rather than spilling over the listing.
pub(super) fn sample_map(outline: Hsla, faint: Hsla) -> Div {
    const MAP_EDGE: Pixels = px(120.);
    // The minimap's own squares per side.
    const GRID_LINES: u16 = 10;

    let grid_lines = (1..GRID_LINES).flat_map(|line| {
        let at = relative(f32::from(line) / f32::from(GRID_LINES));
        [
            div().absolute().top_0().bottom_0().left(at).w(px(1.)).bg(faint),
            div().absolute().left_0().right_0().top(at).h(px(1.)).bg(faint),
        ]
    });
    let landmarks = canvas(
        |_bounds, _window, _cx| {},
        move |bounds, (), window, cx| paint_landmarks(bounds, outline, faint, window, cx),
    )
    .absolute()
    .inset_0();

    div()
        .relative()
        .w_full()
        .max_w(MAP_EDGE)
        .aspect_square()
        .border_1()
        .border_color(outline)
        .children(grid_lines)
        .child(landmarks)
}

/// Paints the islands and the capture points over `bounds`, a square.
///
/// The paths are rebuilt each paint; gpui keeps their tessellation while the
/// map stays the same size.
fn paint_landmarks(bounds: Bounds<Pixels>, outline: Hsla, land: Hsla, window: &mut Window, cx: &mut App) {
    // Coastline vertices, each smoothed into a curve through its neighbours'
    // midpoints.
    const ISLANDS: [&[MapPoint]; 10] = [
        &[
            MapPoint::new(0.29, 0.27),
            MapPoint::new(0.31, 0.21),
            MapPoint::new(0.35, 0.19),
            MapPoint::new(0.37, 0.14),
            MapPoint::new(0.42, 0.15),
            MapPoint::new(0.45, 0.12),
            MapPoint::new(0.50, 0.13),
            MapPoint::new(0.53, 0.10),
            MapPoint::new(0.57, 0.12),
            MapPoint::new(0.62, 0.13),
            MapPoint::new(0.64, 0.18),
            MapPoint::new(0.61, 0.21),
            MapPoint::new(0.63, 0.26),
            MapPoint::new(0.59, 0.29),
            MapPoint::new(0.56, 0.27),
            MapPoint::new(0.55, 0.23),
            MapPoint::new(0.51, 0.22),
            MapPoint::new(0.48, 0.25),
            MapPoint::new(0.49, 0.30),
            MapPoint::new(0.45, 0.33),
            MapPoint::new(0.42, 0.36),
            MapPoint::new(0.39, 0.32),
            MapPoint::new(0.35, 0.33),
            MapPoint::new(0.31, 0.31),
        ],
        &[
            MapPoint::new(0.28, 0.64),
            MapPoint::new(0.31, 0.60),
            MapPoint::new(0.35, 0.61),
            MapPoint::new(0.38, 0.58),
            MapPoint::new(0.42, 0.61),
            MapPoint::new(0.41, 0.66),
            MapPoint::new(0.44, 0.69),
            MapPoint::new(0.43, 0.73),
            MapPoint::new(0.39, 0.74),
            MapPoint::new(0.38, 0.78),
            MapPoint::new(0.41, 0.82),
            MapPoint::new(0.37, 0.86),
            MapPoint::new(0.33, 0.84),
            MapPoint::new(0.30, 0.80),
            MapPoint::new(0.31, 0.75),
            MapPoint::new(0.27, 0.71),
        ],
        &[
            MapPoint::new(0.71, 0.26),
            MapPoint::new(0.74, 0.21),
            MapPoint::new(0.79, 0.22),
            MapPoint::new(0.82, 0.18),
            MapPoint::new(0.87, 0.19),
            MapPoint::new(0.91, 0.23),
            MapPoint::new(0.92, 0.29),
            MapPoint::new(0.89, 0.34),
            MapPoint::new(0.85, 0.37),
            MapPoint::new(0.83, 0.33),
            MapPoint::new(0.86, 0.29),
            MapPoint::new(0.84, 0.25),
            MapPoint::new(0.80, 0.27),
            MapPoint::new(0.77, 0.31),
            MapPoint::new(0.73, 0.31),
        ],
        &[
            MapPoint::new(0.06, 0.20),
            MapPoint::new(0.09, 0.16),
            MapPoint::new(0.13, 0.17),
            MapPoint::new(0.16, 0.21),
            MapPoint::new(0.14, 0.25),
            MapPoint::new(0.15, 0.29),
            MapPoint::new(0.11, 0.30),
            MapPoint::new(0.08, 0.26),
        ],
        &[
            MapPoint::new(0.07, 0.75),
            MapPoint::new(0.11, 0.70),
            MapPoint::new(0.16, 0.72),
            MapPoint::new(0.19, 0.76),
            MapPoint::new(0.16, 0.79),
            MapPoint::new(0.18, 0.85),
            MapPoint::new(0.13, 0.88),
            MapPoint::new(0.09, 0.84),
            MapPoint::new(0.10, 0.79),
        ],
        &[
            MapPoint::new(0.86, 0.69),
            MapPoint::new(0.90, 0.66),
            MapPoint::new(0.94, 0.70),
            MapPoint::new(0.91, 0.75),
            MapPoint::new(0.87, 0.74),
        ],
        &[
            MapPoint::new(0.60, 0.67),
            MapPoint::new(0.63, 0.65),
            MapPoint::new(0.67, 0.66),
            MapPoint::new(0.68, 0.70),
            MapPoint::new(0.65, 0.73),
            MapPoint::new(0.61, 0.72),
        ],
        &[
            MapPoint::new(0.70, 0.78),
            MapPoint::new(0.73, 0.75),
            MapPoint::new(0.77, 0.76),
            MapPoint::new(0.79, 0.80),
            MapPoint::new(0.76, 0.82),
            MapPoint::new(0.72, 0.82),
        ],
        &[
            MapPoint::new(0.59, 0.81),
            MapPoint::new(0.62, 0.80),
            MapPoint::new(0.64, 0.83),
            MapPoint::new(0.62, 0.86),
            MapPoint::new(0.59, 0.85),
        ],
        &[MapPoint::new(0.67, 0.88), MapPoint::new(0.69, 0.87), MapPoint::new(0.70, 0.89), MapPoint::new(0.68, 0.90)],
    ];
    const CAPTURE_POINTS: [CapturePoint; 3] = [
        CapturePoint { letter: "A", centre: MapPoint::new(0.18, 0.50) },
        CapturePoint { letter: "B", centre: MapPoint::new(0.50, 0.48) },
        CapturePoint { letter: "C", centre: MapPoint::new(0.82, 0.53) },
    ];
    // In fractions of the map's edge.
    const RING_RADIUS: f32 = 0.075;
    const LETTER_SIZE: f32 = 0.1;

    let edge = bounds.size.width;
    let radius = edge * RING_RADIUS;

    let mut land_fill = PathBuilder::fill();
    let mut lines = PathBuilder::stroke(px(1.));
    for island in ISLANDS {
        trace_coast(&mut land_fill, island, bounds);
        trace_coast(&mut lines, island, bounds);
    }
    for capture in &CAPTURE_POINTS {
        let centre = capture.centre.on(bounds);
        let radii = point(radius, radius);
        lines.move_to(point(centre.x + radius, centre.y));
        lines.arc_to(radii, px(0.), false, true, point(centre.x - radius, centre.y));
        lines.arc_to(radii, px(0.), false, true, point(centre.x + radius, centre.y));
        lines.close();
    }
    if let Ok(path) = land_fill.build() {
        window.paint_path(path, land);
    }
    if let Ok(path) = lines.build() {
        window.paint_path(path, outline);
    }

    let font_size = edge * LETTER_SIZE;
    let font = window.text_style().font();
    for capture in &CAPTURE_POINTS {
        let centre = capture.centre.on(bounds);
        let run = TextRun {
            len: capture.letter.len(),
            font: font.clone(),
            color: outline,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let letter = window.text_system().shape_line(SharedString::new_static(capture.letter), font_size, &[run], None);
        let origin = point(centre.x - letter.width() * 0.5, centre.y - font_size * 0.5);
        // A letter that cannot be drawn leaves its ring empty; nothing else
        // depends on it.
        let _ = letter.paint(origin, font_size, TextAlign::Left, None, window, cx);
    }
}

/// Adds `island` to `path` as one closed curve. Each vertex pulls the curve
/// toward it as the control point between the midpoints of its two edges, so
/// a vertex set inward reads as a bay and one set outward as a point.
fn trace_coast(path: &mut PathBuilder, island: &[MapPoint], bounds: Bounds<Pixels>) {
    let (Some(&first), Some(&last)) = (island.first(), island.last()) else { return };

    let midpoint = |from: MapPoint, to: MapPoint| MapPoint::new((from.x + to.x) * 0.5, (from.y + to.y) * 0.5);
    path.move_to(midpoint(last, first).on(bounds));
    let following = island.iter().skip(1).chain([&first]);
    for (&vertex, &next) in island.iter().zip(following) {
        path.curve_to(midpoint(vertex, next).on(bounds), vertex.on(bounds));
    }
    path.close();
}
