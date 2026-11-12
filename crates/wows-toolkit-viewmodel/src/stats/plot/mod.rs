//! The stats charts' plot: where the axes, ticks, lines, bars and legend go.
//!
//! The layout is decided here once and drawn through a [`PlotCanvas`], so a window and a copied
//! image agree on where every tick, bar and label goes; only the primitives differ. What is
//! plotted comes from the series builders in [`super::chart`].
//!
//! Coordinates are logical pixels in whatever space the canvas draws in. A canvas that paints at
//! a device scale multiplies them itself.

pub mod image;

use rust_i18n::t;

use super::chart::ChartBar;
use super::chart::ChartSeries;
use super::chart::Rgb;

/// Room left for the value ticks, the game numbers, and the axis titles.
const LEFT_GUTTER: f32 = 64.0;
const BOTTOM_GUTTER: f32 = 40.0;
const TOP_GUTTER: f32 = 24.0;
const RIGHT_GUTTER: f32 = 16.0;
const LEGEND_MIN_WIDTH: f32 = 96.0;
const LEGEND_MAX_WIDTH: f32 = 190.0;
const LEGEND_GAP: f32 = 12.0;

/// Ticks per axis. Enough to read a value off, few enough not to crowd.
const TICKS: usize = 5;
const MIN_GAME_TICK_SPACING: f32 = 56.0;
const MIN_VALUE_TICK_SPACING: f32 = 28.0;

const TICK_FONT: f32 = 11.0;
const LABEL_FONT: f32 = 12.0;
const POINT_RADIUS: f32 = 3.0;
const LEGEND_SWATCH: f32 = 10.0;
const LEGEND_ROW: f32 = 16.0;

/// Maximum zoom-in keeps the plot usable at extreme magnification.
const MAX_ZOOM: f32 = 40.0;

/// How much one wheel notch zooms.
pub const WHEEL_ZOOM_STEP: f32 = 1.1;

/// A position on the canvas.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlotPoint {
    pub x: f32,
    pub y: f32,
}

impl PlotPoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A rectangle on the canvas, from its top-left corner.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlotRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PlotRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// The part of `self` inside `other`, `None` when they do not overlap.
    pub fn intersect(&self, other: &PlotRect) -> Option<PlotRect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            return None;
        }

        Some(PlotRect::new(x, y, right - x, bottom - y))
    }
}

/// A non-premultiplied sRGB colour, each channel from zero to one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl PlotColor {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// The same colour with its alpha scaled by `factor`.
    pub fn opacity(self, factor: f32) -> Self {
        Self { a: self.a * factor, ..self }
    }
}

impl From<Rgb> for PlotColor {
    fn from(color: Rgb) -> Self {
        Self::new(f32::from(color.r) / 255.0, f32::from(color.g) / 255.0, f32::from(color.b) / 255.0, 1.0)
    }
}

/// The theme colours a plot paints with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotColors {
    pub axis: PlotColor,
    pub grid: PlotColor,
    pub text: PlotColor,
}

/// How far the view has been dragged and zoomed away from the whole plot.
///
/// The zoom is about the cursor, so a point under it stays under it, and both axes take it
/// together: the shape of a line is what is being read, and stretching one axis alone would
/// distort it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotView {
    pub zoom: f32,
    pub pan: PlotPoint,
}

impl Default for PlotView {
    fn default() -> Self {
        Self::new()
    }
}

impl PlotView {
    pub const fn new() -> Self {
        Self { zoom: 1.0, pan: PlotPoint::new(0.0, 0.0) }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::new()
    }

    /// Zooms by `factor` about `cursor`, which is where the pointer is in the canvas's space.
    pub fn zoom_about(&mut self, factor: f32, cursor: PlotPoint) {
        let zoom = (self.zoom * factor).clamp(f32::MIN_POSITIVE, MAX_ZOOM);
        let applied = zoom / self.zoom;
        if applied == 1.0 {
            return;
        }

        // The cursor is fixed: everything else moves away from it by as much as the zoom grew.
        self.pan.x = cursor.x - (cursor.x - self.pan.x) * applied;
        self.pan.y = cursor.y - (cursor.y - self.pan.y) * applied;
        self.zoom = zoom;
    }

