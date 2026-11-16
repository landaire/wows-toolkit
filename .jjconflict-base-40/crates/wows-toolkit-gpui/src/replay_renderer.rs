//! Playing a replay's battle back on the minimap, at whatever speed the
//! reader asks for.
//!
//! The egui app draws its renderer by turning each frame's draw commands into
//! egui shapes (`replay/renderer/`). This port has no painter of its own, so
//! it rasterises the same commands through
//! [`wows_minimap_renderer::preview::PreviewRenderer`] -- the path the hover
//! preview already uses -- and shows the resulting image.
//!
//! **What is kept, and why.** A whole battle's frames as images would be tens
//! of gigabytes, so the bake keeps the draw commands and one frame is
//! rasterised at a time. The commands themselves are bounded by
//! [`TrackSink`]'s budget: a long battle is sampled more coarsely rather than
//! costing more memory than a short one.
//!
//! **What it does not draw.** The panel commands (stats, rosters) and
//! position trails are outside `bake_options`, for the reason that module
//! documents. The egui renderer's annotation toolbar and video export are not
//! here either; see `docs/gpui-port-parity.md`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::armor_viewer::catalog::ShipEntry;
use crate::minimap_preview::SharedPreviewRenderer;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::Select;
use gpui_kit::component::select::SelectEvent;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::slider::SliderState;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::VideoCodec;
use wows_minimap_renderer::config::should_draw_command;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::viewport::MAX_ZOOM;
use wows_minimap_renderer::viewport::MIN_ZOOM;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replay_insights::timeline::EventTone;
use wows_replay_insights::timeline::KIND_COUNT;
use wows_replay_insights::timeline::PreExtractedHit;
use wows_replay_insights::timeline::TimelineEvent;
use wows_replay_insights::timeline::TimelineFilter;
use wows_replay_insights::timeline::format_timeline_event;
use wows_replay_insights::timeline::kind_label_key;
use wows_replay_insights::timeline::row_text;
use wows_replay_insights::timeline::row_tone;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::TeamId;
use wt_collab_client::drawing::Drawn;
use wt_collab_client::drawing::Stroke;
use wt_collab_client::drawing::Tool;

use crate::replay_inspector::GameDataCache;

/// How many frames of a battle the track keeps.
///
/// Past this the sink halves what it holds and samples half as often, so a
/// twenty-minute battle costs what a five-minute one does. Twelve hundred
/// frames is two minutes of game time at the bake rate below, and a whole
/// twenty-minute battle once the stride has doubled four times.
const TRACK_BUDGET: usize = 1200;

/// How much game time one baked frame covers.
const BAKE_INTERVAL: f32 = 0.5;

/// How often the viewport advances while playing, before the speed
/// multiplier. One frame of game time per tick at 1x.
const TICK: Duration = Duration::from_millis(500);

/// The speeds the transport offers, which are the egui renderer's own
/// (`PLAYBACK_SPEEDS`). A battle runs twenty minutes, so the useful range is
/// well above real time.
const SPEEDS: [f32; 6] = [1.0, 5.0, 10.0, 20.0, 40.0, 60.0];

/// The speed a viewport opens at, as the egui renderer opens at
/// (`SharedRendererState::speed`). Real time is too slow to watch a battle
/// through.
const DEFAULT_SPEED: f32 = 20.0;

/// How far the skip controls and the left and right keys move, in seconds of
/// game time. The egui renderer's own step.
const SEEK_STEP: f32 = 10.0;

/// What the viewport is doing.
enum State {
    /// Reading the replay and walking the battle.
    Baking,
    Ready(Track),
    Failed(String),
}

/// A baked battle: the commands of every kept frame, and when each was drawn.
struct Track {
    frames: Vec<Vec<DrawCommand>>,
    clocks: Vec<GameClock>,
    /// When the battle proper began. Recording starts at the loading screen,
    /// so the clock a frame carries runs ahead of the one the game showed by
    /// this much.
    battle_start: GameClock,
    /// When the battle ended, if the replay ran that far.
    battle_end: Option<GameClock>,
    /// How wide the map is in the game's own world units, which is what
    /// turns a ship's range in metres into a distance on the minimap.
    space_size: f32,
    /// The build this was recorded on, which some of a ship's ranges are
    /// gated on.
    version: wowsunpack::data::Version,
    /// Where the map's top-left corner sits in these frames.
    map_origin: (f32, f32),
    /// The map's space name, which a peer watching this playback loads art by.
    space: String,
}

impl Track {
    fn len(&self) -> usize {
        self.frames.len()
    }

    /// The game time `index` was drawn at, in seconds.
    fn seconds_at(&self, index: usize) -> f32 {
        self.clocks.get(index).map(|clock| clock.seconds()).unwrap_or(0.0)
    }

    /// How far into the battle `index` is, which is the clock the game showed.
    fn elapsed_at(&self, index: usize) -> f32 {
        self.seconds_at(index) - self.battle_start.seconds()
    }

    /// The last frame drawn at or before `seconds` of game time.
    ///
    /// Clocks ascend, so this is a partition point. A time before the first
    /// frame lands on that frame rather than nowhere.
    fn frame_at(&self, seconds: f32) -> usize {
        self.clocks.partition_point(|clock| clock.seconds() <= seconds).saturating_sub(1)
    }

    /// Where a clock sits along the track, as a fraction of its length.
    ///
    /// `None` when the track is too short to have a length to sit along.
    fn position_of(&self, clock: GameClock) -> Option<f32> {
        let last = self.len().checked_sub(1).filter(|last| *last > 0)?;
        Some((self.frame_at(clock.seconds()) as f32 / last as f32).clamp(0.0, 1.0))
    }
}

/// A replay played back on its own minimap.
pub struct ReplayRendererPanel {
    /// The replay being played, which is how a host keys its bookkeeping and
    /// what titles a window this viewport is popped out into.
    path: PathBuf,
    title: SharedString,
    state: State,
    /// The renderer the track is rasterised through, bound to the build and
    /// map this replay was recorded on, and shared with every other preview
    /// of the same map. Taken while a frame is being drawn on the background
    /// executor, which is also what keeps two rasters of the same frame from
    /// being asked for at once.
    renderer: Option<SharedPreviewRenderer>,
    /// The frame on screen. Stays put while the next one is drawn, so the
    /// viewport never blanks.
    frame: Option<Arc<RenderImage>>,
    /// Which frame of the track that is, or is being drawn for.
    at: usize,
    playing: bool,
    speed: f32,
    /// The speed control. A dropdown, as the egui renderer's is: six
    /// buttons take the width of the whole ladder to say one number.
    speed_select: Entity<SelectState<SearchableVec<SpeedItem>>>,
    _speed_select: Option<Subscription>,
    seek: Entity<SliderState>,
    /// Set when this panel is dropped, so a bake in flight stops walking a
    /// battle nobody is waiting for.
    cancel: Arc<AtomicBool>,
    _bake: Option<Task<()>>,
    _tick: Option<Task<()>>,
    _seek_subscription: Option<Subscription>,
    /// Held for the slider built once the track's length is known, which
    /// replaces the one the panel opened with.
    _rebuilt_seek: Option<Subscription>,
    /// The export under way, if any. Only one at a time: it holds the same
    /// renderer the viewport draws through.
    export: Option<ExportProgress>,
    /// Why the last export stopped, when it did not finish. Cleared when the
    /// next one starts.
    export_failure: Option<String>,
    _export: Option<Task<()>>,
    /// Whether this viewport has a window to itself rather than a dock tab.
    popped_out: bool,
    /// Whether ctrl is held, which is what puts the shortcut sheet up.
    ctrl_held: bool,
    /// Other recordings of this battle, baked alongside it so the map shows what
    /// the primary's team could not see.
    alts: Vec<PathBuf>,
    /// What the viewport draws of what it baked. Applied when a frame is
    /// rasterised rather than when it was baked, so a toggle takes effect on
    /// the next frame without walking the battle again.
    options: RenderOptions,
    /// Whether ships that have sunk are still drawn. Not one of
    /// [`RenderOptions`]: the egui viewer keeps it beside them for the same
    /// reason, since it gates a command rather than a layer.
    show_dead_ships: bool,
    /// Which part of the map the viewport shows.
    view: MapViewport,
    /// The zoom control beside the transport, which moves with the wheel.
    zoom: Entity<SliderState>,
    _zoom_subscription: Option<Subscription>,
    /// Where the frame sits on screen, recorded as it is painted. A pointer
    /// position means nothing without it, and only the painter knows.
    drawn: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// What the frame on screen drew that a pointer can rest on. Comes back
    /// with the frame, because the renderer that drew it is shared.
    regions: Vec<wows_minimap_renderer::drawing::DrawnRegion>,
    /// The consumable icon under the pointer, and where to anchor its
    /// reading.
    hovered_consumable: Option<(ConsumableHover, Point<Pixels>)>,
    /// Where a drag of the map last was, in window coordinates.
    dragging: Option<Point<Pixels>>,
    /// The map point last reported to the session, so an unmoved pointer is
    /// not sent again.
    reported_cursor: Option<[f32; 2]>,
    /// This viewport's end of a collab session. Inert until one is running.
    collab: crate::collab::CollabLink,
    /// What this playback is called in a session, so a peer can ask to watch
    /// this one rather than another window.
    replay_id: crate::collab::ReplayId,
    /// Whether the session has been told this playback is open. Said once, and
    /// again if a session starts after it was opened.
    shared: bool,
    /// The tool in hand and whatever it has drawn so far.
    drawing: wt_collab_client::drawing::Drawing,
    /// Where the pointer last was on the map, which is what a part-drawn
    /// shape is previewed against.
    pointer_at: Option<[f32; 2]>,
    /// What the reader has picked out to move, with no tool in hand.
    picked: wt_collab_client::drawing::Selection,
    /// Where a move of the picked shapes began, in map space, and what they
    /// looked like then. Kept so a move is one update at the end rather than
    /// one per pixel of the drag.
    moving: Option<([f32; 2], Vec<wt_collab_client::types::Annotation>)>,
    /// The shape being turned by its handle, and what it was before.
    turning: Option<(usize, wt_collab_client::types::Annotation)>,
    /// The box a ship is looked up in, to give a placed one an identity.
    ship_search: Entity<InputState>,
    _ship_search: Option<Subscription>,
    /// What has been typed into it, and what that matched. Recomputed as it
    /// is typed rather than while the popover is built, which happens over
    /// and over and cannot reach the catalogue mutably.
    ship_query: String,
    matched_ships: Vec<(wowsunpack::game_params::types::Species, ShipEntry)>,
    /// Every ship the loaded build knows, built once when first asked for.
    ship_catalog: Option<std::rc::Rc<crate::armor_viewer::catalog::ShipCatalog>>,
    /// What the session held before each change this viewport made, newest
    /// last. Bounded: a session left open all evening must not grow a
    /// history of every stroke in it.
    history: Vec<Vec<wt_collab_client::drawing::Held>>,
    /// The redraw a running session needs, held only while there is one.
    _collab_tick: Option<Task<()>>,
    /// Kept so the viewport can build the canvas a layer asks for: the team
    /// rosters need gutters the canvas it opened with does not have.
    game_data: Option<GameDataCache>,
    /// The canvas the frame on screen was drawn on.
    layout: wows_minimap_renderer::drawing::SidePanelLayout,
    /// What an export is encoded with.
    export_settings: ExportSettings,
    /// Ships whose trail the reader has hidden, by player name, which is what
    /// a trail command carries.
    trail_hidden: HashSet<String>,
    /// Which ranges the reader wants for a ship, by player name. A ship with
    /// no entry shows none, which is what keeps the map readable until they
    /// ask for one.
    ship_ranges: HashMap<String, ShipConfigFilter>,
    /// The ship a context menu is open for, and where it was opened.
    menu_for: Option<ShipMenu>,
    /// Which of the battle's events the timeline shows.
    event_filter: TimelineFilter,
    /// The timeline's search box.
    event_search: Entity<InputState>,
    _event_search: Option<Subscription>,
    /// Which side the reader was on, which is what makes a row friendly. Read
    /// with the events.
    viewer_team: Option<TeamId>,
    /// Whether the second walk has finished, which is what the timeline says
    /// instead of an empty list while it runs.
    events_read: bool,
    /// What happened in the battle, once the second walk has read it. Empty
    /// until then, which is what leaves the event controls refused.
    events: Vec<TimelineEvent>,
    /// What each ship took, from the same walk. Keyed by the entity the
    /// frame's own ship commands carry, so a ship the reader points at is
    /// looked up directly.
    shots: HashMap<EntityId, wows_replay_insights::timeline::ShipShotTimeline>,
    /// The ship an armor viewer was opened on, and what it had taken when it
    /// was last told. Kept so playback only disturbs the viewer when the set
    /// of hits actually changes: showing them again rebuilds the hull's
    /// meshes, which is far too much work for every frame.
    armor_following: Option<(EntityId, crate::armor_viewer::realtime::RealtimeArmorFeed)>,
    _events: Option<Task<()>>,
    /// Set when a frame was asked for while one was still being drawn. The
    /// draw in flight starts another as it finishes, so what ends up on
    /// screen is the last thing asked for rather than the first.
    redraw_wanted: bool,
    focus_handle: FocusHandle,
}

/// What a hover over a roster's consumable icon reads, and whose it is.
struct ConsumableHover {
    player: String,
    lines: wows_minimap_renderer::draw_command::ConsumableLines,
}

/// How far an export has got, in frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportProgress {
    pub done: u64,
    pub total: u64,
    pub stage: ExportStage,
}

/// Which part of an export is running.
///
/// Muxing happens after the last frame and can take a noticeable while on a
/// long battle, so saying so is the difference between a bar that has
/// stopped and one that is nearly done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStage {
    Encoding,
    Muxing,
}

impl ExportStage {
    /// What the overlay calls it, worded as the egui renderer words it.
    const fn label(self) -> &'static str {
        match self {
            Self::Encoding => "ui.renderer.encoding",
            Self::Muxing => "ui.renderer.muxing",
        }
    }
}

impl EventEmitter<PanelEvent> for ReplayRendererPanel {}

/// Who was firing at one ship, and with what.
///
/// Read off the battle rather than off the hits, because the filter has to be
/// able to offer an attacker before any of their shells have landed.
#[derive(Clone, Debug, Default)]
pub struct IncomingContext {
    /// Every enemy of the ship being looked at, named as the log lists them.
    /// The keys are also what counts as incoming fire.
    pub attackers: std::collections::BTreeMap<EntityId, String>,
    /// Every hit that ship takes over the whole battle, which is what the log
    /// lists: it is a reading of the battle rather than of where playback is.
    pub taken: Vec<PreExtractedHit>,
    /// That ship's health over the battle, as the strip beside the log draws
    /// it. `None` for a ship nothing recorded the health of.
    pub health: Option<wows_toolkit_viewmodel::armor::health_strip::HealthStrip>,
    /// Where playback stood when the viewer was asked for, so the strip marks it
    /// straight away rather than waiting for playback to move.
    pub at: Option<wows_replays::types::GameClock>,
    /// The main battery shells of every ship in the battle, so secondaries can
    /// be told apart from them. Empty is not knowledge that none are main
    /// battery, and the filter reads it that way.
    pub main_battery: std::collections::HashSet<wows_replays::types::GameParamId>,
    /// The shell each landed salvo was fired with, so the viewer can send that
    /// shell through the hull itself and say whether it agrees with the server.
    /// A salvo whose shell this build cannot name is left out rather than
    /// simulated with a stand-in.
    pub shells: std::collections::HashMap<wows_replays::types::GameParamId, wowsunpack::game_params::types::ShellInfo>,
}

/// What the viewport asks of whoever is hosting it.
#[derive(Clone, Debug)]
pub enum RendererEvent {
    /// Move this viewport into a window of its own. The dock cannot do that
    /// itself: it does not own the window list.
    PopOut,
    /// Show a ship's armor with what it had taken by where playback is.
    ///
    /// The viewport owns neither the armor viewer nor the tab it sits in, so
    /// it says which ship and hands over the hits rather than opening
    /// anything itself.
    ShowArmor {
        param_index: String,
        display_name: String,
        hits: Vec<PreExtractedHit>,
        /// Who was shooting at that ship, so the viewer can say which salvo a
        /// hit came from and offer one attacker at a time.
        incoming: Box<IncomingContext>,
    },
    /// Playback moved, and the ship an armor viewer is already open on has
    /// taken different hits by this point.
    ///
    /// Separate from [`Self::ShowArmor`] because it must not pull the reader
    /// back to the armor tab: they asked for the viewer once, not on every
    /// frame.
    ArmorFollowed {
        hits: Vec<PreExtractedHit>,
        health: Option<f32>,
        /// Where playback has reached, which the health strip marks.
        at: GameClock,
    },
}

impl EventEmitter<RendererEvent> for ReplayRendererPanel {}

