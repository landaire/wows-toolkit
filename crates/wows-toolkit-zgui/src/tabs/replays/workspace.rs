//! The replays tab's dock: a panel per opened replay, playback viewports, and battles a collab peer
//! is playing back.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::PanelEntry;
use landaire_ui::PanelFactory;
use landaire_ui::PanelId;
use landaire_ui::PanelKind;
use landaire_ui::PanelRegistry;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_services::collab::SharedWindow;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;

use crate::services::Services;
use crate::shell::Shell;
use crate::shell::tabs::ModeHint;
use crate::tabs::armor_viewer::library::FeedSource;
use crate::tabs::armor_viewer::library::ViewerFeed;
use crate::tabs::replays::constants::ResultMappings;
use crate::tabs::replays::panel::PanelControl;
use crate::tabs::replays::panel::PanelEvents;
use crate::tabs::replays::panel::PanelSetup;
use crate::tabs::replays::panel::PanelTitle;
use crate::tabs::replays::panel::ReparseCause;
use crate::tabs::replays::panel::ReplayPanelProps;
use crate::tabs::replays::player_table_slot;
use crate::tabs::replays::title::ReplayTabTitleProps;
use crate::windows::renderer::session::ArmorRequest;
use crate::windows::renderer::session::RenderRequest;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::view::ReplayRendererProps;
use crate::windows::renderer::view::SHORTCUTS_MODE;
use crate::windows::renderer::watched::WatchedPlaybackProps;

const REPLAY_KIND: &str = "replay";
const RENDERER_KIND: &str = "renderer";
const WATCHED_KIND: &str = "watched";

const REPLAY_PREFIX: &str = "replay:";
const RENDERER_PREFIX: &str = "renderer:";
const WATCHED_PREFIX: &str = "watched:";

/// A battle a collab peer is playing back, by the id the session gives its window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WatchId(pub u64);

/// What a dock panel shows, as its id spells it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Docked {
    Replay(PathBuf),
    Renderer(PathBuf),
    Watched(WatchId),
}

impl Docked {
    pub fn panel_id(&self) -> PanelId {
        PanelId::new(match self {
            Self::Replay(path) => format!("{REPLAY_PREFIX}{}", path.to_string_lossy()),
            Self::Renderer(path) => format!("{RENDERER_PREFIX}{}", path.to_string_lossy()),
            Self::Watched(id) => format!("{WATCHED_PREFIX}{}", id.0),
        })
    }

    pub fn of(id: &PanelId) -> Option<Self> {
        let id = id.as_str();
        if let Some(path) = id.strip_prefix(REPLAY_PREFIX) {
            return Some(Self::Replay(PathBuf::from(path)));
        }
        if let Some(path) = id.strip_prefix(RENDERER_PREFIX) {
            return Some(Self::Renderer(PathBuf::from(path)));
        }
        id.strip_prefix(WATCHED_PREFIX)?.parse().ok().map(|id| Self::Watched(WatchId(id)))
    }

    fn entry(&self) -> PanelEntry {
        let kind = match self {
            Self::Replay(_) => REPLAY_KIND,
            Self::Renderer(_) => RENDERER_KIND,
            Self::Watched(_) => WATCHED_KIND,
        };
        PanelEntry::new(self.panel_id(), PanelKind::new(kind))
    }
}

/// Where an open puts the replay it was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenTarget {
    /// In place of the replay tab showing.
    ShowingTab,
    NewTab,
}

/// What an open did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabShown {
    Opened,
    /// The replay was already open, and its tab came forward.
    BroughtForward,
}

/// The dock and what each of its panels shows, as one `Copy` handle.
#[derive(Clone, Copy)]
pub struct ReplayWorkspace {
    pub dock: DockState,
    /// Each open replay's panel control, which keeps what it read while the dock builds its view
    /// again.
    replays: RwSignal<HashMap<PathBuf, PanelControl>, LocalStorage>,
    renderers: RwSignal<HashMap<PathBuf, RendererSession>, LocalStorage>,
    /// Viewports moved into windows of their own, kept so a collab session reaches them too.
    popped: StoredValue<Vec<RendererSession>, LocalStorage>,
    watched: RwSignal<HashMap<WatchId, SharedWindow>, LocalStorage>,
    /// The replay last opened or brought forward, which a plain open replaces first.
    current: StoredValue<Option<PathBuf>>,
    /// Set once the reader opens something, so a saved layout read afterwards does not replace it.
    pub(crate) touched: StoredValue<bool>,
    pub columns: Signal<Vec<ReplayColumn>, LocalStorage>,
    pub personal_rating: RwSignal<Option<Arc<PersonalRatingData>>, LocalStorage>,
    pub mappings: ResultMappings,
    owner: StoredValue<Owner, LocalStorage>,
    pub shell: Shell,
    pub services: Services,
}

