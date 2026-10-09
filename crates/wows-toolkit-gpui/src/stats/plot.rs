//! The stats charts' plot surface.
//!
//! gpui-component's own `LineChart`/`BarChart` draw one series each, at fixed
//! bounds, with no axis titles and nothing to pan with. The egui Stats tab
//! draws a line per ship in that ship's own colour, labels both axes, lists
//! the series in a legend and lets the view be dragged and zoomed
//! (`ui/session_stats_chart.rs`), so the surface is painted here instead.
//!
//! What is drawn is decided by the shared series builders
//! (`wows_toolkit_viewmodel::stats::chart`), which both front ends call, so
//! only the painting differs between them.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use wows_toolkit_viewmodel::stats::chart::ChartBar;
use wows_toolkit_viewmodel::stats::chart::ChartSeries;
use wows_toolkit_viewmodel::stats::chart::Rgb;

/// Room left for the value ticks, the game numbers, and the axis titles.
const LEFT_GUTTER: f32 = 64.0;
const BOTTOM_GUTTER: f32 = 40.0;
const TOP_GUTTER: f32 = 24.0;
const RIGHT_GUTTER: f32 = 16.0;

/// Ticks per axis. Enough to read a value off, few enough not to crowd.
const TICKS: usize = 5;

const TICK_FONT: f32 = 11.0;
const LABEL_FONT: f32 = 12.0;
const POINT_RADIUS: f32 = 3.0;
const LEGEND_SWATCH: f32 = 10.0;
const LEGEND_ROW: f32 = 16.0;

/// How far the view has been dragged and zoomed away from the whole plot.
///
/// The zoom is about the cursor, so a point under it stays under it, and both
/// axes take it together: the shape of a line is what is being read, and
/// stretching one axis alone would distort it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotView {
    pub zoom: f32,
    pub pan: Point<Pixels>,
}

impl Default for PlotView {
    fn default() -> Self {
        Self { zoom: 1.0, pan: point(px(0.), px(0.)) }
    }
}

/// Maximum zoom-in keeps the plot usable at extreme magnification.
const MAX_ZOOM: f32 = 40.0;

impl PlotView {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Zooms by `factor` about `cursor`, which is where the pointer is inside
    /// the plot.
    pub fn zoom_about(&mut self, factor: f32, cursor: Point<Pixels>) {
        let zoom = (self.zoom * factor).clamp(f32::MIN_POSITIVE, MAX_ZOOM);
        let applied = zoom / self.zoom;
        if applied == 1.0 {
            return;
        }
        // The cursor is fixed: everything else moves away from it by as much
        // as the zoom grew.
        self.pan.x = cursor.x - (cursor.x - self.pan.x) * applied;
        self.pan.y = cursor.y - (cursor.y - self.pan.y) * applied;
        self.zoom = zoom;
    }

    pub fn drag(&mut self, dx: Pixels, dy: Pixels) {
        self.pan.x += dx;
        self.pan.y += dy;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Everything one plot draws.
pub struct Plot<'a> {
    pub series: &'a [ChartSeries],
    pub bars: &'a [ChartBar],
    pub x_label: &'a str,
    pub y_label: &'a str,
    pub show_values: bool,
    pub view: PlotView,
}

/// Paints `plot` into `bounds`.
///
/// Called from a `canvas` in the chart panel's render, which is the only
/// place a `Window` to paint with is at hand.
pub fn paint(plot: &Plot<'_>, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let theme = cx.theme();
    let colors = Colors { axis: theme.border, grid: theme.border.opacity(0.4), text: theme.muted_foreground };
    let mut canvas = WindowCanvas { window, cx };
    draw(plot, bounds, colors, &mut canvas);
}

/// Whether the bounds leave room for a plot after its axis gutters.
pub fn has_drawable_area(bounds: Bounds<Pixels>) -> bool {
    let area = plot_area(bounds);
    area.size.width > px(0.) && area.size.height > px(0.)
}

/// Draws `plot` into `bounds` on any canvas.
pub fn draw(plot: &Plot<'_>, bounds: Bounds<Pixels>, colors: Colors, canvas: &mut dyn PlotCanvas) {
    let area = plot_area(bounds);
    if area.size.width <= px(0.) || area.size.height <= px(0.) {
        return;
    }

    let (min_y, max_y) = value_range(plot);
    let ticks = tick_values(min_y, max_y);
    let frame = Frame {
        area,
        view: plot.view,
        min: ticks.first().copied().unwrap_or(min_y),
        max: ticks.last().copied().unwrap_or(max_y),
    };

    paint_grid(&ticks, &frame, colors, canvas);
    if plot.bars.is_empty() {
        paint_game_ticks(plot, &frame, colors, canvas);
    }
    paint_axes(area, colors.axis, canvas);

    canvas.clipped(area, &mut |canvas| {
        if plot.bars.is_empty() {
            paint_lines(plot, &frame, canvas);
        } else {
            paint_bars(plot, &frame, colors, canvas);
        }
    });

    paint_axis_titles(plot, bounds, area, colors.text, canvas);
    paint_legend(plot, area, colors.text, canvas);
}

/// What a plot draws through.
///
/// The layout is the same wherever a plot goes; only the primitives differ.
/// A window draws them with GPUI, an image with `tiny-skia`, and neither
/// knows where the ticks or the legend sit.
pub trait PlotCanvas {
    fn fill(&mut self, bounds: Bounds<Pixels>, color: Hsla, radius: Pixels);

