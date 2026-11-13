//! The main window's chrome: the tab strip, the page the selected tab fills, the status bar, the
//! settings drawer, the command palette, hosted dialogs, messages and file drops.
//!
//! [`Shell`] is the handle every part of the window reaches the chrome through. It is provided by
//! [`AppRoot`] inside the window's message host, so [`Shell::toasts`] always has somewhere to put
//! a message.

mod about;
pub mod collab;
mod crash;
mod dialogs;
mod drop;
mod file_dialog;
mod first_run;
mod game_data_cache;
mod menu;
mod notices;
pub mod palette;
pub mod replay_index;
pub mod settings;
mod startup;
mod status_bar;
pub mod tabs;
mod twitch;
mod update;
mod wows_dir;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use landaire_ui::MotionPreference;
use landaire_ui::TabStripProps;
use landaire_ui::TabStripTabProps;
use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::Toasts;
use landaire_ui::WorkspaceInsetProps;
use tokio::runtime::Runtime;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::ZoomFactor;
use wows_toolkit_viewmodel::settings::keys;
use zgui::component;
use zgui::css;
use zgui::prelude::ClipboardKind;
use zgui::prelude::Clipboards;
use zgui::prelude::try_use_clipboard;
use zgui::prelude::try_use_window;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;

use crate::i18n::LanguageCode;
use crate::icons::AppIconProps;
use crate::services::Services;
use crate::shell::collab::CollabHost;
pub(crate) use crate::shell::dialogs::DialogHostProps;
use crate::shell::drop::DropIntent;
use crate::shell::drop::DropScrimProps;
use crate::shell::drop::DropScrimState;
use crate::shell::game_data_cache::GameDataCacheJobs;
use crate::shell::menu::AppMenuProps;
use crate::shell::palette::CommandPaletteProps;
use crate::shell::palette::PaletteControl;
use crate::shell::replay_index::ReplayIndexJobs;
use crate::shell::settings::SettingsButtonProps;
use crate::shell::settings::SettingsDrawerControl;
use crate::shell::settings::SettingsDrawerProps;
use crate::shell::status_bar::ShellStatusBarProps;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::DirectoryStatsId;
use crate::shell::tabs::DirectoryStatsTab;
use crate::shell::tabs::TabChrome;
use crate::shell::tabs::TabKey;
use crate::shell::twitch::TwitchLink;
use crate::shell::update::UpdateFlow;
use crate::tabs::TabBodyProps;
use crate::tabs::armor_viewer::library::ArmorLibrary;
use crate::theme::SemanticTokensProps;
use crate::theme::palette_choice;
use crate::window_shell::DecoratedWindowProps;
use crate::window_shell::RememberGeometryProps;
use crate::window_shell::main_window_title;
use crate::windows::tactics::TacticsHost;
use crate::zoom::ZoomStep;

pub use crate::shell::dialogs::DialogHandle;
pub use crate::shell::dialogs::Dialogs;
pub use crate::shell::dialogs::HostedDialogProps;

const SHEET: &str = "wt-shell";

/// The message a zoom step says, which the next step's replaces.
const ZOOM_TOAST: &str = "zoom";

const CSS: &str = css!(
    ".wt-shell {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        background-color: var(--lui-color-surface);
        color: var(--lui-color-text);
        font-family: var(--lui-font-family);
        font-size: var(--lui-font-size);
    }
    .wt-shell__strip {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
    }
    .wt-shell__tabs {
        display: flex;
        flex: 1 1 auto;
        min-width: 0;
    }
    .wt-tab-label {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
    }
    .wt-shell__strip-end {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 2px;
        padding: 0 4px;
    }
    .wt-shell__rule {
        flex: none;
        height: 1px;
        background-color: var(--lui-color-border-bright);
    }
    .wt-shell__page {
        position: relative;
        display: flex;
        flex: 1 1 auto;
        min-height: 0;
        min-width: 0;
    }
    .wt-tab-page {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }
    .wt-debug-notice {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        justify-content: center;
        gap: 4px;
        padding: 4px 8px;
        color: var(--wt-color-warn);
        font-weight: 700;
    }"
);

