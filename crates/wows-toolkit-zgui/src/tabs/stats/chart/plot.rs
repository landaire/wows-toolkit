//! The plot surface: a custom element that draws the shared plot layout with the window's own
//! quads, paths and glyph runs.

use std::collections::HashMap;
use std::sync::Arc;

use wows_toolkit_viewmodel::stats::chart::ChartBar;
use wows_toolkit_viewmodel::stats::chart::ChartSeries;
use wows_toolkit_viewmodel::stats::plot::Plot;
use wows_toolkit_viewmodel::stats::plot::PlotCanvas;
use wows_toolkit_viewmodel::stats::plot::PlotColor;
use wows_toolkit_viewmodel::stats::plot::PlotColors;
use wows_toolkit_viewmodel::stats::plot::PlotPoint;
use wows_toolkit_viewmodel::stats::plot::PlotRect;
use wows_toolkit_viewmodel::stats::plot::PlotView;
use wows_toolkit_viewmodel::stats::plot::draw;
use zgui::app::LineRequest;
use zgui::app::Shaper;
use zgui::canvas::Clip;
use zgui::canvas::Ink;
use zgui::canvas::Paint;
use zgui::canvas::Shape;
use zgui::canvas::Stroke;
use zgui::canvas::zgui_color::Color;
use zgui::custom::CustomElement;
use zgui::custom::CustomLayoutCx;
use zgui::custom::CustomMeasured;
use zgui::custom::PaintSlot;
use zgui::custom::ScenePainter;
use zgui::custom::ShapedRunOwned;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::geom::Size;
use zgui::scene::kurbo;
use zgui::scene::peniko;
use zgui::view::Ident;

/// The custom properties the element reads its colours from, set by the chart's stylesheet.
pub(super) const AXIS_PROPERTY: &str = "wt-plot-axis";
pub(super) const GRID_PROPERTY: &str = "wt-plot-grid";
pub(super) const TEXT_PROPERTY: &str = "wt-plot-text";
pub(super) const BACKGROUND_PROPERTY: &str = "wt-plot-background";

/// Shaped labels kept between paints, keyed by text and device size. Cleared rather than evicted
/// when it grows past this: a chart's labels are a few dozen strings.
const SHAPED_CACHE_LIMIT: usize = 512;

/// The desktops' interface faces, tried in order. A shaped line takes family names only, not
/// `system-ui`, and left to its fallback it draws digits from a colour emoji face.
const UI_FAMILIES: [&str; 8] =
    ["Segoe UI", "Adwaita Sans", "Cantarell", "SF Pro Text", "Helvetica Neue", "Noto Sans", "DejaVu Sans", "Arial"];

/// What one plot draws, owned so it can be held across frames.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct PlotData {
    pub series: Vec<ChartSeries>,
    pub bars: Vec<ChartBar>,
    pub x_label: String,
    pub y_label: String,
    pub show_values: bool,
    pub view: PlotView,
}

impl PlotData {
    pub fn plot(&self) -> Plot<'_> {
        Plot {
            series: &self.series,
            bars: &self.bars,
            x_label: &self.x_label,
            y_label: &self.y_label,
            show_values: self.show_values,
            view: self.view,
        }
    }
}

/// The theme colours the element last read from its style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ThemeColors {
    pub plot: PlotColors,
    pub background: PlotColor,
}

/// The plot as a retained element.
pub(super) struct PlotElement {
    pub data: PlotData,
    /// `None` until the first layout reads the style.
    pub colors: Option<ThemeColors>,
    shaper: Shaper,
    families: Vec<Ident>,
    shaped: HashMap<(Arc<str>, u32), Shaped>,
}

impl PlotElement {
    pub fn new(shaper: Shaper) -> Self {
        Self {
            data: PlotData::default(),
            colors: None,
            shaper,
            families: UI_FAMILIES.iter().copied().map(Ident::new).collect(),
            shaped: HashMap::new(),
        }
    }
}

impl CustomElement for PlotElement {
    fn layout(&mut self, cx: &mut CustomLayoutCx<'_>) -> CustomMeasured {
        let read = |name: &str| zgui_css::values::custom::color(cx.style, name).map(plot_color);
        if let (Some(axis), Some(grid), Some(text), Some(background)) =
            (read(AXIS_PROPERTY), read(GRID_PROPERTY), read(TEXT_PROPERTY), read(BACKGROUND_PROPERTY))
        {
            self.colors = Some(ThemeColors { plot: PlotColors { axis, grid, text }, background });
        }

        // The box is sized by its stylesheet; the element has no size of its own to ask for.
        CustomMeasured {
            width: cx.known_width.unwrap_or(0.0),
            height: cx.known_height.unwrap_or(0.0),
            ..CustomMeasured::default()
        }
    }

    fn paint(&mut self, painter: &mut ScenePainter<'_>) {
        let Some(colors) = self.colors else { return };
        let scale = painter.scale();
        let size = painter.size();
        let bounds = PlotRect::new(0.0, 0.0, size.width.0 / scale, size.height.0 / scale);

        let Self { data, shaper, families, shaped, .. } = self;
        if shaped.len() > SHAPED_CACHE_LIMIT {
            shaped.clear();
        }
        let mut canvas = SceneCanvas { painter, scale, shaper, families, shaped, clip: None };
        draw(&data.plot(), bounds, colors.plot, &mut canvas);
    }

    fn custom_properties_read(&self) -> Option<&'static [&'static str]> {
        Some(&["wt-plot-*"])
    }
}

/// One label, shaped once: its runs, without the trailing measuring glyph, and its advance.
struct Shaped {
    runs: Vec<ShapedRunOwned>,
    /// Device pixels from the start of the line to where the next character would go.
    width: f32,
}

