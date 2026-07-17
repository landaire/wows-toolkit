//! Top-level Replay Inspector tab: the file browser (`browser_view.rs`) in a
//! resizable sidebar next to a `DockArea` holding one `ReplayPanel` tab per
//! open replay. Double-clicking a replay in the browser
//! (`ReplayBrowserEvent::OpenReplay`) starts a background parse and adds a
//! tab; the tab itself shows "Loading..." until the parse completes (see
//! `panel.rs`). A repeat double-click on an already-open replay is a no-op
//! rather than adding a duplicate tab (see `open_replay`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::dock::DockArea;
use gpui_component::dock::DockItem;
use gpui_component::dock::DockPlacement;
use gpui_component::h_flex;
use gpui_component::resizable::h_resizable;
use gpui_component::resizable::resizable_panel;
use gpui_component::v_flex;

use super::browser_view::ReplayBrowser;
use super::browser_view::ReplayBrowserEvent;
use super::load::GameDataCache;
use super::load::GameDataStatus;
use super::load::spawn_startup_preload;
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
    /// Startup preload of the current installed build's game data (see
    /// `load::spawn_startup_preload`), kicked off from `apply_settings` once
    /// the WoWs directory is known. `Loading` before that (including before
    /// settings arrive at all); a later replay open still works while this
    /// is `Loading` or `Failed` -- `spawn_parse` loads its own build on
    /// demand either way -- this only lets an already-warm build skip the
    /// wait.
    game_data_status: GameDataStatus,
    /// Set once the first replay is opened. Approximates "the dock has at
    /// least one tab" for the empty-state message without reaching into
    /// `DockArea`'s private layout fields; it does not clear if every tab is
    /// later closed, so the empty-state message can under-fire in that edge
    /// case. Acceptable for this milestone: closing tabs back to zero and
    /// re-showing the placeholder is not part of the brief.
    has_opened_replay: bool,
    /// Live replay panels keyed by the path they were opened from, so a
    /// repeat open on an already-open replay can be deduped instead of
    /// adding a second tab for it. Entries survive their panel's tab being
    /// closed until the next open for that same path notices the weak
    /// handle no longer upgrades and replaces the entry.
    open_panels: HashMap<PathBuf, WeakEntity<ReplayPanel>>,
    _subscriptions: Vec<Subscription>,
}

impl ReplayInspectorView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let browser = cx.new(ReplayBrowser::new);
        let dock_area = cx.new(|cx| DockArea::new("replay-inspector-dock", None, window, cx));
        let subscription = cx.subscribe_in(&browser, window, Self::on_browser_event);

        Self {
            browser,
            dock_area,
            game_data: None,
            game_data_status: GameDataStatus::Loading,
            has_opened_replay: false,
            open_panels: HashMap::new(),
            _subscriptions: vec![subscription],
        }
    }

    /// Starts the browser's directory scan, (re)builds the game-data cache
    /// for `wows_dir`, and kicks off the startup preload of the current
    /// installed build through that same cache -- so a later `spawn_parse`
    /// for a replay on that build (see `panel.rs`) finds the slot already
    /// warm instead of reloading it. Called from `App::apply_settings`,
    /// which itself runs without a `Window` (see `main.rs`), so this cannot
    /// take one either.
    pub fn apply_settings(&mut self, wows_dir: String, cx: &mut Context<Self>) {
        if wows_dir.is_empty() {
            self.game_data = None;
            self.game_data_status = GameDataStatus::Failed("World of Warships directory is not set".to_string());
            let status = self.game_data_status.clone();
            self.browser.update(cx, |browser, cx| {
                browser.start_scan(wows_dir, cx);
                browser.set_game_data(&status, cx);
            });
            return;
        }

        let game_data = GameDataCache::new(PathBuf::from(&wows_dir));
        self.game_data = Some(game_data.clone());
        self.game_data_status = GameDataStatus::Loading;
        let status = self.game_data_status.clone();
        self.browser.update(cx, |browser, cx| {
            browser.start_scan(wows_dir.clone(), cx);
            browser.set_game_data(&status, cx);
        });

        let preload = spawn_startup_preload(PathBuf::from(&wows_dir), game_data, cx);
        cx.spawn(async move |this, cx| {
            let status = preload.await;
            let _ = this.update(cx, |this, cx| {
                this.game_data_status = status.clone();
                let browser = this.browser.clone();
                browser.update(cx, |browser, cx| browser.set_game_data(&status, cx));
                cx.notify();
            });
        })
        .detach();
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

    /// Opens `path` in a dock tab. A repeat double-click on a replay that is
    /// already open is a no-op rather than adding a second tab for it:
    /// `open_panels` tracks the live panel entity per path, checked here
    /// before creating a new one.
    ///
    /// This does not re-focus the existing tab on a repeat open --
    /// gpui-component's `TabPanel::add_panel` only dedups new-panel adds by
    /// entity id (so re-adding the same entity is already a no-op) and
    /// exposes no public API to change which tab is active from outside the
    /// crate (`TabPanel::set_active_ix` is private). Skipping the duplicate
    /// is the "do not over-engineer" fallback the milestone brief calls for.
    fn open_replay(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else {
            tracing::warn!(
                path = %path.display(),
                "replay inspector: open requested before the WoWs directory was known"
            );
            return;
        };

        if let Some(existing) = self.open_panels.get(&path)
            && existing.upgrade().is_some()
        {
            return;
        }

        let panel = cx.new(|cx| ReplayPanel::new(path.clone(), game_data, cx));
        self.open_panels.insert(path, panel.downgrade());
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

        let status_banner = match &self.game_data_status {
            GameDataStatus::Loading => {
                Some(div().text_xs().opacity(0.6).child("Loading game data...").into_any_element())
            }
            GameDataStatus::Failed(reason) => Some(
                div().text_xs().opacity(0.6).child(format!("Game data failed to load: {reason}")).into_any_element(),
            ),
            GameDataStatus::Ready(_) => None,
        };

        v_flex()
            .size_full()
            .when_some(status_banner, |this, banner| this.child(h_flex().flex_none().px_2().py_1().child(banner)))
            .child(
                div().flex_1().min_h(px(0.)).child(
                    h_resizable("replay-inspector-split")
                        .child(
                            resizable_panel()
                                .size(BROWSER_WIDTH)
                                .size_range(BROWSER_MIN_WIDTH..BROWSER_MAX_WIDTH)
                                .flex_none()
                                .child(self.browser.clone()),
                        )
                        .child(resizable_panel().child(dock_content)),
                ),
            )
    }
}
