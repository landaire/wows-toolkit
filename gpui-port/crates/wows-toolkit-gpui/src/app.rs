use gpui::*;
use gpui_component::Disableable;
use gpui_component::checkbox::Checkbox;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{h_flex, v_flex};

use crate::settings::GpuiSettings;

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

pub struct App {
    active_tab: AppTab,
    /// `None` until the async DB load (see `main.rs`) completes.
    settings: Option<GpuiSettings>,
}

impl App {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { active_tab: AppTab::Settings, settings: None }
    }

    /// Store the settings snapshot loaded from the shared config DB. Called
    /// once from `main.rs` after the async load completes; read-only for the
    /// lifetime of the session (no write-back).
    pub fn apply_settings(&mut self, settings: GpuiSettings) {
        self.settings = Some(settings);
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
    fn render_settings_tab(&self) -> impl IntoElement {
        let Some(settings) = &self.settings else {
            return v_flex().p_4().child(div().text_sm().opacity(0.6).child("Loading settings...")).into_any_element();
        };

        v_flex()
            .size_full()
            .gap_4()
            .p_4()
            .child(
                v_flex()
                    .gap_2()
                    .child(section_heading("Application Settings", "General application behavior and appearance"))
                    .child(settings_row(
                        "Zoom Factor (Ctrl + and Ctrl - also changes this)",
                        format!("{:.2}", settings.zoom),
                    )),
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_ix = AppTab::ALL.iter().position(|t| *t == self.active_tab).unwrap_or(0);
        let tabs = TabBar::new("app-tabs")
            .selected_index(active_ix)
            .children(AppTab::ALL.iter().map(|t| Tab::new().label(t.label())))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.active_tab = AppTab::ALL[*ix];
                cx.notify();
            }));

        let body = match self.active_tab {
            AppTab::Settings => self.render_settings_tab().into_any_element(),
            AppTab::ReplayInspector | AppTab::ArmorViewer => {
                h_flex().size_full().items_center().justify_center().child(self.active_tab.label()).into_any_element()
            }
        };

        v_flex().size_full().child(tabs).child(body)
    }
}
