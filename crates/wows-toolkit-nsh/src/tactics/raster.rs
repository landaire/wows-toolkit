//! Drawing a board's frame: the map through the minimap renderer, fitted into
//! the frame surface, with the turning handle over it.

use image::RgbImage;
use tiny_skia::Color;
use tiny_skia::FillRule;
use tiny_skia::FilterQuality;
use tiny_skia::Paint;
use tiny_skia::PathBuilder;
use tiny_skia::Pixmap;
use tiny_skia::PixmapPaint;
use tiny_skia::Stroke;
use tiny_skia::Transform;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_minimap_renderer::preview::PreviewRenderer;
use wows_toolkit_viewmodel::tactics::board;
use wows_toolkit_viewmodel::tactics::board::ElementPos;
use wows_toolkit_viewmodel::tactics::board::Extent;
use wows_toolkit_viewmodel::tactics::board::Letterbox;
use wt_collab_client::ids::BoardId;

use crate::frames::AlphaMode;
use crate::frames::Frame;
use crate::frames::FrameError;
use crate::frames::ScaleFactor;
use crate::tactics::board::HANDLE_RADIUS;
use crate::tactics::board::RasterJob;
use crate::tactics::board::TurnHandle;

/// What the map area is filled with beside a map that does not fill it.
const MARGIN: [u8; 3] = [0x17, 0x17, 0x15];

/// The turning handle's colour, the egui board's own.
const HANDLE_COLOR: [u8; 3] = [0xff, 0xff, 0x64];

/// Which art a kept renderer draws: a map this build ships, or the art a peer
/// sent for one board.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ArtKey {
    Space(String),
    Board(BoardId),
}

/// The one renderer the board keeps, for the map it is on.
///
/// A renderer carries a map's whole art set and this build's icons, so it is
/// built once per map rather than per frame.
#[derive(Default)]
pub struct Rasteriser {
    kept: Option<(ArtKey, PreviewRenderer)>,
}

/// What a raster came to.
pub enum Rastered {
    Map(Frame),
    /// The build ships no art for the map, which the board says rather than
    /// claiming to still be drawing it.
    NoArt,
}

impl Rasteriser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn draw(&mut self, job: &RasterJob) -> Result<Rastered, FrameError> {
        let Some(map) = self.map(job) else { return Ok(Rastered::NoArt) };

        Ok(Rastered::Map(compose(&map, job)?))
    }

    /// The map layer at the minimap's own size, with the board's commands on
    /// it and the HUD strip cropped off.
    fn map(&mut self, job: &RasterJob) -> Option<RgbImage> {
        let key = match &job.art {
            Some(_) => ArtKey::Board(job.board_id),
            None => ArtKey::Space(job.space.clone()),
        };
        if self.kept.as_ref().is_none_or(|(held, _)| *held != key) {
            let vfs = job.game.vfs();
            let renderer = match &job.art {
                Some(art) => PreviewRenderer::with_art(RgbImage::clone(art), vfs, None, SidePanelLayout::None),
                // A map whose art cannot be read is shown as having none.
                None => PreviewRenderer::with_layout(vfs, None, &job.space, SidePanelLayout::None).ok()?,
            };
            self.kept = Some((key, renderer));
        }
        let (_, renderer) = self.kept.as_mut()?;

        let commands = commands(job);
        let canvas = renderer.render_at(job.view, &commands);
        let side = MINIMAP_SIZE.min(canvas.width()).min(canvas.height().saturating_sub(HUD_HEIGHT));

        Some(image::imageops::crop_imm(&canvas, 0, HUD_HEIGHT, side, side).to_image())
    }
}

/// Every command a board frame draws: the zones, the ships' range circles
/// under the markers, and the shapes over the zones.
fn commands(job: &RasterJob) -> Vec<DrawCommand> {
    let mut commands: Vec<DrawCommand> = job.caps.iter().map(|cap| cap.command(&job.map_info)).collect();
    commands.extend(board::ship_ranges(
        &job.annotations,
        job.game.metadata(),
        job.game.version(),
        job.map_info.space_size as f32,
    ));
    commands.extend(job.annotations.iter().flat_map(wt_collab_client::geometry::annotation_commands));

    commands
}

/// The map fitted into the frame without stretching, with the turning handle
/// drawn over it in device pixels.
fn compose(map: &RgbImage, job: &RasterJob) -> Result<Frame, FrameError> {
    let size = job.view_size.size;
    let Some(mut canvas) = Pixmap::new(size.width, size.height) else {
        return Frame::new(size, AlphaMode::Premultiplied, Vec::new());
    };
    canvas.fill(Color::from_rgba8(MARGIN[0], MARGIN[1], MARGIN[2], 0xff));

    let side = map.width() as f32;
    let fit = Letterbox::fitting(
        ElementPos { x: 0.0, y: 0.0 },
        Extent { width: size.width as f32, height: size.height as f32 },
        Extent { width: side, height: side },
    );
    if let (Some(fit), Some(layer)) = (fit, map_pixmap(map)) {
        canvas.draw_pixmap(
            0,
            0,
            layer.as_ref(),
            &PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() },
            Transform::from_scale(fit.scale, fit.scale).post_translate(fit.left, fit.top),
            None,
        );
    }

    if let Some(handle) = job.handle {
        draw_handle(&mut canvas, handle, job.view_size.scale);
    }

    Frame::new(size, AlphaMode::Premultiplied, canvas.take())
}

fn map_pixmap(map: &RgbImage) -> Option<Pixmap> {
    let mut layer = Pixmap::new(map.width(), map.height())?;
    for (pixel, rgb) in layer.data_mut().chunks_exact_mut(4).zip(map.pixels()) {
        pixel.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 0xff]);
    }

    Some(layer)
}

/// The handle a picked shape is turned by, and the stem back to the shape so
/// it reads as belonging to it.
fn draw_handle(canvas: &mut Pixmap, handle: TurnHandle, scale: ScaleFactor) {
    let scale = scale.get();
    let mut paint = Paint::default();
    paint.set_color_rgba8(HANDLE_COLOR[0], HANDLE_COLOR[1], HANDLE_COLOR[2], 0xff);
    paint.anti_alias = true;

    let mut stem = PathBuilder::new();
    stem.move_to(handle.center.x, handle.center.y);
    stem.line_to(handle.anchor.x, handle.anchor.y);
    if let Some(stem) = stem.finish() {
        canvas.stroke_path(&stem, &paint, &Stroke { width: scale.max(1.0), ..Stroke::default() }, Transform::identity(), None);
    }

    if let Some(dot) = PathBuilder::from_circle(handle.center.x, handle.center.y, HANDLE_RADIUS * scale) {
        canvas.fill_path(&dot, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// A map image as a PNG, for sending to a peer. `None` when it cannot be
/// encoded, which leaves the board announced without art.
pub fn encode_png(art: &RgbImage) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    art.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;

    Some(png)
}
