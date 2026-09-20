use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::slider::{Slider, SliderState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::armor_viewer::ArmorViewerPane;
use crate::player_tracker::PlayerTrackerView;
use crate::replay_inspector::GameDataStatus;
use crate::replay_inspector::ReplayInspectorView;
use crate::search::SearchView;
use crate::settings::{DEFAULT_ZOOM, GpuiSettings, MAX_ZOOM, MIN_ZOOM};
use crate::settings_store;
use crate::stats::load::SessionData;
use crate::stats::view::StatsView;
use crate::theme;
use crate::ui::selectable;
use crate::unpacker::view::UnpackerView;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::keys;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppTab {
    ReplayInspector,
    Stats,
    PlayerTracker,
    Search,
    ArmorViewer,
    Unpacker,
    Settings,
}

impl AppTab {
    /// Left-to-right order, following the egui app's own dock order
    /// (`app.rs`'s `DockState::new`): replays first, settings last. The tabs
    /// This is every tab the egui dock opens, in its order. `Tab::ModManager`
    /// is deliberately absent: the egui app never adds it to its dock either.
    pub const ALL: [AppTab; 7] = [
        AppTab::ReplayInspector,
        AppTab::Stats,
        AppTab::PlayerTracker,
        AppTab::Search,
        AppTab::ArmorViewer,
        AppTab::Unpacker,
        AppTab::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AppTab::ReplayInspector => "Replay Inspector",
            AppTab::Stats => "Stats",
            AppTab::PlayerTracker => "Player Tracker",
            AppTab::Search => "Search",
            AppTab::ArmorViewer => "Armor Viewer",
            AppTab::Unpacker => "Unpacker",
            AppTab::Settings => "Settings",
        }
    }
}

/// Load status of the settings snapshot fetched from the shared config DB.
enum SettingsState {
    Loading,
    Loaded(GpuiSettings),
    Failed(String),
}

pub struct App {
    active_tab: AppTab,
    settings: SettingsState,
    /// Session-local zoom shown by the zoom slider. Seeded from `settings.zoom`
    /// once the DB load completes, then updated live as the slider moves.
    /// Never written back to the DB.
    zoom: f32,
    zoom_slider: Entity<SliderState>,
    /// Replay Inspector tab: the file browser plus the per-replay dock.
    /// Starts its background directory scan once `apply_settings` knows the
    /// WoWs directory.
    replay_inspector: Entity<ReplayInspectorView>,
    /// App-wide debug-mode flag, seeded from `AppPreferences.debug_mode` in
    /// `apply_settings`, then flippable at runtime via the global Ctrl+Shift+D
    /// shortcut (`toggle_debug_mode`), matching the egui app's
    /// `app.rs:1898-1909`. Never written back to the DB -- the toggle only
    /// overrides the setting for the running session. Pushed down into
    /// `replay_inspector` on every change; there is no per-tab enable UI.
    debug_mode: bool,
    /// Root focus target for the global Ctrl+Shift+D handler
    /// (`Self::render`'s `on_key_down`): focused once at startup so the
    /// shortcut works before any other element claims focus. Key events from
    /// a later-focused descendant (e.g. an open replay tab) still bubble up
    /// through this element, so the shortcut keeps working regardless of
    /// what has focus -- matching egui's window-wide shortcut.
    focus_handle: FocusHandle,
    /// The Armor Viewer tab: ship sidebar + 3D viewport. See
    /// `armor_viewer::ArmorViewerPane`.
    armor_pane: Entity<ArmorViewerPane>,
    /// Set once `armor_pane.load_game_data` has been kicked off, so the
    /// `replay_inspector` game-data observer (`Self::new`) triggers it
    /// exactly once -- see `Self::poll_armor_game_data`.
    armor_game_data_requested: bool,
    /// The Unpacker tab: build selector, VFS browsers and the extraction
    /// queue. Runs its own VFS load per build, independent of the replay
    /// inspector's game-data cache, since it needs only the package tree.
    unpacker: Entity<UnpackerView>,
    /// The Stats tab: session filters over the per-ship aggregate.
    stats: Entity<StatsView>,
    /// The Player Tracker tab: everyone met, from the replay index.
    player_tracker: Entity<PlayerTrackerView>,
    /// The Search tab: a query over the replay index.
    search: Entity<SearchView>,
    /// Settings tab text fields. Held so an edit can be read back and the
    /// saved value can be shown when the tab first renders.
    wows_dir_input: Entity<InputState>,
    proxy_input: Entity<InputState>,
    settings_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let zoom_slider =
            cx.new(|_| SliderState::new().min(MIN_ZOOM).max(MAX_ZOOM).step(0.05).default_value(DEFAULT_ZOOM));
        let replay_inspector = cx.new(|cx| ReplayInspectorView::new(window, cx));
        let armor_pane = cx.new(|cx| ArmorViewerPane::new(window, cx));
        let unpacker = cx.new(|cx| UnpackerView::new(window, cx));
        let stats = cx.new(|cx| StatsView::new(window, cx));
        let player_tracker = cx.new(|cx| PlayerTrackerView::new(window, cx));
        let search = cx.new(|cx| SearchView::new(window, cx));
        let wows_dir_input = cx.new(|cx| InputState::new(window, cx).placeholder("World of Warships directory"));
        let proxy_input = cx.new(|cx| InputState::new(window, cx).placeholder("http://host:port"));
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        // The Armor Viewer reuses the replay inspector's preloaded game data
        // (see `poll_armor_game_data`'s doc comment) rather than running a
        // second VFS/`GameParams` load for the same build; observing here
        // catches the `Loading` -> `Ready` transition whenever it lands,
        // including if it already landed before this tab is ever opened.
        let subscription = cx.observe(&replay_inspector, |this, _replay_inspector, cx| {
            this.poll_armor_game_data(cx);
        });
        let wows_dir_edited = cx.subscribe_in(&wows_dir_input, window, Self::on_wows_dir_edited);
        let proxy_edited = cx.subscribe(&proxy_input, Self::on_proxy_edited);