/// What dropping files, a palette entry or another tab asks the replays tab to do. Queued until
/// the replays tab takes it, which it does as it is built and whenever one arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayRequest {
    OpenReplay(PathBuf),
    /// Lists the directory in a folder tab of the replays tab.
    OpenFolder(PathBuf),
    /// Lists a directory from the recent list, or says it is gone and drops it from the list.
    OpenRecentFolder(PathBuf),
    /// Plays the battle back on its minimap.
    RenderReplay(PathBuf),
    /// Renders the battle to a video in the background and puts the file on the clipboard.
    RenderToClipboard(PathBuf),
    /// Reads the open replays again, through a result mapping that has since been written.
    ReparseOpen,
}

impl ReplayRequest {
    /// Whether the replays tab comes forward for it. A clipboard render reports through messages
    /// and a reparse changes only what is already open, so the reader stays where they are.
    const fn shows_replays(&self) -> bool {
        !matches!(self, Self::RenderToClipboard(_) | Self::ReparseOpen)
    }
}

/// The palette and size the window draws in, seeded from the settings once they are read.
#[derive(Clone, Copy)]
pub struct Appearance {
    /// The saved palette.
    pub theme: RwSignal<ThemeChoice, LocalStorage>,
    /// A palette shown in place of the saved one while the settings drawer previews it.
    pub theme_preview: RwSignal<Option<ThemeChoice>, LocalStorage>,
    pub zoom: RwSignal<ZoomFactor, LocalStorage>,
    pub motion: RwSignal<MotionPreference, LocalStorage>,
}

impl Appearance {
    fn new() -> Self {
        Self {
            theme: RwSignal::new_local(ThemeChoice::default()),
            theme_preview: RwSignal::new_local(None),
            zoom: RwSignal::new_local(ZoomFactor::DEFAULT),
            motion: RwSignal::new_local(MotionPreference::Full),
        }
    }

    /// The palette on screen: the preview while there is one, else the saved choice.
    pub fn shown_theme(self) -> ThemeChoice {
        self.theme_preview.get().unwrap_or_else(|| self.theme.get())
    }
}

/// The main window's chrome, as one handle.
#[derive(Clone, Copy)]
pub struct Shell {
    /// The tab strip's value, which is a [`TabKey::value`].
    selected: RwSignal<String, LocalStorage>,
    directory_stats: RwSignal<Vec<DirectoryStatsTab>, LocalStorage>,
    next_directory_stats: StoredValue<u64>,
    /// Every tab's chrome, made as the tab is first shown.
    chrome: RwSignal<HashMap<TabKey, TabChrome>, LocalStorage>,
    /// The tabs shown at least once, which stay built while hidden.
    visited: RwSignal<Vec<TabKey>, LocalStorage>,
    held: RwSignal<Modifiers, LocalStorage>,
    /// Unhides NDA-hidden stats and the raw viewers. Seeded from the settings and flipped for the
    /// session by Ctrl+Shift+D; never written back.
    pub debug_mode: RwSignal<bool, LocalStorage>,
    replay_requests: RwSignal<Vec<ReplayRequest>, LocalStorage>,
    pub appearance: Appearance,
    pub settings_drawer: SettingsDrawerControl,
    /// The game-data cache's maintenance jobs and what they found.
    pub cache: GameDataCacheJobs,
    pub replay_index: ReplayIndexJobs,
    pub twitch: TwitchLink,
    pub palette: PaletteControl,
    pub dialogs: Dialogs,
    /// The tactics board's window, the layouts it starts from and the session it joins.
    pub tactics: TacticsHost,
    /// The window's collaborative session.
    pub collab: CollabHost,
    /// The Armor Viewer's ship data, which the palette lists from any tab.
    pub armor: ArmorLibrary,
    drop_scrim: DropScrimState,
    toasts: Toasts,
    services: Services,
    update: UpdateFlow,
    recent_crash: StoredValue<Option<String>>,
    /// `None` on a platform with no clipboard.
    clipboard: StoredValue<Option<Clipboards>, LocalStorage>,
}

