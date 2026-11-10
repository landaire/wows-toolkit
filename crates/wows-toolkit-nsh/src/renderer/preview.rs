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
use wows_toolkit_replay_data::load::GameDataCache;
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
use crate::frames::SurfaceInput;
use crate::renderer::ids::RendererId;
use crate::renderer::ids::Revision;
use crate::renderer::raster;

/// What a preview occupies before its first frame arrives: the ocean's water,
/// so a preview that starts as open water and becomes the map does not jump.
const MAP_PLACEHOLDER: [u8; 3] = [0x06, 0x27, 0x2e];

/// How often the dwell is looked at while it counts down, so a preview the
/// pointer has left stops promptly.
const DWELL_STEP: Duration = Duration::from_millis(25);

/// The preview, as the view shows it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct PreviewView {
    pub id: RendererId,
    pub surface: String,
    /// Changes whenever anything here does.
    pub revision: Revision,
    /// Whether there is game data to read the replay with. Without it no
    /// preview comes, and no space is held for one.
    pub has_game_data: bool,
    /// Whether a preview is on its way: the dwell is counting down or a bake
    /// runs. The map's space is held for the whole of it.
    pub expecting: bool,
    /// Whether the surface shows a map or a frame yet.
    pub drawn: bool,
    /// Why there is no preview, once a bake has said so.
    pub failure: Option<String>,
}

/// How the preview is getting on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Progress {
    expecting: bool,
    drawn: bool,
    failure: Option<String>,
}

/// One hover preview and the thread playing it.
pub struct Preview {
    id: RendererId,
    surface: Arc<Surface>,
    /// Whether there was game data when the preview opened, and so whether a
    /// thread plays it.
    has_game_data: bool,
    progress: watch::Sender<(Revision, Progress)>,
    cancel: Arc<AtomicBool>,
}

/// The frames baked so far, and how the bake is doing.
#[derive(Default)]
struct Baked {
    /// The still map, standing in for a track that has not arrived.
    map: Option<Arc<RgbImage>>,
    track: Option<PreviewFrames<Arc<RgbImage>>>,
    done: bool,
    /// Why the bake gave no preview, in the reader's language.
    failure: Option<String>,
}

/// Which image a preview puts up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Still {
    /// The map alone, before the track arrives.
    Map,
    /// Frame `index` of the track.
    Frame(usize),
}

/// What the surface last showed, and at what size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    still: Still,
    size: PixelSize,
}

impl Preview {
    /// Starts the preview of `path` on the surface named `name`. Without game
    /// data nothing is waited for or read.
    pub fn open(id: RendererId, name: &str, path: PathBuf) -> Arc<Self> {
        let surface = frames::surface(&SurfaceId::new(name));
        let game_data = crate::game_data::cache();
        let has_game_data = game_data.is_some();
        let progress = Progress { expecting: has_game_data, ..Progress::default() };
        let preview = Arc::new(Self {
            id,
            surface,
            has_game_data,
            progress: watch::channel((Revision::default(), progress)).0,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        let Some(game_data) = game_data else { return preview };

        let playing = Arc::clone(&preview);
        std::thread::Builder::new()
            .name("minimap-preview".to_owned())
            .spawn(move || playing.run(&path, game_data))
            .expect("the preview thread starts");

        preview
    }

    /// The pointer left: the bake stops and the surface is let go.
    pub fn close(&self) {
        self.cancel.store(true, Ordering::Relaxed);

        // A view still waiting for a change is let go too.
        self.progress.send_modify(|(revision, _)| *revision = revision.next());

        // No thread runs to let the surface go.
        if !self.has_game_data {
            self.finish();
        }
    }

    pub fn view(&self) -> PreviewView {
        let (revision, progress) = self.progress.borrow().clone();
        PreviewView {
            id: self.id,
            surface: self.surface.id().as_str().to_owned(),
            revision,
            has_game_data: self.has_game_data,
            expecting: progress.expecting,
            drawn: progress.drawn,
            failure: progress.failure,
        }
    }

    /// Resolves with the preview once it differs from `after`.
    pub async fn next_change(&self, after: Revision) -> PreviewView {
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
            *revision = revision.next();
            true
        });
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn run(self: Arc<Self>, path: &Path, game_data: GameDataCache) {
        // The view's own input means nothing to a preview, but it is read so
        // it does not pile up while the pointer moves over the map.
        let mut input = self.surface.take_input();
        let drain = |input: &mut Option<tokio::sync::mpsc::UnboundedReceiver<SurfaceInput>>| {
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
        // A view not yet sized is taken as one device pixel per point; the
        // frames are fitted to whatever size it then reports.
        let edge = PreviewEdge::for_scale_factor(self.surface.view_size().map_or(1.0, |view| view.scale.get()));
        self.bake(path, game_data, edge, &baked);

        let mut shown: Option<Shown> = None;
        let mut started: Option<Instant> = None;
        while !self.cancelled() {
            drain(&mut input);
            let (frame, progress) = {
                let baked = baked.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let frame = match baked.track.as_ref().filter(|track| !track.is_empty()) {
                    Some(track) => {
                        let since = *started.get_or_insert_with(Instant::now);
                        track
                            .index_at(since.elapsed())
                            .and_then(|index| Some((Still::Frame(index), track.at(since.elapsed())?)))
                    }
                    None => baked.map.clone().map(|map| (Still::Map, map)),
                };
                let progress = Progress {
                    expecting: !baked.done || frame.is_some(),
                    drawn: shown.is_some(),
                    failure: baked.failure.clone(),
                };
                if baked.done && baked.track.is_none() && baked.map.is_none() {
                    self.tell(progress);
                    break;
                }
                (frame, progress)
            };

            if let Some((still, image)) = frame
                && let Some(view) = self.surface.view_size()
                && shown != Some(Shown { still, size: view.size })
            {
                match present(&image, view.size) {
                    Ok(frame) => {
                        self.surface.present(frame);
                        shown = Some(Shown { still, size: view.size });
                    }
                    Err(err) => log::warn!("minimap preview: a frame could not be built: {err}"),
                }
            }
            self.tell(Progress { drawn: shown.is_some(), ..progress });
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
    fn bake(self: &Arc<Self>, path: &Path, game_data: GameDataCache, edge: PreviewEdge, baked: &Arc<Mutex<Baked>>) {
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
                },
            );
            let failure = match &outcome {
                Ok(()) => None,
                Err(err) => {
                    log::debug!("no preview for {}: {err}", path.display());
                    said(err)
                }
            };
            let mut baked = baked.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            baked.done = true;
            baked.failure = failure;
            // Looping only starts once the whole track is here.
            if let Some(track) = baked.track.as_mut() {
                track.finish();
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