impl ReplayWorkspace {
    /// Made in the replays tab's body; its panels and sessions belong to the tab.
    pub fn new(columns: Signal<Vec<ReplayColumn>, LocalStorage>) -> Self {
        let workspace = Self {
            dock: DockState::new(PanelRegistry::new()),
            replays: RwSignal::new_local(HashMap::new()),
            renderers: RwSignal::new_local(HashMap::new()),
            popped: StoredValue::new_local(Vec::new()),
            watched: RwSignal::new_local(HashMap::new()),
            current: StoredValue::new(None),
            touched: StoredValue::new(false),
            columns,
            personal_rating: RwSignal::new_local(None),
            mappings: ResultMappings::new(),
            owner: StoredValue::new_local(Owner::current().expect("the workspace is made inside a component")),
            shell: Shell::current(),
            services: Services::current(),
        };
        workspace.register();

        workspace
    }

    /// Runs `make` under the tab, so what it makes lives as long as the tab does and finds its
    /// contexts from a listener.
    pub fn under_tab<T>(self, make: impl FnOnce() -> T) -> T {
        self.owner.with_value(|owner| owner.with(make))
    }

    fn register(self) {
        let dock = self.dock;
        dock.register(
            PanelKind::new(REPLAY_KIND),
            PanelFactory::new(
                // The file's name until the battle is read, then "{ship} - {map}" as the title shows.
                move |id| match Docked::of(id) {
                    Some(Docked::Replay(path)) => {
                        match self.control_of(&Docked::Replay(path.clone())).map(PanelControl::title) {
                            Some(PanelTitle::Read { title, .. }) => title,
                            _ => path.file_stem().unwrap_or(path.as_os_str()).to_string_lossy().into_owned(),
                        }
                    }
                    _ => String::new(),
                },
                move |id| match Docked::of(id) {
                    Some(Docked::Replay(path)) => self.replay_view(path),
                    _ => AnyView::new(()),
                },
            )
            .with_title(move |id| match Docked::of(id).and_then(|docked| self.control_of(&docked)) {
                Some(control) => {
                    AnyView::new(view! { ReplayTabTitle(title = Signal::derive_local(move || control.title())) })
                }
                None => AnyView::new(()),
            }),
        );
        dock.register(
            PanelKind::new(RENDERER_KIND),
            PanelFactory::new(
                move |id| match self.session_of(id) {
                    Some(session) if session.is_open() => session.title(),
                    _ => String::new(),
                },
                move |id| match self.session_of(id) {
                    Some(session) if session.is_open() => AnyView::new(view! { ReplayRenderer(session = session) }),
                    _ => AnyView::new(()),
                },
            ),
        );
        dock.register(
            PanelKind::new(WATCHED_KIND),
            PanelFactory::new(
                move |id| self.shared_of(id).map(|shared| shared.replay_name).unwrap_or_default(),
                move |id| match self.shared_of(id) {
                    Some(shared) => {
                        let link = self.shell.collab.link().get_untracked();
                        AnyView::new(view! { WatchedPlayback(shared = shared, link = link) })
                    }
                    None => AnyView::new(()),
                },
            ),
        );
    }

    fn control_of(self, docked: &Docked) -> Option<PanelControl> {
        let Docked::Replay(path) = docked else { return None };
        self.replays.with_untracked(|replays| replays.get(path).copied())
    }

    fn replay_view(self, path: PathBuf) -> AnyView {
        let Some(control) = self.replays.with_untracked(|replays| replays.get(&path).copied()) else {
            return AnyView::new(());
        };
        let setup = PanelSetup {
            path,
            columns: self.columns.get_untracked(),
            personal_rating: self.personal_rating.get_untracked(),
        };
        let events = PanelEvents {
            on_render: Some(UnsyncCallback::new(move |asked: crate::tabs::replays::panel::RenderRequest| {
                self.render(RenderRequest { path: asked.path, alts: asked.alts });
            })),
            on_constants_mismatched: Some(UnsyncCallback::new(
                move |mismatch: crate::tabs::replays::panel::ConstantsMismatched| {
                    self.mappings.recover(self, mismatch.build, mismatch.version);
                },
            )),
        };
        let columns = self.columns;
        let rating: Signal<Option<Arc<PersonalRatingData>>, LocalStorage> = self.personal_rating.into();

        AnyView::new(view! {
            ReplayPanel(
                setup = setup,
                table = player_table_slot(),
                control = control,
                events = events,
                columns = columns,
                personal_rating = rating
            )
        })
    }

