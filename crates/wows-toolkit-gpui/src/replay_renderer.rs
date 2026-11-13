//! Playing a replay's battle back on the minimap, at whatever speed the
//! reader asks for.
//!
//! The egui app draws its renderer by turning each frame's draw commands into
//! egui shapes (`replay/renderer/`). This port has no painter of its own, so
//! it rasterises the same commands through
//! [`wows_minimap_renderer::preview::PreviewRenderer`] -- the path the hover
//! preview already uses -- and shows the resulting image.
//!
//! The track, the transport, the view, the per-ship choices, drawing over the
//! map and the export are [`wows_toolkit_replay_data::replay_render`]; this is
//! the GPUI panel over them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::frame_texture::FrameTexture;
use crate::frame_texture::frame_view;
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
use std::rc::Rc;
use wows_toolkit_armor::catalog::ShipEntry;
use wows_toolkit_replay_data::frame::BgraFrame;
use wows_toolkit_replay_data::minimap_preview::SharedPreviewRenderer;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::viewport::MAX_ZOOM;
use wows_minimap_renderer::viewport::MIN_ZOOM;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replay_insights::timeline::EventTone;
use wows_replay_insights::timeline::KIND_COUNT;
use wows_replay_insights::timeline::TimelineEvent;
use wows_replay_insights::timeline::TimelineFilter;
use wows_replay_insights::timeline::format_timeline_event;
use wows_replay_insights::timeline::kind_index;
use wows_replay_insights::timeline::kind_label_key;
use wows_replay_insights::timeline::row_text;
use wows_replay_insights::timeline::row_tone;
use wows_replay_insights::timeline::searchable_text;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::replay_render::annotate;
use wows_toolkit_replay_data::replay_render::annotate::ANNOTATION_ACTIONS;
use wows_toolkit_replay_data::replay_render::annotate::Annotator;
use wows_toolkit_replay_data::replay_render::annotate::Click;
use wows_toolkit_replay_data::replay_render::annotate::ENEMY_TINT;
use wows_toolkit_replay_data::replay_render::annotate::FRIENDLY_TINT;
use wows_toolkit_replay_data::replay_render::annotate::HANDLE_RADIUS;
use wows_toolkit_replay_data::replay_render::annotate::INKS;
use wows_toolkit_replay_data::replay_render::annotate::MAX_NIB;
use wows_toolkit_replay_data::replay_render::annotate::MIN_NIB;
use wows_toolkit_replay_data::replay_render::annotate::PointerDown;
use wows_toolkit_replay_data::replay_render::annotate::PointerMove;
use wows_toolkit_replay_data::replay_render::annotate::PointerUp;
use wows_toolkit_replay_data::replay_render::annotate::SPECIES;
use wows_toolkit_replay_data::replay_render::annotate::TOOLS;
use wows_toolkit_replay_data::replay_render::annotate::same_tool;
use wows_toolkit_replay_data::replay_render::annotate::tool_for_key;
use wows_toolkit_replay_data::replay_render::armor::ArmorFollow;
use wows_toolkit_replay_data::replay_render::armor::ArmorFollowed;
use wows_toolkit_replay_data::replay_render::armor::ShowArmor;
use wows_toolkit_replay_data::replay_render::armor::has_armor_to_show;
use wows_toolkit_replay_data::replay_render::collab::COLLAB_TICK;
use wows_toolkit_replay_data::replay_render::collab::CursorReport;
use wows_toolkit_replay_data::replay_render::collab::PING_SECONDS;
use wows_toolkit_replay_data::replay_render::collab::frame_broadcast;
use wows_toolkit_replay_data::replay_render::collab::ping_ring;
use wows_toolkit_replay_data::replay_render::collab::shared_replay;
use wows_toolkit_replay_data::replay_render::export;
use wows_toolkit_replay_data::replay_render::export::ExportProgress;
use wows_toolkit_replay_data::replay_render::export::ExportSettings;
use wows_toolkit_replay_data::replay_render::export::ExportStage;
use wows_toolkit_replay_data::replay_render::export::ExportTarget;
use wows_toolkit_replay_data::replay_render::export::encoder_status;
use wows_toolkit_replay_data::replay_render::export::must_use_cpu;
use wows_toolkit_replay_data::replay_render::hover;
use wows_toolkit_replay_data::replay_render::hover::ConsumableHover;
use wows_toolkit_replay_data::replay_render::options::TOGGLES;
use wows_toolkit_replay_data::replay_render::options::adopt_saved_defaults;
use wows_toolkit_replay_data::replay_render::options::defaults_to_save;
use wows_toolkit_replay_data::replay_render::options::playback_options_hidden;
use wows_toolkit_replay_data::replay_render::playback::DEFAULT_SPEED;
use wows_toolkit_replay_data::replay_render::playback::Playback;
use wows_toolkit_replay_data::replay_render::playback::SEEK_STEP;
use wows_toolkit_replay_data::replay_render::playback::SPEEDS;
use wows_toolkit_replay_data::replay_render::playback::Tick;
use wows_toolkit_replay_data::replay_render::playback::mmss;
use wows_toolkit_replay_data::replay_render::playback::speed_index;
use wows_toolkit_replay_data::replay_render::playback::speed_label;
use wows_toolkit_replay_data::replay_render::raster;
use wows_toolkit_replay_data::replay_render::raster::LayoutChange;
use wows_toolkit_replay_data::replay_render::ships;
use wows_toolkit_replay_data::replay_render::ships::ALL_RANGES;
use wows_toolkit_replay_data::replay_render::ships::ShipChoices;
use wows_toolkit_replay_data::replay_render::ships::ShipMenu;
use wows_toolkit_replay_data::replay_render::timeline::BattleEvents;
use wows_toolkit_replay_data::replay_render::timeline::TimelineRow;
use wows_toolkit_replay_data::replay_render::track::DEFAULT_MAP_ORIGIN;
use wows_toolkit_replay_data::replay_render::track::FrameIndex;
use wows_toolkit_replay_data::replay_render::track::Track;
use wows_toolkit_replay_data::replay_render::track::bake;
use wows_toolkit_replay_data::replay_render::view::ElementBounds;
use wows_toolkit_replay_data::replay_render::view::FrameSize;
use wows_toolkit_replay_data::replay_render::view::MapView;
use wows_toolkit_replay_data::replay_render::view::Placement;
use wows_toolkit_replay_data::replay_render::view::Wheel;
use wows_toolkit_replay_data::replay_render::view::WindowPoint;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::ReplayId;
use wt_collab_client::drawing::Tool;

use crate::tactics::ShipLookup;
use crate::tactics::ship_match_label;
use crate::tactics::ship_match_name;
use crate::text_filter::FilterKey;
use crate::text_filter::MarkedLabel;
use crate::text_filter::MarkedQuery;
use wows_toolkit_replay_data::load::GameDataCache;

/// What the viewport is doing.
enum State {
    /// Reading the replay and walking the battle.
    Baking,
    /// Shared with the draw and the export running off the UI thread.
    Ready(Arc<Track>),
    Failed(String),
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
    frame: FrameTexture,
    /// Which frame is on screen, whether playback is running, and how fast.
    playback: Playback,
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
    /// Which part of the map the viewport shows, and a drag of it under way.
    /// Where the frame was painted is read from `drawn` as it is needed.
    map: MapView,
    /// Where the map's corner sits on the canvas the frame is drawn on. Moves
    /// when a gutter layer changes the canvas.
    map_origin: (f32, f32),
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
    /// The map point last reported to the session.
    cursor: CursorReport,
    /// This viewport's end of a collab session. Inert until one is running.
    collab: CollabLink,
    /// What this playback is called in a session, so a peer can ask to watch
    /// this one rather than another window.
    replay_id: ReplayId,
    /// Whether the session has been told this playback is open. Said once, and
    /// again if a session starts after it was opened.
    shared: bool,
    /// The tool in hand, what it has drawn so far, what is picked out, and
    /// what this viewport can undo.
    annotator: Annotator,
    /// The box a ship is looked up in, to give a placed one an identity.
    ship_search: Entity<InputState>,
    _ship_search: Option<Subscription>,
    /// What has been typed into it, and what that matched. Recomputed as it
    /// is typed rather than while the popover is built, which happens over
    /// and over and cannot reach the catalogue mutably.
    ship_query: MarkedQuery,
    matched_ships: Vec<(wowsunpack::game_params::types::Species, ShipEntry)>,
    /// Every ship the loaded build knows, built once when first asked for.
    ship_lookup: Option<ShipLookup>,
    /// The redraw a running session needs, held only while there is one.
    _collab_tick: Option<Task<()>>,
    /// Kept so the viewport can build the canvas a layer asks for: the team
    /// rosters need gutters the canvas it opened with does not have.
    game_data: GameDataCache,
    /// The canvas the frame on screen was drawn on.
    layout: wows_minimap_renderer::drawing::SidePanelLayout,
    /// What an export is encoded with.
    export_settings: ExportSettings,
    /// Whose trails are hidden and whose ranges are drawn.
    choices: ShipChoices,
    /// The ship a context menu is open for, and where it was opened.
    menu_for: Option<ShipMenu>,
    /// What happened in the battle, once the second walk has read it, and
    /// which kinds of it the timeline shows.
    events: BattleEvents,
    /// The filter's search, folded once each time it changes.
    event_query: MarkedQuery,
    /// The timeline's search box.
    event_search: Entity<InputState>,
    _event_search: Option<Subscription>,
    /// The texts the search reads of each of the events, folded once when
    /// the events are read.
    event_keys: Vec<[FilterKey; 2]>,
    /// The ship an armor viewer was opened on, and what it was last told.
    armor_following: Option<ArmorFollow>,
    _events: Option<Task<()>>,
    /// Set when a frame was asked for while one was still being drawn. The
    /// draw in flight starts another as it finishes, so what ends up on
    /// screen is the last thing asked for rather than the first.
    redraw_wanted: bool,
    focus_handle: FocusHandle,
}