impl ReplayRendererPanel {
    /// Opens a viewport on `path` and starts baking it.
    pub fn new(
        path: PathBuf,
        alts: Vec<PathBuf>,
        title: SharedString,
        game_data: GameDataCache,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let seek = cx.new(|_| seek_slider(0));
        let seek_subscription = cx.subscribe_in(&seek, window, Self::on_seek);
        let zoom = cx.new(|_| zoom_slider());
        let speed_select = cx.new(|cx| {
            let ladder = SearchableVec::new((0..SPEEDS.len()).map(SpeedItem).collect::<Vec<_>>());
            SelectState::new(ladder, Some(IndexPath::new(speed_index(DEFAULT_SPEED))), window, cx).searchable(false)
        });
        let speed_subscription = cx.subscribe_in(&speed_select, window, |this, _state, event, window, cx| {
            // `Confirm(None)` is the cleared case, which this control cannot
            // produce: it is not cleanable and always holds a speed.
            let SelectEvent::Confirm(Some(index)) = event else { return };
            let Some(speed) = SPEEDS.get(*index).copied() else { return };
            this.set_speed(speed, window, cx);
        });
        let zoom_subscription = cx.subscribe_in(&zoom, window, Self::on_zoom);
        let event_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.replay.timeline_search_hint").into_owned()));
        let ship_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.renderer.annotations.ship_hint").into_owned()));
        let ship_search_subscription = cx.subscribe(&ship_search, |this, state, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.ship_query = state.read(cx).value().to_string();
                this.matched_ships = this.matching_ships();
                cx.notify();
            }
        });
        let event_search_subscription = cx.subscribe(&event_search, |this, state, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.event_filter.search = state.read(cx).value().to_string();
                cx.notify();
            }
        });

        let mut panel = Self {
            path: path.clone(),
            title,
            state: State::Baking,
            renderer: None,
            frame: None,
            at: 0,
            playing: false,
            export: None,
            export_failure: None,
            _export: None,
            popped_out: false,
            ctrl_held: false,
            alts: Vec::new(),
            // The options the track was baked under, so what is drawn at
            // first is exactly what is in it.
            options: playback_options_hidden(),
            show_dead_ships: true,
            view: MapViewport::default(),
            zoom,
            _zoom_subscription: Some(zoom_subscription),
            drawn: Rc::new(Cell::new(None)),
            regions: Vec::new(),
            hovered_consumable: None,
            dragging: None,
            reported_cursor: None,
            collab: crate::collab::CollabLink::default(),
            replay_id: crate::collab::ReplayId::fresh(),
            shared: false,
            drawing: wt_collab_client::drawing::Drawing::new(DEFAULT_INK, DEFAULT_NIB),
            pointer_at: None,
            picked: wt_collab_client::drawing::Selection::default(),
            moving: None,
            turning: None,
            ship_search,
            _ship_search: Some(ship_search_subscription),
            ship_query: String::new(),
            matched_ships: Vec::new(),
            ship_catalog: None,
            history: Vec::new(),
            _collab_tick: None,
            game_data: None,
            layout: wows_minimap_renderer::drawing::SidePanelLayout::None,
            export_settings: ExportSettings::default(),
            trail_hidden: HashSet::new(),
            ship_ranges: HashMap::new(),
            menu_for: None,
            event_filter: TimelineFilter::default(),
            event_search,
            _event_search: Some(event_search_subscription),
            viewer_team: None,
            events_read: false,
            events: Vec::new(),
            shots: HashMap::new(),
            armor_following: None,
            _events: None,
            redraw_wanted: false,
            speed: DEFAULT_SPEED,
            speed_select,
            _speed_select: Some(speed_subscription),
            seek,
            cancel: Arc::new(AtomicBool::new(false)),
            _bake: None,
            _tick: None,
            _seek_subscription: Some(seek_subscription),
            _rebuilt_seek: None,
            focus_handle: cx.focus_handle(),
        };
        panel.game_data = Some(game_data.clone());
        panel.alts = alts;
        panel.start_bake(path.clone(), game_data.clone(), cx);
        panel.start_event_scan(path, game_data, cx);
        panel
    }

    /// Which other recordings this viewport was baked with.
    pub(crate) fn baked_alts(&self) -> &[PathBuf] {
        &self.alts
    }

    /// Bakes this battle again through another set of recordings, for a tab
    /// opened before one of them was added.
    pub(crate) fn rebake_with_alts(&mut self, alts: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else { return };
        self.alts = alts;
        let path = self.path.clone();
        self.start_bake(path, game_data, cx);
    }

    /// A viewport already holding `clocks`' worth of empty frames, with no
    /// bake behind it and no renderer to rasterise through.
    ///
    /// Test-only: production viewports reach this state through a bake. The
    /// transport is what this exercises -- where playback is, what the clock
    /// reads, where the bar sits -- none of which needs a drawn frame.
    #[cfg(test)]
    pub(crate) fn ready_for_test(clocks: Vec<f32>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let seek = cx.new(|_| seek_slider(clocks.len().saturating_sub(1)));
        let seek_subscription = cx.subscribe_in(&seek, window, Self::on_seek);
        let zoom = cx.new(|_| zoom_slider());
        let speed_select = cx.new(|cx| {
            let ladder = SearchableVec::new((0..SPEEDS.len()).map(SpeedItem).collect::<Vec<_>>());
            SelectState::new(ladder, Some(IndexPath::new(speed_index(DEFAULT_SPEED))), window, cx).searchable(false)
        });
        let speed_subscription = cx.subscribe_in(&speed_select, window, |this, _state, event, window, cx| {
            // `Confirm(None)` is the cleared case, which this control cannot
            // produce: it is not cleanable and always holds a speed.
            let SelectEvent::Confirm(Some(index)) = event else { return };
            let Some(speed) = SPEEDS.get(*index).copied() else { return };
            this.set_speed(speed, window, cx);
        });
        let zoom_subscription = cx.subscribe_in(&zoom, window, Self::on_zoom);
        let event_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.replay.timeline_search_hint").into_owned()));
        let ship_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.renderer.annotations.ship_hint").into_owned()));
        let ship_search_subscription = cx.subscribe(&ship_search, |this, state, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.ship_query = state.read(cx).value().to_string();
                this.matched_ships = this.matching_ships();
                cx.notify();
            }
        });
        let event_search_subscription = cx.subscribe(&event_search, |this, state, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.event_filter.search = state.read(cx).value().to_string();
                cx.notify();
            }
        });
        Self {
            path: PathBuf::new(),
            title: SharedString::from("test"),
            state: State::Ready(Track {
                frames: vec![Vec::new(); clocks.len()],
                clocks: clocks.into_iter().map(GameClock).collect(),
                battle_start: GameClock(0.0),
                battle_end: None,
                // A middling map, so a range in metres lands somewhere
                // sensible on the minimap.
                space_size: 1400.0,
                version: wowsunpack::data::Version::base(99, 0, 0),
                space: String::new(),
                map_origin: (0.0, wows_minimap_renderer::HUD_HEIGHT as f32),
            }),
            renderer: None,
            frame: None,
            at: 0,
            playing: false,
            export: None,
            export_failure: None,
            _export: None,
            popped_out: false,
            ctrl_held: false,
            alts: Vec::new(),
            // The options the track was baked under, so what is drawn at
            // first is exactly what is in it.
            options: playback_options_hidden(),
            show_dead_ships: true,
            view: MapViewport::default(),
            zoom,
            _zoom_subscription: Some(zoom_subscription),
            drawn: Rc::new(Cell::new(None)),
            regions: Vec::new(),
            hovered_consumable: None,
            dragging: None,
            reported_cursor: None,
            collab: crate::collab::CollabLink::default(),
            replay_id: crate::collab::ReplayId::fresh(),
            shared: false,
            drawing: wt_collab_client::drawing::Drawing::new(DEFAULT_INK, DEFAULT_NIB),
            pointer_at: None,
            picked: wt_collab_client::drawing::Selection::default(),
            moving: None,
            turning: None,
            ship_search,
            _ship_search: Some(ship_search_subscription),
            ship_query: String::new(),
            matched_ships: Vec::new(),
            ship_catalog: None,
            history: Vec::new(),
            _collab_tick: None,
            game_data: None,
            layout: wows_minimap_renderer::drawing::SidePanelLayout::None,
            export_settings: ExportSettings::default(),
            trail_hidden: HashSet::new(),
            ship_ranges: HashMap::new(),
            menu_for: None,
            event_filter: TimelineFilter::default(),
            event_search,
            _event_search: Some(event_search_subscription),
            viewer_team: None,
            events_read: false,
            events: Vec::new(),
            shots: HashMap::new(),
            armor_following: None,
            _events: None,
            redraw_wanted: false,
            speed: DEFAULT_SPEED,
            speed_select,
            _speed_select: Some(speed_subscription),
            seek,
            cancel: Arc::new(AtomicBool::new(false)),
            _bake: None,
            _tick: None,
            _seek_subscription: Some(seek_subscription),
            _rebuilt_seek: None,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Replaces the frame on screen's commands.
    ///
    /// Test-only: a baked track carries these from the battle.
    #[cfg(test)]
    pub(crate) fn set_frame_commands_for_test(&mut self, commands: Vec<DrawCommand>) {
        let at = self.at;
        if let State::Ready(track) = &mut self.state
            && let Some(frame) = track.frames.get_mut(at)
        {
            *frame = commands;
        }
    }

    /// Says when the battle inside an already-ready track ran.
    ///
    /// Test-only: a baked track carries this from the replay itself.
    #[cfg(test)]
    pub(crate) fn set_battle_window(&mut self, start: f32, end: Option<f32>) {
        let State::Ready(track) = &mut self.state else { return };
        track.battle_start = GameClock(start);
        track.battle_end = end.map(GameClock);
    }

    /// Reads what happened in the battle, which the event controls step
    /// between.
    ///
    /// Its own task rather than part of the bake: it is a second walk of the
    /// replay, and the viewport is worth showing before it finishes.
    fn start_event_scan(&mut self, path: PathBuf, game_data: GameDataCache, cx: &mut Context<Self>) {
        self._events = Some(cx.spawn(async move |this, cx| {
            let read =
                cx.background_spawn(async move { crate::minimap_preview::extract_events(&path, &game_data) }).await;
            let _ = this.update(cx, |this, cx| {
                match read {
                    Ok((read, shots)) => {
                        this.events = read.events;
                        this.viewer_team = read.viewer_team;
                        this.shots = shots;
                        this.events_read = true;
                    }
                    // The viewport still plays; only the event controls are
                    // worse off, and they stay refused.
                    Err(reason) => tracing::warn!("replay renderer: the battle's events could not be read: {reason}"),
                }
                cx.notify();
            });
        }));
    }

    /// A viewport with `events` already read, at the elapsed clocks given.
    #[cfg(test)]
    fn seed_events_for_test(&mut self, at: &[f32]) {
        use wows_replay_insights::timeline::TimelineEventKind;
        use wows_replays::types::ElapsedClock;
        self.events = at
            .iter()
            .map(|seconds| TimelineEvent {
                clock: ElapsedClock(*seconds),
                kind: TimelineEventKind::AdvantageChanged { label: format!("at {seconds}"), is_friendly: true },
            })
            .collect();
    }

    /// Hands a viewport being built its end of the session.
    pub fn seed_collab(&mut self, link: crate::collab::CollabLink, cx: &mut Context<Self>) {
        self.collab = link;
        self.follow_collab(cx);
    }

    /// Hands this viewport a session to talk to, or takes one away.
    pub fn set_collab(&mut self, link: crate::collab::CollabLink, cx: &mut Context<Self>) {
        // What was drawn before stays on the map: a session starting under an
        // open viewport hears about it as though it had just been drawn, and one
        // ending leaves it where the reader can still see it.
        let carried = self.collab.annotations_held();
        self.collab = link;
        self.collab.adopt(carried);
        self.shared = false;
        self.share_playback(cx);
        self.follow_collab(cx);
        cx.notify();
    }

    /// Starts or stops the redraw a running session needs.
    ///
    /// A peer's pointer and their pings move without this app touching
    /// anything, and a view only redraws when it is told to. Without this a
    /// peer's cursor sits wherever it was when playback last moved, and a
    /// ping freezes part-way through its ripple and never leaves the map.
    /// The egui renderer reaches the same place by asking for a repaint
    /// while a ping is alive.
    fn follow_collab(&mut self, cx: &mut Context<Self>) {
        if !self.collab.is_active() {
            self._collab_tick = None;
            return;
        }
        if self._collab_tick.is_some() {
            return;
        }
        self._collab_tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(COLLAB_TICK).await;
                let running = this
                    .update(cx, |this, cx| {
                        if !this.collab.is_active() {
                            return false;
                        }
                        this.collab.drop_stale_pings(Duration::from_secs_f32(PING_SECONDS));
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !running {
                    return;
                }
            }
        }));
    }

    /// What the consumable icon under the pointer reads, over the frame.
    ///
    /// Anchored at the pointer rather than at the icon: a roster row's icons
    /// are a few pixels apart, and a reading over the row would cover the
    /// ones beside the one being read.
    fn consumable_reading(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let (hover, at) = self.hovered_consumable.as_ref()?;
        let theme = cx.theme();
        let lines = &hover.lines;
        let body = v_flex()
            .gap_0p5()
            .w(px(280.))
            .bg(theme.background)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .p_2()
            .child(div().text_sm().font_weight(FontWeight::BOLD).child(lines.name.clone()))
            .child(div().text_xs().child(hover.player.clone()))
            .child(div().text_xs().child(lines.charges.clone()))
            .children(lines.timing.clone().map(|line| div().text_xs().child(line)))
            .children(lines.active.clone().map(|line| div().text_xs().child(line)))
            .children((!lines.description.is_empty()).then(|| {
                v_flex()
                    .gap_1()
                    .child(crate::ui::rule_h(cx))
                    .child(div().text_xs().text_color(crate::theme::text_dim()).child(lines.description.clone()))
            }));
        Some(
            deferred(
                anchored()
                    .position(point(at.x + px(16.), at.y + px(16.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(body),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// Tells the session this playback is open, and that this end is the one to
    /// watch.
    ///
    /// The map art travels with it: a peer may not have the build the replay was
    /// recorded on, and it still has to draw the battle. Nothing happens until
    /// the replay has been read, because until then there is no map to name.
    fn share_playback(&mut self, cx: &mut Context<Self>) {
        if self.shared || !self.collab.broadcasts_frames() {
            return;
        }
        let Some(track) = self.track() else { return };
        let (space, version) = (track.space.clone(), track.version);
        if space.is_empty() {
            return;
        }
        self.shared = true;
        let replay_id = self.replay_id;
        let replay_name = self.title.to_string();
        let game_data = self.game_data.clone();
        let link = self.collab.clone();
        cx.background_spawn(async move {
            let art_png = game_data
                .as_ref()
                .and_then(|data| data.newest_loaded())
                .and_then(|loaded| wows_minimap_renderer::assets::load_map_image(&space, loaded.vfs()))
                .and_then(|art| {
                    let mut png = Vec::new();
                    art.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok().map(|()| png)
                });
            link.announce_replay(crate::collab::SharedReplay {
                replay_id,
                replay_name,
                map_label: wows_toolkit_viewmodel::tactics::naming::pretty_map_name(&space),
                map_name: space,
                game_version: version.to_path(),
                art_png,
            });
        })
        .detach();
    }

    /// Puts this frame in front of the session, where this app is the one the
    /// rest of it watches.
    ///
    /// Draw commands rather than pixels, so each peer draws the battle with its
    /// own art at its own size.
    fn broadcast_current(&self, commands: &[DrawCommand], track: &Track) {
        if !self.shared {
            return;
        }
        self.collab.broadcast_frame(wt_collab_client::peer::FrameBroadcast {
            replay_id: self.replay_id.raw(),
            clock: track.seconds_at(self.at),
            frame_index: self.at as u32,
            total_frames: track.len() as u32,
            game_duration: track.seconds_at(track.len().saturating_sub(1)),
            commands: commands.to_vec(),
        });
    }

    /// The handle a picked shape is turned by, drawn over the frame.
    ///
    /// An element rather than part of the frame: it is a control rather than
    /// something drawn on the map, so it keeps its size at every zoom and
    /// stays out of an exported video.
    fn rotation_handle_overlay(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let index = self.picked.single()?;
        let annotation = self.collab.annotations().get(index)?.clone();
        if !wt_collab_client::drawing::can_rotate(&annotation) {
            return None;
        }
        let (handle, anchor) = self.rotation_handle(&annotation)?;
        let _ = cx;
        Some(
            div()
                .absolute()
                .left(handle.0 - HANDLE_RADIUS)
                .top(handle.1 - HANDLE_RADIUS)
                .child(div().size(HANDLE_RADIUS * 2.0).rounded_full().bg(gpui_kit::rgb(0xFFFF64)))
                // The stem back to the shape, so the handle reads as
                // belonging to it rather than floating over the map.
                .child(
                    div()
                        .absolute()
                        .left(HANDLE_RADIUS)
                        .top(HANDLE_RADIUS)
                        .w(px(1.))
                        .h(anchor.1 - handle.1)
                        .bg(gpui_kit::rgb(0xFFFF64)),
                )
                .into_any_element(),
        )
    }

    /// How far an export has got, over the video.
    ///
    /// Over it rather than in the transport, because the transport is a row
    /// of controls that are all refused while one runs, and a number tucked
    /// in among them is not where a reader looks to see how long is left.
    fn export_overlay(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let progress = self.export?;
        let theme = cx.theme();
        let body = v_flex()
            .w(px(300.))
            .gap_1()
            .px_3()
            .py_2()
            .rounded(theme.radius)
            .bg(theme.background.opacity(0.85))
            .border_1()
            .border_color(theme.border);

        let body = if progress.total == 0 {
            // Nothing to measure against yet: the encoder is still starting.
            body.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new())
                    .child(div().text_xs().child(t!("ui.renderer.preparing_export").into_owned())),
            )
        } else {
            let done = progress.done.min(progress.total) as f32 / progress.total as f32;
            body.child(div().text_xs().child(format!(
                "{} ({}/{})",
                t!(progress.stage.label()),
                progress.done,
                progress.total
            )))
            .child(Progress::new("replay-renderer-export-progress").value(done * 100.0))
        };

        Some(
            div()
                .id("replay-renderer-export-overlay")
                .test_support()
                .absolute()
                .top_2()
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(body)
                .into_any_element(),
        )
    }

    /// Where the peers' pointers and their pings are, in element space.
    ///
    /// Drawn as elements over the frame rather than into it: they are not
    /// part of the battle, and they move between rasters.
    fn collab_overlay(&self) -> Vec<AnyElement> {
        let mut over: Vec<AnyElement> = Vec::new();
        for cursor in self.collab.peer_cursors() {
            let Some(pos) = cursor.pos else { continue };
            let Some((left, top)) = self.element_point((pos[0], pos[1])) else { continue };
            let color: gpui_kit::Hsla =
                rgb(u32::from_be_bytes([0, cursor.color[0], cursor.color[1], cursor.color[2]])).into();
            over.push(
                div()
                    .absolute()
                    .left(left)
                    .top(top)
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().size(px(8.)).rounded_full().bg(color))
                            .child(div().text_xs().text_color(color).child(cursor.name.clone())),
                    )
                    .into_any_element(),
            );
        }
        for ping in self.collab.peer_pings() {
            let Some((left, top)) = self.element_point((ping.pos[0], ping.pos[1])) else { continue };
            let color: gpui_kit::Hsla =
                rgb(u32::from_be_bytes([0, ping.color[0], ping.color[1], ping.color[2]])).into();
            // A ping is a ring that grows and fades over its first second, as
            // the egui renderer's ripple does.
            let age = ping.time.elapsed().as_secs_f32();
            if age > PING_SECONDS {
                continue;
            }
            let grown = PING_RADIUS * (0.3 + age / PING_SECONDS);
            over.push(
                div()
                    .absolute()
                    .left(px(left.as_f32() - grown))
                    .top(px(top.as_f32() - grown))
                    .size(px(grown * 2.0))
                    .rounded_full()
                    .border_2()
                    .border_color(color.opacity(1.0 - age / PING_SECONDS))
                    .into_any_element(),
            );
        }
        over
    }

    /// Where a map point lands inside the viewport element.
    ///
    /// The inverse of [`Self::map_point`], which is what puts a peer's cursor
    /// where they are pointing rather than where the pointer happens to be.
    /// `None` before the frame has been painted once, and for a point the
    /// viewport is not currently showing.
    fn element_point(&self, at: (f32, f32)) -> Option<(Pixels, Pixels)> {
        let scale = self.drawn_scale()?;
        let (left, top, _) = self.hud_strip()?;
        let view = self.shaped_view();
        let drawn = (view.x(at.0), view.y(at.1));
        let span = wows_minimap_renderer::MINIMAP_SIZE as f32;
        if drawn.0 < 0.0 || drawn.0 >= span || drawn.1 < 0.0 || drawn.1 >= span {
            return None;
        }
        let origin = self.track().map(|track| track.map_origin).unwrap_or(DEFAULT_MAP_ORIGIN);
        Some((px(left.as_f32() + (drawn.0 + origin.0) * scale), px(top.as_f32() + (drawn.1 + origin.1) * scale)))
    }

    /// Where the frame's HUD strip lands inside the viewport element.
    ///
    /// The frame is drawn to fit without stretching, so it is letterboxed:
    /// the strip is not at the element's own top edge.
    fn hud_strip(&self) -> Option<(Pixels, Pixels, Pixels)> {
        let bounds = self.drawn.get()?;
        let frame = self.frame.as_ref()?;
        let scale = self.drawn_scale()?;
        let size = frame.size(0);
        let drawn_width = size.width.0 as f32 * scale;
        let drawn_height = size.height.0 as f32 * scale;
        let left = (bounds.size.width.as_f32() - drawn_width) / 2.0;
        let top = (bounds.size.height.as_f32() - drawn_height) / 2.0;
        Some((px(left), px(top), px(HUD_STRIP_HEIGHT * scale)))
    }

    /// What the advantage reads, for the frame on screen.
    ///
    /// The label itself is drawn into the frame by the renderer, so there is
    /// no element of its own to rest a pointer on; the strip it sits in is
    /// what the reader hovers instead.
    pub(crate) fn advantage_hover(&self) -> Option<Vec<String>> {
        let track = self.track()?;
        let commands = track.frames.get(self.at)?;
        let breakdown = commands.iter().find_map(|command| match command {
            DrawCommand::TeamAdvantage { level: Some(_), breakdown, .. } => Some(breakdown),
            _ => None,
        })?;

        let mut lines = vec![t!("ui.renderer.advantage.breakdown").into_owned()];
        match wows_minimap_renderer::advantage::breakdown_rows(breakdown) {
            None => lines.push(t!("ui.renderer.advantage.team_eliminated").into_owned()),
            Some(rows) => {
                for row in rows {
                    lines.push(format!("{}  {}", t!(row.label_key), row.contribution));
                }
                if !breakdown.hp_data_reliable {
                    lines.push(t!("ui.renderer.advantage.hp_incomplete").into_owned());
                }
            }
        }
        Some(lines)
    }

    /// Tells the session where this app's pointer is on the map.
    ///
    /// Sent only while a session is running, and only when it has moved to a
    /// different map point: a pointer crossing a zoomed-in map would otherwise
    /// send a message per pixel.
    fn report_cursor(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.collab.is_active() {
            return;
        }
        let at = self.map_point(position).map(|(x, y)| [x, y]);
        let moved = match (self.reported_cursor, at) {
            (Some(was), Some(now)) => (was[0] - now[0]).abs() >= 1.0 || (was[1] - now[1]).abs() >= 1.0,
            (None, None) => false,
            _ => true,
        };
        if !moved {
            return;
        }
        self.reported_cursor = at;
        self.collab.report_cursor(at);
        let _ = cx;
    }

    /// Drops a ping where the pointer is, for everyone in the session.
    fn on_ping(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some((x, y)) = self.map_point(event.position) else { return };
        self.collab.send_ping([x, y]);
        let _ = cx;
    }

    /// Opens the per-ship menu on whatever was right-clicked.
    fn on_right_click(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.map_point(event.position) else { return };
        self.menu_for = self.ship_at(at).map(|menu| ShipMenu { at: event.position, ..menu });
        cx.notify();
    }

    /// Closes the per-ship menu.
    pub(crate) fn close_ship_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu_for.take().is_some() {
            cx.notify();
        }
    }

    /// The ship a menu is open for.
    pub(crate) fn ship_menu(&self) -> Option<&ShipMenu> {
        self.menu_for.as_ref()
    }

    /// The ship drawn nearest `at`, if one is close enough to have been meant.
    ///
    /// Reads the frame on screen rather than the whole track: a right-click
    /// picks what the reader can see.
    fn ship_at(&self, at: (f32, f32)) -> Option<ShipMenu> {
        let track = self.track()?;
        let commands = track.frames.get(self.at)?;
        let mut best: Option<(f32, ShipMenu)> = None;
        for command in commands {
            let (pos, player_name, entity_id) = match command {
                DrawCommand::Ship { pos, player_name, entity_id, .. }
                | DrawCommand::DeadShip { pos, player_name, entity_id, .. } => (pos, player_name, entity_id),
                _ => continue,
            };
            let Some(name) = player_name.clone() else { continue };
            let away = ((pos.x - at.0).powi(2) + (pos.y - at.1).powi(2)).sqrt();
            if away > PICK_RADIUS {
                continue;
            }
            if best.as_ref().is_none_or(|(nearest, _)| away < *nearest) {
                // The caller puts the menu where the pointer was; a pick has
                // no position of its own.
                best = Some((away, ShipMenu { entity_id: *entity_id, player_name: name, at: Point::default() }));
            }
        }
        best.map(|(_, menu)| menu)
    }

    /// Which ship the menu's entity was in, and what it is called, when the
    /// build knows.
    ///
    /// Read off the roster the frame already carries: a ship command says
    /// who is in it but not which ship it is, and the roster says both.
    fn armor_target(&self, entity_id: EntityId) -> Option<(String, String)> {
        use wowsunpack::game_params::types::GameParamProvider as _;

        let track = self.track()?;
        let commands = track.frames.get(self.at)?;
        let param_id = commands.iter().find_map(|command| {
            let DrawCommand::TeamRoster { rows, .. } = command else { return None };
            rows.iter().find(|row| row.entity_id == entity_id)?.ship_param_id
        })?;
        let loaded = self.game_data.as_ref()?.newest_loaded()?;
        let param = loaded.provider().game_param_by_id(param_id)?;
        Some((param.index().to_string(), param.name().to_string()))
    }

    /// What the menu's ship had taken by where playback is, for an armor
    /// viewer to draw.
    /// Asks for an armor viewer on the ship the menu is open for, and
    /// follows it from then on.
    pub(crate) fn show_armor(&mut self, entity_id: EntityId, cx: &mut Context<Self>) {
        let Some((param_index, display_name)) = self.armor_target(entity_id) else { return };
        let Some(timeline) = self.shots.get(&entity_id).cloned() else { return };

        let mut feed = crate::armor_viewer::realtime::RealtimeArmorFeed::new(timeline);
        feed.advance_to(GameClock(self.clock_of(self.at)));
        let mut incoming = self.incoming_context(entity_id);
        // The log reads the whole battle, not the part played so far: scrubbing
        // back would otherwise take salvos out of a list nobody was scrubbing.
        incoming.taken = feed.whole_battle().to_vec();
        self.resolve_incoming_shells(&mut incoming);
        incoming.health = feed.health_strip();
        incoming.at = Some(GameClock(self.clock_of(self.at)));
        cx.emit(RendererEvent::ShowArmor {
            param_index,
            display_name,
            hits: feed.taken().to_vec(),
            incoming: Box::new(incoming),
        });
        self.armor_following = Some((entity_id, feed));
        self.close_ship_menu(cx);
    }

    /// Who was shooting at `victim`, and which shells are main battery ones.
    ///
    /// Read off the roster the frame already carries, which is the only place
    /// that says which team a ship is on and what it is called at once.
    fn incoming_context(&self, victim: EntityId) -> IncomingContext {
        use wowsunpack::game_params::types::GameParamProvider as _;

        let mut context = IncomingContext::default();
        let Some(track) = self.track() else { return context };
        let Some(commands) = track.frames.get(self.at) else { return context };
        // One roster per team, so every one of them is read: the enemies are in
        // the roster the victim is not in.
        let rows: Vec<&wows_minimap_renderer::draw_command::RosterRow> = commands
            .iter()
            .filter_map(|command| {
                let DrawCommand::TeamRoster { rows, .. } = command else { return None };
                Some(rows.iter())
            })
            .flatten()
            .collect();
        let Some(victim_team) = rows.iter().find(|row| row.entity_id == victim).map(|row| row.team_id) else {
            return context;
        };

        for row in rows.iter().filter(|row| row.team_id != victim_team) {
            context.attackers.insert(row.entity_id, format!("{} ({})", row.player_name, row.ship_name));
        }

        let Some(loaded) = self.game_data.as_ref().and_then(|data| data.newest_loaded()) else { return context };
        let provider = loaded.provider();
        for row in &rows {
            let Some(ship) = row.ship_param_id.and_then(|id| provider.game_param_by_id(id)) else { continue };
            let Some(config) = ship.vehicle().and_then(|vehicle| vehicle.config_data()) else { continue };
            for name in &config.main_battery_ammo {
                if let Some(shell) = provider.game_param_by_name(name) {
                    context.main_battery.insert(shell.id());
                }
            }
        }
        context
    }

    /// Names the shell behind each salvo that landed on the ship.
    fn resolve_incoming_shells(&self, context: &mut IncomingContext) {
        let Some(loaded) = self.game_data.as_ref().and_then(|data| data.newest_loaded()) else {
            // Without game data no shell can be named, so no shell can be
            // simulated and the log carries no verdicts at all.
            tracing::warn!("armor viewer: no game data loaded, so the incoming shells cannot be named");
            return;
        };
        let provider = loaded.provider();
        for hit in &context.taken {
            let Some(salvo) = hit.hit.salvo.as_ref() else { continue };
            if context.shells.contains_key(&salvo.params_id) {
                continue;
            }
            if let Some(shell) = provider.resolve_shell_from_param_id(salvo.params_id) {
                context.shells.insert(salvo.params_id, shell);
            }
        }
    }

    /// Tells a viewer that is already open what its ship has taken by where
    /// playback has reached.
    ///
    /// Only when the set of hits has changed: telling it again rebuilds the
    /// hull's meshes, which is far more than a frame of playback is worth.
    fn follow_armor(&mut self, cx: &mut Context<Self>) {
        let clock = GameClock(self.clock_of(self.at));
        let Some((_, feed)) = self.armor_following.as_mut() else { return };
        let moved = feed.advance_to(clock);
        let changed = match moved {
            crate::armor_viewer::realtime::Advance::Rewound => true,
            crate::armor_viewer::realtime::Advance::Gained(gained) => !gained.is_empty(),
        };
        if !changed {
            return;
        }
        let (hits, health) = (feed.taken().to_vec(), feed.health());
        cx.emit(RendererEvent::ArmorFollowed { hits, health, at: clock });
    }

    /// The game clock frame `at` was drawn at.
    fn clock_of(&self, at: usize) -> f32 {
        self.track().map(|track| track.seconds_at(at)).unwrap_or_default()
    }

    /// Whether the menu's ship has anything an armor viewer could show.
    ///
    /// Only a ship the battle recorded hits on, since the viewer's whole
    /// point is where those landed.
    pub(crate) fn has_armor_to_show(&self, entity_id: EntityId) -> bool {
        self.shots.get(&entity_id).is_some_and(|timeline| !timeline.hits.is_empty())
            && self.armor_target(entity_id).is_some()
    }

    /// Whether `player` has a trail drawn.
    pub(crate) fn trail_shown(&self, player: &str) -> bool {
        !self.trail_hidden.contains(player)
    }

    /// Shows or hides one ship's trail.
    ///
    /// Turning one on turns the trails on, since a reader who asked for this
    /// ship's trail meant to see it; hiding the last one turns them off again
    /// rather than leaving an empty layer switched on.
    pub(crate) fn set_trail_shown(&mut self, player: &str, shown: bool, cx: &mut Context<Self>) {
        if shown {
            self.trail_hidden.remove(player);
            self.options.show_trails = true;
        } else {
            self.trail_hidden.insert(player.to_owned());
            if self.drawn_ships().iter().all(|name| self.trail_hidden.contains(name)) {
                self.options.show_trails = false;
            }
        }
        self.draw_current(cx);
        cx.notify();
    }

    /// Hides every trail but `player`'s.
    pub(crate) fn only_trail(&mut self, player: &str, cx: &mut Context<Self>) {
        self.trail_hidden = self.drawn_ships().into_iter().filter(|name| name != player).collect();
        self.options.show_trails = true;
        self.draw_current(cx);
        cx.notify();
    }

    /// Which ranges `player` has on.
    pub(crate) fn ranges_for(&self, player: &str) -> ShipConfigFilter {
        self.ship_ranges.get(player).copied().unwrap_or(NO_RANGES)
    }

    /// Changes which ranges one ship shows.
    pub(crate) fn set_ranges_for(
        &mut self,
        player: &str,
        apply: impl FnOnce(&mut ShipConfigFilter),
        cx: &mut Context<Self>,
    ) {
        let mut filter = self.ranges_for(player);
        apply(&mut filter);
        self.ship_ranges.insert(player.to_owned(), filter);
        // Ranges are drawn at all only while some ship has one.
        self.options.show_ship_config = self.ship_ranges.values().any(|filter| *filter != NO_RANGES);
        self.draw_current(cx);
        cx.notify();
    }

    /// Leaves only `player`'s ranges on.
    pub(crate) fn only_ranges(&mut self, player: &str, cx: &mut Context<Self>) {
        let keep = self.ranges_for(player);
        let keep = if keep == NO_RANGES { ALL_RANGES } else { keep };
        self.ship_ranges.clear();
        self.ship_ranges.insert(player.to_owned(), keep);
        self.options.show_ship_config = true;
        self.draw_current(cx);
        cx.notify();
    }

    /// Turns all of one ship's ranges on, or all of them off.
    ///
    /// The six switches one at a time is six clicks to ask a question that
    /// is usually all-or-nothing, which is why the egui menu offers both.
    pub(crate) fn set_every_range_for(&mut self, player: &str, on: bool, cx: &mut Context<Self>) {
        self.set_ranges_for(player, |filter| *filter = if on { ALL_RANGES } else { NO_RANGES }, cx);
    }

    /// Turns every ship's ranges on.
    pub(crate) fn all_ranges(&mut self, cx: &mut Context<Self>) {
        self.ship_ranges = self.drawn_ships().into_iter().map(|name| (name, ALL_RANGES)).collect();
        self.options.show_ship_config = true;
        self.draw_current(cx);
        cx.notify();
    }

    /// The ships drawn in the frame on screen, by player name.
    fn drawn_ships(&self) -> Vec<String> {
        let Some(track) = self.track() else { return Vec::new() };
        let Some(commands) = track.frames.get(self.at) else { return Vec::new() };
        commands
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Ship { player_name, .. } | DrawCommand::DeadShip { player_name, .. } => {
                    player_name.clone()
                }
                _ => None,
            })
            .collect()
    }

    /// The events the filter admits, newest last.
    pub(crate) fn visible_events(&self) -> Vec<&TimelineEvent> {
        self.events.iter().filter(|event| self.event_filter.matches(event)).collect()
    }

    /// Which of the battle's events the timeline shows.
    pub(crate) fn event_filter(&self) -> &TimelineFilter {
        &self.event_filter
    }

    /// Changes which events the timeline shows.
    pub(crate) fn set_event_filter(&mut self, apply: impl FnOnce(&mut TimelineFilter), cx: &mut Context<Self>) {
        apply(&mut self.event_filter);
        cx.notify();
    }

    /// Whether the battle has been read yet, which is what the timeline says
    /// instead of an empty list while the second walk runs.
    pub(crate) fn events_are_read(&self) -> bool {
        self.events_read
    }

    /// The event before where playback is, if there is one.
    ///
    /// Half a second back, as the egui renderer looks: without it, landing on
    /// an event and pressing back again would find the same one.
    fn previous_event(&self) -> Option<&TimelineEvent> {
        let here = self.elapsed_now()?;
        self.events.iter().rev().find(|event| event.clock.seconds() < here - 0.5)
    }

    fn next_event(&self) -> Option<&TimelineEvent> {
        let here = self.elapsed_now()?;
        self.events.iter().find(|event| event.clock.seconds() > here)
    }

    /// How far into the battle playback is, which is what an event's clock is
    /// measured against.
    fn elapsed_now(&self) -> Option<f32> {
        self.track().map(|track| track.elapsed_at(self.at))
    }

    /// Moves playback to `event` and says what it was.
    fn go_to_event(&mut self, at: f32, said: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(track) = self.track() else { return };
        let frame = track.frame_at(at + track.battle_start.seconds());
        self.go_to(frame, window, cx);
        crate::toast::info(said, window, cx);
    }

    /// Moves playback to `at` seconds into the battle.
    pub(crate) fn go_to_event_at(&mut self, at: f32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(track) = self.track() else { return };
        let frame = track.frame_at(at + track.battle_start.seconds());
        self.go_to(frame, window, cx);
    }

    /// Moves playback to the frame `clock` falls in.
    ///
    /// The clock is the battle's own, as the replay's timeline states it, which
    /// is what a salvo in the incoming-fire log is stamped with.
    pub(crate) fn go_to_clock(&mut self, clock: GameClock, window: &mut Window, cx: &mut Context<Self>) {
        let Some(track) = self.track() else { return };
        let frame = track.frame_at(clock.seconds());
        self.go_to(frame, window, cx);
    }

    /// Whether this viewport is the one feeding an armor viewer.
    pub(crate) fn is_following_armor(&self) -> bool {
        self.armor_following.is_some()
    }

    fn jump_to_previous_event(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = self.previous_event() else { return };
        let (at, said) = (event.clock.seconds(), format_timeline_event(event));
        self.go_to_event(at, said, window, cx);
    }

    fn jump_to_next_event(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = self.next_event() else { return };
        let (at, said) = (event.clock.seconds(), format_timeline_event(event));
        self.go_to_event(at, said, window, cx);
    }

    fn start_bake(&mut self, path: PathBuf, game_data: GameDataCache, cx: &mut Context<Self>) {
        let cancel = Arc::clone(&self.cancel);
        let alts = self.alts.clone();
        self._bake = Some(cx.spawn(async move |this, cx| {
            let baked = cx.background_spawn(async move { bake(&path, &alts, &game_data, &cancel) }).await;
            let _ = this.update(cx, |this, cx| {
                match baked {
                    Ok((track, renderer)) => {
                        this.renderer = Some(renderer);
                        this.state = State::Ready(track);
                        this.rebuild_seek(cx);
                        // Draws the first frame as it applies them, so nothing
                        // else has to ask for one.
                        this.adopt_saved_defaults(cx);
                    }
                    Err(err) => this.state = State::Failed(err.to_string()),
                }
                cx.notify();
            });
        }));
    }

    /// The replay this viewport is playing.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// What an export is encoded with.
    pub fn export_settings(&self) -> &ExportSettings {
        &self.export_settings
    }

    /// Changes what an export is encoded with.
    pub fn set_export_settings(&mut self, apply: impl FnOnce(&mut ExportSettings), cx: &mut Context<Self>) {
        apply(&mut self.export_settings);
        cx.notify();
    }

    /// The frames an export covers.
    ///
    /// A replay records the loading screen and the countdown before the battle
    /// proper; unless the reader asked for them, an export starts where the
    /// battle did.
    fn frames_to_export<'a>(&self, track: &'a Track) -> &'a [Vec<DrawCommand>] {
        if self.export_settings.include_pre_battle {
            return &track.frames;
        }
        let from = track.frame_at(track.battle_start.seconds());
        track.frames.get(from..).unwrap_or(&track.frames)
    }

    /// Opens this viewport showing what the reader last saved as the defaults.
    ///
    /// Runs once the bake has landed, because the rosters are a wider canvas
    /// rather than a layer and there is nothing to widen before then.
    fn adopt_saved_defaults(&mut self, cx: &mut Context<Self>) {
        let saved = crate::render_defaults::defaults(cx);
        let export = export_settings_of(&saved);
        self.set_export_settings(|settings| *settings = export, cx);
        self.set_options(
            |options, show_dead| {
                saved.apply_to(options);
                *show_dead = saved.show_dead_ships;
                // The stats panel's ship silhouettes are one of the loads a
                // bake skips, so its gutter has nothing to draw in it and the
                // viewport does not offer the layer at all.
                options.show_stats_panel = false;
            },
            cx,
        );
    }

    /// Writes what this viewport is showing back as what a viewport opens with.
    ///
    /// What this viewport has no control over is left as it was: the self
    /// ranges, which are per-ship here rather than one saved filter, and the
    /// stats panel, which a bake cannot fill and which this viewport therefore
    /// holds off whatever the reader chose in the egui renderer.
    fn remember_defaults(&self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = crate::render_defaults::defaults(cx);
        let mut saved = stored.clone();
        saved.read_from(&self.options);
        saved.show_stats_panel = stored.show_stats_panel;
        saved.show_dead_ships = self.show_dead_ships;
        saved.prefer_cpu_encoder = self.export_settings.prefer_cpu;
        saved.video_codec = self.export_settings.codec;
        saved.include_pre_battle = self.export_settings.include_pre_battle;

        if crate::render_defaults::remember(saved, cx) {
            crate::toast::ok(t!("ui.renderer.settings.save_defaults_done").into_owned(), window, cx);
        } else {
            crate::toast::failed(t!("ui.renderer.settings.save_defaults_failed").into_owned(), window, cx);
        }
    }

    /// What the viewport is drawing.
    pub fn options(&self) -> &RenderOptions {
        &self.options
    }

    pub fn show_dead_ships(&self) -> bool {
        self.show_dead_ships
    }

    /// Changes what is drawn and redraws the frame on screen.
    ///
    /// The frame is re-rasterised rather than waited for: a toggle whose
    /// effect only arrived with the next tick would read as not having worked
    /// while playback is paused.
    pub fn set_options(&mut self, apply: impl FnOnce(&mut RenderOptions, &mut bool), cx: &mut Context<Self>) {
        apply(&mut self.options, &mut self.show_dead_ships);
        self.follow_layout(cx);
        self.draw_current(cx);
        cx.notify();
    }

    /// Moves the viewport onto the canvas its layers now need.
    ///
    /// The rosters sit in gutters either side of the map, so turning them on
    /// is not a layer but a wider canvas. The renderer for one is kept, so
    /// switching back and forth costs nothing after the first time.
    fn follow_layout(&mut self, cx: &mut Context<Self>) {
        let wanted = crate::minimap_preview::layout_for(&self.options);
        if wanted == self.layout {
            return;
        }
        let Some(game_data) = self.game_data.clone() else {
            // Nothing to build a canvas from, so the layer cannot be shown.
            self.options.show_team_rosters = false;
            self.options.show_stats_panel = false;
            return;
        };
        match crate::minimap_preview::renderer_for_replay(&self.path, &game_data, wanted) {
            Ok((renderer, origin)) => {
                self.layout = wanted;
                self.renderer = Some(renderer);
                if let State::Ready(track) = &mut self.state {
                    track.map_origin = (origin.0 as f32, origin.1 as f32);
                }
                // The frame on screen was drawn on the old canvas, so it is
                // the wrong shape until the next one lands.
                self.redraw_wanted = true;
                let _ = cx;
            }
            Err(reason) => {
                // The layer stays off rather than drawing on a canvas with no
                // room for it.
                tracing::warn!("replay renderer: the canvas for {wanted:?} could not be built: {reason}");
                self.options.show_team_rosters = false;
                self.options.show_stats_panel = false;
            }
        }
    }

    /// Whether this viewport is in a window of its own, which is what hides
    /// the control that would put it in one.
    pub fn is_popped_out(&self) -> bool {
        self.popped_out
    }

    /// Records that this viewport now has a window to itself.
    pub fn mark_popped_out(&mut self, cx: &mut Context<Self>) {
        self.popped_out = true;
        cx.notify();
    }

    /// The number of frames in the baked track, or zero while it is baking.
    fn frame_count(&self) -> usize {
        self.track().map(Track::len).unwrap_or(0)
    }

    /// The baked battle, once the bake has landed.
    fn track(&self) -> Option<&Track> {
        match &self.state {
            State::Ready(track) => Some(track),
            _ => None,
        }
    }

    /// Moves playback to `at` and takes the bar with it.
    ///
    /// The transport's own controls move both; only the bar itself moves one
    /// without the other, since it is already where it put itself.
    fn go_to(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.set_at(at, cx);
        self.sync_seek(window, cx);
    }

    /// Moves playback `delta` seconds of game time from where it is.
    fn seek_by(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(track) = self.track() else { return };
        let at = track.frame_at(track.seconds_at(self.at) + delta);
        self.go_to(at, window, cx);
    }

    /// Moves one rung along the speed ladder, in the direction of `step`.
    ///
    /// Nothing happens at either end: the ladder is what the transport
    /// offers, and there is no speed past its ends to move to.
    fn step_speed(&mut self, step: i32, window: &mut Window, cx: &mut Context<Self>) {
        let here = SPEEDS.iter().position(|speed| (speed - self.speed).abs() < f32::EPSILON);
        let Some(here) = here else { return };
        let Some(next) = here.checked_add_signed(step as isize).and_then(|index| SPEEDS.get(index)) else {
            return;
        };
        self.set_speed(*next, window, cx);
    }

    /// The transport's keyboard, which is the egui renderer's own.
    ///
    /// A held key repeats, which is how a reader scrubs; only the keys the
    /// transport claims stop here, so the rest reach whatever else is
    /// listening.
    fn on_key(&mut self, event: &gpui_kit::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Ready(_)) {
            return;
        }
        let shift = event.keystroke.modifiers.shift;
        let ctrl = event.keystroke.modifiers.secondary();

        // The drawing tools, which were mouse-only here: the egui board takes
        // ctrl and a digit for each (`replay/minimap_view/shapes.rs:84`), and a
        // reader drawing over a battle has one hand on the mouse.
        if ctrl && let Some(tool) = tool_for_key(event.keystroke.key.as_str()) {
            self.take_up(tool, cx);
            cx.stop_propagation();
            return;
        }

        match event.keystroke.key.as_str() {
            "space" => self.toggle_playing(window, cx),
            "up" => self.step_speed(1, window, cx),
            "down" => self.step_speed(-1, window, cx),
            "left" if !shift => self.seek_by(-SEEK_STEP, window, cx),
            "right" if !shift => self.seek_by(SEEK_STEP, window, cx),
            "left" => self.jump_to_previous_event(window, cx),
            "right" => self.jump_to_next_event(window, cx),
            // Puts the tool down and drops what was half-drawn, which is what a
            // reader expects of escape with something in hand.
            "escape" if self.has_tool() => self.take_up(Tool::None, cx),
            // What is picked out goes, as it does on the egui board.
            "delete" | "backspace" => self.erase_picked(cx),
            // The nib, thinner and thicker, while a tool is in hand.
            "[" if self.has_tool() => self.step_nib(-1.0, cx),
            "]" if self.has_tool() => self.step_nib(1.0, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Asks for the current frame to be drawn, unless one already is.
    ///
    /// The renderer is moved onto the background executor for the draw and
    /// moved back with the image, so a tick that arrives while a frame is
    /// still being drawn simply does nothing and the next one catches up.
    fn draw_current(&mut self, cx: &mut Context<Self>) {
        let Some(renderer) = self.renderer.take() else {
            // A drag asks for frames faster than one can be drawn. Dropping
            // the last of them leaves the map showing a zoom the reader has
            // already moved past.
            self.redraw_wanted = true;
            return;
        };
        let State::Ready(track) = &self.state else {
            self.renderer = Some(renderer);
            return;
        };
        let trails = if self.options.show_trails {
            wows_minimap_renderer::frame_track::trails_through(&track.frames[..=self.at.min(track.len() - 1)])
        } else {
            Vec::new()
        };
        let Some(commands) = track.frames.get(self.at).cloned() else {
            self.renderer = Some(renderer);
            return;
        };
        self.broadcast_current(&commands, track);

        let options = self.options.clone();
        let show_dead_ships = self.show_dead_ships;
        let view = self.shaped_view();
        let trail_hidden = self.trail_hidden.clone();
        let ship_ranges = self.ship_ranges.clone();
        // Drawn over the battle rather than by it, so they are not filtered
        // with it and they go on last.
        let mut drawn_on: Vec<DrawCommand> =
            self.collab.annotations().iter().flat_map(wt_collab_client::geometry::annotation_commands).collect();
        drawn_on.extend(self.placed_ship_ranges());
        // The shape under the pointer is drawn the same way the finished one
        // will be, so what a reader sees while dragging is what they get.
        if let Some(at) = self.pointer_at
            && let Some(part_drawn) = self.drawing.in_progress(at)
        {
            drawn_on.extend(wt_collab_client::geometry::annotation_commands(&part_drawn));
        }
        cx.spawn(async move |this, cx| {
            let drawn = cx.background_spawn(async move {
                // Filtered here rather than at bake time: a toggle then costs
                // one frame rather than another walk of the battle.
                // Trails first, so they sit behind everything, as the egui
                // renderer draws them.
                let mut shown: Vec<DrawCommand> = trails
                    .into_iter()
                    .chain(commands)
                    .filter(|command| should_draw_command(command, &options, show_dead_ships))
                    .filter(|command| per_ship_allows(command, &trail_hidden, &ship_ranges))
                    .collect();
                // After the battle's own map layers, so a drawn line is not
                // buried under a ship, and before the HUD, which the target
                // puts on top of everything on the map.
                shown.extend(drawn_on);
                let (image, regions) = {
                    let mut drawing = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let (frame, regions) = drawing.render_regions(view, &shown);
                    (to_image(frame), regions)
                };
                (renderer, image, regions)
            });
            let (renderer, image, regions) = drawn.await;
            let _ = this.update(cx, |this, cx| {
                this.renderer = Some(renderer);
                this.frame = Some(image);
                this.regions = regions;
                cx.notify();
                if std::mem::take(&mut this.redraw_wanted) {
                    this.draw_current(cx);
                }
            });
        })
        .detach();
    }

    /// Starts an export, asking first when it would encode in software.
    ///
    /// A GPU encode of a long battle takes a while; a software one takes several
    /// times that, which is worth knowing before the wait rather than after.
    fn export_asking_first(&mut self, target: ExportTarget, window: &mut Window, cx: &mut Context<Self>) {
        if !must_use_cpu(&self.export_settings, encoder_status()) || self.export_settings.prefer_cpu {
            self.export(target, cx);
            return;
        }
        // Weak: the viewport can be closed while the notice is up, and the
        // dialog holds this until it is answered.
        let owner = cx.entity().downgrade();
        crate::notices::before_software_encode(window, cx, move |_window, cx| {
            let Some(owner) = owner.upgrade() else { return };
            owner.update(cx, |panel, cx| panel.export(target, cx));
        });
    }

    /// Asks where to write, then encodes the baked track there.
    ///
    /// The frames are the ones already baked, so nothing is parsed twice: the
    /// track is rasterised through the same renderer the viewport draws with,
    /// and the pixels go straight to the encoder. A clipboard export writes a
    /// temporary file and leaves it behind deliberately: the clipboard holds a
    /// path, and deleting what it points at would paste nothing.
    fn export(&mut self, target: ExportTarget, cx: &mut Context<Self>) {
        if self.export.is_some() {
            return;
        }
        let State::Ready(track) = &self.state else { return };
        if track.frames.is_empty() {
            return;
        }
        // Held for the whole encode, which is why the transport is refused
        // while one runs: a frame cannot be drawn for two things at once.
        let Some(renderer) = self.renderer.take() else { return };

        let settings = self.export_settings;
        // What is on screen, not what was baked: a layer the reader turned off
        // should not come back in the file. The overlays drawn over a battle --
        // trails, annotations, the ranges placed on the map -- are derived at
        // draw time and are not written.
        let frames: Vec<Vec<DrawCommand>> = self
            .frames_to_export(track)
            .iter()
            .map(|frame| {
                frame
                    .iter()
                    .filter(|command| should_draw_command(command, &self.options, self.show_dead_ships))
                    .filter(|command| per_ship_allows(command, &self.trail_hidden, &self.ship_ranges))
                    .cloned()
                    .collect()
            })
            .collect();
        // What the video covers, which is not the whole track when the
        // pre-battle phase is left out.
        let duration = (frames.len().saturating_sub(1)) as f32 * BAKE_INTERVAL;
        let suggested = format!("{}.mp4", self.title);
        // The clipboard needs a file, not a place to put one, so it is not
        // asked for.
        let asked: Option<_> = match target {
            ExportTarget::File => {
                Some(crate::dialog::save_file(Some(&t!("ui.replay.renderer.export_video")), &suggested, Some(MP4)))
            }
            ExportTarget::Clipboard => None,
        };

        self.export_failure = None;
        self.export = Some(ExportProgress { done: 0, total: frames.len() as u64, stage: ExportStage::Encoding });
        self.playing = false;
        cx.notify();

        let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded::<ExportProgress>();
        let watched = cx.entity();
        cx.spawn(async move |_this, cx| {
            use futures::StreamExt as _;
            while let Some(step) = progress_rx.next().await {
                watched.update(cx, |this, cx| {
                    this.export = Some(step);
                    cx.notify();
                });
            }
        })
        .detach();

        self._export = Some(cx.spawn(async move |this, cx| {
            let chosen = match asked {
                Some(dialog) => dialog.await,
                // A temporary file that outlives this process, since the
                // clipboard will point at it.
                None => temporary_output(&suggested),
            };
            let Some(output) = chosen else {
                // Cancelled at the dialog, or no temporary file could be
                // made: the renderer goes back to the viewport and nothing
                // else changes.
                let _ = this.update(cx, |this, cx| {
                    this.renderer = Some(renderer);
                    this.export = None;
                    cx.notify();
                });
                return;
            };

            let written = output.clone();
            let (renderer, outcome) = cx
                .background_spawn(async move {
                    let outcome = encode_track(&renderer, &frames, duration, &written, settings, progress_tx);
                    (renderer, outcome)
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.renderer = Some(renderer);
                this.export = None;
                match outcome {
                    Ok(()) if target == ExportTarget::Clipboard => {
                        if let Err(err) =
                            arboard::Clipboard::new().and_then(|mut board| board.set().file_list(&[output]))
                        {
                            this.export_failure = Some(err.to_string());
                        }
                    }
                    Ok(()) => {}
                    Err(reason) => this.export_failure = Some(reason),
                }
                cx.notify();
            });
        }));
    }

    fn set_at(&mut self, at: usize, cx: &mut Context<Self>) {
        let last = self.frame_count().saturating_sub(1);
        let at = at.min(last);
        if at == self.at && self.frame.is_some() {
            return;
        }
        self.at = at;
        self.follow_armor(cx);
        self.draw_current(cx);
        cx.notify();
    }

    fn toggle_playing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.playing = !self.playing;
        if self.playing {
            // Playing from the end starts again at the beginning, which is
            // what a transport with no stop button has to mean.
            if self.at + 1 >= self.frame_count() {
                self.set_at(0, cx);
            }
            self.start_ticker(window, cx);
        } else {
            self._tick = None;
        }
        cx.notify();
    }

    fn start_ticker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let interval = Duration::from_secs_f32(TICK.as_secs_f32() / self.speed.max(0.1));
        self._tick = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let still_playing = this
                    .update_in(cx, |this, window, cx| {
                        if !this.playing {
                            return false;
                        }
                        let next = this.at + 1;
                        if next >= this.frame_count() {
                            this.playing = false;
                            cx.notify();
                            return false;
                        }
                        this.set_at(next, cx);
                        // The bar follows playback, so a reader can see
                        // where in the battle they are.
                        this.sync_seek(window, cx);
                        true
                    })
                    .unwrap_or(false);
                if !still_playing {
                    return;
                }
            }
        }));
    }

    fn set_speed(&mut self, speed: f32, window: &mut Window, cx: &mut Context<Self>) {
        if (self.speed - speed).abs() < f32::EPSILON {
            return;
        }
        self.speed = speed;
        // The keys walk the ladder too, so the dropdown is told rather than
        // left showing the speed before last.
        let index = speed_index(speed);
        self.speed_select.update(cx, |state, cx| state.set_selected_value(&index, window, cx));
        // The ticker's interval is fixed when it starts, so a speed change
        // replaces it rather than waiting for the next tick.
        if self.playing {
            self.start_ticker(window, cx);
        }
        cx.notify();
    }

    /// Moves the seek slider to where playback has reached.
    ///
    /// The slider reports the move back as a change, which would ask for the
    /// frame it is already on; `set_at` answers that with nothing to do.
    fn sync_seek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.seek.update(cx, |slider, cx| slider.set_value(self.at as f32, window, cx));
    }

    /// Rebuilds the seek slider over the frames the bake produced.
    ///
    /// A slider quantises to its step and its range is fixed when it is
    /// built, so one built before the track was known could only sit at
    /// either end of it. This one steps a frame at a time over the track's
    /// real length, which is what the control is choosing.
    fn rebuild_seek(&mut self, cx: &mut Context<Self>) {
        let last = self.frame_count().saturating_sub(1);
        self.seek = cx.new(|_| seek_slider(last));
        // A rebuilt slider is a different entity, so the old subscription
        // reports nothing; without this the scrubber goes dead on the first
        // drag after a bake.
        self._seek_subscription = None;
        self._rebuilt_seek = Some(cx.subscribe(&self.seek, |this, state, event, cx| {
            let _ = state;
            this.on_seek_event(event, cx);
        }));
    }

    fn on_seek(
        &mut self,
        _state: &Entity<SliderState>,
        event: &gpui_kit::component::slider::SliderEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_seek_event(event, cx);
    }

    /// A slider change is a frame to go to.
    fn on_seek_event(&mut self, event: &gpui_kit::component::slider::SliderEvent, cx: &mut Context<Self>) {
        let gpui_kit::component::slider::SliderEvent::Change(value) = event else { return };
        self.set_at(value.start().round().max(0.) as usize, cx);
    }

    /// Shows a different part of the map, and redraws the frame on screen.
    ///
    /// Redrawn rather than left for the next tick: a zoom whose effect only
    /// arrived with playback would read as not having worked while paused.
    fn set_view(&mut self, view: MapViewport, window: &mut Window, cx: &mut Context<Self>) {
        if view == self.view {
            return;
        }
        self.view = view;
        self.zoom.update(cx, |slider, cx| slider.set_value(view.zoom(), window, cx));
        self.draw_current(cx);
        cx.notify();
    }

    /// How many screen pixels one pixel of the drawn frame covers.
    ///
    /// `None` before the frame has been painted once, which is the only time
    /// nothing knows where it is.
    fn drawn_scale(&self) -> Option<f32> {
        let bounds = self.drawn.get()?;
        let frame = self.frame.as_ref()?;
        let size = frame.size(0);
        let (width, height) = (size.width.0 as f32, size.height.0 as f32);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        // The frame is drawn to fit inside its element without being
        // stretched, so one scale covers both directions.
        Some((bounds.size.width.as_f32() / width).min(bounds.size.height.as_f32() / height))
    }

    /// Where in the map a window position falls, in the map's own pixels.
    ///
    /// Map space, not the drawn layer: this is what a ship's position and a
    /// peer's cursor are both in, so it is what they can be compared against
    /// at any zoom.
    ///
    /// `None` when the pointer is not over the map: on the HUD strip, or in
    /// the margin beside a frame that does not fill its element.
    fn map_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let (x, y) = self.drawn_point(position)?;
        Some(self.shaped_view().to_map(x, y))
    }

    /// Where in the drawn map a window position falls, after the zoom and pan
    /// have been applied.
    ///
    /// What a zoom about the pointer needs, since it re-anchors on a drawn
    /// point rather than a map one.
    fn drawn_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let (x, y) = self.canvas_point(position)?;
        let origin = self.track().map(|track| track.map_origin).unwrap_or(DEFAULT_MAP_ORIGIN);
        let (x, y) = (x - origin.0, y - origin.1);
        let span = wows_minimap_renderer::MINIMAP_SIZE as f32;
        (x >= 0.0 && x < span && y >= 0.0 && y < span).then_some((x, y))
    }

    /// The window as it is drawn: stretched toward the shape of the
    /// viewport it is drawn in.
    ///
    /// A wide viewport leaves the map with empty space either side. Zoomed
    /// in there is map to put there, so the window takes it; at rest there is
    /// not, and the frame stays square.
    fn shaped_view(&self) -> MapViewport {
        let Some(bounds) = self.drawn.get() else { return self.view };
        let (width, height) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
        if height <= 0.0 || width <= 0.0 {
            return self.view;
        }
        // Measured against the whole canvas, HUD strip included, since that
        // is what is fitted into the viewport.
        let canvas = wows_minimap_renderer::CANVAS_HEIGHT as f32;
        let wanted = width / height * canvas / wows_minimap_renderer::MINIMAP_SIZE as f32;
        self.view.widened(wanted)
    }

    /// Where a pointer position lands on the whole rasterised canvas, which
    /// is the map and whatever gutters are beside it.
    ///
    /// `None` before the frame has been painted once. Not clamped to the
    /// canvas: a caller asks what is there, and the answer for a point off
    /// the edge is nothing.
    fn canvas_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.drawn.get()?;
        let frame = self.frame.as_ref()?;
        let scale = self.drawn_scale()?;
        let size = frame.size(0);
        let drawn_width = size.width.0 as f32 * scale;
        let drawn_height = size.height.0 as f32 * scale;
        let left = bounds.origin.x.as_f32() + (bounds.size.width.as_f32() - drawn_width) / 2.0;
        let top = bounds.origin.y.as_f32() + (bounds.size.height.as_f32() - drawn_height) / 2.0;
        Some(((position.x.as_f32() - left) / scale, (position.y.as_f32() - top) / scale))
    }

    /// What the roster icon under the pointer reads as, if it is on one.
    ///
    /// The region says which row and which of its icons; the rest is read off
    /// the roster command the frame already holds, so a reading cannot drift
    /// from what was drawn.
    fn consumable_at(&self, position: Point<Pixels>) -> Option<ConsumableHover> {
        use wows_minimap_renderer::drawing::RegionKind;

        let at = self.canvas_point(position)?;
        let region = self.regions.iter().find(|region| region.contains(at))?;
        let RegionKind::RosterConsumable { entity_id, index } = region.kind;
        let track = self.track()?;
        let commands = track.frames.get(self.at)?;
        for command in commands {
            let DrawCommand::TeamRoster { rows, .. } = command else { continue };
            let Some(row) = rows.iter().find(|row| row.entity_id == entity_id) else { continue };
            let Some(consumable) = row.consumables.get(index) else { continue };
            return Some(ConsumableHover {
                player: row.player_name.clone(),
                lines: wows_minimap_renderer::draw_command::consumable_lines(consumable),
            });
        }
        None
    }

    /// Reads the roster icon under the pointer. Returns whether it changed.
    fn update_consumable_hover(&mut self, position: Point<Pixels>) -> bool {
        let found = self.consumable_at(position);
        let changed = found.as_ref().map(|hover| (&hover.player, &hover.lines.name))
            != self.hovered_consumable.as_ref().map(|(hover, _)| (&hover.player, &hover.lines.name));
        self.hovered_consumable = found.map(|hover| (hover, position));
        changed
    }

    /// The wheel zooms about whatever is under the pointer.
    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.drawn_point(event.position) else { return };
        let delta = event.delta.pixel_delta(SCROLL_LINE).y.as_f32();
        if delta == 0.0 {
            return;
        }
        let zoom = self.view.zoom() * (1.0 + delta * ZOOM_PER_PIXEL);
        self.set_view(self.shaped_view().zoomed_about(zoom, at), window, cx);
        cx.stop_propagation();
    }

    /// A drag of the map moves it under the pointer.
    ///
    /// Only once there is somewhere to drag to: at the whole map, a drag
    /// would do nothing and holding it would swallow the click.
    fn on_drag_start(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.map_point(event.position) else { return };
        // A tool in hand takes the drag: a reader dragging to draw is not
        // asking to pan the map under their line.
        if self.has_tool() {
            self.stroke(Stroke::Began { at: [at.0, at.1] }, cx);
            return;
        }
        // The handle over a picked shape turns it rather than moving it.
        if let Some((index, annotation)) = self.handle_under(event.position) {
            // Snapshotted as the handle is taken hold of: a turn reaches the
            // session as the pointer moves, so by the time it is let go the
            // old bearing has already gone.
            self.remember();
            self.turning = Some((index, annotation));
            return;
        }
        // Nor is one dragging something they have already picked out.
        if !self.picked.is_empty() {
            self.moving = Some(([at.0, at.1], self.collab.annotations()));
            return;
        }
        if self.view.is_whole_map() {
            return;
        }
        self.dragging = Some(event.position);
    }

    /// The shape whose rotation handle is under `position`, if the pointer
    /// is on one.
    ///
    /// A handle belongs to a single picked shape that has a bearing at all:
    /// turning several at once about their own middles is not what one
    /// handle means, and a circle looks the same at every angle.
    fn handle_under(&self, position: Point<Pixels>) -> Option<(usize, wt_collab_client::types::Annotation)> {
        let index = self.picked.single()?;
        let annotation = self.collab.annotations().get(index)?.clone();
        if !wt_collab_client::drawing::can_rotate(&annotation) {
            return None;
        }
        let (handle, _) = self.rotation_handle(&annotation)?;
        let reach = HANDLE_RADIUS + px(8.);
        let away = (position.x - handle.0).as_f32().hypot((position.y - handle.1).as_f32());
        (away < reach.as_f32()).then_some((index, annotation))
    }

    /// Where a shape's rotation handle sits in the element, and where the
    /// line to it starts.
    ///
    /// Above the shape by a fixed number of pixels rather than a map
    /// distance, so the handle stays the same size and the same reach away
    /// at every zoom, as the egui renderer's does.
    fn rotation_handle(
        &self,
        annotation: &wt_collab_client::types::Annotation,
    ) -> Option<((Pixels, Pixels), (Pixels, Pixels))> {
        let [left, top, right, _] = wt_collab_client::drawing::annotation_bounds(annotation);
        let anchor = self.element_point(((left + right) / 2.0, top))?;
        Some(((anchor.0, anchor.1 - HANDLE_DISTANCE), anchor))
    }

    /// The range circles every placed ship asks for.
    ///
    /// Resolved here rather than in the shared layer because a range is read
    /// out of the ship's own game data, which only a front end has. Nothing
    /// is drawn until a ship has been chosen for the annotation, since there
    /// is no ship to read the ranges of.
    fn placed_ship_ranges(&self) -> Vec<DrawCommand> {
        use wowsunpack::data::ResourceLoader as _;

        let Some(track) = self.track() else { return Vec::new() };
        let (space_size, version) = (track.space_size, track.version);
        let annotations = self.collab.annotations();
        if annotations.is_empty() {
            return Vec::new();
        }
        let Some(loaded) = self.game_data.as_ref().and_then(|cache| cache.newest_loaded()) else {
            return Vec::new();
        };
        let provider = loaded.provider();

        let mut circles = Vec::new();
        for annotation in &annotations {
            let wt_collab_client::types::Annotation::Ship { config: Some(config), .. } = annotation else {
                continue;
            };
            let Some(param) = provider.game_param_by_id(config.param_id.into()) else { continue };
            let Some(vehicle) = param.vehicle() else { continue };
            let hull = (!config.hull_name.is_empty()).then_some(config.hull_name.as_str());
            let ranges = vehicle.resolve_ranges(Some(provider.as_ref()), hull, version);
            circles.extend(wt_collab_client::geometry::ship_range_commands(annotation, &ranges, space_size));
        }
        circles
    }

    /// Every ship the loaded build knows, built the first time one is
    /// looked up rather than when the viewport opens.
    fn ships(&mut self) -> Option<std::rc::Rc<crate::armor_viewer::catalog::ShipCatalog>> {
        if let Some(catalog) = &self.ship_catalog {
            return Some(std::rc::Rc::clone(catalog));
        }
        let loaded = self.game_data.as_ref()?.newest_loaded()?;
        let catalog = std::rc::Rc::new(crate::armor_viewer::catalog::ShipCatalog::build(loaded.provider()));
        self.ship_catalog = Some(std::rc::Rc::clone(&catalog));
        Some(catalog)
    }

    /// The ships whose names match what has been typed, with the class each
    /// belongs to. Empty until something is typed, since every ship at once
    /// is not a choice.
    fn matching_ships(&mut self) -> Vec<(wowsunpack::game_params::types::Species, ShipEntry)> {
        let query = self.ship_query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let Some(catalog) = self.ships() else { return Vec::new() };
        let mut found = Vec::new();
        for nation in &catalog.nations {
            for class in &nation.classes {
                for ship in &class.ships {
                    if ship.search_name.contains(&query) {
                        found.push((class.species, ship.clone()));
                        if found.len() >= SHIP_MATCHES {
                            return found;
                        }
                    }
                }
            }
        }
        found
    }

    /// Gives the picked ship an identity, which is what its ranges are read
    /// from.
    fn name_picked_ship(
        &mut self,
        species: wowsunpack::game_params::types::Species,
        ship: &ShipEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use wowsunpack::game_params::types::GameParamProvider as _;

        let Some(index) = self.picked.single() else { return };
        let annotations = self.collab.annotations();
        let Some(wt_collab_client::types::Annotation::Ship { pos, yaw, friendly, .. }) = annotations.get(index) else {
            return;
        };
        let Some(loaded) = self.game_data.as_ref().and_then(|cache| cache.newest_loaded()) else { return };
        let provider = loaded.provider();
        let Some(param) = provider.game_param_by_index(&ship.param_index) else { return };

        self.remember();
        self.collab.update_annotation(
            index,
            wt_collab_client::types::Annotation::Ship {
                pos: *pos,
                yaw: *yaw,
                species: format!("{species:?}"),
                friendly: *friendly,
                config: Some(wt_collab_client::types::AnnotationShipConfig {
                    param_id: param.id().raw(),
                    ship_name: ship.display_name.clone(),
                    // Stock hull and no modifiers until the reader says
                    // otherwise, which is where the egui chooser leaves it.
                    ..Default::default()
                }),
            },
        );
        self.ship_query.clear();
        self.ship_search.update(cx, |state, cx| state.set_value("", window, cx));
        self.draw_current(cx);
        cx.notify();
    }

    /// Remembers what the session holds, before changing it.
    fn remember(&mut self) {
        if !self.collab.is_active() {
            return;
        }
        self.history.push(self.collab.annotations_held());
        if self.history.len() > HISTORY_DEPTH {
            self.history.remove(0);
        }
    }

    /// Puts the session back the way it was before the last change this
    /// viewport made.
    fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(was) = self.history.pop() else { return };
        self.collab.restore(&was);
        // What was picked out may no longer be there.
        self.picked.clear();
        self.draw_current(cx);
        cx.notify();
    }

    /// Widens or narrows the nib by `step`, within the range the egui board
    /// offers (`replay/renderer/mod.rs`'s own bracket keys).
    fn step_nib(&mut self, step: f32, cx: &mut Context<Self>) {
        let width = (self.drawing.width() + step).clamp(MIN_NIB, MAX_NIB);
        if (width - self.drawing.width()).abs() < f32::EPSILON {
            return;
        }
        self.drawing.set_width(width);
        cx.notify();
    }

    /// Erases whatever is picked out, if anything is.
    fn erase_picked(&mut self, cx: &mut Context<Self>) {
        if self.picked.is_empty() {
            return;
        }
        self.remember();
        // Highest index first: erasing shifts what is after it.
        let mut picked: Vec<usize> = self.picked.picked().to_vec();
        picked.sort_unstable_by(|a, b| b.cmp(a));
        for index in picked {
            self.collab.erase_annotation(index);
        }
        self.picked.clear();
        self.draw_current(cx);
        cx.notify();
    }

    /// Takes up `tool`, dropping whatever was half-drawn.
    fn take_up(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.drawing.set_tool(tool);
        self.draw_current(cx);
        cx.notify();
    }

    /// Whether a drawing tool is in hand, which is what decides who gets the
    /// pointer.
    fn has_tool(&self) -> bool {
        *self.drawing.tool() != wt_collab_client::drawing::Tool::None
    }

    /// Hands a pointer event to the tool and does what it drew.
    fn stroke(&mut self, stroke: Stroke, cx: &mut Context<Self>) {
        let existing = self.collab.annotations();
        match self.drawing.handle(stroke, &existing) {
            Some(Drawn::Added(annotation)) => {
                self.remember();
                self.collab.add_annotation(annotation);
            }
            Some(Drawn::Erased(index)) => {
                self.remember();
                self.collab.erase_annotation(index);
            }
            None => {}
        }
        // The frame carries the part-drawn shape, so every stroke redraws.
        self.draw_current(cx);
        cx.notify();
    }

    fn on_drag_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.report_cursor(event.position, cx);
        if self.update_consumable_hover(event.position) {
            cx.notify();
        }
        self.pointer_at = self.map_point(event.position).map(|(x, y)| [x, y]);

        // A handle being held turns the shape it belongs to.
        if let Some((index, before)) = self.turning.clone()
            && event.dragging()
            && let Some(at) = self.pointer_at
        {
            let [left, top, right, bottom] = wt_collab_client::drawing::annotation_bounds(&before);
            let middle = [(left + right) / 2.0, (top + bottom) / 2.0];
            let mut turned = before;
            wt_collab_client::drawing::rotate_annotation(&mut turned, wt_collab_client::drawing::bearing(middle, at));
            self.collab.update_annotation(index, turned);
            self.draw_current(cx);
            cx.notify();
            return;
        }

        // A tool in hand builds its shape from the drag rather than panning.
        if self.has_tool() {
            if self.drawing.is_drawing()
                && event.dragging()
                && let Some(at) = self.pointer_at
            {
                self.stroke(Stroke::Moved { at, straight: event.modifiers.shift }, cx);
            }
            return;
        }

        let Some(from) = self.dragging else { return };
        if !event.dragging() {
            self.dragging = None;
            return;
        }
        let Some(scale) = self.drawn_scale() else { return };
        let delta = ((event.position.x - from.x).as_f32() / scale, (event.position.y - from.y).as_f32() / scale);
        self.dragging = Some(event.position);
        self.set_view(self.shaped_view().dragged(delta), window, cx);
    }

    fn on_drag_end(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.dragging = None;
        if self.has_tool()
            && self.drawing.is_drawing()
            && let Some(at) = self.map_point(event.position)
        {
            self.stroke(Stroke::Ended { at: [at.0, at.1] }, cx);
            return;
        }
        // A turn, like a move, is one update at the end.
        if let Some((index, _)) = self.turning.take() {
            if let Some(annotation) = self.collab.annotations().get(index) {
                self.collab.update_annotation(index, annotation.clone());
            }
            cx.notify();
            return;
        }
        // The session hears about a move once, when it is over, rather than
        // per pixel of the drag.
        if let Some((from, before)) = self.moving.take()
            && let Some((x, y)) = self.map_point(event.position)
        {
            self.remember();
            let delta = [x - from[0], y - from[1]];
            for index in self.picked.picked() {
                let Some(annotation) = before.get(*index) else { continue };
                let mut moved = annotation.clone();
                wt_collab_client::drawing::move_annotation(&mut moved, delta);
                self.collab.update_annotation(*index, moved);
            }
            cx.notify();
        }
    }

    /// A double-click puts the whole map back.
    fn on_viewport_click(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        // A tool that places rather than drags wants the click, and a
        // double-click must not put the whole map back under a half-drawn
        // shape.
        if self.has_tool() {
            if let Some(at) = self.map_point(event.position()) {
                self.stroke(Stroke::Clicked { at: [at.0, at.1] }, cx);
            }
            return;
        }
        // With no tool, a click picks a drawn shape out to move. Only when
        // there is something to pick: otherwise every click on the map would
        // swallow the double-click that puts the whole map back.
        if let Some((x, y)) = self.map_point(event.position()) {
            let annotations = self.collab.annotations();
            if !annotations.is_empty() {
                self.picked.click(&annotations, [x, y], event.modifiers().secondary());
                self.picked.retain_within(&annotations);
                cx.notify();
                if !self.picked.is_empty() {
                    return;
                }
            }
        }
        if event.click_count() < 2 {
            return;
        }
        self.set_view(MapViewport::default(), window, cx);
    }

    /// The zoom control moves about the middle of what is on screen, as the
    /// egui renderer's does: there is no pointer to zoom about.
    fn on_zoom(
        &mut self,
        _state: &Entity<SliderState>,
        event: &gpui_kit::component::slider::SliderEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let gpui_kit::component::slider::SliderEvent::Change(value) = event else { return };
        let middle = wows_minimap_renderer::MINIMAP_SIZE as f32 / 2.0;
        self.set_view(self.view.zoomed_about(value.start(), (middle, middle)), window, cx);
    }

    /// What the viewport shows of the map. Test-only: production code reaches
    /// the field.
    #[cfg(test)]
    pub(crate) fn view(&self) -> MapViewport {
        self.view
    }

    /// Where the frame was painted. Test-only: the painter fills this in, and
    /// a test has no painter.
    #[cfg(test)]
    pub(crate) fn set_drawn_for_test(&mut self, bounds: Bounds<Pixels>) {
        self.drawn.set(Some(bounds));
    }

    /// Replaces what the frame on screen drew that a pointer can rest on.
    ///
    /// Test-only: a rasterised frame comes with its own.
    #[cfg(test)]
    pub(crate) fn set_regions_for_test(&mut self, regions: Vec<wows_minimap_renderer::drawing::DrawnRegion>) {
        self.regions = regions;
    }

    /// A viewport with a frame the size the renderer produces, laid out at
    /// exactly that size, so a window position is a frame position.
    #[cfg(test)]
    fn seed_frame_for_test(&mut self) {
        let width = wows_minimap_renderer::MINIMAP_SIZE;
        let height = wows_minimap_renderer::CANVAS_HEIGHT;
        self.frame = Some(to_image(image::RgbImage::new(width, height)));
        self.set_drawn_for_test(gpui_kit::Bounds {
            origin: gpui_kit::point(px(0.), px(0.)),
            size: gpui_kit::size(px(width as f32), px(height as f32)),
        });
    }

    /// The marks on the seek bar for where the battle began and ended.
    ///
    /// A replay records from the loading screen on and often past the last
    /// shot, so the battle proper is a stretch inside the track rather than
    /// the whole of it. Empty while the bake is still running, and an end the
    /// replay never recorded has no mark.
    fn battle_ticks(&self) -> Vec<AnyElement> {
        let Some(track) = self.track() else { return Vec::new() };
        [Some(track.battle_start), track.battle_end]
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, clock)| {
                let position = track.position_of(clock)?;
                Some(
                    div()
                        .id(("replay-renderer-battle-tick", index))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(position))
                        .w(TICK_WIDTH)
                        .bg(crate::theme::text_dim())
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// Where playback has reached, as the clock the game showed.
    ///
    /// Elapsed rather than absolute: a replay records from the loading screen
    /// on, so the raw clock opens at something like 0:40 on a battle that has
    /// not started.
    fn clock_label(&self) -> String {
        let Some(track) = self.track() else { return String::new() };
        mmss(track.elapsed_at(self.at))
    }

    /// How long the battle runs for, as the clock would read at its end.
    fn length_label(&self) -> String {
        let Some(track) = self.track() else { return String::new() };
        mmss(track.elapsed_at(track.len().saturating_sub(1)))
    }

    /// Where playback has reached and how much there is, which is what says
    /// whether the end is near without reading the bar.
    fn clock_and_length(&self) -> String {
        let Some(_) = self.track() else { return String::new() };
        format!("{} / {}", self.clock_label(), self.length_label())
    }
}