impl Shell {
    fn new(services: Services, appearance: Appearance, toasts: Toasts, clipboard: Option<Clipboards>) -> Self {
        let first = TabKey::App(AppTab::ReplayInspector);
        Self {
            selected: RwSignal::new_local(first.value()),
            directory_stats: RwSignal::new_local(Vec::new()),
            next_directory_stats: StoredValue::new(0),
            chrome: RwSignal::new_local(HashMap::new()),
            visited: RwSignal::new_local(vec![first]),
            held: RwSignal::new_local(Modifiers::NONE),
            debug_mode: RwSignal::new_local(false),
            replay_requests: RwSignal::new_local(Vec::new()),
            appearance,
            settings_drawer: SettingsDrawerControl::new(),
            cache: GameDataCacheJobs::new(),
            replay_index: ReplayIndexJobs::new(),
            twitch: TwitchLink::new(),
            palette: PaletteControl::new(),
            dialogs: Dialogs::new(),
            tactics: TacticsHost::new(),
            collab: CollabHost::new(),
            armor: ArmorLibrary::new(),
            drop_scrim: DropScrimState::new(),
            toasts,
            services,
            update: UpdateFlow::new(),
            recent_crash: StoredValue::new(None),
            clipboard: StoredValue::new_local(clipboard),
        }
    }

    /// The chrome of the window the caller is in. Called from a component's body.
    ///
    /// # Panics
    ///
    /// Outside the main window's shell.
    pub fn current() -> Self {
        use_local_context::<Self>().expect("a component reads the shell from inside the main window")
    }

    /// The window's messages.
    pub fn toasts(self) -> Toasts {
        self.toasts
    }

    /// Whether the shell has been unmounted, for work that can finish after its window has closed.
    pub fn is_unmounted(self) -> bool {
        self.held.try_get_untracked().is_none()
    }

    /// The window's messages, or `None` once the shell has been unmounted.
    pub fn try_toasts(self) -> Option<Toasts> {
        (!self.is_unmounted()).then_some(self.toasts)
    }

    /// The process's services, for a method called from a listener, where no context is found.
    pub fn services(self) -> Services {
        self.services
    }

    /// Puts `text` on the clipboard.
    pub fn copy_text(self, text: String) {
        self.clipboard.with_value(|clipboard| {
            if let Some(clipboard) = clipboard {
                clipboard.set_text(ClipboardKind::Standard, text);
            }
        });
    }

    fn remember_crash(self, report: String) {
        self.recent_crash.set_value(Some(report));
    }

    /// The crash report shown this session, which a bug report carries.
    pub fn recent_crash(self) -> Option<String> {
        self.recent_crash.get_value()
    }

    /// Switches the interface to `code`, and saves it.
    pub fn set_locale(self, code: LanguageCode) {
        let unchanged =
            self.services.settings.read_untracked(|settings| settings.locale.as_deref() == Some(code.as_str()));
        if unchanged == Some(true) {
            return;
        }

        self.services.locale.choose(code.clone());
        self.services.game_data.set_locale(code.clone());
        self.services.settings.edit(keys::LOCALE, |settings| {
            settings.locale = Some(code.as_str().to_owned());
            code.as_str().to_owned()
        });
    }

    /// Adopts what may be shared, and saves it.
    pub fn adopt_data_sharing(self, mode: DataSharingMode) {
        self.services.settings.edit(keys::DATA_SHARING_MODE, |settings| {
            settings.data_sharing = mode;
            mode
        });
        // The older bool beside it, which the egui app reconciles the mode against on load: writing
        // only the mode would have that reconcile undo this choice on its next start.
        self.services.settings.save(keys::SEND_REPLAY_DATA, &mode.shares_anything());
    }

