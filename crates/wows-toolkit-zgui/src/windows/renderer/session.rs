//! One replay played back on its minimap: the baked track, where playback is, what is drawn, and
//! the work behind each of them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use rust_i18n::t;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::drawing::DrawnRegion;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::frame::BgraFrame;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::minimap_preview::SharedPreviewRenderer;
use wows_toolkit_replay_data::replay_render::annotate::Annotator;
use wows_toolkit_replay_data::replay_render::annotate::Click;
use wows_toolkit_replay_data::replay_render::annotate::PointerDown;
use wows_toolkit_replay_data::replay_render::annotate::PointerMove;
use wows_toolkit_replay_data::replay_render::annotate::PointerUp;
use wows_toolkit_replay_data::replay_render::annotate::tool_for_key;
use wows_toolkit_replay_data::replay_render::armor::ArmorFollow;
use wows_toolkit_replay_data::replay_render::armor::ArmorFollowed;
use wows_toolkit_replay_data::replay_render::armor::ShowArmor;
use wows_toolkit_replay_data::replay_render::collab::COLLAB_TICK;
use wows_toolkit_replay_data::replay_render::collab::CursorReport;
use wows_toolkit_replay_data::replay_render::collab::PING_SECONDS;
use wows_toolkit_replay_data::replay_render::collab::frame_broadcast;
use wows_toolkit_replay_data::replay_render::collab::shared_replay;
use wows_toolkit_replay_data::replay_render::export::ExportProgress;
use wows_toolkit_replay_data::replay_render::export::ExportSettings;
use wows_toolkit_replay_data::replay_render::export::ExportStage;
use wows_toolkit_replay_data::replay_render::export::ExportTarget;
use wows_toolkit_replay_data::replay_render::export::encode_track;
use wows_toolkit_replay_data::replay_render::export::encoder_status;
use wows_toolkit_replay_data::replay_render::export::export_duration;
use wows_toolkit_replay_data::replay_render::export::export_frames;
use wows_toolkit_replay_data::replay_render::export::export_settings_of;
use wows_toolkit_replay_data::replay_render::export::must_use_cpu;
use wows_toolkit_replay_data::replay_render::export::suggested_name;
use wows_toolkit_replay_data::replay_render::export::temporary_output;
use wows_toolkit_replay_data::replay_render::hover::ConsumableHover;
use wows_toolkit_replay_data::replay_render::hover::consumable_at;
use wows_toolkit_replay_data::replay_render::hover::hover_changed;
use wows_toolkit_replay_data::replay_render::options::adopt_saved_defaults;
use wows_toolkit_replay_data::replay_render::options::defaults_to_save;
use wows_toolkit_replay_data::replay_render::options::playback_options_hidden;
use wows_toolkit_replay_data::replay_render::playback::Playback;
use wows_toolkit_replay_data::replay_render::playback::SEEK_STEP;
use wows_toolkit_replay_data::replay_render::playback::Tick;
use wows_toolkit_replay_data::replay_render::raster::LayoutChange;
use wows_toolkit_replay_data::replay_render::raster::follow_layout;
use wows_toolkit_replay_data::replay_render::raster::raster_commands;
use wows_toolkit_replay_data::replay_render::ships::ShipChoices;
use wows_toolkit_replay_data::replay_render::ships::ShipMenu;
use wows_toolkit_replay_data::replay_render::ships::drawn_ships;
use wows_toolkit_replay_data::replay_render::ships::ship_at;
use wows_toolkit_replay_data::replay_render::timeline::BattleEvents;
use wows_toolkit_replay_data::replay_render::track::FrameIndex;
use wows_toolkit_replay_data::replay_render::track::Track;
use wows_toolkit_replay_data::replay_render::track::bake;
use wows_toolkit_replay_data::replay_render::view::ElementBounds;
use wows_toolkit_replay_data::replay_render::view::ElementPoint;
use wows_toolkit_replay_data::replay_render::view::FrameSize;
use wows_toolkit_replay_data::replay_render::view::MapView;
use wows_toolkit_replay_data::replay_render::view::Placement;
use wows_toolkit_replay_data::replay_render::view::Wheel;
use wows_toolkit_replay_data::replay_render::view::WindowPoint;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::ReplayId;
use wt_collab_client::drawing::Tool;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::Task;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view::IntervalHandle;
use zgui::view::Timers;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use crate::frames::FrameSink;
use crate::services::Job;
use crate::services::Progress;
use crate::services::Services;

/// A replay to play back, and other recordings of the same battle read alongside it so the map
/// shows what the primary's team could not see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderRequest {
    pub path: PathBuf,
    pub alts: Vec<PathBuf>,
}

impl RenderRequest {
    /// What a viewport on this replay is called: its file stem.
    pub fn title(&self) -> String {
        self.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| t!("ui.replay.context.render_replay").into_owned())
    }
}

/// What the viewport is doing.
#[derive(Clone)]
pub enum Status {
    /// Reading the replay and walking the battle.
    Baking,
    Ready(Arc<Track>),
    Failed(String),
}

/// What the viewport draws of what it baked. Applied when a frame is rasterised rather than when
/// it was baked, so a toggle takes effect on the next frame without walking the battle again.
#[derive(Clone)]
pub struct Layers {
    pub options: RenderOptions,
    /// Whether ships that have sunk are still drawn. Not one of [`RenderOptions`]: it gates a
    /// command rather than a layer.
    pub show_dead_ships: bool,
}

/// The export under way, what it is encoded with, and why the last one stopped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportState {
    pub settings: ExportSettings,
    /// Only one at a time: it holds the renderer the viewport draws through.
    pub progress: Option<ExportProgress>,
    /// Why the last export stopped, when it did not finish. Cleared when the next one starts.
    pub failure: Option<String>,
}

/// What a viewport asks of the view hosting it.
#[derive(Clone, Debug)]
pub enum ArmorRequest {
    /// Show a ship's armor with what it had taken by where playback is.
    Show(ShowArmor),
    /// Playback moved, and the ship an armor viewer is already open on has taken different hits.
    /// Separate from `Show` because it must not pull the reader to the armor viewer again.
    Followed(ArmorFollowed),
}

/// The consumable icon under the pointer, and where its reading is anchored.
pub struct HoveredConsumable {
    pub hover: ConsumableHover,
    pub at: ElementPoint,
}