    fn session_of(self, id: &PanelId) -> Option<RendererSession> {
        let Some(Docked::Renderer(path)) = Docked::of(id) else { return None };
        self.renderers.with_untracked(|renderers| renderers.get(&path).copied())
    }

    fn shared_of(self, id: &PanelId) -> Option<SharedWindow> {
        let Some(Docked::Watched(watch)) = Docked::of(id) else { return None };
        self.watched.with_untracked(|watched| watched.get(&watch).cloned())
    }

    /// The replays open, in no order.
    #[cfg(test)]
    pub fn open_replays(self) -> Vec<PathBuf> {
        self.replays.with_untracked(|replays| replays.keys().cloned().collect())
    }

    #[cfg(test)]
    pub fn open_renderers(self) -> Vec<PathBuf> {
        self.renderers.with_untracked(|renderers| renderers.keys().cloned().collect())
    }

    /// Whether anything shows in the dock's middle. Reactive.
    pub fn is_empty(self) -> bool {
        self.dock.is_region_empty(DockRegion::Center)
            && self.dock.is_region_empty(DockRegion::Left)
            && self.dock.is_region_empty(DockRegion::Right)
            && self.dock.is_region_empty(DockRegion::Bottom)
    }

    /// The held-key mode of the panels on screen, for the status bar. A panel behind another tab
    /// or in a closed region offers none. When several are on screen, the one whose key is down
    /// wins. Reactive.
    pub fn mode(self) -> Option<ModeHint> {
        let modes: Vec<ModeHint> = self
            .dock
            .shown_panels()
            .iter()
            .filter_map(|id| match Docked::of(id)? {
                Docked::Replay(path) => self.replays.with(|replays| replays.get(&path).copied())?.mode(),
                Docked::Renderer(path) => {
                    let session = self.renderers.with(|renderers| renderers.get(&path).copied())?;
                    session.is_open().then_some(SHORTCUTS_MODE)
                }
                Docked::Watched(_) => None,
            })
            .collect();
        let held = self.shell.held_modifiers();

        modes.iter().copied().find(|mode| mode.key.is_down(held)).or_else(|| modes.first().copied())
    }

    /// Shows `path` in a tab. `None` when nothing could be shown, because there is no game data
    /// to read it with yet.
    pub fn open(self, path: PathBuf, target: OpenTarget) -> Option<TabShown> {
        if self.services.game_data.cache_untracked().is_none() {
            tracing::warn!(path = %path.display(), "replays: open asked for before the game directory was known");
            return None;
        }
        self.touched.set_value(true);

        let docked = Docked::Replay(path.clone());
        if self.replays.with_untracked(|replays| replays.contains_key(&path)) {
            if let Err(err) = self.dock.activate(&docked.panel_id()) {
                tracing::warn!("replays: the open replay could not be shown: {err}");
            }
            self.current.set_value(Some(path));
            return Some(TabShown::BroughtForward);
        }

        let replaced = match target {
            OpenTarget::ShowingTab => self.replaceable(),
            OpenTarget::NewTab => None,
        };
        let control = self.under_tab(PanelControl::new);
        self.replays.update(|replays| {
            replays.insert(path.clone(), control);
        });
        // In the group of the replay it replaces, so the replacement shows where it was.
        let place = replaced
            .as_ref()
            .and_then(|shown| self.dock.group_of(&Docked::Replay(shown.clone()).panel_id()))
            .map_or(DockTarget::Region(DockRegion::Center), DockTarget::Group);
        if let Err(err) = self.dock.add_panel(place, docked.entry()) {
            tracing::warn!("replays: the replay was not docked: {err}");
            self.replays.update(|replays| {
                replays.remove(&path);
            });
            return None;
        }
        if let Some(shown) = replaced {
            self.remove(&Docked::Replay(shown));
        }
        self.current.set_value(Some(path));

        Some(TabShown::Opened)
    }