    /// The selected tab.
    pub fn selected(self) -> Option<TabKey> {
        TabKey::parse(&self.selected.get())
    }

    pub fn selected_untracked(self) -> Option<TabKey> {
        TabKey::parse(&self.selected.get_untracked())
    }

    /// Shows `tab`. The strip and the drawer beside it are one surface, so choosing a tab shuts
    /// the drawer.
    pub fn select(self, tab: TabKey) {
        self.settings_drawer.close();
        self.selected.set(tab.value());
    }

    /// Every tab, left to right, with each directory's stats beside the session's.
    pub fn tab_order(self) -> Vec<TabKey> {
        let directories = self.directory_stats.get();
        let mut order = Vec::new();
        for tab in AppTab::ALL {
            order.push(TabKey::App(tab));
            if tab == AppTab::Stats {
                order.extend(directories.iter().map(|directory| TabKey::DirectoryStats(directory.id)));
            }
        }

        order
    }

    /// The directory stats tab `id`, while it is open.
    pub fn directory_stats(self, id: DirectoryStatsId) -> Option<DirectoryStatsTab> {
        self.directory_stats.get().into_iter().find(|tab| tab.id == id)
    }

    /// Opens a stats tab over the replays in `root`, or shows the one already open for it.
    pub fn open_directory_stats(self, root: PathBuf) {
        let root = tabs::canonical_root(&root);
        let open = self.directory_stats.get_untracked().into_iter().find(|tab| tab.root == root);
        if let Some(open) = open {
            self.select(TabKey::DirectoryStats(open.id));
            return;
        }

        let id = DirectoryStatsId(self.next_directory_stats.get_value());
        self.next_directory_stats.update_value(|next| *next += 1);
        self.directory_stats.update(|tabs| tabs.push(DirectoryStatsTab { id, root }));
        self.select(TabKey::DirectoryStats(id));
    }

    /// Drops the tab, which stops its read.
    pub fn close_directory_stats(self, id: DirectoryStatsId) {
        let key = TabKey::DirectoryStats(id);
        self.directory_stats.update(|tabs| tabs.retain(|tab| tab.id != id));
        self.visited.update(|visited| visited.retain(|tab| *tab != key));
        self.chrome.update(|chrome| {
            chrome.remove(&key);
        });
        if self.selected_untracked() == Some(key) {
            self.select(TabKey::App(AppTab::Stats));
        }
    }

    /// The chrome of `tab`, made the first time it is asked for.
    pub fn chrome_of(self, tab: TabKey) -> TabChrome {
        if let Some(chrome) = self.chrome.with_untracked(|chrome| chrome.get(&tab).copied()) {
            return chrome;
        }

        let chrome = TabChrome::new();
        self.chrome.update(|all| {
            all.insert(tab, chrome);
        });
        chrome
    }

    /// The selected tab's chrome, once it has been built. Reactive, so a reader sees a tab's
    /// chrome once its page is first built.
    pub fn selected_chrome(self) -> Option<TabChrome> {
        let tab = self.selected()?;
        self.chrome.with(|chrome| chrome.get(&tab).copied())
    }

    /// The modifier keys held down in the window.
    pub fn held_modifiers(self) -> Modifiers {
        self.held.get()
    }

    fn sync_held(self, modifiers: Modifiers) {
        if self.held.get_untracked() != modifiers {
            self.held.set(modifiers);
        }
    }

    /// Asks the replays tab to carry out `request`, and shows it unless the request runs in the
    /// background.
    pub fn request_replay(self, request: ReplayRequest) {
        let shows = request.shows_replays();
        self.replay_requests.update(|requests| requests.push(request));
        if shows {
            self.select(TabKey::App(AppTab::ReplayInspector));
        }
    }