/// A viewport's state, as one `Copy` handle.
///
/// Its signals and its work belong to a scope of its own, so the view showing it can move between
/// windows: a pop-out keeps the baked track and the renderer. [`Self::close`] ends it.
#[derive(Clone, Copy)]
pub struct RendererSession {
    pub status: RwSignal<Status, LocalStorage>,
    pub playback: RwSignal<Playback, LocalStorage>,
    pub layers: RwSignal<Layers, LocalStorage>,
    pub map: RwSignal<MapView, LocalStorage>,
    /// Where the map's corner sits in the frames being drawn, which moves with the canvas.
    pub map_origin: RwSignal<(f32, f32), LocalStorage>,
    pub choices: RwSignal<ShipChoices, LocalStorage>,
    pub menu: RwSignal<Option<ShipMenu>, LocalStorage>,
    pub events: RwSignal<BattleEvents, LocalStorage>,
    pub export: RwSignal<ExportState, LocalStorage>,
    pub annotator: RwSignal<Annotator, LocalStorage>,
    pub collab: RwSignal<CollabLink, LocalStorage>,
    /// Counts the redraws a running session asks for, which move peers' cursors and pings.
    pub collab_frame: RwSignal<u64, LocalStorage>,
    pub hovered: RwSignal<Option<HoveredConsumable>, LocalStorage>,
    /// The size of the frame on screen, `None` until the first one lands.
    pub frame_size: RwSignal<Option<FrameSize>, LocalStorage>,
    /// Whether the primary modifier is held, which puts the shortcut sheet up.
    pub ctrl_held: RwSignal<bool, LocalStorage>,
    /// Whether the viewport has a window to itself, which withdraws the pop-out control.
    pub popped_out: RwSignal<bool, LocalStorage>,
    /// The export waiting on the reader to accept a software encode.
    pub export_notice: RwSignal<Option<ExportTarget>, LocalStorage>,
    run: StoredValue<Run, LocalStorage>,
}

/// The parts of a session the view does not read.
struct Run {
    request: RenderRequest,
    title: String,
    owner: Owner,
    timers: Option<Timers>,
    services: Services,
    game_data: Option<GameDataCache>,
    sink: FrameSink,
    /// The renderer the track is rasterised through. Taken while a frame is drawn off the UI
    /// thread or an export runs, which is what keeps two draws from running at once.
    renderer: Option<SharedPreviewRenderer>,
    /// Moved on whenever `renderer` is replaced, so a frame drawn on the old canvas, and the old
    /// renderer it brings back, are dropped.
    renderer_generation: Generation,
    layout: SidePanelLayout,
    /// A frame was asked for while one was being drawn. The draw in flight starts another as it
    /// finishes, so what ends up on screen is the newest thing asked for.
    redraw_wanted: bool,
    /// An export asked for while a frame was being drawn. It starts as the draw hands the
    /// renderer back.
    pending_export: Option<ExportTarget>,
    regions: Vec<DrawnRegion>,
    /// Set when the session closes, so an encode in flight stops.
    cancel: Arc<AtomicBool>,
    /// The bake in flight. A newer one cancels it and drops whatever it produces.
    bake_generation: Generation,
    bake_cancel: Arc<AtomicBool>,
    ticker: Option<IntervalHandle>,
    collab_ticker: Option<IntervalHandle>,
    tasks: Vec<Task>,
    replay_id: ReplayId,
    /// Whether the session has been told this playback is open.
    shared: bool,
    /// Moved on when the link changes, so an announcement prepared for the old link is not made.
    share_generation: Generation,
    cursor: CursorReport,
    armor: Option<ArmorFollow>,
    on_armor: Option<UnsyncCallback<ArmorRequest>>,
    /// Where a renderer's window was last, read as the session opens so a pop-out opens there.
    remembered: Option<wows_toolkit_config::WindowSettings>,
}

impl RendererSession {
    /// Opens a viewport on `request` and starts baking it. Called from a component's body inside
    /// the main window.
    pub fn open(request: RenderRequest, collab: CollabLink) -> Self {
        let services = Services::current();
        let owner = Owner::new();
        let session = owner.with(|| Self::new_in(owner.clone(), services, request, collab));
        session.start_bake();
        session.start_event_scan();
        session.follow_collab();
        session.read_remembered_window();
        session
    }

    fn new_in(owner: Owner, services: Services, request: RenderRequest, collab: CollabLink) -> Self {
        let title = request.title();
        Self {
            status: RwSignal::new_local(Status::Baking),
            playback: RwSignal::new_local(Playback::new()),
            // The options the track was baked under, so what is drawn at first is what is in it.
            layers: RwSignal::new_local(Layers { options: playback_options_hidden(), show_dead_ships: true }),
            map: RwSignal::new_local(MapView::new()),
            map_origin: RwSignal::new_local(wows_toolkit_replay_data::replay_render::track::DEFAULT_MAP_ORIGIN),
            choices: RwSignal::new_local(ShipChoices::new()),
            menu: RwSignal::new_local(None),
            events: RwSignal::new_local(BattleEvents::new()),
            export: RwSignal::new_local(ExportState::default()),
            annotator: RwSignal::new_local(Annotator::new()),
            collab: RwSignal::new_local(collab),
            collab_frame: RwSignal::new_local(0),
            hovered: RwSignal::new_local(None),
            frame_size: RwSignal::new_local(None),
            ctrl_held: RwSignal::new_local(false),
            popped_out: RwSignal::new_local(false),
            export_notice: RwSignal::new_local(None),
            run: StoredValue::new_local(Run {
                title,
                owner,
                timers: Timers::current(),
                services,
                game_data: services.game_data.cache_untracked(),
                sink: FrameSink::new(),
                renderer: None,
                renderer_generation: Generation::default(),
                layout: SidePanelLayout::None,
                redraw_wanted: false,
                pending_export: None,
                regions: Vec::new(),
                cancel: Arc::new(AtomicBool::new(false)),
                bake_generation: Generation::default(),
                bake_cancel: Arc::new(AtomicBool::new(false)),
                ticker: None,
                collab_ticker: None,
                tasks: Vec::new(),
                replay_id: ReplayId::fresh(),
                shared: false,
                share_generation: Generation::default(),
                cursor: CursorReport::new(),
                armor: None,
                on_armor: None,
                remembered: None,
                request,
            }),
        }
    }

    /// A session already holding `track`, with no bake behind it and no renderer to draw through.
    #[cfg(test)]
    pub fn ready_for_test(track: Track) -> Self {
        let services = Services::current();
        let owner = Owner::new();
        let request = RenderRequest { path: PathBuf::from("test.wowsreplay"), alts: Vec::new() };
        let session = owner.with(|| Self::new_in(owner.clone(), services, request, CollabLink::default()));
        session.map_origin.set(track.map_origin);
        session.status.set(Status::Ready(Arc::new(track)));
        session
    }

