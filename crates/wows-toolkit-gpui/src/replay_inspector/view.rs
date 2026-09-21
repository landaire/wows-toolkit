//! Top-level Replay Inspector tab: the file browser (`browser_view.rs`) in a
//! resizable sidebar next to a `DockArea` holding one `ReplayPanel` tab per
//! open replay. Double-clicking a replay in the browser
//! (`ReplayBrowserEvent::OpenReplay`) starts a background parse and adds a
//! tab; the tab itself shows "Loading..." until the parse completes (see
//! `panel.rs`). A repeat double-click on an already-open replay brings its
//! tab forward rather than adding a second one (see `open_replay`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::DockPlacement;
use gpui_kit::component::dock::DockSkin;
use gpui_kit::component::dock::PanelId;
use gpui_kit::component::dock::panel_handle;
use gpui_kit::component::h_flex;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::resizable::h_resizable;
use gpui_kit::component::resizable::resizable_panel;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::Select;
use gpui_kit::component::select::SelectEvent;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;

use super::browser_view::ReplayBrowser;
use super::browser_view::ReplayBrowserEvent;
use super::columns::default_columns;
use super::load::GameDataCache;
use super::load::GameDataStatus;
use super::load::spawn_startup_preload;
use super::panel::ReplayPanel;

/// Sidebar width for the file browser, matching the egui app's left panel.
const BROWSER_WIDTH: Pixels = px(280.);
const BROWSER_MIN_WIDTH: Pixels = px(180.);
const BROWSER_MAX_WIDTH: Pixels = px(520.);

/// One entry in the grouping combo box. A local newtype because both
/// `ReplayGrouping` and `SearchableListItem` are foreign, and because
/// carrying the enum keeps the confirm handler off a string round trip.
#[derive(Clone)]
struct GroupingItem(ReplayGrouping);

impl SearchableListItem for GroupingItem {
    type Value = ReplayGrouping;

    fn title(&self) -> SharedString {
        SharedString::from(self.0.label())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }
}

/// Combo-box order, matching the egui app's `selectable_value` order.
const GROUPINGS: [ReplayGrouping; 3] = [ReplayGrouping::Date, ReplayGrouping::Ship, ReplayGrouping::None];

fn grouping_index(grouping: ReplayGrouping) -> usize {
    GROUPINGS.iter().position(|g| *g == grouping).expect("GROUPINGS lists every ReplayGrouping variant")
}

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
    /// The expected-values table every replay tab rates its players against,
    /// loaded once per session beside the Stats tab's copy (`App::
    /// apply_session_stats`). Held here rather than fetched per tab so a
    /// replay opened before it arrives can still be filled in afterward by
    /// `set_personal_rating`.
    personal_rating: Option<Arc<PersonalRatingData>>,
    /// Session debug-mode flag: seeded from `AppPreferences.debug_mode` (the
    /// shared config DB) in `apply_settings`, then flippable at runtime via
    /// `App`'s global Ctrl+Shift+D shortcut (`set_debug_mode`, called from
    /// `app.rs`), which also pushes the new value into every currently open
    /// `ReplayPanel` -- not just panels opened afterward. This crate never
    /// writes settings back to the DB (see `settings.rs`'s module doc), so
    /// the toggle only overrides the setting for the running session.
    debug_mode: bool,
    /// Session-local copy of the persisted `ReplaySettings`, seeded from the
    /// shared config DB in `apply_settings`. The header toolbar's
    /// column-filter checkboxes read/write this and drive `default_columns`
    /// off it (`set_column_filter`); its `grouping` field is not consulted
    /// here after the initial seed -- the live grouping selection lives on
    /// `browser` (see `set_grouping`). Like `debug_mode`, this crate never
    /// writes it back to the DB (see `settings.rs`'s module doc).
    replay_settings: ReplaySettings,
    /// `AppPreferences.auto_load_latest_replay` in the egui app: seeded from
    /// the shared config DB in `apply_settings`, then flippable at runtime via
    /// the header checkbox. Reflects the persisted intent only -- this port
    /// has no replays-directory watcher yet, so toggling it does not
    /// currently start or stop an auto-load; wiring that up is a follow-up.
    auto_load_latest_replay: bool,
    /// Backing state for the header's grouping combo box. The live grouping
    /// lives on `browser`; this mirrors it so the closed combo shows the
    /// current value.
    grouping_select: Entity<SelectState<SearchableVec<GroupingItem>>>,
    _subscriptions: Vec<Subscription>,
}

