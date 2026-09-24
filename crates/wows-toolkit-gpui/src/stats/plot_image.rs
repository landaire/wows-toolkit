//! Drawing a plot without a window, so a chart can be copied as a picture.
//!
//! GPUI cannot hand back a rendered window: `Window::render_to_image` exists
//! but only the test platform implements it. So the plot is drawn a second
//! time here, through the same [`PlotCanvas`] the window uses, which means
//! the two agree on where every tick, bar and label goes; only the
//! primitives differ.
//!
//! Text is the part that cannot be shared. GPUI shapes with a font it will
//! not lend out (`rasterize_glyph` is private, and nothing reads a face's
//! bytes back), so the glyphs here come from the system's own fonts through
//! `fontdb`. Nothing is bundled: a chart in Japanese or Russian is drawn with
//! the faces the desktop already has, rather than tofu from a Latin-only
//! file.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

use ab_glyph::Font as _;
use ab_glyph::FontRef;
use ab_glyph::PxScale;
use ab_glyph::ScaleFont as _;
use gpui_kit::Bounds;
use gpui_kit::Hsla;
use gpui_kit::Pixels;
use gpui_kit::Point;
use gpui_kit::Rgba;
use gpui_kit::px;
use tiny_skia::Paint;
use tiny_skia::PathBuilder;
use tiny_skia::Pixmap;
use tiny_skia::Rect;
use tiny_skia::Stroke;
use tiny_skia::Transform;

use super::plot::Colors;
use super::plot::Plot;
use super::plot::PlotCanvas;
use super::plot::draw;

/// What a copied chart is drawn on, so a pasted picture is readable on a
/// light background as well as a dark one.
const BACKGROUND: [u8; 4] = [24, 24, 27, 255];

/// A face's bytes and which face inside them, which is what `ab_glyph` reads
/// a glyph out of.
type FaceBytes = Arc<(Vec<u8>, u32)>;

/// The faces the system offers, enumerated once for the process.
///
/// Walking the font directories costs the better part of a second on a
/// desktop with a full set installed, and every canvas would otherwise pay
/// it again.
fn faces() -> &'static Faces {
    static FACES: OnceLock<Faces> = OnceLock::new();
    FACES.get_or_init(Faces::load)
}

/// Faces the system offers, with what has already been looked up in them.
struct Faces {
    db: fontdb::Database,
    /// The families to try, in order. The first that covers a character wins,
    /// which is what puts a Japanese label in a Japanese face.
    order: Vec<fontdb::ID>,
    /// Which face covers a character. Finding one parses every face ahead of
    /// it, so a label of repeated characters would pay for each of them.
    covering: Mutex<HashMap<char, Option<fontdb::ID>>>,
    /// A face's bytes, kept. `with_face_data` lends a borrow that cannot be
    /// held while the pixmap is written, so the bytes have to be copied out;
    /// copying a font file per glyph is what made a chart cost seconds.
    bytes: Mutex<HashMap<fontdb::ID, Option<FaceBytes>>>,
}

impl Faces {
    fn load() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        // The desktop's own UI faces first, then everything else as fallback,
        // so a Latin label looks like the app and a CJK one still renders.
        let preferred = ["Segoe UI", "Inter", "Helvetica Neue", "Arial", "DejaVu Sans", "Noto Sans"];
        let mut order: Vec<fontdb::ID> = Vec::new();
        for family in preferred {
            let query = fontdb::Query { families: &[fontdb::Family::Name(family)], ..Default::default() };
            if let Some(id) = db.query(&query) {
                order.push(id);
            }
        }
        let rest: Vec<fontdb::ID> = db.faces().map(|face| face.id).filter(|id| !order.contains(id)).collect();
        order.extend(rest);
        Self { db, order, covering: Mutex::new(HashMap::new()), bytes: Mutex::new(HashMap::new()) }
    }

    /// `id`'s bytes and face index, read out of the database once.
    fn bytes(&self, id: fontdb::ID) -> Option<FaceBytes> {
        let mut bytes = self.bytes.lock().expect("the face cache is only held to read or fill it");
        bytes
            .entry(id)
            .or_insert_with(|| self.db.with_face_data(id, |data, index| Arc::new((data.to_vec(), index))))
            .clone()
    }

    /// The first face that can draw `ch`.
    ///
    /// `None` when nothing installed covers it, which is a character that
    /// would show as a blank box in any application.
    fn face_for(&self, ch: char) -> Option<fontdb::ID> {
        if let Some(known) = self.covering.lock().expect("the cover cache is only held to read or fill it").get(&ch) {
            return *known;
        }
        let found = self.search_for(ch);
        self.covering.lock().expect("the cover cache is only held to read or fill it").insert(ch, found);
        found
    }

    fn search_for(&self, ch: char) -> Option<fontdb::ID> {
        self.order.iter().copied().find(|id| {
            self.db
                .with_face_data(*id, |data, index| {
                    FontRef::try_from_slice_and_index(data, index).map(|font| font.glyph_id(ch).0 != 0).unwrap_or(false)
                })
                .unwrap_or(false)
        })
    }
}