    /// Stops everything this session started and disposes of its state.
    pub fn close(self) {
        let owner = self.run.try_update_value(|run| {
            run.cancel.store(true, Ordering::Relaxed);
            run.bake_cancel.store(true, Ordering::Relaxed);
            run.ticker = None;
            run.collab_ticker = None;
            for task in run.tasks.drain(..) {
                task.cancel();
            }
            let replay_id = run.replay_id;
            let shared = run.shared;
            (run.owner.clone(), shared.then_some(replay_id))
        });
        let Some((owner, shared)) = owner else { return };
        if let Some(replay_id) = shared {
            self.collab.with_untracked(|link| link.close_replay(replay_id));
        }
        owner.cleanup();
    }

    /// Whether the session's state is still there to read.
    pub fn is_open(self) -> bool {
        self.run.try_with_value(|_| ()).is_some()
    }

    pub fn title(self) -> String {
        self.run.with_value(|run| run.title.clone())
    }

    /// Where the frames are shown.
    pub fn sink(self) -> FrameSink {
        self.run.with_value(|run| run.sink.clone())
    }

    /// Who hears this viewport's asks for an armor viewer.
    pub fn set_armor_listener(self, listener: UnsyncCallback<ArmorRequest>) {
        self.run.update_value(|run| run.on_armor = Some(listener));
    }

    /// The baked battle, once the bake has landed. Reactive.
    pub fn track(self) -> Option<Arc<Track>> {
        self.status.with(|status| match status {
            Status::Ready(track) => Some(Arc::clone(track)),
            _ => None,
        })
    }

    pub fn track_untracked(self) -> Option<Arc<Track>> {
        self.status.with_untracked(|status| match status {
            Status::Ready(track) => Some(Arc::clone(track)),
            _ => None,
        })
    }

    /// Whether the transport can act: the track is here.
    pub fn is_ready(self) -> bool {
        self.status.with(|status| matches!(status, Status::Ready(_)))
    }

    /// Bakes this battle again through another set of recordings, for a viewport opened before
    /// one of them was added.
    pub fn rebake_with_alts(self, alts: Vec<PathBuf>) {
        let changed = self.with_run(|run| {
            if run.request.alts == alts {
                return false;
            }
            run.request.alts = alts;
            true
        });
        if changed == Some(true) {
            self.start_bake();
        }
    }

    /// `None` once the session has closed.
    fn with_run<U>(self, change: impl FnOnce(&mut Run) -> U) -> Option<U> {
        self.run.try_update_value(change)
    }

    /// The game data the session was opened with. `None` once it has closed, or when no build was
    /// loaded.
    pub fn game_data(self) -> Option<GameDataCache> {
        self.run.try_with_value(|run| run.game_data.clone()).flatten()
    }

    /// The process's services, for work started from a listener. `None` once the session closed.
    pub fn services(self) -> Option<Services> {
        self.run.try_with_value(|run| run.services)
    }

    /// Runs `work` on the UI thread until it finishes or the session closes.
    pub fn spawn(self, work: impl Future<Output = ()> + 'static) {
        self.run.update_value(|run| {
            run.tasks.retain(|task| !task.is_finished());
            let task = run.owner.with(|| spawn_local(work));
            run.tasks.push(task);
        });
    }

    fn start_bake(self) {
        let started = self.with_run(|run| {
            let game_data = run.game_data.clone()?;
            // A bake through the recordings asked for before this is not waited for.
            run.bake_cancel.store(true, Ordering::Relaxed);
            run.bake_cancel = Arc::new(AtomicBool::new(false));
            let generation = run.bake_generation.next();
            Some(BakeStart {
                request: run.request.clone(),
                game_data,
                cancel: Arc::clone(&run.bake_cancel),
                generation,
            })
        });
        let Some(started) = started else { return };
        let Some(BakeStart { request, game_data, cancel, generation }) = started else {
            self.status.set(Status::Failed(t!("ui.replay.loading_game_data").into_owned()));
            return;
        };
        self.status.set(Status::Baking);
        self.spawn(async move {
            let baked = blocking(move || bake(&request.path, &request.alts, &game_data, &cancel)).await;
            let current = self.run.try_with_value(|run| run.bake_generation == generation);
            if current != Some(true) {
                return;
            }
            match baked {
                Ok((track, renderer)) => {
                    self.run.update_value(|run| {
                        run.renderer = Some(renderer);
                        run.renderer_generation.next();
                        run.layout = SidePanelLayout::None;
                    });
                    self.map_origin.set(track.map_origin);
                    self.playback.update(|playback| playback.stop());
                    self.status.set(Status::Ready(Arc::new(track)));
                    self.share_playback();
                    self.adopt_saved_defaults().await;
                }
                Err(err) => self.status.set(Status::Failed(err.to_string())),
            }
        });
    }

    /// Reads what happened in the battle, which the event controls step between. A second walk of
    /// the replay, so the viewport is worth showing before it finishes.
    fn start_event_scan(self) {
        let Some((path, game_data)) =
            self.run.with_value(|run| run.game_data.clone().map(|data| (run.request.path.clone(), data)))
        else {
            return;
        };
        self.spawn(async move {
            let read =
                blocking(move || wows_toolkit_replay_data::minimap_preview::extract_events(&path, &game_data)).await;
            match read {
                Ok((read, shots)) => {
                    let filter = self.events.with_untracked(|events| events.filter().clone());
                    let mut events = BattleEvents::from_extraction(read, shots);
                    events.set_filter(|kept| *kept = filter);
                    self.events.set(events);
                }
                // The viewport still plays; only the event controls are worse off.
                Err(reason) => tracing::warn!("replay renderer: the battle's events could not be read: {reason}"),
            }
        });
    }

    /// Opens showing what the reader last saved as the defaults. Runs once the bake has landed,
    /// because the rosters are a wider canvas rather than a layer.
    async fn adopt_saved_defaults(self) {
        let Some(services) = self.run.try_with_value(|run| run.services) else { return };
        let saved = match services.settings.pool() {
            Some(pool) => services
                .on_tokio(async move { wows_toolkit_replay_data::render_defaults::load(&pool).await })
                .await
                .unwrap_or_default(),
            // No store to read: what the renderer itself defaults to, as an unwritten row reads.
            None => SavedRenderOptions::default(),
        };
        self.export.update(|export| export.settings = export_settings_of(&saved));
        self.set_layers(|layers| adopt_saved_defaults(&saved, &mut layers.options, &mut layers.show_dead_ships));
    }

