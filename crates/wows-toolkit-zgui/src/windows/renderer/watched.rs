//! A battle someone else is playing back.
//!
//! A peer with no copy of the replay cannot open it, so the end of the session that has it sends
//! what it draws. A frame on that wire is a list of draw commands rather than an image, so this
//! draws the battle with its own art at whatever size its own window is, and a peer on a different
//! build sees the same battle rather than a picture of someone else's window.
//!
//! It does not steer: the clock belongs to the end that owns the replay, and this reports where
//! that end has reached rather than offering a transport of its own.

use std::sync::Arc;

use futures::channel::mpsc::UnboundedReceiver;
use landaire_ui::LoadingPace;
use landaire_ui::LoadingProps;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_minimap_renderer::toggles::TOGGLES;
use wows_minimap_renderer::toggles::Toggle;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::minimap_preview::layout_for;
use wows_toolkit_replay_data::minimap_preview::render_map;
use wows_toolkit_replay_data::minimap_preview::render_map_art;
use wows_toolkit_replay_data::replay_render::collab::adopt_range_overrides;
use wows_toolkit_replay_data::replay_render::collab::adopt_trail_overrides;
use wows_toolkit_replay_data::replay_render::collab::ranges_by_player;
use wows_toolkit_replay_data::replay_render::options::playback_options_hidden;
use wows_toolkit_replay_data::replay_render::raster::shown_commands;
use wows_toolkit_replay_data::replay_render::ships::ShipChoices;
use wows_toolkit_replay_data::replay_render::ships::ship_names;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::DisplayVersions;
use wows_toolkit_services::collab::LatestFrame;
use wows_toolkit_services::collab::ReplayId;
use wows_toolkit_services::collab::SharedWindow;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::frames::FrameSink;
use crate::frames::FrameViewProps;
use crate::icons::AppIconProps;
use crate::services::Services;
use crate::windows::renderer::options;
use crate::windows::renderer::options::LayerListProps;
use crate::windows::renderer::session::Layers;

const SHEET: &str = "wt-watched";

const CSS: &str = css!(
    ".wt-watched { display: flex; flex-direction: column; flex: 1; min-height: 0; min-width: 0; }
    .wt-watched__clock {
        display: flex;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 4px 8px;
        font-size: 12px;
        color: var(--wt-color-text-dim);
    }
    .wt-watched__bar {
        flex: 1;
        min-width: 0;
        height: 3px;
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-border);
    }
    .wt-watched__fill { height: 100%; border-radius: var(--lui-radius); background-color: var(--lui-color-accent); }
    .wt-watched__rule { flex: none; height: 1px; background-color: var(--lui-color-border); }
    .wt-watched__body { position: relative; flex: 1; min-height: 0; }
    .wt-watched__frame { position: absolute; left: 0; top: 0; right: 0; bottom: 0; }"
);

/// What the end that owns the replay says about where it has reached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostClock {
    pub clock: GameClock,
    pub frame: usize,
    pub total_frames: usize,
}

impl HostClock {
    /// How far through the battle it is, as a fraction.
    ///
    /// `None` before the host has said how many frames there are, which is not the same as a
    /// battle with none.
    pub fn through(self) -> Option<f32> {
        (self.total_frames > 0).then(|| (self.frame as f32 / self.total_frames as f32).clamp(0.0, 1.0))
    }

    /// The host's clock as the strip reads it, minutes and seconds.
    pub fn label(self) -> String {
        let seconds = self.clock.0.max(0.0);
        format!("{}:{:02}", (seconds / 60.0).floor() as i32, (seconds % 60.0) as i32)
    }
}

/// One watched window's state, as one `Copy` handle.
#[derive(Clone, Copy)]
pub struct WatchSession {
    /// Where the host was when it drew the last frame that arrived. `None` until one has.
    pub clock: RwSignal<Option<HostClock>, LocalStorage>,
    /// Whether a frame has been drawn into the sink.
    pub drawn: RwSignal<bool, LocalStorage>,
    /// What this end draws of the frames the host sends. The host sends every layer it baked.
    pub layers: RwSignal<Layers, LocalStorage>,
    run: StoredValue<WatchRun, LocalStorage>,
}

