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

use crate::minimap_preview::SharedPreviewRenderer;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::IconName;
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
use wows_replay_insights::timeline::TimelineEvent;
use wows_replay_insights::timeline::TimelineFilter;
use wows_replay_insights::timeline::format_timeline_event;
use wows_replay_insights::timeline::kind_label_key;
use wows_replay_insights::timeline::row_text;
use wows_replay_insights::timeline::row_tone;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::TeamId;

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
    /// Where a drag of the map last was, in window coordinates.
    dragging: Option<Point<Pixels>>,
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
    _events: Option<Task<()>>,
    /// Set when a frame was asked for while one was still being drawn. The
    /// draw in flight starts another as it finishes, so what ends up on
    /// screen is the last thing asked for rather than the first.
    redraw_wanted: bool,
    focus_handle: FocusHandle,
}

/// How far an export has got, in frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportProgress {
    pub done: u64,
    pub total: u64,
}

impl EventEmitter<PanelEvent> for ReplayRendererPanel {}

/// What the viewport asks of whoever is hosting it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererEvent {
    /// Move this viewport into a window of its own. The dock cannot do that
    /// itself: it does not own the window list.
    PopOut,
}

impl EventEmitter<RendererEvent> for ReplayRendererPanel {}

impl ReplayRendererPanel {
    /// Opens a viewport on `path` and starts baking it.
    pub fn new(
        path: PathBuf,
        title: SharedString,
        game_data: GameDataCache,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let seek = cx.new(|_| seek_slider(0));
        let seek_subscription = cx.subscribe_in(&seek, window, Self::on_seek);
        let zoom = cx.new(|_| zoom_slider());
        let zoom_subscription = cx.subscribe_in(&zoom, window, Self::on_zoom);
        let event_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.replay.timeline_search_hint").into_owned()));
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
            // The options the track was baked under, so what is drawn at
            // first is exactly what is in it.
            options: wows_minimap_renderer::frame_track::bake_options(),
            show_dead_ships: true,
            view: MapViewport::default(),
            zoom,
            _zoom_subscription: Some(zoom_subscription),
            drawn: Rc::new(Cell::new(None)),
            dragging: None,
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
            _events: None,
            redraw_wanted: false,
            speed: DEFAULT_SPEED,
            seek,
            cancel: Arc::new(AtomicBool::new(false)),
            _bake: None,
            _tick: None,
            _seek_subscription: Some(seek_subscription),
            _rebuilt_seek: None,
            focus_handle: cx.focus_handle(),
        };
        panel.start_bake(path.clone(), game_data.clone(), cx);
        panel.start_event_scan(path, game_data, cx);
        panel
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
        let zoom_subscription = cx.subscribe_in(&zoom, window, Self::on_zoom);
        let event_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.replay.timeline_search_hint").into_owned()));
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
            }),
            renderer: None,
            frame: None,
            at: 0,
            playing: false,
            export: None,
            export_failure: None,
            _export: None,
            popped_out: false,
            // The options the track was baked under, so what is drawn at
            // first is exactly what is in it.
            options: wows_minimap_renderer::frame_track::bake_options(),
            show_dead_ships: true,
            view: MapViewport::default(),
            zoom,
            _zoom_subscription: Some(zoom_subscription),
            drawn: Rc::new(Cell::new(None)),
            dragging: None,
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
            _events: None,
            redraw_wanted: false,
            speed: DEFAULT_SPEED,
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
                    Ok(read) => {
                        this.events = read.events;
                        this.viewer_team = read.viewer_team;
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

    /// Opens the per-ship menu on whatever was right-clicked.
    fn on_right_click(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.map_point(event.position) else { return };
        self.menu_for = self.ship_at(at);
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
                best = Some((away, ShipMenu { entity_id: *entity_id, player_name: name }));
            }
        }
        best.map(|(_, menu)| menu)
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
        self._bake = Some(cx.spawn(async move |this, cx| {
            let baked = cx.background_spawn(async move { bake(&path, &game_data, &cancel) }).await;
            let _ = this.update(cx, |this, cx| {
                match baked {
                    Ok((track, renderer)) => {
                        this.renderer = Some(renderer);
                        this.state = State::Ready(track);
                        this.rebuild_seek(cx);
                        this.draw_current(cx);
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
        self.draw_current(cx);
        cx.notify();
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
        match event.keystroke.key.as_str() {
            "space" => self.toggle_playing(window, cx),
            "up" => self.step_speed(1, window, cx),
            "down" => self.step_speed(-1, window, cx),
            "left" if !shift => self.seek_by(-SEEK_STEP, window, cx),
            "right" if !shift => self.seek_by(SEEK_STEP, window, cx),
            "left" => self.jump_to_previous_event(window, cx),
            "right" => self.jump_to_next_event(window, cx),
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

        let options = self.options.clone();
        let show_dead_ships = self.show_dead_ships;
        let view = self.view;
        let trail_hidden = self.trail_hidden.clone();
        let ship_ranges = self.ship_ranges.clone();
        cx.spawn(async move |this, cx| {
            let drawn = cx.background_spawn(async move {
                // Filtered here rather than at bake time: a toggle then costs
                // one frame rather than another walk of the battle.
                // Trails first, so they sit behind everything, as the egui
                // renderer draws them.
                let shown: Vec<DrawCommand> = trails
                    .into_iter()
                    .chain(commands)
                    .filter(|command| should_draw_command(command, &options, show_dead_ships))
                    .filter(|command| per_ship_allows(command, &trail_hidden, &ship_ranges))
                    .collect();
                let image = {
                    let mut drawing = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    to_image(drawing.render_at(view, &shown))
                };
                (renderer, image)
            });
            let (renderer, image) = drawn.await;
            let _ = this.update(cx, |this, cx| {
                this.renderer = Some(renderer);
                this.frame = Some(image);
                cx.notify();
                if std::mem::take(&mut this.redraw_wanted) {
                    this.draw_current(cx);
                }
            });
        })
        .detach();
    }

    /// Asks where to write, then encodes the baked track there.
    ///
    /// The frames are the ones already baked, so nothing is parsed twice: the
    /// track is rasterised through the same renderer the viewport draws with,
    /// and the pixels go straight to the encoder.
    fn export_video(&mut self, cx: &mut Context<Self>) {
        self.export(ExportTarget::File, cx)
    }

    /// Renders to a temporary file and puts that file on the clipboard, so it
    /// pastes into a chat window or an upload dialog.
    ///
    /// The file is left behind deliberately: the clipboard holds a path, and
    /// deleting what it points at would paste nothing.
    fn export_to_clipboard(&mut self, cx: &mut Context<Self>) {
        self.export(ExportTarget::Clipboard, cx)
    }

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
        let frames = self.frames_to_export(track).to_vec();
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
        self.export = Some(ExportProgress { done: 0, total: frames.len() as u64 });
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

    /// Where in the drawn map a window position falls, in the map's own
    /// pixels.
    ///
    /// `None` when the pointer is not over the map: on the HUD strip, or in
    /// the margin beside a frame that does not fill its element.
    fn map_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.drawn.get()?;
        let frame = self.frame.as_ref()?;
        let scale = self.drawn_scale()?;
        let size = frame.size(0);
        let drawn_width = size.width.0 as f32 * scale;
        let drawn_height = size.height.0 as f32 * scale;
        let left = bounds.origin.x.as_f32() + (bounds.size.width.as_f32() - drawn_width) / 2.0;
        let top = bounds.origin.y.as_f32() + (bounds.size.height.as_f32() - drawn_height) / 2.0;

        let x = (position.x.as_f32() - left) / scale - MAP_ORIGIN.0;
        let y = (position.y.as_f32() - top) / scale - MAP_ORIGIN.1;
        let span = wows_minimap_renderer::MINIMAP_SIZE as f32;
        (x >= 0.0 && x < span && y >= 0.0 && y < span).then_some((x, y))
    }

    /// The wheel zooms about whatever is under the pointer.
    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.map_point(event.position) else { return };
        let delta = event.delta.pixel_delta(SCROLL_LINE).y.as_f32();
        if delta == 0.0 {
            return;
        }
        let zoom = self.view.zoom() * (1.0 + delta * ZOOM_PER_PIXEL);
        self.set_view(self.view.zoomed_about(zoom, at), window, cx);
        cx.stop_propagation();
    }

    /// A drag of the map moves it under the pointer.
    ///
    /// Only once there is somewhere to drag to: at the whole map, a drag
    /// would do nothing and holding it would swallow the click.
    fn on_drag_start(&mut self, event: &MouseDownEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        if self.view.is_whole_map() || self.map_point(event.position).is_none() {
            return;
        }
        self.dragging = Some(event.position);
    }

    fn on_drag_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(from) = self.dragging else { return };
        if !event.dragging() {
            self.dragging = None;
            return;
        }
        let Some(scale) = self.drawn_scale() else { return };
        let delta = ((event.position.x - from.x).as_f32() / scale, (event.position.y - from.y).as_f32() / scale);
        self.dragging = Some(event.position);
        self.set_view(self.view.dragged(delta), window, cx);
    }

    fn on_drag_end(&mut self, _event: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.dragging = None;
    }

    /// A double-click puts the whole map back.
    fn on_viewport_click(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
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

impl Render for ReplayRendererPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;

        let body: AnyElement = match &self.state {
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
        };

        let ready = matches!(self.state, State::Ready(_));
        let last_frame = self.frame_count().saturating_sub(1);
        // Refused until the second walk has read the battle, and at whichever
        // end of it there is nothing further to step to.
        let has_previous = self.previous_event().is_some();
        let has_next = self.next_event().is_some();
        let transport = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(border)
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
            .child(crate::ui::rule_v(cx))
            .child(
                Button::new("replay-renderer-export")
                    .child(crate::icons::icon(crate::icons::DOWNLOAD_SIMPLE))
                    .compact()
                    .disabled(!ready || self.export.is_some())
                    .tooltip(t!("ui.replay.renderer.export_video").into_owned())
                    .on_click(cx.listener(|this, _event, _window, cx| this.export_video(cx))),
            )
            .child(render_options_popover(&cx.entity(), self, cx))
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
                    .on_click(cx.listener(|this, _event, _window, cx| this.export_to_clipboard(cx))),
            )
            .child(crate::ui::rule_v(cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .relative()
                    .child(Slider::new(&self.seek).disabled(!ready))
                    .children(self.battle_ticks()),
            )
            .child(
                div()
                    .flex_none()
                    .w(if self.export.is_some() { EXPORT_WIDTH } else { CLOCK_WIDTH })
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    // While an export runs it says how far it has got rather
                    // than where playback is: the transport is held and the
                    // clock would sit still.
                    .child(match self.export {
                        Some(progress) => format!("{} / {}", progress.done, progress.total),
                        None => self.clock_label(),
                    }),
            )
            .children(self.export_failure.as_ref().map(|reason| {
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(rgb(crate::theme::semantic().error))
                    .child(reason.clone())
                    .into_any_element()
            }))
            .child(crate::ui::rule_v(cx))
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
            .children(SPEEDS.map(|speed| {
                let chosen = (self.speed - speed).abs() < f32::EPSILON;
                crate::ui::selectable(
                    ("replay-renderer-speed", (speed * 10.0) as usize),
                    chosen,
                    Button::new(("replay-renderer-speed-button", (speed * 10.0) as usize))
                        .label(speed_label(speed))
                        .compact()
                        .disabled(!ready)
                        .selected(chosen)
                        .on_click(cx.listener(move |this, _event, window, cx| this.set_speed(speed, window, cx))),
                )
            }));

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

/// Room for "MM:SS" without the transport shifting as it counts.
const CLOCK_WIDTH: Pixels = px(44.);

/// Room for an export's frame count, which is wider than a clock.
const EXPORT_WIDTH: Pixels = px(86.);

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

    Some(
        div()
            .absolute()
            .top_2()
            .right_2()
            .w(px(220.))
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(crate::theme::border_bright())
            .bg(crate::theme::surface())
            .child(
                v_flex()
                    .gap_1()
                    .child(
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
                    )
                    .child({
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
                    })
                    .child({
                        let owner = owner.clone();
                        let player = player.clone();
                        Button::new("replay-renderer-ship-only-trail")
                            .label(t!("ui.renderer.context.disable_other_trails").into_owned())
                            .compact()
                            .on_click(move |_event, _window, cx: &mut App| {
                                let player = player.clone();
                                owner.update(cx, |panel, cx| panel.only_trail(&player, cx));
                            })
                    })
                    .child(
                        div()
                            .pt_1()
                            .text_xs()
                            .text_color(crate::theme::text_dim())
                            .child(t!("ui.renderer.context.ranges").into_owned()),
                    )
                    .child(range_switch(
                        "replay-renderer-range-detection",
                        "ui.renderer.context.detection",
                        ranges.detection,
                        |filter, on| filter.detection = on,
                    ))
                    .child(range_switch(
                        "replay-renderer-range-main",
                        "ui.renderer.context.main_battery",
                        ranges.main_battery,
                        |filter, on| filter.main_battery = on,
                    ))
                    .child(range_switch(
                        "replay-renderer-range-secondary",
                        "ui.renderer.context.secondary",
                        ranges.secondary_battery,
                        |filter, on| filter.secondary_battery = on,
                    ))
                    .child(range_switch(
                        "replay-renderer-range-torpedo",
                        "ui.renderer.context.torpedo",
                        ranges.torpedo,
                        |filter, on| filter.torpedo = on,
                    ))
                    .child(range_switch(
                        "replay-renderer-range-radar",
                        "ui.renderer.context.radar",
                        ranges.radar,
                        |filter, on| filter.radar = on,
                    ))
                    .child(range_switch(
                        "replay-renderer-range-hydro",
                        "ui.renderer.context.hydro",
                        ranges.hydro,
                        |filter, on| filter.hydro = on,
                    ))
                    .child(
                        h_flex()
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
                    ),
            )
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
}

/// Room for the zoom control. Narrow: the seek bar is what the transport is
/// mostly for, and it takes whatever is left.
const ZOOM_WIDTH: Pixels = px(90.);

/// Where the map's top-left corner sits in a rendered frame.
///
/// The renderer reserves a strip above the map for the score bar and the
/// timer, and none beside it: a preview is built with no side panel, so the
/// map starts at the frame's left edge.
const MAP_ORIGIN: (f32, f32) = (0.0, wows_minimap_renderer::HUD_HEIGHT as f32);

/// How much one line of wheel travel is worth, for a wheel that reports lines
/// rather than pixels.
const SCROLL_LINE: Pixels = px(20.);

/// How much of a zoom one pixel of wheel travel is. The egui renderer's own
/// rate, so the wheel feels the same in both.
const ZOOM_PER_PIXEL: f32 = 0.01;

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
fn bake(
    path: &std::path::Path,
    game_data: &GameDataCache,
    cancel: &AtomicBool,
) -> Result<(Track, SharedPreviewRenderer), RenderError> {
    let baked =
        crate::minimap_preview::bake_track(path, game_data, cancel, TRACK_BUDGET, BAKE_INTERVAL, playback_options())?;
    let track = Track {
        frames: baked.frames,
        clocks: baked.clocks,
        battle_start: baked.battle_start,
        battle_end: baked.battle_end,
    };
    Ok((track, baked.renderer))
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
        let _ = progress.unbounded_send(ExportProgress { done: index as u64 + 1, total });
    }
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
/// Only the commands the track actually holds: `bake_options` leaves the
/// stats panel, the team rosters, ship-range circles and position trails out
/// of it (they cost a roster or a whole match history per frame), so a switch
/// for those would do nothing and is not offered.
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
];

/// The gear on the transport: what the viewport draws of what it baked.
///
/// `view` rather than its entity, because this is built inside that view's
/// own render where reading the entity would panic; the entity is captured
/// only for the callbacks.
fn render_options_popover(
    panel: &Entity<ReplayRendererPanel>,
    view: &ReplayRendererPanel,
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
                .tooltip(t!("ui.renderer.settings.title").into_owned()),
        )
        .content(move |_state, _window, _cx| {
            let owner = owner.clone();
            let options = options.clone();
            div()
                .w(px(240.))
                .max_h(px(420.))
                .id("replay-renderer-settings-list")
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
        .max_h(px(400.))
        .overflow_y_scroll()
        .child(v_flex().gap_0().children(rows.into_iter().enumerate().map(|(index, row)| {
            let owner = panel.clone();
            let at = row.at;
            div()
                .id(("replay-renderer-timeline-row", index))
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
                let at = point(px(on_map.0), px(on_map.1 + super::MAP_ORIGIN.1));
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
}