/// One speed in the dropdown, named by where it sits in [`SPEEDS`] rather
/// than by its own value: a float is a poor thing to match a selection on.
#[derive(Clone)]
pub(crate) struct SpeedItem(usize);

impl gpui_kit::component::searchable_list::SearchableListItem for SpeedItem {
    type Value = usize;

    fn title(&self) -> SharedString {
        SharedString::from(speed_label(SPEEDS[self.0]))
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }
}

/// Where `speed` sits in the ladder.
fn speed_index(speed: f32) -> usize {
    SPEEDS.iter().position(|offered| (offered - speed).abs() < f32::EPSILON).unwrap_or_default()
}

/// A speed as the transport labels it: no trailing zero on a whole one.
fn speed_label(speed: f32) -> String {
    if (speed - speed.round()).abs() < f32::EPSILON {
        format!("{}x", speed.round() as i32)
    } else {
        format!("{speed}x")
    }
}

/// Seconds of game time as the clock the game shows, zero-padded as the egui
/// renderer pads it.
fn mmss(seconds: f32) -> String {
    let seconds = seconds.max(0.0) as u32;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

impl Drop for ReplayRendererPanel {
    fn drop(&mut self) {
        // A viewport that has been closed is not one whose battle is worth
        // finishing.
        self.cancel.store(true, Ordering::Relaxed);
        // Nor is it one the session should still be offering to watch.
        if self.shared {
            self.collab.close_replay(self.replay_id);
        }
    }
}

impl Focusable for ReplayRendererPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ReplayRendererPanel {
    fn panel_name(&self) -> &'static str {
        "ReplayRendererPanel"
    }

    fn closable(&self, _cx: &App) -> bool {
        true
    }
}