/// What the replay inspector takes from the app's settings.
pub struct InspectorSettings {
    pub wows_dir: String,
    pub debug_mode: bool,
    pub replay_settings: ReplaySettings,
    pub auto_load_latest_replay: bool,
    /// The locale the listing's figures are grouped in.
    pub locale: Option<String>,
}

impl ReplayInspectorView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let browser = cx.new(ReplayBrowser::new);
        // The skinned area is what draws a tab bar over a group holding more
        // than one panel; a bare one renders only the displayed panel.
        let (dock_area, _) = DockSkin::dock_area("replay-inspector-dock", None, window, cx);
        let subscription = cx.subscribe_in(&browser, window, Self::on_browser_event);

        let items = SearchableVec::new(GROUPINGS.map(GroupingItem).to_vec());
        let grouping_select = cx.new(|cx| {
            SelectState::new(items, Some(IndexPath::new(grouping_index(ReplayGrouping::default()))), window, cx)
                .searchable(false)
        });
        // `Confirm(None)` is the cleared-selection case, which this combo
        // cannot produce: it is not `.cleanable()` and always holds a value.
        let grouping_subscription = cx.subscribe_in(&grouping_select, window, |this, _state, event, window, cx| {
            let SelectEvent::Confirm(Some(grouping)) = event else {
                return;
            };
            this.set_grouping(*grouping, window, cx);
        });

        Self {
            browser,
            dock_area,
            game_data: None,
            game_data_status: GameDataStatus::Loading,
            has_opened_replay: false,
            open_panels: HashMap::new(),
            personal_rating: None,
            debug_mode: false,
            replay_settings: ReplaySettings::default(),
            auto_load_latest_replay: true,
            grouping_select,
            _subscriptions: vec![subscription, grouping_subscription],
        }
    }

    /// Starts the browser's directory scan, (re)builds the game-data cache
    /// for `wows_dir`, seeds the session debug-mode flag from
    /// `debug_mode` (`AppPreferences.debug_mode`), seeds the session
    /// `replay_settings`/`auto_load_latest_replay` flags and the browser's
    /// initial grouping (`replay_settings.grouping`), and kicks off the
    /// startup preload of the current installed build through that same cache
    /// -- so a later `spawn_parse` for a replay on that build (see
    /// `panel.rs`) finds the slot already warm instead of reloading it.
    /// Adopts a language without touching anything else.
    ///
    /// Separate from `apply_settings`: that one rebuilds the game-data cache
    /// and rescans the directory, which a language change has no reason to
    /// do, and which would strand every open panel on the discarded cache.
    pub fn set_locale(&mut self, locale: Option<String>, cx: &mut Context<Self>) {
        self.browser.update(cx, |browser, cx| browser.set_locale(locale, cx));
        cx.notify();
    }

    /// Called from `App::apply_settings`, which `main.rs` runs inside a
    /// `window.update`, so a `Window` is available for the grouping combo.
    pub fn apply_settings(&mut self, settings: InspectorSettings, window: &mut Window, cx: &mut Context<Self>) {
        let InspectorSettings { wows_dir, debug_mode, replay_settings, auto_load_latest_replay, locale } = settings;
        self.browser.update(cx, |browser, cx| {
            browser.set_locale(locale, cx);
            // The listing's second line is what the index knows about each
            // file, so it is read once the directory is known.
            browser.load_summaries(cx);
        });
        self.debug_mode = debug_mode;
        self.auto_load_latest_replay = auto_load_latest_replay;
        let grouping = replay_settings.grouping;
        self.replay_settings = replay_settings;
        self.set_grouping(grouping, window, cx);

        if wows_dir.is_empty() {
            self.game_data = None;
            self.game_data_status = GameDataStatus::Failed("World of Warships directory is not set".to_string());
            let status = self.game_data_status.clone();
            self.browser.update(cx, |browser, cx| {
                browser.start_scan(wows_dir, cx);
                browser.set_game_data(&status, cx);
                browser.set_build_cache(None);
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
            // The listing's hover previews bake against whichever build a
            // replay was recorded on, which is what this cache loads.
            browser.set_build_cache(Some(game_data.clone()));
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

    /// The current game-data preload status. Read by `App` so the Armor
    /// Viewer tab can adopt the SAME `Arc<LoadedGameData>` once it reaches
    /// `Ready` -- see `armor_viewer::pane::ArmorViewerPane::load_game_data` --
    /// rather than running a second `GameDataCache`/VFS/`GameParams` load for
    /// the same build.
    pub fn game_data_status(&self) -> GameDataStatus {
        self.game_data_status.clone()
    }

    fn on_browser_event(
        &mut self,
        _browser: &Entity<ReplayBrowser>,
        event: &ReplayBrowserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ReplayBrowserEvent::OpenReplay(path) => self.open_replay(path.clone(), window, cx),
            // The game has just finished a match. The egui app opens it
            // straight away when this is on, which is what the checkbox
            // promises.
            ReplayBrowserEvent::ReplayAppeared(path) => {
                if self.auto_load_latest_replay {
                    self.open_replay(path.clone(), window, cx);
                }
            }
        }
    }

    /// Opens `path` in a dock tab. A repeat double-click on a replay that is
    /// already open is a no-op rather than adding a second tab for it:
    /// `open_panels` tracks the live panel entity per path, checked here
    /// before creating a new one.
    /// A repeat open brings that tab forward (`DockArea::select_panel`) rather
    /// than leaving the reader on whichever tab was in front.
    ///
    /// `pub(crate)` so another tab can send a replay here: the Search tab's
    /// results open in this inspector rather than in one of their own.
    pub(crate) fn open_replay(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else {
            tracing::warn!(
                path = %path.display(),
                "replay inspector: open requested before the WoWs directory was known"
            );
            return;
        };

        // A replay that is already open is shown, not opened twice: the tab
        // it is in comes forward, which is what the egui listing does with a
        // replay it has already hydrated.
        if let Some(existing) = self.open_panels.get(&path).and_then(|panel| panel.upgrade()) {
            let id = PanelId::from(existing.entity_id());
            self.dock_area.update(cx, |dock_area, cx| dock_area.select_panel(id, window, cx));
            cx.notify();
            return;
        }

        let columns = default_columns(&self.replay_settings);
        let personal_rating = self.personal_rating.clone();
        let panel =
            cx.new(|cx| ReplayPanel::new(path.clone(), game_data, self.debug_mode, columns, personal_rating, cx));
        self.open_panels.insert(path, panel.downgrade());
        self.dock_area.update(cx, |dock_area, cx| {
            dock_area.add_panel_view(panel_handle(panel), DockPlacement::Center, None, window, cx);
        });
        self.has_opened_replay = true;
        cx.notify();
    }

    /// The expected-values table this tab rates replays against. Test-only:
    /// production code reaches the field directly.
    #[cfg(test)]
    pub(crate) fn personal_rating(&self) -> Option<&Arc<PersonalRatingData>> {
        self.personal_rating.as_ref()
    }

    /// The per-build game-data cache this tab loads replays through, once
    /// the WoWs directory is known. Shared rather than cloned fresh so a
    /// build another tab needs is loaded once for the whole app.
    pub(crate) fn game_data(&self) -> Option<GameDataCache> {
        self.game_data.clone()
    }

    /// Adopts the session's expected-values table and pushes it into every
    /// open replay tab, so a replay opened before the table loaded gets its
    /// Personal Rating column filled in rather than staying empty until it is
    /// reopened. Called from `App::apply_session_stats`.
    ///
    /// Takes the table itself, not an option: a caller that has none must not
    /// be able to clear one already in hand, which would leave later tabs
    /// unrated while the tabs open at the time kept their ratings.
    pub(crate) fn set_personal_rating(&mut self, table: Arc<PersonalRatingData>, cx: &mut Context<Self>) {
        self.personal_rating = Some(table.clone());
        for panel in self.open_panels.values() {
            if let Some(panel) = panel.upgrade() {
                panel.update(cx, |panel, cx| panel.set_personal_rating(table.clone(), cx));
            }
        }
        cx.notify();
    }

    /// Flips the session debug-mode flag and pushes the new value into every
    /// currently open replay tab (`open_panels`'s live entries; closed tabs'
    /// stale weak handles just fail to upgrade and are skipped), so toggling
    /// debug mode takes effect immediately rather than only on the next
    /// replay opened. Called from `App::toggle_debug_mode` (the app-wide
    /// Ctrl+Shift+D shortcut, `app.rs`) -- this crate has no enable UI of its
    /// own for debug mode.
    pub(crate) fn set_debug_mode(&mut self, debug_mode: bool, cx: &mut Context<Self>) {
        self.debug_mode = debug_mode;
        for panel in self.open_panels.values() {
            if let Some(panel) = panel.upgrade() {
                panel.update(cx, |panel, cx| panel.set_debug(debug_mode, cx));
            }
        }
        cx.notify();
    }

    /// "Open manually": the header toolbar's file-picker button. Mirrors the
    /// egui app's `build_replay_header` open-manually handler
    /// (`ui/replay_parser/mod.rs:3659`) exactly -- same file filter -- except
    /// the picked path opens through this port's own dock flow
    /// (`open_replay`) rather than the egui app's `parse_replay_from_path`
    /// background task. A cancelled dialog is a no-op.
    fn open_manually(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let asked = crate::dialog::pick_file(None, Some(crate::dialog::REPLAYS));
        cx.spawn(async move |this, cx| {
            let Some(file) = asked.await else { return };
            let _ = this.update_in(cx, |this, window, cx| this.open_replay(file, window, cx));
        })
        .detach();
    }

    /// Flips the session "Autoload Latest Replay" flag. Reflects the
    /// persisted intent only -- see the field's own doc comment for why this
    /// does not yet start or stop an actual auto-load.
    fn set_auto_load_latest_replay(&mut self, value: bool, cx: &mut Context<Self>) {
        self.auto_load_latest_replay = value;
        cx.notify();
    }

    /// The browser's current grouping strategy. The browser is the single
    /// source of truth for it; the header combo only mirrors it.
    pub(crate) fn grouping(&self, cx: &App) -> ReplayGrouping {
        self.browser.read(cx).grouping()
    }

    /// Switches the browser's grouping strategy. The header toolbar owns this
    /// control (matching the egui app's header placement); `browser` itself
    /// just applies it and rebuilds its tree (`ReplayBrowser::set_grouping`).
    ///
    /// The only writer, so the combo mirror cannot drift from the browser: a
    /// caller that reaches the browser directly would desync the two.
    fn set_grouping(&mut self, grouping: ReplayGrouping, window: &mut Window, cx: &mut Context<Self>) {
        self.browser.update(cx, |browser, cx| browser.set_grouping(grouping, cx));
        self.grouping_select.update(cx, |state, cx| state.set_selected_value(&grouping, window, cx));
        cx.notify();
    }

    /// Applies one column-filter checkbox's change: mutates `replay_settings`
    /// via `apply`, recomputes the visible-column set
    /// (`columns::default_columns`), and pushes it into every currently open
    /// replay tab (mirroring `set_debug_mode`'s live-propagation pattern) so
    /// the table(s) update immediately rather than only on the next replay
    /// opened.
    /// Replaces the whole settings blob, for an edit made on the Settings tab
    /// rather than through this header's own checkboxes.
    pub(crate) fn set_replay_settings(&mut self, settings: ReplaySettings, cx: &mut Context<Self>) {
        self.set_column_filter(|current| *current = settings, cx);
    }

    fn set_column_filter(&mut self, apply: impl FnOnce(&mut ReplaySettings), cx: &mut Context<Self>) {
        apply(&mut self.replay_settings);
        let columns = default_columns(&self.replay_settings);
        for panel in self.open_panels.values() {
            if let Some(panel) = panel.upgrade() {
                panel.update(cx, |panel, cx| panel.set_columns(columns.clone(), cx));
            }
        }
        cx.notify();
    }
}

/// One column-filter checkbox in the header toolbar: `checked` reflects
/// `replay_settings`, clicking applies `apply` to it via `set_column_filter`
/// and recomputes the visible columns. A free function for the same reason as
/// `column_filters_popover` -- built three times, once per optional column the egui
/// app's `build_replay_header` (`ui/replay_parser/mod.rs:3697-3711`) exposes a
/// toggle for and that has a live column in this port: Raw XP, Observed
/// Damage, Heals. Received Damage and Distance Traveled are not exposed here
/// either -- the egui app never lets the user toggle those, so they stay at
/// `ReplaySettings`'s config defaults.
fn column_filter_checkbox(
    entity: Entity<ReplayInspectorView>,
    id: &'static str,
    label: String,
    checked: bool,
    apply: impl Fn(&mut ReplaySettings, bool) + Copy + 'static,
) -> impl IntoElement {
    Checkbox::new(id).label(label).checked(checked).on_click(move |checked: &bool, _window, cx: &mut App| {
        let checked = *checked;
        entity.update(cx, |view, cx| view.set_column_filter(move |settings| apply(settings, checked), cx));
    })
}

/// The column-filter dropdown, matching the egui app's "Column Filters"
/// `ComboBox` (`ui/replay_parser/mod.rs:4609`) rather than showing the
/// checkboxes inline. `settings` is a snapshot: the `.content()` closure
/// re-runs on every open-render, so it always reflects the latest state.
///
/// The egui popup carries a fourth toggle, Entity ID, which neither app has a
/// column for; it is left out here rather than drawn as a control that does
/// nothing.
fn column_filters_popover(entity: Entity<ReplayInspectorView>, settings: ReplaySettings) -> impl IntoElement {
    Popover::new("replay-header-column-filters")
        .trigger(
            Button::new("replay-header-column-filters-trigger")
                .label(t!("ui.replay.column_filters").to_string())
                .compact(),
        )
        .content(move |_state, _window, _cx| {
            v_flex()
                .gap_1()
                .p_1()
                .child(column_filter_checkbox(
                    entity.clone(),
                    "replay-header-filter-raw-xp",
                    t!("stat.raw_xp").into_owned(),
                    settings.show_raw_xp,
                    |settings, value| settings.show_raw_xp = value,
                ))
                .child(column_filter_checkbox(
                    entity.clone(),
                    "replay-header-filter-observed-damage",
                    t!("ui.replay.column.observed_damage").into_owned(),
                    settings.show_observed_damage,
                    |settings, value| settings.show_observed_damage = value,
                ))
                .child(column_filter_checkbox(
                    entity.clone(),
                    "replay-header-filter-heals",
                    t!("ui.replay.column.heals").into_owned(),
                    settings.show_heals,
                    |settings, value| settings.show_heals = value,
                ))
        })
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
                .child(
                    div()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.replay.select_replay").to_string()),
                )
                .into_any_element()
        };

        // The listing says so itself while the game data loads (it is what
        // the rows are named from), so the banner carries only the failure,
        // which nothing else reports.
        let status_banner = match &self.game_data_status {
            GameDataStatus::Failed(reason) => Some(
                div()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(t!("ui.replay.game_data_failed", error = reason).to_string())
                    .into_any_element(),
            ),
            GameDataStatus::Loading | GameDataStatus::Ready(_) => None,
        };

        let entity = cx.entity();

        // Header toolbar: mirrors the egui app's `build_replay_header`
        // (`ui/replay_parser/mod.rs:3657`) -- manual file open, autoload
        // checkbox, grouping selector, column-filter checkboxes -- in the
        // same left-to-right order. The Tactics Board/session-popover
        // controls `build_replay_header` also carries are out of scope (no
        // collab session support in this port yet).
        let replay_header = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("replay-header-open-manually")
                    .icon(IconName::FolderOpen)
                    .label(t!("ui.replay.open_manually").to_string())
                    .compact()
                    .on_click(cx.listener(|this, _event: &ClickEvent, window, cx| this.open_manually(window, cx))),
            )
            .child(
                Checkbox::new("replay-header-auto-load-latest")
                    .label(t!("ui.replay.autoload_latest").to_string())
                    .checked(self.auto_load_latest_replay)
                    .tooltip(t!("ui.replay.autoload_latest_tooltip").to_string())
                    .on_click(
                        cx.listener(|this, checked: &bool, _window, cx| this.set_auto_load_latest_replay(*checked, cx)),
                    ),
            )
            .child(crate::ui::rule_v(cx))
            .child(
                Select::new(&self.grouping_select)
                    .id("replay-header-grouping")
                    .title_prefix(t!("ui.replay.group_prefix").to_string())
                    .accessibility_label(t!("ui.replay.group_label").to_string())
                    .small()
                    .w(px(160.)),
            )
            .child(column_filters_popover(entity.clone(), self.replay_settings.clone()));

        v_flex()
            .size_full()
            .child(replay_header)
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