        Self {
            active_tab: AppTab::ReplayInspector,
            settings: SettingsState::Loading,
            zoom: DEFAULT_ZOOM,
            zoom_slider,
            replay_inspector,
            debug_mode: false,
            focus_handle,
            armor_pane,
            armor_game_data_requested: false,
            unpacker,
            stats,
            player_tracker,
            search,
            wows_dir_input,
            proxy_input,
            settings_scroll: ScrollHandle::new(),
            _subscriptions: vec![subscription, wows_dir_edited, proxy_edited],
        }
    }

    /// Forwards the replay inspector's preloaded game data to the Armor
    /// Viewer pane the first time BOTH it has reached `GameDataStatus::Ready`
    /// AND the Armor Viewer tab has been opened, so `ArmorViewerPane::
    /// load_game_data` -- which reads the ~167 MiB `content/assets.bin` and
    /// builds the ship catalog/icons -- runs exactly once per session, lazily
    /// on first visit to the tab, with the SAME `Arc<LoadedGameData>` the
    /// replay inspector already loaded (never a second `GameDataCache`/VFS/
    /// `GameParams` load for the same build). A user who never opens the
    /// Armor Viewer never pays that cost. Called from the `cx.observe`
    /// subscription set up in `Self::new` (which fires on every
    /// replay-inspector notification), from the tab bar's `on_click` handler
    /// (`Self::render`) whenever the Armor Viewer tab is selected, and,
    /// redundantly but harmlessly, from `apply_settings` in case the
    /// observer's first notification races the settings load.
    fn poll_armor_game_data(&mut self, cx: &mut Context<Self>) {
        if self.armor_game_data_requested {
            return;
        }
        if self.active_tab != AppTab::ArmorViewer {
            return;
        }
        if let GameDataStatus::Ready(loaded) = self.replay_inspector.read(cx).game_data_status() {
            self.armor_game_data_requested = true;
            self.armor_pane.update(cx, |pane, cx| pane.load_game_data(loaded, cx));
        }
    }

    /// Store the settings snapshot loaded from the shared config DB. Called
    /// once from `main.rs` after the async load completes; read-only for the
    /// lifetime of the session (no write-back). Also kicks off the replay
    /// inspector's background directory scan and game-data cache, now that
    /// the WoWs directory is known.
    /// The Replay Inspector tab's view, so state the tab owns (its grouping,
    /// for one) can be asserted without reaching through the rendered frame.
    /// Test-only: the app itself reaches the field directly.
    #[cfg(test)]
    pub(crate) fn replay_inspector(&self) -> &Entity<ReplayInspectorView> {
        &self.replay_inspector
    }

    /// Starts the Player Tracker's first index query, once the config
    /// database is open.
    pub fn start_player_tracker(&mut self, pool: sqlx::sqlite::SqlitePool, cx: &mut Context<Self>) {
        self.player_tracker.update(cx, |tracker, cx| tracker.refresh(pool, cx));
    }

    /// Adopts the session statistics read from the config database.
    pub fn apply_session_stats(&mut self, data: SessionData, window: &mut Window, cx: &mut Context<Self>) {
        self.stats.update(cx, |stats, cx| stats.apply_session(data, window, cx));
        cx.notify();
    }

    pub fn apply_settings(&mut self, settings: GpuiSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom = settings.zoom;
        self.zoom_slider =
            cx.new(|_| SliderState::new().min(MIN_ZOOM).max(MAX_ZOOM).step(0.05).default_value(settings.zoom));
        let wows_dir = settings.wows_dir.clone();
        let debug_mode = settings.debug_mode;
        let replay_settings = settings.replay.clone();
        let auto_load_latest_replay = settings.auto_load_latest_replay;
        self.debug_mode = debug_mode;
        self.replay_inspector.update(cx, |view, cx| {
            view.apply_settings(wows_dir, debug_mode, replay_settings, auto_load_latest_replay, window, cx)
        });
        self.armor_pane.update(cx, |pane, cx| pane.apply_armor_defaults(settings.armor_defaults.as_ref(), cx));
        // Seed the text fields so the tab opens showing what is saved.
        self.wows_dir_input.update(cx, |state, cx| state.set_value(settings.wows_dir.clone(), window, cx));
        self.proxy_input.update(cx, |state, cx| state.set_value(settings.proxy_url.clone(), window, cx));

        let unpacker_dir = settings.wows_dir.clone();
        self.unpacker.update(cx, |unpacker, cx| unpacker.apply_settings(unpacker_dir, window, cx));
        self.poll_armor_game_data(cx);
        self.settings = SettingsState::Loaded(settings);
    }

    /// Record that the DB load failed. Called once from `main.rs` in place of
    /// `apply_settings` when either the DB open or the settings load errors.
    pub fn mark_settings_failed(&mut self, reason: String) {
        self.settings = SettingsState::Failed(reason);
    }

    /// The global Ctrl+Shift+D handler (`Self::render`'s `on_key_down`):
    /// flips `debug_mode` and pushes the new value into the replay inspector,
    /// matching the egui app's `app.rs:1898-1909` toggle.
    fn toggle_debug_mode(&mut self, cx: &mut Context<Self>) {
        self.debug_mode = !self.debug_mode;
        let debug_mode = self.debug_mode;
        self.replay_inspector.update(cx, |view, cx| view.set_debug_mode(debug_mode, cx));
        cx.notify();
    }
}