    /// Takes every waiting request, oldest first, subscribing to the next one.
    pub fn take_replay_requests(self) -> Vec<ReplayRequest> {
        if self.replay_requests.get().is_empty() {
            return Vec::new();
        }
        let mut taken = Vec::new();
        self.replay_requests.update(|requests| taken = std::mem::take(requests));

        taken
    }

    /// Applies `zoom` to the window and saves it.
    pub fn set_zoom(self, zoom: ZoomFactor) {
        let services = self.services;
        self.appearance.zoom.set(zoom);
        services.settings.edit(wows_toolkit_viewmodel::settings::keys::ZOOM_FACTOR, |settings| {
            settings.zoom = zoom;
            zoom.get()
        });
    }

    /// Shows `choice` and saves it.
    pub fn set_theme(self, choice: ThemeChoice) {
        let services = self.services;
        self.appearance.theme_preview.set(None);
        self.appearance.theme.set(choice);
        services.settings.edit(wows_toolkit_viewmodel::settings::keys::THEME, |settings| {
            settings.theme = choice;
            choice
        });
    }

    /// Does what dropping files on the window asked for.
    fn apply_drop(self, intent: DropIntent) {
        match intent {
            DropIntent::OpenReplay(path) => self.request_replay(ReplayRequest::OpenReplay(path)),
            DropIntent::OpenFolder(path) => self.request_replay(ReplayRequest::OpenFolder(path)),
            refused @ (DropIntent::NotAReplay | DropIntent::TooMany) => {
                self.toasts.warn(refused.said());
            }
        }
    }

    /// Notes files dragged over the window, for the scrim.
    pub fn drag_entered(self, paths: Vec<PathBuf>) {
        self.drop_scrim.enter(paths);
    }

    /// Takes the scrim down, for a drag that left the window.
    pub fn drag_left(self) {
        self.drop_scrim.leave();
    }

    /// Takes files dropped on the window.
    pub fn dropped(self, paths: Vec<PathBuf>) {
        self.drop_scrim.leave();
        spawn_local(async move {
            let intent = drop::classify(paths).await;
            self.apply_drop(intent);
        });
    }
}

/// Everything in the main window: the theme, the messages and the shell.
#[component]
pub fn AppRoot(
    /// The shared runtime, or `None` when it could not be started.
    runtime: Option<Arc<Runtime>>,
) -> impl IntoView {
    Services::provide(runtime);
    let appearance = Appearance::new();
    provide_local_context(appearance);

    let choice = Signal::derive_local(move || palette_choice(appearance.shown_theme()));
    let shown = Signal::derive_local(move || appearance.shown_theme());
    let motion = Signal::derive_local(move || appearance.motion.get());
    let zoom = Signal::derive_local(move || crate::zoom::to_window(appearance.zoom.get()));

    view! {
        ThemeRoot(choice = choice, motion = motion, zoom = zoom) {
            SemanticTokens(choice = shown)
            ToastHost {
                DecoratedWindow(title = main_window_title()) {
                    ShellFrame(appearance = appearance)
                }
            }
        }
    }
}

/// The main window's contexts and theme around `children` alone, for a component's tests.
#[cfg(test)]
#[component]
pub(crate) fn ShellHarness(theme: ThemeChoice, children: zgui::view::Children) -> impl IntoView {
    Services::provide(None);
    let appearance = Appearance::new();
    appearance.theme.set(theme);
    provide_local_context(appearance);
    let choice = Signal::derive_local(move || palette_choice(appearance.shown_theme()));
    let shown = Signal::derive_local(move || appearance.shown_theme());

    view! {
        ThemeRoot(choice = choice) {
            SemanticTokens(choice = shown)
            ToastHost {
                HarnessShell(appearance = appearance) {{children.into_view_once()}}
            }
        }
    }
}

