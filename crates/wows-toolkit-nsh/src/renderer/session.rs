//! One open viewport: its viewer, the frame surface it draws on, and the
//! producer thread that turns the surface's input into viewer changes, plays
//! the battle and draws whatever changed.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::watch;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_toolkit_replay_data::minimap;
use wows_toolkit_replay_data::playback;
use wowsunpack::game_params::types::GameParamProvider;
use wt_collab_client::link::CollabLink;
use wt_collab_client::types::Annotation;

use crate::frames;
use crate::frames::Surface;
use crate::frames::SurfaceId;
use crate::frames::SurfaceInput;
use crate::renderer::raster;
use crate::renderer::services;
use crate::renderer::viewer::Viewer;

/// How often a viewport in a running session redraws, which is what moves a
/// peer's cursor and carries a ping through its ripple.
const COLLAB_TICK: Duration = Duration::from_millis(33);

enum Message {
    Input(SurfaceInput),
    /// The viewer changed from outside the producer and needs drawing.
    Redraw,
    /// The viewport closed.
    Stop,
}

/// A viewport, the surface it is drawn on, and how the view hears it changed.
pub struct Session {
    viewer: Mutex<Viewer>,
    surface: Arc<Surface>,
    revision: watch::Sender<u32>,
    wake: mpsc::Sender<Message>,
    /// Set when the viewport closes, so a bake in flight stops walking a
    /// battle nobody is waiting for.
    cancel: Arc<AtomicBool>,
    /// The saved defaults a viewport opens with.
    defaults: SavedRenderOptions,
}

impl Session {
    /// Opens a viewport on `path` drawn on the surface `id`, and starts baking
    /// it and reading its events.
    pub fn open(id: &SurfaceId, path: PathBuf, alts: Vec<PathBuf>, defaults: SavedRenderOptions) -> Arc<Self> {
        let surface = frames::surface(id);
        let (wake, messages) = mpsc::channel();
        let (revision, _) = watch::channel(0);
        let links = services::collab_links();
        let collab = links.borrow().clone();
        let session = Arc::new(Self {
            viewer: Mutex::new(Viewer::new(path, alts, collab)),
            surface: Arc::clone(&surface),
            revision,
            wake,
            cancel: Arc::new(AtomicBool::new(false)),
            defaults,
        });

        if let Some(mut input) = surface.take_input() {
            let forward = session.wake.clone();
            std::thread::Builder::new()
                .name("renderer-input".to_owned())
                .spawn(move || {
                    while let Some(event) = input.blocking_recv() {
                        if forward.send(Message::Input(event)).is_err() {
                            return;
                        }
                    }
                })
                .expect("the renderer input thread starts");
        }

        let producing = Arc::clone(&session);
        std::thread::Builder::new()
            .name("renderer".to_owned())
            .spawn(move || producing.run(&messages, links))
            .expect("the renderer producer thread starts");

        session.bake();
        session.read_events();
        session
    }

    pub fn lock(&self) -> MutexGuard<'_, Viewer> {
        self.viewer.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Changes the viewer, then has it drawn and the view told as the change
    /// asks.
    pub fn update<R>(&self, change: impl FnOnce(&mut Viewer) -> R) -> R {
        let result = {
            let mut viewer = self.lock();
            let result = change(&mut viewer);
            if viewer.take_changed() {
                self.bump();
            }
            result
        };
        // Wakes the producer for a draw the change left due, and for a change
        // of tick: playing started, or the speed moved. A stopped producer has
        // nothing to wake.
        let _ = self.wake.send(Message::Redraw);
        result
    }

    /// The revision the view last heard of, and a way to wait for the next.
    pub fn subscribe(&self) -> watch::Receiver<u32> {
        self.revision.subscribe()
    }

