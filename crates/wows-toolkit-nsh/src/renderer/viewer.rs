//! One playback viewport's state: the baked battle and where playback stands
//! in it, what is drawn, the reader's pointer and keys, and the drawings shared
//! with a session. The producer thread feeds it surface input and draws what it
//! asks for; the module's commands change it from the view.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rust_i18n::t;
use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::config::should_draw_command;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::drawing::DrawnRegion;
use wows_minimap_renderer::drawing::RegionKind;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_minimap_renderer::viewport::MAX_ZOOM;
use wows_minimap_renderer::viewport::MIN_ZOOM;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replay_insights::timeline::KIND_COUNT;
use wows_replay_insights::timeline::ShipShotTimeline;
use wows_replay_insights::timeline::TimelineEvent;
use wows_replay_insights::timeline::TimelineFilter;
use wows_replay_insights::timeline::format_timeline_event;
use wows_toolkit_replay_data::minimap::SharedPreviewRenderer;
use wows_toolkit_replay_data::minimap::layout_for;
use wows_toolkit_replay_data::playback;
use wows_toolkit_replay_data::playback::ALL_RANGES;
use wows_toolkit_replay_data::playback::AdvantageReading;
use wows_toolkit_replay_data::playback::DEFAULT_INK;
use wows_toolkit_replay_data::playback::DEFAULT_MAP_ORIGIN;
use wows_toolkit_replay_data::playback::DEFAULT_NIB;
use wows_toolkit_replay_data::playback::DEFAULT_SPEED;
use wows_toolkit_replay_data::playback::ExportProgress;
use wows_toolkit_replay_data::playback::ExportSettings;
use wows_toolkit_replay_data::playback::HANDLE_DISTANCE;
use wows_toolkit_replay_data::playback::HANDLE_RADIUS;
use wows_toolkit_replay_data::playback::HISTORY_DEPTH;
use wows_toolkit_replay_data::playback::MAX_NIB;
use wows_toolkit_replay_data::playback::MIN_NIB;
use wows_toolkit_replay_data::playback::NO_RANGES;
use wows_toolkit_replay_data::playback::PING_SECONDS;
use wows_toolkit_replay_data::playback::SEEK_STEP;
use wows_toolkit_replay_data::playback::SHIP_MATCHES;
use wows_toolkit_replay_data::playback::SPEEDS;
use wows_toolkit_replay_data::playback::ShipPick;
use wows_toolkit_replay_data::playback::TICK;
use wows_toolkit_replay_data::playback::Track;
use wows_toolkit_replay_data::playback::ZOOM_PER_PIXEL;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_viewmodel::ship_catalog::ShipCatalog;
use wows_toolkit_viewmodel::ship_catalog::ShipEntry;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::TeamId;
use wt_collab_client::drawing::Drawing;
use wt_collab_client::drawing::Drawn;
use wt_collab_client::drawing::Held;
use wt_collab_client::drawing::Selection;
use wt_collab_client::drawing::Stroke;
use wt_collab_client::drawing::Tool;
use wt_collab_client::ids::ReplayId;
use wt_collab_client::link::CollabLink;
use wt_collab_client::types::Annotation;

use crate::frames::Modifiers;
use crate::frames::PixelPoint;
use crate::frames::PixelSize;
use crate::frames::PointerButton;
use crate::frames::SurfaceInput;
use crate::renderer::armor_link::ArmorLink;
use crate::renderer::armor_link::RealtimeFollow;
use crate::renderer::armor_link::RealtimeShow;

/// How far, in device pixels, a press may travel and still be a click.
const CLICK_SLOP: f32 = 4.0;

/// How much one wheel step is worth in pixels of travel, for the zoom rate.
const WHEEL_STEP_PIXELS: f32 = 20.0;

/// The size of the view the frames are drawn for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub size: PixelSize,
    /// Device pixels per point.
    pub scale: f32,
}

/// What the viewport is doing.
pub enum Phase {
    /// Reading the replay and walking the battle.
    Baking,
    Ready(Track),
    Failed(String),
}

/// Where the canvas lands in the viewport: drawn to fit without stretching,
/// centred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub scale: f32,
    pub left: f32,
    pub top: f32,
}

impl Fit {
    /// `canvas` fitted into `into`, or `None` while either is empty.
    pub fn of(canvas: (u32, u32), into: PixelSize) -> Option<Self> {
        if canvas.0 == 0 || canvas.1 == 0 || into.is_empty() {
            return None;
        }
        let scale = (into.width as f32 / canvas.0 as f32).min(into.height as f32 / canvas.1 as f32);
        let left = (into.width as f32 - canvas.0 as f32 * scale) / 2.0;
        let top = (into.height as f32 - canvas.1 as f32 * scale) / 2.0;

        Some(Self { scale, left, top })
    }
}

/// What a hover over the frame reads: a roster consumable or the advantage
/// label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    /// Where the pointer is, in device pixels.
    pub at: (i32, i32),
    pub title: String,
    pub lines: Vec<String>,
    /// The reading's explanation, under a rule.
    pub detail: Option<String>,
}

/// A press of the primary button the producer is following.
#[derive(Clone, Debug)]
enum Press {
    /// Building a shape with the tool in hand.
    Stroking,
    /// Turning a picked shape by its handle, from how it was.
    Turning { index: usize, before: Annotation },
    /// Moving the picked shapes, from where the drag began in map space and
    /// what they looked like then.
    Moving { from: [f32; 2], before: Vec<Annotation> },
    /// Panning a zoomed map.
    Panning { last: PixelPoint },
    /// Nothing yet; it becomes a click if it does not travel.
    Idle,
}

#[derive(Clone, Debug)]
struct Pressed {
    at: PixelPoint,
    clicks: u32,
    modifiers: Modifiers,
    press: Press,
}

/// A message about something the viewport did, for the view to show once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub id: u32,
    pub text: String,
}

/// The work an export needs, taken from the viewer while it starts.
pub struct ExportJob {
    pub renderer: SharedPreviewRenderer,
    pub frames: Vec<Vec<DrawCommand>>,
    pub settings: ExportSettings,
}

/// What a frame draws.
pub struct DrawJob {
    pub renderer: SharedPreviewRenderer,
    pub commands: Vec<DrawCommand>,
    pub view: MapViewport,
    pub viewport: Viewport,
    /// The rotation handle and the point on its shape the stem runs to, in
    /// device pixels.
    pub handle: Option<((f32, f32), (f32, f32))>,
    /// The peers' pings: where, in device pixels, how far grown, and colour
    /// with alpha.
    pub pings: Vec<PingMark>,
}

/// One ping's ring as drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PingMark {
    pub at: (f32, f32),
    pub radius: f32,
    pub color: [u8; 3],
    pub alpha: f32,
}

