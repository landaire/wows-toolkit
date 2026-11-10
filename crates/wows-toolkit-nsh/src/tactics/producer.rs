//! The thread that owns a board's frame surface: it turns the surface's input
//! into board changes and draws the board whenever it changed.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::mpsc;
use std::time::Duration;

use tokio::sync::watch;
use wows_toolkit_viewmodel::tactics::board::ScanProgress;
use wows_toolkit_viewmodel::tactics::layout_store;
use wt_collab_client::link::CollabLink;

use crate::frames;
use crate::frames::Surface;
use crate::frames::SurfaceId;
use crate::frames::SurfaceInput;
use crate::tactics::board::Announcement;
use crate::tactics::board::Board;
use crate::tactics::board::Frame;
use crate::tactics::board::ScanStart;
use crate::tactics::board::Viewport;
use crate::tactics::raster;
use crate::tactics::raster::Rastered;
use crate::tactics::raster::Rasteriser;

/// How often a board looks at what the session holds while it is in one.
const COLLAB_TICK: Duration = Duration::from_millis(200);

enum Message {
    Input(SurfaceInput),
    /// The board changed from outside the producer and needs drawing.
    Redraw,
}

/// A board, the surface it is drawn on, and how the view hears it changed.
pub struct Shared {
    board: Mutex<Board>,
    surface: Arc<Surface>,
    revision: watch::Sender<u32>,
    wake: mpsc::Sender<Message>,
}

impl Shared {
    /// Starts the producer for `board` on the surface named `id`.
    ///
    /// The surface's input stream is taken here, so one surface has one
    /// producer for the life of the process.
    pub fn start(id: &SurfaceId, board: Board) -> Arc<Self> {
        let surface = frames::surface(id);
        let (wake, messages) = mpsc::channel();
        let (revision, _) = watch::channel(0);
        let shared = Arc::new(Self { board: Mutex::new(board), surface: Arc::clone(&surface), revision, wake });

        if let Some(mut input) = surface.take_input() {
            let forward = shared.wake.clone();
            std::thread::Builder::new()
                .name("tactics-input".to_owned())
                .spawn(move || {
                    while let Some(event) = input.blocking_recv() {
                        if forward.send(Message::Input(event)).is_err() {
                            return;
                        }
                    }
                })
                .expect("the tactics input thread starts");
        }

        let producing = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("tactics-board".to_owned())
            .spawn(move || producing.run(&messages))
            .expect("the tactics producer thread starts");

        shared
    }

    #[cfg(test)]
    pub fn surface(&self) -> &Arc<Surface> {
        &self.surface
    }

    pub fn lock(&self) -> MutexGuard<'_, Board> {
        self.board.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Changes the board, then has it drawn and the view told as the change
    /// asks. Work the change started (an announcement, a walk) runs after.
    pub fn update<R>(self: &Arc<Self>, change: impl FnOnce(&mut Board) -> R) -> R {
        let (result, dirty) = {
            let mut board = self.lock();
            let result = change(&mut board);
            let dirty = board.take_dirty();
            if board.take_changed() {
                self.bump();
            }
            (result, dirty)
        };
        if dirty {
            // The producer stops only with the process.
            let _ = self.wake.send(Message::Redraw);
        }
        result
    }

    /// The revision the view last heard of, and a way to wait for the next.
    pub fn subscribe(&self) -> watch::Receiver<u32> {
        self.revision.subscribe()
    }

