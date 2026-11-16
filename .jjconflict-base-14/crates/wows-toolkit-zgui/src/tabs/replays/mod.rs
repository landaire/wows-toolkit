//! The Replays tab: the replay listing beside a dock holding a panel per opened replay, playback
//! viewports and battles a collab peer is playing back.
//!
//! [`workspace::ReplayWorkspace`] owns the dock and what each panel shows. The tab routes the
//! listing's events, `Shell::take_replay_requests` and the collab session's watch requests to it.

pub mod arrivals;
mod batch;
pub mod browser;
mod constants;
mod layout;
mod offer;
pub mod panel;
mod session_stats;
pub mod table;
#[cfg(test)]
pub mod testing;
#[cfg(test)]
mod tests;
mod title;
pub mod workspace;

use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::DockProps;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::PanelSide;
use landaire_ui::PanelWidth;
use landaire_ui::SidePanel;
use landaire_ui::SideSplitProps;
use landaire_ui::WidthRange;
use rust_i18n::t;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::columns::default_columns;
use wows_toolkit_services::constants::Cached;
use wows_toolkit_viewmodel::settings::keys;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Track;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui::vocab::PropValue;

use crate::services::GameDataState;
use crate::services::Services;
use crate::shell::ReplayRequest;
use crate::shell::Shell;
use crate::shell::tabs::PanelToggle;
use crate::shell::tabs::TabChrome;
use crate::shell::tabs::panel_side_name;
use crate::tabs::dock_menu_strings;
use crate::tabs::replays::arrivals::Arrivals;
use crate::tabs::replays::batch::BatchTarget;
use crate::tabs::replays::browser::BrowserControl;
use crate::tabs::replays::browser::ReplayBrowserEvent;
use crate::tabs::replays::browser::ReplayBrowserProps;
use crate::tabs::replays::offer::GameDataOffer;
use crate::tabs::replays::panel::PanelTable;
use crate::tabs::replays::panel::ReparseCause;
use crate::tabs::replays::panel::TableSlot;
use crate::tabs::replays::table::PlayerTableProps;
use crate::tabs::replays::workspace::OpenTarget;
use crate::tabs::replays::workspace::ReplayWorkspace;
use crate::tabs::tab_menu_strings;
use crate::windows::renderer::session::RenderRequest;

const SHEET: &str = "wt-replays-tab";

