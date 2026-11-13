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

/// How far the view can zoom in or out from the whole plot, as a ratio of spans.
const MAX_ZOOM: f64 = 40.0;

/// How much one wheel notch zooms.
pub const WHEEL_ZOOM_STEP: f64 = 1.1;

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

/// The part of one axis a plot shows, in data units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisRange {
    pub min: f64,
    pub max: f64,
}

impl AxisRange {
    pub const fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }

    pub fn span(&self) -> f64 {
        (self.max - self.min).max(f64::EPSILON)
    }

    /// How far along the range `value` is, zero at `min` and one at `max`.
    fn fraction(&self, value: f64) -> f64 {
        (value - self.min) / self.span()
    }

    /// The value `fraction` of the way along the range.
    fn at(&self, fraction: f64) -> f64 {
        self.min + self.span() * fraction
    }

    /// The range scaled to `span` about `anchor`, which stays at the same fraction of it.
    fn scaled_about(&self, anchor: f64, span: f64) -> Self {
        let fraction = self.fraction(anchor);
        let min = anchor - span * fraction;
        Self::new(min, min + span)
    }

    fn shifted(&self, by: f64) -> Self {
        Self::new(self.min + by, self.max + by)
    }
}

/// The data a plot shows across both axes.
///
/// The game axis counts games from zero for a line plot and bar slots from zero for a bar plot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataWindow {
    pub x: AxisRange,
    pub y: AxisRange,
}

/// What a plot shows: the whole of its data until the reader pans or zooms, then the data window
/// they left it on.
///
/// Held in data units, so the same games and values stay in view when the plot is drawn at
/// another size. Zooming scales both axes together: the shape of a line is what is being read,
/// and stretching one axis alone would distort it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlotView {
    Fit,
    Window(DataWindow),
}

impl Default for PlotView {
    fn default() -> Self {
        Self::new()
    }
}

impl PlotView {
    pub const fn new() -> Self {
        Self::Fit
    }

    pub fn is_default(&self) -> bool {
        *self == Self::Fit
    }

    /// The data shown, given the window that fits all of it.
    pub fn window(&self, fit: DataWindow) -> DataWindow {
        match self {
            Self::Fit => fit,
            Self::Window(window) => *window,
        }
    }

    /// Zooms by `factor` about `cursor`, a position in the canvas's space, over the area
    /// `layout` drew in. The data under the cursor stays under it.
    pub fn zoom_about(&mut self, factor: f64, cursor: PlotPoint, layout: &PlotLayout) {
        let shown = self.window(layout.fit);
        let span = shown.x.span();
        let fit = layout.fit.x.span();
        // A window kept over changed data can sit past a stop; a step toward the stops still
        // applies whole, and only a step past one is cut short.
        let applied = if factor >= 1.0 {
            factor.min((span / (fit / MAX_ZOOM)).max(1.0))
        } else {
            factor.max((span / (fit * MAX_ZOOM)).min(1.0))
        };
        if applied == 1.0 || !applied.is_finite() {
            return;
        }

        let (game, value) = layout.data_at(&shown, cursor);
        *self = Self::Window(DataWindow {
            x: shown.x.scaled_about(game, shown.x.span() / applied),
            y: shown.y.scaled_about(value, shown.y.span() / applied),
        });
    }

    /// Zooms one wheel notch about `cursor`: in for a positive `delta`, out for a negative one.
    pub fn wheel(&mut self, delta: f32, cursor: PlotPoint, layout: &PlotLayout) {
        if delta == 0.0 {
            return;
        }
        let factor = if delta > 0.0 { WHEEL_ZOOM_STEP } else { 1.0 / WHEEL_ZOOM_STEP };
        self.zoom_about(factor, cursor, layout);
    }

    /// Moves the data with a pointer that moved by `dx`, `dy` canvas pixels over the area
    /// `layout` drew in.
    pub fn drag(&mut self, dx: f32, dy: f32, layout: &PlotLayout) {
        let shown = self.window(layout.fit);
        let x = -f64::from(dx) / f64::from(layout.area.width) * shown.x.span();
        let y = f64::from(dy) / f64::from(layout.area.height) * shown.y.span();
        *self = Self::Window(DataWindow { x: shown.x.shifted(x), y: shown.y.shifted(y) });
    }

    pub fn reset(&mut self) {
        *self = Self::Fit;
    }
}

