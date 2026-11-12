//! The off-thread half of a preview: the still map, then the battle's frames as they are drawn.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use futures::FutureExt as _;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::future::LocalBoxFuture;
use wows_toolkit_replay_data::frame::BgraFrame;
use wows_toolkit_replay_data::minimap_preview;
use wows_toolkit_replay_data::minimap_preview::PreviewEdge;
use wows_toolkit_replay_data::minimap_preview::PreviewError;
use wows_toolkit_replay_data::minimap_preview::PreviewFrames;
use zgui::task::blocking;

use crate::services::GameData;

/// Starts bakes. The game data in the app; a script under test.
pub(crate) trait PreviewBaker {
    /// Whether anything can be baked at all. Without game data there is no preview, and nothing
    /// is waited for.
    fn is_available(&self) -> bool;

    /// Whether `path`'s battle was walked recently, so baking it costs too little to wait for the
    /// pointer to settle.
    fn is_walked(&self, path: &Path) -> bool;

    /// Starts baking off the UI thread. `None` when there is nothing to bake against.
    fn start(&self, request: BakeRequest) -> Option<BakeStreams>;
}

/// What one bake is for.
pub(crate) struct BakeRequest {
    pub path: PathBuf,
    /// The map's name as a build names it, for drawing the still map from a build already open.
    pub map_name: Option<String>,
    /// The size frames are drawn at.
    pub edge: PreviewEdge,
    /// Set when the pointer has moved on.
    pub cancel: Arc<AtomicBool>,
}

/// What a running bake hands back, read in this order.
pub(crate) struct BakeStreams {
    /// The still map, drawn from a build already open. It costs one image decode where the bake
    /// may cost loading a build, so it goes up first.
    pub open_map: LocalBoxFuture<'static, Option<PreviewFrames>>,
    /// The still map from the build the replay was recorded on, for when no build was open or it
    /// had no art for the map.
    pub own_map: oneshot::Receiver<PreviewFrames>,
    /// The battle's frames, as they are drawn.
    pub frames: mpsc::UnboundedReceiver<BgraFrame>,
    pub finished: LocalBoxFuture<'static, Result<(), PreviewError>>,
}

/// Bakes against the game data the process shares.
pub(crate) struct GameDataBaker {
    pub game_data: GameData,
}

impl PreviewBaker for GameDataBaker {
    fn is_available(&self) -> bool {
        self.game_data.cache_untracked().is_some()
    }

    fn is_walked(&self, path: &Path) -> bool {
        minimap_preview::is_walked(path)
    }

    fn start(&self, request: BakeRequest) -> Option<BakeStreams> {
        let cache = self.game_data.cache_untracked()?;
        let BakeRequest { path, map_name, edge, cancel } = request;
        let (map_sender, own_map) = oneshot::channel();
        let (frame_sender, frames) = mpsc::unbounded();

        let baking_cache = cache.clone();
        let finished = blocking(move || {
            minimap_preview::bake_from_file(
                &path,
                &baking_cache,
                edge,
                &cancel,
                move |map| {
                    // The receiver is gone once the pointer has moved on.
                    let _ = map_sender.send(map);
                },
                move |frame| {
                    let _ = frame_sender.unbounded_send(frame);
                },
            )
        });
        let open_map = match map_name {
            Some(name) => blocking(move || minimap_preview::map_frame(&name, &cache, edge)).boxed_local(),
            None => futures::future::ready(None).boxed_local(),
        };

        Some(BakeStreams { open_map, own_map, frames, finished: finished.boxed_local() })
    }
}
