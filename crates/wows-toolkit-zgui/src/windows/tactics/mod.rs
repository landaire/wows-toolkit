//! The tactics board, in a window of its own.
//!
//! Its own window rather than a tab, as the egui board is: a board is drawn beside the battle it
//! is about, not instead of it. [`TacticsHost`] is what the main window holds of it: the open
//! window, the capture layouts a board starts from and the collab session a board joins. The board
//! itself is `wows_toolkit_replay_data::tactics::board::Board`.

mod map;
mod model;
#[cfg(test)]
mod tests;
mod toolbar;

use std::path::Path;

use rust_i18n::t;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_toolkit_config::WindowKind;
use wows_toolkit_config::WindowSettings;
use wows_toolkit_services::collab::CollabLink;
use zgui::component;
use zgui::css;
use zgui::prelude::WindowHandle;
use zgui::prelude::WindowOptions;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::WithValue;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::Timers;
use zgui::view::install_stylesheet;

use crate::services::GameDataState;
use crate::services::Services;
use crate::window_shell::RememberGeometryProps;
use crate::windows::tactics::map::BoardMapProps;
use crate::windows::tactics::model::BoardModel;
use crate::windows::tactics::model::COLLAB_TICK;
use crate::windows::tactics::toolbar::BoardToolbarProps;

/// Where a board's window opens when nothing was remembered, and the smallest it may be made.
const DEFAULT_SIZE: (f32, f32) = (1200.0, 800.0);
const MIN_SIZE: (f32, f32) = (640.0, 480.0);

/// What the main window holds of the tactics board. A `Copy` handle.
#[derive(Clone, Copy)]
pub struct TacticsHost {
    /// The board's window while one is open, so the menu brings it forward rather than opening
    /// another: two windows on one board would each walk the replays and each write what they
    /// found.
    window: StoredValue<Option<WindowHandle>, LocalStorage>,
    /// The capture-point layouts a board picks its modes from, read from the store both apps
    /// write. Empty until the read lands, and empty on a machine that has never indexed a replay,
    /// which is a board with maps and no modes rather than no board.
    layouts: StoredValue<CapLayoutDb, LocalStorage>,
    /// Counts reads of the store, which an open board follows.
    layouts_read: RwSignal<u64, LocalStorage>,
    layouts_requested: StoredValue<bool>,
    /// What the database remembers of the board's window, read once.
    remembered: StoredValue<Option<WindowSettings>>,
    /// The session the board is one of the windows of. `None` outside one.
    session: RwSignal<Option<CollabLink>, LocalStorage>,
}

impl TacticsHost {
    pub fn new() -> Self {
        Self {
            window: StoredValue::new_local(None),
            layouts: StoredValue::new_local(CapLayoutDb::default()),
            layouts_read: RwSignal::new_local(0),
            layouts_requested: StoredValue::new(false),
            remembered: StoredValue::new(None),
            session: RwSignal::new_local(None),
        }
    }

    /// Opens the board, or brings the open one forward. Called from a component's body or from a
    /// listener inside the main window.
    pub fn open(self) {
        if let Some(open) = self.window.get_value().filter(WindowHandle::is_open) {
            open.focus();
            return;
        }

        let saved = self.remembered.get_value().unwrap_or_default();
        let [width, height] = saved.inner_size_points.unwrap_or([DEFAULT_SIZE.0, DEFAULT_SIZE.1]);
        let mut options = WindowOptions::new(t!("ui.windows.tactics_board").into_owned())
            .with_size(width, height)
            .with_min_size(MIN_SIZE.0, MIN_SIZE.1);
        if let Some([x, y]) = saved.outer_position_pixels {
            options = options.with_position(x, y);
        }
        let host = self;
        let handle = crate::windows::open(options, move || view! { TacticsWindow(host = host) });
        if saved.maximized {
            handle.set_maximized(true);
        }
        self.window.set_value(Some(handle));
    }

    /// Hands an open board its end of a session, and a board opened later starts in it. `None`
    /// ends the session for the board, which keeps what was drawn.
    pub fn share_session(self, link: Option<CollabLink>) {
        self.session.set(link);
    }

    /// Takes what a board's walk of the replays found, for a board opened later to start from.
    fn adopt_layouts(self, found: CapLayoutDb) {
        self.layouts.set_value(found);
    }

    /// Reads the layouts and the remembered window once the database is open, so a board opened
    /// after that starts from them. Called once, from the main window's shell.
    pub fn begin(self, services: Services) {
        let reading = RenderEffect::new(move |_| {
            services.settings.with_state(|_| ());
            self.read_store(services);
        });
        on_cleanup_local(move || drop(reading));
    }

