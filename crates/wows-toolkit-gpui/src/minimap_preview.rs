//! The minimap hover preview: a replay's battle played back as a small
//! animation while the pointer rests on its row.
//!
//! When the preview appears is [`wows_toolkit_viewmodel::preview_dwell`],
//! shared with the egui app so both wait the same amount. What it draws is
//! [`wows_minimap_renderer::preview`], the same command set the egui popup
//! paints and the video export encodes, rasterised to an image here because
//! this front end has no painter of its own.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use image::RgbImage;
use wows_battle_world::ids::ShotTracking;
use wows_battle_world::merged::MergedReplays;
use wows_minimap_renderer::assets;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::frame_track::SNAPSHOTS_PER_SECOND;
use wows_minimap_renderer::frame_track::TrackSink;
use wows_minimap_renderer::frame_track::bake_options;
use wows_minimap_renderer::frame_track::build_frame_track;
use wows_minimap_renderer::preview::PreviewRenderer;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::vfs::VfsPath;

/// How long each baked frame is shown. The bake keeps an evenly spaced subset
/// of the battle, so this is a display rate rather than the replay's own
/// clock.
pub const FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(80);

/// Why a preview could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error("the replay could not be read")]
    UnreadableReplay,
    #[error("the replay reports an unreadable client version {raw:?}")]
    UnknownBuild { raw: String },
    #[error("this replay's build is not loaded: {reason}")]
    NoGameData { reason: String },
    #[error("the build ships no map info for {map:?}")]
    NoMapInfo { map: String },
    #[error("the preview was superseded")]
    Cancelled,
    #[error(transparent)]
    Render(#[from] wows_minimap_renderer::preview::PreviewRenderError),
}

/// Reads the replay at `path` and bakes its preview against its own build.
///
/// The build is loaded through the shared cache, so a replay from a build
/// already open costs nothing extra and one from a build that is not
/// installed simply has no preview.
pub fn bake_from_file(
    path: &std::path::Path,
    game_data: &crate::replay_inspector::GameDataCache,
    cancel: &AtomicBool,
) -> Result<PreviewFrames, PreviewError> {
    let replay = ReplayFile::from_file(path).map_err(|_| PreviewError::UnreadableReplay)?;
    let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe)
        .ok_or_else(|| PreviewError::UnknownBuild { raw: replay.meta.clientVersionFromExe.clone() })?;

    // A replay whose header carries no build number names no build to load.
    let build =
        version.build.ok_or_else(|| PreviewError::UnknownBuild { raw: replay.meta.clientVersionFromExe.clone() })?;
    let loaded =
        game_data.get_or_load_build(build.get()).map_err(|err| PreviewError::NoGameData { reason: err.to_string() })?;

    bake(&replay, loaded.provider(), loaded.base_constants(), loaded.vfs(), Some(&version), cancel)
}

/// Bakes `replay` into the frames a preview plays, then rasterises them.
///
/// One forward pass over the battle, sampled by the shared [`TrackSink`], so
/// the port shows the same track the egui app does. `cancel` is checked
/// between the phases that cost anything: a preview the pointer has already
/// left should not finish parsing a replay.
pub fn bake(
    replay: &ReplayFile,
    provider: &GameMetadataProvider,
    constants: &GameConstants,
    vfs: &VfsPath,
    version: Option<&Version>,
    cancel: &AtomicBool,
) -> Result<PreviewFrames, PreviewError> {
    let cancelled = || cancel.load(std::sync::atomic::Ordering::Relaxed);
    if cancelled() {
        return Err(PreviewError::Cancelled);
    }

    let map_name = replay.meta.mapName.clone();
    let map_info =
        assets::load_map_info(&map_name, vfs).ok_or_else(|| PreviewError::NoMapInfo { map: map_name.clone() })?;

    let session_version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
    let mut renderer = MinimapRenderer::new(Some(map_info), provider, session_version, bake_options());
    renderer.set_fonts(assets::load_game_fonts(vfs));

    let mut session = MergedReplays::new(provider.entity_specs(), provider, constants, session_version, replay, &[])
        .map_err(|_| PreviewError::UnreadableReplay)?;
    // Tracked, not Untracked: the tracer commands come from `active_shots()`,
    // which stays empty unless shot recording is on.
    session.world_mut().set_shot_tracking(ShotTracking::Tracked);

    let mut sink = TrackSink::new();
    build_frame_track(&mut session, &mut renderer, 1.0 / SNAPSHOTS_PER_SECOND, cancel, &mut sink);
    session.finish();

    if cancelled() {
        return Err(PreviewError::Cancelled);
    }

    let mut renderer = PreviewRenderer::new(vfs, version, &map_name)?;
    Ok(PreviewFrames::render(&mut renderer, &sink.finish()))
}

