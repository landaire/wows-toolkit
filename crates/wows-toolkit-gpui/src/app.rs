use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::Disableable;
use gpui_component::Icon;
use gpui_component::IconName;
use gpui_component::button::Button;
use gpui_component::checkbox::Checkbox;
use gpui_component::slider::{Slider, SliderState};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{h_flex, v_flex};

use crate::replay_inspector::ReplayInspectorView;
use crate::settings::{DEFAULT_ZOOM, GpuiSettings, MAX_ZOOM, MIN_ZOOM};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AppTab {
    Settings,
    ReplayInspector,
    ArmorViewer,
}

impl AppTab {
    pub const ALL: [AppTab; 3] = [AppTab::Settings, AppTab::ReplayInspector, AppTab::ArmorViewer];

    pub fn label(self) -> &'static str {
        match self {
            AppTab::Settings => "Settings",
            AppTab::ReplayInspector => "Replay Inspector",
            AppTab::ArmorViewer => "Armor Viewer",
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
}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let zoom_slider =
            cx.new(|_| SliderState::new().min(MIN_ZOOM).max(MAX_ZOOM).step(0.05).default_value(DEFAULT_ZOOM));
        let replay_inspector = cx.new(|cx| ReplayInspectorView::new(window, cx));
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            active_tab: AppTab::Settings,
            settings: SettingsState::Loading,
            zoom: DEFAULT_ZOOM,
            zoom_slider,
            replay_inspector,
            debug_mode: false,
            focus_handle,
        }
    }

    /// Store the settings snapshot loaded from the shared config DB. Called
    /// once from `main.rs` after the async load completes; read-only for the
    /// lifetime of the session (no write-back). Also kicks off the replay
    /// inspector's background directory scan and game-data cache, now that
    /// the WoWs directory is known.
    pub fn apply_settings(&mut self, settings: GpuiSettings, cx: &mut Context<Self>) {
        self.zoom = settings.zoom;
        self.zoom_slider =
            cx.new(|_| SliderState::new().min(MIN_ZOOM).max(MAX_ZOOM).step(0.05).default_value(settings.zoom));
        let wows_dir = settings.wows_dir.clone();
        let debug_mode = settings.debug_mode;
        let replay_settings = settings.replay.clone();
        let auto_load_latest_replay = settings.auto_load_latest_replay;
        self.debug_mode = debug_mode;
        self.replay_inspector.update(cx, |view, cx| {
            view.apply_settings(wows_dir, debug_mode, replay_settings, auto_load_latest_replay, cx)
        });
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

    fn render_settings_tab(&self, cx: &Context<Self>) -> impl IntoElement {
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

        v_flex()
            .size_full()
            .gap_4()
            .p_4()
            .child(
                v_flex()
                    .gap_2()
                    .child(section_heading("Application Settings", "General application behavior and appearance"))
                    .child(self.render_zoom_row(cx)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_heading("World of Warships Settings", "Path to your World of Warships installation"))
                    .child(settings_row("World of Warships Directory", settings.wows_dir.clone())),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_heading(
                        "Replay Settings",
                        "Configure which columns appear in the replay results table",
                    ))
                    .child(settings_row("Current Replay Path", settings.current_replay_path.display().to_string()))
                    .child(
                        h_flex()
                            .gap_4()
                            .child(
                                Checkbox::new("show-raw-xp")
                                    .label("Show Raw XP")
                                    .checked(settings.replay.show_raw_xp)
                                    .disabled(true),
                            )
                            .child(
                                Checkbox::new("show-entity-id")
                                    .label("Show Entity ID")
                                    .checked(settings.replay.show_entity_id)
                                    .disabled(true),
                            )
                            .child(
                                Checkbox::new("show-observed-damage")
                                    .label("Show Observed Damage")
                                    .checked(settings.replay.show_observed_damage)
                                    .disabled(true),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_heading("Armor Viewer Defaults", "Saved defaults for the armor viewport"))
                    .child(match &settings.armor_defaults {
                        Some(d) => h_flex()
                            .gap_4()
                            .child(
                                Checkbox::new("armor-show-plate-edges")
                                    .label("Show Plate Edges")
                                    .checked(d.show_plate_edges)
                                    .disabled(true),
                            )
                            .child(
                                Checkbox::new("armor-show-waterline")
                                    .label("Show Waterline")
                                    .checked(d.show_waterline)
                                    .disabled(true),
                            )
                            .child(
                                Checkbox::new("armor-hull-opaque")
                                    .label("Hull Opaque")
                                    .checked(d.hull_opaque)
                                    .disabled(true),
                            )
                            .into_any_element(),
                        None => div().text_sm().opacity(0.6).child("(no saved defaults)").into_any_element(),
                    }),
            )
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
                cx.notify();
            }));

        let body = match self.active_tab {
            AppTab::Settings => self.render_settings_tab(cx).into_any_element(),
            AppTab::ReplayInspector => self.replay_inspector.clone().into_any_element(),
            AppTab::ArmorViewer => {
                h_flex().size_full().items_center().justify_center().child(self.active_tab.label()).into_any_element()
            }
        };

        // Reproduces the egui app's `app.rs:720-722` bottom-panel notice: a
        // small warn-colored strip flanked by a warning-triangle icon, shown
        // only while `debug_mode` is on. The egui version renders one
        // leading and one trailing "warning" glyph around the text; this
        // mirrors that with rendered `IconName::TriangleAlert` glyphs
        // instead of a unicode character in source.
        let warning_color = cx.theme().warning;
        let debug_notice = h_flex()
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