/// Draws a plot into an image.
pub struct ImageCanvas {
    pixmap: Pixmap,
    /// What the current [`clipped`](PlotCanvas::clipped) confines drawing to.
    clip: Option<Bounds<Pixels>>,
}

impl ImageCanvas {
    /// An empty canvas of `width` by `height`.
    ///
    /// `None` for a size no pixmap can be made at, which is a zero dimension.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let mut pixmap = Pixmap::new(width, height)?;
        pixmap.fill(tiny_skia::Color::from_rgba8(BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], BACKGROUND[3]));
        Some(Self { pixmap, clip: None })
    }

    /// The finished image, as the clipboard wants it.
    pub fn into_rgba(self) -> (u32, u32, Vec<u8>) {
        let (width, height) = (self.pixmap.width(), self.pixmap.height());
        (width, height, self.pixmap.take())
    }

    /// Whether `bounds` survives the current clip.
    fn visible(&self, bounds: Bounds<Pixels>) -> bool {
        match self.clip {
            None => true,
            Some(clip) => {
                bounds.origin.x < clip.origin.x + clip.size.width
                    && bounds.origin.x + bounds.size.width > clip.origin.x
                    && bounds.origin.y < clip.origin.y + clip.size.height
                    && bounds.origin.y + bounds.size.height > clip.origin.y
            }
        }
    }

    /// Lays `text` out, one glyph at a time, in whatever face covers each
    /// character.
    ///
    /// `draw` false measures without marking the pixmap, which is what
    /// centring a label needs before it is placed.
    fn run_text(&mut self, origin: Point<Pixels>, text: &str, size: f32, color: Hsla, draw: bool) -> f32 {
        let rgba: Rgba = color.into();
        let mut pen = origin.x.as_f32();
        // ab_glyph positions from the baseline; the callers give a top edge.
        let baseline = origin.y.as_f32() + size;

        for ch in text.chars() {
            let Some(id) = faces().face_for(ch) else { continue };
            // Read out of the database rather than borrowed from it, so the
            // pixmap can be written while the glyph is drawn.
            let Some(data) = faces().bytes(id) else { continue };
            let Ok(font) = FontRef::try_from_slice_and_index(&data.0, data.1) else { continue };
            let scaled = font.as_scaled(PxScale::from(size));
            let glyph_id = font.glyph_id(ch);
            let advance = scaled.h_advance(glyph_id);

            if draw {
                let glyph = glyph_id.with_scale_and_position(size, ab_glyph::point(pen, baseline));
                if let Some(outline) = font.outline_glyph(glyph) {
                    let bounds = outline.px_bounds();
                    outline.draw(|x, y, coverage| {
                        let px_x = bounds.min.x + x as f32;
                        let px_y = bounds.min.y + y as f32;
                        blend(&mut self.pixmap, px_x, px_y, rgba, coverage);
                    });
                }
            }
            pen += advance;
        }
        pen - origin.x.as_f32()
    }
}

impl PlotCanvas for ImageCanvas {
    fn fill(&mut self, bounds: Bounds<Pixels>, color: Hsla, radius: Pixels) {
        if !self.visible(bounds) {
            return;
        }
        let Some(rect) = Rect::from_xywh(
            bounds.origin.x.as_f32(),
            bounds.origin.y.as_f32(),
            bounds.size.width.as_f32().max(0.1),
            bounds.size.height.as_f32().max(0.1),
        ) else {
            return;
        };
        let rgba: Rgba = color.into();
        // A rounded corner on a plot is a point marker or a legend swatch,
        // both small enough that a path is cheaper than it looks.
        if radius > px(0.) {
            let mut path = PathBuilder::new();
            path.push_oval(rect);
            if let Some(path) = path.finish() {
                self.pixmap.fill_path(&path, &solid(rgba), tiny_skia::FillRule::Winding, Transform::identity(), None);
            }
            return;
        }
        self.pixmap.fill_rect(rect, &solid(rgba), Transform::identity(), None);
    }

    fn polyline(&mut self, points: &[Point<Pixels>], width: f32, color: Hsla) {
        if points.len() < 2 {
            return;
        }
        let mut path = PathBuilder::new();
        path.move_to(points[0].x.as_f32(), points[0].y.as_f32());
        for position in &points[1..] {
            path.line_to(position.x.as_f32(), position.y.as_f32());
        }
        let Some(path) = path.finish() else { return };
        let rgba: Rgba = color.into();
        let stroke = Stroke { width, ..Stroke::default() };
        let clip = self.clip.and_then(|area| {
            let rect = Rect::from_xywh(
                area.origin.x.as_f32(),
                area.origin.y.as_f32(),
                area.size.width.as_f32(),
                area.size.height.as_f32(),
            )?;
            let mut mask = tiny_skia::Mask::new(self.pixmap.width(), self.pixmap.height())?;
            let mut rect_path = PathBuilder::new();
            rect_path.push_rect(rect);
            mask.fill_path(&rect_path.finish()?, tiny_skia::FillRule::Winding, true, Transform::identity());
            Some(mask)
        });
        self.pixmap.stroke_path(&path, &solid(rgba), &stroke, Transform::identity(), clip.as_ref());
    }