impl Panel for ReplayRendererPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.title.clone()
    }
}

impl ReplayRendererPanel {
    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.state {
            State::Baking => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Spinner::new().large())
                .child(
                    div()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.replay.renderer.baking").into_owned()),
                )
                .into_any_element(),
            State::Failed(reason) => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .child(
                    div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.replay.renderer.failed").into_owned()),
                )
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(reason.clone()))
                .into_any_element(),
            State::Ready(_) => match self.frame.clone() {
                Some(frame) => {
                    let drawn = Rc::clone(&self.drawn);
                    div()
                        .id("replay-renderer-viewport")
                        .test_support()
                        .relative()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(!self.view.is_whole_map(), |this| this.cursor_grab())
                        .child(img(frame).size_full())
                        // Where the frame landed, which is the only way a
                        // pointer position can be turned into a map one.
                        .child(
                            canvas(
                                move |bounds, _window, _cx| drawn.set(Some(bounds)),
                                |_bounds, _state, _window, _cx| {},
                            )
                            .absolute()
                            .inset_0(),
                        )
                        .children(self.hud_strip().zip(self.advantage_hover()).map(|((left, top, height), lines)| {
                            div()
                                .id("replay-renderer-advantage-hover")
                                .absolute()
                                .left(left)
                                .top(top)
                                .right(left)
                                .h(height)
                                .tooltip(hover_lines(
                                    lines
                                        .join(
                                            "
",
                                        )
                                        .into(),
                                ))
                        }))
                        .children(self.export_overlay(cx))
                        // What the keys do, while ctrl is held: the chords are
                        // not written anywhere else, so this is how they are
                        // found.
                        .when(self.ctrl_held, |this| this.child(shortcut_sheet(cx)))
                        .children(self.collab_overlay())
                        .children(self.rotation_handle_overlay(cx))
                        .children(self.consumable_reading(cx))
                        .on_modifiers_changed(cx.listener(
                            |this, event: &gpui_kit::ModifiersChangedEvent, _window, cx| {
                                if this.ctrl_held == event.modifiers.secondary() {
                                    return;
                                }
                                this.ctrl_held = event.modifiers.secondary();
                                cx.notify();
                            },
                        ))
                        .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_ping))
                        .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_click))
                        .on_scroll_wheel(cx.listener(Self::on_scroll))
                        .on_mouse_down(MouseButton::Left, cx.listener(Self::on_drag_start))
                        .on_mouse_move(cx.listener(Self::on_drag_move))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::on_drag_end))
                        .on_click(cx.listener(Self::on_viewport_click))
                        .into_any_element()
                }
                None => div().size_full().into_any_element(),
            },
        }
    }

    fn render_transport(&self, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.theme().border;
        let ready = matches!(self.state, State::Ready(_));
        let last_frame = self.frame_count().saturating_sub(1);
        // Refused until the second walk has read the battle, and at whichever
        // end of it there is nothing further to step to.
        let has_previous = self.previous_event().is_some();
        let has_next = self.next_event().is_some();
        let controls =
            h_flex()
                .flex_none()
                .gap_2()
                .items_center()
                .px_2()
                .pb_1()
                .child(
                    Button::new("replay-renderer-jump-to-start")
                        .child(crate::icons::icon(crate::icons::SKIP_BACK))
                        .compact()
                        .disabled(!ready)
                        .tooltip(t!("ui.renderer.controls.jump_to_start").into_owned())
                        .on_click(cx.listener(|this, _event, window, cx| this.go_to(0, window, cx))),
                )
                .child(
                    Button::new("replay-renderer-previous-event")
                        .child(crate::icons::icon(crate::icons::REWIND))
                        .compact()
                        .disabled(!ready || !has_previous)
                        .tooltip(t!("ui.renderer.controls.previous_event").into_owned())
                        .on_click(cx.listener(|this, _event, window, cx| this.jump_to_previous_event(window, cx))),
                )
                .child(
                    Button::new("replay-renderer-back-10s")
                        .child(crate::icons::icon(crate::icons::CLOCK_COUNTER_CLOCKWISE))
                        .compact()
                        .disabled(!ready)
                        .tooltip(t!("ui.renderer.controls.back_10s").into_owned())
                        .on_click(cx.listener(|this, _event, window, cx| this.seek_by(-SEEK_STEP, window, cx))),
                )
                .child(
                    Button::new("replay-renderer-play")
                        .icon(if self.playing { IconName::Pause } else { IconName::Play })
                        .compact()
                        .disabled(!ready)
                        .tooltip(
                            t!(if self.playing { "ui.replay.renderer.pause" } else { "ui.replay.renderer.play" })
                                .into_owned(),
                        )
                        .on_click(cx.listener(|this, _event, window, cx| this.toggle_playing(window, cx))),
                )
                .child(
                    Button::new("replay-renderer-forward-10s")
                        .child(crate::icons::icon(crate::icons::CLOCK_CLOCKWISE))
                        .compact()
                        .disabled(!ready)
                        .tooltip(t!("ui.renderer.controls.forward_10s").into_owned())
                        .on_click(cx.listener(|this, _event, window, cx| this.seek_by(SEEK_STEP, window, cx))),
                )
                .child(
                    Button::new("replay-renderer-next-event")
                        .child(crate::icons::icon(crate::icons::FAST_FORWARD))
                        .compact()
                        .disabled(!ready || !has_next)
                        .tooltip(t!("ui.renderer.controls.next_event").into_owned())
                        .on_click(cx.listener(|this, _event, window, cx| this.jump_to_next_event(window, cx))),
                )
                .child(
                    Button::new("replay-renderer-jump-to-end")
                        .child(crate::icons::icon(crate::icons::SKIP_FORWARD))
                        .compact()
                        .disabled(!ready)
                        .tooltip(t!("ui.renderer.controls.jump_to_end").into_owned())
                        .on_click(cx.listener(move |this, _event, window, cx| this.go_to(last_frame, window, cx))),
                )
                .child(
                    div()
                        .flex_none()
                        .w(CLOCK_WIDTH)
                        .text_xs()
                        .text_color(crate::theme::text_dim())
                        .child(self.clock_and_length()),
                )
                // Everything past here is not playback, so it sits at the far
                // end rather than between the reader and the transport.
                .child(div().flex_1().min_w(px(0.)))
                .child(div().flex_none().text_xs().child(crate::icons::icon(crate::icons::MAGNIFYING_GLASS)))
                .child(div().flex_none().w(ZOOM_WIDTH).child(Slider::new(&self.zoom).disabled(!ready)))
                .child(
                    Button::new("replay-renderer-zoom-reset")
                        .label(t!("ui.buttons.reset").into_owned())
                        .compact()
                        .disabled(!ready || self.view.is_whole_map())
                        .on_click(
                            cx.listener(|this, _event, window, cx| this.set_view(MapViewport::default(), window, cx)),
                        ),
                )
                .child(crate::ui::rule_v(cx))
                .child(
                    crate::ui::boxed(SPEED_WIDTH, crate::ui::SELECT_SMALL_HEIGHT).child(
                        Select::new(&self.speed_select)
                            .id("replay-renderer-speed")
                            .accessibility_label(t!("ui.renderer.controls.speed").into_owned())
                            .small(),
                    ),
                )
                .child(crate::ui::rule_v(cx))
                .child(
                    Button::new("replay-renderer-export")
                        .child(crate::icons::icon(crate::icons::DOWNLOAD_SIMPLE))
                        .compact()
                        .disabled(!ready || self.export.is_some())
                        .tooltip(t!("ui.replay.renderer.export_video").into_owned())
                        .on_click(cx.listener(|this, _event, window, cx| {
                            this.export_asking_first(ExportTarget::File, window, cx)
                        })),
                )
                .child(tools_popover(&cx.entity(), self, cx))
                .child(render_options_popover(&cx.entity(), self, ready, cx))
                .child(timeline_popover(&cx.entity(), self, cx))
                .when(!self.popped_out, |this| {
                    this.child(
                        Button::new("replay-renderer-pop-out")
                            .child(crate::icons::icon(crate::icons::ARROW_SQUARE_OUT))
                            .compact()
                            .tooltip(t!("ui.replay.renderer.pop_out").into_owned())
                            .on_click(cx.listener(|_this, _event, _window, cx| cx.emit(RendererEvent::PopOut))),
                    )
                })
                .child(
                    Button::new("replay-renderer-clipboard")
                        .child(crate::icons::icon(crate::icons::CLIPBOARD))
                        .compact()
                        .disabled(!ready || self.export.is_some())
                        .tooltip(t!("ui.replay.renderer.export_clipboard").into_owned())
                        .on_click(cx.listener(|this, _event, window, cx| {
                            this.export_asking_first(ExportTarget::Clipboard, window, cx)
                        })),
                );

        // The scrubber has a row to itself, as a video player gives it: a
        // bar sharing a row with a dozen controls is both hard to aim at and
        // hard to read a position off.
        let scrubber = div().id("replay-renderer-scrubber").test_support().flex_none().px(THUMB_INSET).pt_1().child(
            div().relative().w_full().child(Slider::new(&self.seek).disabled(!ready)).children(self.battle_ticks()),
        );

        let transport = v_flex()
            .flex_none()
            .bg(crate::theme::surface())
            .border_t_1()
            .border_color(border)
            .children(self.export_failure.as_ref().map(|reason| {
                // A whole row, because a codec's complaint is a sentence and
                // squeezing one between the controls pushes them about.
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .text_color(rgb(crate::theme::semantic().error))
                    .child(reason.clone())
                    .into_any_element()
            }))
            .child(scrubber)
            .child(controls);
        transport.into_any_element()
    }
}

impl Render for ReplayRendererPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = self.render_body(cx);
        let transport = self.render_transport(cx);
        v_flex()
            .id("replay-renderer")
            .relative()
            .children(ship_menu(&cx.entity(), self))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event, window, cx| this.on_key(event, window, cx)))
            .size_full()
            .child(div().flex_1().min_h(px(0.)).child(body))
            .child(transport)
    }
}

/// Room for "MM:SS / MM:SS" without the transport shifting as it counts.
const CLOCK_WIDTH: Pixels = px(88.);

/// How far the scrubber is held in from the edges.
///
/// A slider's thumb is drawn eight pixels left of where it sits and grows a
/// ring outside that under the pointer. At the very start of a track all of
/// that hangs off the left of the row, over the panel's edge and under
/// whatever is beside it, so this row starts further in than the rest of the
/// transport.
const THUMB_INSET: Pixels = px(13.);

/// Room for the widest speed the ladder offers.
const SPEED_WIDTH: Pixels = px(72.);

/// How wide a battle-start or battle-end mark is drawn on the seek bar.
const TICK_WIDTH: Pixels = px(1.5);

/// The menu a right-click on a ship opens: its trail and its ranges.
///
/// Drawn over the viewport rather than beside it, because it belongs to the
/// ship that was clicked.
fn ship_menu(panel: &Entity<ReplayRendererPanel>, view: &ReplayRendererPanel) -> Option<AnyElement> {
    let menu = view.ship_menu()?;
    let player = menu.player_name.clone();
    let trail_on = view.trail_shown(&player);
    let ranges = view.ranges_for(&player);
    let owner = panel.clone();

    let range_switch = |id: &'static str, label: &'static str, on: bool, set: fn(&mut ShipConfigFilter, bool)| {
        let owner = panel.clone();
        let player = player.clone();
        Checkbox::new(id).label(t!(label).to_string()).checked(on).on_click(move |checked, _window, cx: &mut App| {
            let checked = *checked;
            let player = player.clone();
            owner.update(cx, |panel, cx| panel.set_ranges_for(&player, |filter| set(filter, checked), cx));
        })
    };

    // Separate statements limit temporary storage in debug builds.
    let mut content = v_flex().gap_1();
    content = content.child(
        h_flex()
            .justify_between()
            .items_center()
            .child(div().text_xs().font_weight(FontWeight::BOLD).child(player.clone()))
            .child({
                let owner = owner.clone();
                Button::new("replay-renderer-ship-menu-close")
                    .child(crate::icons::icon(crate::icons::X))
                    .compact()
                    .on_click(move |_event, _window, cx: &mut App| {
                        owner.update(cx, |panel, cx| panel.close_ship_menu(cx));
                    })
            }),
    );
    content = content.child({
        let owner = owner.clone();
        let player = player.clone();
        Checkbox::new("replay-renderer-ship-trail")
            .label(t!("ui.renderer.context.show_trail").to_string())
            .checked(trail_on)
            .on_click(move |checked, _window, cx: &mut App| {
                let checked = *checked;
                let player = player.clone();
                owner.update(cx, |panel, cx| panel.set_trail_shown(&player, checked, cx));
            })
    });
    content = content.children(view.has_armor_to_show(menu.entity_id).then(|| {
        let owner = owner.clone();
        let entity_id = menu.entity_id;
        Button::new("replay-renderer-ship-armor")
            .label(t!("ui.renderer.context.realtime_armor").into_owned())
            .compact()
            .on_click(move |_event, _window, cx: &mut App| {
                owner.update(cx, |panel, cx| panel.show_armor(entity_id, cx));
            })
    }));
    content = content.child({
        let owner = owner.clone();
        let player = player.clone();
        Button::new("replay-renderer-ship-only-trail")
            .label(t!("ui.renderer.context.disable_other_trails").into_owned())
            .compact()
            .on_click(move |_event, _window, cx: &mut App| {
                let player = player.clone();
                owner.update(cx, |panel, cx| panel.only_trail(&player, cx));
            })
    });
    content = content.child(
        div()
            .pt_1()
            .text_xs()
            .text_color(crate::theme::text_dim())
            .child(t!("ui.renderer.context.ranges").into_owned()),
    );
    content = content.child(range_switch(
        "replay-renderer-range-detection",
        "ui.renderer.context.detection",
        ranges.detection,
        |filter, on| filter.detection = on,
    ));
    content = content.child(range_switch(
        "replay-renderer-range-main",
        "ui.renderer.context.main_battery",
        ranges.main_battery,
        |filter, on| filter.main_battery = on,
    ));
    content = content.child(range_switch(
        "replay-renderer-range-secondary",
        "ui.renderer.context.secondary",
        ranges.secondary_battery,
        |filter, on| filter.secondary_battery = on,
    ));
    content = content.child(range_switch(
        "replay-renderer-range-torpedo",
        "ui.renderer.context.torpedo",
        ranges.torpedo,
        |filter, on| filter.torpedo = on,
    ));
    content = content.child(range_switch(
        "replay-renderer-range-radar",
        "ui.renderer.context.radar",
        ranges.radar,
        |filter, on| filter.radar = on,
    ));
    content = content.child(range_switch(
        "replay-renderer-range-hydro",
        "ui.renderer.context.hydro",
        ranges.hydro,
        |filter, on| filter.hydro = on,
    ));
    content = content.child({
        let owner = owner.clone();
        let player = player.clone();
        let all_on = ranges == ALL_RANGES;
        Button::new("replay-renderer-every-range")
            .label(
                if all_on { t!("ui.renderer.context.disable_all") } else { t!("ui.renderer.context.enable_all") }
                    .into_owned(),
            )
            .compact()
            .on_click(move |_event, _window, cx: &mut App| {
                let player = player.clone();
                owner.update(cx, |panel, cx| panel.set_every_range_for(&player, !all_on, cx));
            })
    });
    content = content.child(
        v_flex()
            .gap_1()
            .child({
                let owner = owner.clone();
                let player = player.clone();
                Button::new("replay-renderer-only-ranges")
                    .label(t!("ui.renderer.context.disable_other_ranges").into_owned())
                    .compact()
                    .on_click(move |_event, _window, cx: &mut App| {
                        let player = player.clone();
                        owner.update(cx, |panel, cx| panel.only_ranges(&player, cx));
                    })
            })
            .child({
                let owner = owner.clone();
                Button::new("replay-renderer-all-ranges")
                    .label(t!("ui.renderer.context.enable_all_ranges").into_owned())
                    .compact()
                    .on_click(move |_event, _window, cx: &mut App| {
                        owner.update(cx, |panel, cx| panel.all_ranges(cx));
                    })
            }),
    );

    let at = menu.at;
    Some(
        deferred(
            anchored().position(at).snap_to_window_with_margin(px(8.)).child(
                div()
                    .w(px(220.))
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(crate::theme::border_bright())
                    .bg(crate::theme::surface())
                    .child(content),
            ),
        )
        .with_priority(1)
        .into_any_element(),
    )
}

/// Whether the reader has asked for this ship's trail or this range.
///
/// The global switches decide whether a layer is drawn at all; this decides
/// whose, which is what the per-ship menu changes.
fn per_ship_allows(
    command: &DrawCommand,
    trail_hidden: &HashSet<String>,
    ship_ranges: &HashMap<String, ShipConfigFilter>,
) -> bool {
    match command {
        DrawCommand::PositionTrail { player_name, .. } => {
            player_name.as_ref().is_none_or(|name| !trail_hidden.contains(name))
        }
        DrawCommand::ShipConfigCircle { player_name, kind, .. } => {
            ship_ranges.get(player_name).is_some_and(|filter| filter.is_enabled(kind))
        }
        _ => true,
    }
}

/// How many ships a search offers at once.
const SHIP_MATCHES: usize = 10;

/// How many changes back an undo can reach.
const HISTORY_DEPTH: usize = 32;

/// How big a rotation handle is drawn, and how far above the shape it sits.
/// The egui renderer's own figures, in screen pixels at any zoom.
const HANDLE_RADIUS: Pixels = px(5.);
const HANDLE_DISTANCE: Pixels = px(25.);

/// What a tool draws with until the reader says otherwise: the white and the
/// width the egui toolbar opens on.
const DEFAULT_INK: [u8; 4] = [255, 255, 255, 255];
const DEFAULT_NIB: f32 = 2.0;

/// What the bracket keys hold the nib between, as the egui board does.
const MIN_NIB: f32 = 1.0;
const MAX_NIB: f32 = 8.0;

/// How long a ping stays on the map, in seconds.
const PING_SECONDS: f32 = 1.0;

/// How often a running session redraws, which is what moves a peer's cursor
/// and carries a ping through its ripple.
const COLLAB_TICK: Duration = Duration::from_millis(33);

/// How wide a ping's ring grows, in element pixels.
const PING_RADIUS: f32 = 26.0;

/// The strip the renderer reserves above the map, which is where the score
/// bar and the advantage label are drawn.
const HUD_STRIP_HEIGHT: f32 = wows_minimap_renderer::HUD_HEIGHT as f32;

/// How near a right-click has to land to have meant a ship, in map pixels.
/// A ship icon is about this wide.
const PICK_RADIUS: f32 = 14.0;

/// A ship showing none of its ranges.
const NO_RANGES: ShipConfigFilter = ShipConfigFilter {
    detection: false,
    main_battery: false,
    secondary_battery: false,
    torpedo: false,
    radar: false,
    hydro: false,
};

/// A ship showing all of them.
const ALL_RANGES: ShipConfigFilter = ShipConfigFilter {
    detection: true,
    main_battery: true,
    secondary_battery: true,
    torpedo: true,
    radar: true,
    hydro: true,
};

/// The ship a context menu is open for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShipMenu {
    pub entity_id: EntityId,
    pub player_name: String,
    /// Where the right-click landed, which is where the menu opens: a menu
    /// about one ship belongs beside that ship, not in a corner.
    pub at: Point<Pixels>,
}

/// Room for the zoom control. Narrow: the seek bar is what the transport is
/// mostly for, and it takes whatever is left.
const ZOOM_WIDTH: Pixels = px(90.);

/// Where the map's corner sits before a track says otherwise.
///
/// The renderer reserves a strip above the map for the score bar and the
/// timer; a side panel would also move the map right, which is why a baked
/// track carries its own origin rather than every caller assuming this one.
const DEFAULT_MAP_ORIGIN: (f32, f32) = (0.0, wows_minimap_renderer::HUD_HEIGHT as f32);

/// How much one line of wheel travel is worth, for a wheel that reports lines
/// rather than pixels.
const SCROLL_LINE: Pixels = px(20.);

/// How much of a zoom one pixel of wheel travel is. The egui renderer's own
/// rate, so the wheel feels the same in both.
const ZOOM_PER_PIXEL: f32 = 0.01;

/// What a playback viewport draws when it opens.
///
/// Everything it baked except the layers that are asked for rather than
/// assumed: the rosters take a gutter either side of the map, and the ranges
/// would cover it.
fn playback_options_hidden() -> RenderOptions {
    RenderOptions { show_ship_config: false, show_team_rosters: false, ..playback_options() }
}

/// What a playback viewport bakes.
///
/// The hover preview's set plus every ship's range circles. They are per-frame
/// and small, so keeping them costs little, and a viewport that did not bake
/// them could not offer them at all. Trails are still left out: a trail
/// command carries every point so far, so baking one per frame would cost the
/// square of the track's length.
fn playback_options() -> RenderOptions {
    RenderOptions {
        show_ship_config: true,
        // The rosters' own commands carry fixed coordinates, so baking them
        // costs a roster per frame and nothing else; the canvas they need is
        // the target's business, not the bake's.
        show_team_rosters: true,
        // Every ship's, so the reader can choose whose to look at afterwards
        // rather than re-baking to change their mind.
        ship_config_visibility: wows_minimap_renderer::draw_command::ShipConfigVisibility::Filtered(
            std::sync::Arc::new(|_entity| {
                Some(wows_minimap_renderer::draw_command::ShipConfigFilter {
                    detection: true,
                    main_battery: true,
                    secondary_battery: true,
                    torpedo: true,
                    radar: true,
                    hydro: true,
                })
            }),
        ),
        ..wows_minimap_renderer::frame_track::bake_options()
    }
}

/// Why a battle could not be played back.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Preview(#[from] crate::minimap_preview::PreviewError),
}

/// Walks `path`'s battle once, keeping every frame's draw commands and the
/// renderer they are drawn through.
///
/// `alts` are other recordings of the same battle, read alongside it: the map
/// then shows what the primary's team could not see.
fn bake(
    path: &std::path::Path,
    alts: &[PathBuf],
    game_data: &GameDataCache,
    cancel: &AtomicBool,
) -> Result<(Track, SharedPreviewRenderer), RenderError> {
    let baked = crate::minimap_preview::bake_track(
        path,
        alts,
        game_data,
        cancel,
        TRACK_BUDGET,
        BAKE_INTERVAL,
        playback_options(),
    )?;
    let track = Track {
        frames: baked.frames,
        clocks: baked.clocks,
        battle_start: baked.battle_start,
        battle_end: baked.battle_end,
        space_size: baked.space_size,
        version: baked.version,
        map_origin: (baked.map_origin.0 as f32, baked.map_origin.1 as f32),
        space: baked.map_name,
    };
    Ok((track, baked.renderer))
}

