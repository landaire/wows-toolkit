//! Drawing a plot without a window, so a chart can be copied as a picture.
//!
//! The plot goes through the same [`PlotCanvas`] a window uses, so the two agree on where every
//! tick, bar and label goes. The glyphs come from the system's own fonts through `fontdb`.
//! Nothing is bundled: a chart in Japanese or Russian is drawn with the faces the desktop already
//! has, rather than tofu from a Latin-only file.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;

use ab_glyph::Font as _;
use ab_glyph::FontRef;
use ab_glyph::PxScale;
use ab_glyph::ScaleFont as _;
use tiny_skia::Paint;
use tiny_skia::PathBuilder;
use tiny_skia::Pixmap;
use tiny_skia::Rect;
use tiny_skia::Stroke;
use tiny_skia::Transform;

use super::Plot;
use super::PlotCanvas;
use super::PlotColor;
use super::PlotColors;
use super::PlotPoint;
use super::PlotRect;
use super::draw;

/// What a copied chart is drawn on when the caller has no theme surface to name.
pub const DEFAULT_BACKGROUND: PlotColor = PlotColor::new(24.0 / 255.0, 24.0 / 255.0, 27.0 / 255.0, 1.0);

/// A face's bytes and which face inside them, which is what `ab_glyph` reads a glyph out of.
type FaceBytes = Arc<(Vec<u8>, u32)>;

/// A finished image, as the clipboard wants it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlotImage {
    pub width: u32,
    pub height: u32,
    /// Row-major, non-premultiplied RGBA, four bytes per pixel.
    pub rgba: Vec<u8>,
}

/// Draws a plot into an image.
pub struct ImageCanvas {
    pixmap: Pixmap,
    /// What the current [`clipped`](PlotCanvas::clipped) confines drawing to.
    clip: Option<PlotRect>,
}

impl ImageCanvas {
    /// An empty canvas of `width` by `height`, filled with `background`.
    ///
    /// `None` for a size no pixmap can be made at, which is a zero dimension.
    pub fn new(width: u32, height: u32, background: PlotColor) -> Option<Self> {
        let mut pixmap = Pixmap::new(width, height)?;
        pixmap.fill(skia_color(background));
        Some(Self { pixmap, clip: None })
    }

    pub fn into_image(self) -> PlotImage {
        let (width, height) = (self.pixmap.width(), self.pixmap.height());
        PlotImage { width, height, rgba: self.pixmap.take() }
    }

    /// Whether `bounds` survives the current clip.
    fn visible(&self, bounds: PlotRect) -> bool {
        match self.clip {
            None => true,
            Some(clip) => {
                bounds.x < clip.right()
                    && bounds.right() > clip.x
                    && bounds.y < clip.bottom()
                    && bounds.bottom() > clip.y
            }
        }
    }

    /// Lays `text` out, one glyph at a time, in whatever face covers each character.
    ///
    /// `draw` false measures without marking the pixmap, which is what centring a label needs
    /// before it is placed.
    fn run_text(&mut self, origin: PlotPoint, text: &str, size: f32, color: PlotColor, draw: bool) -> f32 {
        let mut pen = origin.x;
        // ab_glyph positions from the baseline; the callers give a top edge.
        let baseline = origin.y + size;

        for ch in text.chars() {
            let Some(id) = FACES.face_for(ch) else { continue };
            // Read out of the database rather than borrowed from it, so the pixmap can be written
            // while the glyph is drawn.
            let Some(data) = FACES.bytes(id) else { continue };
            let Ok(font) = FontRef::try_from_slice_and_index(&data.0, data.1) else { continue };
            let scaled = font.as_scaled(PxScale::from(size));
            let glyph_id = font.glyph_id(ch);
            let advance = scaled.h_advance(glyph_id);

            if draw {
                let glyph = glyph_id.with_scale_and_position(size, ab_glyph::point(pen, baseline));
                if let Some(outline) = font.outline_glyph(glyph) {
                    let bounds = outline.px_bounds();
                    outline.draw(|x, y, coverage| {
                        blend(&mut self.pixmap, bounds.min.x + x as f32, bounds.min.y + y as f32, color, coverage);
                    });
                }
            }
            pen += advance;
        }

        pen - origin.x
    }
}

impl PlotCanvas for ImageCanvas {
    fn fill(&mut self, bounds: PlotRect, color: PlotColor, radius: f32) {
        if !self.visible(bounds) {
            return;
        }
        let Some(rect) = Rect::from_xywh(bounds.x, bounds.y, bounds.width.max(0.1), bounds.height.max(0.1)) else {
            return;
        };
        // A rounded corner on a plot is a point marker or a legend swatch, both small enough that
        // a path is cheaper than it looks.
        if radius > 0.0 {
            let mut path = PathBuilder::new();
            path.push_oval(rect);
            if let Some(path) = path.finish() {
                self.pixmap.fill_path(&path, &solid(color), tiny_skia::FillRule::Winding, Transform::identity(), None);
            }
            return;
        }
        self.pixmap.fill_rect(rect, &solid(color), Transform::identity(), None);
    }

    fn polyline(&mut self, points: &[PlotPoint], width: f32, color: PlotColor) {
        let [first, rest @ ..] = points else { return };
        if rest.is_empty() {
            return;
        }
        let mut path = PathBuilder::new();
        path.move_to(first.x, first.y);
        for position in rest {
            path.line_to(position.x, position.y);
        }
        let Some(path) = path.finish() else { return };
        let stroke = Stroke { width, ..Stroke::default() };
        let clip = self.clip.and_then(|area| {
            let rect = Rect::from_xywh(area.x, area.y, area.width, area.height)?;
            let mut mask = tiny_skia::Mask::new(self.pixmap.width(), self.pixmap.height())?;
            let mut rect_path = PathBuilder::new();
            rect_path.push_rect(rect);
            mask.fill_path(&rect_path.finish()?, tiny_skia::FillRule::Winding, true, Transform::identity());
            Some(mask)
        });
        self.pixmap.stroke_path(&path, &solid(color), &stroke, Transform::identity(), clip.as_ref());
    }