    /// A stroked open path through `points`.
    fn polyline(&mut self, points: &[Point<Pixels>], width: f32, color: Hsla);

    /// How wide `text` will be, which is what right-aligning a tick label or
    /// centring a bar's name needs before it is drawn.
    fn measure(&mut self, text: &str, size: f32) -> Pixels;

    /// Draws `text` with its top-left at `origin`.
    fn text(&mut self, origin: Point<Pixels>, text: &str, size: f32, color: Hsla);

    /// Runs `body` with everything it draws confined to `area`.
    ///
    /// A closure rather than a push/pop pair because GPUI's own content mask
    /// is closure-scoped, and a panned line has to be cut off at the plot's
    /// edge rather than drawn across its gutters.
    fn clipped(&mut self, area: Bounds<Pixels>, body: &mut dyn FnMut(&mut dyn PlotCanvas));
}

/// Where the data is drawn, how the view is placed on it, and the range the
/// value axis covers.
struct Frame {
    area: Bounds<Pixels>,
    view: PlotView,
    min: f64,
    max: f64,
}

impl Frame {
    /// Where `value` sits, at `index` of `count` points, once the view is
    /// applied.
    fn place(&self, index: usize, count: usize, value: f64) -> Point<Pixels> {
        viewed(index_to_x(index, count, self.area), value_to_y(value, self.min, self.max, self.area), self.view)
    }
}

/// The theme colours a plot paints with.
#[derive(Clone, Copy)]
pub struct Colors {
    pub axis: Hsla,
    pub grid: Hsla,
    pub text: Hsla,
}

/// The rectangle the data itself is drawn in, inside the gutters the axes
/// and their titles take.
fn plot_area(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let width = (bounds.size.width.as_f32() - LEFT_GUTTER - RIGHT_GUTTER).max(0.0);
    let height = (bounds.size.height.as_f32() - TOP_GUTTER - BOTTOM_GUTTER).max(0.0);
    Bounds::new(point(bounds.origin.x + px(LEFT_GUTTER), bounds.origin.y + px(TOP_GUTTER)), size(px(width), px(height)))
}

/// The range the value axis covers, always including zero so a line is read
/// against it rather than against its own smallest point.
fn value_range(plot: &Plot<'_>) -> (f64, f64) {
    let values = plot
        .series
        .iter()
        .flat_map(|series| series.points.iter().map(|point| point.value))
        .chain(plot.bars.iter().map(|bar| bar.value));
    let mut min = 0.0f64;
    let mut max = 0.0f64;
    for value in values {
        min = min.min(value);
        max = max.max(value);
    }
    if max <= min { (min, min + 1.0) } else { (min, max) }
}

/// Tick values covering `min..=max`, rounded to a step that reads as a round
/// number rather than to the data's own extremes.
fn tick_values(min: f64, max: f64) -> Vec<f64> {
    let rough = (max - min) / TICKS as f64;
    if !rough.is_finite() || rough <= 0.0 {
        return vec![min, max];
    }
    let magnitude = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|multiple| multiple * magnitude)
        .find(|step| *step >= rough)
        .unwrap_or(magnitude * 10.0);