    /// Writes what this viewport shows back as what a viewport opens with. Resolves to whether it
    /// was written.
    pub async fn save_defaults(self) -> bool {
        let Some(services) = self.run.try_with_value(|run| run.services) else { return false };
        let Some(pool) = services.settings.pool() else { return false };
        let read_pool = pool.clone();
        let stored = services
            .on_tokio(async move { wows_toolkit_replay_data::render_defaults::load(&read_pool).await })
            .await
            .unwrap_or_default();
        let settings = self.export.with_untracked(|export| export.settings);
        let saved = self
            .layers
            .with_untracked(|layers| defaults_to_save(&stored, &layers.options, layers.show_dead_ships, &settings));
        let json = match wows_toolkit_replay_data::render_defaults::serialize(&saved) {
            Ok(json) => json,
            Err(err) => {
                tracing::warn!("replay renderer: the defaults could not be saved: {err}");
                return false;
            }
        };
        let written =
            services.on_tokio(async move { wows_toolkit_replay_data::render_defaults::save(&pool, &json).await }).await;
        match written {
            Ok(Ok(())) => true,
            Ok(Err(err)) => {
                tracing::warn!("replay renderer: the defaults could not be saved: {err}");
                false
            }
            Err(err) => {
                tracing::warn!("replay renderer: the defaults could not be saved: {err}");
                false
            }
        }
    }

    /// Changes what is drawn and redraws the frame on screen, so a toggle shows while paused.
    pub fn set_layers(self, apply: impl FnOnce(&mut Layers)) {
        let mut layers = self.layers.get_untracked();
        apply(&mut layers);
        self.follow_layout(&mut layers.options);
        self.layers.set(layers);
        self.draw();
    }

    /// Moves onto the canvas the layers now need: the rosters are a wider canvas, not a layer.
    fn follow_layout(self, options: &mut RenderOptions) {
        let Some((path, game_data, current)) =
            self.run.try_with_value(|run| (run.request.path.clone(), run.game_data.clone(), run.layout))
        else {
            return;
        };
        let Some(game_data) = game_data else {
            // Nothing to build a canvas from, so the gutter layers cannot be shown.
            options.show_team_rosters = false;
            options.show_stats_panel = false;
            return;
        };
        match follow_layout(&path, &game_data, options, current) {
            LayoutChange::Unchanged | LayoutChange::Refused => {}
            LayoutChange::Moved { layout, renderer, map_origin } => {
                self.run.update_value(|run| {
                    run.layout = layout;
                    // A draw or an export in flight on the old renderer drops it as it finishes.
                    run.renderer = Some(renderer);
                    run.renderer_generation.next();
                    run.redraw_wanted = true;
                });
                self.map_origin.set(map_origin);
            }
        }
    }

    /// Moves playback to `at`, follows it with the armor viewer, and draws it.
    pub fn go_to(self, at: FrameIndex) {
        let Some(track) = self.track_untracked() else { return };
        let moved = self.change_playback(|playback| playback.go_to(&track, at));
        if moved || self.frame_size.get_untracked().is_none() {
            self.moved();
        }
    }

    /// Moves playback to the frame at game time `clock`.
    pub fn go_to_clock(self, clock: GameClock) {
        let Some(track) = self.track_untracked() else { return };
        self.go_to(track.frame_at(clock.0));
    }

    /// Moves playback `delta` seconds of game time from where it is.
    pub fn seek_by(self, delta: f32) {
        let Some(track) = self.track_untracked() else { return };
        let moved = self.change_playback(|playback| playback.seek_by(&track, delta));
        if moved {
            self.moved();
        }
    }

    /// The scrubber's value, a frame index as a float.
    pub fn seek_to_slider(self, value: f32) {
        let Some(track) = self.track_untracked() else { return };
        let moved = self.change_playback(|playback| playback.seek_to_slider(&track, value));
        if moved {
            self.moved();
        }
    }

    /// Moves to `elapsed` seconds into the battle, which is how the timeline names a moment.
    pub fn go_to_elapsed(self, elapsed: f32) {
        let Some(track) = self.track_untracked() else { return };
        let moved = self.change_playback(|playback| playback.go_to_elapsed(&track, elapsed));
        if moved {
            self.moved();
        }
    }

    /// The event before where playback is, in the timeline's filter. Reactive.
    pub fn previous_event_at(self) -> Option<f32> {
        let track = self.track()?;
        let elapsed = self.playback.with(|playback| playback.elapsed(&track));
        self.events.with(|events| events.previous(elapsed).map(|event| event.clock.0))
    }

    /// The event after where playback is. Reactive.
    pub fn next_event_at(self) -> Option<f32> {
        let track = self.track()?;
        let elapsed = self.playback.with(|playback| playback.elapsed(&track));
        self.events.with(|events| events.next(elapsed).map(|event| event.clock.0))
    }

    pub fn jump_to_previous_event(self) {
        let Some(at) = self.neighbour_event(false) else { return };
        self.go_to_elapsed(at);
    }

    pub fn jump_to_next_event(self) {
        let Some(at) = self.neighbour_event(true) else { return };
        self.go_to_elapsed(at);
    }

    /// When the event after, or before, where playback is happened.
    fn neighbour_event(self, forward: bool) -> Option<f32> {
        let track = self.track_untracked()?;
        let elapsed = self.playback.with_untracked(|playback| playback.elapsed(&track));
        self.events.with_untracked(|events| {
            let event = if forward { events.next(elapsed) } else { events.previous(elapsed) };
            event.map(|event| event.clock.0)
        })
    }

    fn moved(self) {
        self.follow_armor();
        self.draw();
    }

    /// Changes playback, telling its readers only when `change` says something changed: a
    /// control bound to it writes its value back as it is built.
    fn change_playback(self, change: impl FnOnce(&mut Playback) -> bool) -> bool {
        self.playback
            .try_maybe_update(|playback| {
                let changed = change(playback);
                (changed, changed)
            })
            .unwrap_or(false)
    }

    /// [`Self::change_playback`] for the map's view.
    fn change_map(self, change: impl FnOnce(&mut MapView) -> bool) -> bool {
        self.map
            .try_maybe_update(|map| {
                let changed = change(map);
                (changed, changed)
            })
            .unwrap_or(false)
    }