/// Renders each replay in `paths` to its own file under `out_dir`, without
/// opening a viewport for any of them.
///
/// This is what the listing's "Render N Replays to Video" is for: the egui app
/// runs one background batch into a chosen folder
/// (`replay/renderer/video_export.rs:458`), where opening N viewports would bake
/// N tracks that nobody watches and leave the reader to save each one.
///
/// Sequential, because each encode holds a GPU device and the machine has one:
/// two at once would contend for it and finish no sooner. A replay that fails is
/// reported and the rest still run, since one unreadable file in a marked set
/// should not lose the other forty.
///
/// `defaults` is what the reader saved as what a renderer opens with, so a
/// batch writes what a viewport would have shown rather than the built-in set.
///
/// `report` is told which replay the batch has reached before it is read, so the
/// reader is not left watching a still line for forty files.
///
/// Returns the files written and the replays that failed.
pub fn batch_export(
    paths: Vec<PathBuf>,
    game_data: GameDataCache,
    out_dir: PathBuf,
    defaults: wows_minimap_renderer::SavedRenderOptions,
    report: futures::channel::mpsc::UnboundedSender<BatchStep>,
    cx: &App,
) -> Task<(Vec<PathBuf>, Vec<PathBuf>)> {
    let settings = export_settings_of(&defaults);
    let (options, show_dead_ships) = batch_options(&defaults);

    cx.background_spawn(async move {
        let mut written = Vec::new();
        let mut failed = Vec::new();
        let cancel = AtomicBool::new(false);
        let total = paths.len();

        for (index, path) in paths.into_iter().enumerate() {
            let named = path.file_stem().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
            let _ = report.unbounded_send(BatchStep { done: index, total, replay: named });
            let Some(stem) = path.file_stem() else {
                failed.push(path);
                continue;
            };
            let output = out_dir.join(stem).with_extension("mp4");

            let baked = match bake(&path, &[], &game_data, &cancel) {
                Ok(baked) => baked,
                Err(err) => {
                    tracing::warn!(path = %path.display(), error = %err, "batch render: the replay did not bake");
                    failed.push(path);
                    continue;
                }
            };
            let (track, renderer) = baked;
            let covered: &[Vec<DrawCommand>] = if settings.include_pre_battle {
                &track.frames
            } else {
                let from = track.frame_at(track.battle_start.seconds());
                track.frames.get(from..).unwrap_or(&track.frames)
            };
            // The bake holds every layer it could; which of them are drawn is
            // what the saved defaults decide, as they do in a viewport.
            let frames: Vec<Vec<DrawCommand>> = covered
                .iter()
                .map(|frame| {
                    frame
                        .iter()
                        .filter(|command| should_draw_command(command, &options, show_dead_ships))
                        .cloned()
                        .collect()
                })
                .collect();
            if frames.is_empty() {
                failed.push(path);
                continue;
            }
            let duration = (frames.len().saturating_sub(1)) as f32 * BAKE_INTERVAL;

            // The progress channel is per encode; nothing reads it here, since
            // the batch reports per replay rather than per frame.
            let (progress, _ignored) = futures::channel::mpsc::unbounded();
            match encode_track(&renderer, &frames, duration, &output, settings, progress) {
                Ok(()) => written.push(output),
                Err(reason) => {
                    tracing::warn!(path = %path.display(), %reason, "batch render: the encode failed");
                    failed.push(path);
                }
            }
        }

        (written, failed)
    })
}

/// The tool a digit takes up, in the order the egui board numbers them
/// (`replay/minimap_view/shapes.rs:88-104`): the two apps' readers learn one set
/// of keys. `m` is the measurement, as it is there.
fn tool_for_key(key: &str) -> Option<Tool> {
    Some(match key {
        "1" => Tool::Arrow,
        "2" => Tool::Freehand,
        "3" => Tool::Eraser,
        "4" => Tool::Line,
        "5" => Tool::Circle { filled: false },
        "6" => Tool::Rectangle { filled: false },
        "7" => Tool::Triangle { filled: false },
        "m" => Tool::Measurement,
        _ => return None,
    })
}

/// One rasterised frame, as an image gpui can draw.
fn to_image(frame: image::RgbImage) -> Arc<RenderImage> {
    let (width, height) = frame.dimensions();
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for pixel in frame.pixels() {
        bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let buffer = image::RgbaImage::from_raw(width, height, bgra)
        .expect("the buffer is four bytes per pixel of the size it was built at");
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// Where a render goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExportTarget {
    /// A file the reader chose.
    File,
    /// A temporary file, handed to the clipboard.
    Clipboard,
}

/// A place to render to that outlives this process, for the clipboard.
///
/// `None` when no temporary directory could be made, which is the only way
/// this fails.
fn temporary_output(name: &str) -> Option<PathBuf> {
    let dir = tempfile::Builder::new().prefix("wt-gpui-render-").tempdir().ok()?;
    let path = dir.path().join(name);
    // Kept: the clipboard will hold this path, and a directory removed on
    // drop would leave it pointing at nothing.
    let _ = dir.keep();
    Some(path)
}

/// The file an export writes.
const MP4: crate::dialog::Filter = crate::dialog::Filter { label: "MP4", extensions: &["mp4"] };

/// Rasterises a baked track and encodes it to `output`.
///
/// Runs on a background thread: it draws every frame and blocks on the
/// encoder. The renderer is the viewport's own, which is why the caller hands
/// it over for the duration rather than sharing it.
/// What an export is encoded with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportSettings {
    /// Encode in software even where the GPU could do it.
    pub prefer_cpu: bool,
    /// Start the video at the loading screen rather than at the battle.
    pub include_pre_battle: bool,
    /// The codec to encode with. `None` lets the encoder pick the best one it
    /// can, which is what most readers want.
    pub codec: Option<VideoCodec>,
}

/// What this machine can encode with, probed once.
///
/// The probe builds a GPU device to ask it, which costs long enough to notice,
/// and the answer cannot change while the app runs.
fn encoder_status() -> &'static wows_minimap_renderer::encoder::EncoderStatus {
    static STATUS: std::sync::OnceLock<wows_minimap_renderer::encoder::EncoderStatus> = std::sync::OnceLock::new();
    STATUS.get_or_init(wows_minimap_renderer::check_encoder)
}

/// Which replay of how many a batch has reached.
///
/// `done` counts the ones behind it, so the first report reads as none done of
/// however many there are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchStep {
    pub done: usize,
    pub total: usize,
    /// The replay being read, by its file stem.
    pub replay: String,
}

/// A directory a batch can write into that outlives this process, for the
/// clipboard.
///
/// `None` when no temporary directory could be made, which is the only way this
/// fails.
pub fn temporary_batch_dir() -> Option<PathBuf> {
    let dir = tempfile::Builder::new().prefix("wt-gpui-batch-").tempdir().ok()?;
    let path = dir.path().to_path_buf();
    // Kept: the clipboard will hold paths under it, and a directory removed on
    // drop would leave them pointing at nothing.
    let _ = dir.keep();
    Some(path)
}

/// What a batch draws of what it baked, and whether dead ships are among it.
///
/// The reader's saved layers, less the two a viewport holds off for the same
/// reasons: the stats panel's ship silhouettes are a load the bake skips, so its
/// gutter would be empty and the kill feed and the chat that share it would go
/// unfilmed; and which ships have their range circles drawn is a per-ship choice
/// a batch is never given, so drawing every ship's would bury the map.
fn batch_options(defaults: &wows_minimap_renderer::SavedRenderOptions) -> (RenderOptions, bool) {
    let mut options = playback_options();
    defaults.apply_to(&mut options);
    options.show_stats_panel = false;
    options.show_ship_config = false;
    (options, defaults.show_dead_ships)
}

/// What an export is encoded with, as the reader last saved it.
fn export_settings_of(saved: &wows_minimap_renderer::SavedRenderOptions) -> ExportSettings {
    ExportSettings {
        prefer_cpu: saved.prefer_cpu_encoder,
        codec: saved.video_codec,
        include_pre_battle: saved.include_pre_battle,
    }
}

/// Whether an export has to be encoded in software.
///
/// A codec the GPU cannot encode falls back rather than failing: picking AV1
/// is a choice of codec, not a demand for the GPU.
fn must_use_cpu(settings: &ExportSettings, status: &wows_minimap_renderer::encoder::EncoderStatus) -> bool {
    if settings.prefer_cpu || !status.gpu_available() {
        return true;
    }
    match settings.codec {
        Some(codec) => !status.supports(wows_minimap_renderer::EncoderKind::Gpu, codec),
        None => false,
    }
}

fn encode_track(
    renderer: &SharedPreviewRenderer,
    frames: &[Vec<DrawCommand>],
    duration_seconds: f32,
    output: &std::path::Path,
    settings: ExportSettings,
    progress: futures::channel::mpsc::UnboundedSender<ExportProgress>,
) -> Result<(), String> {
    let mut drawing = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (width, height) = drawing.canvas_size();

    let Some(path) = output.to_str() else {
        return Err(t!("ui.replay.renderer.export_path_unusable").into_owned());
    };
    let mut encoder =
        wows_minimap_renderer::VideoEncoder::new(Some(path), None, false, duration_seconds, width, height);
    encoder.set_prefer_cpu(must_use_cpu(&settings, encoder_status()));
    encoder.set_codec(match settings.codec {
        Some(codec) => wows_minimap_renderer::video::CodecChoice::Explicit(codec),
        None => wows_minimap_renderer::video::CodecChoice::Auto,
    });
    encoder.init().map_err(|err| err.to_string())?;

    let total = frames.len() as u64;
    for (index, commands) in frames.iter().enumerate() {
        let image = drawing.render(commands);
        encoder.submit_frame(&image).map_err(|err| err.to_string())?;
        let _ = progress.unbounded_send(ExportProgress { done: index as u64 + 1, total, stage: ExportStage::Encoding });
    }
    // Said before rather than after, because finishing is the part that
    // takes a while with nothing else to show for it.
    let _ = progress.unbounded_send(ExportProgress { done: total, total, stage: ExportStage::Muxing });
    encoder.finish_submitted().map_err(|err| err.to_string())
}

/// One toggle in the settings popover: what it says, what it reads, and what
/// it sets.
struct Toggle {
    id: &'static str,
    label: &'static str,
    read: fn(&RenderOptions, bool) -> bool,
    write: fn(&mut RenderOptions, &mut bool, bool),
}

/// What the viewport can be told to draw.
///
/// Only the commands the track actually holds. A playback bake adds the team
/// rosters and every ship's range circles to `bake_options`; the stats panel
/// is still left out, because its silhouettes are one of the loads a bake
/// skips to stay cheap, so a switch for it would draw an empty gutter and is
/// not offered. Position trails are derived at raster time rather than baked.
const TOGGLES: &[Toggle] = &[
    Toggle {
        id: "renderer-opt-hp-bars",
        label: "ui.renderer.settings.hp_bars",
        read: |o, _| o.show_hp_bars,
        write: |o, _, v| o.show_hp_bars = v,
    },
    Toggle {
        id: "renderer-opt-tracers",
        label: "ui.renderer.settings.tracers",
        read: |o, _| o.show_tracers,
        write: |o, _, v| o.show_tracers = v,
    },
    Toggle {
        id: "renderer-opt-torpedoes",
        label: "ui.renderer.settings.torpedoes",
        read: |o, _| o.show_torpedoes,
        write: |o, _, v| o.show_torpedoes = v,
    },
    Toggle {
        id: "renderer-opt-planes",
        label: "ui.renderer.settings.planes",
        read: |o, _| o.show_planes,
        write: |o, _, v| o.show_planes = v,
    },
    Toggle {
        id: "renderer-opt-smoke",
        label: "ui.renderer.settings.smoke",
        read: |o, _| o.show_smoke,
        write: |o, _, v| o.show_smoke = v,
    },
    Toggle {
        id: "renderer-opt-consumables",
        label: "ui.renderer.settings.consumables",
        read: |o, _| o.show_consumables,
        write: |o, _, v| o.show_consumables = v,
    },
    Toggle {
        id: "renderer-opt-capture-points",
        label: "ui.renderer.settings.capture_points",
        read: |o, _| o.show_capture_points,
        write: |o, _, v| o.show_capture_points = v,
    },
    Toggle {
        id: "renderer-opt-buildings",
        label: "ui.renderer.settings.buildings",
        read: |o, _| o.show_buildings,
        write: |o, _, v| o.show_buildings = v,
    },
    Toggle {
        id: "renderer-opt-player-names",
        label: "ui.renderer.settings.player_names",
        read: |o, _| o.show_player_names,
        write: |o, _, v| o.show_player_names = v,
    },
    Toggle {
        id: "renderer-opt-ship-names",
        label: "ui.renderer.settings.ship_names",
        read: |o, _| o.show_ship_names,
        write: |o, _, v| o.show_ship_names = v,
    },
    Toggle {
        id: "renderer-opt-dead-ships",
        label: "ui.renderer.settings.dead_ships",
        read: |_, dead| dead,
        write: |_, dead, v| *dead = v,
    },
    Toggle {
        id: "renderer-opt-dead-ship-names",
        label: "ui.renderer.settings.dead_ship_names",
        read: |o, _| o.show_dead_ship_names,
        write: |o, _, v| o.show_dead_ship_names = v,
    },
    Toggle {
        id: "renderer-opt-camera-direction",
        label: "ui.renderer.settings.camera_direction",
        read: |o, _| o.show_camera_direction,
        write: |o, _, v| o.show_camera_direction = v,
    },
    Toggle {
        id: "renderer-opt-armament",
        label: "ui.renderer.settings.armament",
        read: |o, _| o.show_armament,
        write: |o, _, v| o.show_armament = v,
    },
    Toggle {
        id: "renderer-opt-score",
        label: "ui.renderer.settings.score_label",
        read: |o, _| o.show_score,
        write: |o, _, v| o.show_score = v,
    },
    Toggle {
        id: "renderer-opt-timer",
        label: "ui.renderer.settings.timer",
        read: |o, _| o.show_timer,
        write: |o, _, v| o.show_timer = v,
    },
    Toggle {
        id: "renderer-opt-kill-feed",
        label: "ui.renderer.settings.kill_feed",
        read: |o, _| o.show_kill_feed,
        write: |o, _, v| o.show_kill_feed = v,
    },
    Toggle {
        id: "renderer-opt-chat",
        label: "ui.renderer.settings.chat_label",
        read: |o, _| o.show_chat,
        write: |o, _, v| o.show_chat = v,
    },
    Toggle {
        id: "renderer-opt-battle-result",
        label: "ui.renderer.settings.battle_result",
        read: |o, _| o.show_battle_result,
        write: |o, _, v| o.show_battle_result = v,
    },
    Toggle {
        id: "renderer-opt-buffs",
        label: "ui.renderer.settings.buff_counters",
        read: |o, _| o.show_buffs,
        write: |o, _, v| o.show_buffs = v,
    },
    Toggle {
        id: "renderer-opt-trails",
        label: "ui.renderer.settings.heat_trail",
        read: |o, _| o.show_trails,
        write: |o, _, v| o.show_trails = v,
    },
    Toggle {
        id: "renderer-opt-ship-ranges",
        label: "ui.renderer.settings.ship_ranges",
        read: |o, _| o.show_ship_config,
        write: |o, _, v| o.show_ship_config = v,
    },
    Toggle {
        id: "renderer-opt-advantage",
        label: "ui.renderer.settings.team_advantage",
        read: |o, _| o.show_advantage,
        write: |o, _, v| o.show_advantage = v,
    },
    Toggle {
        id: "renderer-opt-team-rosters",
        label: "ui.renderer.settings.team_rosters",
        read: |o, _| o.show_team_rosters,
        // The rosters sit in gutters either side of the map, which the
        // stats panel wants for itself, so turning one on clears the other
        // as the egui checkboxes do.
        write: |o, _, v| {
            o.show_team_rosters = v;
            if v {
                o.show_stats_panel = false;
            }
        },
    },
];

/// The tools the toolbar offers, in the order the egui toolbar offers them.
///
/// Selecting is `Tool::None`: nothing is being drawn, so the pointer goes to
/// the map as it otherwise would.
const TOOLS: &[ToolButton] = &[
    ToolButton {
        id: "replay-renderer-tool-select",
        glyph: crate::icons::CURSOR,
        label: "ui.renderer.annotations.tool_select",
        chord: None,
        tool: || Tool::None,
    },
    ToolButton {
        id: "replay-renderer-tool-arrow",
        glyph: crate::icons::ARROW_BEND_UP_RIGHT,
        label: "ui.renderer.annotations.tool_arrow",
        chord: Some("Ctrl+1"),
        tool: || Tool::Arrow,
    },
    ToolButton {
        id: "replay-renderer-tool-freehand",
        glyph: crate::icons::PAINT_BRUSH,
        label: "ui.renderer.annotations.tool_freehand",
        chord: Some("Ctrl+2"),
        tool: || Tool::Freehand,
    },
    ToolButton {
        id: "replay-renderer-tool-eraser",
        glyph: crate::icons::ERASER,
        label: "ui.renderer.annotations.tool_eraser",
        chord: Some("Ctrl+3"),
        tool: || Tool::Eraser,
    },
    ToolButton {
        id: "replay-renderer-tool-line",
        glyph: crate::icons::LINE_SEGMENT,
        label: "ui.renderer.annotations.tool_line",
        chord: Some("Ctrl+4"),
        tool: || Tool::Line,
    },
    ToolButton {
        id: "replay-renderer-tool-circle",
        glyph: crate::icons::CIRCLE,
        label: "ui.renderer.annotations.tool_circle",
        chord: Some("Ctrl+5"),
        tool: || Tool::Circle { filled: false },
    },
    ToolButton {
        id: "replay-renderer-tool-rectangle",
        glyph: crate::icons::SQUARE,
        label: "ui.renderer.annotations.tool_rectangle",
        chord: Some("Ctrl+6"),
        tool: || Tool::Rectangle { filled: false },
    },
    ToolButton {
        id: "replay-renderer-tool-triangle",
        glyph: crate::icons::TRIANGLE,
        label: "ui.renderer.annotations.tool_triangle",
        chord: Some("Ctrl+7"),
        tool: || Tool::Triangle { filled: false },
    },
    ToolButton {
        id: "replay-renderer-tool-measure",
        glyph: crate::icons::RULER,
        label: "ui.renderer.annotations.tool_measurement",
        chord: Some("Ctrl+M"),
        tool: || Tool::Measurement,
    },
];

/// One button on the toolbar: what it shows, what it is called, the chord that
/// takes it up, and what it puts in hand.
struct ToolButton {
    id: &'static str,
    glyph: &'static str,
    label: &'static str,
    /// `None` for the one tool that has no chord of its own: the selector, which
    /// escape goes back to.
    chord: Option<&'static str>,
    tool: fn() -> Tool,
}

/// One line in the cheat sheet's actions: the chord, and what it does.
const ANNOTATION_ACTIONS: &[(&str, &str)] = &[
    ("Ctrl+Z", "ui.renderer.annotations.action_undo"),
    ("Ctrl+Click", "ui.renderer.annotations.action_multi_select"),
    ("[ / ]", "ui.renderer.annotations.action_stroke_width"),
    ("Del", "ui.renderer.annotations.action_delete"),
    ("Esc", "ui.renderer.annotations.action_cancel"),
];

/// The ships that can be placed on the map, as the egui toolbar offers
/// them, short name first.
const SPECIES: &[(&str, &str)] =
    &[("DD", "Destroyer"), ("CA", "Cruiser"), ("BB", "Battleship"), ("CV", "AirCarrier"), ("SS", "Submarine")];

/// What a placed ship is tinted by which side it is on.
const FRIENDLY_TINT: u32 = 0x4CE8AA;
const ENEMY_TINT: u32 = 0xFE4D2A;

/// A row of ships to place, for one side.
fn species_row(panel: &Entity<ReplayRendererPanel>, chosen: &Tool, friendly: bool) -> AnyElement {
    let tint = if friendly { FRIENDLY_TINT } else { ENEMY_TINT };
    h_flex()
        .gap_1()
        .children(SPECIES.iter().map(|(short, species)| {
            let owner = panel.clone();
            let species = (*species).to_string();
            let on = matches!(
                chosen,
                Tool::Ship { species: in_hand, friendly: side, .. } if in_hand == &species && *side == friendly
            );
            let id = format!("replay-renderer-place-{}-{short}", if friendly { "friendly" } else { "enemy" });
            crate::ui::selectable(
                gpui_kit::SharedString::from(id.clone()),
                on,
                Button::new(gpui_kit::SharedString::from(id))
                    .label((*short).to_string())
                    .compact()
                    .selected(on)
                    .text_color(gpui_kit::rgb(tint))
                    .on_click(move |_event, _window, cx: &mut App| {
                        let species = species.clone();
                        owner.update(cx, |panel, cx| panel.take_up(Tool::Ship { species, friendly, yaw: 0.0 }, cx));
                    }),
            )
        }))
        .into_any_element()
}

/// What a tool draws with, as the egui toolbar offers it.
const INKS: &[(&str, [u8; 4])] = &[
    ("replay-renderer-ink-white", [255, 255, 255, 255]),
    ("replay-renderer-ink-gray", [160, 160, 160, 255]),
    ("replay-renderer-ink-red", [230, 50, 50, 255]),
    ("replay-renderer-ink-orange", [240, 140, 30, 255]),
    ("replay-renderer-ink-yellow", [240, 230, 50, 255]),
    ("replay-renderer-ink-green", [50, 200, 50, 255]),
    ("replay-renderer-ink-blue", [50, 120, 230, 255]),
    ("replay-renderer-ink-purple", [180, 60, 230, 255]),
    ("replay-renderer-ink-pink", [255, 130, 180, 255]),
];

/// The drawing tools, on the transport beside the gear.
///
/// Refused without a session: an annotation is something everyone in one
/// sees, and there is nowhere to put one otherwise.
fn tools_popover(
    panel: &Entity<ReplayRendererPanel>,
    view: &ReplayRendererPanel,
    cx: &Context<ReplayRendererPanel>,
) -> AnyElement {
    let _ = cx;
    let owner = panel.clone();
    let in_session = view.collab.is_active();
    let chosen = view.drawing.tool().clone();
    let ink = view.drawing.color();
    let nib = view.drawing.width();
    let undoable = !view.history.is_empty();
    let picked_ship = picked_ship(view);
    let matches = view.matched_ships.clone();
    let search = view.ship_search.clone();

    Popover::new("replay-renderer-tools")
        .trigger(
            Button::new("replay-renderer-tools-toggle")
                .child(crate::icons::icon(crate::icons::NOTE_PENCIL))
                .compact()
                .disabled(!in_session)
                .tooltip(t!("ui.renderer.annotations.title").into_owned()),
        )
        .content(move |_state, _window, _cx| {
            let owner = owner.clone();
            let chosen = chosen.clone();
            v_flex()
                .w(px(230.))
                .p_2()
                .gap_2()
                .child(h_flex().gap_1().flex_wrap().children(TOOLS.iter().map(|button| {
                    let owner = owner.clone();
                    let tool = button.tool;
                    let on = same_tool(&chosen, &tool());
                    let named = match button.chord {
                        Some(chord) => format!("{} ({chord})", t!(button.label)),
                        None => t!(button.label).into_owned(),
                    };
                    crate::ui::selectable(
                        button.id,
                        on,
                        Button::new(button.id)
                            .child(crate::icons::icon(button.glyph))
                            .compact()
                            .selected(on)
                            .tooltip(named)
                            .on_click(move |_event, _window, cx: &mut App| {
                                owner.update(cx, |panel, cx| panel.take_up(tool(), cx));
                            }),
                    )
                })))
                .children(picked_ship.clone().map(|named| ship_chooser(&owner, &search, named, matches.clone(), _cx)))
                .child(crate::ui::rule_h(_cx))
                .child(species_row(&owner, &chosen, true))
                .child(species_row(&owner, &chosen, false))
                .child(crate::ui::rule_h(_cx))
                .child({
                    let owner = owner.clone();
                    Button::new("replay-renderer-annotations-undo")
                        .child(crate::icons::icon(crate::icons::ARROW_COUNTER_CLOCKWISE))
                        .label(t!("ui.renderer.annotations.undo").into_owned())
                        .compact()
                        .disabled(!undoable)
                        .on_click(move |_event, _window, cx: &mut App| {
                            owner.update(cx, |panel, cx| panel.undo(cx));
                        })
                })
                .child(crate::ui::rule_h(_cx))
                .child(nib_row(&owner, nib))
                .child(h_flex().gap_1().flex_wrap().children(INKS.iter().map(|(id, color)| {
                    let owner = owner.clone();
                    let color = *color;
                    let on = ink == color;
                    div()
                        .id(*id)
                        .test_support()
                        .size(px(18.))
                        .rounded(px(3.))
                        .bg(rgb(u32::from_be_bytes([0, color[0], color[1], color[2]])))
                        .when(on, |this| this.border_2().border_color(gpui_kit::white()))
                        .on_click(move |_event, _window, cx: &mut App| {
                            owner.update(cx, |panel, cx| {
                                panel.drawing.set_color(color);
                                cx.notify();
                            });
                        })
                })))
                .into_any_element()
        })
        .into_any_element()
}

/// How thick what is drawn is, and the two steps either way.
///
/// The bracket keys move the same value, so the number here is what they change.
fn nib_row(panel: &Entity<ReplayRendererPanel>, width: f32) -> AnyElement {
    let thinner = panel.clone();
    let thicker = panel.clone();

    h_flex()
        .gap_1()
        .items_center()
        .child(
            div()
                .flex_1()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.renderer.annotations.nib").into_owned()),
        )
        .child(
            Button::new("replay-renderer-nib-thinner")
                .label("-")
                .compact()
                .disabled(width <= MIN_NIB)
                .tooltip(t!("ui.renderer.annotations.thinner").into_owned())
                .on_click(move |_event, _window, cx: &mut App| {
                    thinner.update(cx, |panel, cx| panel.step_nib(-1.0, cx));
                }),
        )
        .child(
            div()
                .id("replay-renderer-nib")
                .test_support()
                .aria_label(format!("{width:.0}"))
                .w(px(16.))
                .text_xs()
                .child(format!("{width:.0}")),
        )
        .child(
            Button::new("replay-renderer-nib-thicker")
                .label("+")
                .compact()
                .disabled(width >= MAX_NIB)
                .tooltip(t!("ui.renderer.annotations.thicker").into_owned())
                .on_click(move |_event, _window, cx: &mut App| {
                    thicker.update(cx, |panel, cx| panel.step_nib(1.0, cx));
                }),
        )
        .into_any_element()
}

/// What the keys do, shown in the corner while ctrl is held.
///
/// The egui board and renderer both put this up (`minimap_view/shapes.rs`'s
/// `draw_shortcut_overlay`), which is how a reader finds the chords at all: none
/// of them appear anywhere else.
fn shortcut_sheet(cx: &App) -> AnyElement {
    let heading = |text: String| div().text_xs().text_color(crate::theme::text_dim()).child(text);
    let row = |chord: &'static str, named: String| {
        h_flex()
            .gap_2()
            .justify_between()
            .child(div().text_xs().child(chord))
            .child(div().text_xs().text_color(crate::theme::text_dim()).child(named))
    };

    div()
        .absolute()
        .inset_0()
        .flex()
        .justify_end()
        .items_end()
        .p_2()
        .child(
            v_flex()
                .id("replay-renderer-shortcuts")
                .test_support()
                .gap_1()
                .p_2()
                .rounded(px(6.))
                .bg(cx.theme().popover)
                .border_1()
                .border_color(cx.theme().border)
                .child(div().text_xs().child(t!("ui.renderer.annotations.shortcuts_title").into_owned()))
                .child(heading(t!("ui.renderer.annotations.shortcuts_tools").into_owned()))
                .children(
                    TOOLS
                        .iter()
                        .filter_map(|button| button.chord.map(|chord| row(chord, t!(button.label).into_owned()))),
                )
                .child(heading(t!("ui.renderer.annotations.shortcuts_actions").into_owned()))
                .children(ANNOTATION_ACTIONS.iter().map(|(chord, label)| row(chord, t!(*label).into_owned()))),
        )
        .into_any_element()
}

/// The box a placed ship is given an identity in, shown only while one is
/// picked out.
///
/// A ship's ranges are read from its own game data, so until one is chosen
/// there is nothing to read; the egui chooser is the same search over the
/// same catalogue.
fn ship_chooser(
    panel: &Entity<ReplayRendererPanel>,
    search: &Entity<InputState>,
    named: Option<String>,
    matches: Vec<(wowsunpack::game_params::types::Species, ShipEntry)>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(crate::ui::rule_h(cx))
        .child(
            div()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(named.unwrap_or_else(|| t!("ui.renderer.annotations.no_ship").into_owned())),
        )
        .child(Input::new(search).small())
        .children(matches.into_iter().map(|(species, ship)| {
            let owner = panel.clone();
            Button::new(gpui_kit::SharedString::from(format!("replay-renderer-ship-{}", ship.param_index)))
                .label(format!("{} {}", crate::armor_viewer::catalog::tier_roman(ship.tier), ship.display_name))
                .compact()
                .on_click(move |_event, window, cx: &mut App| {
                    let ship = ship.clone();
                    owner.update(cx, |panel, cx| panel.name_picked_ship(species, &ship, window, cx));
                })
        }))
        .into_any_element()
}

/// Whether the picked shape is a ship, and what it is called if one has been
/// chosen for it.
fn picked_ship(view: &ReplayRendererPanel) -> Option<Option<String>> {
    let index = view.picked.single()?;
    let annotations = view.collab.annotations();
    let wt_collab_client::types::Annotation::Ship { config, .. } = annotations.get(index)? else {
        return None;
    };
    Some(config.as_ref().map(|config| config.ship_name.clone()).filter(|name| !name.is_empty()))
}