    /// Reads the layouts and the remembered window once, when the database is open.
    fn read_store(self, services: Services) {
        if self.layouts_requested.get_value() {
            return;
        }
        let Some(pool) = services.settings.pool() else { return };
        self.layouts_requested.set_value(true);
        let read = services.on_tokio(async move {
            let layouts = wows_toolkit_replay_data::cap_layout_store::load_from_db(&pool).await;
            let remembered = wows_toolkit_config::load_all_window_settings(&pool).await;
            (layouts, remembered.get(&WindowKind::TacticsBoard).copied())
        });
        spawn_local(async move {
            match read.await {
                Ok((layouts, remembered)) => {
                    self.layouts.set_value(layouts);
                    self.remembered.set_value(remembered);
                    self.layouts_read.update(|read| *read += 1);
                }
                Err(err) => tracing::warn!("tactics board: the capture layouts were not read: {err}"),
            }
        });
    }
}

impl Default for TacticsHost {
    fn default() -> Self {
        Self::new()
    }
}

const SHEET: &str = "wt-tactics";

const CSS: &str = css!(
    ".wt-tactics {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background-color: var(--lui-color-bg);
        color: var(--lui-color-text);
    }
    .wt-tactics__peer {
        padding: 0 8px 4px 8px;
        font-size: var(--zui-type-size-xs);
        color: var(--wt-color-text-dim);
    }
    .wt-tactics__rule {
        height: 1px;
        flex: none;
        background-color: var(--lui-color-border);
    }"
);

/// The board's window content.
#[component]
fn TacticsWindow(host: TacticsHost) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let model = BoardModel::new(services, host, services.game_data.cache_untracked(), host.layouts.get_value());
    #[cfg(test)]
    tests::mounted(model);

    follow_services(model, services, host);
    let ticking = Timers::current().map(|timers| {
        timers.set_interval(COLLAB_TICK, move || {
            if model.with_untracked(|board| board.in_session()) {
                model.update(|board| board.tick());
            }
        })
    });
    on_cleanup_local(move || {
        drop(ticking);
        model.update(|board| board.close());
    });

    let root = NodeRef::new();
    hear_unfocused_keys(root);
    let peers = Memo::new(move |_| model.with(|board| board.is_a_peers()));
    let locale = services.locale;

    view! {
        column(
            class = "wt-tactics",
            node_ref = root,
            attr:data-testid = "tactics-board",
            on:key_down = move |ev| {
                if ev.repeat {
                    return;
                }
                if map::board_key(model, ev) {
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            }
        ) {
            RememberGeometry(kind = WindowKind::TacticsBoard)
            BoardToolbar(model = model)
            if move || peers.get() {
                // Says why zones and shapes this reader did not put there are on the board: it is
                // a peer's, opened because the session is on it.
                text(class = "wt-tactics__peer") {{locale.text("ui.tactics.a_peers_board")}}
            } else {}
            box(class = "wt-tactics__rule")
            BoardMap(model = model)
        }
    }
}

/// Keeps the board on the build, the install, the stored layouts and the session the app has.
fn follow_services(model: BoardModel, services: Services, host: TacticsHost) {
    let game_data = RenderEffect::new(move |_| {
        if matches!(services.game_data.state(), GameDataState::Ready(_)) {
            let cache = services.game_data.cache_untracked();
            model.update(|board| board.set_game_data(cache));
        }
    });
    let install = RenderEffect::new(move |_| {
        let Some(wows_dir) = services.settings.read(|settings| settings.wows_dir.clone()) else { return };
        let (dir, version) = if wows_dir.is_empty() {
            (None, None)
        } else {
            let path = Path::new(&wows_dir);
            (
                Some(wows_toolkit_replay_data::install::resolve_replays_dir(path)),
                wows_toolkit_replay_data::install::installed_version(path),
            )
        };
        model.update(|board| board.set_install(dir, version));
    });
    let layouts = RenderEffect::new(move |seen: Option<u64>| {
        let read = host.layouts_read.get();
        if seen.is_some_and(|seen| seen != read) {
            model.update(|board| board.set_layouts(host.layouts.get_value()));
        }
        read
    });
    let session = RenderEffect::new(move |seen: Option<bool>| {
        let link = host.session.get();
        let active = link.as_ref().is_some_and(CollabLink::is_active);
        // A board opened outside a session has nothing to leave.
        if seen.is_some() || active {
            model.update(|board| board.set_collab(link.unwrap_or_default()));
        }
        active
    });
    on_cleanup_local(move || {
        drop(game_data);
        drop(install);
        drop(layouts);
        drop(session);
    });
}

/// Lets the board's chords reach it while nothing in its window has focus.
fn hear_unfocused_keys(root: NodeRef) {
    let held = StoredValue::new_local(None);
    let registering = RenderEffect::new(move |_| {
        if root.get().is_some() && held.with_value(Option::is_none) {
            held.set_value(Some(root.window_shortcut()));
        }
    });
    on_cleanup_local(move || {
        drop(registering);
        held.set_value(None);
    });
}