const CSS: &str = css!(
    ".wt-replays { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-replays__banner {
        flex: none;
        padding: 4px 8px;
        font-size: 12px;
        color: var(--wt-color-text-dim);
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-replays__main { position: relative; display: flex; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-replays__dock { display: flex; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-replays__empty {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 8px;
        overflow: hidden;
        background-color: var(--lui-color-surface);
    }
    .wt-replays__drop {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 12px;
        max-width: 100%;
        padding: 24px;
        border: 2px dashed var(--lui-color-border);
        border-radius: var(--lui-radius);
    }
    .wt-replays__map {
        position: relative;
        width: 120px;
        height: 120px;
        border: 1px solid var(--lui-color-border);
    }
    .wt-replays__grid-v { position: absolute; top: 0; bottom: 0; width: 1px; background-color: var(--lui-color-border); opacity: 0.35; }
    .wt-replays__grid-h { position: absolute; left: 0; right: 0; height: 1px; background-color: var(--lui-color-border); opacity: 0.35; }
    .wt-replays__islands { position: absolute; left: 0; top: 0; width: 100%; height: 100%; color: var(--lui-color-border); }
    .wt-replays__cap {
        position: absolute;
        width: 18px;
        height: 18px;
        margin-left: -9px;
        margin-top: -9px;
        display: flex;
        align-items: center;
        justify-content: center;
        border: 1px solid var(--lui-color-border);
        border-radius: 9px;
        font-size: 9px;
        color: var(--wt-color-text-dim);
    }
    .wt-replays__title { font-size: 13px; }
    .wt-replays__hint { max-width: 260px; font-size: 11px; text-align: center; color: var(--wt-color-text-dim); }"
);

/// The listing's width, as the egui app's left panel, and how far it may be dragged.
const LISTING_WIDTH: PanelWidth = PanelWidth(280.0);
const LISTING_RANGE: WidthRange = WidthRange { min: PanelWidth(180.0), max: PanelWidth(520.0) };

/// The dock's side regions, with the status bar toggle each puts up while it holds a panel.
const DOCK_SIDES: [(DockRegion, PanelSide, &str); 3] = [
    (DockRegion::Left, PanelSide::Left, "replay-inspector-dock-toggle-left"),
    (DockRegion::Right, PanelSide::Right, "replay-inspector-dock-toggle-right"),
    (DockRegion::Bottom, PanelSide::Bottom, "replay-inspector-dock-toggle-bottom"),
];

#[cfg(test)]
thread_local! {
    /// The workspace the replays tab last made, for a test to drive.
    pub static BUILT: std::cell::Cell<Option<ReplayWorkspace>> = const { std::cell::Cell::new(None) };
}

#[component]
pub fn ReplaysTab() -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let shell = Shell::current();
    let services = Services::current();
    let columns: Signal<Vec<ReplayColumn>, LocalStorage> = Signal::derive_local(move || {
        services
            .settings
            .read(|settings| default_columns(&settings.replay))
            .unwrap_or_else(|| default_columns(&ReplaySettings::default()))
    });
    let workspace = ReplayWorkspace::new(columns);
    #[cfg(test)]
    BUILT.with(|built| built.set(Some(workspace)));
    let control = BrowserControl::new();
    let arrivals = Arrivals::new();
    let offer = GameDataOffer::new();

    load_personal_rating(workspace);
    layout::persist(workspace);
    follow_dock(workspace);
    follow_session(workspace);
    follow_game_data(workspace, control, offer);

    let requests = RenderEffect::new(move |_| {
        for request in shell.take_replay_requests() {
            carry_out(workspace, control, request);
        }
    });
    on_cleanup_local(move || drop(requests));

    let on_event = UnsyncCallback::new(move |event: ReplayBrowserEvent| {
        on_browser_event(workspace, arrivals, offer, event);
    });

    let listing_open = RwSignal::new_local(true);
    let listing_width = RwSignal::new_local(LISTING_WIDTH);
    publish_toggles(workspace.dock, listing_open);
    publish_mode(workspace);

    let banner = move || match services.game_data.state() {
        GameDataState::Failed(reason) => Some(view! {
            text(class = "wt-replays__banner", attr:data-testid = "replays-game-data-failed") {{
                t!("ui.replay.game_data_failed", error = reason).into_owned()
            }}
        }),
        GameDataState::NoInstall | GameDataState::Loading { .. } | GameDataState::Ready(_) => None,
    };
    let locale = services.locale;

    view! {
        column(class = "wt-replays", attr:data-testid = "replays-tab") {
            {banner}
            SideSplit(
                open = listing_open,
                width = listing_width,
                range = LISTING_RANGE,
                resize_label = t!("ui.replay.listing_caption").into_owned(),
                keep_mounted = true,
                attr:data-testid = "replay-listing-split"
            ) {
                SidePanel(slot) {
                    ReplayBrowser(control = control, on_event = on_event)
                }
                box(class = "wt-replays__main") {
                    Dock(
                        state = workspace.dock,
                        close_label = t!("ui.replay_folders.close").into_owned(),
                        tabs_label = t!("ui.replay.listing_caption").into_owned(),
                        class = "wt-replays__dock",
                        menu = dock_menu_strings(locale),
                        strings = tab_menu_strings(locale),
                        attr:data-testid = "replays-dock"
                    )
                    Show(when = move || workspace.is_empty()) {
                        {move || {
                            locale.track();
                            view! { EmptyPane() }
                        }}
                    }
                }
            }
        }
    }
}

/// The slot a [`ReplayPanel`](panel::ReplayPanel) draws its battle's [`PlayerTable`](table::PlayerTable) in.
pub fn player_table_slot() -> TableSlot {
    Rc::new(|table: PanelTable| {
        let PanelTable { model, debug, detail, on_view_raw_json } = table;
        AnyView::new(view! {
            PlayerTable(model = model, debug = debug, detail = detail, on_view_raw_json = on_view_raw_json)
        })
    })
}

/// Carries out what a drop, the palette or another tab asked of the replays tab.
fn carry_out(workspace: ReplayWorkspace, control: BrowserControl, request: ReplayRequest) {
    match request {
        ReplayRequest::OpenReplay(path) => {
            workspace.open(path, OpenTarget::ShowingTab);
        }
        ReplayRequest::ListReplays(paths) => control.open_dropped(paths),
        ReplayRequest::OpenFolder(root) => control.open_folder(root),
        ReplayRequest::OpenRecentFolder(root) => control.open_recent_folder(root),
        ReplayRequest::RenderReplay(path) => workspace.render(RenderRequest { path, alts: Vec::new() }),
        ReplayRequest::RenderToClipboard(path) => batch::render_many(workspace, vec![path], BatchTarget::Clipboard),
        ReplayRequest::ReparseOpen => workspace.reparse_all(ReparseCause::GameDataArrived),
    }
}

fn on_browser_event(workspace: ReplayWorkspace, arrivals: Arrivals, offer: GameDataOffer, event: ReplayBrowserEvent) {
    let shell = workspace.shell;
    match event {
        ReplayBrowserEvent::Open(path) => {
            workspace.open(path, OpenTarget::ShowingTab);
        }
        ReplayBrowserEvent::OpenInNewTab(path) => {
            workspace.open(path, OpenTarget::NewTab);
        }
        ReplayBrowserEvent::RenderReplay(path) => workspace.render(RenderRequest { path, alts: Vec::new() }),
        ReplayBrowserEvent::RenderManyToVideo(paths) => batch::render_many(workspace, paths, BatchTarget::Directory),
        ReplayBrowserEvent::RenderManyToClipboard(paths) => {
            batch::render_many(workspace, paths, BatchTarget::Clipboard);
        }
        ReplayBrowserEvent::SessionStats { paths, replace } => session_stats::record(workspace, paths, replace),
        ReplayBrowserEvent::ReplayAppeared { path, battle, install } => {
            arrivals.appeared(workspace, path, &battle, install);
        }
        ReplayBrowserEvent::ReplayChanged(path) => arrivals.changed(workspace, path),
        // The listing forgets the row itself; an open tab keeps what it read.
        ReplayBrowserEvent::ReplayRemoved(_) => {}
        ReplayBrowserEvent::BuildsMissing(missing) => offer.offer(shell, missing),
        ReplayBrowserEvent::BuildsListed(builds) => workspace.mappings.fetch_missing(workspace, builds, Cached::Keep),
        ReplayBrowserEvent::CheckGameData(missing) => offer.check(shell, missing),
        ReplayBrowserEvent::ConstantsImported { .. } => workspace.reparse_all(ReparseCause::GameDataArrived),
    }
}

/// Reads the expected-values table every replay tab rates its players against.
fn load_personal_rating(workspace: ReplayWorkspace) {
    let services = workspace.services;
    let started = zgui::reactive::StoredValue::new(false);
    let loading = RenderEffect::new(move |_| {
        if started.get_value() {
            return;
        }
        // The proxy is a setting, so the table is read once the settings are.
        let Some(proxy) = services.settings.read(|settings| settings.proxy_url.clone()) else { return };
        started.set_value(true);
        let _task = spawn_local(async move {
            let rating = services
                .on_tokio(async move {
                    wows_toolkit_services::personal_rating::load_refreshed(crate::http::USER_AGENT, &proxy).await
                })
                .await;
            match rating {
                Ok(table) => workspace.personal_rating.set(table.map(Arc::new)),
                Err(err) => tracing::warn!("replays: the personal rating table was not read: {err}"),
            }
        });
    });
    on_cleanup_local(move || drop(loading));
}

/// Lets go of closed tabs, and of viewports that moved into windows of their own.
fn follow_dock(workspace: ReplayWorkspace) {
    let closing = RenderEffect::new(move |_| {
        let _ = workspace.dock.panels();
        workspace.forget_closed();
    });
    let popping = RenderEffect::new(move |_| workspace.release_popped_out());
    on_cleanup_local(move || {
        drop(closing);
        drop(popping);
    });
}

/// Hands open viewports the collab session as it starts and stops, and opens the battles it asks
/// to be watched.
fn follow_session(workspace: ReplayWorkspace) {
    let collab = workspace.shell.collab;
    let link = collab.link();
    let sharing = RenderEffect::new(move |seen: Option<()>| {
        link.track();
        // The first run is the link the viewports were opened with.
        if seen.is_some() {
            workspace.share_session();
        }
    });
    let watching = RenderEffect::new(move |_| {
        for shared in collab.take_watch_requests() {
            workspace.watch(shared);
        }
    });
    on_cleanup_local(move || {
        drop(sharing);
        drop(watching);
    });
}

/// Reads the open replays again when a build loads or a download the reader accepted lands, and
/// lists the directories again for the latter.
fn follow_game_data(workspace: ReplayWorkspace, control: BrowserControl, offer: GameDataOffer) {
    let services = workspace.services;
    let shell = workspace.shell;
    let arriving = RenderEffect::new(move |was_ready: Option<bool>| {
        let ready = matches!(services.game_data.state(), GameDataState::Ready(_));
        if ready && was_ready == Some(false) {
            workspace.reparse_all(ReparseCause::GameDataArrived);
        }
        ready
    });
    let downloading = RenderEffect::new(move |was_busy: Option<bool>| {
        let busy = shell.cache.busy();
        if was_busy == Some(true) && !busy {
            // Builds reported while the cache was busy are offered now.
            offer.offer_pending(shell);
        }
        if was_busy == Some(true) && !busy && offer.fetching.get_value() {
            offer.fetching.set_value(false);
            control.relist();
            workspace.reparse_all(ReparseCause::GameDataArrived);
        }
        busy
    });
    on_cleanup_local(move || {
        drop(arriving);
        drop(downloading);
    });
}

/// Puts the listing's toggle on the status bar, and one per dock side region that holds a panel.
/// The listing's state is the stored replay setting, as in the egui app.
fn publish_toggles(dock: DockState, listing_open: RwSignal<bool, LocalStorage>) {
    let services = Services::current();
    let mut effects = Vec::new();

    // The setting moves the toggle, and the toggle writes the setting.
    effects.push(RenderEffect::new(move |_| {
        let Some(collapsed) = services.settings.read(|settings| settings.replay.listing_collapsed) else { return };
        if listing_open.get_untracked() == collapsed {
            listing_open.set(!collapsed);
        }
    }));
    effects.push(RenderEffect::new(move |_| {
        let open = listing_open.get();
        let stored = services.settings.read_untracked(|settings| settings.replay.listing_collapsed);
        if stored.is_some_and(|collapsed| collapsed == open) {
            services.settings.edit(keys::REPLAY_SETTINGS, |settings| {
                settings.replay.listing_collapsed = !open;
                settings.replay.clone()
            });
        }
    }));

    let opens = DOCK_SIDES.map(|(region, _, _)| RwSignal::new_local(dock.is_region_open(region)));
    for ((region, _, _), open) in DOCK_SIDES.into_iter().zip(opens) {
        effects.push(RenderEffect::new(move |_| {
            let shown = dock.is_region_open(region);
            if open.get_untracked() != shown {
                open.set(shown);
            }
        }));
        effects.push(RenderEffect::new(move |_| {
            let wanted = open.get();
            if dock.is_region_open(region) != wanted {
                dock.set_region_open(region, wanted);
            }
        }));
    }
    if let Some(chrome) = TabChrome::current() {
        let locale = services.locale;
        effects.push(RenderEffect::new(move |_| {
            locale.track();
            let mut toggles = vec![PanelToggle {
                test_id: "replay-listing-toggle",
                name: t!("ui.replay.listing_caption").into_owned(),
                side: PanelSide::Left,
                open: listing_open,
            }];
            toggles.extend(
                DOCK_SIDES
                    .into_iter()
                    .zip(opens)
                    .filter(|((region, _, _), _)| !dock.is_region_empty(*region))
                    .map(|((_, side, test_id), open)| PanelToggle { test_id, name: panel_side_name(side), side, open }),
            );
            chrome.set_toggles(toggles);
        }));
    }
    on_cleanup_local(move || drop(effects));
}

/// Names on the status bar the held-key mode of the panels on screen.
fn publish_mode(workspace: ReplayWorkspace) {
    let Some(chrome) = TabChrome::current() else { return };
    let mode = RenderEffect::new(move |_| chrome.set_mode(workspace.mode()));
    on_cleanup_local(move || {
        drop(mode);
        chrome.set_mode(None);
    });
}

/// What the dock shows with nothing open: a faint map, and what can be dropped on it.
#[component]
fn EmptyPane() -> impl IntoView {
    const GRID_LINES: u16 = 10;
    const CAPTURES: [(&str, f32, f32); 3] = [("A", 0.18, 0.50), ("B", 0.50, 0.48), ("C", 0.82, 0.53)];

    let percent = |fraction: f32| format!("{:.1}%", fraction * 100.0);
    let grid: Vec<AnyView> = (1..GRID_LINES)
        .flat_map(|line| {
            let at = percent(f32::from(line) / f32::from(GRID_LINES));
            [
                AnyView::new(view! { box(class = "wt-replays__grid-v", style:left = at.clone()) }),
                AnyView::new(view! { box(class = "wt-replays__grid-h", style:top = at) }),
            ]
        })
        .collect();
    let captures: Vec<AnyView> = CAPTURES
        .into_iter()
        .map(|(letter, x, y)| {
            AnyView::new(view! {
                box(class = "wt-replays__cap", style:left = percent(x), style:top = percent(y)) {
                    text {{letter}}
                }
            })
        })
        .collect();

    view! {
        box(class = "wt-replays__empty", attr:data-testid = "replay-pane-empty") {
            column(class = "wt-replays__drop") {
                box(class = "wt-replays__map", a11y:hidden = true) {
                    {grid}
                    vector(class = "wt-replays__islands", prop:svg = PropValue::from(islands_svg())) {}
                    {captures}
                }
                text(class = "wt-replays__title") {{t!("ui.replay.select_replay").into_owned()}}
                text(class = "wt-replays__hint") {{t!("ui.replay.drop_hint").into_owned()}}
            }
        }
    }
}

/// The empty pane's islands as an SVG document over the 120 pixel map, filled faintly and
/// outlined in `currentColor`. Each vertex pulls its coast toward it as the control point between
/// the midpoints of its two edges, so a vertex set inward reads as a bay and one set outward as a
/// point.
fn islands_svg() -> String {
    const EDGE: f32 = 120.0;
    // Coastline vertices in fractions of the map's edge, from the top left.
    const ISLANDS: [&[(f32, f32)]; 10] = [
        &[
            (0.29, 0.27),
            (0.31, 0.21),
            (0.35, 0.19),
            (0.37, 0.14),
            (0.42, 0.15),
            (0.45, 0.12),
            (0.50, 0.13),
            (0.53, 0.10),
            (0.57, 0.12),
            (0.62, 0.13),
            (0.64, 0.18),
            (0.61, 0.21),
            (0.63, 0.26),
            (0.59, 0.29),
            (0.56, 0.27),
            (0.55, 0.23),
            (0.51, 0.22),
            (0.48, 0.25),
            (0.49, 0.30),
            (0.45, 0.33),
            (0.42, 0.36),
            (0.39, 0.32),
            (0.35, 0.33),
            (0.31, 0.31),
        ],
        &[
            (0.28, 0.64),
            (0.31, 0.60),
            (0.35, 0.61),
            (0.38, 0.58),
            (0.42, 0.61),
            (0.41, 0.66),
            (0.44, 0.69),
            (0.43, 0.73),
            (0.39, 0.74),
            (0.38, 0.78),
            (0.41, 0.82),
            (0.37, 0.86),
            (0.33, 0.84),
            (0.30, 0.80),
            (0.31, 0.75),
            (0.27, 0.71),
        ],
        &[
            (0.71, 0.26),
            (0.74, 0.21),
            (0.79, 0.22),
            (0.82, 0.18),
            (0.87, 0.19),
            (0.91, 0.23),
            (0.92, 0.29),
            (0.89, 0.34),
            (0.85, 0.37),
            (0.83, 0.33),
            (0.86, 0.29),
            (0.84, 0.25),
            (0.80, 0.27),
            (0.77, 0.31),
            (0.73, 0.31),
        ],
        &[
            (0.06, 0.20),
            (0.09, 0.16),
            (0.13, 0.17),
            (0.16, 0.21),
            (0.14, 0.25),
            (0.15, 0.29),
            (0.11, 0.30),
            (0.08, 0.26),
        ],
        &[
            (0.07, 0.75),
            (0.11, 0.70),
            (0.16, 0.72),
            (0.19, 0.76),
            (0.16, 0.79),
            (0.18, 0.85),
            (0.13, 0.88),
            (0.09, 0.84),
            (0.10, 0.79),
        ],
        &[(0.86, 0.69), (0.90, 0.66), (0.94, 0.70), (0.91, 0.75), (0.87, 0.74)],
        &[(0.60, 0.67), (0.63, 0.65), (0.67, 0.66), (0.68, 0.70), (0.65, 0.73), (0.61, 0.72)],
        &[(0.70, 0.78), (0.73, 0.75), (0.77, 0.76), (0.79, 0.80), (0.76, 0.82), (0.72, 0.82)],
        &[(0.59, 0.81), (0.62, 0.80), (0.64, 0.83), (0.62, 0.86), (0.59, 0.85)],
        &[(0.67, 0.88), (0.69, 0.87), (0.70, 0.89), (0.68, 0.90)],
    ];

    let at = |(x, y): (f32, f32)| format!("{:.2} {:.2}", x * EDGE, y * EDGE);
    let midpoint = |from: (f32, f32), to: (f32, f32)| ((from.0 + to.0) * 0.5, (from.1 + to.1) * 0.5);
    let mut path = String::new();
    for island in ISLANDS {
        let (Some(&first), Some(&last)) = (island.first(), island.last()) else { continue };
        path.push_str(&format!("M{} ", at(midpoint(last, first))));
        let following = island.iter().skip(1).chain([&first]);
        for (&vertex, &next) in island.iter().zip(following) {
            path.push_str(&format!("Q{} {} ", at(vertex), at(midpoint(vertex, next))));
        }
        path.push_str("Z ");
    }

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {EDGE} {EDGE}\"><path d=\"{path}\" \
         fill=\"currentColor\" fill-opacity=\"0.35\" stroke=\"currentColor\" stroke-width=\"1\"/></svg>"
    )
}