/// A rendered preview: every frame as an image gpui can draw.
pub struct PreviewFrames {
    frames: Vec<Arc<gpui_kit::Image>>,
}

impl PreviewFrames {
    /// Rasterises every frame of a baked track.
    ///
    /// Done once per preview rather than per paint: the frames are small and
    /// few, and re-rasterising on every frame of the *UI* would redraw the
    /// whole map for each one.
    pub fn render(renderer: &mut PreviewRenderer, track: &[Vec<DrawCommand>]) -> Self {
        let frames = track.iter().map(|commands| Arc::new(to_image(renderer.render(commands)))).collect();
        Self { frames }
    }

    /// How many frames the bake produced. Only the tests and the Search
    /// tab's own test accessor read this back; what plays is decided by
    /// [`Self::at`].
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// The frame to show `elapsed` into the preview, looping.
    ///
    /// `None` only when there are no frames at all, which is a track that
    /// baked nothing.
    pub fn at(&self, elapsed: std::time::Duration) -> Option<Arc<gpui_kit::Image>> {
        if self.frames.is_empty() {
            return None;
        }
        let step = (elapsed.as_millis() / FRAME_INTERVAL.as_millis().max(1)) as usize;
        self.frames.get(step % self.frames.len()).cloned()
    }
}

/// An RGB frame as a gpui image.
///
/// gpui draws RGBA, so the alpha channel is added here; the renderer composes
/// every frame over opaque map art, so it is fully opaque by construction.
fn to_image(frame: RgbImage) -> gpui_kit::Image {
    let (width, height) = frame.dimensions();
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for pixel in frame.pixels() {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
    gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, encode_png(width, height, &rgba))
}

/// gpui takes encoded image bytes, so the raw frame is wrapped in a PNG.
///
/// Lossless and cheap at preview sizes; the alternative is an uncompressed
/// surface type this crate would have to keep in step with gpui's own.
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(std::io::Cursor::new(&mut out));
    // The buffer was built from the frame's own dimensions, so the only way
    // this fails is an allocation failure, which is not something a preview
    // can do anything about.
    let _ = image::ImageEncoder::write_image(encoder, rgba, width, height, image::ExtendedColorType::Rgba8);
    out
}

#[cfg(test)]
mod tests {
    use super::FRAME_INTERVAL;
    use super::PreviewFrames;
    use std::sync::Arc;
    use std::time::Duration;

    fn frames(count: usize) -> PreviewFrames {
        // The images themselves are irrelevant to the timing rule; a one
        // pixel PNG stands in for a rendered frame.
        let pixel = super::encode_png(1, 1, &[0, 0, 0, 255]);
        PreviewFrames {
            frames: (0..count)
                .map(|_| Arc::new(gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, pixel.clone())))
                .collect(),
        }
    }

    #[test]
    fn a_track_with_no_frames_shows_nothing() {
        let empty = frames(0);
        assert_eq!(empty.len(), 0);
        assert!(empty.at(Duration::ZERO).is_none());
    }

    /// Each frame is held for the display interval, and the track loops
    /// rather than stopping on its last frame.
    #[test]
    fn frames_advance_on_the_interval_and_then_loop() {
        let track = frames(3);
        let shown = |elapsed: Duration| {
            let frame = track.at(elapsed).expect("a frame");
            track.frames.iter().position(|candidate| Arc::ptr_eq(candidate, &frame)).expect("one of ours")
        };

        assert_eq!(shown(Duration::ZERO), 0);
        assert_eq!(shown(FRAME_INTERVAL / 2), 0, "a frame is held for the whole interval");
        assert_eq!(shown(FRAME_INTERVAL), 1);
        assert_eq!(shown(FRAME_INTERVAL * 2), 2);
        assert_eq!(shown(FRAME_INTERVAL * 3), 0, "and it loops");
        assert_eq!(shown(FRAME_INTERVAL * 7), 1);
    }
}