impl EventEmitter<PanelEvent> for ReplayRendererPanel {}

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
    ShowArmor(ShowArmor),
    /// Playback moved, and the ship an armor viewer is already open on has
    /// taken different hits by this point.
    ///
    /// Separate from [`Self::ShowArmor`] because it must not pull the reader
    /// back to the armor tab: they asked for the viewer once, not on every
    /// frame.
    ArmorFollowed(ArmorFollowed),
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
        let mut panel = Self::build(path.clone(), title, State::Baking, game_data.clone(), window, cx);
        panel.alts = alts;
        panel.start_bake(path.clone(), game_data.clone(), cx);
        panel.start_event_scan(path, game_data, cx);
        panel
    }

    /// A viewport in `state`, with its controls and nothing else started.
    fn build(
        path: PathBuf,
        title: SharedString,
        state: State,
        game_data: GameDataCache,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let last = match &state {
            State::Ready(track) => track.last().get(),
            State::Baking | State::Failed(_) => 0,
        };
        let map_origin = match &state {
            State::Ready(track) => track.map_origin,
            State::Baking | State::Failed(_) => DEFAULT_MAP_ORIGIN,
        };
        let seek = cx.new(|_| seek_slider(last));
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
                this.ship_query = MarkedQuery::new(&state.read(cx).value());
                this.matched_ships = this.matching_ships();
                cx.notify();
            }
        });
        let event_search_subscription = cx.subscribe(&event_search, |this, state, event, cx| {
            if matches!(event, InputEvent::Change) {
                let typed = state.read(cx).value().to_string();
                this.set_event_filter(|filter| filter.search = typed, cx);
            }
        });

        Self {
            path,
            title,
            state,
            map_origin,
            renderer: None,
            frame: FrameTexture::new(),
            playback: Playback::new(),
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
            map: MapView::new(),
            zoom,
            _zoom_subscription: Some(zoom_subscription),
            drawn: Rc::new(Cell::new(None)),
            regions: Vec::new(),
            hovered_consumable: None,
            cursor: CursorReport::new(),
            collab: CollabLink::default(),
            replay_id: ReplayId::fresh(),
            shared: false,
            annotator: Annotator::new(),
            ship_search,
            _ship_search: Some(ship_search_subscription),
            ship_query: MarkedQuery::default(),
            matched_ships: Vec::new(),
            ship_lookup: None,
            _collab_tick: None,
            game_data,
            layout: wows_minimap_renderer::drawing::SidePanelLayout::None,
            export_settings: ExportSettings::default(),
            choices: ShipChoices::new(),
            menu_for: None,
            events: BattleEvents::new(),
            event_query: MarkedQuery::default(),
            event_search,
            _event_search: Some(event_search_subscription),
            event_keys: Vec::new(),
            armor_following: None,
            _events: None,
            redraw_wanted: false,
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

    /// Which other recordings this viewport was baked with.
    pub(crate) fn baked_alts(&self) -> &[PathBuf] {
        &self.alts
    }

    /// Bakes this battle again through another set of recordings, for a tab
    /// opened before one of them was added.
    pub(crate) fn rebake_with_alts(&mut self, alts: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.alts = alts;
        let path = self.path.clone();
        self.start_bake(path, self.game_data.clone(), cx);
    }

    /// A viewport already holding `clocks`' worth of empty frames, with no
    /// bake behind it and no renderer to rasterise through.
    ///
    /// Test-only: production viewports reach this state through a bake. The
    /// transport is what this exercises -- where playback is, what the clock
    /// reads, where the bar sits -- none of which needs a drawn frame.
    #[cfg(test)]
    pub(crate) fn ready_for_test(clocks: Vec<f32>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let track = Track {
            frames: vec![Vec::new(); clocks.len()],
            clocks: clocks.into_iter().map(GameClock).collect(),
            battle_start: GameClock(0.0),
            battle_end: None,
            // A middling map, so a range in metres lands somewhere
            // sensible on the minimap.
            space_size: 1400.0,
            version: wowsunpack::data::Version::base(99, 0, 0),
            space: String::new(),
            map_origin: DEFAULT_MAP_ORIGIN,
        };
        Self::build(
            PathBuf::new(),
            SharedString::from("test"),
            State::Ready(Arc::new(track)),
            GameDataCache::without_install(),
            window,
            cx,
        )
    }

    /// Replaces the frame on screen's commands.
    ///
    /// Test-only: a baked track carries these from the battle.
    #[cfg(test)]
    pub(crate) fn set_frame_commands_for_test(&mut self, commands: Vec<DrawCommand>) {
        let at = self.playback.at().get();
        if let Some(frame) = self.track_for_test().frames.get_mut(at) {
            *frame = commands;
        }
    }

    /// Says when the battle inside an already-ready track ran.
    ///
    /// Test-only: a baked track carries this from the replay itself.
    #[cfg(test)]
    pub(crate) fn set_battle_window(&mut self, start: f32, end: Option<f32>) {
        let track = self.track_for_test();
        track.battle_start = GameClock(start);
        track.battle_end = end.map(GameClock);
    }

    /// The track a test viewport holds, to change. Nothing else holds it
    /// while there is no renderer to draw or export through.
    #[cfg(test)]
    fn track_for_test(&mut self) -> &mut Track {
        let State::Ready(track) = &mut self.state else { panic!("a test viewport is ready") };
        Arc::get_mut(track).expect("nothing else holds a test track")
    }

    /// Reads what happened in the battle, which the event controls step
    /// between.
    ///
    /// Its own task rather than part of the bake: it is a second walk of the
    /// replay, and the viewport is worth showing before it finishes.
    fn start_event_scan(&mut self, path: PathBuf, game_data: GameDataCache, cx: &mut Context<Self>) {
        self._events = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    wows_toolkit_replay_data::minimap_preview::extract_events(&path, &game_data)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match read {
                    Ok((read, shots)) => this.set_events(BattleEvents::from_extraction(read, shots)),
                    // The viewport still plays; only the event controls are
                    // worse off, and they stay refused.
                    Err(reason) => tracing::warn!("replay renderer: the battle's events could not be read: {reason}"),
                }
                cx.notify();
            });
        }));
    }

    /// Takes the battle's events, with the texts the timeline search reads of
    /// each folded once here rather than on every keystroke.
    fn set_events(&mut self, events: BattleEvents) {
        self.event_keys = events
            .events()
            .iter()
            .map(|event| {
                let (first, second) = searchable_text(&event.kind);
                [FilterKey::new(first), FilterKey::new(second)]
            })
            .collect();
        let filter = self.events.filter().clone();
        self.events = events;
        self.events.set_filter(|kept| *kept = filter);
    }

    /// A viewport with `events` already read, at the elapsed clocks given.
    #[cfg(test)]
    fn seed_events_for_test(&mut self, at: &[f32]) {
        use wows_replay_insights::timeline::TimelineEventKind;
        use wows_replays::types::ElapsedClock;
        let events = at
            .iter()
            .map(|seconds| TimelineEvent {
                clock: ElapsedClock(*seconds),
                kind: TimelineEventKind::AdvantageChanged { label: format!("at {seconds}"), is_friendly: true },
            })
            .collect();
        self.set_events(BattleEvents::from_read(events, None, std::collections::HashMap::new()));
    }

    /// Hands a viewport being built its end of the session.
    pub fn seed_collab(&mut self, link: CollabLink, cx: &mut Context<Self>) {
        self.collab = link;
        self.follow_collab(cx);
    }

    /// Hands this viewport a session to talk to, or takes one away.
    pub fn set_collab(&mut self, link: CollabLink, cx: &mut Context<Self>) {
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
        let lines = &hover.lines;
        let body = crate::ui::overlay_surface(v_flex().gap_0p5().w(px(280.)).p_2(), cx)
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
        if track.space.is_empty() {
            return;
        }
        let track = Arc::clone(track);
        self.shared = true;
        let replay_id = self.replay_id;
        let replay_name = self.title.to_string();
        let game_data = self.game_data.clone();
        let link = self.collab.clone();
        cx.background_spawn(async move {
            if let Some(shared) = shared_replay(replay_id, replay_name, &track, &game_data) {
                link.announce_replay(shared);
            }
        })
        .detach();
    }

    /// Puts this frame in front of the session, where this app is the one the
    /// rest of it watches.
    fn broadcast_current(&self, commands: &[DrawCommand], track: &Track) {
        if !self.shared {
            return;
        }
        self.collab.broadcast_frame(frame_broadcast(self.replay_id, track, self.playback.at(), commands));
    }

    /// The handle a picked shape is turned by, drawn over the frame.
    ///
    /// An element rather than part of the frame: it is a control rather than
    /// something drawn on the map, so it keeps its size at every zoom and
    /// stays out of an exported video.
    fn rotation_handle_overlay(&self) -> Option<AnyElement> {
        let handle = self.annotator.rotation_handle(&self.collab, &self.map_view(), self.map_origin)?;
        let radius = px(HANDLE_RADIUS);
        Some(
            div()
                .absolute()
                .left(px(handle.handle.x) - radius)
                .top(px(handle.handle.y) - radius)
                .child(div().size(radius * 2.0).rounded_full().bg(gpui_kit::rgb(0xFFFF64)))
                // The stem back to the shape, so the handle reads as
                // belonging to it rather than floating over the map.
                .child(
                    div()
                        .absolute()
                        .left(radius)
                        .top(radius)
                        .w(px(1.))
                        .h(px(handle.anchor.y - handle.handle.y))
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

        let body = match progress.fraction() {
            // Nothing to measure against yet: the encoder is still starting.
            None => body.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new())
                    .child(div().text_xs().child(t!("ui.renderer.preparing_export").into_owned())),
            ),
            Some(done) => body
                .child(div().text_xs().child(format!(
                    "{} ({}/{})",
                    t!(progress.stage.label_key()),
                    progress.done,
                    progress.total
                )))
                .child(Progress::new("replay-renderer-export-progress").value(done * 100.0)),
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
        let map = self.map_view();
        let mut over: Vec<AnyElement> = Vec::new();
        for cursor in self.collab.peer_cursors() {
            let Some(pos) = cursor.pos else { continue };
            let Some(at) = map.element_point((pos[0], pos[1]), self.map_origin) else { continue };
            let color: gpui_kit::Hsla =
                rgb(u32::from_be_bytes([0, cursor.color[0], cursor.color[1], cursor.color[2]])).into();
            over.push(
                div()
                    .absolute()
                    .left(px(at.x))
                    .top(px(at.y))
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
            let Some(at) = map.element_point((ping.pos[0], ping.pos[1]), self.map_origin) else { continue };
            let Some(ring) = ping_ring(ping.time.elapsed()) else { continue };
            let color: gpui_kit::Hsla =
                rgb(u32::from_be_bytes([0, ping.color[0], ping.color[1], ping.color[2]])).into();
            over.push(
                div()
                    .absolute()
                    .left(px(at.x - ring.radius))
                    .top(px(at.y - ring.radius))
                    .size(px(ring.radius * 2.0))
                    .rounded_full()
                    .border_2()
                    .border_color(color.opacity(ring.opacity))
                    .into_any_element(),
            );
        }
        over
    }

    /// Where the frame was last painted: the element it fills, and its own
    /// size. `None` before it has been painted once.
    fn placement(&self) -> Option<Placement> {
        let bounds = self.drawn.get()?;
        let size = self.frame.size()?;
        Some(Placement {
            bounds: ElementBounds {
                origin: window_point(bounds.origin),
                width: bounds.size.width.as_f32(),
                height: bounds.size.height.as_f32(),
            },
            frame: FrameSize { width: size.width.0 as u32, height: size.height.0 as u32 },
        })
    }

    /// The map view, placed where the frame was last painted.
    ///
    /// Read from the painter's record each time rather than kept: the frame
    /// moves whenever the panel is laid out again, which nothing here hears.
    fn map_view(&self) -> MapView {
        let mut map = self.map.clone();
        map.set_placement(self.placement());
        map
    }

    /// Where a map point lands inside the viewport element. Test-only: the
    /// overlays read the map view once for all they place.
    #[cfg(test)]
    fn element_point(&self, at: (f32, f32)) -> Option<(Pixels, Pixels)> {
        let at = self.map_view().element_point(at, self.map_origin)?;
        Some((px(at.x), px(at.y)))
    }

    /// Where in the map a window position falls, in the map's own pixels.
    fn map_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        self.map_view().map_point(window_point(position), self.map_origin)
    }

    /// The commands of the frame on screen.
    fn current_commands(&self) -> &[DrawCommand] {
        self.track().and_then(|track| track.commands(self.playback.at())).unwrap_or_default()
    }

    /// What the advantage reads, for the frame on screen.
    pub(crate) fn advantage_hover(&self) -> Option<Vec<String>> {
        hover::advantage_hover(self.current_commands())
    }

    /// Tells the session where this app's pointer is on the map.
    fn report_cursor(&mut self, position: Point<Pixels>) {
        let at = self.map_point(position).map(|(x, y)| [x, y]);
        self.cursor.report(&self.collab, at);
    }

    /// Drops a ping where the pointer is, for everyone in the session.
    fn on_ping(&mut self, event: &MouseDownEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        let Some((x, y)) = self.map_point(event.position) else { return };
        self.collab.send_ping([x, y]);
    }

    /// Opens the per-ship menu on whatever was right-clicked.
    fn on_right_click(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.map_point(event.position) else { return };
        self.menu_for =
            ships::ship_at(self.current_commands(), at).map(|ship| ShipMenu::open(ship, window_point(event.position)));
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

    /// Asks for an armor viewer on the ship the menu is open for, and
    /// follows it from then on.
    pub(crate) fn show_armor(&mut self, entity_id: EntityId, cx: &mut Context<Self>) {
        let clock = GameClock(self.clock_now());
        let Some((following, shown)) =
            ArmorFollow::start(entity_id, self.current_commands(), self.events.shots(), clock, &self.game_data)
        else {
            return;
        };
        cx.emit(RendererEvent::ShowArmor(shown));
        self.armor_following = Some(following);
        self.close_ship_menu(cx);
    }

    /// Tells a viewer that is already open what its ship has taken by where
    /// playback has reached.
    fn follow_armor(&mut self, cx: &mut Context<Self>) {
        let clock = GameClock(self.clock_now());
        let Some(following) = self.armor_following.as_mut() else { return };
        if let Some(followed) = following.follow(clock) {
            cx.emit(RendererEvent::ArmorFollowed(followed));
        }
    }

    /// The game clock of the frame on screen.
    fn clock_now(&self) -> f32 {
        self.track().map(|track| self.playback.seconds(track)).unwrap_or_default()
    }

    /// Whether the menu's ship has anything an armor viewer could show.
    pub(crate) fn has_armor_to_show(&self, entity_id: EntityId) -> bool {
        has_armor_to_show(self.events.shots(), self.current_commands(), entity_id, &self.game_data)
    }

    /// Whether `player` has a trail drawn.
    pub(crate) fn trail_shown(&self, player: &str) -> bool {
        self.choices.trail_shown(player)
    }

    /// Shows or hides one ship's trail.
    pub(crate) fn set_trail_shown(&mut self, player: &str, shown: bool, cx: &mut Context<Self>) {
        let drawn = ships::drawn_ships(self.current_commands());
        self.choices.set_trail_shown(player, shown, &drawn, &mut self.options);
        self.draw_current(cx);
        cx.notify();
    }

    /// Hides every trail but `player`'s.
    pub(crate) fn only_trail(&mut self, player: &str, cx: &mut Context<Self>) {
        let drawn = ships::drawn_ships(self.current_commands());
        self.choices.only_trail(player, &drawn, &mut self.options);
        self.draw_current(cx);
        cx.notify();
    }

    /// Which ranges `player` has on.
    pub(crate) fn ranges_for(&self, player: &str) -> ShipConfigFilter {
        self.choices.ranges_for(player)
    }

    /// Changes which ranges one ship shows.
    pub(crate) fn set_ranges_for(
        &mut self,
        player: &str,
        apply: impl FnOnce(&mut ShipConfigFilter),
        cx: &mut Context<Self>,
    ) {
        self.choices.set_ranges_for(player, apply, &mut self.options);
        self.draw_current(cx);
        cx.notify();
    }

    /// Leaves only `player`'s ranges on.
    pub(crate) fn only_ranges(&mut self, player: &str, cx: &mut Context<Self>) {
        self.choices.only_ranges(player, &mut self.options);
        self.draw_current(cx);
        cx.notify();
    }

    /// Turns all of one ship's ranges on, or all of them off.
    pub(crate) fn set_every_range_for(&mut self, player: &str, on: bool, cx: &mut Context<Self>) {
        self.choices.set_every_range_for(player, on, &mut self.options);
        self.draw_current(cx);
        cx.notify();
    }

    /// Turns every ship's ranges on.
    pub(crate) fn all_ranges(&mut self, cx: &mut Context<Self>) {
        let drawn = ships::drawn_ships(self.current_commands());
        self.choices.all_ranges(&drawn, &mut self.options);
        self.draw_current(cx);
        cx.notify();
    }

    /// The events the filter admits, newest last. A timeline is read in the
    /// order things happened, so the search only drops events and never
    /// reorders them.
    pub(crate) fn visible_events(&self) -> Vec<&TimelineEvent> {
        let searching = self.event_query.is_active();

        self.events
            .events()
            .iter()
            .zip(&self.event_keys)
            .filter(|(event, keys)| {
                self.events.filter().kinds[kind_index(&event.kind)]
                    && (!searching || self.event_query.best(keys.iter()).is_some())
            })
            .map(|(event, _)| event)
            .collect()
    }

    /// Which of the battle's events the timeline shows.
    pub(crate) fn event_filter(&self) -> &TimelineFilter {
        self.events.filter()
    }

    /// Changes which events the timeline shows.
    pub(crate) fn set_event_filter(&mut self, apply: impl FnOnce(&mut TimelineFilter), cx: &mut Context<Self>) {
        self.events.set_filter(apply);
        self.event_query = MarkedQuery::new(&self.events.filter().search);
        cx.notify();
    }

    /// Whether the battle has been read yet, which is what the timeline says
    /// instead of an empty list while the second walk runs.
    pub(crate) fn events_are_read(&self) -> bool {
        self.events.is_read()
    }

    /// The event before where playback is, if there is one.
    fn previous_event(&self) -> Option<&TimelineEvent> {
        self.events.previous(self.elapsed_now()?)
    }

    fn next_event(&self) -> Option<&TimelineEvent> {
        self.events.next(self.elapsed_now()?)
    }

    /// How far into the battle playback is, which is what an event's clock is
    /// measured against.
    fn elapsed_now(&self) -> Option<f32> {
        self.track().map(|track| self.playback.elapsed(track))
    }

    /// Moves playback to `at` seconds into the battle and says what is there.
    fn go_to_event(&mut self, at: f32, said: String, window: &mut Window, cx: &mut Context<Self>) {
        self.go_to_event_at(at, window, cx);
        crate::toast::info(said, window, cx);
    }

    /// Moves playback to `at` seconds into the battle.
    pub(crate) fn go_to_event_at(&mut self, at: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.change_playback(|playback, track| playback.go_to_elapsed(track, at), cx);
        self.sync_seek(window, cx);
    }

    /// Moves playback to the frame `clock` falls in.
    pub(crate) fn go_to_clock(&mut self, clock: GameClock, window: &mut Window, cx: &mut Context<Self>) {
        self.change_playback(|playback, track| playback.go_to_clock(track, clock), cx);
        self.sync_seek(window, cx);
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
                        this.map_origin = track.map_origin;
                        this.state = State::Ready(Arc::new(track));
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

    /// Opens this viewport showing what the reader last saved as the defaults.
    ///
    /// Runs once the bake has landed, because the rosters are a wider canvas
    /// rather than a layer and there is nothing to widen before then.
    fn adopt_saved_defaults(&mut self, cx: &mut Context<Self>) {
        let saved = crate::render_defaults::defaults(cx);
        let export = export::export_settings_of(&saved);
        self.set_export_settings(|settings| *settings = export, cx);
        self.set_options(|options, show_dead| adopt_saved_defaults(&saved, options, show_dead), cx);
    }

    /// Writes what this viewport is showing back as what a viewport opens with.
    fn remember_defaults(&self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = crate::render_defaults::defaults(cx);
        let saved = defaults_to_save(&stored, &self.options, self.show_dead_ships, &self.export_settings);

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
        self.follow_layout();
        self.draw_current(cx);
        cx.notify();
    }

    /// Moves the viewport onto the canvas its layers now need.
    fn follow_layout(&mut self) {
        match raster::follow_layout(&self.path, &self.game_data, &mut self.options, self.layout) {
            LayoutChange::Unchanged | LayoutChange::Refused => {}
            LayoutChange::Moved { layout, renderer, map_origin } => {
                self.layout = layout;
                self.renderer = Some(renderer);
                self.map_origin = map_origin;
                // The frame on screen was drawn on the old canvas, so it is
                // the wrong shape until the next one lands.
                self.redraw_wanted = true;
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

    /// The baked battle, once the bake has landed.
    fn track(&self) -> Option<&Arc<Track>> {
        match &self.state {
            State::Ready(track) => Some(track),
            State::Baking | State::Failed(_) => None,
        }
    }

    /// Changes where playback is, then follows it.
    fn change_playback(&mut self, change: impl FnOnce(&mut Playback, &Track) -> bool, cx: &mut Context<Self>) {
        let Some(track) = self.track().cloned() else { return };
        let moved = change(&mut self.playback, &track);
        self.follow_playback(moved, cx);
    }

    /// Follows a move of playback with the armor viewer and the frame on
    /// screen.
    ///
    /// A viewport with no frame on screen yet draws one even when playback did
    /// not move.
    fn follow_playback(&mut self, moved: bool, cx: &mut Context<Self>) {
        if !moved && self.frame.size().is_some() {
            return;
        }
        self.follow_armor(cx);
        self.draw_current(cx);
        cx.notify();
    }

    /// Moves playback to `at`, without moving the bar.
    fn set_at(&mut self, at: FrameIndex, cx: &mut Context<Self>) {
        self.change_playback(|playback, track| playback.go_to(track, at), cx);
    }

    /// Moves playback to `at` and takes the bar with it.
    ///
    /// The transport's own controls move both; only the bar itself moves one
    /// without the other, since it is already where it put itself.
    fn go_to(&mut self, at: FrameIndex, window: &mut Window, cx: &mut Context<Self>) {
        self.set_at(at, cx);
        self.sync_seek(window, cx);
    }

    /// Moves playback `delta` seconds of game time from where it is.
    fn seek_by(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.change_playback(|playback, track| playback.seek_by(track, delta), cx);
        self.sync_seek(window, cx);
    }

    /// Moves one rung along the speed ladder, in the direction of `step`.
    fn step_speed(&mut self, step: i32, window: &mut Window, cx: &mut Context<Self>) {
        if self.playback.step_speed(step) {
            self.speed_changed(window, cx);
        }
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
        let ctrl = event.keystroke.modifiers.secondary(cx);

        if ctrl && !shift && event.keystroke.key == "z" {
            self.undo(cx);
            cx.stop_propagation();
            return;
        }

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
        let Some(track) = self.track().cloned() else {
            self.renderer = Some(renderer);
            return;
        };
        let at = self.playback.at();
        let Some(commands) = track.commands(at) else {
            self.renderer = Some(renderer);
            return;
        };
        self.broadcast_current(commands, &track);

        let drawn_on = self.annotator.overlay_commands(&self.collab, &track, &self.game_data);
        let Some(shown) =
            raster::raster_commands(&track, at, &self.options, self.show_dead_ships, &self.choices, drawn_on)
        else {
            self.renderer = Some(renderer);
            return;
        };
        let view = self.map_view().shaped_view();
        cx.spawn(async move |this, cx| {
            let drawn = cx.background_spawn(async move {
                let (frame, regions) = {
                    let mut drawing = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let (frame, regions) = drawing.render_regions(view, &shown);
                    (BgraFrame::from_rgb(&frame), regions)
                };
                (renderer, frame, regions)
            });
            let (renderer, frame, regions) = drawn.await;
            let _ = this.update(cx, |this, cx| {
                this.renderer = Some(renderer);
                // The regions are only read against the pointer, so a frame
                // that reached the window needs no re-render of the view.
                if !this.frame.present(frame, cx) {
                    cx.notify();
                }
                this.regions = regions;
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
        let Some(track) = self.track().cloned() else { return };
        if track.is_empty() {
            return;
        }
        // Held for the whole encode, which is why the transport is refused
        // while one runs: a frame cannot be drawn for two things at once.
        let Some(renderer) = self.renderer.take() else { return };

        let settings = self.export_settings;
        let frames = export::export_frames(&track, &settings, &self.options, self.show_dead_ships, &self.choices);
        // What the video covers, which is not the whole track when the
        // pre-battle phase is left out.
        let duration = export::export_duration(frames.len());
        let suggested = export::suggested_name(&self.title);
        // The clipboard needs a file, not a place to put one, so it is not
        // asked for.
        let asked: Option<_> = match target {
            ExportTarget::File => Some(wows_toolkit_services::dialog::save_file(
                Some(&t!("ui.replay.renderer.export_video")),
                &suggested,
                Some(MP4),
            )),
            ExportTarget::Clipboard => None,
        };

        self.export_failure = None;
        self.export = Some(ExportProgress { done: 0, total: frames.len() as u64, stage: ExportStage::Encoding });
        self.playback.stop();
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
                Some(dialog) => crate::dialog::chosen(dialog, &this, cx).await,
                // A temporary file that outlives this process, since the
                // clipboard will point at it.
                None => export::temporary_output(&suggested),
            };
            let Some(output) = chosen else {
                // Dismissed at the dialog, the dialog failed and said so, or
                // no temporary file could be made: the renderer goes back to
                // the viewport and nothing else changes.
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
                    // Never set: an export runs to the end even when the
                    // viewport that asked for it closes, since the file was
                    // asked for.
                    let cancel = AtomicBool::new(false);
                    let outcome =
                        export::encode_track(&renderer, &frames, duration, &written, settings, &cancel, |step| {
                            let _ = progress_tx.unbounded_send(step);
                        });
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
                    Err(reason) => this.export_failure = Some(reason.to_string()),
                }
                cx.notify();
            });
        }));
    }

    fn toggle_playing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(track) = self.track().cloned() else { return };
        // Playing from the end starts again at the beginning, which is what a
        // transport with no stop button has to mean.
        if self.playback.toggle_playing(&track) {
            self.follow_playback(true, cx);
        }
        if self.playback.is_playing() {
            self.start_ticker(window, cx);
        } else {
            self._tick = None;
        }
        cx.notify();
    }

    fn start_ticker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let interval = self.playback.tick_interval();
        self._tick = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let still_playing = this
                    .update_in(cx, |this, window, cx| {
                        let Some(track) = this.track().cloned() else { return false };
                        match this.playback.advance(&track) {
                            Tick::Advanced => {
                                this.follow_playback(true, cx);
                                // The bar follows playback, so a reader can
                                // see where in the battle they are.
                                this.sync_seek(window, cx);
                                true
                            }
                            Tick::Finished => {
                                cx.notify();
                                false
                            }
                            Tick::Idle => false,
                        }
                    })
                    .unwrap_or(false);
                if !still_playing {
                    return;
                }
            }
        }));
    }

    fn set_speed(&mut self, speed: f32, window: &mut Window, cx: &mut Context<Self>) {
        if self.playback.set_speed(speed) {
            self.speed_changed(window, cx);
        }
    }

    /// Follows a change of speed with the dropdown and the ticker.
    fn speed_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The keys walk the ladder too, so the dropdown is told rather than
        // left showing the speed before last.
        let index = speed_index(self.playback.speed());
        self.speed_select.update(cx, |state, cx| state.set_selected_value(&index, window, cx));
        // The ticker's interval is fixed when it starts, so a speed change
        // replaces it rather than waiting for the next tick.
        if self.playback.is_playing() {
            self.start_ticker(window, cx);
        }
        cx.notify();
    }

    /// Moves the seek slider to where playback has reached.
    ///
    /// The slider reports the move back as a change, which would ask for the
    /// frame it is already on; `set_at` answers that with nothing to do.
    fn sync_seek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.playback.at().get() as f32;
        self.seek.update(cx, |slider, cx| slider.set_value(at, window, cx));
    }

    /// Rebuilds the seek slider over the frames the bake produced.
    ///
    /// A slider quantises to its step and its range is fixed when it is
    /// built, so one built before the track was known could only sit at
    /// either end of it. This one steps a frame at a time over the track's
    /// real length, which is what the control is choosing.
    fn rebuild_seek(&mut self, cx: &mut Context<Self>) {
        let last = self.track().map(|track| track.last().get()).unwrap_or_default();
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
        let value = value.start();
        self.change_playback(|playback, track| playback.seek_to_slider(track, value), cx);
    }

    /// Shows a different part of the map, and redraws the frame on screen.
    fn set_view(&mut self, view: MapViewport, window: &mut Window, cx: &mut Context<Self>) {
        if self.map.set_view(view) {
            self.view_changed(window, cx);
        }
    }

    /// Follows a change of view with the zoom control and the frame on screen.
    ///
    /// Redrawn rather than left for the next tick: a zoom whose effect only
    /// arrived with playback would read as not having worked while paused.
    fn view_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let zoom = self.map.view().zoom();
        self.zoom.update(cx, |slider, cx| slider.set_value(zoom, window, cx));
        self.draw_current(cx);
        cx.notify();
    }

    /// The map view, placed where the frame was last painted, to change.
    fn placed_map(&mut self) -> &mut MapView {
        let placement = self.placement();
        self.map.set_placement(placement);
        &mut self.map
    }

    /// What the roster icon under the pointer reads as, if it is on one.
    fn consumable_at(&self, position: Point<Pixels>) -> Option<ConsumableHover> {
        let at = self.map_view().canvas_point(window_point(position))?;
        hover::consumable_at(&self.regions, self.current_commands(), at)
    }

    /// Reads the roster icon under the pointer. Returns whether it changed.
    fn update_consumable_hover(&mut self, position: Point<Pixels>) -> bool {
        let found = self.consumable_at(position);
        let changed = hover::hover_changed(self.hovered_consumable.as_ref().map(|(hover, _)| hover), found.as_ref());
        self.hovered_consumable = found.map(|hover| (hover, position));
        changed
    }

    /// The wheel zooms about whatever is under the pointer.
    fn on_scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(SCROLL_LINE).y.as_f32();
        let map_origin = self.map_origin;
        match self.placed_map().zoom_by_wheel(window_point(event.position), delta, map_origin) {
            Wheel::Ignored => return,
            Wheel::Unchanged => {}
            Wheel::Zoomed => self.view_changed(window, cx),
        }
        cx.stop_propagation();
    }

    /// A press on the map goes to the tool in hand, a rotation handle, or
    /// what is picked out, and otherwise starts a drag of the map.
    fn on_drag_start(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let position = window_point(event.position);
        match self.annotator.pointer_down(&self.collab, &self.map_view(), self.map_origin, position) {
            PointerDown::Stroked => {
                self.draw_current(cx);
                cx.notify();
            }
            PointerDown::Unclaimed => {
                self.placed_map().start_pan(position);
            }
            PointerDown::OffMap | PointerDown::Turning | PointerDown::Moving => {}
        }
    }

    /// Every ship the loaded build knows, built the first time one is
    /// looked up rather than when the viewport opens.
    fn ships(&mut self) -> Option<&ShipLookup> {
        if self.ship_lookup.is_none() {
            let loaded = self.game_data.newest_loaded()?;
            let catalog = wows_toolkit_armor::catalog::ShipCatalog::build(loaded.provider());
            self.ship_lookup = Some(ShipLookup::new(&catalog));
        }

        self.ship_lookup.as_ref()
    }

    /// The ships whose names match what has been typed, best first, with the
    /// class each belongs to. Empty until something is typed, since every
    /// ship at once is not a choice.
    fn matching_ships(&mut self) -> Vec<(wowsunpack::game_params::types::Species, ShipEntry)> {
        if !self.ship_query.is_active() {
            return Vec::new();
        }

        let query = self.ship_query.clone();
        let Some(ships) = self.ships() else { return Vec::new() };

        ships.find(&query)
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
        let named = self.annotator.name_picked_ship(
            &self.collab,
            &self.game_data,
            species,
            &ship.param_index,
            ship.display_name.clone(),
        );
        if !named {
            return;
        }
        self.ship_query = MarkedQuery::default();
        self.ship_search.update(cx, |state, cx| state.set_value("", window, cx));
        self.draw_current(cx);
        cx.notify();
    }

    /// Puts the session back the way it was before the last change this
    /// viewport made.
    fn undo(&mut self, cx: &mut Context<Self>) {
        if self.annotator.undo(&self.collab) {
            self.draw_current(cx);
            cx.notify();
        }
    }

    /// Widens or narrows the nib by `step`, within the range the egui board
    /// offers (`replay/renderer/mod.rs`'s own bracket keys).
    fn step_nib(&mut self, step: f32, cx: &mut Context<Self>) {
        if self.annotator.step_nib(step) {
            cx.notify();
        }
    }

    /// Erases whatever is picked out, if anything is.
    fn erase_picked(&mut self, cx: &mut Context<Self>) {
        if self.annotator.erase_picked(&self.collab) {
            self.draw_current(cx);
            cx.notify();
        }
    }

    /// Takes up `tool`, dropping whatever was half-drawn.
    fn take_up(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.annotator.take_up(tool);
        self.draw_current(cx);
        cx.notify();
    }

    /// Whether a drawing tool is in hand, which is what decides who gets the
    /// pointer.
    fn has_tool(&self) -> bool {
        self.annotator.has_tool()
    }

    fn on_drag_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.report_cursor(event.position);
        if self.update_consumable_hover(event.position) {
            cx.notify();
        }

        let position = window_point(event.position);
        let held = event.dragging();
        let straight = event.modifiers.shift;
        match self.annotator.pointer_move(&self.collab, &self.map_view(), self.map_origin, position, held, straight) {
            PointerMove::Turned | PointerMove::Stroked => {
                self.draw_current(cx);
                cx.notify();
            }
            PointerMove::Held => {}
            PointerMove::Unclaimed => {
                if self.placed_map().pan_to(position, held) {
                    self.view_changed(window, cx);
                }
            }
        }
    }

    fn on_drag_end(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.map.end_pan();
        let position = window_point(event.position);
        match self.annotator.pointer_up(&self.collab, &self.map_view(), self.map_origin, position) {
            PointerUp::Stroked => {
                self.draw_current(cx);
                cx.notify();
            }
            PointerUp::Turned | PointerUp::Moved => cx.notify(),
            PointerUp::Nothing => {}
        }
    }

    /// A click goes to the tool in hand or picks a drawn shape out; a
    /// double-click on nothing puts the whole map back.
    fn on_viewport_click(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let position = window_point(event.position());
        let add = event.modifiers().secondary(cx);
        let map = self.map_view();
        match self.annotator.click(&self.collab, &map, self.map_origin, position, add, event.click_count()) {
            Click::Stroked => {
                self.draw_current(cx);
                cx.notify();
            }
            Click::Picked | Click::Ignored => cx.notify(),
            Click::ResetView => {
                cx.notify();
                self.set_view(MapViewport::default(), window, cx);
            }
        }
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
        if self.map.zoom_to(value.start()) {
            self.view_changed(window, cx);
        }
    }

    /// What the viewport shows of the map. Test-only: production code reaches
    /// the field.
    #[cfg(test)]
    pub(crate) fn view(&self) -> MapViewport {
        self.map.view()
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
        self.frame.hold_for_test(BgraFrame::from_rgb(&image::RgbImage::new(width, height)));
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
        track
            .battle_marks()
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                div()
                    .id(("replay-renderer-battle-tick", index))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(position))
                    .w(TICK_WIDTH)
                    .bg(crate::theme::text_dim())
                    .into_any_element()
            })
            .collect()
    }

    /// Where playback has reached, as the clock the game showed. Test-only:
    /// the transport reads it with the length.
    #[cfg(test)]
    fn clock_label(&self) -> String {
        self.track().map(|track| self.playback.clock_label(track)).unwrap_or_default()
    }

    /// How long the battle runs for, as the clock would read at its end.
    #[cfg(test)]
    fn length_label(&self) -> String {
        self.track().map(|track| track.length_label()).unwrap_or_default()
    }

    /// Where playback has reached and how much there is, which is what says
    /// whether the end is near without reading the bar.
    fn clock_and_length(&self) -> String {
        self.track().map(|track| self.playback.clock_and_length(track)).unwrap_or_default()
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
            State::Baking => {
                crate::loading::Loading::new("replay-renderer-baking", t!("ui.replay.renderer.baking").into_owned())
                    .into_any_element()
            }
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
            State::Ready(_) => match self.frame.size() {
                Some(_) => {
                    let drawn = Rc::clone(&self.drawn);
                    div()
                        .id("replay-renderer-viewport")
                        .test_support()
                        .relative()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(!self.map.view().is_whole_map(), |this| this.cursor_grab())
                        // Where the frame landed, which is the only way a
                        // pointer position can be turned into a map one.
                        .child(frame_view(&self.frame, Some(drawn)).size_full())
                        .children(self.map_view().hud_strip().zip(self.advantage_hover()).map(|(strip, lines)| {
                            div()
                                .id("replay-renderer-advantage-hover")
                                .absolute()
                                .left(px(strip.left))
                                .top(px(strip.top))
                                .right(px(strip.left))
                                .h(px(strip.height))
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
                        .children(self.rotation_handle_overlay())
                        .children(self.consumable_reading(cx))
                        .on_modifiers_changed(cx.listener(
                            |this, event: &gpui_kit::ModifiersChangedEvent, _window, cx| {
                                if this.ctrl_held == event.modifiers.secondary(cx) {
                                    return;
                                }
                                this.ctrl_held = event.modifiers.secondary(cx);
                                cx.notify();
                            },
                        ))
                        .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_ping))
                        .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_click))
                        .on_scroll(cx.listener(Self::on_scroll))
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
        let last_frame = self.track().map(|track| track.last()).unwrap_or(FrameIndex::FIRST);
        let playing = self.playback.is_playing();
        // Refused until the second walk has read the battle, and at whichever
        // end of it there is nothing further to step to.
        let has_previous = self.previous_event().is_some();
        let has_next = self.next_event().is_some();
        let controls = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .pb_1()
            .child(
                Button::new("replay-renderer-jump-to-start")
                    .child(crate::icons::icon(crate::icons::Glyph::SkipBack))
                    .compact()
                    .disabled(!ready)
                    .tooltip(t!("ui.renderer.controls.jump_to_start").into_owned())
                    .on_click(cx.listener(|this, _event, window, cx| this.go_to(FrameIndex::FIRST, window, cx))),
            )
            .child(
                Button::new("replay-renderer-previous-event")
                    .child(crate::icons::icon(crate::icons::Glyph::Rewind))
                    .compact()
                    .disabled(!ready || !has_previous)
                    .tooltip(t!("ui.renderer.controls.previous_event").into_owned())
                    .on_click(cx.listener(|this, _event, window, cx| this.jump_to_previous_event(window, cx))),
            )
            .child(
                Button::new("replay-renderer-back-10s")
                    .child(crate::icons::icon(crate::icons::Glyph::RotateCcw))
                    .compact()
                    .disabled(!ready)
                    .tooltip(t!("ui.renderer.controls.back_10s").into_owned())
                    .on_click(cx.listener(|this, _event, window, cx| this.seek_by(-SEEK_STEP, window, cx))),
            )
            .child(
                Button::new("replay-renderer-play")
                    .icon(if playing { IconName::Pause } else { IconName::Play })
                    .compact()
                    .disabled(!ready)
                    .tooltip(
                        t!(if playing { "ui.replay.renderer.pause" } else { "ui.replay.renderer.play" }).into_owned(),
                    )
                    .on_click(cx.listener(|this, _event, window, cx| this.toggle_playing(window, cx))),
            )
            .child(
                Button::new("replay-renderer-forward-10s")
                    .child(crate::icons::icon(crate::icons::Glyph::RotateCw))
                    .compact()
                    .disabled(!ready)
                    .tooltip(t!("ui.renderer.controls.forward_10s").into_owned())
                    .on_click(cx.listener(|this, _event, window, cx| this.seek_by(SEEK_STEP, window, cx))),
            )
            .child(
                Button::new("replay-renderer-next-event")
                    .child(crate::icons::icon(crate::icons::Glyph::FastForward))
                    .compact()
                    .disabled(!ready || !has_next)
                    .tooltip(t!("ui.renderer.controls.next_event").into_owned())
                    .on_click(cx.listener(|this, _event, window, cx| this.jump_to_next_event(window, cx))),
            )
            .child(
                Button::new("replay-renderer-jump-to-end")
                    .child(crate::icons::icon(crate::icons::Glyph::SkipForward))
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
            .child(div().flex_none().text_xs().child(crate::icons::icon(crate::icons::Glyph::Search)))
            .child(div().flex_none().w(ZOOM_WIDTH).child(Slider::new(&self.zoom).disabled(!ready)))
            .child(
                Button::new("replay-renderer-zoom-reset")
                    .label(t!("ui.buttons.reset").into_owned())
                    .compact()
                    .disabled(!ready || self.map.view().is_whole_map())
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
                    .child(crate::icons::icon(crate::icons::Glyph::Download))
                    .compact()
                    .disabled(!ready || self.export.is_some())
                    .tooltip(t!("ui.replay.renderer.export_video").into_owned())
                    .on_click(
                        cx.listener(|this, _event, window, cx| {
                            this.export_asking_first(ExportTarget::File, window, cx)
                        }),
                    ),
            )
            .child(tools_popover(&cx.entity(), self, cx))
            .child(render_options_popover(&cx.entity(), self, ready, cx))
            .child(timeline_popover(&cx.entity(), self, cx))
            .when(!self.popped_out, |this| {
                this.child(
                    Button::new("replay-renderer-pop-out")
                        .child(crate::icons::icon(crate::icons::Glyph::SquareArrowOutUpRight))
                        .compact()
                        .tooltip(t!("ui.replay.renderer.pop_out").into_owned())
                        .on_click(cx.listener(|_this, _event, _window, cx| cx.emit(RendererEvent::PopOut))),
                )
            })
            .child(
                Button::new("replay-renderer-clipboard")
                    .child(crate::icons::icon(crate::icons::Glyph::Clipboard))
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
                    .child(crate::icons::icon(crate::icons::Glyph::X))
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

    let at = pixel_point(menu.at);
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
                    .shadow(crate::theme::overlay_shadow())
                    .child(content),
            ),
        )
        .with_priority(1)
        .into_any_element(),
    )
}