    fn bump(&self) {
        self.revision.send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    /// Stops the producer and any bake; the session stops offering the
    /// playback.
    pub fn close(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.wake.send(Message::Stop);
        let viewer = self.lock();
        if viewer.is_shared() {
            viewer.collab().close_replay(viewer.replay_id());
        }
    }

    /// Walks the battle on its own thread.
    pub fn bake(self: &Arc<Self>) {
        let (path, alts) = {
            let viewer = self.lock();
            (viewer.path().clone(), viewer.alts().to_vec())
        };
        let session = Arc::clone(self);
        std::thread::spawn(move || {
            let game_data = services::game_data();
            let baked = playback::bake(&path, &alts, &game_data, &session.cancel);
            if session.cancel.load(Ordering::Relaxed) {
                return;
            }
            match baked {
                Ok((track, renderer)) => session.update(|viewer| viewer.baked(track, renderer, &session.defaults)),
                Err(err) => {
                    log::warn!("renderer: {} did not bake: {err}", path.display());
                    session.update(|viewer| viewer.bake_failed(err.to_string()));
                }
            }
        });
    }

    /// Reads what happened in the battle: a second walk, so the viewport is
    /// worth showing before it finishes.
    fn read_events(self: &Arc<Self>) {
        let path = self.lock().path().clone();
        let session = Arc::clone(self);
        std::thread::spawn(move || {
            let read = minimap::extract_events(&path, &services::game_data());
            match read {
                // What each ship took is read for the armor viewer, which
                // this viewport does not feed.
                Ok((read, _shots)) => {
                    session.update(|viewer| viewer.events_found(read.events, read.viewer_team));
                }
                Err(err) => {
                    log::warn!("renderer: the battle's events could not be read: {err}");
                    session.update(Viewer::events_failed);
                }
            }
        });
    }

    fn run(self: Arc<Self>, messages: &mpsc::Receiver<Message>, mut links: watch::Receiver<CollabLink>) {
        let mut next_tick: Option<Instant> = None;
        loop {
            let (tick, following) = {
                let viewer = self.lock();
                (viewer.tick_interval(), viewer.collab().is_active())
            };
            next_tick = match tick {
                Some(interval) => Some(next_tick.unwrap_or_else(|| Instant::now() + interval)),
                None => None,
            };
            let deadline = [next_tick, following.then(|| Instant::now() + COLLAB_TICK)].into_iter().flatten().min();
            let first = match deadline {
                Some(deadline) => match messages.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(message) => Some(message),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                },
                None => match messages.recv() {
                    Ok(message) => Some(message),
                    Err(mpsc::RecvError) => return,
                },
            };

            {
                let mut viewer = self.lock();
                if let Some(due) = next_tick
                    && Instant::now() >= due
                {
                    viewer.tick();
                    next_tick = viewer.tick_interval().map(|interval| due.max(Instant::now() - interval) + interval);
                }
                if following {
                    viewer.follow_session();
                }
                // Everything waiting is taken before drawing, so a burst of
                // pointer events draws once.
                for message in first.into_iter().chain(messages.try_iter()) {
                    match message {
                        Message::Input(input) => viewer.apply(input),
                        Message::Redraw => {}
                        Message::Stop => {
                            self.surface.clear();
                            frames::release(self.surface.id());
                            return;
                        }
                    }
                }
                if links.has_changed().unwrap_or(false) {
                    let link = links.borrow_and_update().clone();
                    viewer.set_collab(link);
                }
                if viewer.take_changed() {
                    self.bump();
                }
            }

            self.follow_layout();
            self.announce();
            self.draw();
        }
    }

    /// Moves the viewport onto the canvas its layers need, building the
    /// renderer for it outside the lock: the first build reads a map's art.
    fn follow_layout(&self) {
        let (wanted, path) = {
            let viewer = self.lock();
            let Some(wanted) = viewer.wanted_layout() else { return };
            (wanted, viewer.path().clone())
        };
        let built = minimap::renderer_for_replay(&path, &services::game_data(), wanted);
        self.update(|viewer| match built {
            Ok((renderer, origin)) => viewer.adopt_layout(wanted, renderer, origin),
            Err(err) => {
                // The layer stays off rather than drawing on a canvas with no
                // room for it.
                log::warn!("renderer: the canvas for {wanted:?} could not be built: {err}");
                viewer.layout_failed();
            }
        });
    }

    /// Tells the session this playback is open, with the map art a peer
    /// without this build draws it on.
    fn announce(&self) {
        let shared = {
            let mut viewer = self.lock();
            if !viewer.wants_announcing() {
                return;
            }
            viewer.announced();
            let Some(track) = viewer.track() else { return };
            (viewer.collab().clone(), viewer.replay_id(), viewer.title().to_owned(), track.space.clone(), track.version)
        };
        std::thread::spawn(move || {
            let (link, replay_id, replay_name, space, version) = shared;
            let art_png = services::game_data()
                .newest_loaded()
                .and_then(|loaded| wows_minimap_renderer::assets::load_map_image(&space, loaded.vfs()))
                .and_then(|art| {
                    let mut png = Vec::new();
                    art.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok().map(|()| png)
                });
            link.announce_replay(wt_collab_client::link::SharedReplay {
                replay_id,
                replay_name,
                map_label: wows_toolkit_viewmodel::tactics::naming::pretty_map_name(&space),
                map_name: space,
                game_version: version.to_path(),
                art_png,
            });
        });
    }

    /// Draws the viewer if it changed, holding the lock only to read it.
    fn draw(&self) {
        let job = {
            let mut viewer = self.lock();
            if !viewer.take_dirty() {
                return;
            }
            let placed = viewer.placed_ships().map(|(ships, space_size, version)| placed_ranges(&ships, space_size, version));
            viewer.broadcast_current();
            viewer.draw_job(placed.unwrap_or_default())
        };
        let Some(job) = job else { return };

        match raster::draw(&job) {
            Ok(drawn) => {
                self.surface.present(drawn.frame);
                let mut viewer = self.lock();
                viewer.drew(drawn.canvas, drawn.regions);
                if viewer.take_changed() {
                    self.bump();
                }
            }
            Err(err) => log::warn!("renderer: a frame could not be built: {err}"),
        }
    }
}

/// The range circles every placed ship asks for, read out of its own game
/// data. Nothing is drawn for a ship not yet named, since there is no ship to
/// read the ranges of.
fn placed_ranges(ships: &[Annotation], space_size: f32, version: wowsunpack::data::Version) -> Vec<DrawCommand> {
    let Some(loaded) = services::game_data().newest_loaded() else { return Vec::new() };
    let provider: &dyn GameParamProvider = loaded.provider().as_ref();

    let mut circles = Vec::new();
    for ship in ships {
        let Annotation::Ship { config: Some(config), .. } = ship else { continue };
        let Some(param) = provider.game_param_by_id(config.param_id.into()) else { continue };
        let Some(vehicle) = param.vehicle() else { continue };
        let hull = (!config.hull_name.is_empty()).then_some(config.hull_name.as_str());
        let ranges = vehicle.resolve_ranges(Some(provider), hull, version);
        circles.extend(wt_collab_client::geometry::ship_range_commands(ship, &ranges, space_size));
    }

    circles
}