/// A peer's pointer, in points from the frame's top left.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerCursor {
    pub at: (f32, f32),
    pub name: String,
    pub color: [u8; 3],
}

/// One playback viewport.
pub struct Viewer {
    path: PathBuf,
    title: String,
    phase: Phase,
    renderer: Option<SharedPreviewRenderer>,
    alts: Vec<PathBuf>,

    at: usize,
    playing: bool,
    speed: f32,
    view: MapViewport,
    viewport: Option<Viewport>,
    canvas: Option<(u32, u32)>,

    options: RenderOptions,
    show_dead_ships: bool,
    layout: SidePanelLayout,
    trail_hidden: HashSet<String>,
    ship_ranges: HashMap<String, ShipConfigFilter>,
    export_settings: ExportSettings,
    export: Option<ExportProgress>,
    export_failure: Option<String>,

    events: Vec<TimelineEvent>,
    events_read: bool,
    viewer_team: Option<TeamId>,
    event_filter: TimelineFilter,

    regions: Vec<DrawnRegion>,
    pointer: Option<PixelPoint>,
    reading: Option<Reading>,
    pressed: Option<Pressed>,
    pinch_base: Option<f32>,
    ctrl_held: bool,

    collab: CollabLink,
    replay_id: ReplayId,
    shared: bool,
    reported_cursor: Option<[f32; 2]>,
    drawing: Drawing,
    picked: Selection,
    history: Vec<Vec<Held>>,
    ship_query: String,
    matched_ships: Vec<(Species, ShipEntry)>,
    catalog: Option<Arc<ShipCatalog>>,

    notice: Option<Notice>,
    next_notice: u32,
    /// The armor viewer this viewport drives, once a ship's armor was asked for.
    armor: ArmorLink,

    /// The frame on screen is stale.
    dirty: bool,
    /// What the view reads changed.
    changed: bool,
}