/// Where a plot was drawn and what it showed, which turning a pointer into data needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotLayout {
    /// The rectangle the data is drawn in, in the canvas's space.
    pub area: PlotRect,
    /// The data shown.
    pub window: DataWindow,
    /// The window that shows all of the data.
    pub fit: DataWindow,
}

impl PlotLayout {
    /// The data at `point` in the canvas's space, as game and value, with `window` shown.
    fn data_at(&self, window: &DataWindow, point: PlotPoint) -> (f64, f64) {
        let x = f64::from((point.x - self.area.x) / self.area.width);
        let y = f64::from((self.area.bottom() - point.y) / self.area.height);
        (window.x.at(x), window.y.at(y))
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

/// Draws `plot` into `bounds` on any canvas. `None` when the bounds leave no room for the data.
pub fn draw(plot: &Plot<'_>, bounds: PlotRect, colors: PlotColors, canvas: &mut dyn PlotCanvas) -> Option<PlotLayout> {
    let legend_width = legend_width(plot, bounds, canvas);
    let area = plot_area(bounds, legend_width);
    if area.width <= 0.0 || area.height <= 0.0 {
        return None;
    }

    let fit = fit_window(plot);
    let frame = Frame { area, window: plot.view.window(fit) };

    paint_grid(&frame, colors, canvas);
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

    Some(PlotLayout { area, window: frame.window, fit })
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

/// Where the data is drawn and which of it is shown.
struct Frame {
    area: PlotRect,
    window: DataWindow,
}

impl Frame {
    fn x(&self, game: f64) -> f32 {
        self.area.x + self.window.x.fraction(game) as f32 * self.area.width
    }

    fn y(&self, value: f64) -> f32 {
        self.area.bottom() - self.window.y.fraction(value) as f32 * self.area.height
    }
}

/// The window that shows all of the data: every game or bar across the width, and the value
/// ticks' range up the height.
fn fit_window(plot: &Plot<'_>) -> DataWindow {
    let (min, max) = value_range(plot);
    let ticks = tick_values(min, max);
    let y = AxisRange::new(ticks.first().copied().unwrap_or(min), ticks.last().copied().unwrap_or(max));

    let x = if !plot.bars.is_empty() {
        AxisRange::new(0.0, plot.bars.len() as f64)
    } else {
        match longest_line(plot) {
            // A lone game is centred rather than pinned to the value axis.
            0 | 1 => AxisRange::new(-1.0, 1.0),
            longest => AxisRange::new(0.0, (longest - 1) as f64),
        }
    };

    DataWindow { x, y }
}

/// The number of games in the longest line, which numbers the game axis.
fn longest_line(plot: &Plot<'_>) -> usize {
    plot.series.iter().map(|series| series.points.len()).max().unwrap_or(0)
}

/// Where the `index`th of a line's `count` games sits on the game axis of a plot whose longest
/// line has `longest`. Every line is drawn across the whole width, each against its own games,
/// which is how the egui chart plots a ship that played fewer of them.
fn game_position(index: usize, count: usize, longest: usize) -> f64 {
    let last = longest.saturating_sub(1) as f64;
    if count <= 1 {
        return last * 0.5;
    }

    index as f64 * last / (count - 1) as f64
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
    while value < max + step && ticks.len() < TICKS * 3 {
        ticks.push(value);
        value += step;
    }

    ticks
}

fn paint_grid(frame: &Frame, colors: PlotColors, canvas: &mut dyn PlotCanvas) {
    let area = frame.area;
    let window = frame.window.y;
    let ticks = tick_values(window.min, window.max);
    let step = match ticks.as_slice() {
        [first, second, ..] => second - first,
        _ => window.span(),
    };
    let spacing = area.height * (step / window.span()) as f32;
    let stride = spacing_stride(spacing, MIN_VALUE_TICK_SPACING);
    for tick in ticks.iter().step_by(stride) {
        let y = frame.y(*tick);
        if y < area.y - 0.5 || y > area.bottom() + 0.5 {
            continue;
        }
        canvas.fill(PlotRect::new(area.x, y, area.width, 1.0), colors.grid, 0.0);
        let label = format_value(*tick);
        let width = canvas.measure(&label, TICK_FONT);
        let origin = PlotPoint::new(area.x - width - 6.0, y - TICK_FONT * 0.7);
        canvas.text(origin, &label, TICK_FONT, colors.text);
    }
}

/// Numbers the games under the value axis, counting the longest line's.
fn paint_game_ticks(plot: &Plot<'_>, frame: &Frame, colors: PlotColors, canvas: &mut dyn PlotCanvas) {
    let longest = longest_line(plot);
    if longest == 0 {
        return;
    }

    let area = frame.area;
    let window = frame.window.x;
    let per_game = area.width / window.span() as f32;
    let step =
        ((window.span() / TICKS as f64).ceil() as usize).max(1).max(spacing_stride(per_game, MIN_GAME_TICK_SPACING));
    let first = ((window.min.max(0.0) / step as f64).ceil() as usize).saturating_mul(step);
    let last = (longest - 1).min(window.max.max(0.0).floor() as usize);
    for index in (first..=last).step_by(step) {
        let x = frame.x(game_position(index, longest, longest));
        if x < area.x - 0.5 || x > area.right() + 0.5 {
            continue;
        }
        canvas.fill(PlotRect::new(x, area.y, 1.0, area.height), colors.grid, 0.0);
        let label = (index + 1).to_string();
        let width = canvas.measure(&label, TICK_FONT);
        let origin = PlotPoint::new(x - width * 0.5, area.bottom() + 6.0);
        canvas.text(origin, &label, TICK_FONT, colors.text);
    }
}

/// Drops axis marks whose spacing would make their labels overlap.
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
    let longest = longest_line(plot);
    for series in plot.series {
        let count = series.points.len();
        let positions: Vec<PlotPoint> = series
            .points
            .iter()
            .enumerate()
            .map(|(index, point)| PlotPoint::new(frame.x(game_position(index, count, longest)), frame.y(point.value)))
            .collect();
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
    let slot = frame.area.width / frame.window.x.span() as f32;
    let width = (slot * 0.7).max(1.0);
    let baseline = frame.y(0.0f64.max(frame.window.y.min));

    for (index, bar) in plot.bars.iter().enumerate() {
        let centre = PlotPoint::new(frame.x(index as f64 + 0.5), frame.y(bar.value));
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
    fn the_top_tick_is_at_or_above_the_largest_value() {
        let ticks = tick_values(0.0, 337_082.0);
        assert_eq!(ticks.last().copied(), Some(400_000.0), "a line is not cut off above the top tick");
        assert_eq!(tick_values(0.0, 300_000.0).last().copied(), Some(300_000.0));
    }

    #[test]
    fn a_flat_range_still_yields_ticks() {
        assert_eq!(tick_values(5.0, 5.0).len(), 2);
    }

    /// A three-game line drawn into `bounds` through `view`, and where it went.
    fn drawn(view: PlotView, bounds: PlotRect) -> (Recorder, PlotLayout) {
        let lines = [series(&[1_000.0, 3_000.0, 2_000.0])];
        let plot = Plot { series: &lines, bars: &[], x_label: "Game", y_label: "Damage", show_values: false, view };
        let mut canvas = Recorder::default();
        let layout = draw(&plot, bounds, grey(), &mut canvas).expect("the bounds hold a plot");

        (canvas, layout)
    }

    /// Equal to within what the canvas's single-precision pixels carry.
    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-2
    }

    const WIDE: PlotRect = PlotRect::new(0.0, 0.0, 600.0, 300.0);
    const SMALL: PlotRect = PlotRect::new(0.0, 0.0, 300.0, 180.0);

    #[test]
    fn the_whole_plot_fills_its_area_at_any_size() {
        for bounds in [WIDE, SMALL] {
            let (canvas, layout) = drawn(PlotView::default(), bounds);

            let line = &canvas.lines[0];
            assert_eq!(line[0].x, layout.area.x, "the first game is on the value axis");
            assert_eq!(line[2].x, layout.area.right(), "the last game is on the right edge");
            assert_eq!(layout.window.y.min, 0.0);
            let zero = layout.area.bottom() - layout.window.y.fraction(0.0) as f32 * layout.area.height;
            assert_eq!(zero, layout.area.bottom(), "zero is on the game axis");
            assert_eq!(layout.window, layout.fit);
        }
    }

    #[test]
    fn zooming_keeps_the_data_under_the_cursor() {
        let (_, layout) = drawn(PlotView::default(), WIDE);
        let cursor = PlotPoint::new(layout.area.x + 100.0, layout.area.y + 50.0);
        let under = layout.data_at(&layout.window, cursor);

        let mut view = PlotView::default();
        view.zoom_about(2.0, cursor, &layout);
        let (_, zoomed) = drawn(view, WIDE);

        assert!(close(zoomed.data_at(&zoomed.window, cursor), under), "the cursor's data does not move");
        assert!((zoomed.window.x.span() - layout.window.x.span() / 2.0).abs() < 1e-9, "half the games show");
        assert!((zoomed.window.y.span() - layout.window.y.span() / 2.0).abs() < 1e-6, "both axes zoom together");
    }

    #[test]
    fn a_wheel_notch_zooms_in_forward_and_out_backward() {
        let (_, layout) = drawn(PlotView::default(), WIDE);
        let cursor = PlotPoint::new(layout.area.x + 10.0, layout.area.y + 10.0);
        let mut view = PlotView::default();

        view.wheel(0.0, cursor, &layout);
        assert!(view.is_default(), "no movement, no zoom");

        view.wheel(3.0, cursor, &layout);
        let (_, zoomed) = drawn(view, WIDE);
        assert!((layout.window.x.span() / zoomed.window.x.span() - WHEEL_ZOOM_STEP).abs() < 1e-9);

        view.wheel(-3.0, cursor, &zoomed);
        let (_, back) = drawn(view, WIDE);
        assert!((back.window.x.span() - layout.window.x.span()).abs() < 1e-9, "a notch back undoes it");
    }

    #[test]
    fn zoom_has_a_stop_each_way() {
        let mut view = PlotView::default();
        for _ in 0..40 {
            let (_, layout) = drawn(view, WIDE);
            view.zoom_about(2.0, PlotPoint::new(200.0, 100.0), &layout);
        }
        let (_, layout) = drawn(view, WIDE);
        assert!((layout.fit.x.span() / layout.window.x.span() - MAX_ZOOM).abs() < 1e-6);

        let mut view = PlotView::default();
        view.zoom_about(0.5, PlotPoint::new(200.0, 100.0), &drawn(view, WIDE).1);
        let (_, layout) = drawn(view, WIDE);
        assert!(layout.window.x.span() > layout.fit.x.span(), "zooming out passes the whole plot");
    }

    /// A window kept from a longer session sits far past the zoom-out stop of a shorter one, and
    /// a wheel notch still zooms by one notch.
    #[test]
    fn a_notch_over_a_window_kept_from_other_data_zooms_by_one_notch() {
        let (_, layout) = drawn(PlotView::default(), WIDE);
        let kept = DataWindow { x: AxisRange::new(0.0, 999.0), y: layout.fit.y };
        let mut view = PlotView::Window(kept);

        view.wheel(1.0, PlotPoint::new(layout.area.x, layout.area.y), &layout);

        let zoomed = view.window(layout.fit);
        assert!((kept.x.span() / zoomed.x.span() - WHEEL_ZOOM_STEP).abs() < 1e-9);
    }

    #[test]
    fn dragging_moves_the_data_with_the_pointer() {
        let (_, layout) = drawn(PlotView::default(), WIDE);
        let from = PlotPoint::new(layout.area.x + 100.0, layout.area.y + 100.0);
        let to = PlotPoint::new(from.x + 30.0, from.y - 20.0);
        let held = layout.data_at(&layout.window, from);

        // Two moves before the plot is drawn again both count.
        let mut view = PlotView::default();
        view.drag(10.0, -5.0, &layout);
        view.drag(to.x - from.x - 10.0, to.y - from.y + 5.0, &layout);
        let (_, dragged) = drawn(view, WIDE);

        assert!(close(dragged.data_at(&dragged.window, to), held), "what was under the pointer is still under it");
        view.reset();
        assert!(view.is_default());
    }

    /// A resize changes how big the data is drawn, not which data is shown.
    #[test]
    fn a_resize_keeps_the_data_in_view() {
        let (_, layout) = drawn(PlotView::default(), WIDE);
        let mut view = PlotView::default();
        view.zoom_about(2.0, PlotPoint::new(layout.area.x + 50.0, layout.area.y + 40.0), &layout);
        let (_, zoomed) = drawn(view, WIDE);
        view.drag(-25.0, 10.0, &zoomed);
        let (wide_canvas, wide) = drawn(view, WIDE);

        let (small_canvas, small) = drawn(view, SMALL);

        assert_eq!(small.window, wide.window, "the same games and values are shown");
        let fraction =
            |point: PlotPoint, area: PlotRect| ((point.x - area.x) / area.width, (point.y - area.y) / area.height);
        for (big, little) in wide_canvas.lines[0].iter().zip(&small_canvas.lines[0]) {
            let (a, b) = (fraction(*big, wide.area), fraction(*little, small.area));
            assert!((a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4, "{a:?} and {b:?} sit alike");
        }
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