/// Room for the zoom control. Narrow: the seek bar is what the transport is
/// mostly for, and it takes whatever is left.
const ZOOM_WIDTH: Pixels = px(90.);

/// How much one line of wheel travel is worth, for a wheel that reports lines
/// rather than pixels.
const SCROLL_LINE: Pixels = px(20.);

/// Where a pointer is, as the shared map view reads it.
fn window_point(position: Point<Pixels>) -> WindowPoint {
    WindowPoint { x: position.x.as_f32(), y: position.y.as_f32() }
}

/// A window position the shared map view gave back, as GPUI places things.
fn pixel_point(position: WindowPoint) -> Point<Pixels> {
    point(px(position.x), px(position.y))
}

/// The file an export writes.
const MP4: wows_toolkit_services::dialog::Filter =
    wows_toolkit_services::dialog::Filter { label: "MP4", extensions: &["mp4"] };

/// A row of ships to place, for one side.
fn species_row(panel: &Entity<ReplayRendererPanel>, chosen: &Tool, friendly: bool) -> AnyElement {
    let tint = if friendly { FRIENDLY_TINT } else { ENEMY_TINT };
    h_flex()
        .gap_1()
        .children(SPECIES.iter().map(|(short, species)| {
            let owner = panel.clone();
            let species = (*species).to_string();
            let on = annotate::places_ship(chosen, &species, friendly);
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
                        owner.update(cx, |panel, cx| panel.take_up(annotate::ship_tool(&species, friendly), cx));
                    }),
            )
        }))
        .into_any_element()
}

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
    let drawing = view.annotator.drawing();
    let chosen = drawing.tool().clone();
    let ink = drawing.color();
    let nib = drawing.width();
    let undoable = view.annotator.can_undo();
    let picked_ship = view.annotator.picked_ship(&view.collab);
    let matches = view.matched_ships.clone();
    let ship_query = view.ship_query.clone();
    let search = view.ship_search.clone();

    Popover::new("replay-renderer-tools")
        .trigger(
            Button::new("replay-renderer-tools-toggle")
                .child(crate::icons::icon(crate::icons::Glyph::NotebookPen))
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
                        Some(chord) => format!("{} ({chord})", t!(button.label_key)),
                        None => t!(button.label_key).into_owned(),
                    };
                    crate::ui::selectable(
                        button.id,
                        on,
                        Button::new(button.id)
                            .child(crate::icons::icon(button.glyph.into()))
                            .compact()
                            .selected(on)
                            .tooltip(named)
                            .on_click(move |_event, _window, cx: &mut App| {
                                owner.update(cx, |panel, cx| panel.take_up(tool(), cx));
                            }),
                    )
                })))
                .children(
                    picked_ship
                        .clone()
                        .map(|named| ship_chooser(&owner, &search, named, matches.clone(), &ship_query, _cx)),
                )
                .child(crate::ui::rule_h(_cx))
                .child(species_row(&owner, &chosen, true))
                .child(species_row(&owner, &chosen, false))
                .child(crate::ui::rule_h(_cx))
                .child({
                    let owner = owner.clone();
                    Button::new("replay-renderer-annotations-undo")
                        .icon(crate::icons::Glyph::RotateCcw)
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
                                panel.annotator.set_ink(color);
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
                        .filter_map(|button| button.chord.map(|chord| row(chord, t!(button.label_key).into_owned()))),
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
    query: &MarkedQuery,
    cx: &App,
) -> AnyElement {
    v_flex()
        .gap_1()
        .capture_action(crate::text_filter::clear_on_escape(search))
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
                .accessibility_label(ship_match_name(&ship))
                .child(ship_match_label(&ship, query))
                .compact()
                .on_click(move |_event, window, cx: &mut App| {
                    let ship = ship.clone();
                    owner.update(cx, |panel, cx| panel.name_picked_ship(species, &ship, window, cx));
                })
        }))
        .into_any_element()
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
                .child(crate::icons::icon(crate::icons::Glyph::Settings))
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
                        .label(t!(toggle.label_key).to_string())
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
    let any_events = !view.events.events().is_empty();
    let filter = view.event_filter().clone();
    let query = view.event_query.clone();
    let viewer_team = view.events.viewer_team();
    let search = view.event_search.clone();
    // Cloned out because the content closure runs after this render returns,
    // when the panel cannot be read.
    let rows: Vec<TimelineRow> = view
        .visible_events()
        .into_iter()
        .map(|event| {
            let (label, hover) = row_text(&event.kind);
            // Marked by `query` when drawn.
            TimelineRow {
                at: event.clock.seconds(),
                label,
                hover,
                tone: row_tone(&event.kind, viewer_team),
                marks: Vec::new(),
            }
        })
        .collect();
    let copyable: String = view.visible_events().into_iter().map(format_timeline_event).collect::<Vec<_>>().join("\n");

    Popover::new("replay-renderer-timeline")
        .trigger(
            Button::new("replay-renderer-timeline-toggle")
                .child(crate::icons::icon(crate::icons::Glyph::List))
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
                .child(timeline_rows(&owner, rows, &query, read, any_events))
                .into_any_element()
        })
        .into_any_element()
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
                .child(
                    div()
                        .flex_1()
                        .capture_action(crate::text_filter::clear_on_escape(search))
                        .child(Input::new(search).small()),
                ),
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

