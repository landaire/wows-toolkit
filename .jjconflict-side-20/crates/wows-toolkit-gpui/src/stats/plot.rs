//! The stats charts' plot surface, painted into a GPUI window.
//!
//! gpui-component's own `LineChart`/`BarChart` draw one series each, at fixed bounds, with no
//! axis titles and nothing to pan with. The egui Stats tab draws a line per ship in that ship's
//! own colour, labels both axes, lists the series in a legend and lets the view be dragged and
//! zoomed (`ui/session_stats_chart.rs`), so the surface is painted here instead.
//!
//! Where everything goes is decided by `wows_toolkit_viewmodel::stats::plot`, which both front
//! ends and the copied image share; this module only supplies the window's primitives.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use wows_toolkit_viewmodel::stats::plot::Plot;
use wows_toolkit_viewmodel::stats::plot::PlotCanvas;
use wows_toolkit_viewmodel::stats::plot::PlotColor;
use wows_toolkit_viewmodel::stats::plot::PlotColors;
use wows_toolkit_viewmodel::stats::plot::PlotLayout;
use wows_toolkit_viewmodel::stats::plot::PlotPoint;
use wows_toolkit_viewmodel::stats::plot::PlotRect;

/// Paints `plot` into `bounds`, returning where the data went in window pixels.
///
/// Called from a `canvas` in the chart panel's render, which is the only place a `Window` to
/// paint with is at hand.
pub fn paint(plot: &Plot<'_>, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) -> Option<PlotLayout> {
    let colors = colors(cx);
    let mut canvas = WindowCanvas { window, cx };
    wows_toolkit_viewmodel::stats::plot::draw(plot, plot_rect(bounds), colors, &mut canvas)
}

/// The theme colours a plot paints with.
pub fn colors(cx: &App) -> PlotColors {
    let theme = cx.theme();
    PlotColors {
        axis: plot_color(theme.border),
        grid: plot_color(theme.border.opacity(0.4)),
        text: plot_color(theme.muted_foreground),
    }
}

/// Whether the bounds leave room for a plot after its axis gutters.
pub fn has_drawable_area(bounds: Bounds<Pixels>) -> bool {
    wows_toolkit_viewmodel::stats::plot::has_drawable_area(plot_rect(bounds))
}

pub fn plot_point(position: Point<Pixels>) -> PlotPoint {
    PlotPoint::new(position.x.as_f32(), position.y.as_f32())
}

fn plot_rect(bounds: Bounds<Pixels>) -> PlotRect {
    PlotRect::new(
        bounds.origin.x.as_f32(),
        bounds.origin.y.as_f32(),
        bounds.size.width.as_f32(),
        bounds.size.height.as_f32(),
    )
}

fn bounds_of(rect: PlotRect) -> Bounds<Pixels> {
    Bounds::new(point(px(rect.x), px(rect.y)), size(px(rect.width), px(rect.height)))
}

fn plot_color(color: Hsla) -> PlotColor {
    let rgba: Rgba = color.into();
    PlotColor::new(rgba.r, rgba.g, rgba.b, rgba.a)
}

fn hsla(color: PlotColor) -> Hsla {
    Rgba { r: color.r, g: color.g, b: color.b, a: color.a }.into()
}

/// Draws a plot onto the window, which is what the chart panel shows.
struct WindowCanvas<'a, 'b> {
    window: &'a mut Window,
    cx: &'b mut App,
}

impl PlotCanvas for WindowCanvas<'_, '_> {
    fn fill(&mut self, bounds: PlotRect, color: PlotColor, radius: f32) {
        self.window.paint_quad(fill(bounds_of(bounds), hsla(color)).corner_radii(px(radius)));
    }

    fn polyline(&mut self, points: &[PlotPoint], width: f32, color: PlotColor) {
        let [first, rest @ ..] = points else { return };
        if rest.is_empty() {
            return;
        }
        let mut path = PathBuilder::stroke(px(width));
        path.move_to(point(px(first.x), px(first.y)));
        for position in rest {
            path.line_to(point(px(position.x), px(position.y)));
        }
        if let Ok(path) = path.build() {
            self.window.paint_path(path, hsla(color));
        }
    }

    fn measure(&mut self, text: &str, size: f32) -> f32 {
        shape(text, size, gpui_kit::black(), self.window).width().as_f32()
    }

    fn text(&mut self, origin: PlotPoint, text: &str, size: f32, color: PlotColor) {
        let shaped = shape(text, size, hsla(color), self.window);
        let _ = shaped.paint(
            point(px(origin.x), px(origin.y)),
            px(size * 1.3),
            TextAlign::Left,
            None,
            self.window,
            self.cx,
        );
    }

    fn clipped(&mut self, area: PlotRect, body: &mut dyn FnMut(&mut dyn PlotCanvas)) {
        let cx = &mut *self.cx;
        self.window.with_content_mask(Some(ContentMask { bounds: bounds_of(area) }), |window| {
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