impl Viewer {
    pub fn new(path: PathBuf, alts: Vec<PathBuf>, collab: CollabLink) -> Self {
        let title = path.file_stem().map_or_else(
            || t!("ui.replay.context.render_replay").into_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );

        Self {
            path,
            title,
            phase: Phase::Baking,
            renderer: None,
            alts,
            at: 0,
            playing: false,
            speed: DEFAULT_SPEED,
            view: MapViewport::default(),
            viewport: None,
            canvas: None,
            // The options the track is baked under, so what is drawn at first
            // is exactly what is in it.
            options: playback::playback_options_hidden(),
            show_dead_ships: true,
            layout: SidePanelLayout::None,
            trail_hidden: HashSet::new(),
            ship_ranges: HashMap::new(),
            export_settings: ExportSettings::default(),
            export: None,
            export_failure: None,
            events: Vec::new(),
            events_read: false,
            viewer_team: None,
            event_filter: TimelineFilter::default(),
            regions: Vec::new(),
            pointer: None,
            reading: None,
            pressed: None,
            pinch_base: None,
            ctrl_held: false,
            collab,
            replay_id: ReplayId::fresh(),
            shared: false,
            reported_cursor: None,
            drawing: Drawing::new(DEFAULT_INK, DEFAULT_NIB),
            picked: Selection::default(),
            history: Vec::new(),
            ship_query: String::new(),
            matched_ships: Vec::new(),
            catalog: None,
            notice: None,
            next_notice: 0,
            armor: ArmorLink::default(),
            dirty: true,
            changed: true,
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn alts(&self) -> &[PathBuf] {
        &self.alts
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn track(&self) -> Option<&Track> {
        match &self.phase {
            Phase::Ready(track) => Some(track),
            Phase::Baking | Phase::Failed(_) => None,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.track().is_some()
    }

    pub fn at(&self) -> usize {
        self.at
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn map_view(&self) -> MapViewport {
        self.view
    }

    pub fn viewport(&self) -> Option<Viewport> {
        self.viewport
    }

    pub fn options(&self) -> &RenderOptions {
        &self.options
    }

    pub fn show_dead_ships(&self) -> bool {
        self.show_dead_ships
    }

    pub fn export_settings(&self) -> ExportSettings {
        self.export_settings
    }

    pub fn export(&self) -> Option<ExportProgress> {
        self.export
    }

    pub fn export_failure(&self) -> Option<&str> {
        self.export_failure.as_deref()
    }

    pub fn reading(&self) -> Option<&Reading> {
        self.reading.as_ref()
    }

    pub fn ctrl_held(&self) -> bool {
        self.ctrl_held
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn events_read(&self) -> bool {
        self.events_read
    }

    pub fn events(&self) -> &[TimelineEvent] {
        &self.events
    }

    pub fn viewer_team(&self) -> Option<TeamId> {
        self.viewer_team
    }

    pub fn event_filter(&self) -> &TimelineFilter {
        &self.event_filter
    }

    pub fn collab(&self) -> &CollabLink {
        &self.collab
    }

    pub fn replay_id(&self) -> ReplayId {
        self.replay_id
    }

    pub fn drawing(&self) -> &Drawing {
        &self.drawing
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn ship_query(&self) -> &str {
        &self.ship_query
    }

    pub fn matched_ships(&self) -> &[(Species, ShipEntry)] {
        &self.matched_ships
    }

    /// Takes whether the frame on screen is stale.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Takes whether what the view reads changed.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.changed = true;
    }

    fn say(&mut self, text: String) {
        self.next_notice = self.next_notice.wrapping_add(1);
        self.notice = Some(Notice { id: self.next_notice, text });
        self.changed = true;
    }

    /// How long until the next playback step at the current speed, while
    /// playing.
    pub fn tick_interval(&self) -> Option<Duration> {
        if !self.playing || self.export.is_some() {
            return None;
        }
        Some(Duration::from_secs_f32(TICK.as_secs_f32() / self.speed.max(0.1)))
    }

    /// Adopts a finished bake. Opens showing what the reader saved as the
    /// defaults.
    pub fn baked(&mut self, track: Track, renderer: SharedPreviewRenderer, defaults: &SavedRenderOptions) {
        self.renderer = Some(renderer);
        self.at = self.at.min(track.last());
        self.phase = Phase::Ready(track);
        self.export_settings = playback::export_settings_of(defaults);
        defaults.apply_to(&mut self.options);
        self.show_dead_ships = defaults.show_dead_ships;
        // The stats panel's ship silhouettes are one of the loads a bake skips,
        // so its gutter has nothing to draw in it and the viewport does not
        // offer the layer at all.
        self.options.show_stats_panel = false;
        self.touch();
    }

    /// A battle with no renderer behind it. Test-only: production viewers
    /// reach this state through a bake, and these tests draw nothing.
    #[cfg(test)]
    pub fn adopt_track_for_test(&mut self, track: Track) {
        self.phase = Phase::Ready(track);
        self.touch();
    }

    pub fn bake_failed(&mut self, reason: String) {
        self.phase = Phase::Failed(reason);
        self.touch();
    }

    /// Bakes again through another set of recordings.
    pub fn rebake(&mut self, alts: Vec<PathBuf>) {
        self.alts = alts;
        self.phase = Phase::Baking;
        self.playing = false;
        self.touch();
    }

    /// Adopts the battle's events, once the second walk has read them.
    pub fn events_found(
        &mut self,
        events: Vec<TimelineEvent>,
        viewer_team: Option<TeamId>,
        shots: HashMap<EntityId, ShipShotTimeline>,
    ) {
        self.armor.set_shots(shots);
        self.events = events;
        self.viewer_team = viewer_team;
        self.events_read = true;
        self.changed = true;
    }

    /// The events could not be read. The viewport still plays; only the event
    /// controls are worse off, and they stay refused.
    pub fn events_failed(&mut self) {
        self.events_read = true;
        self.changed = true;
    }

    /// The canvas the options now need, when it differs from the one the
    /// renderer draws on.
    pub fn wanted_layout(&self) -> Option<SidePanelLayout> {
        let wanted = layout_for(&self.options);
        (wanted != self.layout && self.is_ready()).then_some(wanted)
    }

    /// Moves onto the canvas `layout` needs, drawn through `renderer` with the
    /// map's corner at `origin`.
    pub fn adopt_layout(&mut self, layout: SidePanelLayout, renderer: SharedPreviewRenderer, origin: (u32, u32)) {
        self.layout = layout;
        self.renderer = Some(renderer);
        if let Phase::Ready(track) = &mut self.phase {
            track.map_origin = (origin.0 as f32, origin.1 as f32);
        }
        self.canvas = None;
        self.touch();
    }

    /// The canvas a layer needs could not be built, so the layer stays off.
    pub fn layout_failed(&mut self) {
        self.options.show_team_rosters = false;
        self.options.show_stats_panel = false;
        self.touch();
    }

    /// Takes the next step of playback; stops at the end.
    pub fn tick(&mut self) {
        if !self.playing {
            return;
        }
        let Some(last) = self.track().map(Track::last) else { return };
        if self.at >= last {
            self.playing = false;
            self.changed = true;
            return;
        }
        self.set_at(self.at + 1);
    }

    pub fn set_at(&mut self, at: usize) {
        let Some(last) = self.track().map(Track::last) else { return };
        let at = at.min(last);
        if at == self.at {
            return;
        }
        self.at = at;
        self.touch();
    }

    /// Whether the transport may move: refused while the bake runs and while
    /// an export holds the renderer.
    fn steerable(&self) -> bool {
        self.is_ready() && self.export.is_none()
    }

    pub fn toggle_playing(&mut self) {
        if !self.steerable() {
            return;
        }
        self.playing = !self.playing;
        // Playing from the end starts again at the beginning, which is what a
        // transport with no stop button has to mean.
        if self.playing && self.track().is_some_and(|track| self.at >= track.last()) {
            self.set_at(0);
        }
        self.changed = true;
    }

    pub fn go_to(&mut self, at: usize) {
        if self.steerable() {
            self.set_at(at);
        }
    }

    pub fn go_to_end(&mut self) {
        let Some(last) = self.track().map(Track::last) else { return };
        self.go_to(last);
    }

    /// Moves playback `delta` seconds of game time from where it is.
    pub fn seek_by(&mut self, delta: f32) {
        let Some(track) = self.track() else { return };
        let at = track.frame_at(track.seconds_at(self.at) + delta);
        self.go_to(at);
    }

    pub fn set_speed_index(&mut self, index: usize) {
        let Some(speed) = SPEEDS.get(index) else { return };
        if (self.speed - speed).abs() < f32::EPSILON {
            return;
        }
        self.speed = *speed;
        self.changed = true;
    }

    /// Moves one rung along the speed ladder. Nothing happens at either end.
    pub fn step_speed(&mut self, step: isize) {
        let here = playback::speed_index(self.speed);
        if let Some(next) = here.checked_add_signed(step) {
            self.set_speed_index(next);
        }
    }

    /// How far into the battle playback is.
    fn elapsed_now(&self) -> Option<f32> {
        self.track().map(|track| track.elapsed_at(self.at))
    }

    /// The event before where playback is. Half a second back, as the egui
    /// renderer looks: without it, landing on an event and pressing back again
    /// would find the same one.
    pub fn previous_event(&self) -> Option<&TimelineEvent> {
        let here = self.elapsed_now()?;
        self.events.iter().rev().find(|event| event.clock.seconds() < here - 0.5)
    }

    pub fn next_event(&self) -> Option<&TimelineEvent> {
        let here = self.elapsed_now()?;
        self.events.iter().find(|event| event.clock.seconds() > here)
    }

    /// Moves playback to `at` seconds into the battle.
    pub fn go_to_event_at(&mut self, at: f32) {
        let Some(track) = self.track() else { return };
        let frame = track.frame_at(at + track.battle_start.seconds());
        self.go_to(frame);
    }

    /// Moves playback to the frame drawn at `clock`, as the armor viewer's
    /// health strip asks.
    pub fn go_to_clock(&mut self, clock: GameClock) {
        let Some(track) = self.track() else { return };
        let frame = track.frame_at(clock.seconds());
        self.go_to(frame);
    }

    /// Whether the armor viewer could show `entity`'s ship.
    pub fn has_armor_to_show(&self, entity: EntityId) -> bool {
        let loaded = crate::game_data::newest_loaded();
        self.track().is_some_and(|track| self.armor.can_show(track, self.at, entity, loaded.as_deref()))
    }

    /// What the armor viewer opens on for `entity`, which this viewport then
    /// keeps up to date.
    pub fn show_armor(&mut self, entity: EntityId) -> Option<RealtimeShow> {
        let loaded = crate::game_data::newest_loaded();
        let Phase::Ready(track) = &self.phase else { return None };
        self.armor.show(track, self.at, entity, loaded.as_deref())
    }

    /// What the followed ship had taken by where playback is, when that changed.
    pub fn follow_armor(&mut self) -> Option<RealtimeFollow> {
        let clock = GameClock(self.track()?.seconds_at(self.at));
        self.armor.follow(clock)
    }

    fn jump_to_event(&mut self, event: Option<(f32, String)>) {
        let Some((at, said)) = event else { return };
        if !self.steerable() {
            return;
        }
        self.go_to_event_at(at);
        self.say(said);
    }

    pub fn jump_to_previous_event(&mut self) {
        let event = self.previous_event().map(|event| (event.clock.seconds(), format_timeline_event(event)));
        self.jump_to_event(event);
    }

    pub fn jump_to_next_event(&mut self) {
        let event = self.next_event().map(|event| (event.clock.seconds(), format_timeline_event(event)));
        self.jump_to_event(event);
    }

    pub fn set_event_filter(&mut self, apply: impl FnOnce(&mut TimelineFilter)) {
        apply(&mut self.event_filter);
        self.changed = true;
    }

    /// The events the filter admits, in the order they happened.
    pub fn visible_events(&self) -> Vec<&TimelineEvent> {
        self.events.iter().filter(|event| self.event_filter.matches(event)).collect()
    }

    pub fn set_all_kinds(&mut self, on: bool) {
        self.set_event_filter(|filter| filter.kinds = [on; KIND_COUNT]);
    }

    /// Shows a different part of the map.
    pub fn set_view(&mut self, view: MapViewport) {
        if view == self.view {
            return;
        }
        self.view = view;
        self.touch();
    }

    /// The zoom control moves about the middle of what is on screen: there is
    /// no pointer to zoom about.
    pub fn set_zoom(&mut self, zoom: f32) {
        if !self.is_ready() {
            return;
        }
        let middle = MINIMAP_SIZE as f32 / 2.0;
        self.set_view(self.view.zoomed_about(zoom.clamp(MIN_ZOOM, MAX_ZOOM), (middle, middle)));
    }

    pub fn reset_view(&mut self) {
        self.set_view(MapViewport::default());
    }

    /// Changes what is drawn and redraws the frame on screen.
    pub fn set_options(&mut self, apply: impl FnOnce(&mut RenderOptions, &mut bool)) {
        apply(&mut self.options, &mut self.show_dead_ships);
        self.touch();
    }

    pub fn set_export_settings(&mut self, apply: impl FnOnce(&mut ExportSettings)) {
        apply(&mut self.export_settings);
        self.changed = true;
    }

    /// What this viewport shows, as what a viewport opens with next, over
    /// `stored`: the self ranges, which are per-ship here, and the stats panel,
    /// which a bake cannot fill, are left as they were.
    pub fn defaults_over(&self, stored: &SavedRenderOptions) -> SavedRenderOptions {
        let mut saved = stored.clone();
        saved.read_from(&self.options);
        saved.show_stats_panel = stored.show_stats_panel;
        saved.show_dead_ships = self.show_dead_ships;
        saved.prefer_cpu_encoder = self.export_settings.prefer_cpu;
        saved.video_codec = self.export_settings.codec;
        saved.include_pre_battle = self.export_settings.include_pre_battle;
        saved
    }

    pub fn defaults_saved(&mut self, done: bool) {
        let said = if done {
            t!("ui.renderer.settings.save_defaults_done")
        } else {
            t!("ui.renderer.settings.save_defaults_failed")
        };
        self.say(said.into_owned());
    }

    /// Whether `player` has a trail drawn.
    pub fn trail_shown(&self, player: &str) -> bool {
        !self.trail_hidden.contains(player)
    }

    /// Shows or hides one ship's trail. Turning one on turns the trails on;
    /// hiding the last one turns them off rather than leaving an empty layer
    /// switched on.
    pub fn set_trail_shown(&mut self, player: &str, shown: bool) {
        if shown {
            self.trail_hidden.remove(player);
            self.options.show_trails = true;
        } else {
            self.trail_hidden.insert(player.to_owned());
            if self.drawn_ships().iter().all(|name| self.trail_hidden.contains(name)) {
                self.options.show_trails = false;
            }
        }
        self.touch();
    }

    /// Hides every trail but `player`'s.
    pub fn only_trail(&mut self, player: &str) {
        self.trail_hidden = self.drawn_ships().into_iter().filter(|name| name != player).collect();
        self.options.show_trails = true;
        self.touch();
    }

    /// Which ranges `player` has on. A ship with no entry shows none.
    pub fn ranges_for(&self, player: &str) -> ShipConfigFilter {
        self.ship_ranges.get(player).copied().unwrap_or(NO_RANGES)
    }

    pub fn set_ranges_for(&mut self, player: &str, apply: impl FnOnce(&mut ShipConfigFilter)) {
        let mut filter = self.ranges_for(player);
        apply(&mut filter);
        self.ship_ranges.insert(player.to_owned(), filter);
        // Ranges are drawn at all only while some ship has one.
        self.options.show_ship_config = self.ship_ranges.values().any(|filter| *filter != NO_RANGES);
        self.touch();
    }

    /// Leaves only `player`'s ranges on; all of them, if it had none.
    pub fn only_ranges(&mut self, player: &str) {
        let keep = self.ranges_for(player);
        let keep = if keep == NO_RANGES { ALL_RANGES } else { keep };
        self.ship_ranges.clear();
        self.ship_ranges.insert(player.to_owned(), keep);
        self.options.show_ship_config = true;
        self.touch();
    }

    /// Turns every ship's ranges on.
    pub fn all_ranges(&mut self) {
        self.ship_ranges = self.drawn_ships().into_iter().map(|name| (name, ALL_RANGES)).collect();
        self.options.show_ship_config = true;
        self.touch();
    }

    fn drawn_ships(&self) -> Vec<String> {
        self.track().map(|track| track.drawn_ships(self.at)).unwrap_or_default()
    }

    /// The ship under `at` points from the frame's top left, for its menu.
    pub fn ship_under(&self, at: (f32, f32)) -> Option<ShipPick> {
        let scale = self.viewport?.scale;
        let point = PixelPoint { x: at.0 * scale, y: at.1 * scale };
        let (x, y) = self.map_point(point)?;
        self.track()?.ship_near(self.at, (x, y))
    }

    /// Starts an export of what is on screen: the frames the settings cover,
    /// drawn with the layers the reader has on. The overlays drawn over a
    /// battle (trails, annotations, placed ranges) are derived at draw time and
    /// are not written. `None` while one runs or there is nothing to export.
    pub fn begin_export(&mut self) -> Option<ExportJob> {
        if self.export.is_some() {
            return None;
        }
        let track = self.track()?;
        let frames = playback::filtered_frames(
            playback::frames_to_export(track, &self.export_settings),
            &self.options,
            self.show_dead_ships,
            &self.trail_hidden,
            &self.ship_ranges,
        );
        if frames.is_empty() {
            return None;
        }
        // Held for the whole encode: a frame cannot be drawn for two things
        // at once.
        let renderer = self.renderer.take()?;
        self.export_failure = None;
        self.export = Some(ExportProgress {
            done: 0,
            total: frames.len() as u64,
            stage: playback::ExportStage::Encoding,
        });
        self.playing = false;
        self.changed = true;

        Some(ExportJob { renderer, frames, settings: self.export_settings })
    }

    pub fn export_progressed(&mut self, progress: ExportProgress) {
        if self.export.is_some() {
            self.export = Some(progress);
            self.changed = true;
        }
    }

    /// An export ended: the renderer comes back, and a failure is kept to say.
    pub fn export_finished(&mut self, renderer: SharedPreviewRenderer, outcome: Result<Option<String>, String>) {
        self.renderer = Some(renderer);
        self.export = None;
        match outcome {
            Ok(Some(said)) => self.say(said),
            Ok(None) => {}
            Err(reason) => self.export_failure = Some(reason),
        }
        self.touch();
    }

    fn fit(&self) -> Option<Fit> {
        Fit::of(self.canvas?, self.viewport?.size)
    }

    fn map_origin(&self) -> (f32, f32) {
        self.track().map_or(DEFAULT_MAP_ORIGIN, |track| track.map_origin)
    }

    /// The window as it is drawn: stretched toward the shape of the viewport.
    /// Zoomed in there is map to put beside it; at rest there is not, and the
    /// frame stays square.
    pub fn shaped_view(&self) -> MapViewport {
        let Some(viewport) = self.viewport else { return self.view };
        if viewport.size.is_empty() {
            return self.view;
        }
        let aspect = viewport.size.width as f32 / viewport.size.height as f32;
        self.view.widened(aspect * CANVAS_HEIGHT as f32 / MINIMAP_SIZE as f32)
    }

    /// Where a device point lands on the whole canvas. Not clamped: a point
    /// off the edge reads as nothing there.
    fn canvas_point(&self, at: PixelPoint) -> Option<(f32, f32)> {
        let fit = self.fit()?;
        Some(((at.x - fit.left) / fit.scale, (at.y - fit.top) / fit.scale))
    }

    /// Where a device point lands in the drawn map, after zoom and pan.
    fn drawn_point(&self, at: PixelPoint) -> Option<(f32, f32)> {
        let (x, y) = self.canvas_point(at)?;
        let origin = self.map_origin();
        let (x, y) = (x - origin.0, y - origin.1);
        let width = self.shaped_view().drawn_width();
        let height = MINIMAP_SIZE as f32;
        (x >= 0.0 && x < width && y >= 0.0 && y < height).then_some((x, y))
    }

    /// Where a device point lands in map space, which is what ships, shapes
    /// and peers' cursors are in.
    pub fn map_point(&self, at: PixelPoint) -> Option<(f32, f32)> {
        let (x, y) = self.drawn_point(at)?;
        Some(self.shaped_view().to_map(x, y))
    }

    /// The inverse: where a map point is drawn, in device pixels. `None` for a
    /// point the window does not show.
    pub fn device_point(&self, at: (f32, f32)) -> Option<(f32, f32)> {
        let fit = self.fit()?;
        let view = self.shaped_view();
        let drawn = (view.x(at.0), view.y(at.1));
        if drawn.0 < 0.0 || drawn.0 >= view.drawn_width() || drawn.1 < 0.0 || drawn.1 >= MINIMAP_SIZE as f32 {
            return None;
        }
        let origin = self.map_origin();
        Some((fit.left + (drawn.0 + origin.0) * fit.scale, fit.top + (drawn.1 + origin.1) * fit.scale))
    }

    /// Takes one input event of the frame surface.
    pub fn apply(&mut self, input: SurfaceInput) {
        if !matches!(input, SurfaceInput::Pinch { .. }) {
            self.pinch_base = None;
        }
        match input {
            SurfaceInput::Resized { size, scale } => {
                self.viewport = Some(Viewport { size, scale });
                self.touch();
            }
            SurfaceInput::PointerDown { at, button: PointerButton::Primary, clicks, modifiers } => {
                self.pointer_down(at, clicks, modifiers);
            }
            SurfaceInput::PointerDown { at, button: PointerButton::Middle, .. } => self.ping(at),
            // The secondary button opens the ship menu through the view, and
            // the others mean nothing here.
            SurfaceInput::PointerDown { .. } => {}
            SurfaceInput::PointerUp { at, button: PointerButton::Primary, .. } => self.pointer_up(at),
            SurfaceInput::PointerUp { .. } => {}
            SurfaceInput::PointerMoved { at, modifiers } => self.pointer_moved(at, modifiers),
            SurfaceInput::PointerLeft => {
                self.pointer = None;
                self.report_cursor(None);
                if self.reading.take().is_some() {
                    self.changed = true;
                }
            }
            SurfaceInput::Wheel { at, dy, .. } => self.wheel(at, dy),
            SurfaceInput::Pinch { scale } => {
                let base = *self.pinch_base.get_or_insert(self.view.zoom());
                self.set_zoom(base * scale);
            }
            SurfaceInput::Key { key, pressed, modifiers } => self.key(&key, pressed, modifiers),
            SurfaceInput::Focus { focused } => {
                if !focused && self.ctrl_held {
                    self.ctrl_held = false;
                    self.changed = true;
                }
            }
        }
    }

    /// The transport's keyboard, the egui renderer's own, and the drawing
    /// chords the egui board takes.
    fn key(&mut self, key: &str, pressed: bool, modifiers: Modifiers) {
        if matches!(key, "Control_L" | "Control_R") {
            if self.ctrl_held != pressed {
                self.ctrl_held = pressed;
                self.changed = true;
            }
            return;
        }
        if !pressed || !self.is_ready() {
            return;
        }
        if modifiers.control {
            if key == "z" {
                self.undo();
            } else if let Some(tool) = tool_for_key(key) {
                self.take_up(tool);
            }
            return;
        }
        match key {
            "space" => self.toggle_playing(),
            "Up" => self.step_speed(1),
            "Down" => self.step_speed(-1),
            "Left" if modifiers.shift => self.jump_to_previous_event(),
            "Right" if modifiers.shift => self.jump_to_next_event(),
            "Left" => self.seek_by(-SEEK_STEP),
            "Right" => self.seek_by(SEEK_STEP),
            // Puts the tool down and drops what was half-drawn.
            "Escape" if self.has_tool() => self.take_up(Tool::None),
            "Delete" | "BackSpace" => self.erase_picked(),
            "bracketleft" if self.has_tool() => self.step_nib(-1.0),
            "bracketright" if self.has_tool() => self.step_nib(1.0),
            // A key the transport does not claim.
            _ => {}
        }
    }

    /// The wheel zooms about whatever is under the pointer.
    fn wheel(&mut self, at: Option<PixelPoint>, dy: f32) {
        if !self.is_ready() || dy == 0.0 {
            return;
        }
        let Some(drawn) = at.and_then(|at| self.drawn_point(at)) else { return };
        // A step down is travel away from the map, which zooms out.
        let travel = -dy * WHEEL_STEP_PIXELS;
        let zoom = self.view.zoom() * (1.0 + travel * ZOOM_PER_PIXEL);
        self.set_view(self.shaped_view().zoomed_about(zoom, drawn));
    }

    fn pointer_down(&mut self, at: PixelPoint, clicks: u32, modifiers: Modifiers) {
        let press = match self.map_point(at) {
            None => Press::Idle,
            Some(point) => {
                let point = [point.0, point.1];
                if self.has_tool() {
                    // A tool in hand takes the drag: a reader dragging to draw
                    // is not asking to pan the map under their line.
                    self.stroke(Stroke::Began { at: point });
                    Press::Stroking
                } else if let Some((index, before)) = self.handle_under(at) {
                    // Snapshotted as the handle is taken hold of: the turn
                    // reaches the session as the pointer moves.
                    self.remember();
                    Press::Turning { index, before }
                } else if !self.picked.is_empty() {
                    Press::Moving { from: point, before: self.collab.annotations() }
                } else if !self.view.is_whole_map() {
                    Press::Panning { last: at }
                } else {
                    Press::Idle
                }
            }
        };
        self.pressed = Some(Pressed { at, clicks, modifiers, press });
    }

    fn pointer_moved(&mut self, at: PixelPoint, modifiers: Modifiers) {
        self.pointer = Some(at);
        let map = self.map_point(at);
        self.report_cursor(map.map(|(x, y)| [x, y]));
        self.update_reading(at);

        let Some(pressed) = self.pressed.clone() else {
            // The part-drawn shape follows the pointer.
            if self.has_tool() && self.drawing.is_drawing() {
                self.dirty = true;
            }
            return;
        };
        match pressed.press {
            Press::Stroking => {
                if let Some((x, y)) = map {
                    self.stroke(Stroke::Moved { at: [x, y], straight: modifiers.shift });
                }
            }
            Press::Turning { index, before } => {
                let Some((x, y)) = map else { return };
                let [left, top, right, bottom] = wt_collab_client::drawing::annotation_bounds(&before);
                let middle = [(left + right) / 2.0, (top + bottom) / 2.0];
                let mut turned = before;
                wt_collab_client::drawing::rotate_annotation(&mut turned, wt_collab_client::drawing::bearing(middle, [x, y]));
                self.collab.update_annotation(index, turned);
                self.touch();
            }
            Press::Moving { .. } | Press::Idle => {}
            Press::Panning { last } => {
                let Some(fit) = self.fit() else { return };
                let delta = ((at.x - last.x) / fit.scale, (at.y - last.y) / fit.scale);
                if let Some(pressed) = self.pressed.as_mut() {
                    pressed.press = Press::Panning { last: at };
                }
                self.set_view(self.shaped_view().dragged(delta));
            }
        }
    }

    fn pointer_up(&mut self, at: PixelPoint) {
        let Some(pressed) = self.pressed.take() else { return };
        let travelled = (at.x - pressed.at.x).hypot(at.y - pressed.at.y);
        let (clicks, add) = (pressed.clicks, pressed.modifiers.control);
        let map = self.map_point(at).map(|(x, y)| [x, y]);
        match pressed.press {
            Press::Stroking => {
                if travelled < CLICK_SLOP {
                    // A tool that places rather than drags wants the click.
                    self.drawing.cancel();
                    if let Some(point) = map {
                        self.stroke(Stroke::Clicked { at: point });
                    }
                } else if let Some(point) = map {
                    self.stroke(Stroke::Ended { at: point });
                }
            }
            // A turn, like a move, reaches the session once more at the end.
            Press::Turning { index, .. } => {
                if let Some(annotation) = self.collab.annotations().get(index).cloned() {
                    self.collab.update_annotation(index, annotation);
                }
                self.changed = true;
            }
            Press::Moving { from, before } => {
                if travelled < CLICK_SLOP {
                    self.click(map, clicks, add);
                    return;
                }
                let Some(point) = map else { return };
                self.remember();
                let delta = [point[0] - from[0], point[1] - from[1]];
                for index in self.picked.picked().to_vec() {
                    let Some(annotation) = before.get(index) else { continue };
                    let mut moved = annotation.clone();
                    wt_collab_client::drawing::move_annotation(&mut moved, delta);
                    self.collab.update_annotation(index, moved);
                }
                self.touch();
            }
            Press::Panning { .. } | Press::Idle => {
                if travelled < CLICK_SLOP {
                    self.click(map, clicks, add);
                }
            }
        }
    }

    /// A click with no tool picks a drawn shape out to move; a double-click
    /// anywhere else puts the whole map back.
    fn click(&mut self, at: Option<[f32; 2]>, clicks: u32, add: bool) {
        if let Some(point) = at {
            let annotations = self.collab.annotations();
            if !annotations.is_empty() {
                self.picked.click(&annotations, point, add);
                self.picked.retain_within(&annotations);
                self.touch();
                if !self.picked.is_empty() {
                    return;
                }
            }
        }
        if clicks >= 2 {
            self.reset_view();
        }
    }

    /// Reads the roster icon or the advantage label under the pointer.
    fn update_reading(&mut self, at: PixelPoint) {
        let found = self.consumable_at(at).or_else(|| self.advantage_at(at));
        let same = |a: &Reading, b: &Reading| a.title == b.title && a.lines == b.lines;
        let changed = match (&self.reading, &found) {
            (Some(was), Some(now)) => !same(was, now) || was.at != now.at,
            (None, None) => false,
            _ => true,
        };
        if changed {
            self.reading = found;
            self.changed = true;
        }
    }

    fn consumable_at(&self, at: PixelPoint) -> Option<Reading> {
        let point = self.canvas_point(at)?;
        let region = self.regions.iter().find(|region| region.contains(point))?;
        let RegionKind::RosterConsumable { entity_id, index } = region.kind;
        let commands = self.track()?.frames.get(self.at)?;
        commands.iter().find_map(|command| {
            let DrawCommand::TeamRoster { rows, .. } = command else { return None };
            let row = rows.iter().find(|row| row.entity_id == entity_id)?;
            let consumable = row.consumables.get(index)?;
            let lines = wows_minimap_renderer::draw_command::consumable_lines(consumable);
            let mut said = vec![row.player_name.clone(), lines.charges.clone()];
            said.extend(lines.timing.clone());
            said.extend(lines.active.clone());
            Some(Reading {
                at: (at.x as i32, at.y as i32),
                title: lines.name.clone(),
                lines: said,
                detail: (!lines.description.is_empty()).then(|| lines.description.clone()),
            })
        })
    }

    /// The advantage label is drawn into the frame, so the strip it sits in
    /// is what the reader hovers.
    fn advantage_at(&self, at: PixelPoint) -> Option<Reading> {
        let (_, y) = self.canvas_point(at)?;
        if !(0.0..DEFAULT_MAP_ORIGIN.1).contains(&y) {
            return None;
        }
        let reading = self.track()?.advantage_reading(self.at)?;
        let lines = match reading {
            AdvantageReading::TeamEliminated => vec![t!("ui.renderer.advantage.team_eliminated").into_owned()],
            AdvantageReading::Rows { rows, hp_incomplete } => {
                let mut lines: Vec<String> =
                    rows.into_iter().map(|(key, contribution)| format!("{}  {contribution}", t!(key))).collect();
                if hp_incomplete {
                    lines.push(t!("ui.renderer.advantage.hp_incomplete").into_owned());
                }
                lines
            }
        };
        Some(Reading {
            at: (at.x as i32, at.y as i32),
            title: t!("ui.renderer.advantage.breakdown").into_owned(),
            lines,
            detail: None,
        })
    }

    /// Hands the viewport a session to talk to, or takes one away. What was
    /// drawn before stays on the map.
    pub fn set_collab(&mut self, link: CollabLink) {
        let carried = self.collab.annotations_held();
        if self.shared {
            self.collab.close_replay(self.replay_id);
        }
        self.collab = link;
        self.collab.adopt(carried);
        self.shared = false;
        self.history.clear();
        if !self.collab.is_active() {
            self.drawing.set_tool(Tool::None);
        }
        self.touch();
    }

    /// Whether the session should be told this playback is open: once, when
    /// there is a battle to name and this end is the one watched.
    pub fn wants_announcing(&self) -> bool {
        !self.shared && self.collab.broadcasts_frames() && self.track().is_some_and(|track| !track.space.is_empty())
    }

    pub fn announced(&mut self) {
        self.shared = true;
    }

    pub fn is_shared(&self) -> bool {
        self.shared
    }

    /// Puts the frame on screen in front of the session, as draw commands so
    /// each peer draws it with its own art at its own size.
    pub fn broadcast_current(&self) {
        if !self.shared {
            return;
        }
        let Some(track) = self.track() else { return };
        let Some(commands) = track.frames.get(self.at) else { return };
        self.collab.broadcast_frame(wt_collab_client::peer::FrameBroadcast {
            replay_id: self.replay_id.raw(),
            clock: track.seconds_at(self.at),
            frame_index: self.at as u32,
            total_frames: track.len() as u32,
            game_duration: track.seconds_at(track.last()),
            commands: commands.clone(),
        });
    }

    /// Tells the session where this app's pointer is on the map, when it has
    /// moved to a different map point.
    fn report_cursor(&mut self, at: Option<[f32; 2]>) {
        if !self.collab.is_active() {
            return;
        }
        let moved = match (self.reported_cursor, at) {
            (Some(was), Some(now)) => (was[0] - now[0]).abs() >= 1.0 || (was[1] - now[1]).abs() >= 1.0,
            (None, None) => false,
            _ => true,
        };
        if moved {
            self.reported_cursor = at;
            self.collab.report_cursor(at);
        }
    }

    /// Drops a ping where the pointer is, for everyone in the session.
    fn ping(&mut self, at: PixelPoint) {
        let Some((x, y)) = self.map_point(at) else { return };
        self.collab.send_ping([x, y]);
    }

    /// Forgets the pings whose ripple has ended; part of a running session's
    /// redraw.
    pub fn follow_session(&mut self) {
        self.collab.drop_stale_pings(Duration::from_secs_f32(PING_SECONDS));
        self.touch();
    }

    /// The peers' pointers, in points from the frame's top left.
    pub fn peer_cursors(&self) -> Vec<PeerCursor> {
        let Some(scale) = self.viewport.map(|viewport| viewport.scale) else { return Vec::new() };
        self.collab
            .peer_cursors()
            .into_iter()
            .filter_map(|cursor| {
                let pos = cursor.pos?;
                let (x, y) = self.device_point((pos[0], pos[1]))?;
                Some(PeerCursor { at: (x / scale, y / scale), name: cursor.name, color: cursor.color })
            })
            .collect()
    }

    fn ping_marks(&self) -> Vec<PingMark> {
        let scale = self.viewport.map_or(1.0, |viewport| viewport.scale);
        self.collab
            .peer_pings()
            .into_iter()
            .filter_map(|ping| {
                let age = ping.time.elapsed().as_secs_f32();
                if age > PING_SECONDS {
                    return None;
                }
                let at = self.device_point((ping.pos[0], ping.pos[1]))?;
                Some(PingMark {
                    at,
                    radius: playback::PING_RADIUS * scale * (0.3 + age / PING_SECONDS),
                    color: ping.color,
                    alpha: 1.0 - age / PING_SECONDS,
                })
            })
            .collect()
    }

    /// Whether a drawing tool is in hand, which decides who gets the pointer.
    pub fn has_tool(&self) -> bool {
        *self.drawing.tool() != Tool::None
    }

    /// Takes up `tool`, dropping whatever was half-drawn. Refused without a
    /// session: an annotation is something everyone in one sees.
    pub fn take_up(&mut self, tool: Tool) {
        if tool != Tool::None && !self.collab.is_active() {
            return;
        }
        self.drawing.set_tool(tool);
        self.touch();
    }

    pub fn set_ink(&mut self, ink: [u8; 4]) {
        self.drawing.set_color(ink);
        self.changed = true;
    }

    /// Widens or narrows the nib, within the range the egui board offers.
    pub fn step_nib(&mut self, step: f32) {
        let width = (self.drawing.width() + step).clamp(MIN_NIB, MAX_NIB);
        if (width - self.drawing.width()).abs() < f32::EPSILON {
            return;
        }
        self.drawing.set_width(width);
        self.changed = true;
    }

    /// Hands a pointer event to the tool and does what it drew.
    fn stroke(&mut self, stroke: Stroke) {
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
        self.touch();
    }

    /// Remembers what the session holds, before changing it. Bounded: a
    /// session left open all evening must not grow a history of every stroke.
    fn remember(&mut self) {
        if !self.collab.is_active() {
            return;
        }
        self.history.push(self.collab.annotations_held());
        if self.history.len() > HISTORY_DEPTH {
            self.history.remove(0);
        }
    }

    /// Puts the session back the way it was before this viewport's last change.
    pub fn undo(&mut self) {
        let Some(was) = self.history.pop() else { return };
        self.collab.restore(&was);
        // What was picked out may no longer be there.
        self.picked.clear();
        self.touch();
    }

    /// Erases whatever is picked out.
    pub fn erase_picked(&mut self) {
        if self.picked.is_empty() {
            return;
        }
        self.remember();
        // Highest index first: erasing shifts what is after it.
        let mut picked = self.picked.picked().to_vec();
        picked.sort_unstable_by(|a, b| b.cmp(a));
        for index in picked {
            self.collab.erase_annotation(index);
        }
        self.picked.clear();
        self.touch();
    }

    /// The shape whose rotation handle is under the device point `at`. A
    /// handle belongs to a single picked shape that has a bearing at all.
    fn handle_under(&self, at: PixelPoint) -> Option<(usize, Annotation)> {
        let (index, annotation, (handle, _)) = self.picked_handle()?;
        let scale = self.viewport.map_or(1.0, |viewport| viewport.scale);
        let reach = (HANDLE_RADIUS + 8.0) * scale;
        let away = (at.x - handle.0).hypot(at.y - handle.1);
        (away < reach).then_some((index, annotation))
    }

    /// The picked shape that can be turned, and where its handle and the top
    /// of the shape are drawn. Above the shape by a fixed screen distance, so
    /// the handle stays the same size and reach at every zoom.
    fn picked_handle(&self) -> Option<(usize, Annotation, ((f32, f32), (f32, f32)))> {
        let index = self.picked.single()?;
        let annotation = self.collab.annotations().get(index)?.clone();
        if !wt_collab_client::drawing::can_rotate(&annotation) {
            return None;
        }
        let [left, top, right, _] = wt_collab_client::drawing::annotation_bounds(&annotation);
        let anchor = self.device_point(((left + right) / 2.0, top))?;
        let scale = self.viewport.map_or(1.0, |viewport| viewport.scale);
        let handle = (anchor.0, anchor.1 - HANDLE_DISTANCE * scale);

        Some((index, annotation, (handle, anchor)))
    }

    /// Whether the picked shape is a ship, and what it is called if one has
    /// been chosen for it.
    pub fn picked_ship(&self) -> Option<Option<String>> {
        let index = self.picked.single()?;
        let annotations = self.collab.annotations();
        let Annotation::Ship { config, .. } = annotations.get(index)? else { return None };
        Some(config.as_ref().map(|config| config.ship_name.clone()).filter(|name| !name.is_empty()))
    }

    /// Looks ships up by name, for naming a placed one. Empty until something
    /// is typed, since every ship at once is not a choice.
    pub fn search_ships(&mut self, query: &str, catalog: impl FnOnce() -> Option<Arc<ShipCatalog>>) {
        self.ship_query = query.to_owned();
        let query = query.trim().to_lowercase();
        self.matched_ships.clear();
        self.changed = true;
        if query.is_empty() {
            return;
        }
        if self.catalog.is_none() {
            self.catalog = catalog();
        }
        let Some(catalog) = self.catalog.clone() else { return };
        for nation in &catalog.nations {
            for class in &nation.classes {
                for ship in &class.ships {
                    if ship.search_name.contains(&query) {
                        self.matched_ships.push((class.species, ship.clone()));
                        if self.matched_ships.len() >= SHIP_MATCHES {
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Gives the picked ship the identity of match `index`, which is what its
    /// ranges are read from. `param_id` is that ship's game param.
    pub fn name_picked_ship(&mut self, index: usize, param_id: u64) {
        let Some(picked) = self.picked.single() else { return };
        let Some((species, ship)) = self.matched_ships.get(index).cloned() else { return };
        let annotations = self.collab.annotations();
        let Some(Annotation::Ship { pos, yaw, friendly, .. }) = annotations.get(picked) else { return };

        self.remember();
        self.collab.update_annotation(
            picked,
            Annotation::Ship {
                pos: *pos,
                yaw: *yaw,
                species: format!("{species:?}"),
                friendly: *friendly,
                config: Some(wt_collab_client::types::AnnotationShipConfig {
                    param_id,
                    ship_name: ship.display_name.clone(),
                    // Stock hull and no modifiers, where the egui chooser
                    // leaves it.
                    ..Default::default()
                }),
            },
        );
        self.ship_query.clear();
        self.matched_ships.clear();
        self.touch();
    }

    /// The ship search's match `index`, by its param index.
    pub fn matched_param_index(&self, index: usize) -> Option<&str> {
        self.matched_ships.get(index).map(|(_, ship)| ship.param_index.as_str())
    }

    /// What the next frame draws, or `None` while there is nothing to draw it
    /// with: no track, no renderer (an export holds it), or no view size.
    pub fn draw_job(&self, placed_ranges: Vec<DrawCommand>) -> Option<DrawJob> {
        let track = self.track()?;
        let renderer = self.renderer.clone()?;
        let viewport = self.viewport.filter(|viewport| !viewport.size.is_empty())?;
        let at = self.at.min(track.last());
        let frame = track.frames.get(at)?;

        // Trails first, so they sit behind everything, as the egui renderer
        // draws them. Filtered here rather than at bake time: a toggle then
        // costs one frame rather than another walk of the battle.
        let trails = if self.options.show_trails {
            wows_minimap_renderer::frame_track::trails_through(&track.frames[..=at])
        } else {
            Vec::new()
        };
        let mut commands: Vec<DrawCommand> = trails
            .into_iter()
            .chain(frame.iter().cloned())
            .filter(|command| should_draw_command(command, &self.options, self.show_dead_ships))
            .filter(|command| playback::per_ship_allows(command, &self.trail_hidden, &self.ship_ranges))
            .collect();
        // After the battle's own map layers, so a drawn line is not buried
        // under a ship.
        let annotations = self.collab.annotations();
        commands.extend(annotations.iter().flat_map(wt_collab_client::geometry::annotation_commands));
        commands.extend(placed_ranges);
        if let Some(pointer) = self.pointer.and_then(|at| self.map_point(at))
            && let Some(part_drawn) = self.drawing.in_progress([pointer.0, pointer.1])
        {
            commands.extend(wt_collab_client::geometry::annotation_commands(&part_drawn));
        }

        Some(DrawJob {
            renderer,
            commands,
            view: self.shaped_view(),
            viewport,
            handle: self.picked_handle().map(|(_, _, handle)| handle),
            pings: self.ping_marks(),
        })
    }

    /// The frame the producer drew: its canvas size and what a pointer can
    /// rest on in it.
    pub fn drew(&mut self, canvas: (u32, u32), regions: Vec<DrawnRegion>) {
        if self.canvas != Some(canvas) {
            self.changed = true;
        }
        self.canvas = Some(canvas);
        self.regions = regions;
        if let Some(pointer) = self.pointer {
            self.update_reading(pointer);
        }
    }

    /// The placed ships whose ranges need drawing: their annotations, with the
    /// map's size and build to read them at.
    pub fn placed_ships(&self) -> Option<(Vec<Annotation>, f32, wowsunpack::data::Version)> {
        let track = self.track()?;
        let ships: Vec<Annotation> = self
            .collab
            .annotations()
            .into_iter()
            .filter(|annotation| matches!(annotation, Annotation::Ship { config: Some(_), .. }))
            .collect();
        (!ships.is_empty()).then(|| (ships, track.space_size, track.version))
    }
}

/// The tool a key takes up with ctrl held, in the order the egui board
/// numbers them; `m` is the measurement.
pub fn tool_for_key(key: &str) -> Option<Tool> {
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