    let first = (min / step).floor() * step;
    let mut ticks = Vec::new();
    let mut value = first;
    while value <= max + step * 0.5 && ticks.len() < TICKS * 3 {
        ticks.push(value);
        value += step;
    }
    ticks
}

/// Where a value sits vertically, before the view is applied.
fn value_to_y(value: f64, min: f64, max: f64, area: Bounds<Pixels>) -> f32 {
    let span = (max - min).max(f64::EPSILON);
    let fraction = ((value - min) / span) as f32;
    area.origin.y.as_f32() + area.size.height.as_f32() * (1.0 - fraction)
}

/// Where the `index`th of `count` points sits horizontally, before the view
/// is applied. A lone point is centred rather than pinned to the left edge.
fn index_to_x(index: usize, count: usize, area: Bounds<Pixels>) -> f32 {
    if count <= 1 {
        return area.origin.x.as_f32() + area.size.width.as_f32() * 0.5;
    }
    let step = area.size.width.as_f32() / (count - 1) as f32;
    area.origin.x.as_f32() + step * index as f32
}

/// Applies the pan and zoom to a painted position.
fn viewed(x: f32, y: f32, view: PlotView) -> Point<Pixels> {
    point(px(x * view.zoom + view.pan.x.as_f32()), px(y * view.zoom + view.pan.y.as_f32()))
}

fn paint_grid(ticks: &[f64], frame: &Frame, colors: Colors, canvas: &mut dyn PlotCanvas) {
    let area = frame.area;
    for tick in ticks {
        let y = viewed(0.0, value_to_y(*tick, frame.min, frame.max, area), frame.view).y;
        if y < area.origin.y || y > area.origin.y + area.size.height {
            continue;
        }
        canvas.fill(Bounds::new(point(area.origin.x, y), size(area.size.width, px(1.))), colors.grid, px(0.));
        let label = format_value(*tick);
        let width = canvas.measure(&label, TICK_FONT);
        let origin = point(area.origin.x - width - px(6.), y - px(TICK_FONT * 0.7));
        canvas.text(origin, &label, TICK_FONT, colors.text);
    }
}

/// Numbers the games under the value axis.
///
/// The count is the longest line's: every line is drawn across the whole
/// width, each against its own games, which is how the egui chart plots a
/// ship that played fewer of them.
fn paint_game_ticks(plot: &Plot<'_>, frame: &Frame, colors: Colors, canvas: &mut dyn PlotCanvas) {
    let count = plot.series.iter().map(|series| series.points.len()).max().unwrap_or(0);
    if count == 0 {
        return;
    }
    let area = frame.area;
    let step = (count / TICKS).max(1);
    for index in (0..count).step_by(step) {
        let x = viewed(index_to_x(index, count, area), 0.0, frame.view).x;
        if x < area.origin.x || x > area.origin.x + area.size.width {
            continue;
        }
        canvas.fill(Bounds::new(point(x, area.origin.y), size(px(1.), area.size.height)), colors.grid, px(0.));
        let label = (index + 1).to_string();
        let width = canvas.measure(&label, TICK_FONT);
        let origin = point(x - width * 0.5, area.origin.y + area.size.height + px(6.));
        canvas.text(origin, &label, TICK_FONT, colors.text);
    }
}

fn paint_axes(area: Bounds<Pixels>, color: Hsla, canvas: &mut dyn PlotCanvas) {
    canvas.fill(
        Bounds::new(point(area.origin.x, area.origin.y + area.size.height), size(area.size.width, px(1.))),
        color,
        px(0.),
    );
    canvas.fill(Bounds::new(area.origin, size(px(1.), area.size.height)), color, px(0.));
}

fn paint_lines(plot: &Plot<'_>, frame: &Frame, canvas: &mut dyn PlotCanvas) {
    for series in plot.series {
        let count = series.points.len();
        let positions: Vec<Point<Pixels>> =
            series.points.iter().enumerate().map(|(index, point)| frame.place(index, count, point.value)).collect();
        let color = hsla_from(series.color);

        canvas.polyline(&positions, 1.5, color);

        for (index, position) in positions.iter().enumerate() {
            canvas.fill(dot_bounds(*position), color, px(POINT_RADIUS));
            if plot.show_values {
                let label = format_value(series.points[index].value);
                let width = canvas.measure(&label, TICK_FONT);
                let origin = point(position.x - width * 0.5, position.y - px(TICK_FONT + 6.0));
                canvas.text(origin, &label, TICK_FONT, color);
            }
        }
    }
}