/// The parts of a watch the view does not read.
struct WatchRun {
    /// Which of the session's windows this is, which is what its frames are addressed to and what
    /// its sink is registered under.
    replay_id: ReplayId,
    /// The map's space name, for drawing with this build's own art where it has it.
    space: String,
    /// The art the owning end sent, for a map this build ships none of.
    art: Option<Arc<image::RgbImage>>,
    game_data: Option<GameDataCache>,
    /// The newest frame the host has sent, taken as it is drawn.
    frames: LatestFrame,
    /// The session's nudges, each a frame or a change to what is drawn. Taken by the view.
    wakes: Option<UnboundedReceiver<()>>,
    /// The commands the last frame carried.
    latest: Option<Vec<DrawCommand>>,
    /// Whose ranges and trails are drawn: nobody's ranges until asked for, as in the renderer.
    choices: ShipChoices,
    /// The ranges the session says each ship shows, by entity. Matched to players frame by
    /// frame, since a frame names only the ships in it.
    range_overrides: Vec<(EntityId, ShipConfigFilter)>,
    /// The versions of the session's display settings this window has taken up.
    display_seen: DisplayVersions,
    sink: FrameSink,
    /// One raster at a time: frames arrive faster than a map-sized image is drawn on a fast
    /// playback. A frame that arrives during one is drawn when it finishes.
    rasterising: bool,
    stale: bool,
    owner: Owner,
    link: CollabLink,
}

impl WatchSession {
    /// Asks the session for the frames of `shared`. Called from a component's body.
    pub fn open(shared: SharedWindow, link: CollabLink, game_data: Option<GameDataCache>) -> Self {
        let (nudge, wakes) = futures::channel::mpsc::unbounded();
        let replay_id = ReplayId::from_raw(shared.replay_id);
        let frames = link.watch_window(replay_id, move || {
            // The view is gone once nothing reads the nudges, and then there is nobody to wake.
            let _ = nudge.unbounded_send(());
        });

        let art = shared.art_png.as_ref().and_then(|png| match image::load_from_memory(png) {
            Ok(art) => Some(Arc::new(art.to_rgb8())),
            // Art that will not decode leaves this drawing with the build's own, which is what a
            // peer that has the map does anyway.
            Err(err) => {
                tracing::warn!("watched playback: the art the host sent could not be read: {err}");
                None
            }
        });

        Self {
            clock: RwSignal::new_local(None),
            drawn: RwSignal::new_local(false),
            // What a renderer opens with, so a battle watched here starts as one played here does.
            layers: RwSignal::new_local(Layers { options: playback_options_hidden(), show_dead_ships: true }),
            run: StoredValue::new_local(WatchRun {
                replay_id,
                space: shared.map_name,
                art,
                game_data,
                frames,
                wakes: Some(wakes),
                latest: None,
                choices: ShipChoices::new(),
                range_overrides: Vec::new(),
                display_seen: DisplayVersions::NONE,
                sink: FrameSink::new(),
                rasterising: false,
                stale: false,
                owner: Owner::current().expect("a watch is opened from a component's body"),
                link,
            }),
        }
    }

    pub fn sink(self) -> FrameSink {
        self.run.with_value(|run| run.sink.clone())
    }

    /// The session's nudges, for the view to follow. `None` once taken.
    pub fn take_wakes(self) -> Option<UnboundedReceiver<()>> {
        self.run.try_update_value(|run| run.wakes.take()).flatten()
    }

    /// Takes up what the session has changed: the host's newest frame and its display settings.
    pub fn follow_session(self) {
        self.follow_session_display();
        self.take_frames();
    }

    /// Takes the newest frame the host has sent. The ones before it are moments the host has
    /// already left, and drawing them would put this viewport behind it.
    fn take_frames(self) {
        let newest = self.run.try_with_value(|run| run.frames.take()).flatten();
        let Some(frame) = newest else { return };

        self.run.update_value(|run| run.latest = Some(frame.commands));
        self.clock.set(Some(HostClock {
            clock: frame.clock,
            frame: frame.frame_index,
            total_frames: frame.total_frames,
        }));
        self.redraw();
    }

    /// Says the session can stop sending frames nobody is drawing.
    pub fn stop(self) {
        self.run.try_with_value(|run| run.link.stop_watching(run.replay_id));
    }

    /// The commands the next raster draws: the last frame, less what the layers leave out. `None`
    /// until a frame has arrived.
    pub fn shown(self) -> Option<Vec<DrawCommand>> {
        self.run
            .try_with_value(|run| {
                let latest = run.latest.as_ref()?;
                let mut choices = run.choices.clone();
                let names = ship_names(std::slice::from_ref(latest));
                choices.replace_ranges(ranges_by_player(&run.range_overrides, &names));
                let shown = self.layers.with_untracked(|layers| {
                    shown_commands(latest.iter().cloned(), &layers.options, layers.show_dead_ships, &choices)
                });
                Some(shown)
            })
            .flatten()
    }

    /// Changes what is drawn and draws the last frame again, so a toggle shows before the host
    /// sends another.
    pub fn set_layers(self, apply: impl FnOnce(&mut Layers)) {
        let mut layers = self.layers.get_untracked();
        apply(&mut layers);
        self.layers.set(layers);
        self.redraw();
    }