    /// Zooms one wheel notch about `cursor`: in for a positive `delta`, out for a negative one.
    pub fn wheel(&mut self, delta: f32, cursor: PlotPoint) {
        if delta == 0.0 {
            return;
        }
        let factor = if delta > 0.0 { WHEEL_ZOOM_STEP } else { 1.0 / WHEEL_ZOOM_STEP };
        self.zoom_about(factor, cursor);
    }

    pub fn drag(&mut self, dx: f32, dy: f32) {
        self.pan.x += dx;
        self.pan.y += dy;
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Applies the pan and zoom to a painted position.
    pub fn apply(&self, x: f32, y: f32) -> PlotPoint {
        PlotPoint::new(x * self.zoom + self.pan.x, y * self.zoom + self.pan.y)
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

/// What a plot draws through.
///
/// The layout is the same wherever a plot goes; only the primitives differ.
pub trait PlotCanvas {
    fn fill(&mut self, bounds: PlotRect, color: PlotColor, radius: f32);

    /// A stroked open path through `points`.
    fn polyline(&mut self, points: &[PlotPoint], width: f32, color: PlotColor);

    /// How wide `text` will be, which right-aligning a tick label or centring a bar's name needs
    /// before it is drawn.
    fn measure(&mut self, text: &str, size: f32) -> f32;

    /// Draws `text` with its top-left at `origin`.
    fn text(&mut self, origin: PlotPoint, text: &str, size: f32, color: PlotColor);

    /// Runs `body` with everything it draws confined to `area`.
    ///
    /// A closure rather than a push/pop pair because a window's content mask may be
    /// closure-scoped, and a panned line has to be cut off at the plot's edge rather than drawn
    /// across its gutters.
    fn clipped(&mut self, area: PlotRect, body: &mut dyn FnMut(&mut dyn PlotCanvas));
}

/// Whether the bounds leave room for a plot after its axis gutters.
pub fn has_drawable_area(bounds: PlotRect) -> bool {
    let area = plot_area(bounds, 0.0);
    area.width > 0.0 && area.height > 0.0
}

/// Draws `plot` into `bounds` on any canvas.
pub fn draw(plot: &Plot<'_>, bounds: PlotRect, colors: PlotColors, canvas: &mut dyn PlotCanvas) {
    let legend_width = legend_width(plot, bounds, canvas);
    let area = plot_area(bounds, legend_width);
    if area.width <= 0.0 || area.height <= 0.0 {
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
    paint_legend(plot, bounds, legend_width, colors.text, canvas);
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

/// Where the data is drawn, how the view is placed on it, and the range the value axis covers.
struct Frame {
    area: PlotRect,
    view: PlotView,
    min: f64,
    max: f64,
}

impl Frame {
    /// Where `value` sits, at `index` of `count` points, once the view is applied.
    fn place(&self, index: usize, count: usize, value: f64) -> PlotPoint {
        self.view.apply(index_to_x(index, count, self.area), value_to_y(value, self.min, self.max, self.area))
    }
}

/// The rectangle the data itself is drawn in, inside the gutters the axes and their titles take.
fn plot_area(bounds: PlotRect, legend_width: f32) -> PlotRect {
    let legend_space = if legend_width > 0.0 { legend_width + LEGEND_GAP } else { 0.0 };
    let width = (bounds.width - LEFT_GUTTER - RIGHT_GUTTER - legend_space).max(0.0);
    let height = (bounds.height - TOP_GUTTER - BOTTOM_GUTTER).max(0.0);
    PlotRect::new(bounds.x + LEFT_GUTTER, bounds.y + TOP_GUTTER, width, height)
}

/// The range the value axis covers, always including zero so a line is read against it rather
/// than against its own smallest point.
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

/// Tick values covering `min..=max`, rounded to a step that reads as a round number rather than
/// to the data's own extremes.
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
fn value_to_y(value: f64, min: f64, max: f64, area: PlotRect) -> f32 {
    let span = (max - min).max(f64::EPSILON);
    let fraction = ((value - min) / span) as f32;
    area.y + area.height * (1.0 - fraction)
}

/// Where the `index`th of `count` points sits horizontally, before the view is applied. A lone
/// point is centred rather than pinned to the left edge.
fn index_to_x(index: usize, count: usize, area: PlotRect) -> f32 {
    if count <= 1 {
        return area.x + area.width * 0.5;
    }
    let step = area.width / (count - 1) as f32;
    area.x + step * index as f32
}

fn paint_grid(ticks: &[f64], frame: &Frame, colors: PlotColors, canvas: &mut dyn PlotCanvas) {
    let area = frame.area;
    let base_spacing = area.height / ticks.len().saturating_sub(1).max(1) as f32;
    let stride = spacing_stride(base_spacing * frame.view.zoom, MIN_VALUE_TICK_SPACING);
    for tick in ticks.iter().step_by(stride) {
        let y = frame.view.apply(0.0, value_to_y(*tick, frame.min, frame.max, area)).y;
        if y < area.y || y > area.bottom() {
            continue;
        }
        canvas.fill(PlotRect::new(area.x, y, area.width, 1.0), colors.grid, 0.0);
        let label = format_value(*tick);
        let width = canvas.measure(&label, TICK_FONT);
        let origin = PlotPoint::new(area.x - width - 6.0, y - TICK_FONT * 0.7);
        canvas.text(origin, &label, TICK_FONT, colors.text);
    }
}

/// Numbers the games under the value axis.
///
/// The count is the longest line's: every line is drawn across the whole width, each against its
/// own games, which is how the egui chart plots a ship that played fewer of them.
fn paint_game_ticks(plot: &Plot<'_>, frame: &Frame, colors: PlotColors, canvas: &mut dyn PlotCanvas) {
    let count = plot.series.iter().map(|series| series.points.len()).max().unwrap_or(0);
    if count == 0 {
        return;
    }

    let area = frame.area;
    let base_step = (count / TICKS).max(1);
    let point_spacing = area.width / count.saturating_sub(1).max(1) as f32 * frame.view.zoom;
    let step = base_step.max(spacing_stride(point_spacing, MIN_GAME_TICK_SPACING));
    for index in (0..count).step_by(step) {
        let x = frame.view.apply(index_to_x(index, count, area), 0.0).x;
        if x < area.x || x > area.right() {
            continue;
        }
        canvas.fill(PlotRect::new(x, area.y, 1.0, area.height), colors.grid, 0.0);
        let label = (index + 1).to_string();
        let width = canvas.measure(&label, TICK_FONT);
        let origin = PlotPoint::new(x - width * 0.5, area.bottom() + 6.0);
        canvas.text(origin, &label, TICK_FONT, colors.text);
    }
}

/// Drops axis marks whose transformed spacing would make their labels overlap.
fn spacing_stride(spacing: f32, minimum: f32) -> usize {
    if !spacing.is_finite() || spacing <= 0.0 { usize::MAX } else { (minimum / spacing).ceil().max(1.0) as usize }
}

fn paint_axes(area: PlotRect, color: PlotColor, canvas: &mut dyn PlotCanvas) {
    canvas.fill(PlotRect::new(area.x, area.bottom(), area.width, 1.0), color, 0.0);
    canvas.fill(PlotRect::new(area.x, area.y, 1.0, area.height), color, 0.0);
}

fn paint_lines(plot: &Plot<'_>, frame: &Frame, canvas: &mut dyn PlotCanvas) {
    let (line_width, point_radius, opacity) = match plot.series.len() {
        0..=7 => (1.5, POINT_RADIUS, 1.0),
        8..=15 => (1.2, 2.25, 0.82),
        _ => (1.0, 1.75, 0.68),
    };
    for series in plot.series {
        let count = series.points.len();
        let positions: Vec<PlotPoint> =
            series.points.iter().enumerate().map(|(index, point)| frame.place(index, count, point.value)).collect();
        let color = PlotColor::from(series.color);

        canvas.polyline(&positions, line_width, color.opacity(opacity));

        for (position, point) in positions.iter().zip(&series.points) {
            canvas.fill(dot_bounds(*position, point_radius), color.opacity(opacity), point_radius);
            if plot.show_values {
                let label = format_value(point.value);
                let width = canvas.measure(&label, TICK_FONT);
                let origin = PlotPoint::new(position.x - width * 0.5, position.y - (TICK_FONT + 6.0));
                canvas.text(origin, &label, TICK_FONT, color);
            }
        }
    }
}

fn paint_bars(plot: &Plot<'_>, frame: &Frame, colors: PlotColors, canvas: &mut dyn PlotCanvas) {
    let area = frame.area;
    let count = plot.bars.len();
    let slot = area.width * frame.view.zoom / count.max(1) as f32;
    let width = (slot * 0.7).max(1.0);
    let baseline = frame.view.apply(0.0, value_to_y(0.0f64.max(frame.min), frame.min, frame.max, area)).y;

    for (index, bar) in plot.bars.iter().enumerate() {
        let centre = frame.view.apply(
            area.x + area.width / count.max(1) as f32 * (index as f32 + 0.5),
            value_to_y(bar.value, frame.min, frame.max, area),
        );
        let top = centre.y.min(baseline);
        let height = (centre.y - baseline).abs().max(1.0);
        canvas.fill(PlotRect::new(centre.x - width * 0.5, top, width, height), PlotColor::from(bar.color), 0.0);

        // A label per bar is unreadable once the bars are thinner than the names, so they are
        // dropped rather than overlapped.
        if slot >= 48.0 {
            let label_width = canvas.measure(&bar.label, TICK_FONT);
            let origin = PlotPoint::new(centre.x - label_width * 0.5, baseline + 4.0);
            canvas.text(origin, &bar.label, TICK_FONT, colors.text);
        }
        if plot.show_values {
            let label = format_value(bar.value);
            let label_width = canvas.measure(&label, TICK_FONT);
            let origin = PlotPoint::new(centre.x - label_width * 0.5, top - (TICK_FONT + 4.0));
            canvas.text(origin, &label, TICK_FONT, PlotColor::from(bar.color));
        }
    }
}

fn paint_axis_titles(plot: &Plot<'_>, bounds: PlotRect, area: PlotRect, color: PlotColor, canvas: &mut dyn PlotCanvas) {
    // The value axis is named above it rather than rotated alongside it, which every canvas can
    // draw.
    canvas.text(PlotPoint::new(bounds.x + 4.0, bounds.y + 4.0), plot.y_label, LABEL_FONT, color);

    let width = canvas.measure(plot.x_label, LABEL_FONT);
    let origin = PlotPoint::new(area.x + (area.width - width) * 0.5, bounds.bottom() - (LABEL_FONT + 6.0));
    canvas.text(origin, plot.x_label, LABEL_FONT, color);
}

/// Names each line in a column beside the plot. Bars carry their name under them, so they are
/// not listed again.
fn legend_width(plot: &Plot<'_>, bounds: PlotRect, canvas: &mut dyn PlotCanvas) -> f32 {
    if plot.series.is_empty() {
        return 0.0;
    }
    let available = (bounds.width - LEFT_GUTTER - RIGHT_GUTTER - LEGEND_GAP - 120.0).max(0.0);
    if available < LEGEND_MIN_WIDTH {
        return 0.0;
    }
    let widest = plot.series.iter().map(|series| canvas.measure(&series.name, TICK_FONT)).fold(0.0, f32::max);

    (widest + LEGEND_SWATCH + 10.0).clamp(LEGEND_MIN_WIDTH.min(available), LEGEND_MAX_WIDTH.min(available))
}

fn paint_legend(plot: &Plot<'_>, bounds: PlotRect, width: f32, color: PlotColor, canvas: &mut dyn PlotCanvas) {
    if width == 0.0 {
        return;
    }
    let right = bounds.right() - RIGHT_GUTTER;
    let column = PlotRect::new(
        right - width,
        bounds.y + TOP_GUTTER,
        width,
        (bounds.height - (TOP_GUTTER + BOTTOM_GUTTER)).max(0.0),
    );
    let rows = (column.height / LEGEND_ROW).floor() as usize;
    let show_more = plot.series.len() > rows && rows > 0;
    let visible = if show_more { rows.saturating_sub(1) } else { rows };

    canvas.clipped(column, &mut |canvas| {
        for (row, series) in plot.series.iter().take(visible).enumerate() {
            let top = column.y + LEGEND_ROW * row as f32 + 2.0;
            let swatch = PlotPoint::new(column.x, top + 3.0);
            canvas.fill(
                PlotRect::new(swatch.x, swatch.y, LEGEND_SWATCH, LEGEND_SWATCH),
                PlotColor::from(series.color),
                2.0,
            );
            let label_width = (width - LEGEND_SWATCH - 6.0).max(0.0);
            let label = fit_legend_label(&series.name, label_width, canvas);
            canvas.text(PlotPoint::new(swatch.x + LEGEND_SWATCH + 6.0, top), &label, TICK_FONT, color);
        }
        if show_more {
            let top = column.y + LEGEND_ROW * visible as f32 + 2.0;
            let remaining = plot.series.len() - visible;
            canvas.text(
                PlotPoint::new(column.x, top),
                &t!("ui.stats.series_more", count = remaining),
                TICK_FONT,
                color,
            );
        }
    });
}

fn fit_legend_label(name: &str, width: f32, canvas: &mut dyn PlotCanvas) -> String {
    if canvas.measure(name, TICK_FONT) <= width {
        return name.to_string();
    }
    let mut label = String::new();
    for character in name.chars() {
        let mut candidate = label.clone();
        candidate.push(character);
        candidate.push_str("...");
        if canvas.measure(&candidate, TICK_FONT) > width {
            break;
        }
        label.push(character);
    }

    format!("{label}...")
}

fn dot_bounds(centre: PlotPoint, radius: f32) -> PlotRect {
    PlotRect::new(centre.x - radius, centre.y - radius, radius * 2.0, radius * 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::chart::SeriesPoint;

    /// Records what a plot asks for, measuring every character as six pixels.
    #[derive(Default)]
    struct Recorder {
        fills: Vec<PlotRect>,
        lines: Vec<Vec<PlotPoint>>,
        texts: Vec<String>,
    }

    impl PlotCanvas for Recorder {
        fn fill(&mut self, bounds: PlotRect, _color: PlotColor, _radius: f32) {
            self.fills.push(bounds);
        }

        fn polyline(&mut self, points: &[PlotPoint], _width: f32, _color: PlotColor) {
            self.lines.push(points.to_vec());
        }

        fn measure(&mut self, text: &str, _size: f32) -> f32 {
            text.chars().count() as f32 * 6.0
        }

        fn text(&mut self, _origin: PlotPoint, text: &str, _size: f32, _color: PlotColor) {
            self.texts.push(text.to_string());
        }

        fn clipped(&mut self, _area: PlotRect, body: &mut dyn FnMut(&mut dyn PlotCanvas)) {
            body(self);
        }
    }

    fn grey() -> PlotColors {
        let grey = PlotColor::new(0.6, 0.6, 0.6, 1.0);
        PlotColors { axis: grey, grid: grey, text: grey }
    }

    fn series(values: &[f64]) -> ChartSeries {
        ChartSeries {
            name: "Yamato".to_string(),
            color: Rgb { r: 200, g: 80, b: 80 },
            points: values.iter().map(|value| SeriesPoint { label: String::new(), value: *value }).collect(),
        }
    }

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
        view.zoom_about(2.0, PlotPoint::new(100.0, 50.0));
        let under = view.apply(100.0, 50.0);
        assert!(
            (under.x - 100.0).abs() < 0.001 && (under.y - 50.0).abs() < 0.001,
            "the cursor's own position does not move"
        );
    }

    #[test]
    fn a_wheel_notch_zooms_in_forward_and_out_backward() {
        let mut view = PlotView::default();
        view.wheel(3.0, PlotPoint::new(10.0, 10.0));
        assert_eq!(view.zoom, WHEEL_ZOOM_STEP);
        view.wheel(-3.0, PlotPoint::new(10.0, 10.0));
        assert!((view.zoom - 1.0).abs() < 1e-6);
        view.wheel(0.0, PlotPoint::new(10.0, 10.0));
        assert!((view.zoom - 1.0).abs() < 1e-6, "no movement, no zoom");
    }

    #[test]
    fn zoom_in_has_a_stop_and_zoom_out_can_continue() {
        let mut view = PlotView::default();
        for _ in 0..40 {
            view.zoom_about(2.0, PlotPoint::default());
        }
        assert_eq!(view.zoom, MAX_ZOOM);

        let mut view = PlotView::default();
        view.zoom_about(0.01, PlotPoint::default());
        assert!(view.zoom < 1.0, "zooming out passes the initial view");
    }

    #[test]
    fn a_reset_view_is_the_default_one() {
        let mut view = PlotView::default();
        view.drag(10.0, -4.0);
        assert!(!view.is_default());
        assert_eq!(view.apply(0.0, 0.0), PlotPoint::new(10.0, -4.0));
        view.reset();
        assert!(view.is_default());
    }

    #[test]
    fn values_are_labelled_at_the_precision_their_size_calls_for() {
        assert_eq!(format_value(47_231.4), "47231");
        assert_eq!(format_value(52.26), "52.3");
        assert_eq!(format_value(1.234), "1.23");
    }

    #[test]
    fn a_line_spans_the_plot_area_and_is_named_in_the_legend() {
        let lines = [series(&[1_000.0, 3_000.0, 2_000.0])];
        let plot = Plot {
            series: &lines,
            bars: &[],
            x_label: "Game",
            y_label: "Damage",
            show_values: true,
            view: PlotView::default(),
        };
        let bounds = PlotRect::new(0.0, 0.0, 600.0, 300.0);
        let mut canvas = Recorder::default();

        draw(&plot, bounds, grey(), &mut canvas);

        let line = &canvas.lines[0];
        assert_eq!(line.len(), 3);
        assert_eq!(line[0].x, LEFT_GUTTER, "the first game sits on the value axis");
        assert!(line[1].y < line[0].y && line[1].y < line[2].y, "the largest value is drawn highest");
        assert!(line[2].x < bounds.width - RIGHT_GUTTER - LEGEND_MIN_WIDTH, "the legend keeps its column");
        assert!(canvas.texts.iter().any(|text| text == "Yamato"), "the legend names the line");
        assert!(canvas.texts.iter().any(|text| text == "3000"), "a value label is drawn per point");
        assert!(canvas.texts.iter().any(|text| text == "Damage") && canvas.texts.iter().any(|text| text == "Game"));
    }

    #[test]
    fn bars_rise_from_zero_with_their_names_beneath() {
        let bars = [
            ChartBar { label: "Yamato".into(), value: 40_000.0, color: Rgb { r: 1, g: 2, b: 3 } },
            ChartBar { label: "Shima".into(), value: 20_000.0, color: Rgb { r: 3, g: 2, b: 1 } },
        ];
        let plot = Plot {
            series: &[],
            bars: &bars,
            x_label: "Ship",
            y_label: "Damage",
            show_values: false,
            view: PlotView::default(),
        };
        let bounds = PlotRect::new(0.0, 0.0, 600.0, 300.0);
        let mut canvas = Recorder::default();

        draw(&plot, bounds, grey(), &mut canvas);

        let area = plot_area(bounds, 0.0);
        let drawn: Vec<&PlotRect> = canvas.fills.iter().filter(|rect| rect.width > 1.0 && rect.height > 1.0).collect();
        assert_eq!(drawn.len(), 2);
        assert!((drawn[0].bottom() - area.bottom()).abs() < 0.5, "a bar stands on the zero line");
        assert!(drawn[0].height > drawn[1].height * 1.9, "heights follow the values");
        assert!(canvas.texts.iter().any(|text| text == "Shima"));
        assert!(canvas.lines.is_empty(), "a bar chart draws no line");
    }

    #[test]
    fn a_long_name_is_cut_to_fit_the_legend() {
        let mut canvas = Recorder::default();
        let label = fit_legend_label("Admiral Graf Spee", 60.0, &mut canvas);
        assert!(label.ends_with("..."));
        assert!(canvas.measure(&label, TICK_FONT) <= 60.0);
    }

    #[test]
    fn a_plot_too_small_for_its_gutters_has_no_area() {
        assert!(!has_drawable_area(PlotRect::new(0.0, 0.0, 70.0, 300.0)));
        assert!(has_drawable_area(PlotRect::new(0.0, 0.0, 300.0, 200.0)));
    }
}