    pub fn toggle_playing(self) {
        let Some(track) = self.track_untracked() else { return };
        // The playing flag always flips, so readers are always told, moved or not.
        self.change_playback(|playback| {
            playback.toggle_playing(&track);
            true
        });
        if self.playback.with_untracked(Playback::is_playing) {
            // Playing from the end starts again at the beginning, which `toggle_playing` did.
            self.moved();
            self.start_ticker();
        } else {
            self.run.update_value(|run| run.ticker = None);
        }
    }

    pub fn set_speed(self, speed: f32) {
        let changed = self.change_playback(|playback| playback.set_speed(speed));
        // The ticker's interval is fixed when it starts, so a speed change replaces it.
        if changed && self.playback.with_untracked(Playback::is_playing) {
            self.start_ticker();
        }
    }

    pub fn step_speed(self, step: i32) {
        let changed = self.change_playback(|playback| playback.step_speed(step));
        if changed && self.playback.with_untracked(Playback::is_playing) {
            self.start_ticker();
        }
    }

    fn start_ticker(self) {
        let interval = self.playback.with_untracked(Playback::tick_interval);
        self.run.update_value(|run| {
            run.ticker = run.timers.as_ref().map(|timers| timers.set_interval(interval, move || self.tick()));
        });
    }

    fn tick(self) {
        let Some(track) = self.track_untracked() else { return };
        let tick = self
            .playback
            .try_maybe_update(|playback| {
                let tick = playback.advance(&track);
                (tick != Tick::Idle, tick)
            })
            .unwrap_or(Tick::Idle);
        match tick {
            Tick::Advanced => self.moved(),
            Tick::Finished | Tick::Idle => {
                self.run.update_value(|run| run.ticker = None);
            }
        }
    }

    /// Shows a different part of the map, and redraws, so a zoom shows while paused.
    pub fn set_view(self, view: MapViewport) {
        let changed = self.change_map(|map| map.set_view(view));
        if changed {
            self.draw();
        }
    }

    pub fn zoom_to(self, zoom: f32) {
        let changed = self.change_map(|map| map.zoom_to(zoom));
        if changed {
            self.draw();
        }
    }

    /// Records where the frame sits on screen, which every pointer position is read against.
    pub fn place(self, bounds: ElementBounds) {
        let frame = self.frame_size.get_untracked();
        let placement = frame.map(|frame| Placement { bounds, frame });
        if self.map.with_untracked(|map| map.placement() == placement) {
            return;
        }
        self.map.update(|map| map.set_placement(placement));
    }

    /// The wheel zooms about whatever is under the pointer.
    pub fn wheel(self, position: WindowPoint, delta_y: f32) {
        let origin = self.map_origin.get_untracked();
        let wheel = self
            .map
            .try_maybe_update(|map| {
                let wheel = map.zoom_by_wheel(position, delta_y, origin);
                (wheel == Wheel::Zoomed, wheel)
            })
            .unwrap_or(Wheel::Ignored);
        if wheel == Wheel::Zoomed {
            self.draw();
        }
    }

    /// A primary press on the map: the tool in hand, a handle, a move, or a pan.
    pub fn pointer_down(self, position: WindowPoint) {
        self.menu.set(None);
        let origin = self.map_origin.get_untracked();
        let link = self.collab.get_untracked();
        let map = self.map.get_untracked();
        let down = self
            .annotator
            .try_maybe_update(|annotator| {
                let down = annotator.pointer_down(&link, &map, origin, position);
                (!matches!(down, PointerDown::OffMap | PointerDown::Unclaimed), down)
            })
            .unwrap_or(PointerDown::OffMap);
        match down {
            PointerDown::Stroked => self.draw(),
            PointerDown::Unclaimed => {
                self.change_map(|map| map.start_pan(position));
            }
            PointerDown::OffMap | PointerDown::Turning | PointerDown::Moving => {}
        }
    }

    /// The pointer moved. `held` is whether the primary button is down, `straight` whether a
    /// straight line is asked for.
    pub fn pointer_move(self, position: WindowPoint, held: bool, straight: bool) {
        self.report_cursor(position);
        self.update_hover(position);

        let origin = self.map_origin.get_untracked();
        let link = self.collab.get_untracked();
        let map = self.map.get_untracked();
        let moved = self
            .annotator
            .try_maybe_update(|annotator| {
                let moved = annotator.pointer_move(&link, &map, origin, position, held, straight);
                (matches!(moved, PointerMove::Turned | PointerMove::Stroked), moved)
            })
            .unwrap_or(PointerMove::Unclaimed);
        match moved {
            PointerMove::Turned | PointerMove::Stroked => self.draw(),
            // A part-drawn shape follows the pointer.
            PointerMove::Held => self.draw(),
            PointerMove::Unclaimed => {
                if self.change_map(|map| map.pan_to(position, held)) {
                    self.draw();
                }
            }
        }
    }

    pub fn pointer_up(self, position: WindowPoint) {
        self.change_map(|map| {
            let panning = map.is_panning();
            map.end_pan();
            panning
        });
        let origin = self.map_origin.get_untracked();
        let link = self.collab.get_untracked();
        let map = self.map.get_untracked();
        let up = self
            .annotator
            .try_maybe_update(|annotator| {
                let up = annotator.pointer_up(&link, &map, origin, position);
                (up != PointerUp::Nothing, up)
            })
            .unwrap_or(PointerUp::Nothing);
        if up != PointerUp::Nothing {
            self.draw();
        }
    }

    /// A click on the map. `add` adds to what is picked; `count` is how many clicks this ends.
    pub fn click(self, position: WindowPoint, add: bool, count: usize) {
        let origin = self.map_origin.get_untracked();
        let link = self.collab.get_untracked();
        let map = self.map.get_untracked();
        let mut click = Click::Ignored;
        self.annotator.update(|annotator| click = annotator.click(&link, &map, origin, position, add, count));
        match click {
            Click::ResetView => self.set_view(MapViewport::default()),
            Click::Stroked | Click::Picked | Click::Ignored => self.draw(),
        }
    }

    /// The pointer left the map.
    pub fn pointer_left(self) {
        let link = self.collab.get_untracked();
        self.run.update_value(|run| run.cursor.report(&link, None));
        if self.hovered.with_untracked(Option::is_some) {
            self.hovered.set(None);
        }
    }

    /// Opens the per-ship menu on whatever was right-clicked.
    pub fn open_ship_menu(self, position: WindowPoint) {
        let origin = self.map_origin.get_untracked();
        let Some(at) = self.map.with_untracked(|map| map.map_point(position, origin)) else { return };
        let Some(track) = self.track_untracked() else { return };
        let at_frame = self.playback.with_untracked(Playback::at);
        let picked = track.commands(at_frame).and_then(|commands| ship_at(commands, at));
        self.menu.set(picked.map(|ship| ShipMenu::open(ship, position)));
    }