fn section_heading(title: &'static str, description: &'static str) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(title))
        .child(div().text_xs().opacity(0.6).child(description))
}

fn settings_row(label: &'static str, value: String) -> impl IntoElement {
    h_flex()
        .gap_2()
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(label))
        .child(div().text_sm().child(if value.is_empty() { "(not set)".to_string() } else { value }))
}

impl App {
    fn render_zoom_row(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Zoom Factor (Ctrl + and Ctrl - also changes this)"),
            )
            .child(Slider::new(&self.zoom_slider).w(px(160.)))
            .child(div().text_sm().w(px(40.)).child(format!("{:.2}", self.zoom)))
            .child(Button::new("reset-zoom").label("Reset").compact().on_click(cx.listener(
                |this, _event: &ClickEvent, window, cx| {
                    this.zoom = DEFAULT_ZOOM;
                    theme::apply_egui_dark_theme(this.zoom, window, cx);
                    this.zoom_slider.update(cx, |slider, slider_cx| {
                        slider.set_value(DEFAULT_ZOOM, window, slider_cx);
                    });
                    cx.notify();
                },
            )))
    }

    fn on_wows_dir_edited(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let path = state.read(cx).value().trim().to_string();
        self.apply_wows_dir(path, window, cx);
    }

    /// Saved on blur or Enter rather than per keystroke, so a half-typed URL
    /// never reaches the database.
    fn on_proxy_edited(&mut self, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let url = state.read(cx).value().trim().to_string();
        let Some(settings) = self.settings_mut() else { return };
        if settings.proxy_url == url {
            return;
        }
        settings.proxy_url = url.clone();
        settings_store::save(keys::PROXY_URL, &url, cx);
    }

    fn settings_mut(&mut self) -> Option<&mut GpuiSettings> {
        match &mut self.settings {
            SettingsState::Loaded(settings) => Some(settings),
            _ => None,
        }
    }

    /// Applies one edit to the in-memory snapshot and persists it.
    ///
    /// Edits take effect immediately and are written as they happen, as the
    /// egui settings tab does; there is no apply step to forget.
    fn edit_setting<T: serde::Serialize>(
        &mut self,
        key: &'static str,
        cx: &mut Context<Self>,
        apply: impl FnOnce(&mut GpuiSettings) -> T,
    ) {
        let Some(settings) = self.settings_mut() else { return };
        let value = apply(settings);
        settings_store::save(key, &value, cx);
        cx.notify();
    }

    /// Rewrites the whole `ReplaySettings` blob, which is stored as one row,
    /// and pushes it into the replay inspector so its columns follow.
    fn edit_replay_settings(&mut self, cx: &mut Context<Self>, apply: impl FnOnce(&mut ReplaySettings)) {
        let Some(settings) = self.settings_mut() else { return };
        apply(&mut settings.replay);
        let replay = settings.replay.clone();
        settings_store::save(keys::REPLAY_SETTINGS, &replay, cx);
        self.replay_inspector.update(cx, |view, cx| view.set_replay_settings(replay.clone(), cx));
        cx.notify();
    }

    fn browse_for_wows_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picked) = rfd::FileDialog::new().set_title("World of Warships directory").pick_folder() else {
            return;
        };
        let path = picked.to_string_lossy().into_owned();
        self.wows_dir_input.update(cx, |state, cx| state.set_value(path.clone(), window, cx));
        self.apply_wows_dir(path, window, cx);
    }

    /// Adopts a new game directory: saved, then pushed into the tabs that read
    /// it so they reload rather than keep showing the old install.
    fn apply_wows_dir(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.settings_mut() else { return };
        if settings.wows_dir == path {
            return;
        }
        settings.wows_dir = path.clone();
        settings_store::save(keys::WOWS_DIR, &path, cx);

        let replay_settings = settings.replay.clone();
        let debug_mode = settings.debug_mode;
        let auto_load = settings.auto_load_latest_replay;
        let for_unpacker = path.clone();
        self.replay_inspector
            .update(cx, |view, cx| view.apply_settings(path, debug_mode, replay_settings, auto_load, window, cx));
        self.unpacker.update(cx, |unpacker, cx| unpacker.apply_settings(for_unpacker, window, cx));
        cx.notify();
    }

    fn render_settings_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = match &self.settings {
            SettingsState::Loading => {
                return v_flex()
                    .p_4()
                    .child(div().text_sm().opacity(0.6).child("Loading settings..."))
                    .into_any_element();
            }
            SettingsState::Failed(reason) => {
                return v_flex()
                    .p_4()
                    .gap_1()
                    .child(div().text_sm().font_weight(FontWeight::BOLD).child("Failed to load settings"))
                    .child(div().text_sm().opacity(0.6).child(reason.clone()))
                    .into_any_element();
            }
            SettingsState::Loaded(settings) => settings,
        };

        let check_for_updates = settings.check_for_updates;
        let enable_logging = settings.enable_logging;
        let data_sharing = settings.data_sharing;
        let replay = settings.replay.clone();
        let current_replay_path = settings.current_replay_path.display().to_string();

        let application = v_flex()
            .gap_2()
            .child(section_heading("Application Settings", "General application behavior and appearance"))
            .child(
                Checkbox::new("check-for-updates")
                    .label("Check for updates at startup")
                    .checked(check_for_updates)
                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                        let checked = *checked;
                        this.edit_setting(keys::CHECK_FOR_UPDATES, cx, |settings| {
                            settings.check_for_updates = checked;
                            checked
                        });
                    })),
            )
            .child(Checkbox::new("enable-logging").label("Write a log file").checked(enable_logging).on_click(
                cx.listener(|this, checked: &bool, _window, cx| {
                    let checked = *checked;
                    this.edit_setting(keys::ENABLE_LOGGING, cx, |settings| {
                        settings.enable_logging = checked;
                        checked
                    });
                }),
            ))
            .child(self.render_zoom_row(cx))
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().child("Data sharing"))
                    .child(h_flex().gap_2().children(DataSharingMode::ALL.map(|mode| {
                        selectable(
                            ("data-sharing", mode as usize),
                            data_sharing == mode,
                            Button::new(("data-sharing-button", mode as usize))
                                .label(mode.label())
                                .compact()
                                .selected(data_sharing == mode)
                                .tooltip(mode.description())
                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                    this.edit_setting(keys::DATA_SHARING_MODE, cx, |settings| {
                                        settings.data_sharing = mode;
                                        mode
                                    });
                                })),
                        )
                    })))
                    .child(div().text_xs().opacity(0.6).child(data_sharing.description())),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().child("Proxy URL"))
                    .child(Input::new(&self.proxy_input).id("proxy-url").small().w_full()),
            );

        let game = v_flex()
            .gap_2()
            .child(section_heading("World of Warships Settings", "Path to your World of Warships installation"))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().child(Input::new(&self.wows_dir_input).id("wows-dir").small().w_full()))
                    .child(
                        Button::new("wows-dir-browse")
                            .icon(IconName::FolderOpen)
                            .label("Browse...")
                            .compact()
                            .on_click(cx.listener(|this, _event, window, cx| this.browse_for_wows_dir(window, cx))),
                    ),
            );

        let replay_section = v_flex()
            .gap_2()
            .child(section_heading("Replay Settings", "Which columns appear in the replay results table"))
            .child(settings_row("Current Replay Path", current_replay_path))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_4()
                    .child(Checkbox::new("show-raw-xp").label("Show Raw XP").checked(replay.show_raw_xp).on_click(
                        cx.listener(|this, checked: &bool, _window, cx| {
                            let checked = *checked;
                            this.edit_replay_settings(cx, |replay| replay.show_raw_xp = checked);
                        }),
                    ))
                    .child(
                        Checkbox::new("show-observed-damage")
                            .label("Show Observed Damage")
                            .checked(replay.show_observed_damage)
                            .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                let checked = *checked;
                                this.edit_replay_settings(cx, |replay| replay.show_observed_damage = checked);
                            })),
                    )
                    .child(Checkbox::new("show-heals").label("Show Heals").checked(replay.show_heals).on_click(
                        cx.listener(|this, checked: &bool, _window, cx| {
                            let checked = *checked;
                            this.edit_replay_settings(cx, |replay| replay.show_heals = checked);
                        }),
                    ))
                    .child(
                        Checkbox::new("enable-replay-previews")
                            .label("Hover a replay to preview it")
                            .checked(replay.enable_replay_previews)
                            .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                let checked = *checked;
                                this.edit_replay_settings(cx, |replay| replay.enable_replay_previews = checked);
                            })),
                    ),
            );

        // The viewport writes these itself when a pane changes, so the tab
        // reports them rather than offering a second way to set them.
        let armor = v_flex()
            .gap_2()
            .child(section_heading("Armor Viewer Defaults", "Saved defaults for the armor viewport"))
            .child(match &settings.armor_defaults {
                Some(defaults) => h_flex()
                    .gap_4()
                    .child(
                        Checkbox::new("armor-show-plate-edges")
                            .label("Show Plate Edges")
                            .checked(defaults.show_plate_edges)
                            .disabled(true),
                    )
                    .child(
                        Checkbox::new("armor-show-waterline")
                            .label("Show Waterline")
                            .checked(defaults.show_waterline)
                            .disabled(true),
                    )
                    .child(
                        Checkbox::new("armor-hull-opaque")
                            .label("Hull Opaque")
                            .checked(defaults.hull_opaque)
                            .disabled(true),
                    )
                    .into_any_element(),
                None => div().text_sm().opacity(0.6).child("(no saved defaults)").into_any_element(),
            });

        div()
            .id("settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .child(v_flex().gap_4().p_4().child(application).child(game).child(replay_section).child(armor))
            .into_any_element()
    }
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Pick up drag changes made directly on the zoom slider (the reset
        // button applies the theme itself, so this only fires for drags).
        let slider_zoom = self.zoom_slider.read(cx).value().start();
        if (slider_zoom - self.zoom).abs() > f32::EPSILON {
            self.zoom = slider_zoom;
            theme::apply_egui_dark_theme(self.zoom, window, cx);
        }

        let active_ix = AppTab::ALL.iter().position(|t| *t == self.active_tab).unwrap_or(0);
        let tabs = TabBar::new("app-tabs")
            .selected_index(active_ix)
            .children(AppTab::ALL.iter().map(|t| Tab::new().label(t.label())))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.active_tab = AppTab::ALL[*ix];
                this.poll_armor_game_data(cx);
                cx.notify();
            }));

        let body = match self.active_tab {
            AppTab::Settings => self.render_settings_tab(cx).into_any_element(),
            AppTab::ReplayInspector => self.replay_inspector.clone().into_any_element(),
            AppTab::ArmorViewer => self.armor_pane.clone().into_any_element(),
            AppTab::Stats => self.stats.clone().into_any_element(),
            AppTab::PlayerTracker => self.player_tracker.clone().into_any_element(),
            AppTab::Search => self.search.clone().into_any_element(),
            AppTab::Unpacker => self.unpacker.clone().into_any_element(),
        };

        // Reproduces the egui app's `app.rs:720-722` bottom-panel notice: a
        // small warn-colored strip flanked by a warning-triangle icon, shown
        // only while `debug_mode` is on. The egui version renders one
        // leading and one trailing "warning" glyph around the text; this
        // mirrors that with rendered `IconName::TriangleAlert` glyphs
        // instead of a unicode character in source.
        let warning_color = cx.theme().warning;
        let debug_notice = h_flex()
            .id("app-debug-notice")
            .test_support()
            .flex_none()
            .gap_1()
            .items_center()
            .justify_center()
            .px_2()
            .py_1()
            .child(Icon::new(IconName::TriangleAlert).text_color(warning_color))
            .child(div().text_sm().font_weight(FontWeight::BOLD).text_color(warning_color).child("Debug build"))
            .child(Icon::new(IconName::TriangleAlert).text_color(warning_color));

        v_flex()
            .id("app-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let modifiers = event.keystroke.modifiers;
                if !event.is_held && modifiers.control && modifiers.shift && event.keystroke.key == "d" {
                    this.toggle_debug_mode(cx);
                }
            }))
            .child(tabs)
            .child(div().flex_1().child(body))
            .when(self.debug_mode, |this| this.child(debug_notice))
    }
}