    /// Turns one of the display toggles on or off, and tells the session which layers changed,
    /// as an egui peer does from the same toggles. Refused while the host has locked the
    /// settings, since the host drops the change.
    pub fn toggle_layer(self, toggle: &'static Toggle, on: bool) {
        if self.run.try_with_value(|run| run.link.settings_locked()) != Some(false) {
            return;
        }
        let was = self.layers.with_untracked(Layers::collab_display);
        self.set_layers(|layers| (toggle.write)(&mut layers.options, &mut layers.show_dead_ships, on));
        let now = self.layers.with_untracked(Layers::collab_display);
        self.run.try_with_value(|run| run.link.send_display_toggles(&was, &now));
    }

    /// Takes up the display settings the session holds where they have changed since last
    /// looked at, over whatever was chosen here, as an egui peer does.
    fn follow_session_display(self) {
        let Some((link, seen)) = self.run.try_with_value(|run| (run.link.clone(), run.display_seen)) else { return };
        let versions = link.display_versions();
        if versions == seen {
            return;
        }
        let mut layers = self.layers.get_untracked();
        let mut taken = seen;

        if versions.settings > seen.settings
            && let Some(settings) = link.display_settings()
        {
            settings.value.apply_to(&mut layers.options, &mut layers.show_dead_ships);
            taken.settings = settings.version;
        }
        let ranges = (versions.ranges > seen.ranges).then(|| link.range_overrides()).flatten();
        let trails = (versions.trails > seen.trails).then(|| link.trail_overrides()).flatten();
        self.run.update_value(|run| {
            if let Some(ranges) = ranges {
                // Before any frame there are no ships to name; `shown` matches them per frame.
                let names =
                    run.latest.as_ref().map(|latest| ship_names(std::slice::from_ref(latest))).unwrap_or_default();
                adopt_range_overrides(&ranges.value, &names, &mut run.choices, &mut layers.options);
                run.range_overrides = ranges.value;
                taken.ranges = ranges.version;
            }
            if let Some(trails) = trails {
                adopt_trail_overrides(&trails.value, &mut run.choices, &mut layers.options);
                taken.trails = trails.version;
            }
            run.display_seen = taken;
        });

        self.layers.set(layers);
        self.redraw();
    }

    /// Draws the frame this viewport last had, off the UI thread.
    fn redraw(self) {
        let Some(commands) = self.shown() else { return };
        let started = self.run.try_update_value(|run| {
            // The fonts and icons a frame needs are this build's own: without them there is
            // nothing to draw the commands with.
            let game_data = run.game_data.clone()?;
            if run.rasterising {
                run.stale = true;
                return None;
            }
            run.rasterising = true;
            let job = RasterJob {
                commands,
                game_data,
                space: run.space.clone(),
                art: run.art.clone(),
                replay_id: run.replay_id,
                sink: run.sink.clone(),
            };
            Some((job, run.owner.clone()))
        });
        let Some((job, owner)) = started.flatten() else { return };

        owner.with(|| {
            spawn_local(async move {
                let drawn = blocking(move || job.run()).await;
                self.finish(drawn);
            });
        });
    }

    fn finish(self, drawn: bool) {
        let again = self.run.try_update_value(|run| {
            run.rasterising = false;
            std::mem::take(&mut run.stale)
        });
        let Some(again) = again else { return };
        if drawn && !self.drawn.get_untracked() {
            self.drawn.set(true);
        }
        if again {
            self.redraw();
        }
    }
}

/// One frame to rasterise, carried off the UI thread.
struct RasterJob {
    commands: Vec<DrawCommand>,
    game_data: GameDataCache,
    space: String,
    art: Option<Arc<image::RgbImage>>,
    replay_id: ReplayId,
    sink: FrameSink,
}

impl RasterJob {
    /// Draws the frame into the sink. Returns whether there was anything to draw it over.
    fn run(self) -> bool {
        let view = MapViewport::default();
        let frame = match &self.art {
            Some(art) => {
                let held_as = format!("watched-{}", self.replay_id.raw());
                render_map_art(&held_as, art, &self.game_data, view, &self.commands)
            }
            None => render_map(&self.space, &self.game_data, view, &self.commands),
        };
        let Some(frame) = frame else { return false };
        self.sink.push(frame);

        true
    }
}

/// A viewport onto a battle the session is playing elsewhere.
#[component]
pub fn WatchedPlayback(shared: SharedWindow, link: CollabLink) -> impl IntoView {
    let game_data = Services::current().game_data.cache_untracked();
    let watch = WatchSession::open(shared, link, game_data);

    view! { WatchedView(watch = watch) }
}

