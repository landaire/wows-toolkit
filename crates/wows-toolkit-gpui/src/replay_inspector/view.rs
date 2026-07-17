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
use gpui_component::checkbox::Checkbox;
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
    /// Session debug-mode flag: seeded from `AppPreferences.debug_mode` (the
    /// shared config DB) in `apply_settings`, then flippable at runtime via
    /// the header checkbox (`set_debug_mode`), which also pushes the new
    /// value into every currently open `ReplayPanel` -- not just panels
    /// opened afterward. This crate never writes settings back to the DB
    /// (see `settings.rs`'s module doc), so the toggle only overrides the
    /// setting for the running session.
    debug_mode: bool,
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
            debug_mode: false,
            _subscriptions: vec![subscription],
        }
    }

    /// Starts the browser's directory scan, (re)builds the game-data cache
    /// for `wows_dir`, seeds the session debug-mode flag from
    /// `debug_mode` (`AppPreferences.debug_mode`), and kicks off the startup
    /// preload of the current installed build through that same cache -- so
    /// a later `spawn_parse` for a replay on that build (see `panel.rs`)
    /// finds the slot already warm instead of reloading it. Called from
    /// `App::apply_settings`, which itself runs without a `Window` (see
    /// `main.rs`), so this cannot take one either.
    pub fn apply_settings(&mut self, wows_dir: String, debug_mode: bool, cx: &mut Context<Self>) {
        self.debug_mode = debug_mode;
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

        let panel = cx.new(|cx| ReplayPanel::new(path.clone(), game_data, self.debug_mode, cx));
        self.open_panels.insert(path, panel.downgrade());
        self.dock_area.update(cx, |dock_area, cx| {
            dock_area.add_panel(Arc::new(panel), DockPlacement::Center, None, window, cx);
        });
        self.has_opened_replay = true;
        cx.notify();
    }

    /// Flips the session debug-mode flag and pushes the new value into every
    /// currently open replay tab (`open_panels`'s live entries; closed tabs'
    /// stale weak handles just fail to upgrade and are skipped), so toggling
    /// debug mode takes effect immediately rather than only on the next
    /// replay opened.
    fn set_debug_mode(&mut self, debug_mode: bool, cx: &mut Context<Self>) {
        self.debug_mode = debug_mode;
        for panel in self.open_panels.values() {
            if let Some(panel) = panel.upgrade() {
                panel.update(cx, |panel, cx| panel.set_debug(debug_mode, cx));
            }
        }
        cx.notify();
    }
}

impl Render for ReplayInspectorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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

        // Mirrors `AppPreferences.debug_mode`: unhides NDA-hidden stats
        // (threaded into every open tab's table) and reveals the per-replay
        // raw-metadata/raw-results viewers. Session-only override; see
        // `debug_mode`'s doc comment.
        let debug_toggle = h_flex().flex_none().px_2().py_1().items_center().child(
            Checkbox::new("replay-inspector-debug-toggle")
                .label("Debug Mode")
                .checked(self.debug_mode)
                .on_click(cx.listener(|this, checked: &bool, _window, cx| this.set_debug_mode(*checked, cx))),
        );

        v_flex()
            .size_full()
            .child(debug_toggle)
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