    /// The replay a plain open replaces: one showing in its tab group. The one last opened or
    /// brought forward wins; failing that the first by path, so the choice is repeatable.
    fn replaceable(self) -> Option<PathBuf> {
        let dock = self.dock;
        let mut showing: Vec<PathBuf> = dock
            .panels_untracked()
            .into_iter()
            .filter_map(|entry| {
                let Some(Docked::Replay(path)) = Docked::of(&entry.id) else { return None };
                let group = dock.group_of(&entry.id)?;
                (dock.active_panel(group)? == entry.id).then_some(path)
            })
            .collect();
        showing.sort();
        let current = self.current.get_value();

        showing.iter().find(|path| Some(*path) == current.as_ref()).cloned().or_else(|| showing.into_iter().next())
    }

    /// Opens a playback viewport on `request`, or brings forward the one already open on its
    /// replay, baked again when the recordings it reads differ.
    pub fn render(self, request: RenderRequest) {
        if self.services.game_data.cache_untracked().is_none() {
            tracing::warn!(path = %request.path.display(), "replays: render asked for before the game directory was known");
            return;
        }
        self.touched.set_value(true);
        let docked = Docked::Renderer(request.path.clone());
        let existing = self.renderers.with_untracked(|renderers| renderers.get(&request.path).copied());
        if let Some(session) = existing.filter(|session| session.is_open()) {
            session.rebake_with_alts(request.alts);
            if let Err(err) = self.dock.activate(&docked.panel_id()) {
                tracing::warn!("replays: the open viewport could not be shown: {err}");
            }
            return;
        }

        let path = request.path.clone();
        let link = self.shell.collab.link().get_untracked();
        let session = self.under_tab(|| RendererSession::open(request, link));
        let shell = self.shell;
        let source = shell.armor.new_source();
        session.set_armor_listener(UnsyncCallback::new(move |asked: ArmorRequest| {
            view_armor(shell, session, source, asked)
        }));
        self.renderers.update(|renderers| {
            renderers.insert(path.clone(), session);
        });
        if let Err(err) = self.dock.add_panel(DockTarget::Region(DockRegion::Center), docked.entry()) {
            tracing::warn!("replays: the viewport was not docked: {err}");
            self.renderers.update(|renderers| {
                renderers.remove(&path);
            });
            session.close();
        }
    }

    /// Opens a viewport on a battle the session is playing elsewhere, or brings the open one
    /// forward.
    pub fn watch(self, shared: SharedWindow) {
        self.touched.set_value(true);
        let watch = WatchId(shared.replay_id);
        let docked = Docked::Watched(watch);
        if self.watched.with_untracked(|watched| watched.contains_key(&watch)) {
            if let Err(err) = self.dock.activate(&docked.panel_id()) {
                tracing::warn!("replays: the watched playback could not be shown: {err}");
            }
            return;
        }
        self.watched.update(|watched| {
            watched.insert(watch, shared);
        });
        if let Err(err) = self.dock.add_panel(DockTarget::Region(DockRegion::Center), docked.entry()) {
            tracing::warn!("replays: the watched playback was not docked: {err}");
            self.watched.update(|watched| {
                watched.remove(&watch);
            });
        }
    }

    /// Takes a panel out of the dock. A viewport's session closes with it.
    pub fn remove(self, docked: &Docked) {
        let _ = self.dock.remove(&docked.panel_id());
        self.forget_closed();
    }

    /// Lets go of what the dock no longer holds: a closed tab's replay and what it read, and a
    /// closed viewport's session.
    pub fn forget_closed(self) {
        let shown: HashSet<Docked> =
            self.dock.panels_untracked().iter().filter_map(|entry| Docked::of(&entry.id)).collect();
        let held = |docked: Docked| shown.contains(&docked);
        let released: Vec<PanelControl> = self.replays.with_untracked(|replays| {
            replays
                .iter()
                .filter(|(path, _)| !held(Docked::Replay((*path).clone())))
                .map(|(_, control)| *control)
                .collect()
        });
        if !released.is_empty() {
            self.replays.update(|replays| replays.retain(|path, _| held(Docked::Replay(path.clone()))));
            for control in released {
                control.release();
            }
        }
        let closed: Vec<RendererSession> = self.renderers.with_untracked(|renderers| {
            renderers
                .iter()
                .filter(|(path, _)| !held(Docked::Renderer((*path).clone())))
                .map(|(_, session)| *session)
                .collect()
        });
        if !closed.is_empty() {
            self.renderers.update(|renderers| renderers.retain(|path, _| held(Docked::Renderer(path.clone()))));
            for session in closed {
                session.close();
            }
        }
        if self.watched.with_untracked(|watched| watched.keys().any(|watch| !held(Docked::Watched(*watch)))) {
            self.watched.update(|watched| watched.retain(|watch, _| held(Docked::Watched(*watch))));
        }
    }