/// Whether two tools are the same one, ignoring whether a shape is filled.
///
/// A reader pressing the circle button while the circle tool is in hand is
/// not choosing a different tool, so the button reads as already chosen.
fn same_tool(chosen: &Tool, other: &Tool) -> bool {
    std::mem::discriminant(chosen) == std::mem::discriminant(other)
}

/// The gear on the transport: what the viewport draws of what it baked.
///
/// `view` rather than its entity, because this is built inside that view's
/// own render where reading the entity would panic; the entity is captured
/// only for the callbacks.
fn render_options_popover(
    panel: &Entity<ReplayRendererPanel>,
    view: &ReplayRendererPanel,
    ready: bool,
    cx: &Context<ReplayRendererPanel>,
) -> AnyElement {
    let _ = cx;
    let options = view.options().clone();
    let dead = view.show_dead_ships();
    let export = *view.export_settings();
    let owner = panel.clone();

    Popover::new("replay-renderer-settings")
        .trigger(
            Button::new("replay-renderer-settings-toggle")
                .child(crate::icons::icon(crate::icons::GEAR_FINE))
                .compact()
                // Until the bake lands the options are the ones it is baking
                // under, not the reader's, and a Save Defaults would store
                // those over what they chose.
                .disabled(!ready)
                .tooltip(t!("ui.renderer.settings.title").into_owned()),
        )
        .content(move |_state, _window, _cx| {
            let owner = owner.clone();
            let options = options.clone();
            div()
                .w(px(240.))
                .max_h(px(420.))
                .id("replay-renderer-settings-list")
                .test_support()
                .overflow_y_scroll()
                .p_2()
                .child(v_flex().gap_0().children(TOGGLES.iter().map(|toggle| {
                    let owner = owner.clone();
                    let on = (toggle.read)(&options, dead);
                    let write = toggle.write;
                    Checkbox::new(toggle.id)
                        .label(t!(toggle.label).to_string())
                        .checked(on)
                        .on_click(move |checked, _window, cx: &mut App| {
                            let checked = *checked;
                            owner.update(cx, |panel, cx| {
                                panel.set_options(|options, dead| write(options, dead, checked), cx)
                            });
                        })
                        .into_any_element()
                })))
                .child(export_settings_section(&owner, export))
                .child(save_defaults_button(&owner))
                .into_any_element()
        })
        .into_any_element()
}

/// What an export is encoded with, under the display toggles as the egui
/// renderer arranges them.
fn export_settings_section(panel: &Entity<ReplayRendererPanel>, settings: ExportSettings) -> AnyElement {
    let status = encoder_status();
    let chosen = settings.codec;

    v_flex()
        .gap_0()
        .pt_2()
        .child(
            div()
                .pt_1()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(crate::theme::text_dim())
                .child(t!("ui.renderer.settings.export_settings").into_owned()),
        )
        .child({
            let owner = panel.clone();
            Checkbox::new("replay-renderer-prefer-cpu")
                .label(t!("ui.renderer.settings.prefer_cpu").to_string())
                .checked(settings.prefer_cpu)
                .tooltip(t!("ui.renderer.settings.prefer_cpu_tooltip").into_owned())
                .on_click(move |checked, _window, cx: &mut App| {
                    let checked = *checked;
                    owner.update(cx, |panel, cx| {
                        panel.set_export_settings(|settings| settings.prefer_cpu = checked, cx)
                    });
                })
        })
        .child({
            let owner = panel.clone();
            Checkbox::new("replay-renderer-include-pre-battle")
                .label(t!("ui.renderer.settings.include_pre_battle").to_string())
                .checked(settings.include_pre_battle)
                .tooltip(t!("ui.renderer.settings.include_pre_battle_tooltip").into_owned())
                .on_click(move |checked, _window, cx: &mut App| {
                    let checked = *checked;
                    owner.update(cx, |panel, cx| {
                        panel.set_export_settings(|settings| settings.include_pre_battle = checked, cx)
                    });
                })
        })
        .child(
            div()
                .pt_1()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.renderer.settings.codec").into_owned()),
        )
        .child({
            let owner = panel.clone();
            // Auto names the codec it would pick, so the choice is not blind.
            let auto = format!(
                "{} ({})",
                t!("ui.renderer.settings.codec_auto"),
                status.best_codec(settings.prefer_cpu).display_name()
            );
            crate::ui::selectable(
                "replay-renderer-codec-auto",
                chosen.is_none(),
                Button::new("replay-renderer-codec-auto-button")
                    .label(auto)
                    .compact()
                    .selected(chosen.is_none())
                    .on_click(move |_event, _window, cx: &mut App| {
                        owner.update(cx, |panel, cx| panel.set_export_settings(|settings| settings.codec = None, cx));
                    }),
            )
        })
        // Only what this machine can actually encode with.
        .children(status.supported_codecs().map(|codec| {
            let owner = panel.clone();
            let picked = chosen == Some(codec);
            crate::ui::selectable(
                ("replay-renderer-codec", codec as usize),
                picked,
                Button::new(("replay-renderer-codec-button", codec as usize))
                    .label(codec.display_name().to_string())
                    .compact()
                    .selected(picked)
                    .on_click(move |_event, _window, cx: &mut App| {
                        owner.update(cx, |panel, cx| {
                            panel.set_export_settings(|settings| settings.codec = Some(codec), cx)
                        });
                    }),
            )
            .into_any_element()
        }))
        .into_any_element()
}

/// Keeps what this viewport is showing as what the next one opens with.
fn save_defaults_button(panel: &Entity<ReplayRendererPanel>) -> AnyElement {
    let owner = panel.clone();
    div()
        .pt_2()
        .child(
            Button::new("replay-renderer-save-defaults")
                .label(t!("ui.renderer.settings.save_defaults").to_string())
                .compact()
                .on_click(move |_event, window, cx: &mut App| {
                    owner.update(cx, |panel, cx| panel.remember_defaults(window, cx));
                }),
        )
        .into_any_element()
}

/// The transport's event timeline: what happened, filtered, and clickable.
///
/// `view` rather than its entity, because this is built inside that view's
/// own render where reading the entity would panic.
fn timeline_popover(
    panel: &Entity<ReplayRendererPanel>,
    view: &ReplayRendererPanel,
    cx: &Context<ReplayRendererPanel>,
) -> AnyElement {
    let _ = cx;
    let owner = panel.clone();
    let read = view.events_are_read();
    let any_events = !view.events.is_empty();
    let filter = view.event_filter().clone();
    let viewer_team = view.viewer_team;
    let search = view.event_search.clone();
    // Cloned out because the content closure runs after this render returns,
    // when the panel cannot be read.
    let rows: Vec<TimelineRow> = view
        .visible_events()
        .into_iter()
        .map(|event| {
            let (label, hover) = row_text(&event.kind);
            TimelineRow { at: event.clock.seconds(), label, hover, tone: row_tone(&event.kind, viewer_team) }
        })
        .collect();
    let copyable: String = view.visible_events().into_iter().map(format_timeline_event).collect::<Vec<_>>().join("\n");

    Popover::new("replay-renderer-timeline")
        .trigger(
            Button::new("replay-renderer-timeline-toggle")
                .child(crate::icons::icon(crate::icons::LIST_BULLETS))
                .compact()
                .tooltip(t!("ui.renderer.settings.event_timeline").into_owned()),
        )
        .content(move |_state, _window, cx| {
            let owner = owner.clone();
            let filter = filter.clone();
            let rows = rows.clone();
            let copyable = copyable.clone();
            v_flex()
                .w(px(340.))
                .p_2()
                .gap_1()
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .child(t!("ui.renderer.settings.event_timeline").into_owned()),
                        )
                        .child(
                            Button::new("replay-renderer-timeline-copy")
                                .label(t!("ui.buttons.copy").into_owned())
                                .compact()
                                .disabled(rows.is_empty())
                                .tooltip(t!("ui.replay.timeline_export_tooltip").into_owned())
                                .on_click(move |_event, window, cx: &mut App| {
                                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copyable.clone()));
                                    crate::toast::ok(t!("ui.replay.timeline_copied").into_owned(), window, cx);
                                }),
                        ),
                )
                .child(timeline_filter_bar(&owner, &filter, &search))
                .child(crate::ui::rule_h(cx))
                .child(timeline_rows(&owner, rows, read, any_events))
                .into_any_element()
        })
        .into_any_element()
}

/// One row of the timeline, already worded and sided.
#[derive(Clone)]
struct TimelineRow {
    /// Where in the battle it happened, in seconds.
    at: f32,
    label: String,
    hover: String,
    tone: EventTone,
}

/// The kind switches and the search box.
fn timeline_filter_bar(
    panel: &Entity<ReplayRendererPanel>,
    filter: &TimelineFilter,
    search: &Entity<InputState>,
) -> AnyElement {
    let all = panel.clone();
    let none = panel.clone();
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new("replay-renderer-timeline-all")
                        .label(t!("ui.replay.timeline_filter_all").into_owned())
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            all.update(cx, |panel, cx| {
                                panel.set_event_filter(|filter| filter.kinds = [true; KIND_COUNT], cx)
                            });
                        }),
                )
                .child(
                    Button::new("replay-renderer-timeline-none")
                        .label(t!("ui.replay.timeline_filter_none").into_owned())
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            none.update(cx, |panel, cx| {
                                panel.set_event_filter(|filter| filter.kinds = [false; KIND_COUNT], cx)
                            });
                        }),
                )
                .child(div().flex_1().child(Input::new(search).small())),
        )
        .child(v_flex().gap_0().children((0..KIND_COUNT).map(|index| {
            let owner = panel.clone();
            Checkbox::new(("replay-renderer-timeline-kind", index))
                .label(t!(kind_label_key(index)).to_string())
                .checked(filter.kinds[index])
                .on_click(move |checked, _window, cx: &mut App| {
                    let checked = *checked;
                    owner.update(cx, |panel, cx| panel.set_event_filter(|filter| filter.kinds[index] = checked, cx));
                })
                .into_any_element()
        })))
        .into_any_element()
}

/// The filtered list, or what to say instead of one.
fn timeline_rows(
    panel: &Entity<ReplayRendererPanel>,
    rows: Vec<TimelineRow>,
    read: bool,
    any_events: bool,
) -> AnyElement {
    if !read {
        return h_flex()
            .gap_2()
            .items_center()
            .py_2()
            .child(Spinner::new())
            .child(div().text_xs().child(t!("ui.replay.timeline_parsing").into_owned()))
            .into_any_element();
    }
    if rows.is_empty() {
        let said = if any_events { t!("ui.replay.timeline_no_matches") } else { t!("ui.replay.timeline_no_events") };
        return div().py_2().text_xs().text_color(crate::theme::text_dim()).child(said.into_owned()).into_any_element();
    }

    div()
        .id("replay-renderer-timeline-list")
        .test_support()
        .max_h(px(400.))
        .overflow_y_scroll()
        .child(v_flex().gap_0().children(rows.into_iter().enumerate().map(|(index, row)| {
            let owner = panel.clone();
            let at = row.at;
            div()
                .id(("replay-renderer-timeline-row", index))
                .test_support()
                .flex()
                .gap_2()
                .px_1()
                .py_0p5()
                .cursor_pointer()
                .hover(|style| style.bg(crate::theme::surface()))
                .when(!row.hover.is_empty(), |row_div| row_div.tooltip(hover_lines(row.hover.clone().into())))
                .child(div().flex_none().w(px(40.)).text_xs().text_color(crate::theme::text_dim()).child(mmss(row.at)))
                .child(div().flex_1().text_xs().text_color(tone_color(row.tone)).child(row.label))
                .on_click(move |_event, window, cx: &mut App| {
                    owner.update(cx, |panel, cx| panel.go_to_event_at(at, window, cx));
                })
                .into_any_element()
        })))
        .into_any_element()
}

/// The colour a row's side is drawn in.
fn tone_color(tone: EventTone) -> gpui_kit::Hsla {
    let semantic = crate::theme::semantic();
    match tone {
        // The two colours this app already paints a friendly and an enemy
        // team in.
        EventTone::Friendly => rgb(semantic.win).into(),
        EventTone::Enemy => rgb(semantic.loss).into(),
        EventTone::Neutral => crate::theme::text_dim(),
    }
}

/// A hover that puts each newline-separated line on its own row.
fn hover_lines(text: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |window, cx| {
        let text = text.clone();
        gpui_kit::component::tooltip::Tooltip::element(move |_window, _cx| {
            let text = text.clone();
            v_flex().gap_0().text_xs().children(text.split('\n').map(|line| div().child(line.to_string())))
        })
        .build(window, cx)
    }
}

/// The seek slider for a track of `last + 1` frames.
///
/// It selects a frame rather than a fraction: a slider quantises to its step,
/// and the step cannot be finer than the thing being chosen without the
/// rounding throwing frames away.
fn seek_slider(last: usize) -> SliderState {
    SliderState::new().min(0.).max(last.max(1) as f32).step(1.).default_value(0.)
}

/// The zoom control, over what the renderer can draw.
///
/// A tenth of a step, because a zoom is a magnification rather than a count
/// of anything: a whole-number step would jump the map.
fn zoom_slider() -> SliderState {
    SliderState::new().min(MIN_ZOOM).max(MAX_ZOOM).step(0.1).default_value(MIN_ZOOM)
}

#[cfg(test)]
mod tests {
    /// The drawing tools answer to the keys the egui board uses, so a reader who
    /// learned them there does not learn them again.
    #[test]
    fn the_tool_shortcuts_are_the_egui_boards() {
        use wt_collab_client::drawing::Tool;

        assert_eq!(super::tool_for_key("1"), Some(Tool::Arrow));
        assert_eq!(super::tool_for_key("2"), Some(Tool::Freehand));
        assert_eq!(super::tool_for_key("3"), Some(Tool::Eraser));
        assert_eq!(super::tool_for_key("4"), Some(Tool::Line));
        assert_eq!(super::tool_for_key("5"), Some(Tool::Circle { filled: false }));
        assert_eq!(super::tool_for_key("6"), Some(Tool::Rectangle { filled: false }));
        assert_eq!(super::tool_for_key("7"), Some(Tool::Triangle { filled: false }));
        assert_eq!(super::tool_for_key("m"), Some(Tool::Measurement));
        assert_eq!(super::tool_for_key("8"), None, "a key that is not a tool takes nothing up");
    }

    use gpui_kit::AppContext;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;

    use super::ReplayRendererPanel;
    use super::SPEEDS;
    use super::mmss;

    /// The transport reads in the clock the game shows, not in seconds.
    #[test]
    fn game_time_reads_as_minutes_and_seconds() {
        assert_eq!(mmss(0.0), "00:00");
        assert_eq!(mmss(9.4), "00:09");
        assert_eq!(mmss(75.0), "01:15");
        assert_eq!(mmss(1205.0), "20:05");
        // The battle clock runs backwards from the loading screen, so a frame
        // before the battle started reads as its beginning rather than as a
        // minus sign.
        assert_eq!(mmss(-3.0), "00:00");
    }