    fn bump(&self) {
        self.revision.send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    /// Hands the board its end of a session.
    pub fn set_collab(self: &Arc<Self>, link: CollabLink) {
        let announcement = self.update(|board| board.set_collab(link));
        self.announce(announcement);
        // The producer's wait shortens while a session is followed.
        let _ = self.wake.send(Message::Redraw);
    }

    /// Reads the map's art for a session to draw from, off the caller's
    /// thread, then finishes the announcement.
    pub fn announce(self: &Arc<Self>, announcement: Option<Announcement>) {
        let Some(Announcement { game, map }) = announcement else { return };
        let shared = Arc::clone(self);
        std::thread::spawn(move || {
            let art = wows_minimap_renderer::assets::load_map_image(&map.space, game.vfs());
            let info = wows_minimap_renderer::assets::load_map_info(&map.space, game.vfs());
            let png = art.as_ref().and_then(raster::encode_png);
            shared.update(|board| board.finish_announce(map, png, info));
        });
    }

    /// Walks the replays for the capture layouts nothing has yet, saving each
    /// new one, then hands them to the board.
    pub fn scan(self: &Arc<Self>, start: ScanStart) {
        let shared = Arc::clone(self);
        std::thread::spawn(move || {
            let ScanStart { dir, game, known, cancelled } = start;
            let report = |step: ScanProgress| shared.update(|board| board.scan_step(&cancelled, step));
            let outcome = wows_toolkit_viewmodel::tactics::board::scan_layouts(
                &dir,
                game.metadata(),
                game.constants(),
                known,
                &cancelled,
                report,
            );
            save_layouts(&outcome.fresh);
            let added = outcome.fresh.len();
            shared.update(|board| board.finish_scan(&cancelled, outcome.found, added, outcome.total));
        });
    }

    fn run(self: Arc<Self>, messages: &mpsc::Receiver<Message>) {
        let mut rasteriser = Rasteriser::new();
        loop {
            let following = self.lock().is_following();
            let first = if following {
                match messages.recv_timeout(COLLAB_TICK) {
                    Ok(message) => Some(message),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match messages.recv() {
                    Ok(message) => Some(message),
                    Err(mpsc::RecvError) => return,
                }
            };

            {
                let mut board = self.lock();
                if following {
                    board.follow();
                }
                // Everything waiting is taken before drawing, so a burst of
                // pointer events draws once.
                for message in first.into_iter().chain(messages.try_iter()) {
                    match message {
                        Message::Input(input) => apply(&mut board, input),
                        Message::Redraw => board.redraw_requested(),
                    }
                }
                if board.take_changed() {
                    self.bump();
                }
            }

            self.draw(&mut rasteriser);
        }
    }

    /// Draws the board if it changed, holding the lock only to read it.
    fn draw(&self, rasteriser: &mut Rasteriser) {
        let frame = {
            let mut board = self.lock();
            if !board.take_dirty() {
                return;
            }
            board.frame()
        };
        let Some(frame) = frame else { return };

        let drawn = match frame {
            Frame::Empty => {
                self.surface.clear();
                (false, false)
            }
            Frame::Map(job) => match rasteriser.draw(&job) {
                Ok(Rastered::Map(frame)) => {
                    self.surface.present(frame);
                    (true, false)
                }
                Ok(Rastered::NoArt) => {
                    self.surface.clear();
                    (false, true)
                }
                Err(err) => {
                    log::warn!("tactics board: a frame could not be built: {err}");
                    (false, false)
                }
            },
        };

        let mut board = self.lock();
        board.drew(drawn.0, drawn.1);
        if board.take_changed() {
            self.bump();
        }
    }
}

/// One input event of the frame surface, as the board takes it. Keys are the
/// view's keymap's to handle, so the frame's own are not read here.
fn apply(board: &mut Board, input: SurfaceInput) {
    match input {
        SurfaceInput::Resized { size, scale } => board.resize(Viewport { size, scale }),
        SurfaceInput::PointerDown { at, button, clicks, modifiers } => board.pointer_down(at, button, clicks, modifiers),
        SurfaceInput::PointerUp { at, button, .. } => board.pointer_up(at, button),
        SurfaceInput::PointerMoved { at, .. } => board.pointer_moved(at),
        SurfaceInput::PointerLeft => board.pointer_left(),
        SurfaceInput::Wheel { at, dy, .. } => board.wheel(at, dy),
        // A board zooms by the wheel, as the GPUI and egui boards do; keys
        // reach it through the view's keymap, which takes them first; focus
        // changes nothing it draws.
        SurfaceInput::Pinch { .. } | SurfaceInput::Key { .. } | SurfaceInput::Focus { .. } => {}
    }
}

/// Saves each new layout on its own rather than the whole set: a walk holds a
/// copy taken when it started, and writing all of it would put those rows back
/// over anything written since.
fn save_layouts(fresh: &[wows_replay_insights::cap_layout::CapLayout]) {
    if fresh.is_empty() {
        return;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            log::warn!("tactics: the capture layouts were not saved: {err}");
            return;
        }
    };
    runtime.block_on(async {
        let pool = match wows_toolkit_config::open_db().await {
            Ok(pool) => pool,
            Err(err) => {
                log::warn!("tactics: the capture layouts were not saved: {err}");
                return;
            }
        };
        for layout in fresh {
            if let Err(err) = layout_store::save_layout_to_db(&pool, layout).await {
                log::warn!("tactics: a capture layout was not saved: {err}");
            }
        }
    });
}