    fn measure(&mut self, text: &str, size: f32) -> Pixels {
        px(self.run_text(Point::default(), text, size, gpui_kit::black(), false))
    }

    fn text(&mut self, origin: Point<Pixels>, text: &str, size: f32, color: Hsla) {
        self.run_text(origin, text, size, color, true);
    }

    fn clipped(&mut self, area: Bounds<Pixels>, body: &mut dyn FnMut(&mut dyn PlotCanvas)) {
        let previous = self.clip.replace(area);
        body(self);
        self.clip = previous;
    }
}

fn solid(rgba: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(tiny_skia::Color::from_rgba(rgba.r, rgba.g, rgba.b, rgba.a).unwrap_or(tiny_skia::Color::BLACK));
    paint.anti_alias = true;
    paint
}

/// Mixes one glyph pixel into the image at `coverage`.
fn blend(pixmap: &mut Pixmap, x: f32, y: f32, color: Rgba, coverage: f32) {
    let (x, y) = (x.round() as i32, y.round() as i32);
    if x < 0 || y < 0 || x >= pixmap.width() as i32 || y >= pixmap.height() as i32 {
        return;
    }
    let alpha = (coverage * color.a).clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    let index = (y as usize * pixmap.width() as usize + x as usize) * 4;
    let data = pixmap.data_mut();
    for (channel, value) in [color.r, color.g, color.b].into_iter().enumerate() {
        let existing = data[index + channel] as f32 / 255.0;
        data[index + channel] = ((value * alpha + existing * (1.0 - alpha)) * 255.0) as u8;
    }
    data[index + 3] = 255;
}

/// Draws `plot` into an image of `width` by `height`.
///
/// `None` when no pixmap of that size can be made, which is a zero
/// dimension.
pub fn render(plot: &Plot<'_>, colors: Colors, width: u32, height: u32) -> Option<(u32, u32, Vec<u8>)> {
    let mut canvas = ImageCanvas::new(width, height)?;
    let bounds = Bounds::new(Point::default(), gpui_kit::size(px(width as f32), px(height as f32)));
    draw(plot, bounds, colors, &mut canvas);
    Some(canvas.into_rgba())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wows_toolkit_viewmodel::stats::chart::ChartSeries;
    use wows_toolkit_viewmodel::stats::chart::Rgb;
    use wows_toolkit_viewmodel::stats::chart::SeriesPoint;

    fn colors() -> Colors {
        let grey = Hsla { h: 0., s: 0., l: 0.6, a: 1. };
        Colors { axis: grey, grid: grey, text: grey }
    }

    fn series() -> Vec<ChartSeries> {
        vec![ChartSeries {
            name: "Yamato".to_string(),
            color: Rgb { r: 200, g: 80, b: 80 },
            points: (0..5)
                .map(|index| SeriesPoint { label: index.to_string(), value: index as f64 * 1000.0 })
                .collect(),
        }]
    }

    /// The pixmap is drawn on, not handed back blank: a copied chart that is
    /// all background would paste as an empty rectangle.
    #[test]
    fn a_plot_marks_the_image_it_is_drawn_into() {
        let series = series();
        let plot = Plot {
            series: &series,
            bars: &[],
            x_label: "Games",
            y_label: "Damage",
            show_values: false,
            view: Default::default(),
        };

        let (width, height, pixels) = render(&plot, colors(), 400, 300).expect("a pixmap of this size is made");

        assert_eq!((width, height), (400, 300));
        assert_eq!(pixels.len(), 400 * 300 * 4);
        let drawn = pixels.chunks_exact(4).filter(|px| px[..3] != BACKGROUND[..3]).count();
        assert!(drawn > 0, "the plot left nothing on the image");
    }

    #[test]
    fn a_zero_sized_image_is_refused_rather_than_drawn_into() {
        let series = series();
        let plot = Plot {
            series: &series,
            bars: &[],
            x_label: "Games",
            y_label: "Damage",
            show_values: false,
            view: Default::default(),
        };

        assert!(render(&plot, colors(), 0, 300).is_none());
    }

    /// Every label is laid out per character against the faces installed, so
    /// a non-Latin one measures as something rather than collapsing to zero.
    /// This is what bundling a Latin-only font would have broken.
    #[test]
    fn a_non_latin_label_measures_wider_than_nothing() {
        let Some(mut canvas) = ImageCanvas::new(64, 64) else { return };

        let latin = canvas.measure("Yamato", 11.0);
        let japanese = canvas.measure("大和", 11.0);

        assert!(latin > px(0.), "a Latin label has width");
        // Skipped rather than failed where the host installs no CJK face:
        // that is the machine's font set, not this code's behaviour.
        if super::faces().face_for('大').is_some() {
            assert!(japanese > px(0.), "a Japanese label has width too");
        }
    }
}
