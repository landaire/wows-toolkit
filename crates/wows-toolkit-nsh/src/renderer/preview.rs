//! The minimap hover preview: a replay's battle played back as a small
//! animation while the pointer rests on its row.
//!
//! A preview opens when the view mounts it over a hovered row and closes when
//! the pointer leaves. It waits the shared dwell before reading anything, puts
//! the map up as soon as there is art for it, and plays the frames at the
//! preview rate as they are rasterised, looping once all of them are here.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use image::RgbImage;
use nsh_core::prelude::*;
use rust_i18n::t;
use tokio::sync::watch;
use wows_toolkit_replay_data::minimap;
use wows_toolkit_replay_data::minimap::FRAME_INTERVAL;
use wows_toolkit_replay_data::minimap::PreviewEdge;
use wows_toolkit_replay_data::minimap::PreviewError;
use wows_toolkit_replay_data::minimap::PreviewFrames;
use wows_toolkit_viewmodel::preview_dwell::DWELL;
use wows_toolkit_viewmodel::preview_dwell::Dwell;

use crate::frames;
use crate::frames::Frame;
use crate::frames::FrameError;
use crate::frames::PixelSize;
use crate::frames::Surface;
use crate::frames::SurfaceId;
use crate::renderer::raster;
use crate::renderer::services;

/// How long a preview goes without a new frame before the bake behind it
/// reads as stuck rather than busy. Comfortably longer than the gap between
/// frames, so the spinner never flickers over a preview that is merely slow.
const STALLED_AFTER: Duration = Duration::from_millis(1500);

/// What a preview occupies before its first frame arrives: the ocean's water,
/// so a preview that starts as open water and becomes the map does not jump.
const MAP_PLACEHOLDER: [u8; 3] = [0x06, 0x27, 0x2e];

/// How often the dwell is looked at while it counts down, so a preview the
/// pointer has left stops promptly.
const DWELL_STEP: Duration = Duration::from_millis(25);

/// The preview, as the view shows it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct PreviewView {
    pub id: u32,
    pub surface: String,
    /// Changes whenever anything here does.
    pub revision: u32,
    /// Whether a preview is on its way: the dwell is counting down or a bake
    /// runs. The map's space is held for the whole of it.
    pub expecting: bool,
    /// Whether the bake has stopped feeding the preview, which is what a
    /// spinner over it says.
    pub stalled: bool,
    /// Why there is no preview, once a bake has said so.
    pub failure: Option<String>,
}

/// How the preview is getting on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Progress {
    expecting: bool,
    stalled: bool,
    failure: Option<String>,
}

/// One hover preview and the thread playing it.
pub struct Preview {
    id: u32,
    surface: Arc<Surface>,
    progress: watch::Sender<(u32, Progress)>,
    cancel: Arc<AtomicBool>,
}

/// The frames baked so far, and how the bake is doing.
#[derive(Default)]
struct Baked {
    /// The still map, standing in for a track that has not arrived.
    map: Option<Arc<RgbImage>>,
    track: Option<PreviewFrames<Arc<RgbImage>>>,
    last_frame: Option<Instant>,
    done: bool,
}

