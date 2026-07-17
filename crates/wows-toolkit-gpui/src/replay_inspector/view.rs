//! Top-level Replay Inspector tab: the file browser (`browser_view.rs`) in a
//! resizable sidebar next to a `DockArea` holding one `ReplayPanel` tab per
//! open replay. Double-clicking a replay in the browser
//! (`ReplayBrowserEvent::OpenReplay`) starts a background parse and adds a
//! tab; the tab itself shows "Loading..." until the parse completes (see
//! `panel.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;
use gpui_component::dock::DockArea;
use gpui_component::dock::DockItem;
use gpui_component::dock::DockPlacement;
use gpui_component::resizable::h_resizable;
use gpui_component::resizable::resizable_panel;
use gpui_component::v_flex;

use super::browser_view::ReplayBrowser;
use super::browser_view::ReplayBrowserEvent;
use super::load::GameDataCache;
use super::panel::ReplayPanel;

/// Sidebar width for the file browser, matching the egui app's left panel.
const BROWSER_WIDTH: Pixels = px(280.);
const BROWSER_MIN_WIDTH: Pixels = px(180.);
const BROWSER_MAX_WIDTH: Pixels = px(520.);

pub struct ReplayInspectorView {
    browser: Entity<ReplayBrowser>,
    dock_area: Entity<DockArea>,
    /// `None` until `apply_settings` learns the WoWs directory; opening a
    /// replay before then is a no-op (the browser has nothing to
    /// double-click yet either, since its scan needs the same directory).
    game_data: Option<GameDataCache>,
    /// Set once the first replay is opened. Approximates "the dock has at
    /// least one tab" for the empty-state message without reaching into
    /// `DockArea`'s private layout fields; it does not clear if every tab is
    /// later closed, so the empty-state message can under-fire in that edge
    /// case. Acceptable for this milestone: closing tabs back to zero and
    /// re-showing the placeholder is not part of the brief.
    has_opened_replay: bool,
    _subscriptions: Vec<Subscription>,
}

impl ReplayInspectorView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let browser = cx.new(ReplayBrowser::new);
        let dock_area = cx.new(|cx| DockArea::new("replay-inspector-dock", None, window, cx));
        let subscription = cx.subscribe_in(&browser, window, Self::on_browser_event);

        Self { browser, dock_area, game_data: None, has_opened_replay: false, _subscriptions: vec![subscription] }
    }

    /// Starts the browser's directory scan and (re)builds the game-data
    /// cache for `wows_dir`. Called from `App::apply_settings`, which itself
    /// runs without a `Window` (see `main.rs`), so this cannot take one
    /// either.
    pub fn apply_settings(&mut self, wows_dir: String, cx: &mut Context<Self>) {
        self.game_data = if wows_dir.is_empty() { None } else { Some(GameDataCache::new(PathBuf::from(&wows_dir))) };
        self.browser.update(cx, |browser, cx| browser.start_scan(wows_dir, cx));
    }

    fn on_browser_event(
        &mut self,
        _browser: &Entity<ReplayBrowser>,
        event: &ReplayBrowserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ReplayBrowserEvent::OpenReplay(path) = event;
        self.open_replay(path.clone(), window, cx);
    }

    /// Opens `path` in a new dock tab. Every open, including a repeat
    /// double-click on the same replay, adds another tab: focusing an
    /// already-open replay's existing tab needs per-path panel identity
    /// tracking this milestone does not add, so always-add is the simple,
    /// correct-but-not-deduplicated behavior.
    fn open_replay(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else {
            tracing::warn!(
                path = %path.display(),
                "replay inspector: open requested before the WoWs directory was known"
            );
            return;
        };

        let panel = cx.new(|cx| ReplayPanel::new(path, game_data, cx));
        self.dock_area.update(cx, |dock_area, cx| {
            dock_area.add_panel(Arc::new(panel), DockPlacement::Center, None, window, cx);
        });
        self.has_opened_replay = true;
        cx.notify();
    }
}

impl Render for ReplayInspectorView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let dock_content: AnyElement = if self.has_opened_replay {
            self.dock_area.clone().into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().opacity(0.6).child("Select a replay"))
                .into_any_element()
        };

        h_resizable("replay-inspector-split")
            .child(
                resizable_panel()
                    .size(BROWSER_WIDTH)
                    .size_range(BROWSER_MIN_WIDTH..BROWSER_MAX_WIDTH)
                    .flex_none()
                    .child(self.browser.clone()),
            )
            .child(resizable_panel().child(dock_content))
    }
}