#[cfg(test)]
#[component]
fn HarnessShell(appearance: Appearance, children: zgui::view::Children) -> impl IntoView {
    let services = Services::current();
    let toasts = Toasts::current().expect("HarnessShell is placed inside ToastHost");
    provide_local_context(Shell::new(services, appearance, toasts, None));

    view! { column(style = "width: 100%; height: 100%") {{children.into_view_once()}} }
}

/// The shell itself, inside the window's message host.
#[component]
fn ShellFrame(appearance: Appearance) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let toasts = Toasts::current().expect("ShellFrame is placed inside ToastHost, which publishes its messages");
    let shell = Shell::new(services, appearance, toasts, try_use_clipboard());
    provide_local_context(shell);
    #[cfg(test)]
    crate::interaction_tests::capture_shell(shell);

    startup::begin(services, shell);
    shell.tactics.begin(services);
    shell.collab.begin(shell);
    shell.armor.begin(services);
    follow_selection(shell);
    let root = NodeRef::new();
    hear_unfocused_keys(root);

    // A window that loses focus never sees the release of a key held as it went.
    let window_focused = try_use_window().map(|window| window.focused());
    let releasing = RenderEffect::new(move |_| {
        if window_focused.is_some_and(|focused| !focused.get()) {
            shell.held.set(Modifiers::NONE);
        }
    });
    on_cleanup_local(move || drop(releasing));

    let locale = services.locale;
    let on_close_tab = move |id: DirectoryStatsId| UnsyncCallback::new(move |()| shell.close_directory_stats(id));

    view! {
        column(
            class = "wt-shell",
            attr:data-testid = "app-root",
            node_ref = root,
            on:key_down = move |ev| {
                shell.held.set(ev.modifiers);
                let modifiers = ev.modifiers;
                if shell_shortcut(shell, ev, modifiers) {
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            },
            on:key_up = move |ev| shell.held.set(ev.modifiers),
            // A key pressed while another window had focus is only seen on the pointer's events.
            on:pointer_down:capture = move |ev| shell.sync_held(ev.modifiers),
            on:pointer_move:capture = move |ev| shell.sync_held(ev.modifiers),
            on:drag_enter = move |ev| shell.drag_entered(ev.paths.clone()),
            on:drag_leave = move |_| shell.drag_left(),
            on:drop = move |ev| shell.dropped(ev.paths.clone())
        ) {
            row(class = "wt-shell__strip") {
                box(class = "wt-shell__tabs") {
                    TabStrip(value = shell.selected, label = "Tabs", attr:data-testid = "app-tabs") {
                        for tab in move || shell.tab_order(), key = |tab: &TabKey| *tab {
                            {match tab {
                                TabKey::App(app_tab) => AnyView::new(view! {
                                    TabStripTab(value = tab.value(), attr:data-testid = tab.value()) {
                                        row(class = "wt-tab-label") {
                                            AppIcon(glyph = app_tab.glyph())
                                            text {{locale.text(app_tab.label_key())}}
                                        }
                                    }
                                }),
                                TabKey::DirectoryStats(id) => AnyView::new(view! {
                                    TabStripTab(
                                        value = tab.value(),
                                        on_close = on_close_tab(id),
                                        close_label = Signal::derive_local(locale.text("ui.tabs.close_directory_stats")),
                                        attr:data-testid = tab.value()
                                    ) {
                                        row(class = "wt-tab-label") {
                                            AppIcon(glyph = AppTab::Stats.glyph())
                                            text {{move || {
                                                locale.track();
                                                shell.directory_stats(id).map(|tab| tab.label()).unwrap_or_default()
                                            }}}
                                        }
                                    }
                                }),
                            }}
                        }
                    }
                }
                row(class = "wt-shell__strip-end") {
                    SettingsButton(shell = shell)
                    AppMenu(shell = shell)
                }
            }
            // The rule under the strip, in the tone meant to be seen: it separates the chrome
            // from the page rather than two greys meeting.
            box(class = "wt-shell__rule")
            // The drawer covers the page below the strip, not the strip, so the tabs and the gear
            // stay where they are while it is open.
            box(class = "wt-shell__page") {
                WorkspaceInset {
                    TabPages(shell = shell)
                }
                SettingsDrawer(shell = shell)
            }
            ShellStatusBar(shell = shell)
            Show(when = move || shell.debug_mode.get()) {
                row(class = "wt-debug-notice", attr:data-testid = "app-debug-notice") {
                    AppIcon(glyph = wows_toolkit_icons::Glyph::TriangleAlert)
                    text {{locale.text("ui.app.debug_build")}}
                    AppIcon(glyph = wows_toolkit_icons::Glyph::TriangleAlert)
                }
            }
            DropScrim(state = shell.drop_scrim)
            DialogHost(dialogs = shell.dialogs)
            CommandPalette(shell = shell)
            RememberGeometry(kind = wows_toolkit_config::WindowKind::Main)
        }
    }
}