    /// Drops a ping where the pointer is, for everyone in the session.
    pub fn ping(self, position: WindowPoint) {
        let origin = self.map_origin.get_untracked();
        let Some((x, y)) = self.map.with_untracked(|map| map.map_point(position, origin)) else { return };
        self.collab.with_untracked(|link| link.send_ping([x, y]));
    }

    /// Changes whose trails and ranges are drawn, and redraws.
    pub fn choose(self, apply: impl FnOnce(&mut ShipChoices, &[String], &mut RenderOptions)) {
        let drawn = self.drawn_ships();
        let mut layers = self.layers.get_untracked();
        self.choices.update(|choices| apply(choices, &drawn, &mut layers.options));
        self.follow_layout(&mut layers.options);
        self.layers.set(layers);
        self.draw();
    }

    /// The players whose ships are on the frame on screen.
    pub fn drawn_ships(self) -> Vec<String> {
        let Some(track) = self.track_untracked() else { return Vec::new() };
        let at = self.playback.with_untracked(Playback::at);
        track.commands(at).map(drawn_ships).unwrap_or_default()
    }

    /// Changes the annotations through the tool state and the session, and redraws.
    pub fn annotate<T>(self, apply: impl FnOnce(&mut Annotator, &CollabLink) -> T) -> T {
        let link = self.collab.get_untracked();
        let mut out = None;
        self.annotator.update(|annotator| out = Some(apply(annotator, &link)));
        self.draw();
        out.expect("update runs its closure")
    }

    /// Takes up `tool`, or puts the one in hand down with `Tool::None`.
    pub fn take_up(self, tool: Tool) {
        self.annotate(|annotator, _| annotator.take_up(tool));
    }

    /// Hands this viewport a session to talk to, or takes one away. What was drawn before stays
    /// on the map.
    pub fn set_collab(self, link: CollabLink) {
        let old = self.collab.get_untracked();
        let carried = old.annotations_held();
        let closed = self
            .with_run(|run| {
                run.share_generation.next();
                run.cursor = CursorReport::new();
                std::mem::take(&mut run.shared).then_some(run.replay_id)
            })
            .flatten();
        // Peers on the old session stop listing this playback.
        if let Some(replay_id) = closed {
            old.close_replay(replay_id);
        }
        link.adopt(carried);
        self.collab.set(link);
        self.share_playback();
        self.follow_collab();
        self.draw();
    }

    /// Starts or stops the redraw a running session needs: a peer's pointer and pings move without
    /// anything here touching them.
    fn follow_collab(self) {
        let active = self.collab.with_untracked(CollabLink::is_active);
        self.run.update_value(|run| {
            if !active {
                run.collab_ticker = None;
                return;
            }
            if run.collab_ticker.is_some() {
                return;
            }
            run.collab_ticker = run.timers.as_ref().map(|timers| {
                timers.set_interval(COLLAB_TICK, move || {
                    let active = self.collab.with_untracked(|link| {
                        link.drop_stale_pings(std::time::Duration::from_secs_f32(PING_SECONDS));
                        link.is_active()
                    });
                    if active {
                        self.collab_frame.update(|frame| *frame = frame.wrapping_add(1));
                    } else {
                        self.run.update_value(|run| run.collab_ticker = None);
                    }
                })
            });
        });
    }

    /// Tells the session this playback is open. The map art travels with it, since a peer may not
    /// have the build the replay was recorded on.
    fn share_playback(self) {
        let link = self.collab.get_untracked();
        if !link.broadcasts_frames() {
            return;
        }
        let Some(track) = self.track_untracked() else { return };
        let Some((replay_id, title, game_data)) = self
            .with_run(|run| {
                if run.shared {
                    return None;
                }
                let game_data = run.game_data.clone()?;
                run.shared = true;
                Some((run.replay_id, run.title.clone(), game_data))
            })
            .flatten()
        else {
            return;
        };
        let Some(generation) = self.run.try_with_value(|run| run.share_generation) else { return };
        self.spawn(async move {
            let shared = blocking(move || shared_replay(replay_id, title, &track, &game_data)).await;
            // The link this was prepared for has been replaced.
            if self.run.try_with_value(|run| run.share_generation) != Some(generation) {
                return;
            }
            if let Some(shared) = shared {
                link.announce_replay(shared);
            }
        });
    }

    fn report_cursor(self, position: WindowPoint) {
        let link = self.collab.get_untracked();
        if !link.is_active() {
            return;
        }
        let origin = self.map_origin.get_untracked();
        let at = self.map.with_untracked(|map| map.map_point(position, origin)).map(|(x, y)| [x, y]);
        self.run.update_value(|run| run.cursor.report(&link, at));
    }

    /// Reads the roster icon under the pointer.
    fn update_hover(self, position: WindowPoint) {
        let Some(track) = self.track_untracked() else { return };
        let at = self.playback.with_untracked(Playback::at);
        let (canvas, element) = self.map.with_untracked(|map| (map.canvas_point(position), map.to_element(position)));
        let found = match (canvas, track.commands(at)) {
            (Some(canvas), Some(commands)) => self.run.with_value(|run| consumable_at(&run.regions, commands, canvas)),
            _ => None,
        };
        let changed = self
            .hovered
            .with_untracked(|hovered| hover_changed(hovered.as_ref().map(|shown| &shown.hover), found.as_ref()));
        let anchor = element.unwrap_or_default();
        if changed {
            self.hovered.set(found.map(|hover| HoveredConsumable { hover, at: anchor }));
        } else if found.is_some() {
            // The same icon: the reading follows the pointer.
            self.hovered.update(|hovered| {
                if let Some(hovered) = hovered.as_mut() {
                    hovered.at = anchor;
                }
            });
        }
    }

    /// Opens the armor viewer on a ship and follows it from then on.
    pub fn show_armor(self, entity_id: EntityId) {
        let Some(track) = self.track_untracked() else { return };
        let at = self.playback.with_untracked(Playback::at);
        let clock = GameClock(track.seconds_at(at));
        let Some(commands) = track.commands(at) else { return };
        let Some(game_data) = self.run.with_value(|run| run.game_data.clone()) else { return };
        let started = self
            .events
            .with_untracked(|events| ArmorFollow::start(entity_id, commands, events.shots(), clock, &game_data));
        let Some((follow, shown)) = started else { return };
        let listener = self.with_run(|run| {
            run.armor = Some(follow);
            run.on_armor
        });
        if let Some(listener) = listener.flatten() {
            listener.run(ArmorRequest::Show(shown));
        }
    }