fn paint_bars(plot: &Plot<'_>, frame: &Frame, colors: Colors, canvas: &mut dyn PlotCanvas) {
    let area = frame.area;
    let count = plot.bars.len();
    let slot = area.size.width.as_f32() * frame.view.zoom / count.max(1) as f32;
    let width = (slot * 0.7).max(1.0);
    let baseline = viewed(0.0, value_to_y(0.0f64.max(frame.min), frame.min, frame.max, area), frame.view).y;

    for (index, bar) in plot.bars.iter().enumerate() {
        let centre = viewed(
            area.origin.x.as_f32() + area.size.width.as_f32() / count.max(1) as f32 * (index as f32 + 0.5),
            value_to_y(bar.value, frame.min, frame.max, area),
            frame.view,
        );
        let top = centre.y.min(baseline);
        let height = (centre.y - baseline).abs().max(px(1.));
        canvas.fill(
            Bounds::new(point(centre.x - px(width * 0.5), top), size(px(width), height)),
            hsla_from(bar.color),
            px(0.),
        );

        // A label per bar is unreadable once the bars are thinner than the
        // names, so they are dropped rather than overlapped.
        if slot >= 48.0 {
            let label_width = canvas.measure(&bar.label, TICK_FONT);
            let origin = point(centre.x - label_width * 0.5, baseline + px(4.));
            canvas.text(origin, &bar.label, TICK_FONT, colors.text);
        }
        if plot.show_values {
            let label = format_value(bar.value);
            let label_width = canvas.measure(&label, TICK_FONT);
            let origin = point(centre.x - label_width * 0.5, top - px(TICK_FONT + 4.0));
            canvas.text(origin, &label, TICK_FONT, hsla_from(bar.color));
        }
    }
}

fn paint_axis_titles(
    plot: &Plot<'_>,
    bounds: Bounds<Pixels>,
    area: Bounds<Pixels>,
    color: Hsla,
    canvas: &mut dyn PlotCanvas,
) {
    // The value axis is named above it rather than rotated alongside it:
    // gpui paints text on one baseline only.
    let origin = point(bounds.origin.x + px(4.), bounds.origin.y + px(4.));
    canvas.text(origin, plot.y_label, LABEL_FONT, color);

    let width = canvas.measure(plot.x_label, LABEL_FONT);
    let origin = point(
        area.origin.x + (area.size.width - width) * 0.5,
        bounds.origin.y + bounds.size.height - px(LABEL_FONT + 6.0),
    );
    canvas.text(origin, plot.x_label, LABEL_FONT, color);
}

/// Names each line beside its own colour, in the top right of the plot. Bars
/// carry their name under them, so they are not listed again.
fn paint_legend(plot: &Plot<'_>, area: Bounds<Pixels>, color: Hsla, canvas: &mut dyn PlotCanvas) {
    for (row, series) in plot.series.iter().enumerate() {
        let top = area.origin.y + px(LEGEND_ROW * row as f32 + 2.0);
        if top + px(LEGEND_ROW) > area.origin.y + area.size.height {
            return;
        }
        let width = canvas.measure(&series.name, TICK_FONT);
        let right = area.origin.x + area.size.width;
        let swatch = point(right - width - px(LEGEND_SWATCH + 10.0), top + px(3.));
        canvas.fill(Bounds::new(swatch, size(px(LEGEND_SWATCH), px(LEGEND_SWATCH))), hsla_from(series.color), px(2.));
        canvas.text(point(swatch.x + px(LEGEND_SWATCH + 4.0), top), &series.name, TICK_FONT, color);
    }
}

fn dot_bounds(centre: Point<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        point(centre.x - px(POINT_RADIUS), centre.y - px(POINT_RADIUS)),
        size(px(POINT_RADIUS * 2.0), px(POINT_RADIUS * 2.0)),
    )
}

