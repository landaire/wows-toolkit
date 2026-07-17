use gpui::*;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{h_flex, v_flex};

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
}

impl App {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { active_tab: AppTab::Settings }
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

        v_flex()
            .size_full()
            .child(tabs)
            .child(
                h_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .child(self.active_tab.label()),
            )
    }
}