    /// The transport opens at the speed the egui renderer opens at, over the
    /// ladder it offers. Real time is too slow to watch a battle through, so
    /// a viewport that opens at 1x reads as broken.
    #[gpui_kit::test]
    fn the_transport_opens_at_the_speed_the_egui_renderer_does(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx)
        });

        window
            .update(cx, |panel, _window, _cx| {
                assert_eq!(panel.speed, 20.0);
                assert!(SPEEDS.contains(&panel.speed), "and it is one of the offered speeds");
                assert_eq!(SPEEDS, [1.0, 5.0, 10.0, 20.0, 40.0, 60.0], "the egui ladder");
            })
            .expect("the window is open");
    }

    /// The scrubber addresses every frame, not just the two ends.
    ///
    /// A slider quantises to its step, and the step defaults to 1.0. Built
    /// over a 0-to-1 fraction that rounded every position to one end or the
    /// other, so every drag that did not reach an end looked dead. It is
    /// built over the frames instead.
    #[gpui_kit::test]
    fn the_scrubber_steps_over_the_frames_rather_than_a_fraction(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let clocks: Vec<f32> = (0..40).map(|frame| frame as f32 * 5.0).collect();
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(clocks, window, cx)
        });

        window
            .update(cx, |panel, _window, cx| {
                let slider = panel.seek.read(cx);
                assert_eq!(slider.max_value(), 39.0, "the slider runs over the track, not over 0 to 1");
                // The step is what broke it: quantised against a range of 1,
                // every position rounded to an end.
                assert_eq!(slider.step_value(), 1.0);
                assert!(
                    slider.step_value() <= (slider.max_value() - slider.min_value()) / 2.0,
                    "a step that coarse can only reach the ends"
                );
            })
            .expect("the window is open");
    }

    /// The clock reads the battle's own time, not the recording's.
    ///
    /// A replay starts recording on the loading screen, so the raw clock of
    /// the first frame is already a good half-minute in. The egui renderer
    /// subtracts the battle's start; a port that did not would open a battle
    /// at 00:40.
    #[gpui_kit::test]
    fn the_clock_counts_from_the_battle_rather_than_the_recording(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let clocks: Vec<f32> = (0..40).map(|frame| frame as f32 * 5.0).collect();
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(clocks, window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.set_battle_window(40.0, Some(160.0));
                assert_eq!(panel.clock_label(), "00:00", "the first frame is the loading screen, not minus 40s");

                // Frame 10 is 50s of recording, which is 10s of battle.
                panel.go_to(10, window, cx);
                assert_eq!(panel.clock_label(), "00:10");
            })
            .expect("the window is open");
    }

    /// The skip controls move by game time, not by frames.
    ///
    /// A track is sampled at a fixed interval, so ten seconds is however many
    /// frames that interval divides into; a control that stepped frames would
    /// move a different distance on every replay.
    #[gpui_kit::test]
    fn a_ten_second_skip_moves_ten_seconds(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        // Half-second frames, as a bake produces.
        let clocks: Vec<f32> = (0..200).map(|frame| frame as f32 * 0.5).collect();
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(clocks, window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.go_to(0, window, cx);
                panel.seek_by(10.0, window, cx);
                assert_eq!(panel.at, 20, "twenty half-second frames");

                panel.seek_by(-10.0, window, cx);
                assert_eq!(panel.at, 0);

                // Neither end runs off the track.
                panel.seek_by(-10.0, window, cx);
                assert_eq!(panel.at, 0);
                panel.go_to(199, window, cx);
                panel.seek_by(10.0, window, cx);
                assert_eq!(panel.at, 199);
            })
            .expect("the window is open");
    }

    /// The bar follows the transport's own controls.
    ///
    /// Only a drag of the bar moves playback without moving the bar, since it
    /// is already where the reader put it. A jump that left the thumb behind
    /// would read as not having worked.
    #[gpui_kit::test]
    fn a_jump_takes_the_scrubber_with_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let clocks: Vec<f32> = (0..40).map(|frame| frame as f32 * 5.0).collect();
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(clocks, window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.go_to(39, window, cx);
                assert_eq!(panel.at, 39);
                assert_eq!(panel.seek.read(cx).value().start(), 39.0);

                panel.go_to(0, window, cx);
                assert_eq!(panel.seek.read(cx).value().start(), 0.0);
            })
            .expect("the window is open");
    }

    /// The speed keys walk the offered ladder and stop at its ends.
    #[gpui_kit::test]
    fn the_speed_keys_walk_the_ladder(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5, 1.0], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                assert_eq!(panel.speed, 20.0);
                panel.step_speed(1, window, cx);
                assert_eq!(panel.speed, 40.0);
                panel.step_speed(1, window, cx);
                assert_eq!(panel.speed, 60.0);
                panel.step_speed(1, window, cx);
                assert_eq!(panel.speed, 60.0, "there is no rung past the top of the ladder");

                for _ in 0..5 {
                    panel.step_speed(-1, window, cx);
                }
                assert_eq!(panel.speed, 1.0, "nor below its bottom");
            })
            .expect("the window is open");
    }

    /// The bar marks where the battle began and ended.
    ///
    /// A replay records from the loading screen on and often past the last
    /// shot, so those marks are what tell a reader which stretch of the bar
    /// is the battle.
    #[gpui_kit::test]
    fn the_seek_bar_marks_the_battle_inside_the_recording(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        // A hundred half-second frames: fifty seconds of recording.
        let clocks: Vec<f32> = (0..101).map(|frame| frame as f32 * 0.5).collect();
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(clocks, window, cx)
        });

        window
            .update(cx, |panel, _window, _cx| {
                panel.set_battle_window(10.0, Some(40.0));
                assert_eq!(panel.battle_ticks().len(), 2);

                let track = panel.track().expect("the track is ready");
                assert_eq!(track.position_of(super::GameClock(10.0)), Some(0.2));
                assert_eq!(track.position_of(super::GameClock(40.0)), Some(0.8));

                // A replay cut short before the battle ended has one mark.
                panel.set_battle_window(10.0, None);
                assert_eq!(panel.battle_ticks().len(), 1);
            })
            .expect("the window is open");
    }

    /// A move of the scrubber is a frame to go to.
    #[gpui_kit::test]
    fn a_scrubber_move_lands_on_the_frame_it_names(cx: &mut TestAppContext) {
        use gpui_kit::component::slider::SliderEvent;
        use gpui_kit::component::slider::SliderValue;

        cx.update(gpui_kit::init);
        let clocks: Vec<f32> = (0..40).map(|frame| frame as f32 * 5.0).collect();
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(clocks, window, cx)
        });

        window
            .update(cx, |panel, _window, cx| {
                panel.on_seek_event(&SliderEvent::Change(SliderValue::Single(17.0)), cx);
                assert_eq!(panel.at, 17, "the middle of the track is reachable");

                panel.on_seek_event(&SliderEvent::Change(SliderValue::Single(3.0)), cx);
                assert_eq!(panel.at, 3);

                // Past the end clamps rather than panicking on the index.
                panel.on_seek_event(&SliderEvent::Change(SliderValue::Single(999.0)), cx);
                assert_eq!(panel.at, 39);
            })
            .expect("the window is open");
    }

    /// A viewport opens showing what was last saved as the defaults, and the
    /// gear's Save Defaults writes what it is showing back.
    #[gpui_kit::test]
    fn a_viewport_opens_showing_what_was_saved_as_the_defaults(cx: &mut TestAppContext) {
        use wows_minimap_renderer::SavedRenderOptions;

        cx.update(gpui_kit::init);
        cx.update(|cx| {
            crate::render_defaults::adopt(
                SavedRenderOptions {
                    show_torpedoes: false,
                    show_player_names: true,
                    include_pre_battle: true,
                    prefer_cpu_encoder: true,
                    // Saved by the egui renderer, which has the silhouettes for
                    // it; a bake here does not.
                    show_stats_panel: true,
                    // Kept for the egui renderer rather than read here, so a
                    // save from this app must not clear it.
                    show_self_radar_range: true,
                    ..SavedRenderOptions::default()
                },
                cx,
            );
        });

        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.adopt_saved_defaults(cx);

                assert!(!panel.options().show_torpedoes, "a layer turned off stays off");
                assert!(panel.options().show_player_names, "and one turned on comes back on");
                assert!(!panel.options().show_stats_panel, "the panel a bake cannot fill is not drawn");
                assert!(panel.export_settings().include_pre_battle);
                assert!(panel.export_settings().prefer_cpu);

                panel.set_options(|options, _dead| options.show_smoke = false, cx);
                panel.remember_defaults(window, cx);

                let saved = crate::render_defaults::defaults(cx);
                assert!(!saved.show_smoke, "what the viewport shows is what is saved");
                assert!(!saved.show_torpedoes);
                assert!(saved.show_self_radar_range, "the ranges this app does not read are left as they were");
                assert!(saved.include_pre_battle, "as are the export settings it did not change");
                assert!(
                    saved.show_stats_panel,
                    "and the panel this viewport holds off is not turned off for the egui renderer"
                );
            })
            .expect("the window is open");
    }

    /// The bracket keys widen and narrow the nib, within the range the egui
    /// board holds it to, and only while a tool is in hand.
    #[gpui_kit::test]
    fn the_bracket_keys_move_the_nib_within_its_range(cx: &mut TestAppContext) {
        use wt_collab_client::drawing::Tool;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx)
        });

        window
            .update(cx, |panel, _window, cx| {
                let opened_at = panel.drawing.width();

                panel.take_up(Tool::Freehand, cx);
                panel.step_nib(1.0, cx);
                assert!(panel.drawing.width() > opened_at, "] draws thicker");

                for _ in 0..20 {
                    panel.step_nib(1.0, cx);
                }
                assert_eq!(panel.drawing.width(), super::MAX_NIB, "and stops at the thickest the board offers");

                for _ in 0..20 {
                    panel.step_nib(-1.0, cx);
                }
                assert_eq!(panel.drawing.width(), super::MIN_NIB, "as it does at the thinnest");
            })
            .expect("the window is open");
    }

    /// Every chord the cheat sheet lists is one the viewport answers to, so the
    /// sheet cannot drift from the keys.
    #[test]
    fn the_cheat_sheet_lists_the_chords_that_work() {
        for button in super::TOOLS {
            let Some(chord) = button.chord else { continue };
            let key = chord.strip_prefix("Ctrl+").expect("every tool chord is a ctrl one").to_ascii_lowercase();
            assert_eq!(
                super::tool_for_key(&key).map(|tool| std::mem::discriminant(&tool)),
                Some(std::mem::discriminant(&(button.tool)())),
                "{chord} is listed as {}",
                button.label
            );
        }
    }

    /// A batch says which replay it has reached before it reads it, in order,
    /// and a replay it cannot read is reported as failed rather than stopping
    /// the rest.
    #[gpui_kit::test]
    async fn a_batch_says_which_replay_it_has_reached(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().expect("a temp directory");
        // Not replays, so every one of them fails to bake: what is under test is
        // the reporting and the walk, not what a battle looks like.
        let paths: Vec<std::path::PathBuf> = ["first", "second"]
            .into_iter()
            .map(|stem| {
                let path = dir.path().join(format!("{stem}.wowsreplay"));
                std::fs::write(&path, b"not a replay").expect("the file is written");
                path
            })
            .collect();

        let (report, mut steps) = futures::channel::mpsc::unbounded();
        let batch = cx.update(|cx| {
            super::batch_export(
                paths,
                crate::replay_inspector::load::GameDataCache::new(dir.path().to_path_buf()),
                dir.path().to_path_buf(),
                wows_minimap_renderer::SavedRenderOptions::default(),
                report,
                cx,
            )
        });
        let (written, failed) = batch.await;

        assert!(written.is_empty(), "nothing renders out of files that are not replays");
        assert_eq!(failed.len(), 2, "and both are reported rather than the walk stopping at the first");

        let mut reached = Vec::new();
        while let Some(step) = futures::StreamExt::next(&mut steps).await {
            reached.push(step);
        }
        assert_eq!(
            reached,
            vec![
                super::BatchStep { done: 0, total: 2, replay: "first".to_owned() },
                super::BatchStep { done: 1, total: 2, replay: "second".to_owned() },
            ],
            "each replay is named as the batch reaches it"
        );
    }

    /// A batch draws the reader's saved layers, less the two that would leave
    /// it worse off than the built-in set did.
    #[test]
    fn a_batch_draws_the_saved_layers_without_the_two_it_cannot_fill() {
        use wows_minimap_renderer::SavedRenderOptions;
        use wows_minimap_renderer::config::should_draw_command;

        let saved = SavedRenderOptions {
            show_torpedoes: false,
            // On in the egui renderer, which has what it takes to fill them.
            show_stats_panel: true,
            show_ship_config: true,
            show_dead_ships: false,
            ..SavedRenderOptions::default()
        };
        let (options, show_dead_ships) = super::batch_options(&saved);

        assert!(!options.show_torpedoes, "a layer the reader turned off is not filmed");
        assert!(!show_dead_ships);
        assert!(!options.show_stats_panel, "the panel a bake cannot fill is left out");
        assert!(!options.show_ship_config, "as are range circles nobody asked for per ship");
        assert!(
            should_draw_command(&super::DrawCommand::KillFeed { entries: Vec::new() }, &options, show_dead_ships),
            "so the kill feed, which shares the panel's gutter, is still filmed"
        );
    }

    /// A toggle changes what the viewport draws without re-baking: the
    /// commands are filtered as a frame is rasterised, so the track is
    /// untouched and the options are what moved.
    #[gpui_kit::test]
    fn a_display_toggle_changes_what_is_drawn_without_rebaking(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx)
        });

        window
            .update(cx, |panel, _window, cx| {
                let baked = panel.frame_count();
                assert!(panel.options().show_torpedoes, "the track was baked with them on");

                panel.set_options(|options, _dead| options.show_torpedoes = false, cx);

                assert!(!panel.options().show_torpedoes);
                assert_eq!(panel.frame_count(), baked, "the track is untouched; only what is drawn of it moved");
            })
            .expect("the window is open");
    }

    /// Dead ships are gated beside the options rather than in them, as the
    /// egui viewer gates them, so the toggle has to reach its own flag.
    #[gpui_kit::test]
    fn the_dead_ship_toggle_reaches_its_own_flag(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0], window, cx)
        });

        window
            .update(cx, |panel, _window, cx| {
                assert!(panel.show_dead_ships());
                panel.set_options(|_options, dead| *dead = false, cx);
                assert!(!panel.show_dead_ships());
            })
            .expect("the window is open");
    }

    /// The control that moves this viewport into a window of its own is
    /// offered while it is in the dock and withdrawn once it has one, so a
    /// popped-out viewport cannot be popped out again.
    #[gpui_kit::test]
    fn the_pop_out_control_is_withdrawn_once_the_viewport_has_a_window(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx)
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("replay-renderer-pop-out").is_some(), "a docked viewport offers it");
        })
        .expect("the window is open");

        window
            .update(cx, |panel, _window, cx| {
                assert!(!panel.is_popped_out());
                panel.mark_popped_out(cx);
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("replay-renderer-pop-out").is_none(), "one with a window of its own does not");
            // The transport is still there; only that one control went.
            assert!(window.try_find("replay-renderer-play").is_some());
        })
        .expect("the window is open");
    }

    /// A wheel over the map zooms about what is under the pointer, which is
    /// what makes a zoom feel like it is following the reader rather than the
    /// corner of the map.
    #[gpui_kit::test]
    fn the_wheel_zooms_about_what_is_under_the_pointer(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        use gpui_kit::ScrollDelta;
        use gpui_kit::ScrollWheelEvent;
        use gpui_kit::TouchPhase;
        use gpui_kit::point;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.seed_frame_for_test();
                // A point on the map, and the same point in window
                // coordinates: the frame reserves a strip above the map.
                let on_map = (200.0_f32, 300.0_f32);
                let at = point(px(on_map.0), px(on_map.1 + super::DEFAULT_MAP_ORIGIN.1));
                let before = panel.view().to_map(on_map.0, on_map.1);

                let event = ScrollWheelEvent {
                    position: at,
                    delta: ScrollDelta::Pixels(point(px(0.), px(120.))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                };
                panel.on_scroll(&event, window, cx);

                assert!(panel.view().zoom() > 1.0, "the wheel zoomed in");
                let after = panel.view().to_map(on_map.0, on_map.1);
                assert!(
                    (before.0 - after.0).abs() < 1.0 && (before.1 - after.1).abs() < 1.0,
                    "the map did not slide out from under the pointer: {before:?} then {after:?}"
                );
            })
            .expect("the window is open");
    }

    /// A wheel outside the map does nothing.
    ///
    /// The strip above the map carries the score bar and the timer, which do
    /// not zoom; a wheel there has no map point to zoom about.
    #[gpui_kit::test]
    fn a_wheel_over_the_hud_does_not_zoom(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        use gpui_kit::ScrollDelta;
        use gpui_kit::ScrollWheelEvent;
        use gpui_kit::TouchPhase;
        use gpui_kit::point;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.seed_frame_for_test();
                let event = ScrollWheelEvent {
                    position: point(px(200.), px(4.)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(120.))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                };
                panel.on_scroll(&event, window, cx);
                assert!(panel.view().is_whole_map());
            })
            .expect("the window is open");
    }

    /// The zoom control moves the view, and Reset puts the whole map back.
    #[gpui_kit::test]
    fn the_zoom_control_and_reset_agree_with_the_view(cx: &mut TestAppContext) {
        use gpui_kit::component::slider::SliderEvent;
        use gpui_kit::component::slider::SliderValue;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.seed_frame_for_test();
                let state = panel.zoom.clone();
                panel.on_zoom(&state, &SliderEvent::Change(SliderValue::Single(4.0)), window, cx);
                assert_eq!(panel.view().zoom(), 4.0);
                assert!(!panel.view().is_whole_map());
                assert_eq!(panel.zoom.read(cx).value().start(), 4.0, "the control says what the view shows");

                panel.set_view(super::MapViewport::default(), window, cx);
                assert!(panel.view().is_whole_map());
                assert_eq!(panel.zoom.read(cx).value().start(), 1.0, "and Reset takes the control with it");
            })
            .expect("the window is open");
    }

    /// Dragging the whole map does nothing: there is nowhere to drag to.
    #[gpui_kit::test]
    fn the_whole_map_does_not_drag(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        use gpui_kit::MouseButton;
        use gpui_kit::MouseDownEvent;
        use gpui_kit::point;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.seed_frame_for_test();
                let press = MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(px(200.), px(300.)),
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                };
                panel.on_drag_start(&press, window, cx);
                assert!(panel.dragging.is_none(), "the whole map has nowhere to drag to");
            })
            .expect("the window is open");
    }

    /// A viewport inside a `Root`, which is what the toast layer needs, with
    /// its entity kept so a test can drive it.
    fn viewport_in_root(
        cx: &mut TestAppContext,
        clocks: Vec<f32>,
    ) -> (gpui_kit::WindowHandle<gpui_kit::component::Root>, gpui_kit::Entity<ReplayRendererPanel>) {
        cx.update(gpui_kit::init);
        let panel = std::cell::RefCell::new(None);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            let view = cx.new(|cx| ReplayRendererPanel::ready_for_test(clocks, window, cx));
            *panel.borrow_mut() = Some(view.clone());
            let view: gpui_kit::AnyView = view.into();
            gpui_kit::component::Root::new(view, window, cx)
        });
        let panel = panel.borrow_mut().take().expect("the viewport was built inside the window");
        (window, panel)
    }

    /// The event controls step to the next thing that happened, and stop at
    /// each end of the battle.
    ///
    /// Stepping back looks half a second behind the clock, as the egui
    /// renderer does: without that, landing on an event and pressing back
    /// again would find the same one and never move.
    #[gpui_kit::test]
    fn the_event_controls_step_between_what_happened(cx: &mut TestAppContext) {
        // Half-second frames over two minutes of recording.
        let clocks: Vec<f32> = (0..240).map(|frame| frame as f32 * 0.5).collect();
        let (window, panel) = viewport_in_root(cx, clocks);

        cx.update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                // The battle starts forty seconds into the recording, so an
                // event at 10s of battle is at 50s of track.
                panel.set_battle_window(40.0, Some(110.0));
                panel.seed_events_for_test(&[10.0, 30.0, 60.0]);

                assert!(panel.previous_event().is_none(), "nothing happened before the battle began");
                panel.jump_to_next_event(window, cx);
                assert_eq!(panel.clock_label(), "00:10");

                panel.jump_to_next_event(window, cx);
                assert_eq!(panel.clock_label(), "00:30");

                // Back from an event finds the one before it, not itself.
                panel.jump_to_previous_event(window, cx);
                assert_eq!(panel.clock_label(), "00:10");

                panel.set_at(239, cx);
                assert!(panel.next_event().is_none(), "nothing happened after the last one");
            });
        })
        .expect("the window is open");
    }

    /// With no events read yet, the controls have nowhere to go.
    ///
    /// The second walk of the replay lands after the viewport opens, and a
    /// control that jumped to the start of the battle in the meantime would
    /// read as broken.
    #[gpui_kit::test]
    fn the_event_controls_do_nothing_until_the_battle_has_been_read(cx: &mut TestAppContext) {
        let clocks: Vec<f32> = (0..40).map(|frame| frame as f32 * 0.5).collect();
        let (window, panel) = viewport_in_root(cx, clocks);

        cx.update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.go_to(10, window, cx);
                assert!(panel.previous_event().is_none());
                assert!(panel.next_event().is_none());

                panel.jump_to_next_event(window, cx);
                panel.jump_to_previous_event(window, cx);
                assert_eq!(panel.at, 10, "playback stayed where it was");
            });
        })
        .expect("the window is open");
    }

    /// An export starts at the battle unless the reader asked for what came
    /// before it.
    ///
    /// A replay records the loading screen and the countdown; exporting those
    /// by default would put most of a minute of a still map at the front of
    /// every video.
    #[gpui_kit::test]
    fn an_export_leaves_out_the_pre_battle_phase_unless_it_is_asked_for(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        // Half-second frames: the battle starts forty seconds in, at frame 80.
        let clocks: Vec<f32> = (0..200).map(|frame| frame as f32 * 0.5).collect();
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(clocks, window, cx)
        });

        window
            .update(cx, |panel, _window, cx| {
                panel.set_battle_window(40.0, Some(95.0));
                let track = panel.track().expect("the track is ready");
                assert_eq!(track.frames.len(), 200);

                assert_eq!(panel.frames_to_export(track).len(), 120, "the eighty frames before the battle are cut");

                panel.set_export_settings(|settings| settings.include_pre_battle = true, cx);
                let track = panel.track().expect("the track is ready");
                assert_eq!(panel.frames_to_export(track).len(), 200, "and kept when asked for");
            })
            .expect("the window is open");
    }

    /// A codec the GPU cannot encode falls back to software rather than
    /// failing the export.
    #[test]
    fn a_codec_the_gpu_cannot_encode_falls_back_to_software() {
        use wows_minimap_renderer::EncoderKind;
        use wows_minimap_renderer::VideoCodec;
        use wows_minimap_renderer::encoder::CodecSupport;
        use wows_minimap_renderer::encoder::EncoderStatus;

        let mut status = EncoderStatus {
            gpu_adapter_name: Some("test".into()),
            gpu_error: None,
            gpu_codecs: Default::default(),
            cpu_codecs: Default::default(),
        };
        status.gpu_codecs.insert(VideoCodec::H264, CodecSupport::Supported);
        status.cpu_codecs.insert(VideoCodec::Av1, CodecSupport::Supported);
        assert!(status.supports(EncoderKind::Gpu, VideoCodec::H264));

        let settings = super::ExportSettings::default();
        assert!(!super::must_use_cpu(&settings, &status), "the GPU handles the codec it would pick");

        let asked_for_av1 = super::ExportSettings { codec: Some(VideoCodec::Av1), ..settings };
        assert!(super::must_use_cpu(&asked_for_av1, &status), "AV1 has no GPU encoder here, so it falls back");

        let asked_for_cpu = super::ExportSettings { prefer_cpu: true, ..settings };
        assert!(super::must_use_cpu(&asked_for_cpu, &status));

        // Availability is read off the codec table, not the adapter name.
        let no_gpu = EncoderStatus { gpu_codecs: Default::default(), ..status };
        assert!(!no_gpu.gpu_available());
        assert!(super::must_use_cpu(&settings, &no_gpu), "with no GPU there is nothing to fall back from");
    }

    /// The timeline shows the events the filter admits, and says which of
    /// "nothing happened" and "nothing matches" it means.
    #[gpui_kit::test]
    fn the_timeline_filter_decides_what_the_list_shows(cx: &mut TestAppContext) {
        use wows_replay_insights::timeline::KIND_COUNT;
        use wows_replay_insights::timeline::kind_index;

        let clocks: Vec<f32> = (0..200).map(|frame| frame as f32 * 0.5).collect();
        let (window, panel) = viewport_in_root(cx, clocks);

        cx.update_window(window.into(), |_, _window, cx| {
            panel.update(cx, |panel, cx| {
                assert!(!panel.events_are_read(), "the second walk has not landed yet");

                panel.seed_events_for_test(&[10.0, 30.0, 60.0]);
                panel.events_read = true;
                assert_eq!(panel.visible_events().len(), 3);

                // Every seeded event is an advantage change, so switching that
                // kind off empties the list without emptying the battle.
                let advantage = kind_index(&panel.events[0].kind);
                panel.set_event_filter(|filter| filter.kinds[advantage] = false, cx);
                assert!(panel.visible_events().is_empty());
                assert!(!panel.events.is_empty(), "which is 'nothing matches', not 'nothing happened'");

                panel.set_event_filter(|filter| filter.kinds = [true; KIND_COUNT], cx);
                panel.set_event_filter(|filter| filter.search = "at 30".into(), cx);
                assert_eq!(panel.visible_events().len(), 1, "the search looks through what a row says");
            });
        })
        .expect("the window is open");
    }

    /// Clicking a row moves playback to when it happened.
    #[gpui_kit::test]
    fn clicking_a_timeline_row_seeks_to_it(cx: &mut TestAppContext) {
        let clocks: Vec<f32> = (0..200).map(|frame| frame as f32 * 0.5).collect();
        let (window, panel) = viewport_in_root(cx, clocks);

        cx.update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.set_battle_window(40.0, Some(95.0));
                panel.go_to_event_at(30.0, window, cx);
                assert_eq!(panel.clock_label(), "00:30");
            });
        })
        .expect("the window is open");
    }

    /// A playback bake carries every ship's range circles, so the viewport can
    /// offer them without walking the battle again.
    ///
    /// The bake reads a real replay against a real game install. Run with:
    ///
    /// ```text
    /// WOWS_RENDERER_TEST_REPLAY="E:\WoWs\World_of_Warships\replays\some.wowsreplay"     /// WOWS_RENDERER_TEST_GAME_DIR="E:\WoWs\World_of_Warships"     /// cargo test -p wows-toolkit-gpui -- --ignored a_playback_bake_carries_every_ships_ranges
    /// ```
    #[test]
    #[ignore = "needs a local game install and a replay; see the doc comment for the run command"]
    fn a_playback_bake_carries_every_ships_ranges() {
        use std::collections::HashSet;
        use std::path::PathBuf;
        use std::sync::atomic::AtomicBool;

        use super::DrawCommand;
        use crate::replay_inspector::GameDataCache;

        let replay = std::env::var("WOWS_RENDERER_TEST_REPLAY").expect("set WOWS_RENDERER_TEST_REPLAY to a replay");
        let game_dir =
            std::env::var("WOWS_RENDERER_TEST_GAME_DIR").expect("set WOWS_RENDERER_TEST_GAME_DIR to a WoWs install");

        let cache = GameDataCache::new(PathBuf::from(game_dir));
        let cancel = AtomicBool::new(false);
        let baked = crate::minimap_preview::bake_track(
            std::path::Path::new(&replay),
            &[],
            &cache,
            &cancel,
            super::TRACK_BUDGET,
            super::BAKE_INTERVAL,
            super::playback_options(),
        )
        .expect("the replay bakes");

        let mut ships: HashSet<String> = HashSet::new();
        let mut kinds: HashSet<String> = HashSet::new();
        for commands in &baked.frames {
            for command in commands {
                if let DrawCommand::ShipConfigCircle { player_name, kind, .. } = command {
                    ships.insert(player_name.clone());
                    kinds.insert(format!("{kind:?}"));
                }
            }
        }

        assert!(ships.len() > 1, "every ship's ranges are baked, not just the replay owner's: got {ships:?}");
        assert!(kinds.len() > 1, "and more than one kind of range: got {kinds:?}");
    }

    /// Turning one ship's ranges on draws ranges; turning the last one off
    /// stops drawing them.
    ///
    /// The global switch is what decides whether the layer is drawn at all,
    /// so leaving it on with nothing selected would be a layer that shows
    /// nothing, and leaving it off would ignore the ship that was asked for.
    #[gpui_kit::test]
    fn asking_for_one_ships_ranges_turns_the_layer_on_and_off_with_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, _window, cx| {
                assert!(!panel.options().show_ship_config, "nothing is asked for yet");
                assert_eq!(panel.ranges_for("gapedd"), super::NO_RANGES);

                panel.set_ranges_for("gapedd", |filter| filter.detection = true, cx);
                assert!(panel.options().show_ship_config);
                assert!(panel.ranges_for("gapedd").detection);
                assert!(!panel.ranges_for("someone_else").detection, "only the ship that was asked for");

                panel.set_ranges_for("gapedd", |filter| filter.detection = false, cx);
                assert!(!panel.options().show_ship_config, "the last one off puts the layer away");
            })
            .expect("the window is open");
    }

    /// Hiding a trail is per ship, and the layer follows the last one.
    #[gpui_kit::test]
    fn hiding_the_last_trail_puts_the_layer_away(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, _window, cx| {
                assert!(panel.trail_shown("gapedd"), "a ship shows its trail until it is hidden");

                panel.set_trail_shown("gapedd", true, cx);
                assert!(panel.options().show_trails, "asking for one turns trails on");

                // The test track carries no ship commands, so every drawn ship
                // is hidden the moment this one is.
                panel.set_trail_shown("gapedd", false, cx);
                assert!(!panel.trail_shown("gapedd"));
                assert!(!panel.options().show_trails);
            })
            .expect("the window is open");
    }

    /// The per-ship filters decide whose trail and whose ranges are drawn.
    #[test]
    fn the_per_ship_filters_decide_whose_layers_are_drawn() {
        use std::collections::HashMap;
        use std::collections::HashSet;

        use wows_minimap_renderer::draw_command::ShipConfigCircleKind;
        use wows_minimap_renderer::map_data::MinimapPos;

        use super::DrawCommand;

        let mut hidden = HashSet::new();
        hidden.insert("hidden_player".to_string());
        let mut ranges = HashMap::new();
        ranges.insert("ranged_player".to_string(), super::ALL_RANGES);

        let trail = |name: &str| DrawCommand::PositionTrail {
            entity_id: wows_replays::types::EntityId::from(1u32),
            player_name: Some(name.to_string()),
            points: Vec::new(),
        };
        let circle = |name: &str| DrawCommand::ShipConfigCircle {
            entity_id: wows_replays::types::EntityId::from(1u32),
            pos: MinimapPos { x: 0.0, y: 0.0 },
            radius_px: 10.0,
            color: [0, 0, 0],
            alpha: 1.0,
            dashed: false,
            label: None,
            kind: ShipConfigCircleKind::Detection,
            player_name: name.to_string(),
            is_self: false,
        };

        assert!(super::per_ship_allows(&trail("shown_player"), &hidden, &ranges));
        assert!(!super::per_ship_allows(&trail("hidden_player"), &hidden, &ranges));
        assert!(super::per_ship_allows(&circle("ranged_player"), &hidden, &ranges));
        assert!(!super::per_ship_allows(&circle("unranged_player"), &hidden, &ranges), "a ship nobody asked for");
    }

    /// The advantage hover reads the breakdown out of the frame on screen.
    ///
    /// The label is drawn into the frame by the renderer, so there is nothing
    /// to hover that knows what it says; this reads the command behind it.
    #[gpui_kit::test]
    fn the_advantage_hover_reads_the_frames_own_breakdown(cx: &mut TestAppContext) {
        use wows_minimap_renderer::advantage::AdvantageBreakdown;
        use wowsunpack::game_types::AdvantageLevel;

        use super::DrawCommand;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, _window, _cx| {
                assert!(panel.advantage_hover().is_none(), "a frame with no advantage has nothing to say");

                let breakdown = AdvantageBreakdown {
                    score_projection: (6.0, 2.0),
                    total: (6.0, 2.0),
                    hp_data_reliable: false,
                    ..Default::default()
                };
                panel.set_frame_commands_for_test(vec![DrawCommand::TeamAdvantage {
                    level: Some(AdvantageLevel::Weak),
                    color: [255, 255, 255],
                    breakdown,
                }]);

                let lines = panel.advantage_hover().expect("the frame carries an advantage");
                assert!(lines.iter().any(|line| line.contains("+4.0")), "the difference is shown: {lines:?}");
                assert!(
                    lines.iter().any(|line| line.contains("incomplete")),
                    "and that the HP data was not reliable: {lines:?}"
                );
            })
            .expect("the window is open");
    }

    /// A map point projects to where the pointer that named it was, so a
    /// peer's cursor lands where they are pointing.
    ///
    /// This is the inverse of the picking projection; the two have to agree or
    /// a shared pointer drifts from what it is over.
    #[gpui_kit::test]
    fn a_map_point_and_a_pointer_position_agree_both_ways(cx: &mut TestAppContext) {
        use gpui_kit::point;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.seed_frame_for_test();

                for zoom in [1.0_f32, 3.5] {
                    panel.set_view(super::MapViewport::new(zoom, (40.0, 90.0)), window, cx);

                    let at = point(px(300.), px(200.0 + super::DEFAULT_MAP_ORIGIN.1));
                    let on_map = panel.map_point(at).expect("the pointer is over the map");
                    let (left, top) = panel.element_point(on_map).expect("and the map point is on screen");

                    assert!((left.as_f32() - at.x.as_f32()).abs() < 0.5, "x at zoom {zoom}");
                    assert!((top.as_f32() - at.y.as_f32()).abs() < 0.5, "y at zoom {zoom}");
                }
            })
            .expect("the window is open");
    }

    /// A map point the viewport is not showing has nowhere to be drawn.
    #[gpui_kit::test]
    fn a_point_outside_the_view_is_not_placed(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.seed_frame_for_test();
                assert!(panel.element_point((10.0, 10.0)).is_some(), "the whole map shows its corner");

                // Zoomed into the far corner, the near one is off screen.
                panel.set_view(super::MapViewport::new(4.0, (2304.0, 2304.0)), window, cx);
                assert!(panel.element_point((10.0, 10.0)).is_none());
            })
            .expect("the window is open");
    }

    /// Right-clicking a ship picks it at any zoom.
    ///
    /// A pointer position and a ship's position have to be compared in the
    /// same space. Picking used to compare a drawn-layer point against a
    /// map-space one, so it only agreed at zoom 1 and picked the wrong ship,
    /// or none, as soon as the reader zoomed in.
    #[gpui_kit::test]
    fn a_ship_is_picked_at_any_zoom(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        use gpui_kit::MouseButton;
        use gpui_kit::MouseDownEvent;
        use gpui_kit::point;
        use wows_minimap_renderer::ShipVisibility;
        use wows_minimap_renderer::map_data::MinimapPos;

        use super::DrawCommand;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.seed_frame_for_test();
                let on_map = MinimapPos { x: 300.0, y: 400.0 };
                panel.set_frame_commands_for_test(vec![DrawCommand::Ship {
                    entity_id: wows_replays::types::EntityId::from(7u32),
                    pos: on_map,
                    yaw: 0.0,
                    species: None,
                    color: None,
                    visibility: ShipVisibility::Visible,
                    opacity: 1.0,
                    is_self: false,
                    player_name: Some("gapedd".into()),
                    ship_name: None,
                    is_detected_teammate: false,
                    is_disconnected: false,
                    name_color: None,
                }]);

                for zoom in [1.0_f32, 2.0, 5.0] {
                    panel.set_view(super::MapViewport::new(zoom, (200.0, 300.0)), window, cx);
                    let Some((left, top)) = panel.element_point((on_map.x, on_map.y)) else {
                        continue;
                    };
                    let press = MouseDownEvent {
                        button: MouseButton::Right,
                        position: point(left, top),
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    };
                    panel.on_right_click(&press, window, cx);
                    let picked = panel.ship_menu().map(|menu| menu.player_name.clone());
                    assert_eq!(picked.as_deref(), Some("gapedd"), "at zoom {zoom}");
                    panel.close_ship_menu(cx);
                }
            })
            .expect("the window is open");
    }

    /// Playing advances through the track and stops at the end rather than
    /// running past it, and the clock reads the game time of where it got to.
    #[gpui_kit::test]
    fn playing_walks_the_track_and_stops_at_its_end(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0, 61.0], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                assert_eq!(panel.clock_label(), "00:00", "it opens at the start of the battle");
                panel.toggle_playing(window, cx);
                assert!(panel.playing);
            })
            .expect("the window is open");

        // Two ticks reach the last frame; the third would run past it.
        for _ in 0..3 {
            cx.executor().advance_clock(super::TICK * 2);
            cx.run_until_parked();
        }

        window
            .update(cx, |panel, _window, _cx| {
                assert_eq!(panel.at, 2, "playback reached the last frame");
                assert!(!panel.playing, "and stopped there rather than running past it");
                assert_eq!(panel.clock_label(), "01:01");
            })
            .expect("the window is open");
    }

    /// A viewport with nothing baked yet has no frames to walk, and the
    /// transport must not divide by their count.
    #[gpui_kit::test]
    fn an_empty_track_has_nowhere_to_play_to(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(Vec::new(), window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                assert_eq!(panel.clock_label(), "00:00");
                panel.toggle_playing(window, cx);
                panel.set_at(5, cx);
                assert_eq!(panel.at, 0, "there is nowhere to seek to");
            })
            .expect("the window is open");
    }

    /// A viewport in a session redraws on its own.
    ///
    /// A peer's pointer moves and their pings ripple while this app sits
    /// still, and a paused viewport is told to redraw by nothing else, so
    /// what the last redraw happened to catch would stay on the map. The
    /// tick shedding a finished ping is what makes it visible from here.
    #[gpui_kit::test]
    fn a_running_session_redraws_without_being_touched(cx: &mut TestAppContext) {
        use std::time::Duration;

        cx.update(gpui_kit::init);
        let state = std::sync::Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
        crate::collab::push_ping_aged(&state, [10.0, 10.0], Duration::from_secs(5));
        crate::collab::push_ping_aged(&state, [20.0, 20.0], Duration::ZERO);
        let (link, _sent) = crate::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel
        });

        assert_eq!(state.lock().pings.len(), 2, "both are there before the session has ticked");

        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();

        let held = state.lock();
        assert_eq!(held.pings.len(), 1, "the finished ripple is shed");
        assert_eq!(held.pings[0].pos, [20.0, 20.0], "and the one still running is kept");
        drop(held);

        // Handing back an inert link stops it: a viewport with no session
        // must not sit redrawing for nothing.
        window
            .update(cx, |panel, _window, cx| {
                panel.set_collab(crate::collab::CollabLink::default(), cx);
            })
            .expect("the window is open");
        crate::collab::push_ping_aged(&state, [30.0, 30.0], Duration::from_secs(5));
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert_eq!(state.lock().pings.len(), 2, "nothing is shed once the session has gone");
    }

    /// A roster icon reads what the frame drew, looked up through the region
    /// the target recorded as it drew it.
    #[gpui_kit::test]
    fn a_roster_icon_reads_what_the_frame_drew(cx: &mut TestAppContext) {
        use gpui_kit::point;
        use wows_minimap_renderer::draw_command::ChargeCount;
        use wows_minimap_renderer::draw_command::ConsumableAvailability;
        use wows_minimap_renderer::draw_command::RosterConsumable;
        use wows_minimap_renderer::draw_command::RosterRow;
        use wows_minimap_renderer::draw_command::RosterSide;
        use wows_minimap_renderer::drawing::DrawnRegion;
        use wows_minimap_renderer::drawing::RegionKind;

        use super::DrawCommand;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        let entity_id = wows_replays::types::EntityId::from(7u32);
        let consumable = |name: &str| RosterConsumable {
            icon_key: format!("PCY_{name}"),
            display_name: name.to_string(),
            description: String::new(),
            total_charges: ChargeCount::Finite(3),
            charges_used: 0,
            work_time_secs: 0.0,
            reload_time_secs: 0.0,
            active_remaining_secs: None,
            availability: ConsumableAvailability::Ready,
        };

        window
            .update(cx, |panel, _window, _cx| {
                panel.seed_frame_for_test();
                panel.set_frame_commands_for_test(vec![DrawCommand::TeamRoster {
                    side: RosterSide::Friendly,
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 400,
                    rows: vec![RosterRow {
                        entity_id,
                        team_id: 0,
                        player_name: "gapedd".to_string(),
                        clan_tag: None,
                        clan_color: None,
                        ship_name: "Smaland".to_string(),
                        ship_param_id: None,
                        class_icon_key: None,
                        species: None,
                        hp_current: 100.0,
                        hp_max: 100.0,
                        hp_healable: 0.0,
                        hp_healable_per_charge: 0.0,
                        heal_availability: ConsumableAvailability::Unavailable,
                        is_dead: false,
                        is_self: false,
                        is_spotted: false,
                        is_disconnected: false,
                        kills: 0,
                        damage_dealt: 0.0,
                        seconds_since_damage: None,
                        consumables: vec![consumable("Damage Control"), consumable("Repair Party")],
                    }],
                }]);
                // The frame is laid out at its own size, so a canvas pixel is
                // a window pixel.
                panel.set_regions_for_test(vec![
                    DrawnRegion {
                        rect: [10.0, 10.0, 20.0, 20.0],
                        kind: RegionKind::RosterConsumable { entity_id, index: 0 },
                    },
                    DrawnRegion {
                        rect: [40.0, 10.0, 20.0, 20.0],
                        kind: RegionKind::RosterConsumable { entity_id, index: 1 },
                    },
                ]);

                let first = panel.consumable_at(point(px(15.), px(15.))).expect("the pointer is on the first icon");
                assert_eq!(first.lines.name, "Damage Control");
                assert_eq!(first.player, "gapedd", "and it says whose it is");

                let second = panel.consumable_at(point(px(45.), px(15.))).expect("and on the second");
                assert_eq!(second.lines.name, "Repair Party", "the index picks the icon, not the row");

                assert!(panel.consumable_at(point(px(32.), px(15.))).is_none(), "between two icons is neither");
                assert!(panel.consumable_at(point(px(400.), px(400.))).is_none(), "and the map is not a roster");
            })
            .expect("the window is open");
    }

    /// The rosters and the stats panel compete for the same gutters, so
    /// turning one on clears the other, as the egui checkboxes do.
    #[test]
    fn the_rosters_and_the_stats_panel_do_not_share_the_gutters() {
        let toggle = super::TOGGLES
            .iter()
            .find(|toggle| toggle.id == "renderer-opt-team-rosters")
            .expect("the rosters have a switch");
        let mut options = wows_minimap_renderer::RenderOptions { show_stats_panel: true, ..super::playback_options() };
        let mut dead = true;

        (toggle.write)(&mut options, &mut dead, true);
        assert!(options.show_team_rosters);
        assert!(!options.show_stats_panel, "the stats panel gives the gutter up");

        (toggle.write)(&mut options, &mut dead, false);
        assert!(!options.show_team_rosters);
        assert!(!options.show_stats_panel, "and turning the rosters off does not bring it back");
    }

    /// Turning the rosters on is a wider canvas rather than another layer, so
    /// the layout has to follow the switch.
    #[test]
    fn the_canvas_follows_the_gutter_layer_that_is_on() {
        use wows_minimap_renderer::drawing::SidePanelLayout;

        let plain = super::playback_options_hidden();
        assert_eq!(crate::minimap_preview::layout_for(&plain), SidePanelLayout::None);

        let rosters = wows_minimap_renderer::RenderOptions { show_team_rosters: true, ..plain.clone() };
        assert_eq!(crate::minimap_preview::layout_for(&rosters), SidePanelLayout::TeamRosters);

        let stats = wows_minimap_renderer::RenderOptions { show_stats_panel: true, ..plain };
        assert_eq!(crate::minimap_preview::layout_for(&stats), SidePanelLayout::StatsPanel);
    }

    /// A playback bake carries the rosters, or the switch would have nothing
    /// to show.
    #[test]
    fn a_playback_bake_carries_the_rosters() {
        assert!(super::playback_options().show_team_rosters);
        assert!(!super::playback_options_hidden().show_team_rosters, "but a viewport opens without them");
    }

    /// What a session has drawn reaches the frame, over the battle rather
    /// than filtered with it.
    #[gpui_kit::test]
    fn what_a_session_has_drawn_is_put_on_the_frame(cx: &mut TestAppContext) {
        use super::DrawCommand;
        use wows_minimap_renderer::config::should_draw_command;
        use wt_collab_client::types::Annotation;

        cx.update(gpui_kit::init);
        let state = std::sync::Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
        state.lock().current_annotation_sync = Some(wt_collab_client::AnnotationSyncState {
            annotations: vec![Annotation::Circle {
                center: [400.0, 300.0],
                radius: 40.0,
                color: [255, 0, 0, 255],
                width: 2.0,
                filled: false,
            }],
            ..Default::default()
        });
        let (link, _sent) = crate::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel
        });

        window
            .update(cx, |panel, _window, _cx| {
                let drawn: Vec<DrawCommand> = panel
                    .collab
                    .annotations()
                    .iter()
                    .flat_map(wt_collab_client::geometry::annotation_commands)
                    .collect();
                assert_eq!(drawn.len(), 1, "the circle became one command");
                // No display option hides what a reader drew deliberately.
                let hidden = super::playback_options_hidden();
                assert!(should_draw_command(&drawn[0], &hidden, false), "and nothing filters it out");
            })
            .expect("the window is open");
    }

    /// Drawing with a tool sends what it drew to the session, and the
    /// pointer goes to the tool rather than to panning the map.
    #[gpui_kit::test]
    fn a_tool_in_hand_draws_rather_than_pans(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        use gpui_kit::MouseButton;
        use gpui_kit::MouseDownEvent;
        use gpui_kit::MouseUpEvent;
        use gpui_kit::point;
        use wt_collab_client::drawing::Tool;
        use wt_collab_client::peer::LocalAnnotationEvent;
        use wt_collab_client::peer::LocalEvent;
        use wt_collab_client::types::Annotation;

        cx.update(gpui_kit::init);
        let state = std::sync::Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
        let (link, sent) = crate::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, window, cx| {
                let zoomed = super::MapViewport::new(2.0, (100.0, 100.0));
                panel.set_view(zoomed, window, cx);
                panel.take_up(Tool::Line, cx);

                let from = panel.element_point((200.0, 200.0)).expect("a point the viewport is showing");
                let to = panel.element_point((300.0, 260.0)).expect("and another");
                let press = MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(from.0, from.1),
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                };
                panel.on_drag_start(&press, window, cx);
                assert!(panel.dragging.is_none(), "the drag went to the tool, not to a pan");
                assert_eq!(panel.view, zoomed, "so the map did not move");

                let release = MouseUpEvent {
                    button: MouseButton::Left,
                    position: point(to.0, to.1),
                    modifiers: Modifiers::default(),
                    click_count: 1,
                };
                panel.on_drag_end(&release, window, cx);
            })
            .expect("the window is open");

        let drawn: Vec<Annotation> = sent
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::Annotation(LocalAnnotationEvent::Set { annotation, .. }) => Some(annotation),
                _ => None,
            })
            .collect();
        let [Annotation::Line { start, end, .. }] = &drawn[..] else {
            panic!("the line tool sent one line, got {drawn:?}");
        };
        assert!((start[0] - 200.0).abs() < 1.0 && (start[1] - 200.0).abs() < 1.0, "drawn in map space: {start:?}");
        assert!((end[0] - 300.0).abs() < 1.0 && (end[1] - 260.0).abs() < 1.0, "{end:?}");
    }

    /// Without a tool in hand a drag still pans, so drawing does not cost the
    /// reader the map.
    #[gpui_kit::test]
    fn without_a_tool_a_drag_still_pans(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        use gpui_kit::MouseButton;
        use gpui_kit::MouseDownEvent;
        use gpui_kit::point;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, window, cx| {
                panel.set_view(super::MapViewport::new(2.0, (100.0, 100.0)), window, cx);
                let at = panel.element_point((200.0, 200.0)).expect("a point the viewport is showing");
                let press = MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(at.0, at.1),
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                };
                panel.on_drag_start(&press, window, cx);
                assert!(panel.dragging.is_some());
            })
            .expect("the window is open");
    }

    /// A shape picked out and dragged moves, and the session hears about it
    /// once, at the end, under the id it already knows.
    #[gpui_kit::test]
    fn a_picked_shape_moves_with_the_drag(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        use gpui_kit::MouseButton;
        use gpui_kit::MouseDownEvent;
        use gpui_kit::MouseUpEvent;
        use gpui_kit::point;
        use wt_collab_client::AnnotationSyncState;
        use wt_collab_client::peer::LocalAnnotationEvent;
        use wt_collab_client::peer::LocalEvent;
        use wt_collab_client::types::Annotation;

        cx.update(gpui_kit::init);
        let state = std::sync::Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
        state.lock().current_annotation_sync = Some(AnnotationSyncState {
            annotations: vec![Annotation::Circle {
                center: [200.0, 200.0],
                radius: 20.0,
                color: [255, 255, 255, 255],
                width: 2.0,
                filled: false,
            }],
            ids: vec![4242],
            owners: vec![7],
        });
        let (link, sent) = crate::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, window, cx| {
                // On the circle's own edge, which is where a hit test finds
                // it.
                let on = panel.element_point((220.0, 200.0)).expect("a point the viewport is showing");
                let click = gpui_kit::ClickEvent::Mouse(gpui_kit::MouseClickEvent {
                    down: MouseDownEvent {
                        button: MouseButton::Left,
                        position: point(on.0, on.1),
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    },
                    up: MouseUpEvent {
                        button: MouseButton::Left,
                        position: point(on.0, on.1),
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    },
                });
                panel.on_viewport_click(&click, window, cx);
                assert_eq!(panel.picked.picked(), [0], "the click picked the circle out");

                let press = MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(on.0, on.1),
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                };
                panel.on_drag_start(&press, window, cx);
                assert!(panel.dragging.is_none(), "the drag moves the shape rather than the map");

                let to = panel.element_point((260.0, 230.0)).expect("and another");
                let release = MouseUpEvent {
                    button: MouseButton::Left,
                    position: point(to.0, to.1),
                    modifiers: Modifiers::default(),
                    click_count: 1,
                };
                panel.on_drag_end(&release, window, cx);
            })
            .expect("the window is open");

        let moved: Vec<(u64, Annotation)> = sent
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::Annotation(LocalAnnotationEvent::Set { id, annotation, .. }) => Some((id, annotation)),
                _ => None,
            })
            .collect();
        let [(id, Annotation::Circle { center, .. })] = &moved[..] else {
            panic!("one update for the one shape, got {moved:?}");
        };
        assert_eq!(*id, 4242, "under the id the session already knows");
        assert!((center[0] - 240.0).abs() < 1.0 && (center[1] - 230.0).abs() < 1.0, "moved by the drag: {center:?}");
    }

    /// A part-drawn shape reaches the frame, so a reader dragging sees what
    /// they are about to get rather than nothing until they let go.
    #[gpui_kit::test]
    fn a_part_drawn_shape_reaches_the_frame(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        use gpui_kit::MouseButton;
        use gpui_kit::MouseDownEvent;
        use gpui_kit::MouseMoveEvent;
        use gpui_kit::point;
        use wt_collab_client::drawing::Tool;

        cx.update(gpui_kit::init);
        let state = std::sync::Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
        let (link, _sent) = crate::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, window, cx| {
                panel.take_up(Tool::Circle { filled: false }, cx);
                let from = panel.element_point((200.0, 200.0)).expect("a point the viewport is showing");
                let to = panel.element_point((240.0, 200.0)).expect("and another");

                panel.on_drag_start(
                    &MouseDownEvent {
                        button: MouseButton::Left,
                        position: point(from.0, from.1),
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    },
                    window,
                    cx,
                );
                panel.on_drag_move(
                    &MouseMoveEvent {
                        position: point(to.0, to.1),
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Modifiers::default(),
                    },
                    window,
                    cx,
                );

                assert_eq!(panel.pointer_at.map(|at| at[0].round()), Some(240.0), "the pointer is followed");
                let part_drawn = panel.drawing.in_progress(panel.pointer_at.expect("the pointer is on the map"));
                let Some(wt_collab_client::types::Annotation::Circle { center, radius, .. }) = part_drawn else {
                    panic!("a circle is part-drawn, got {part_drawn:?}");
                };
                assert!((center[0] - 200.0).abs() < 1.0 && (center[1] - 200.0).abs() < 1.0, "{center:?}");
                assert!((radius - 40.0).abs() < 1.0, "as wide as the drag has gone: {radius}");
            })
            .expect("the window is open");
    }

    /// Drawing puts the session back within reach, and undoing takes the
    /// shape off again under the id it was added with.
    #[gpui_kit::test]
    fn drawing_without_a_session_still_puts_a_shape_on_the_map(cx: &mut TestAppContext) {
        use wt_collab_client::drawing::Stroke as DrawStroke;
        use wt_collab_client::drawing::Tool;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, _window, cx| {
                panel.take_up(Tool::Line, cx);
                panel.stroke(DrawStroke::Began { at: [100.0, 100.0] }, cx);
                panel.stroke(DrawStroke::Ended { at: [200.0, 100.0] }, cx);
                assert_eq!(panel.collab.annotations().len(), 1, "the line a reader drew with nobody connected");
            })
            .expect("the window is open");
    }

    #[gpui_kit::test]
    fn undoing_takes_back_what_was_just_drawn(cx: &mut TestAppContext) {
        use wt_collab_client::AnnotationSyncState;
        use wt_collab_client::drawing::Stroke as DrawStroke;
        use wt_collab_client::drawing::Tool;
        use wt_collab_client::peer::LocalAnnotationEvent;
        use wt_collab_client::peer::LocalEvent;
        use wt_collab_client::types::Annotation;

        cx.update(gpui_kit::init);
        let state = std::sync::Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
        let (link, sent) = crate::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, _window, cx| {
                assert!(panel.history.is_empty(), "nothing to undo before anything is drawn");

                panel.take_up(Tool::Line, cx);
                panel.stroke(DrawStroke::Began { at: [100.0, 100.0] }, cx);
                panel.stroke(DrawStroke::Ended { at: [200.0, 100.0] }, cx);
                assert_eq!(panel.history.len(), 1, "the list as it was before the line");
            })
            .expect("the window is open");

        // The session now holds what was drawn, under the id it was sent
        // with, which is what an undo has to name.
        let drawn: Vec<(u64, Annotation)> = sent
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::Annotation(LocalAnnotationEvent::Set { id, annotation, .. }) => Some((id, annotation)),
                _ => None,
            })
            .collect();
        let [(id, annotation)] = &drawn[..] else { panic!("one line was drawn, got {drawn:?}") };
        state.lock().current_annotation_sync =
            Some(AnnotationSyncState { annotations: vec![annotation.clone()], ids: vec![*id], owners: vec![0] });

        window
            .update(cx, |panel, _window, cx| {
                panel.undo(cx);
                assert!(panel.history.is_empty(), "and there is nothing left to undo");
            })
            .expect("the window is open");

        let removed: Vec<u64> = sent
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::Annotation(LocalAnnotationEvent::Remove { id, .. }) => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(removed, vec![*id], "the line goes, under the id it arrived with");
    }

    /// Placing a ship sends one, tinted by the side it was placed for and
    /// facing the way the tool was holding it.
    #[gpui_kit::test]
    fn placing_a_ship_sends_one_for_the_side_it_was_placed_for(cx: &mut TestAppContext) {
        use wt_collab_client::drawing::Stroke as DrawStroke;
        use wt_collab_client::drawing::Tool;
        use wt_collab_client::peer::LocalAnnotationEvent;
        use wt_collab_client::peer::LocalEvent;
        use wt_collab_client::types::Annotation;

        cx.update(gpui_kit::init);
        let state = std::sync::Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
        let (link, sent) = crate::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, _window, cx| {
                panel.take_up(Tool::Ship { species: "Destroyer".to_string(), friendly: false, yaw: 0.75 }, cx);
                panel.stroke(DrawStroke::Clicked { at: [150.0, 250.0] }, cx);
            })
            .expect("the window is open");

        let placed: Vec<Annotation> = sent
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::Annotation(LocalAnnotationEvent::Set { annotation, .. }) => Some(annotation),
                _ => None,
            })
            .collect();
        let [Annotation::Ship { pos, yaw, species, friendly, .. }] = &placed[..] else {
            panic!("one ship was placed, got {placed:?}");
        };
        assert_eq!(*pos, [150.0, 250.0]);
        assert_eq!(*yaw, 0.75);
        assert_eq!(species, "Destroyer");
        assert!(!friendly, "for the side the tool was holding");

        // And it draws as a ship rather than as nothing.
        let drawn = wt_collab_client::geometry::annotation_commands(&placed[0]);
        assert!(matches!(drawn.as_slice(), [super::DrawCommand::Ship { .. }]), "{drawn:?}");
    }

    /// Naming a placed ship gives it an identity, which is what its ranges
    /// are read from, and leaves everything else about it alone.
    #[gpui_kit::test]
    fn naming_a_placed_ship_gives_it_an_identity(cx: &mut TestAppContext) {
        use wt_collab_client::AnnotationSyncState;
        use wt_collab_client::peer::LocalAnnotationEvent;
        use wt_collab_client::peer::LocalEvent;
        use wt_collab_client::types::Annotation;

        cx.update(gpui_kit::init);
        let state = std::sync::Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
        state.lock().current_annotation_sync = Some(AnnotationSyncState {
            annotations: vec![Annotation::Ship {
                pos: [300.0, 400.0],
                yaw: 0.5,
                species: "Cruiser".to_string(),
                friendly: false,
                config: None,
            }],
            ids: vec![11],
            owners: vec![3],
        });
        let (link, sent) = crate::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, _window, _cx| {
                // Nothing to choose a ship for until one is picked out.
                assert!(super::picked_ship(panel).is_none());
                panel.picked.click(&panel.collab.annotations(), [300.0, 400.0], false);
                assert_eq!(super::picked_ship(panel), Some(None), "picked, and unnamed");
            })
            .expect("the window is open");

        // Nothing was sent by picking alone.
        assert!(sent.try_iter().next().is_none(), "picking a shape changes nothing");

        window
            .update(cx, |panel, window, cx| {
                let ship = crate::armor_viewer::catalog::ShipEntry {
                    param_index: "PRSC610".to_string(),
                    display_name: "Moskva".to_string(),
                    search_name: "moskva".to_string(),
                    tier: 10,
                };
                // Without game data loaded there is no param to resolve, so
                // nothing is sent rather than a ship with an id of nothing.
                panel.name_picked_ship(wowsunpack::game_params::types::Species::Cruiser, &ship, window, cx);
            })
            .expect("the window is open");

        let updates: Vec<Annotation> = sent
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::Annotation(LocalAnnotationEvent::Set { annotation, .. }) => Some(annotation),
                _ => None,
            })
            .collect();
        assert!(updates.is_empty(), "a ship with no game data behind it is not named: {updates:?}");
    }

    /// The armor item is offered only for a ship the battle recorded hits
    /// on, since where those landed is the whole point of the viewer.
    #[gpui_kit::test]
    fn the_armor_item_is_offered_only_for_a_ship_that_was_hit(cx: &mut TestAppContext) {
        use std::collections::BTreeMap;
        use wows_replay_insights::timeline::ShipShotTimeline;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx)
        });

        let hit_ship = wows_replays::types::EntityId::from(7u32);
        let untouched = wows_replays::types::EntityId::from(8u32);

        window
            .update(cx, |panel, _window, _cx| {
                assert!(!panel.has_armor_to_show(hit_ship), "before the battle has been read");

                // A ship with a timeline but nothing in it was never hit.
                panel.shots.insert(untouched, ShipShotTimeline { hits: Vec::new(), health_history: BTreeMap::new() });
                assert!(!panel.has_armor_to_show(untouched), "a timeline with no hits is not an offer");

                // Asking for one anyway is refused rather than opening an
                // empty viewer: without game data there is no ship to load.
                panel.show_armor(untouched, _cx);
            })
            .expect("the window is open");
    }

    /// A viewer that is open follows playback, and is only disturbed when
    /// the hits it shows actually change: showing them again rebuilds the
    /// hull, which is far more than a frame of playback is worth.
    #[gpui_kit::test]
    fn an_open_armor_viewer_follows_playback_but_only_when_it_must(cx: &mut TestAppContext) {
        use std::collections::BTreeMap;
        use wows_replay_insights::timeline::HealthSnapshot;
        use wows_replay_insights::timeline::ShipShotTimeline;
        use wows_replays::types::GameClock;

        cx.update(gpui_kit::init);
        let clocks: Vec<f32> = (0..20).map(|frame| frame as f32 * 5.0).collect();
        let window = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(clocks, window, cx)
        });

        let ship = wows_replays::types::EntityId::from(7u32);
        let mut health_history = BTreeMap::new();
        health_history.insert(GameClock(0.0), HealthSnapshot { health: 15400.0, max_health: 15400.0 });
        health_history.insert(GameClock(30.0), HealthSnapshot { health: 12000.0, max_health: 15400.0 });

        let told = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let counter = std::rc::Rc::clone(&told);
        let panel = cx.update(|cx| window.root(cx).expect("the viewport is open"));
        let _subscription = cx.update(|cx| {
            cx.subscribe(&panel, move |_view, event, _cx| {
                if matches!(event, super::RendererEvent::ArmorFollowed { .. }) {
                    counter.set(counter.get() + 1);
                }
            })
        });

        window
            .update(cx, |panel, _window, cx| {
                panel.shots.insert(ship, ShipShotTimeline { hits: Vec::new(), health_history });
                // Following starts without a viewer, which is what the
                // menu's own guard is for; seeded here directly.
                let mut feed = crate::armor_viewer::realtime::RealtimeArmorFeed::new(
                    panel.shots.get(&ship).expect("just inserted").clone(),
                );
                feed.advance_to(GameClock(0.0));
                panel.armor_following = Some((ship, feed));

                // This battle recorded no hits, so walking it never changes
                // what the viewer shows.
                for frame in 1..6 {
                    panel.set_at(frame, cx);
                }
            })
            .expect("the window is open");

        assert_eq!(told.get(), 0, "nothing changed, so the viewer was left alone");

        // Scrubbing back is the other case: what was drawn can no longer be
        // right, and a hull cannot be un-hit one shell at a time, so the
        // viewer is told to draw it again.
        window.update(cx, |panel, _window, cx| panel.set_at(1, cx)).expect("the window is open");
        assert_eq!(told.get(), 1, "a step backwards asks for the hull again");
    }

    /// The clock says where playback is and how long the battle runs, so
    /// the end is visible without reading the bar.
    #[gpui_kit::test]
    fn the_clock_says_where_playback_is_and_how_long_there_is(cx: &mut TestAppContext) {
        // Half-second frames over a minute of recording.
        let clocks: Vec<f32> = (0..120).map(|frame| frame as f32 * 0.5).collect();
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(clocks, window, cx)
        });

        window
            .update(cx, |panel, _window, cx| {
                // The battle starts forty seconds into the recording, so the
                // clock counts from there and so does the length.
                panel.set_battle_window(40.0, None);
                assert_eq!(panel.clock_label(), "00:00", "the loading screen is not minus forty");
                assert_eq!(panel.length_label(), "00:19", "and the battle runs to the end of the track");
                assert_eq!(panel.clock_and_length(), "00:00 / 00:19");

                panel.set_at(100, cx);
                assert_eq!(panel.clock_and_length(), "00:10 / 00:19", "the length does not move with playback");
            })
            .expect("the window is open");
    }

    /// The speed keys walk the ladder, and the dropdown follows them rather
    /// than being left showing the speed before last.
    #[gpui_kit::test]
    fn the_speed_dropdown_follows_the_keys(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                assert_eq!(panel.speed, super::DEFAULT_SPEED);
                assert_eq!(
                    panel.speed_select.read(cx).selected_value().copied(),
                    Some(super::speed_index(super::DEFAULT_SPEED)),
                    "the dropdown opens on the speed the viewport does"
                );

                panel.set_speed(5.0, window, cx);
                assert_eq!(
                    panel.speed_select.read(cx).selected_value().copied(),
                    Some(super::speed_index(5.0)),
                    "and follows a change made anywhere else"
                );
            })
            .expect("the window is open");
    }

    /// An export says how far it has got over the video, and says which part
    /// of the work is running: muxing comes after the last frame and takes a
    /// noticeable while with nothing else to show for it.
    #[gpui_kit::test]
    fn an_export_reports_its_progress_over_the_video(cx: &mut TestAppContext) {
        use super::ExportProgress;
        use super::ExportStage;

        cx.update(gpui_kit::init);
        let panel = std::cell::RefCell::new(None);
        let window = cx.open_window(size(px(900.), px(700.)), |window, cx| {
            let view = cx.new(|cx| {
                let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
                panel.seed_frame_for_test();
                panel
            });
            *panel.borrow_mut() = Some(view.clone());
            let view: gpui_kit::AnyView = view.into();
            gpui_kit::component::Root::new(view, window, cx)
        });
        let panel = panel.borrow_mut().take().expect("the viewport was built inside the window");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("replay-renderer-export-overlay").is_none(), "nothing is being exported");

            panel.update(cx, |panel, cx| {
                panel.export = Some(ExportProgress { done: 40, total: 100, stage: ExportStage::Encoding });
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.try_find("replay-renderer-export-overlay").is_some(), "the bar is over the video");
        })
        .expect("the window is open");

        assert_eq!(ExportStage::Encoding.label(), "ui.renderer.encoding");
        assert_eq!(ExportStage::Muxing.label(), "ui.renderer.muxing", "and the wait after the last frame is named");
    }

    /// A wide viewport fills its sides as the map is zoomed into, and stays
    /// square while there is no map to put there.
    #[gpui_kit::test]
    fn a_wide_viewport_fills_its_sides_as_the_map_is_zoomed(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1600.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.seed_frame_for_test();
                // A viewport twice as wide as the canvas is tall.
                let frame = panel.frame.as_ref().expect("a frame was seeded").size(0);
                let (w, h) = (frame.width.0 as f32, frame.height.0 as f32);
                panel.set_drawn_for_test(gpui_kit::Bounds {
                    origin: gpui_kit::point(px(0.), px(0.)),
                    size: gpui_kit::size(px(w * 2.0), px(h)),
                });

                assert_eq!(panel.shaped_view().widen(), 1.0, "at rest there is no map to put beside it");

                panel.set_view(super::MapViewport::new(1.2, (0.0, 0.0)), window, cx);
                let creeping = panel.shaped_view().widen();
                assert!(creeping > 1.0 && creeping <= 1.2, "it starts to fill, held to the zoom: {creeping}");

                panel.set_view(super::MapViewport::new(6.0, (0.0, 0.0)), window, cx);
                let filled = panel.shaped_view().widen();
                assert!(filled > creeping, "and fills further in: {filled} against {creeping}");
                assert!(filled > 1.9 && filled < 2.1, "about the shape of the viewport: {filled}");
            })
            .expect("the window is open");
    }

    /// A ping is shed only once its ripple has run out.
    #[test]
    fn a_ping_outlives_its_own_frame() {
        use std::time::Duration;

        let state = std::sync::Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
        crate::collab::push_ping_aged(&state, [10.0, 10.0], Duration::from_millis(200));
        let (link, _sent) = crate::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        link.drop_stale_pings(Duration::from_secs_f32(super::PING_SECONDS));
        assert_eq!(state.lock().pings.len(), 1, "a fifth of a second into a one-second ripple");

        link.drop_stale_pings(Duration::from_millis(100));
        assert!(state.lock().pings.is_empty(), "and it goes once it has run out");
    }
}