    fn follow_armor(self) {
        let Some(track) = self.track_untracked() else { return };
        let clock = GameClock(track.seconds_at(self.playback.with_untracked(Playback::at)));
        let followed = self.with_run(|run| {
            let followed = run.armor.as_mut()?.follow(clock)?;
            Some((followed, run.on_armor?))
        });
        if let Some((followed, listener)) = followed.flatten() {
            listener.run(ArmorRequest::Followed(followed));
        }
    }

    /// Draws the frame playback is on, unless one is already being drawn, in which case that one
    /// draws again as it finishes.
    pub fn draw(self) {
        let Some(track) = self.track_untracked() else { return };
        let Some(game_data) = self.run.try_with_value(|run| run.game_data.clone()).flatten() else { return };
        let Some((renderer, generation)) = self
            .with_run(|run| {
                let renderer = run.renderer.take();
                // This draw is of the newest state, which is what any earlier ask wanted.
                run.redraw_wanted = renderer.is_none();
                renderer.map(|renderer| (renderer, run.renderer_generation))
            })
            .flatten()
        else {
            return;
        };

        let at = self.playback.with_untracked(Playback::at);
        let link = self.collab.get_untracked();
        let drawn_on = self.annotator.with_untracked(|annotator| annotator.overlay_commands(&link, &track, &game_data));
        let shown = self.layers.with_untracked(|layers| {
            self.choices.with_untracked(|choices| {
                raster_commands(&track, at, &layers.options, layers.show_dead_ships, choices, drawn_on)
            })
        });
        let Some(shown) = shown else {
            self.run.update_value(|run| run.renderer = Some(renderer));
            return;
        };
        self.broadcast(&track, at);

        let view = self.map.with_untracked(MapView::shaped_view);
        let drawing = Arc::clone(&renderer);
        self.spawn(async move {
            let drawn = blocking(move || {
                let mut drawing = drawing.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let (image, regions) = drawing.render_regions(view, &shown);
                let size = FrameSize { width: image.width(), height: image.height() };
                RasterOutput { frame: BgraFrame::from_rgb(&image), size, regions }
            })
            .await;
            self.finish_draw(generation, renderer, drawn);
        });
    }

    fn finish_draw(self, generation: Generation, renderer: SharedPreviewRenderer, drawn: RasterOutput) {
        let RasterOutput { frame, size, regions } = drawn;
        let current = self.with_run(|run| {
            // Drawn on a canvas a layout change replaced: neither the frame nor the renderer is wanted.
            let current = run.renderer_generation == generation;
            if current {
                run.renderer = Some(renderer);
                run.regions = regions;
            }
            current
        });
        let Some(current) = current else { return };

        if current {
            self.sink().push(frame);
            if self.frame_size.get_untracked() != Some(size) {
                self.frame_size.set(Some(size));
                let bounds = self.map.with_untracked(|map| map.placement().map(|placement| placement.bounds));
                if let Some(bounds) = bounds {
                    self.place(bounds);
                }
            }
        }
        self.after_renderer_returned();
    }

    /// Starts what waited on the renderer: an export asked for while it was busy, or else the
    /// newest frame asked for.
    fn after_renderer_returned(self) {
        let Some(export) = self.with_run(|run| run.pending_export.take()) else { return };
        if let Some(target) = export {
            self.start_export(target);
            return;
        }
        if self.with_run(|run| std::mem::take(&mut run.redraw_wanted)) == Some(true) {
            self.draw();
        }
    }

    /// Puts back the renderer an export held, unless a layout change replaced it meanwhile.
    fn give_back(self, generation: Generation, renderer: SharedPreviewRenderer) {
        self.with_run(|run| {
            if run.renderer_generation == generation {
                run.renderer = Some(renderer);
            }
        });
    }

    /// Puts the frame in front of the session, where this end is the one the rest watches.
    fn broadcast(self, track: &Track, at: FrameIndex) {
        let Some((shared, replay_id)) = self.run.try_with_value(|run| (run.shared, run.replay_id)) else { return };
        if !shared {
            return;
        }
        let Some(commands) = track.commands(at) else { return };
        let broadcast = frame_broadcast(replay_id, track, at, commands);
        self.collab.with_untracked(|link| link.broadcast_frame(broadcast));
    }

    /// The transport's keyboard. Returns whether the key was taken.
    pub fn key(self, key: &Key, unmodified: &Key, modifiers: Modifiers) -> bool {
        if !self.status.with_untracked(|status| matches!(status, Status::Ready(_))) {
            return false;
        }
        let has_tool = self.annotator.with_untracked(Annotator::has_tool);
        if modifiers.primary()
            && let Key::Character(text) = unmodified
        {
            let text = text.to_lowercase();
            if text == "z" && !modifiers.shift() {
                self.annotate(|annotator, link| annotator.undo(link));
                return true;
            }
            if let Some(tool) = tool_for_key(&text) {
                self.take_up(tool);
                return true;
            }
        }
        let exporting = self.export.with_untracked(|export| export.progress.is_some());
        match key {
            // The transport refuses while an export holds the renderer, as its buttons do.
            Key::Named(
                NamedKey::Space | NamedKey::ArrowUp | NamedKey::ArrowDown | NamedKey::ArrowLeft | NamedKey::ArrowRight,
            ) if exporting => return false,
            Key::Named(NamedKey::Space) => self.toggle_playing(),
            Key::Named(NamedKey::ArrowUp) => self.step_speed(1),
            Key::Named(NamedKey::ArrowDown) => self.step_speed(-1),
            Key::Named(NamedKey::ArrowLeft) if !modifiers.shift() => self.seek_by(-SEEK_STEP),
            Key::Named(NamedKey::ArrowRight) if !modifiers.shift() => self.seek_by(SEEK_STEP),
            Key::Named(NamedKey::ArrowLeft) => self.jump_to_previous_event(),
            Key::Named(NamedKey::ArrowRight) => self.jump_to_next_event(),
            // Puts the tool down and drops what was half-drawn.
            Key::Named(NamedKey::Escape) if has_tool => self.take_up(Tool::None),
            Key::Named(NamedKey::Delete | NamedKey::Backspace) => {
                self.annotate(|annotator, link| annotator.erase_picked(link));
            }
            Key::Character(text) if has_tool && text.as_str() == "[" => {
                self.annotate(|annotator, _| annotator.step_nib(-1.0));
            }
            Key::Character(text) if has_tool && text.as_str() == "]" => {
                self.annotate(|annotator, _| annotator.step_nib(1.0));
            }
            _ => return false,
        }
        true
    }