/// Draws a plot onto the window, which is what the chart panel shows.
struct WindowCanvas<'a, 'b> {
    window: &'a mut Window,
    cx: &'b mut App,
}

impl PlotCanvas for WindowCanvas<'_, '_> {
    fn fill(&mut self, bounds: Bounds<Pixels>, color: Hsla, radius: Pixels) {
        self.window.paint_quad(fill(bounds, color).corner_radii(radius));
    }

    fn polyline(&mut self, points: &[Point<Pixels>], width: f32, color: Hsla) {
        if points.len() < 2 {
            return;
        }
        let mut path = PathBuilder::stroke(px(width));
        path.move_to(points[0]);
        for position in &points[1..] {
            path.line_to(*position);
        }
        if let Ok(path) = path.build() {
            self.window.paint_path(path, color);
        }
    }

    fn measure(&mut self, text: &str, size: f32) -> Pixels {
        shape(text, size, gpui_kit::black(), self.window).width()
    }

    fn text(&mut self, origin: Point<Pixels>, text: &str, size: f32, color: Hsla) {
        let shaped = shape(text, size, color, self.window);
        let _ = shaped.paint(origin, px(size * 1.3), TextAlign::Left, None, self.window, self.cx);
    }

    fn clipped(&mut self, area: Bounds<Pixels>, body: &mut dyn FnMut(&mut dyn PlotCanvas)) {
        let cx = &mut *self.cx;
        self.window.with_content_mask(Some(ContentMask { bounds: area }), |window| {
            let mut inner = WindowCanvas { window, cx };
            body(&mut inner);
        });
    }
}

fn shape(text: &str, font_size: f32, color: Hsla, window: &mut Window) -> gpui_kit::ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: window.text_style().font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(SharedString::from(text.to_string()), px(font_size), &[run], None)
}

fn hsla_from(color: Rgb) -> Hsla {
    Rgba { r: color.r as f32 / 255.0, g: color.g as f32 / 255.0, b: color.b as f32 / 255.0, a: 1.0 }.into()
}

/// A plotted number, at the precision the egui charts label points with.
pub fn format_value(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 1000.0 {
        format!("{value:.0}")
    } else if magnitude >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::PlotView;
    use super::format_value;
    use super::tick_values;
    use gpui_kit::point;
    use gpui_kit::px;

    #[test]
    fn ticks_cover_the_range_with_round_steps() {
        let ticks = tick_values(0.0, 47_000.0);
        assert!(ticks.len() >= 2);
        assert_eq!(ticks[0], 0.0);
        assert!(ticks.last().copied().unwrap() >= 47_000.0, "the range is covered");
        let step = ticks[1] - ticks[0];
        assert!(ticks.windows(2).all(|pair| (pair[1] - pair[0] - step).abs() < step * 1e-6), "one even step");
    }

    #[test]
    fn a_flat_range_still_yields_ticks() {
        assert_eq!(tick_values(5.0, 5.0).len(), 2);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_cursor() {
        let mut view = PlotView::default();
        let cursor = point(px(100.), px(50.));
        view.zoom_about(2.0, cursor);
        let x = 100.0 * view.zoom + view.pan.x.as_f32();
        assert!((x - 100.0).abs() < 0.001, "the cursor's own position does not move");
    }

    #[test]
    fn zoom_in_has_a_stop_and_zoom_out_can_continue() {
        let mut view = PlotView::default();
        for _ in 0..40 {
            view.zoom_about(2.0, point(px(0.), px(0.)));
        }
        assert_eq!(view.zoom, super::MAX_ZOOM);

        let mut view = PlotView::default();
        view.zoom_about(0.01, point(px(0.), px(0.)));
        assert!(view.zoom < 1.0, "zooming out passes the initial view");
    }

    #[test]
    fn a_reset_view_is_the_default_one() {
        let mut view = PlotView::default();
        view.drag(px(10.), px(-4.));
        assert!(!view.is_default());
        view.reset();
        assert!(view.is_default());
    }

    #[test]
    fn values_are_labelled_at_the_precision_their_size_calls_for() {
        assert_eq!(format_value(47_231.4), "47231");
        assert_eq!(format_value(52.26), "52.3");
        assert_eq!(format_value(1.234), "1.23");
    }
}