/// The shared plot layout drawn through a scene painter, in logical pixels scaled to the device.
struct SceneCanvas<'a, 'p> {
    painter: &'a mut ScenePainter<'p>,
    scale: f32,
    shaper: &'a mut Shaper,
    families: &'a [Ident],
    shaped: &'a mut HashMap<(Arc<str>, u32), Shaped>,
    clip: Option<PlotRect>,
}

impl SceneCanvas<'_, '_> {
    fn device_rect(&self, rect: PlotRect) -> Rect<DevicePx, Device> {
        Rect::new(
            Point::new(DevicePx(rect.x * self.scale), DevicePx(rect.y * self.scale)),
            Size::new(DevicePx(rect.width * self.scale), DevicePx(rect.height * self.scale)),
        )
    }

    fn device_path(&self, points: &[PlotPoint]) -> kurbo::BezPath {
        let mut path = kurbo::BezPath::new();
        for (index, point) in points.iter().enumerate() {
            let at = kurbo::Point::new(f64::from(point.x * self.scale), f64::from(point.y * self.scale));
            if index == 0 {
                path.move_to(at);
            } else {
                path.line_to(at);
            }
        }

        path
    }

    /// `text` at `size` logical pixels, shaped at the device size.
    fn shape(&mut self, text: &str, size: f32) -> &Shaped {
        let device_size = size * self.scale;
        let key = (Arc::<str>::from(text), device_size.to_bits());
        let Self { shaper, families, shaped, .. } = self;

        shaped.entry(key).or_insert_with(|| {
            let request = LineRequest {
                families,
                weight: 400,
                italic: false,
                size_device_px: device_size,
                letter_spacing: 0.0,
                ligatures: true,
            };
            // A shaped line reports where each glyph starts but not where the line ends, so one
            // more glyph is shaped after the text and its start taken as the text's advance.
            let mut runs = shaper.shape_line(&format!("{text}0"), &request);
            let width = match runs.last_mut() {
                Some(last) => {
                    let end = last.glyphs.pop().map_or(0.0, |glyph| glyph.x);
                    last.clusters.pop();
                    end
                }
                None => 0.0,
            };
            runs.retain(|run| !run.glyphs.is_empty());

            Shaped { runs, width }
        })
    }
}

impl PlotCanvas for SceneCanvas<'_, '_> {
    fn fill(&mut self, bounds: PlotRect, color: PlotColor, radius: f32) {
        let bounds = match self.clip {
            Some(clip) => match bounds.intersect(&clip) {
                Some(inside) => inside,
                None => return,
            },
            None => bounds,
        };
        let rect = self.device_rect(bounds);
        self.painter.fill(rect, radius * self.scale, zgui_color(color));
    }

    fn polyline(&mut self, points: &[PlotPoint], width: f32, color: PlotColor) {
        if points.len() < 2 {
            return;
        }
        let path = self.device_path(points);
        let clips = match self.clip {
            Some(clip) => {
                let area = self.device_rect(clip);
                let rect = kurbo::Rect::new(
                    f64::from(area.origin.x.0),
                    f64::from(area.origin.y.0),
                    f64::from(area.origin.x.0 + area.size.width.0),
                    f64::from(area.origin.y.0 + area.size.height.0),
                );
                vec![Clip { path: Arc::new(kurbo::Shape::to_path(&rect, 0.1)), rule: peniko::Fill::NonZero }]
            }
            None => Vec::new(),
        };
        let stroke = kurbo::Stroke::new(f64::from(width * self.scale))
            .with_join(kurbo::Join::Round)
            .with_caps(kurbo::Cap::Round);
        self.painter.shape(&Shape {
            path: Arc::new(path),
            fill: None,
            stroke: Some(Stroke { paint: Paint::Solid(Ink::Solid(zgui_color(color))), style: stroke }),
            clips,
        });
    }

    fn measure(&mut self, text: &str, size: f32) -> f32 {
        let scale = self.scale;
        self.shape(text, size).width / scale
    }

    fn text(&mut self, origin: PlotPoint, text: &str, size: f32, color: PlotColor) {
        // A clipped label is drawn whole or not at all: a painter has no inner clip for glyphs.
        if let Some(clip) = self.clip
            && (origin.x < clip.x || origin.y < clip.y || origin.x > clip.right() || origin.y > clip.bottom())
        {
            return;
        }
        let scale = self.scale;
        let baseline = (origin.y + size) * scale;
        let left = origin.x * scale;
        let color = zgui_color(color);

        // Moved out of the cache for the borrow of the painter, and put back after.
        let key = (Arc::<str>::from(text), (size * scale).to_bits());
        self.shape(text, size);
        let Some(shaped) = self.shaped.remove(&key) else { return };
        for run in &shaped.runs {
            let Some(first) = run.glyphs.first() else { continue };
            // A run's glyph positions are measured from its line box's top; the plot places
            // labels by their baseline, one font size below the origin it gives.
            let line_top = baseline - first.y;
            self.painter.glyphs(&run.as_run(PaintSlot(0)), Point::new(DevicePx(left), DevicePx(line_top)), color);
        }
        self.shaped.insert(key, shaped);
    }

    fn clipped(&mut self, area: PlotRect, body: &mut dyn FnMut(&mut dyn PlotCanvas)) {
        let previous = self.clip.replace(area);
        body(self);
        self.clip = previous;
    }
}

fn zgui_color(color: PlotColor) -> Color {
    Color::srgb(color.r, color.g, color.b, color.a)
}

fn plot_color(color: Color) -> PlotColor {
    let [r, g, b, a] = color.to_premultiplied_srgb();
    if a <= 0.0 {
        return PlotColor::new(0.0, 0.0, 0.0, 0.0);
    }

    PlotColor::new(r / a, g / a, b / a, a)
}