impl Preview {
    /// Starts the preview of `path` on the surface named `name`.
    pub fn open(id: u32, name: &str, path: PathBuf) -> Arc<Self> {
        let surface = frames::surface(&SurfaceId::new(name));
        let preview = Arc::new(Self {
            id,
            surface,
            progress: watch::channel((0, Progress { expecting: true, ..Progress::default() })).0,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        let playing = Arc::clone(&preview);
        std::thread::Builder::new()
            .name("minimap-preview".to_owned())
            .spawn(move || playing.run(&path))
            .expect("the preview thread starts");

        preview
    }

    /// The pointer left: the bake stops and the surface is let go.
    pub fn close(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn view(&self) -> PreviewView {
        let (revision, progress) = self.progress.borrow().clone();
        PreviewView {
            id: self.id,
            surface: self.surface.id().as_str().to_owned(),
            revision,
            expecting: progress.expecting,
            stalled: progress.stalled,
            failure: progress.failure,
        }
    }

    /// Resolves with the preview once it differs from `after`.
    pub async fn next_change(&self, after: u32) -> PreviewView {
        let mut changes = self.progress.subscribe();
        let _ = changes.wait_for(|(revision, _)| *revision != after).await;
        self.view()
    }

    fn tell(&self, progress: Progress) {
        self.progress.send_if_modified(|(revision, held)| {
            if *held == progress {
                return false;
            }
            *held = progress;
            *revision = revision.wrapping_add(1);
            true
        });
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn run(self: Arc<Self>, path: &Path) {
        // The view's own input means nothing to a preview, but it is read so
        // it does not pile up while the pointer moves over the map.
        let mut input = self.surface.take_input();
        let drain = |input: &mut Option<tokio::sync::mpsc::UnboundedReceiver<crate::frames::SurfaceInput>>| {
            if let Some(input) = input.as_mut() {
                while input.try_recv().is_ok() {}
            }
        };

        // A battle walked recently plays without the dwell: what the dwell
        // holds back is the walk.
        if !minimap::is_walked(path) && !self.dwell(path, || drain(&mut input)) {
            self.finish();
            return;
        }

        let baked = Arc::new(Mutex::new(Baked::default()));
        let edge = PreviewEdge::for_scale_factor(self.surface.view_size().map_or(1.0, |(_, scale)| scale));
        self.bake(path, edge, &baked);

        let mut shown: Option<(usize, PixelSize)> = None;
        let mut started: Option<Instant> = None;
        while !self.cancelled() {
            drain(&mut input);
            let (frame, progress) = {
                let baked = baked.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let frame = match baked.track.as_ref().filter(|track| !track.is_empty()) {
                    Some(track) => {
                        let since = *started.get_or_insert_with(Instant::now);
                        track.index_at(since.elapsed()).and_then(|index| Some((index + 1, track.at(since.elapsed())?)))
                    }
                    None => baked.map.clone().map(|map| (0, map)),
                };
                let stalled = !baked.done
                    && match (&baked.track, baked.last_frame) {
                        (Some(_), Some(last)) => last.elapsed() > STALLED_AFTER,
                        _ => true,
                    };
                let progress = Progress { expecting: !baked.done || frame.is_some(), stalled, failure: None };
                if baked.done && baked.track.is_none() && baked.map.is_none() {
                    break;
                }
                (frame, progress)
            };
            self.tell(Progress { failure: self.progress.borrow().1.failure.clone(), ..progress });

            if let Some((index, image)) = frame
                && let Some((size, _)) = self.surface.view_size()
                && shown != Some((index, size))
            {
                match present(&image, size) {
                    Ok(frame) => {
                        self.surface.present(frame);
                        shown = Some((index, size));
                    }
                    Err(err) => log::warn!("minimap preview: a frame could not be built: {err}"),
                }
            }
            std::thread::sleep(FRAME_INTERVAL);
        }

        self.finish();
    }

    /// Waits out the shared dwell, so a row the pointer only crosses is not
    /// read. `false` when the preview closed first.
    fn dwell(&self, path: &Path, mut between: impl FnMut()) -> bool {
        let mut dwell = Dwell::new();
        let started = Instant::now();
        let mut last = started;
        while !self.cancelled() {
            let now = Instant::now();
            dwell.hover(path.to_owned(), now - last);
            last = now;
            if dwell.pending_request().as_deref() == Some(path) {
                return true;
            }
            between();
            std::thread::sleep(DWELL_STEP.min(DWELL));
        }

        false
    }

    /// Bakes the preview on its own thread. The map goes up first, from a
    /// build already open, while the bake may still be loading the one the
    /// replay was recorded on.
    fn bake(self: &Arc<Self>, path: &Path, edge: PreviewEdge, baked: &Arc<Mutex<Baked>>) {
        let game_data = services::game_data();
        if let Ok(meta) = wows_replays::ReplayFile::meta_from_file(path)
            && let Some(map) = minimap::map_frame(&meta.mapName, &game_data, edge)
        {
            baked.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).map = Some(Arc::new(map));
        }

        let preview = Arc::clone(self);
        let path = path.to_owned();
        let baked = Arc::clone(baked);
        std::thread::spawn(move || {
            let outcome = minimap::bake_from_file(
                &path,
                &game_data,
                edge,
                &preview.cancel,
                |map| {
                    let mut baked = baked.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    // The first map put up stays: replacing a map with another
                    // build's art of the same map would only stutter.
                    if baked.map.is_none() {
                        baked.map = Some(Arc::new(map));
                    }
                },
                |frame| {
                    let mut baked = baked.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    baked.track.get_or_insert_with(PreviewFrames::streaming).push(Arc::new(frame));
                    baked.last_frame = Some(Instant::now());
                },
            );
            let failure = match &outcome {
                Ok(()) => None,
                Err(err) => {
                    log::debug!("no preview for {}: {err}", path.display());
                    said(err)
                }
            };
            {
                let mut baked = baked.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                baked.done = true;
                // Looping only starts once the whole track is here.
                if let Some(track) = baked.track.as_mut() {
                    track.finish();
                }
            }
            if let Some(failure) = failure {
                preview.tell(Progress { expecting: false, stalled: false, failure: Some(failure) });
            }
        });
    }

    fn finish(&self) {
        if self.cancelled() {
            self.surface.clear();
            frames::release(self.surface.id());
        }
    }
}

/// What a surface says instead of a map. `None` for a superseded bake.
fn said(error: &PreviewError) -> Option<String> {
    let message = error.message()?;
    Some(t!(message.key, value = message.value).into_owned())
}

/// A square preview image fitted into the view over open water.
fn present(image: &RgbImage, size: PixelSize) -> Result<Frame, FrameError> {
    raster::finish(raster::fit(image, size, MAP_PLACEHOLDER)?, size)
}