/// The filtered list, or what to say instead of one. A row's label is marked
/// where the search matched it; a row found by a player name its label does
/// not carry is drawn unmarked.
fn timeline_rows(
    panel: &Entity<ReplayRendererPanel>,
    rows: Vec<TimelineRow>,
    query: &MarkedQuery,
    read: bool,
    any_events: bool,
) -> AnyElement {
    if !read {
        return div()
            .py_2()
            .child(crate::loading::LoadingLine::new(t!("ui.replay.timeline_parsing").into_owned()))
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
                .child(div().flex_1().text_xs().text_color(tone_color(row.tone)).child(query.marked(row.label)))
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
    use gpui_kit::AppContext;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;

    use super::FrameIndex;
    use super::ReplayRendererPanel;
    use super::SPEEDS;

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
                assert_eq!(panel.playback.speed(), 20.0);
                assert!(SPEEDS.contains(&panel.playback.speed()), "and it is one of the offered speeds");
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
                panel.go_to(FrameIndex::new(10), window, cx);
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
                panel.go_to(FrameIndex::FIRST, window, cx);
                panel.seek_by(10.0, window, cx);
                assert_eq!(panel.playback.at().get(), 20, "twenty half-second frames");

                panel.seek_by(-10.0, window, cx);
                assert_eq!(panel.playback.at().get(), 0);

                // Neither end runs off the track.
                panel.seek_by(-10.0, window, cx);
                assert_eq!(panel.playback.at().get(), 0);
                panel.go_to(FrameIndex::new(199), window, cx);
                panel.seek_by(10.0, window, cx);
                assert_eq!(panel.playback.at().get(), 199);
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
                panel.go_to(FrameIndex::new(39), window, cx);
                assert_eq!(panel.playback.at().get(), 39);
                assert_eq!(panel.seek.read(cx).value().start(), 39.0);

                panel.go_to(FrameIndex::FIRST, window, cx);
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
                assert_eq!(panel.playback.speed(), 20.0);
                panel.step_speed(1, window, cx);
                assert_eq!(panel.playback.speed(), 40.0);
                panel.step_speed(1, window, cx);
                assert_eq!(panel.playback.speed(), 60.0);
                panel.step_speed(1, window, cx);
                assert_eq!(panel.playback.speed(), 60.0, "there is no rung past the top of the ladder");

                for _ in 0..5 {
                    panel.step_speed(-1, window, cx);
                }
                assert_eq!(panel.playback.speed(), 1.0, "nor below its bottom");
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
                assert_eq!(panel.playback.at().get(), 17, "the middle of the track is reachable");

                panel.on_seek_event(&SliderEvent::Change(SliderValue::Single(3.0)), cx);
                assert_eq!(panel.playback.at().get(), 3);

                // Past the end clamps rather than panicking on the index.
                panel.on_seek_event(&SliderEvent::Change(SliderValue::Single(999.0)), cx);
                assert_eq!(panel.playback.at().get(), 39);
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
                let opened_at = panel.annotator.drawing().width();

                panel.take_up(Tool::Freehand, cx);
                panel.step_nib(1.0, cx);
                assert!(panel.annotator.drawing().width() > opened_at, "] draws thicker");

                for _ in 0..20 {
                    panel.step_nib(1.0, cx);
                }
                assert_eq!(
                    panel.annotator.drawing().width(),
                    super::MAX_NIB,
                    "and stops at the thickest the board offers"
                );

                for _ in 0..20 {
                    panel.step_nib(-1.0, cx);
                }
                assert_eq!(panel.annotator.drawing().width(), super::MIN_NIB, "as it does at the thinnest");
            })
            .expect("the window is open");
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
                let baked = panel.track().expect("the track is ready").len();
                assert!(panel.options().show_torpedoes, "the track was baked with them on");

                panel.set_options(|options, _dead| options.show_torpedoes = false, cx);

                assert!(!panel.options().show_torpedoes);
                assert_eq!(
                    panel.track().expect("the track is ready").len(),
                    baked,
                    "the track is untouched; only what is drawn of it moved"
                );
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
        use gpui_kit::ScrollEvent;
        use gpui_kit::ScrollSource;
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

                let event = ScrollEvent {
                    position: at,
                    delta: ScrollDelta::Pixels(point(px(0.), px(120.))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                    source: ScrollSource::Pointer,
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
        use gpui_kit::ScrollEvent;
        use gpui_kit::ScrollSource;
        use gpui_kit::TouchPhase;
        use gpui_kit::point;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 0.5], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                panel.seed_frame_for_test();
                let event = ScrollEvent {
                    position: point(px(200.), px(4.)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(120.))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                    source: ScrollSource::Pointer,
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
                assert!(!panel.map.is_panning(), "the whole map has nowhere to drag to");
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

                panel.set_at(FrameIndex::new(239), cx);
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
                panel.go_to(FrameIndex::new(10), window, cx);
                assert!(panel.previous_event().is_none());
                assert!(panel.next_event().is_none());

                panel.jump_to_next_event(window, cx);
                panel.jump_to_previous_event(window, cx);
                assert_eq!(panel.playback.at().get(), 10, "playback stayed where it was");
            });
        })
        .expect("the window is open");
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
                assert_eq!(panel.visible_events().len(), 3);

                // Every seeded event is an advantage change, so switching that
                // kind off empties the list without emptying the battle.
                let advantage = kind_index(&panel.events.events()[0].kind);
                panel.set_event_filter(|filter| filter.kinds[advantage] = false, cx);
                assert!(panel.visible_events().is_empty());
                assert!(!panel.events.events().is_empty(), "which is 'nothing matches', not 'nothing happened'");

                panel.set_event_filter(|filter| filter.kinds = [true; KIND_COUNT], cx);
                panel.set_event_filter(|filter| filter.search = "at 30".into(), cx);
                assert_eq!(panel.visible_events().len(), 1, "the search looks through what a row says");

                // Letters that each start a word, which a plain substring
                // search does not find.
                panel.set_event_filter(|filter| filter.search = "a30".into(), cx);
                let kept: Vec<f32> = panel.visible_events().iter().map(|event| event.clock.seconds()).collect();
                assert_eq!(kept, [30.0]);

                panel.set_event_filter(|filter| filter.search = "t".into(), cx);
                let kept: Vec<f32> = panel.visible_events().iter().map(|event| event.clock.seconds()).collect();
                assert_eq!(kept, [10.0, 30.0, 60.0], "a timeline keeps the order things happened in");
            });
        })
        .expect("the window is open");
    }

    /// The timeline draws each row it kept marked where the search matched.
    #[gpui_kit::test]
    fn the_timeline_marks_what_the_search_matched(cx: &mut TestAppContext) {
        let clocks: Vec<f32> = (0..120).map(|frame| frame as f32 * 0.5).collect();
        let (window, panel) = viewport_in_root(cx, clocks);

        cx.update_window(window.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.seed_events_for_test(&[10.0, 30.0, 50.0]);
                panel.set_event_filter(|filter| filter.search = "a30".into(), cx);
            });
            window.render_frame(cx);

            crate::text_filter::take_marks();
            window.click("replay-renderer-timeline-toggle", cx);
            window.render_frame(cx);
            assert!(window.try_find(("replay-renderer-timeline-row", 0usize)).is_some());
            assert!(window.try_find(("replay-renderer-timeline-row", 1usize)).is_none(), "one row is kept");
            assert_eq!(crate::text_filter::marks_for("at 30"), Some(vec![0..1, 3..5]));
        })
        .expect("the window is open");
    }

    /// The ship chooser offers the best named ship first and draws each
    /// name marked where the search matched it.
    #[gpui_kit::test]
    fn the_ship_chooser_ranks_and_marks_the_ships_it_finds(cx: &mut TestAppContext) {
        use wowsunpack::game_params::types::Species;

        use crate::tactics::ShipLookup;
        use crate::text_filter::MarkedQuery;
        use wows_toolkit_armor::catalog::ClassGroup;
        use wows_toolkit_armor::catalog::NationGroup;
        use wows_toolkit_armor::catalog::ShipCatalog;
        use wows_toolkit_armor::catalog::ShipEntry;

        let ship = |display_name: &str| ShipEntry {
            param_index: format!("P{display_name}"),
            display_name: display_name.to_string(),
            filter_fields: wows_toolkit_armor::filter_fields::FilterFields::new(),
            tier: 10,
        };
        let catalog = ShipCatalog {
            nations: vec![NationGroup {
                nation: "japan".to_string(),
                display_name: "Japan".to_string(),
                classes: vec![ClassGroup { species: Species::Destroyer, ships: vec![ship("Hayate"), ship("Yamato")] }],
            }],
            hidden: Vec::new(),
        };
        let names = |found: Vec<(Species, ShipEntry)>| -> Vec<String> {
            found.into_iter().map(|(_, ship)| ship.display_name).collect()
        };
        let (window, panel) = viewport_in_root(cx, vec![0.0, 30.0]);

        cx.update_window(window.into(), |_, _window, cx| {
            panel.update(cx, |panel, _cx| {
                panel.ship_lookup = Some(ShipLookup::new(&catalog));

                panel.ship_query = MarkedQuery::new("ya");
                assert_eq!(names(panel.matching_ships()), ["Yamato", "Hayate"]);

                panel.ship_query = MarkedQuery::new("ymt");
                panel.matched_ships = panel.matching_ships();
                assert_eq!(names(panel.matched_ships.clone()), ["Yamato"]);
            });

            let search = panel.read(cx).ship_search.clone();
            let matches = panel.read(cx).matched_ships.clone();
            crate::text_filter::take_marks();
            let _ = super::ship_chooser(&panel, &search, None, matches, &MarkedQuery::new("ymt"), cx);
            assert_eq!(crate::text_filter::marks_for("Yamato"), Some(vec![0..1, 2..3, 4..5]));
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
                assert_eq!(panel.ranges_for("gapedd"), super::ships::NO_RANGES);

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
                assert!(panel.playback.is_playing());
            })
            .expect("the window is open");

        // Two ticks reach the last frame; the third would run past it.
        for _ in 0..3 {
            cx.executor().advance_clock(wows_toolkit_replay_data::replay_render::playback::TICK * 2);
            cx.run_until_parked();
        }

        window
            .update(cx, |panel, _window, _cx| {
                assert_eq!(panel.playback.at().get(), 2, "playback reached the last frame");
                assert!(!panel.playback.is_playing(), "and stopped there rather than running past it");
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
                panel.set_at(FrameIndex::new(5), cx);
                assert_eq!(panel.playback.at().get(), 0, "there is nowhere to seek to");
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
        wows_toolkit_services::collab::push_ping_aged(&state, [10.0, 10.0], Duration::from_secs(5));
        wows_toolkit_services::collab::push_ping_aged(&state, [20.0, 20.0], Duration::ZERO);
        let (link, _sent) = wows_toolkit_services::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

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
                panel.set_collab(wows_toolkit_services::collab::CollabLink::default(), cx);
            })
            .expect("the window is open");
        wows_toolkit_services::collab::push_ping_aged(&state, [30.0, 30.0], Duration::from_secs(5));
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
                        hp: Some(wows_minimap_renderer::draw_command::HitPoints {
                            current: 100.0,
                            max: 100.0,
                            healable: 0.0,
                            healable_per_charge: 0.0,
                        }),
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
        let (link, _sent) = wows_toolkit_services::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

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
        let (link, sent) = wows_toolkit_services::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

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
                assert!(!panel.map.is_panning(), "the drag went to the tool, not to a pan");
                assert_eq!(panel.map.view(), zoomed, "so the map did not move");

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
                assert!(panel.map.is_panning());
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
        let (link, sent) = wows_toolkit_services::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

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
                assert_eq!(panel.annotator.picked().picked(), [0], "the click picked the circle out");

                let press = MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(on.0, on.1),
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                };
                panel.on_drag_start(&press, window, cx);
                assert!(!panel.map.is_panning(), "the drag moves the shape rather than the map");

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
        let (link, _sent) = wows_toolkit_services::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

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

                let pointer_at = panel.annotator.pointer_at();
                assert_eq!(pointer_at.map(|at| at[0].round()), Some(240.0), "the pointer is followed");
                let part_drawn = panel.annotator.drawing().in_progress(pointer_at.expect("the pointer is on the map"));
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
                panel.annotator.stroke(&panel.collab, DrawStroke::Began { at: [100.0, 100.0] });
                panel.annotator.stroke(&panel.collab, DrawStroke::Ended { at: [200.0, 100.0] });
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
        let (link, sent) = wows_toolkit_services::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, _window, cx| {
                assert!(!panel.annotator.can_undo(), "nothing to undo before anything is drawn");

                panel.take_up(Tool::Line, cx);
                panel.annotator.stroke(&panel.collab, DrawStroke::Began { at: [100.0, 100.0] });
                panel.annotator.stroke(&panel.collab, DrawStroke::Ended { at: [200.0, 100.0] });
                assert_eq!(panel.annotator.undo_depth(), 1, "the list as it was before the line");
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
                assert!(!panel.annotator.can_undo(), "and there is nothing left to undo");
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
        let (link, sent) = wows_toolkit_services::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, _window, cx| {
                panel.take_up(Tool::Ship { species: "Destroyer".to_string(), friendly: false, yaw: 0.75 }, cx);
                panel.annotator.stroke(&panel.collab, DrawStroke::Clicked { at: [150.0, 250.0] });
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
        let (link, sent) = wows_toolkit_services::collab::CollabLink::for_test(std::sync::Arc::clone(&state));

        let window = cx.open_window(size(px(900.), px(900.)), |window, cx| {
            let mut panel = ReplayRendererPanel::ready_for_test(vec![0.0, 30.0], window, cx);
            panel.seed_collab(link, cx);
            panel.seed_frame_for_test();
            panel
        });

        window
            .update(cx, |panel, _window, _cx| {
                // Nothing to choose a ship for until one is picked out.
                assert!(panel.annotator.picked_ship(&panel.collab).is_none());
                panel.annotator.pick_at(&panel.collab, [300.0, 400.0], false);
                assert_eq!(panel.annotator.picked_ship(&panel.collab), Some(None), "picked, and unnamed");
            })
            .expect("the window is open");

        // Nothing was sent by picking alone.
        assert!(sent.try_iter().next().is_none(), "picking a shape changes nothing");

        window
            .update(cx, |panel, window, cx| {
                let ship = wows_toolkit_armor::catalog::ShipEntry {
                    param_index: "PRSC610".to_string(),
                    display_name: "Moskva".to_string(),
                    filter_fields: wows_toolkit_armor::filter_fields::FilterFields::new(),
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
        use std::collections::HashMap;
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
                let unhit = ShipShotTimeline { hits: Vec::new(), health_history: BTreeMap::new() };
                panel.set_events(super::BattleEvents::from_read(Vec::new(), None, HashMap::from([(untouched, unhit)])));
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
        use std::collections::HashMap;
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
                if matches!(event, super::RendererEvent::ArmorFollowed(_)) {
                    counter.set(counter.get() + 1);
                }
            })
        });

        window
            .update(cx, |panel, _window, cx| {
                let timeline = ShipShotTimeline { hits: Vec::new(), health_history };
                let shots = HashMap::from([(ship, timeline.clone())]);
                panel.set_events(super::BattleEvents::from_read(Vec::new(), None, shots));
                // Following starts without a viewer, which is what the
                // menu's own guard is for; seeded here directly.
                let mut feed = wows_toolkit_replay_data::replay_render::armor::RealtimeArmorFeed::new(timeline);
                feed.advance_to(GameClock(0.0));
                panel.armor_following = Some(super::ArmorFollow::new(ship, feed));

                // This battle recorded no hits, so walking it never changes
                // what the viewer shows.
                for frame in 1..6 {
                    panel.set_at(FrameIndex::new(frame), cx);
                }
            })
            .expect("the window is open");

        assert_eq!(told.get(), 0, "nothing changed, so the viewer was left alone");

        // Scrubbing back is the other case: what was drawn can no longer be
        // right, and a hull cannot be un-hit one shell at a time, so the
        // viewer is told to draw it again.
        window.update(cx, |panel, _window, cx| panel.set_at(FrameIndex::new(1), cx)).expect("the window is open");
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

                panel.set_at(FrameIndex::new(100), cx);
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
                assert_eq!(panel.playback.speed(), super::DEFAULT_SPEED);
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

        assert_eq!(ExportStage::Encoding.label_key(), "ui.renderer.encoding");
        assert_eq!(ExportStage::Muxing.label_key(), "ui.renderer.muxing", "and the wait after the last frame is named");
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
                let frame = panel.frame.size().expect("a frame was seeded");
                let (w, h) = (frame.width.0 as f32, frame.height.0 as f32);
                panel.set_drawn_for_test(gpui_kit::Bounds {
                    origin: gpui_kit::point(px(0.), px(0.)),
                    size: gpui_kit::size(px(w * 2.0), px(h)),
                });

                assert_eq!(panel.map_view().shaped_view().widen(), 1.0, "at rest there is no map to put beside it");

                panel.set_view(super::MapViewport::new(1.2, (0.0, 0.0)), window, cx);
                let creeping = panel.map_view().shaped_view().widen();
                assert!(creeping > 1.0 && creeping <= 1.2, "it starts to fill, held to the zoom: {creeping}");

                panel.set_view(super::MapViewport::new(6.0, (0.0, 0.0)), window, cx);
                let filled = panel.map_view().shaped_view().widen();
                assert!(filled > creeping, "and fills further in: {filled} against {creeping}");
                assert!(filled > 1.9 && filled < 2.1, "about the shape of the viewport: {filled}");
            })
            .expect("the window is open");
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
            assert_eq!(panel.read(cx).playback.at().get(), 119, "the end of the track");

            window.click("replay-renderer-back-10s", cx);
            window.render_frame(cx);
            assert_eq!(panel.read(cx).playback.at().get(), 99, "ten seconds is twenty half-second frames");

            window.click("replay-renderer-jump-to-start", cx);
            window.render_frame(cx);
            assert_eq!(panel.read(cx).playback.at().get(), 0);

            window.click("replay-renderer-play", cx);
            window.render_frame(cx);
            assert!(panel.read(cx).playback.is_playing(), "the play control starts playback");
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
            assert_eq!(panel.read(cx).playback.at().get(), 0, "and clicking it moves nothing");

            panel.update(cx, |panel, cx| {
                panel.seed_events_for_test(&[10.0, 30.0]);
                cx.notify();
            });
            window.render_frame(cx);
            assert!(
                window.find("replay-renderer-next-event").focused().is_some(),
                "and takes a click once the battle is read"
            );

            window.click("replay-renderer-next-event", cx);
            window.render_frame(cx);
            assert!(panel.read(cx).playback.at().get() > 0, "which steps to the first thing that happened");
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