    /// Whether an export would have to encode in software although the reader did not ask for it,
    /// which is worth saying before a wait several times as long.
    pub fn export_needs_notice(self) -> bool {
        let settings = self.export.with_untracked(|export| export.settings);
        must_use_cpu(&settings, encoder_status()) && !settings.prefer_cpu
    }

    /// Asks where to write, then encodes the baked track there, or to a temporary file handed to
    /// the clipboard.
    pub fn start_export(self, target: ExportTarget) {
        if self.export.with_untracked(|export| export.progress.is_some()) {
            return;
        }
        let Some(track) = self.track_untracked() else { return };
        if track.is_empty() {
            return;
        }
        let settings = self.export.with_untracked(|export| export.settings);
        let frames = self.layers.with_untracked(|layers| {
            self.choices.with_untracked(|choices| {
                export_frames(&track, &settings, &layers.options, layers.show_dead_ships, choices)
            })
        });
        // Held for the whole encode, which is why the transport refuses while one runs.
        let Some((renderer, generation)) = self
            .with_run(|run| {
                let renderer = run.renderer.take();
                if renderer.is_none() {
                    // A frame is being drawn; the export starts as the draw hands the renderer back.
                    run.pending_export = Some(target);
                    run.ticker = None;
                }
                renderer.map(|renderer| (renderer, run.renderer_generation))
            })
            .flatten()
        else {
            self.playback.update(Playback::stop);
            return;
        };
        let Some((title, services, cancel)) =
            self.run.try_with_value(|run| (run.title.clone(), run.services, Arc::clone(&run.cancel)))
        else {
            return;
        };
        let duration = export_duration(frames.len());
        let suggested = suggested_name(&title);
        let total = frames.len() as u64;

        self.run.update_value(|run| run.ticker = None);
        self.playback.update(Playback::stop);
        self.export.update(|export| {
            export.failure = None;
            export.progress = Some(ExportProgress { done: 0, total, stage: ExportStage::Encoding });
        });

        self.spawn(async move {
            let output = match target {
                ExportTarget::File => {
                    let title = t!("ui.replay.renderer.export_video").into_owned();
                    let asked = wows_toolkit_services::dialog::save_file(
                        Some(&title),
                        &suggested,
                        Some(wows_toolkit_services::dialog::Filter { label: "MP4", extensions: &["mp4"] }),
                    )
                    .await;
                    match asked {
                        Ok(chosen) => chosen,
                        Err(err) => {
                            self.give_back(generation, renderer);
                            self.export.update(|export| {
                                export.progress = None;
                                export.failure = Some(err.to_string());
                            });
                            self.after_renderer_returned();
                            return;
                        }
                    }
                }
                // A temporary file that outlives this process, since the clipboard points at it.
                ExportTarget::Clipboard => temporary_output(&suggested),
            };
            let Some(output) = output else {
                self.give_back(generation, renderer);
                self.export.update(|export| export.progress = None);
                self.after_renderer_returned();
                return;
            };

            let job = services.jobs.start(Job { name: export_job_name(ExportStage::Encoding), progress: None });
            let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded::<ExportProgress>();
            let written = output.clone();
            // Dropping this task leaves the encode running, so closing the session stops it
            // through `cancel`.
            let encode = blocking(move || {
                let outcome = encode_track(&renderer, &frames, duration, &written, settings, &cancel, |step| {
                    let _ = progress_tx.unbounded_send(step);
                });
                (renderer, outcome)
            });
            let follow = async {
                use futures::StreamExt as _;
                while let Some(step) = progress_rx.next().await {
                    job.report(Job {
                        name: export_job_name(step.stage),
                        progress: Progress::new(step.done, step.total),
                    });
                    self.export.update(|export| export.progress = Some(step));
                }
            };
            let ((renderer, outcome), ()) = futures::join!(encode, follow);
            drop(job);

            self.give_back(generation, renderer);
            let failure = match outcome {
                Ok(()) if target == ExportTarget::Clipboard => copy_file_to_clipboard(output).err(),
                Ok(()) => None,
                Err(err) => Some(err.to_string()),
            };
            self.export.update(|export| {
                export.progress = None;
                export.failure = failure;
            });
            self.after_renderer_returned();
        });
    }

    /// Where a renderer's window was last, once that has been read.
    pub fn remembered_window(self) -> Option<wows_toolkit_config::WindowSettings> {
        self.run.try_with_value(|run| run.remembered).flatten()
    }

    fn read_remembered_window(self) {
        let Some(services) = self.run.try_with_value(|run| run.services) else { return };
        let Some(pool) = services.settings.pool() else { return };
        let read = services.on_tokio(async move { wows_toolkit_config::load_all_window_settings(&pool).await });
        self.spawn(async move {
            if let Ok(remembered) = read.await {
                let remembered = remembered.get(&wows_toolkit_config::WindowKind::ReplayRenderer).copied();
                self.with_run(|run| run.remembered = remembered);
            }
        });
    }

    /// Records that the viewport has a window to itself.
    pub fn mark_popped_out(self) {
        self.popped_out.set(true);
    }
}

/// Counts the starts of one kind of work, so a result that a newer start replaced is recognised
/// and dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Generation(u64);

impl Generation {
    /// Moves on to the next generation and returns it.
    fn next(&mut self) -> Self {
        self.0 += 1;
        *self
    }
}

/// What a bake needs, read as it starts.
struct BakeStart {
    request: RenderRequest,
    game_data: GameDataCache,
    cancel: Arc<AtomicBool>,
    generation: Generation,
}

/// A frame rasterised off the UI thread, and where its pieces landed on the canvas.
struct RasterOutput {
    frame: BgraFrame,
    size: FrameSize,
    regions: Vec<DrawnRegion>,
}

/// What the status bar calls an export at `stage`.
fn export_job_name(stage: ExportStage) -> String {
    format!("{}: {}", t!("ui.replay.renderer.export_video"), t!(stage.label_key()))
}

/// Puts `path` on the clipboard as a file, which is what a chat client pastes as an attachment.
fn copy_file_to_clipboard(path: PathBuf) -> Result<(), String> {
    arboard::Clipboard::new().and_then(|mut board| board.set().file_list(&[path])).map_err(|err| err.to_string())
}
