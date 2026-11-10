//! The stats charts' plot surface on a GPUI window.
//!
//! gpui-component's own `LineChart`/`BarChart` draw one series each, at fixed
//! bounds, with no axis titles and nothing to pan with, so the plot is laid
//! out by the shared `wows_toolkit_viewmodel::stats::plot` and painted here.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use wows_toolkit_viewmodel::stats::plot::Colors;
use wows_toolkit_viewmodel::stats::plot::Plot;
use wows_toolkit_viewmodel::stats::plot::PlotCanvas;
use wows_toolkit_viewmodel::stats::plot::PlotColor;
use wows_toolkit_viewmodel::stats::plot::PlotPoint;
use wows_toolkit_viewmodel::stats::plot::PlotRect;

/// Paints `plot` into `bounds`.
///
/// Called from a `canvas` in the chart panel's render, which is the only
/// place a `Window` to paint with is at hand.
pub fn paint(plot: &Plot<'_>, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let colors = colors(cx);
    let mut canvas = WindowCanvas { window, cx };
    wows_toolkit_viewmodel::stats::plot::draw(plot, plot_rect(bounds), colors, &mut canvas);
}

/// The theme colours a plot paints with.
pub fn colors(cx: &App) -> Colors {
    let theme = cx.theme();
    Colors {
        axis: plot_color(theme.border),
        grid: plot_color(theme.border.opacity(0.4)),
        text: plot_color(theme.muted_foreground),
    }
}

/// Whether the bounds leave room for a plot after its axis gutters.
pub fn has_drawable_area(bounds: Bounds<Pixels>) -> bool {
    wows_toolkit_viewmodel::stats::plot::has_drawable_area(plot_rect(bounds))
}

fn plot_rect(bounds: Bounds<Pixels>) -> PlotRect {
    PlotRect::new(
        bounds.origin.x.as_f32(),
        bounds.origin.y.as_f32(),
        bounds.size.width.as_f32(),
        bounds.size.height.as_f32(),
    )
}

fn plot_color(color: Hsla) -> PlotColor {
    let rgba: Rgba = color.into();
    PlotColor::new(rgba.r, rgba.g, rgba.b, rgba.a)
}

fn hsla(color: PlotColor) -> Hsla {
    Rgba { r: color.r, g: color.g, b: color.b, a: color.a }.into()
}

fn bounds(rect: PlotRect) -> Bounds<Pixels> {
    Bounds::new(point(px(rect.origin.x), px(rect.origin.y)), size(px(rect.width), px(rect.height)))
}

fn pixel_point(position: PlotPoint) -> Point<Pixels> {
    point(px(position.x), px(position.y))
}

/// Draws a plot onto the window, which is what the chart panel shows.
struct WindowCanvas<'a, 'b> {
    window: &'a mut Window,
    cx: &'b mut App,
}

impl PlotCanvas for WindowCanvas<'_, '_> {
    fn fill(&mut self, rect: PlotRect, color: PlotColor, radius: f32) {
        self.window.paint_quad(fill(bounds(rect), hsla(color)).corner_radii(px(radius)));
    }

    fn polyline(&mut self, points: &[PlotPoint], width: f32, color: PlotColor) {
        let [first, rest @ ..] = points else { return };
        if rest.is_empty() {
            return;
        }
        let mut path = PathBuilder::stroke(px(width));
        path.move_to(pixel_point(*first));
        for position in rest {
            path.line_to(pixel_point(*position));
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
        let _ = shaped.paint(pixel_point(origin), px(size * 1.3), TextAlign::Left, None, self.window, self.cx);
    }

    fn clipped(&mut self, area: PlotRect, body: &mut dyn FnMut(&mut dyn PlotCanvas)) {
        let cx = &mut *self.cx;
        self.window.with_content_mask(Some(ContentMask { bounds: bounds(area) }), |window| {
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