/// Every tab shown so far, each built once and hidden while another is selected, so a tab keeps
/// what the reader did in it.
#[component]
fn TabPages(shell: Shell) -> impl IntoView {
    view! {
        for tab in move || shell.visited.get(), key = |tab: &TabKey| *tab {
            box(
                class = "wt-tab-page",
                attr:data-testid = format!("tab-page-{}", tab.value()),
                style:display = move || (shell.selected() != Some(tab)).then(|| "none".to_owned())
            ) {
                TabPage(shell = shell, tab = tab)
            }
        }
    }
}

/// One tab's content, with its chrome published to it.
#[component]
fn TabPage(shell: Shell, tab: TabKey) -> impl IntoView {
    shell.chrome_of(tab).provide();

    view! { TabBody(tab = tab) }
}

/// Builds a tab the first time it is selected.
fn follow_selection(shell: Shell) {
    let visiting = RenderEffect::new(move |_| {
        let Some(tab) = shell.selected() else { return };
        if !shell.visited.get_untracked().contains(&tab) {
            shell.visited.update(|visited| visited.push(tab));
        }
    });
    on_cleanup_local(move || drop(visiting));
}

/// Lets the shell's own shortcuts work before anything in the window has focus.
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

/// Runs the window-wide shortcut `key` with `modifiers` is, and says whether it was one. Only a
/// zoom step repeats while its key is held.
fn shell_shortcut(shell: Shell, key: &zgui::vocab::KeyEvent, modifiers: Modifiers) -> bool {
    if let Some(step) = ZoomStep::of(key, modifiers) {
        // The zoom waits for the settings, which hold the zoom it steps from.
        let services = shell.services;
        if services.settings.read_untracked(|_| ()).is_some() {
            let zoom = step.applied_to(shell.appearance.zoom.get_untracked());
            shell.set_zoom(zoom);
            // Said, since the tab with the zoom slider may not be the one open.
            let said = rust_i18n::t!("ui.settings.panel.zoom_changed", factor = format!("{:.1}", zoom.get()));
            shell.toasts().show(landaire_ui::Notice::info(said).keyed(landaire_ui::ToastKey::new(ZOOM_TOAST)));
        }
        return true;
    }

    if key.repeat || !modifiers.primary() || modifiers.alt() {
        return false;
    }

    let Key::Character(text) = &key.key_without_modifiers else {
        return false;
    };
    match (modifiers.shift(), text.to_lowercase().as_str()) {
        (true, "d") => {
            shell.debug_mode.update(|debug| *debug = !*debug);
            true
        }
        // In the Armor Viewer, ctrl+p goes straight to the ship list.
        (false, "p") if shell.selected_untracked() == Some(TabKey::App(AppTab::ArmorViewer)) => {
            shell.palette.open_mode(wows_toolkit_viewmodel::palette::PaletteMode::ArmorShips);
            true
        }
        // Both, as the egui app takes both.
        (false, "k" | "p") => {
            shell.palette.open_root();
            true
        }
        (false, ",") => {
            shell.settings_drawer.toggle();
            true
        }
        _ => false,
    }
}