    /// Takes the tab of every viewport that moved into a window of its own, leaving its session
    /// to that window. Reactive.
    pub fn release_popped_out(self) {
        let popped: Vec<PathBuf> = self.renderers.with(|renderers| {
            renderers
                .iter()
                .filter(|(_, session)| session.is_open() && session.popped_out.get())
                .map(|(path, _)| path.clone())
                .collect()
        });
        for path in popped {
            // Out of the map first, so the closed tab is not taken for one the reader closed.
            let mut moved = None;
            self.renderers.update(|renderers| moved = renderers.remove(&path));
            if let Some(session) = moved {
                self.popped.update_value(|popped| {
                    popped.retain(|session| session.is_open());
                    popped.push(session);
                });
            }
            let _ = self.dock.remove(&Docked::Renderer(path).panel_id());
        }
    }

    /// Hands every open viewport, docked or in its own window, this end of the collab session.
    pub fn share_session(self) {
        let mut sessions: Vec<RendererSession> =
            self.renderers.with_untracked(|renderers| renderers.values().copied().collect());
        self.popped.update_value(|popped| {
            popped.retain(|session| session.is_open());
            sessions.extend(popped.iter().copied());
        });
        let link = self.shell.collab.link();
        for session in sessions.into_iter().filter(|session| session.is_open()) {
            // One link each: a link holds what its own viewport drew outside a session.
            session.set_collab(link.get_untracked());
        }
    }

    /// Reads `path` again, where a tab is open on it.
    pub fn reparse(self, path: &std::path::Path, cause: ReparseCause) {
        if let Some(control) = self.replays.with_untracked(|replays| replays.get(path).copied()) {
            control.reparse(cause);
        }
    }

    /// Reads every open replay again.
    pub fn reparse_all(self, cause: ReparseCause) {
        let controls: Vec<PanelControl> = self.replays.with_untracked(|replays| replays.values().copied().collect());
        for control in controls {
            control.reparse(cause);
        }
    }

    /// Records the replays a restored layout shows, so their panels find them as they are built.
    pub(crate) fn adopt_restored(self, paths: Vec<PathBuf>) {
        let opened: Vec<(PathBuf, PanelControl)> =
            paths.into_iter().map(|path| (path, self.under_tab(PanelControl::new))).collect();
        self.replays.update(|replays| replays.extend(opened));
    }
}

/// Carries a viewport's ask for an armor viewer to the Armor Viewer tab. A salvo picked out of the
/// viewer's incoming-fire log moves this viewport's playback.
fn view_armor(shell: Shell, session: RendererSession, source: FeedSource, asked: ArmorRequest) {
    match asked {
        ArmorRequest::Show(shown) => {
            let seek = UnsyncCallback::new(move |clock: wows_replays::types::GameClock| session.go_to_clock(clock));
            shell.armor.send(ViewerFeed::Show { source, shown: Box::new(shown), seek: Some(seek) });
            shell.select(crate::shell::tabs::TabKey::App(crate::shell::tabs::AppTab::ArmorViewer));
        }
        // Not shown: playback moving must not pull the reader to the viewer.
        ArmorRequest::Followed(followed) => shell.armor.send(ViewerFeed::Followed { source, followed }),
    }
}

#[cfg(test)]
impl ReplayWorkspace {
    pub fn session_for_test(self, path: &std::path::Path) -> Option<RendererSession> {
        self.renderers.with_untracked(|renderers| renderers.get(path).copied())
    }

    /// Why the replay open at `path` failed to read, once it has.
    pub fn read_failure_for_test(self, path: &std::path::Path) -> Option<String> {
        self.replays.with_untracked(|replays| replays.get(path).and_then(|control| control.failure()))
    }

    pub fn control_for_test(self, path: &std::path::Path) -> Option<PanelControl> {
        self.replays.with_untracked(|replays| replays.get(path).copied())
    }
}