    fn measure(&mut self, text: &str, size: f32) -> f32 {
        self.run_text(PlotPoint::default(), text, size, DEFAULT_BACKGROUND, false)
    }

    fn text(&mut self, origin: PlotPoint, text: &str, size: f32, color: PlotColor) {
        self.run_text(origin, text, size, color, true);
    }

    fn clipped(&mut self, area: PlotRect, body: &mut dyn FnMut(&mut dyn PlotCanvas)) {
        let previous = self.clip.replace(area);
        body(self);
        self.clip = previous;
    }
}

/// Faces the system offers, with what has already been looked up in them.
struct Faces {
    db: fontdb::Database,
    /// The families to try, in order. The first that covers a character wins, which is what puts
    /// a Japanese label in a Japanese face.
    order: Vec<fontdb::ID>,
    /// Which face covers a character. Finding one parses every face ahead of it, so a label of
    /// repeated characters would pay for each of them.
    covering: Mutex<HashMap<char, Option<fontdb::ID>>>,
    /// A face's bytes, kept. `with_face_data` lends a borrow that cannot be held while the pixmap
    /// is written, so the bytes are copied out once.
    bytes: Mutex<HashMap<fontdb::ID, Option<FaceBytes>>>,
}

impl Faces {
    fn load() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        // The desktop's own UI faces first, then everything else as fallback, so a Latin label
        // looks like the app and a CJK one still renders.
        let preferred =
            ["Segoe UI", "Adwaita Sans", "Cantarell", "Helvetica Neue", "Arial", "DejaVu Sans", "Noto Sans"];
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
    /// `None` when nothing installed covers it, which is a character that would show as a blank
    /// box in any application.
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
                    FontRef::try_from_slice_and_index(data, index).is_ok_and(|font| font.glyph_id(ch).0 != 0)
                })
                .unwrap_or(false)
        })
    }
}

/// Draws `plot` into an image of `width` by `height` logical pixels on `background`.
///
/// `None` when no pixmap of that size can be made, which is a zero dimension.
pub fn render(
    plot: &Plot<'_>,
    colors: PlotColors,
    background: PlotColor,
    width: u32,
    height: u32,
) -> Option<PlotImage> {
    let mut canvas = ImageCanvas::new(width, height, background)?;
    let bounds = PlotRect::new(0.0, 0.0, width as f32, height as f32);
    draw(plot, bounds, colors, &mut canvas);

    Some(canvas.into_image())
}

/// The faces the system offers, enumerated once for the process.
///
/// Walking the font directories costs the better part of a second on a desktop with a full set
/// installed, and every canvas would otherwise pay it again.
static FACES: LazyLock<Faces> = LazyLock::new(Faces::load);

fn skia_color(color: PlotColor) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(color.r, color.g, color.b, color.a).unwrap_or(tiny_skia::Color::BLACK)
}

fn solid(color: PlotColor) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(skia_color(color));
    paint.anti_alias = true;
    paint
}

/// Mixes one glyph pixel into the image at `coverage`.
fn blend(pixmap: &mut Pixmap, x: f32, y: f32, color: PlotColor, coverage: f32) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::chart::ChartSeries;
    use crate::stats::chart::Rgb;
    use crate::stats::chart::SeriesPoint;
    use crate::stats::plot::PlotView;

    fn colors() -> PlotColors {
        let grey = PlotColor::new(0.6, 0.6, 0.6, 1.0);
        PlotColors { axis: grey, grid: grey, text: grey }
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

    fn plot(series: &[ChartSeries]) -> Plot<'_> {
        Plot { series, bars: &[], x_label: "Games", y_label: "Damage", show_values: false, view: PlotView::default() }
    }

    /// The pixmap is drawn on, not handed back blank: a copied chart that is all background would
    /// paste as an empty rectangle.
    #[test]
    fn a_plot_marks_the_image_it_is_drawn_into() {
        let series = series();

        let image =
            render(&plot(&series), colors(), DEFAULT_BACKGROUND, 400, 300).expect("a pixmap of this size is made");

        assert_eq!((image.width, image.height), (400, 300));
        assert_eq!(image.rgba.len(), 400 * 300 * 4);
        let background = [24u8, 24, 27];
        let drawn = image.rgba.as_chunks::<4>().0.iter().filter(|px| px[..3] != background).count();
        assert!(drawn > 0, "the plot left nothing on the image");
    }

    #[test]
    fn a_zero_sized_image_is_refused_rather_than_drawn_into() {
        let series = series();

        assert!(render(&plot(&series), colors(), DEFAULT_BACKGROUND, 0, 300).is_none());
    }

    /// Every label is laid out per character against the faces installed, so a non-Latin one
    /// measures as something rather than collapsing to zero.
    #[test]
    fn a_non_latin_label_measures_wider_than_nothing() {
        let Some(mut canvas) = ImageCanvas::new(64, 64, DEFAULT_BACKGROUND) else { return };

        let latin = canvas.measure("Yamato", 11.0);
        let japanese = canvas.measure("\u{5927}\u{548c}", 11.0);

        assert!(latin > 0.0, "a Latin label has width");
        // Skipped rather than failed where the host installs no CJK face: that is the machine's
        // font set, not this code's behaviour.
        if FACES.face_for('\u{5927}').is_some() {
            assert!(japanese > 0.0, "a Japanese label has width too");
        }
    }
}