/// Driving the viewport's own controls, rather than the methods behind them.
///
/// The rest of this file's tests call a handler directly, which says nothing
/// about whether the control that calls it is on screen, enabled, or wired to
/// it at all. These click the elements, so a control that stops being drawn
/// or starts refusing a click fails a test rather than only being noticed by
/// someone looking at it.
#[cfg(test)]
mod controls {
    use super::ReplayRendererPanel;
    use gpui_kit::AppContext;
    use gpui_kit::Entity;
    use gpui_kit::TestAppContext;
    use gpui_kit::WindowHandle;
    use gpui_kit::component::Root;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;

    /// A viewport mounted the way production mounts it, wide enough that the
    /// whole transport is in one frame: the harness refuses to click what it
    /// cannot see.
    fn viewport(cx: &mut TestAppContext, clocks: Vec<f32>) -> (WindowHandle<Root>, Entity<ReplayRendererPanel>) {
        cx.update(gpui_kit::init);
        let panel = std::cell::RefCell::new(None);
        let window = cx.open_window(size(px(1400.), px(700.)), |window, cx| {
            let view = cx.new(|cx| ReplayRendererPanel::ready_for_test(clocks, window, cx));
            *panel.borrow_mut() = Some(view.clone());
            let view: gpui_kit::AnyView = view.into();
            Root::new(view, window, cx)
        });
        let panel = panel.borrow_mut().take().expect("the viewport was built inside the window");
        (window, panel)
    }

    /// Every transport control is drawn, takes a click, and does what it says.
    #[gpui_kit::test]
    fn the_transport_controls_are_drawn_and_take_a_click(cx: &mut TestAppContext) {
        let clocks: Vec<f32> = (0..120).map(|frame| frame as f32 * 0.5).collect();
        let (window, panel) = viewport(cx, clocks);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            for control in [
                "replay-renderer-play",
                "replay-renderer-jump-to-start",
                "replay-renderer-jump-to-end",
                "replay-renderer-back-10s",
                "replay-renderer-forward-10s",
                "replay-renderer-previous-event",
                "replay-renderer-next-event",
                "replay-renderer-settings-toggle",
                "replay-renderer-timeline-toggle",
                "replay-renderer-export",
                "replay-renderer-pop-out",
                "replay-renderer-zoom-reset",
            ] {
                assert!(window.try_find(control).is_some(), "{control} is drawn");
            }

            window.click("replay-renderer-jump-to-end", cx);
            window.render_frame(cx);
            assert_eq!(panel.read(cx).at, 119, "the end of the track");

            window.click("replay-renderer-back-10s", cx);
            window.render_frame(cx);
            assert_eq!(panel.read(cx).at, 99, "ten seconds is twenty half-second frames");

            window.click("replay-renderer-jump-to-start", cx);
            window.render_frame(cx);
            assert_eq!(panel.read(cx).at, 0);

            window.click("replay-renderer-play", cx);
            window.render_frame(cx);
            assert!(panel.read(cx).playing, "the play control starts playback");
        })
        .expect("the window is open");
    }

    /// The transport is two rows, with the scrubber above the controls, as
    /// a video player lays them out: a bar sharing a row with a dozen
    /// controls is both hard to aim at and hard to read a position off.
    #[gpui_kit::test]
    fn the_scrubber_has_a_row_above_the_controls(cx: &mut TestAppContext) {
        let clocks: Vec<f32> = (0..120).map(|frame| frame as f32 * 0.5).collect();
        let (window, _panel) = viewport(cx, clocks);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let scrubber = window.find("replay-renderer-scrubber").bounds();
            let play = window.find("replay-renderer-play").bounds();

            assert!(
                scrubber.bottom() <= play.origin.y,
                "the scrubber row ends before the controls begin: {scrubber:?} then {play:?}"
            );
            // And it is a row of its own, spanning far more than a control.
            assert!(scrubber.size.width > play.size.width * 4., "{scrubber:?}");
        })
        .expect("the window is open");
    }

    /// The event controls refuse a click until the battle has been read,
    /// rather than doing nothing and looking broken.
    #[gpui_kit::test]
    fn the_event_controls_refuse_until_the_battle_has_been_read(cx: &mut TestAppContext) {
        let clocks: Vec<f32> = (0..120).map(|frame| frame as f32 * 0.5).collect();
        let (window, panel) = viewport(cx, clocks);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            // A refused button is drawn but takes no focus, which is how the
            // harness sees one: gpui-kit's Button publishes no disabled flag,
            // so a refused control reads as having no focus state at all
            // while one that would take a click reads as unfocused.
            assert!(window.find("replay-renderer-next-event").focused().is_none(), "refused while nothing is read");
            assert!(window.find("replay-renderer-previous-event").focused().is_none());

            window.click("replay-renderer-next-event", cx);
            window.render_frame(cx);
            assert_eq!(panel.read(cx).at, 0, "and clicking it moves nothing");

            panel.update(cx, |panel, cx| {
                panel.seed_events_for_test(&[10.0, 30.0]);
                panel.events_read = true;
                cx.notify();
            });
            window.render_frame(cx);
            assert!(
                window.find("replay-renderer-next-event").focused().is_some(),
                "and takes a click once the battle is read"
            );

            window.click("replay-renderer-next-event", cx);
            window.render_frame(cx);
            assert!(panel.read(cx).at > 0, "which steps to the first thing that happened");
        })
        .expect("the window is open");
    }

    /// The gear opens, and the switches inside it reach what the viewport
    /// draws.
    #[gpui_kit::test]
    fn the_gear_opens_and_its_switches_reach_the_frame(cx: &mut TestAppContext) {
        let (window, panel) = viewport(cx, vec![0.0, 30.0]);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("renderer-opt-team-rosters").is_none(), "nothing is open yet");

            window.click("replay-renderer-settings-toggle", cx);
            window.render_frame(cx);

            for switch in ["renderer-opt-hp-bars", "renderer-opt-trails", "renderer-opt-team-rosters"] {
                assert!(window.try_find(switch).is_some(), "{switch} is in the open gear");
            }

            let was = panel.read(cx).options.show_hp_bars;
            window.click("renderer-opt-hp-bars", cx);
            window.render_frame(cx);
            assert_eq!(panel.read(cx).options.show_hp_bars, !was, "the switch reaches what is drawn");
        })
        .expect("the window is open");
    }

    /// The timeline opens and lists what happened.
    #[gpui_kit::test]
    fn the_timeline_opens_and_lists_the_battle(cx: &mut TestAppContext) {
        let clocks: Vec<f32> = (0..120).map(|frame| frame as f32 * 0.5).collect();
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1400.), px(700.)), |window, cx| {
            let view = cx.new(|cx| {
                let mut panel = ReplayRendererPanel::ready_for_test(clocks, window, cx);
                panel.seed_events_for_test(&[10.0, 30.0, 50.0]);
                panel.events_read = true;
                panel
            });
            let view: gpui_kit::AnyView = view.into();
            Root::new(view, window, cx)
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);

            window.click("replay-renderer-timeline-toggle", cx);
            window.render_frame(cx);
            assert!(window.try_find("replay-renderer-timeline-list").is_some(), "the list is open");
            assert!(window.try_find(("replay-renderer-timeline-row", 0usize)).is_some(), "with a row per event");
            assert!(window.try_find(("replay-renderer-timeline-row", 2usize)).is_some());
        })
        .expect("the window is open");
    }
}