/// What `watch` shows, following the session while it is up and stopping the watch as it goes.
#[component]
pub(super) fn WatchedView(watch: WatchSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    watch.follow_session();
    if let Some(mut wakes) = watch.take_wakes() {
        spawn_local(async move {
            use futures::StreamExt as _;

            while wakes.next().await.is_some() {
                // A burst of nudges is one look at the session.
                while wakes.try_recv().is_ok() {}
                watch.follow_session();
            }
        });
    }
    on_cleanup_local(move || watch.stop());
    let sink = watch.sink();

    view! {
        column(class = "wt-watched", attr:data-testid = "watched-playback") {
            HostClockStrip(watch = watch)
            box(class = "wt-watched__rule")
            box(class = "wt-watched__body") {
                Show(when = move || watch.drawn.get()) {
                    FrameView(sink = sink.clone(), class = "wt-watched__frame")
                }
                Show(when = move || !watch.drawn.get()) {
                    // The host may never send a frame, so the wait has no end.
                    Loading(
                        label = t!("ui.collab.waiting_for_frames").into_owned(),
                        pace = LoadingPace::Unbounded,
                        attr:data-testid = "watched-playback-waiting"
                    )
                }
            }
        }
    }
}

/// Where the end that owns the replay has reached, as a strip to read, with what this end draws
/// of it.
#[component]
fn HostClockStrip(watch: WatchSession) -> impl IntoView {
    let clock = move || watch.clock.get();
    let label = move || clock().map(HostClock::label).unwrap_or_default();
    let through = move || clock().and_then(HostClock::through);

    view! {
        Show(when = move || clock().is_some()) {
            row(class = "wt-watched__clock", attr:data-testid = "watched-playback-clock") {
                text {{t!("ui.collab.watching").into_owned()}}
                text(attr:data-testid = "watched-playback-clock-time") {{label}}
                Show(when = move || through().is_some()) {
                    box(class = "wt-watched__bar") {
                        box(
                            class = "wt-watched__fill",
                            attr:data-testid = "watched-playback-progress",
                            style:width = move || through().map(|through| format!("{}%", through * 100.0))
                        )
                    }
                }
                WatchedLayers(watch = watch)
            }
        }
    }
}

/// The renderer's display toggles over the host's frames, less those a watched frame has no room
/// for.
#[component]
fn WatchedLayers(watch: WatchSession) -> impl IntoView {
    install_stylesheet(options::SHEET, options::CSS);
    let label = t!("ui.renderer.settings.title").into_owned();
    let named = label.clone();
    let set = UnsyncCallback::new(move |(toggle, on): (&'static Toggle, bool)| watch.toggle_layer(toggle, on));

    view! {
        Popover {
            PopoverTrigger(
                variant = ButtonVariant::Outline,
                size = ButtonSize::IconXs,
                attr:data-testid = "watched-playback-layers-toggle",
                a11y:label = label.clone()
            ) {
                Tooltip {
                    TooltipTrigger {
                        AppIcon(glyph = Glyph::Settings)
                    }
                    TooltipContent {{named.clone()}}
                }
            }
            PopoverContent(
                placement = Placement::new(Side::Bottom, Align::End),
                class = "wt-renderer-options__panel",
                attr:data-testid = "watched-playback-layers-list"
            ) {
                LayerList(layers = watch.layers, toggles = map_toggles(), set = set)
            }
        }
    }
}

/// The toggles a watched frame can show. It is drawn on the map alone, so the layers that need a
/// gutter beside it are left out.
fn map_toggles() -> Vec<&'static Toggle> {
    TOGGLES
        .iter()
        .filter(|toggle| {
            let mut options =
                RenderOptions { show_team_rosters: false, show_stats_panel: false, ..RenderOptions::default() };
            let mut show_dead_ships = true;
            (toggle.write)(&mut options, &mut show_dead_ships, true);
            layout_for(&options) == SidePanelLayout::None
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use wows_replays::types::GameClock;

    use super::HostClock;

    /// How far through reads as a fraction, and says nothing at all before the host has said how
    /// long the battle is.
    #[test]
    fn progress_needs_a_length_to_be_a_fraction() {
        let at = |frame, total_frames| HostClock { clock: GameClock(10.0), frame, total_frames };

        assert_eq!(HostClock { clock: GameClock(0.0), frame: 0, total_frames: 0 }.through(), None);
        assert_eq!(at(5, 10).through(), Some(0.5));
        assert_eq!(at(0, 10).through(), Some(0.0));
    }

    /// A frame past the end reads as the end rather than as more than all of it.
    #[test]
    fn a_frame_past_the_end_reads_as_the_end() {
        assert_eq!(HostClock { clock: GameClock(10.0), frame: 30, total_frames: 10 }.through(), Some(1.0));
    }
}
