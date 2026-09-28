use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Root;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::command::Command;
use gpui_kit::component::command::CommandState;
use gpui_kit::component::form::Form;
use gpui_kit::component::form::field;
use gpui_kit::component::form::v_form;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::Select;
use gpui_kit::component::select::SelectEvent;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::slider::SliderState;
use gpui_kit::component::tab::Tab;
use gpui_kit::component::tab::TabBar;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use std::rc::Rc;

use crate::armor_viewer::ArmorViewerPane;
use crate::game_data_cache;
use crate::palette::PaletteAction;
use crate::palette::PaletteEntry;
use crate::player_tracker::PlayerTrackerEvent;
use crate::player_tracker::PlayerTrackerView;
use crate::replay_inspector::GameDataStatus;
use crate::replay_inspector::InspectorSettings;
use crate::replay_inspector::MissingBuild;
use crate::replay_inspector::ReplayInspectorView;
use crate::replay_inspector::view::ConstantsUnfit;
use crate::replay_inspector::view::GameDataMissing;
use crate::replay_inspector::view::ReplaySettingsChanged;

/// Where the menu's links go, as the egui menu bar sends them.
const ISSUES_URL: &str = "https://github.com/landaire/wows-toolkit/issues/new/choose";
const DISCORD_URL: &str = "https://discord.gg/RJXjXHUj7rh";
/// What the PR figures are credited to, which the About window links.
const PR_INFO_URL: &str = "https://wows-numbers.com/personal/rating";
const PROJECT_URL: &str = "https://github.com/landaire/wows-toolkit";

/// Asks the window's own app view to check, for a menu item that has no handle
/// on it.
fn check_for_update_from_menu(window: &mut Window, cx: &mut gpui_kit::App) {
    let Some(app) =
        window.root::<Root>().flatten().and_then(|root| root.read(cx).view().clone().downcast::<App>().ok())
    else {
        return;
    };
    app.update(cx, |app, cx| app.check_for_update(Asked::ByHand, window, cx));
}

/// One running job on the status strip: what it is, and how far it has got.
fn status_job(named: String, progress: Option<(u64, u64)>) -> AnyElement {
    let counted = progress
        .filter(|(_, total)| *total > 0)
        .map(|(done, total)| t!("ui.app.status_of", done = done, total = total).into_owned());

    h_flex().gap_1().items_center().child(Spinner::new().xsmall()).child(named).children(counted).into_any_element()
}

/// What the download offer says: a line per build, and what the whole selection
/// would come to.
///
/// Each line names the version and build, how many replays are waiting on it,
/// and what the repository has for it. `plan` is `None` when the repository could
/// not be asked, which leaves the availability out rather than claiming one.
fn describe_missing_builds(
    missing: &[MissingBuild],
    plan: Option<&wows_data_mgr::download_repo::DownloadPlan>,
) -> Vec<(u32, String)> {
    missing
        .iter()
        .map(|build| {
            let named = t!(
                "ui.dialogs.download_build_row",
                version = build.version.clone().unwrap_or_else(|| "?".to_owned()),
                build = build.build
            );
            let needed = t!("ui.dialogs.download_replays_needing", count = build.replays);
            let said = plan
                .and_then(|plan| plan.resolved.iter().find(|resolved| resolved.requested_build == build.build))
                .map(|resolved| availability_said(&resolved.availability));
            let row = match said {
                Some(said) => format!("{named} -- {needed} -- {said}"),
                None => format!("{named} -- {needed}"),
            };
            (build.build, row)
        })
        .collect()
}

/// How one build's availability reads in the offer.
fn availability_said(availability: &wows_data_mgr::download_repo::RemoteAvailability) -> String {
    use wows_data_mgr::download_repo::RemoteAvailability;

    match availability {
        RemoteAvailability::Exact => t!("ui.dialogs.download_availability_exact").into_owned(),
        RemoteAvailability::Nearest { version, build } => {
            t!("ui.dialogs.download_availability_nearest", version = version, build = build).into_owned()
        }
        RemoteAvailability::Unpublished => t!("ui.dialogs.download_availability_unpublished").into_owned(),
        RemoteAvailability::Unreachable => t!("ui.dialogs.download_availability_unreachable").into_owned(),
    }
}

/// Whether a dragged or dropped path is a replay, which is the only thing this
/// window takes.
fn is_replay(path: &std::path::Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("wowsreplay")
}

/// Who asked for the update check.
///
/// A startup check that finds nothing says nothing; one the reader asked for says
/// so, because silence would read as a check that never ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Asked {
    AtStartup,
    ByHand,
}

/// Offers the release, and installs it if the reader says so.
///
/// Installing replaces this executable and restarts it, so the dialog is the last
/// thing this process does.
fn offer_update(
    release: wows_toolkit_viewmodel::update::Release,
    asset_url: String,
    proxy: String,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let tag = release.tag_name.clone();
    let notes = release.body.clone().unwrap_or_default();
    // Built once, outside the builder: that closure runs on every draw of the
    // dialog, and the release it describes does not change under one.
    let announced = t!("ui.dialogs.update_message", tag = tag).into_owned();
    let described =
        if notes.trim().is_empty() { announced.clone() } else { format!("{announced}\n\n{}", notes.trim()) };

    window.open_alert_dialog(cx, move |alert, _window, _cx| {
        let asset_url = asset_url.clone();
        let proxy = proxy.clone();
        let described = described.clone();
        let installing_said = announced.clone();
        alert
            .title(t!("ui.windows.update_available").into_owned())
            .description(described)
            .ok_text(t!("ui.buttons.install_update").into_owned())
            .show_cancel(true)
            .on_ok(move |_event, window, cx| {
                let asset_url = asset_url.clone();
                let proxy = proxy.clone();
                crate::toast::info(installing_said.clone(), window, cx);
                let installing = crate::update::install(asset_url, proxy, cx);
                window
                    .spawn(cx, async move |cx| {
                        // Only a failure returns: a successful install restarts
                        // the app from the new executable.
                        if let Err(reason) = installing.await {
                            let _ = cx.update(|window, cx| {
                                crate::toast::failed(reason, window, cx);
                            });
                        }
                    })
                    .detach();
                true
            })
    });
}

/// Who made this, what it is built on, and where to look next.
///
/// The same lines as the egui About window (`app.rs`'s `build_about_window`),
/// with this app's own version at the top: a bug report that names a build is
/// worth more than one that does not.
fn show_about(window: &mut Window, cx: &mut gpui_kit::App) {
    window.open_alert_dialog(cx, move |alert, _window, _cx| {
        alert
            .title(t!("ui.windows.about").into_owned())
            .description(
                [
                    format!("{} v{}", wows_toolkit_config::APP_NAME, env!("CARGO_PKG_VERSION")),
                    t!("ui.labels.made_by").into_owned(),
                    t!("ui.labels.credits").into_owned(),
                    format!("{} {PR_INFO_URL}", t!("ui.labels.pr_credits")),
                    PROJECT_URL.to_string(),
                ]
                .join(
                    "

",
                ),
            )
            .ok_text(t!("ui.buttons.view_github").into_owned())
            .show_cancel(true)
            .on_ok(move |_event, _window, cx| {
                cx.open_url(PROJECT_URL);
                true
            })
    });
}

/// How long a failure may be before it is worth a window rather than a toast.
const TOO_LONG_TO_TOAST: usize = 160;

/// What a palette mode backed by the replay index says when it has no rows.
const NOTHING_INDEXED: &str = "ui.palette.nothing_indexed";

/// What a finished background job has to say, until the next draw says it.
///
/// Kept rather than said where it is decided: a job finishes without a window,
/// and a message needs one. This is the port's answer to the egui app's own
/// catch-all (`app.rs`'s task-completion error arm): a job the reader started and
/// then walked away from still reports where they are.
struct JobReport {
    said: String,
    level: ReportLevel,
}

enum ReportLevel {
    Ok,
    Warn,
    Failed,
}

impl JobReport {
    fn ok(said: String) -> Self {
        Self { said, level: ReportLevel::Ok }
    }

    fn warn(said: String) -> Self {
        Self { said, level: ReportLevel::Warn }
    }

    fn failed(said: String) -> Self {
        Self { said, level: ReportLevel::Failed }
    }

    fn say(self, window: &mut Window, cx: &mut gpui_kit::App) {
        match self.level {
            ReportLevel::Ok => crate::toast::ok(self.said, window, cx),
            ReportLevel::Warn => crate::toast::warn(self.said, window, cx),
            ReportLevel::Failed => {
                // A sentence is a toast; a chain of causes is what a bug report
                // is made of, and a toast can be neither read at length nor
                // copied.
                if self.said.len() > TOO_LONG_TO_TOAST || self.said.lines().count() > 1 {
                    crate::notices::show_error(self.said, window, cx);
                } else {
                    crate::toast::failed(self.said, window, cx);
                }
            }
        }
    }
}

/// The states the app reports with a message that stays until they are fixed.
const STUCK_WOWS_DIR: &str = "wows-dir";
const STUCK_TWITCH_TOKEN: &str = "twitch-token";
use crate::runtime;
use crate::search::SearchEvent;
use crate::search::SearchView;
use crate::settings::DEFAULT_ZOOM;
use crate::settings::GpuiSettings;
use crate::settings::MAX_ZOOM;
use crate::settings::MIN_ZOOM;
use crate::settings_store;
use crate::stats::load::SessionData;
use crate::stats::view::StatsView;
use crate::theme;
use crate::ui::selectable;
use crate::unpacker::view::UnpackerView;
use gpui_kit::component::spinner::Spinner;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use wows_toolkit_config::ReplayExportFormat;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::twitch::Token as TwitchToken;
use wows_toolkit_viewmodel::twitch::keys as twitch_keys;

/// What Twitch made of the stored credential.
///
/// Only Twitch can answer this, so it is a reply rather than a guess: a
/// credential that is present is not therefore working.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TwitchStatus {
    /// Nothing stored to check.
    Unset,
    /// Stored, and the reply has not come back.
    Checking,
    /// Twitch took it.
    Accepted { who: String },
    /// Twitch refused it: expired, revoked, or never valid.
    Refused,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppTab {
    ReplayInspector,
    Stats,
    PlayerTracker,
    Search,
    ArmorViewer,
    Unpacker,
    Settings,
}

impl AppTab {
    /// Left-to-right order, following the egui app's own dock order
    /// (`app.rs`'s `DockState::new`): replays first, settings last. The tabs
    /// This is every tab the egui dock opens, in its order. `Tab::ModManager`
    /// is deliberately absent: the egui app never adds it to its dock either.
    pub const ALL: [AppTab; 7] = [
        AppTab::ReplayInspector,
        AppTab::Stats,
        AppTab::PlayerTracker,
        AppTab::Search,
        AppTab::ArmorViewer,
        AppTab::Unpacker,
        AppTab::Settings,
    ];

    pub const fn label_key(self) -> &'static str {
        match self {
            AppTab::ReplayInspector => "ui.tabs.replay_parser",
            AppTab::Stats => "ui.tabs.stats",
            AppTab::PlayerTracker => "ui.tabs.player_tracker",
            AppTab::Search => "ui.tabs.search",
            AppTab::ArmorViewer => "ui.tabs.armor_viewer",
            AppTab::Unpacker => "ui.tabs.unpacker",
            AppTab::Settings => "ui.tabs.settings",
        }
    }

    /// The tab's name in the reader's own language.
    pub fn label(self) -> String {
        t!(self.label_key()).into_owned()
    }

    /// The glyph in front of the label, as the egui tab strip carries
    /// (`app.rs`'s `icon_t` pairs).
    pub fn glyph(self) -> &'static str {
        match self {
            AppTab::ReplayInspector => crate::icons::ARCHIVE,
            AppTab::Stats => crate::icons::CHART_BAR,
            AppTab::PlayerTracker => crate::icons::DETECTIVE,
            AppTab::Search => crate::icons::MAGNIFYING_GLASS,
            AppTab::ArmorViewer => crate::icons::SHIELD,
            AppTab::Unpacker => crate::icons::ARCHIVE,
            AppTab::Settings => crate::icons::GEAR_FINE,
        }
    }
}

/// The language combo, and the menu under it.
const LANGUAGE_COMBO_WIDTH: Pixels = px(200.);

/// The label column in the settings form. Wide enough for the longest label
/// the tab draws without wrapping it.
const SETTINGS_LABEL_WIDTH: Pixels = px(260.);

/// The corner a settings card is drawn with.
const SECTION_RADIUS: Pixels = px(6.);

/// The monitored-channel field: a Twitch login, not a sentence.
const TWITCH_CHANNEL_WIDTH: Pixels = px(240.);

/// Where a credential with the permissions this needs is issued.
///
/// Chatterino's login page, which is what the egui app opens: it mints a
/// chat-scoped token, so the toolkit does not have to host a login of its own.
const TWITCH_TOKEN_URL: &str = "https://chatterino.com/client_login";

/// One entry in the language combo. A local newtype: the language list is
/// `wt_translations`' and `SearchableListItem` is the component library's.
#[derive(Clone)]
struct LanguageItem(&'static wt_translations::LanguageInfo);

impl SearchableListItem for LanguageItem {
    type Value = &'static str;

    fn title(&self) -> SharedString {
        SharedString::from(self.0.native_name)
    }

    fn value(&self) -> &Self::Value {
        &self.0.code
    }
}

fn language_index(locale: Option<&str>) -> usize {
    let code = locale.unwrap_or("en");
    wt_translations::SUPPORTED_LANGUAGES.iter().position(|lang| lang.code == code).unwrap_or(0)
}

/// Load status of the settings snapshot fetched from the shared config DB.
enum SettingsState {
    Loading,
    /// Boxed: the settings are far larger than the other variants, and this
    /// enum is a field of the app itself.
    Loaded(Box<GpuiSettings>),
    Failed(String),
}

pub struct App {
    active_tab: AppTab,
    settings: SettingsState,
    /// The zoom the slider shows. Seeded from `settings.zoom` once the DB
    /// load completes, updated live as the slider moves, and written back so
    /// it survives a restart the way the egui slider's does.
    zoom: f32,
    /// Which palette is on screen. Seeded from the shared setting, then
    /// changed from the Settings tab; a zoom change re-applies the theme, so
    /// it has to be kept rather than re-read.
    theme: ThemeChoice,
    /// What the last credential paste did, shown beside the button. `None`
    /// before anything has been pasted this session.
    twitch_paste: Option<Result<String, String>>,
    twitch_channel_input: Entity<InputState>,
    /// The chat poll, owned so it stops with the app rather than outliving it.
    _twitch_poll: Option<Task<()>>,
    zoom_slider: Entity<SliderState>,
    /// Replay Inspector tab: the file browser plus the per-replay dock.
    /// Starts its background directory scan once `apply_settings` knows the
    /// WoWs directory.
    replay_inspector: Entity<ReplayInspectorView>,
    /// App-wide debug-mode flag, seeded from `AppPreferences.debug_mode` in
    /// `apply_settings`, then flippable at runtime via the global Ctrl+Shift+D
    /// shortcut (`toggle_debug_mode`), matching the egui app's
    /// `app.rs:1898-1909`. Never written back to the DB -- the toggle only
    /// overrides the setting for the running session. Pushed down into
    /// `replay_inspector` on every change; there is no per-tab enable UI.
    debug_mode: bool,
    /// Root focus target for the global Ctrl+Shift+D handler
    /// (`Self::render`'s `on_key_down`): focused once at startup so the
    /// shortcut works before any other element claims focus. Key events from
    /// a later-focused descendant (e.g. an open replay tab) still bubble up
    /// through this element, so the shortcut keeps working regardless of
    /// what has focus -- matching egui's window-wide shortcut.
    focus_handle: FocusHandle,
    /// The Armor Viewer tab: ship sidebar + 3D viewport. See
    /// `armor_viewer::ArmorViewerPane`.
    armor_pane: Entity<ArmorViewerPane>,
    /// Set once `armor_pane.load_game_data` has been kicked off, so the
    /// `replay_inspector` game-data observer (`Self::new`) triggers it
    /// exactly once -- see `Self::poll_armor_game_data`.
    armor_game_data_requested: bool,
    /// Whether the Stats roundup has been handed the build's art.
    stats_game_data_requested: bool,
    /// What Twitch made of the stored credential, which is the only thing
    /// that can say whether it still works.
    twitch_status: TwitchStatus,
    /// The command palette's own list and query state. Built once: what it
    /// offers does not depend on what is on screen.
    palette: Entity<CommandState>,
    palette_entries: Rc<Vec<PaletteEntry>>,
    /// The Unpacker tab: build selector, VFS browsers and the extraction
    /// queue. Runs its own VFS load per build, independent of the replay
    /// inspector's game-data cache, since it needs only the package tree.
    unpacker: Entity<UnpackerView>,
    /// The Stats tab: session filters over the per-ship aggregate.
    stats: Entity<StatsView>,
    /// The Player Tracker tab: everyone met, from the replay index.
    player_tracker: Entity<PlayerTrackerView>,
    /// The Search tab: a query over the replay index.
    search: Entity<SearchView>,
    /// Settings tab text fields. Held so an edit can be read back and the
    /// saved value can be shown when the tab first renders.
    wows_dir_input: Entity<InputState>,
    proxy_input: Entity<InputState>,
    /// Where the game-data cache is kept. Empty shows the default location as
    /// its placeholder, which is what an empty setting means.
    cache_dir_input: Entity<InputState>,
    /// The game-data cache: what is there, what is stale, and what job is
    /// running against it.
    cache: game_data_cache::CacheState,
    /// What finished jobs have to say, until a draw says it. A list rather than
    /// one message: two jobs can finish between draws, and the first of them is
    /// not worth less than the second.
    job_said: Vec<JobReport>,
    /// The builds already put to the reader as missing, so a walk that runs
    /// again does not ask about the same ones twice.
    offered_builds: std::collections::BTreeSet<u32>,
    /// Whether the published result mappings have been checked this session. The
    /// egui app throttles its own check to one per half hour for the same reason:
    /// the mapping changes when the game does, not while the app is open.
    constants_checked: bool,
    /// The capture-point layouts a tactics board picks its modes from, read
    /// from the cache both apps write. Empty until the read lands, and empty on
    /// a machine that has never indexed a replay, which is a board with maps
    /// and no modes rather than no board.
    cap_layouts: wows_replay_insights::cap_layout::CapLayoutDb,
    cap_layouts_requested: bool,
    /// Whether the game-data cache has been tidied this session.
    cache_maintained: bool,
    /// The files being dragged over the window, while any are. What the scrim
    /// says is which of them would open, or that only one may.
    hovering_files: Option<Vec<PathBuf>>,
    /// The name this app appears under to the peers in a session. Written
    /// back to the row the Replay Inspector's own session popover reads.
    collab_name_input: Entity<InputState>,
    /// How far an index build has got, while one is running.
    index_progress: Option<crate::replay_index::IndexProgress>,
    /// What an index build reported when it stopped. Cleared when the next
    /// one starts.
    index_outcome: Option<String>,
    /// Set when an index build should stop, so a long one can be abandoned
    /// without waiting for the whole directory.
    index_cancel: Option<Arc<AtomicBool>>,
    _index_build: Option<Task<()>>,
    /// Backing state for the settings tab's language combo.
    language_select: Entity<SelectState<SearchableVec<LanguageItem>>>,
    /// Whether the directory in the field is one an install could be in. The
    /// field says so, and the Settings tab itself carries the mark.
    wows_dir_invalid: bool,
    settings_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let zoom_slider =
            cx.new(|_| SliderState::new().min(MIN_ZOOM).max(MAX_ZOOM).step(0.05).default_value(DEFAULT_ZOOM));
        let replay_inspector = cx.new(|cx| ReplayInspectorView::new(window, cx));
        let armor_pane = cx.new(|cx| ArmorViewerPane::new(window, cx));
        let unpacker = cx.new(|cx| UnpackerView::new(window, cx));
        let stats = cx.new(|cx| StatsView::new(window, cx));
        let player_tracker = cx.new(|cx| PlayerTrackerView::new(window, cx));
        let search = cx.new(|cx| SearchView::new(window, cx));
        let wows_dir_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.settings.wows.directory_hint").to_string()));
        let twitch_channel_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(t!("ui.settings.twitch.monitored_channel").to_string()));
        let proxy_input = cx.new(|cx| InputState::new(window, cx).placeholder("http://host:port"));
        let collab_name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.collab.display_name_hint").to_string()));
        let cache_dir_input = cx.new(|cx| {
            let default = wows_toolkit_config::game_data_dump_base()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default();
            InputState::new(window, cx).placeholder(default)
        });
        let languages: SearchableVec<LanguageItem> =
            SearchableVec::new(wt_translations::SUPPORTED_LANGUAGES.iter().map(LanguageItem).collect::<Vec<_>>());
        let language_select =
            cx.new(|cx| SelectState::new(languages, Some(IndexPath::new(language_index(None))), window, cx));
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        // The Armor Viewer reuses the replay inspector's preloaded game data
        // (see `poll_armor_game_data`'s doc comment) rather than running a
        // second VFS/`GameParams` load for the same build; observing here
        // catches the `Loading` -> `Ready` transition whenever it lands,
        // including if it already landed before this tab is ever opened.
        let subscription = cx.observe_in(&replay_inspector, window, |this, _replay_inspector, window, cx| {
            this.poll_armor_game_data(window, cx);
        });
        // A column toggle or the listing's collapse is a preference, so the
        // tab says when one changed and the row is written here.
        // A viewport asked for an armor viewer on one of the battle's
        // ships: the tab it lives in is this view's to switch to.
        let show_armor_requested = cx.subscribe(&replay_inspector, |this, _view, event, cx| {
            let crate::replay_inspector::view::ShowArmorRequested { param_index, display_name, hits, incoming } = event;
            let (param_index, display_name) = (param_index.clone(), display_name.clone());
            let (hits, incoming) = (hits.clone(), incoming.clone());
            this.active_tab = AppTab::ArmorViewer;
            this.armor_pane.update(cx, |pane, cx| pane.show_with_hits(param_index, display_name, hits, incoming, cx));
            cx.notify();
        });

        // A salvo picked out of the incoming-fire log is a moment in the
        // battle, which the playback behind the viewer moves to.
        let armor_seek = cx.subscribe_in(&armor_pane, window, |this, _pane, event, window, cx| {
            let crate::armor_viewer::pane::SeekRequested(clock) = event;
            let clock = *clock;
            this.replay_inspector.update(cx, |view, cx| view.seek_following_playback(clock, window, cx));
        });

        // The followed ship took more hits, which the open viewer shows
        // without pulling the reader away from what they are watching.
        let armor_followed = cx.subscribe(&replay_inspector, |this, _view, event, cx| {
            let crate::replay_inspector::view::ArmorFollowed { hits, health } = event;
            let (hits, health) = (hits.clone(), *health);
            this.armor_pane.update(cx, |pane, cx| pane.follow_hits(hits, health, cx));
        });

        let replay_settings_changed = cx.subscribe(&replay_inspector, |this, _view, event, cx| {
            let ReplaySettingsChanged(settings) = event;
            let settings = settings.clone();
            this.edit_replay_settings(cx, move |replay| *replay = settings);
        });
        // A directory of old replays needs game data this machine may not have.
        let game_data_missing = cx.subscribe_in(&replay_inspector, window, |this, _view, event, window, cx| {
            let GameDataMissing(missing) = event;
            this.offer_missing_game_data(missing.clone(), window, cx);
        });
        // A mapping that does not fit means the commit this app last checked at
        // is no answer: forget it so the newest published one is fetched rather
        // than the repository reported unchanged.
        let constants_unfit =
            cx.subscribe_in(&replay_inspector, window, |this, _view, _event: &ConstantsUnfit, window, cx| {
                this.forget_constants_commit(window, cx);
            });
        let wows_dir_edited = cx.subscribe_in(&wows_dir_input, window, Self::on_wows_dir_edited);
        let search_event = cx.subscribe_in(&search, window, Self::on_search_event);
        // A "find matches" button on a tracker row asks a question the Search
        // tab answers, so the tracker names the query and the app shows it.
        let tracker_event = cx.subscribe_in(&player_tracker, window, |this, _tracker, event, window, cx| {
            let PlayerTrackerEvent::SearchFor(query) = event;
            this.run_search(query.clone(), window, cx);
        });
        let proxy_edited = cx.subscribe(&proxy_input, Self::on_proxy_edited);
        let twitch_channel_edited = cx.subscribe(&twitch_channel_input, Self::on_twitch_channel_edited);
        // The desktop switching between light and dark is followed while the
        // theme is "System", which is what that choice means; the egui app gets
        // this from `ThemePreference::System`.
        let watched = cx.entity().downgrade();
        let appearance_changed = window.observe_window_appearance(move |window, cx| {
            let Some(app) = watched.upgrade() else { return };
            app.update(cx, |this, cx| {
                if this.theme != ThemeChoice::System {
                    return;
                }
                theme::apply_egui_theme(this.theme, this.zoom, window, cx);
                cx.notify();
            });
        });
        let cache_dir_edited = cx.subscribe(&cache_dir_input, Self::on_cache_dir_edited);
        let collab_name_edited = cx.subscribe(&collab_name_input, Self::on_collab_name_edited);
        // `Confirm(None)` is the cleared-selection case, which this combo
        // cannot produce: it always holds a language.
        let language_chosen = cx.subscribe_in(&language_select, window, |this, _state, event, window, cx| {
            let SelectEvent::Confirm(Some(code)) = event else { return };
            this.set_locale((*code).to_string(), window, cx);
        });

        Self {
            theme: ThemeChoice::default(),
            twitch_paste: None,
            twitch_channel_input,
            _twitch_poll: None,
            active_tab: AppTab::ReplayInspector,
            settings: SettingsState::Loading,
            zoom: DEFAULT_ZOOM,
            zoom_slider,
            replay_inspector,
            debug_mode: false,
            focus_handle,
            armor_pane,
            armor_game_data_requested: false,
            stats_game_data_requested: false,
            twitch_status: TwitchStatus::Unset,
            palette: cx.new(|cx| CommandState::new(window, cx)),
            palette_entries: Rc::new(crate::palette::entries()),
            unpacker,
            stats,
            player_tracker,
            search,
            wows_dir_input,
            proxy_input,
            cache_dir_input,
            cache: game_data_cache::CacheState::default(),
            offered_builds: std::collections::BTreeSet::new(),
            constants_checked: false,
            cap_layouts: wows_replay_insights::cap_layout::CapLayoutDb::default(),
            cap_layouts_requested: false,
            cache_maintained: false,
            hovering_files: None,
            job_said: Vec::new(),
            collab_name_input,
            index_progress: None,
            index_outcome: None,
            index_cancel: None,
            _index_build: None,
            language_select,
            wows_dir_invalid: false,
            settings_scroll: ScrollHandle::new(),
            _subscriptions: vec![
                subscription,
                show_armor_requested,
                armor_followed,
                armor_seek,
                replay_settings_changed,
                game_data_missing,
                constants_unfit,
                wows_dir_edited,
                proxy_edited,
                twitch_channel_edited,
                appearance_changed,
                cache_dir_edited,
                collab_name_edited,
                search_event,
                tracker_event,
                language_chosen,
            ],
        }
    }

    /// Looks for a newer release, and offers to install one.
    ///
    /// Asked at startup when the setting says to, and from the menu at any time.
    /// A check nobody asked for says nothing when this build is the newest one:
    /// the egui app reports "up to date" only for a manual check too.
    pub(crate) fn check_for_update(&mut self, asked: Asked, window: &mut Window, cx: &mut Context<Self>) {
        let proxy = self.proxy_url();
        let found = crate::update::check(proxy.clone(), cx);
        cx.spawn_in(window, async move |this, cx| {
            let found = found.await;
            let _ = this.update_in(cx, |_this, window, cx| match found {
                crate::update::Found::Newer { release, asset_url } => {
                    offer_update(release, asset_url, proxy, window, cx)
                }
                crate::update::Found::UpToDate => {
                    if asked == Asked::ByHand {
                        crate::toast::ok(t!("ui.messages.app_up_to_date").into_owned(), window, cx);
                    }
                }
                crate::update::Found::Failed(reason) => {
                    if asked == Asked::ByHand {
                        crate::toast::failed(t!("ui.messages.update_check_failed").into_owned(), window, cx);
                    } else {
                        tracing::warn!("the update check failed: {reason}");
                    }
                }
            });
        })
        .detach();
    }

    /// What is running, along the bottom of the window.
    ///
    /// The egui app has a status panel that names every background task and its
    /// progress (`app.rs`'s `build_bottom_panel`); this names the two the app owns
    /// -- a game-data cache job and an index build -- so neither runs invisibly.
    /// A tab's own work (a directory scan, a replay parse) is still reported by
    /// that tab and not here.
    fn status_strip(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let mut jobs: Vec<AnyElement> = Vec::new();

        if let Some(job) = self.cache.running {
            let named = match job {
                game_data_cache::CacheJob::Checking => t!("ui.app.status_cache_checking"),
                game_data_cache::CacheJob::Validating => t!("ui.app.status_cache_validating"),
                game_data_cache::CacheJob::Downloading => t!("ui.app.status_cache_downloading"),
                game_data_cache::CacheJob::Planning => t!("ui.app.status_cache_planning"),
            };
            jobs.push(status_job(named.into_owned(), self.cache.progress.map(|at| (at.done, at.total))));
        }
        if let Some(step) = self.index_progress {
            jobs.push(status_job(t!("ui.app.status_indexing").into_owned(), Some((step.done, step.total))));
        }

        if jobs.is_empty() {
            return None;
        }

        Some(
            h_flex()
                .id("app-status-strip")
                .test_support()
                .flex_none()
                .gap_3()
                .items_center()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(crate::theme::text_dim())
                .children(jobs)
                .into_any_element(),
        )
    }

    /// Notes the files now being dragged over the window.
    pub(crate) fn note_hovering_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.hovering_files.as_deref() == Some(paths.as_slice()) {
            return;
        }
        self.hovering_files = Some(paths);
        cx.notify();
    }

    /// Takes the scrim down, for a drag that has left the window or landed.
    pub(crate) fn forget_hovering_files(&mut self, cx: &mut Context<Self>) {
        if self.hovering_files.take().is_some() {
            cx.notify();
        }
    }

    /// The scrim over the window while files are being dragged onto it.
    ///
    /// The egui app draws the same thing (`app.rs`'s `ui_file_drag_and_drop`):
    /// without it a drag has nothing to say it will be taken, and a reader
    /// dragging two files learns only after letting go that one was expected.
    pub(crate) fn drop_scrim(&self) -> Option<AnyElement> {
        let hovering = self.hovering_files.as_deref()?;
        let replays: Vec<&PathBuf> = hovering.iter().filter(|path| is_replay(path)).collect();
        let said = match replays.as_slice() {
            // The drop will say the same, rather than the scrim promising
            // something and the drop refusing it.
            [] => t!("ui.messages.drop_not_a_replay").into_owned(),
            [one] => {
                let named = one.file_name().unwrap_or(one.as_os_str()).to_string_lossy().into_owned();
                t!("ui.app.drop_to_load", file = named).into_owned()
            }
            _ => t!("ui.messages.drop_one_at_a_time").into_owned(),
        };

        Some(
            div()
                .id("app-drop-scrim")
                .test_support()
                .aria_label(said.clone())
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                // theme-exempt: white on this overlay's own dark scrim, as the
                // egui overlay is.
                .bg(gpui_kit::black().opacity(0.75))
                .child(div().text_lg().text_color(gpui_kit::white()).child(said))
                .into_any_element(),
        )
    }

    /// Reports what the last run's crash left behind, once.
    ///
    /// The egui app shows the same text in its Crash Detected window with a copy
    /// button and a link to file an issue; this reports it and puts it on the
    /// clipboard, which is what a reader does with it next.
    pub(crate) fn report_last_crash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(report) = crate::logging::take_crash_report() else { return };
        let copied = report.clone();
        window.open_alert_dialog(cx, move |alert, _window, _cx| {
            let copied = copied.clone();
            alert
                .title(t!("ui.windows.crash_detected").into_owned())
                .description(format!("{}\n\n{}", t!("ui.dialogs.crash_message"), copied.trim()))
                .ok_text(t!("ui.buttons.copy").into_owned())
                .show_cancel(true)
                .on_ok(move |_event, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                    crate::toast::ok(t!("ui.stats.copied").into_owned(), window, cx);
                    true
                })
        });
    }

    /// Puts the newest log file on the clipboard, for a bug report.
    pub(crate) fn copy_latest_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = crate::logging::newest_log() else {
            crate::toast::warn(t!("ui.messages.log_not_found").into_owned(), window, cx);
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                let bytes = contents.len() / 1024;
                let named = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                cx.write_to_clipboard(ClipboardItem::new_string(contents));
                crate::toast::ok(t!("ui.messages.log_copied", bytes = bytes, file = named).into_owned(), window, cx);
            }
            Err(err) => {
                crate::toast::failed(t!("ui.messages.log_copy_failed", error = err).into_owned(), window, cx);
            }
        }
    }

    /// Offers to fetch the game data a directory of replays needs and this
    /// machine does not have.
    ///
    /// Ports the egui app's download prompt (`app.rs`'s `GameDataDownloadPrompt`):
    /// without it the rows are listed and silently refuse to open. Each build is a
    /// row the reader can untick, with what the repository has for it beside it,
    /// and the whole selection's object count under them.
    fn offer_missing_game_data(&mut self, missing: Vec<MissingBuild>, window: &mut Window, cx: &mut Context<Self>) {
        // Each build is put to the reader once a session: a walk that runs again
        // after a download reports whatever it still cannot read, and offering
        // that again would be a loop rather than a question.
        let missing: Vec<MissingBuild> =
            missing.into_iter().filter(|build| !self.offered_builds.contains(&build.build)).collect();
        if missing.is_empty() || self.cache.busy() {
            return;
        }
        let Some(base) = self.cache_base() else {
            crate::toast::warn(t!("ui.dialogs.download_plan_no_cache_dir").into_owned(), window, cx);
            return;
        };

        // What the repository actually publishes for each build, and how much the
        // whole selection would fetch, before the reader is asked to commit to
        // it: an offer to download data that was never published is worse than
        // saying so.
        crate::toast::info(t!("ui.dialogs.download_plan_pending").into_owned(), window, cx);
        let builds: Vec<(u32, Option<String>)> =
            missing.iter().map(|build| (build.build, build.version.clone())).collect();
        let held = window.window_handle();
        let asked = base.clone();
        game_data_cache::plan(
            |this: &mut Self| &mut this.cache,
            base,
            builds,
            self.proxy_url(),
            &cx.entity(),
            cx,
            move |this, outcome, cx| {
                let plan = match outcome {
                    game_data_cache::CacheOutcome::Planned { plan } => Some(plan),
                    // The repository could not be asked. The builds are still
                    // missing and the data may still be there, so the offer is
                    // made without the per-build detail rather than withheld.
                    outcome => {
                        if let game_data_cache::CacheOutcome::Failed(reason) = &outcome {
                            tracing::warn!("game data: the download could not be planned: {reason}");
                        }
                        this.job_said.push(JobReport::warn(t!("ui.dialogs.download_plan_failed").into_owned()));
                        None
                    }
                };
                let proxy = this.proxy_url();
                let rows = describe_missing_builds(&missing, plan.as_ref());
                let total = plan.as_ref().map(|plan| plan.unique_missing_objects);
                // Everything is ticked to begin with, because the reader was asked
                // about these builds for the replays waiting on them.
                let ticked: Rc<std::cell::RefCell<std::collections::BTreeSet<u32>>> =
                    Rc::new(std::cell::RefCell::new(missing.iter().map(|build| build.build).collect()));
                let entity = cx.entity().downgrade();
                let _ = held.update(cx, move |_root: gpui_kit::AnyView, window, cx| {
                    window.open_dialog(cx, move |dialog, _window, _cx| {
                        let entity = entity.clone();
                        let missing = missing.clone();
                        let base = asked.clone();
                        let proxy = proxy.clone();
                        let ticked = Rc::clone(&ticked);
                        let picking = Rc::clone(&ticked);

                        dialog
                            .title(t!("ui.windows.download_game_data").into_owned())
                            .child(
                                v_flex()
                                    .id("download-offer")
                                    .gap_1()
                                    .max_w(px(560.))
                                    .child(
                                        div().text_sm().child(t!("ui.dialogs.download_game_data_intro").into_owned()),
                                    )
                                    .children(rows.iter().cloned().map(|(build, said)| {
                                        let picking = Rc::clone(&picking);
                                        let on = picking.borrow().contains(&build);
                                        Checkbox::new(("download-build", build as usize))
                                            .label(said)
                                            .checked(on)
                                            .on_click(move |checked, _window, _cx| {
                                                let mut picked = picking.borrow_mut();
                                                if *checked {
                                                    picked.insert(build);
                                                } else {
                                                    picked.remove(&build);
                                                }
                                            })
                                    }))
                                    .children(total.map(|count| {
                                        div().text_xs().text_color(theme::text_dim()).child(
                                            t!("ui.dialogs.download_objects_to_fetch", count = count).into_owned(),
                                        )
                                    })),
                            )
                            .footer(
                                h_flex()
                                    .gap_2()
                                    .justify_end()
                                    .child({
                                        let entity = entity.clone();
                                        let missing = missing.clone();
                                        let base = base.clone();
                                        let proxy = proxy.clone();
                                        let ticked = Rc::clone(&ticked);
                                        Button::new("download-offer-ok")
                                            .primary()
                                            .label(t!("ui.buttons.download").into_owned())
                                            .small()
                                            .on_click(move |_event, window, cx: &mut gpui_kit::App| {
                                                let wanted: Vec<MissingBuild> = {
                                                    let picked = ticked.borrow();
                                                    missing
                                                        .iter()
                                                        .filter(|build| picked.contains(&build.build))
                                                        .cloned()
                                                        .collect()
                                                };
                                                window.close_dialog(cx);
                                                let Some(entity) = entity.upgrade() else { return };
                                                // Every build in the dialog is
                                                // answered for, ticked or not:
                                                // the walk that follows a
                                                // download reports the unticked
                                                // ones as missing again, and
                                                // offering them back is a loop
                                                // rather than a question.
                                                let offered = missing.clone();
                                                entity.update(cx, |this, _cx| {
                                                    for build in &offered {
                                                        this.offered_builds.insert(build.build);
                                                    }
                                                });
                                                if wanted.is_empty() {
                                                    return;
                                                }
                                                let base = base.clone();
                                                let proxy = proxy.clone();
                                                entity.update(cx, |this, cx| {
                                                    this.fetch_missing_game_data(wanted, base, proxy, cx)
                                                });
                                            })
                                    })
                                    .child(
                                        Button::new("download-offer-cancel")
                                            .label(t!("ui.buttons.cancel").into_owned())
                                            .small()
                                            .on_click(|_event, window, cx: &mut gpui_kit::App| window.close_dialog(cx)),
                                    ),
                            )
                    });
                });
            },
        );
    }

    /// Fetches the named builds into the game-data cache, reporting through the
    /// same progress line and messages the Settings section uses.
    fn fetch_missing_game_data(
        &mut self,
        missing: Vec<MissingBuild>,
        base: std::path::PathBuf,
        proxy: String,
        cx: &mut Context<Self>,
    ) {
        // Spent now the reader has said yes. A refused offer leaves the build
        // worth asking about again.
        for build in &missing {
            self.offered_builds.insert(build.build);
        }
        let builds: Vec<wows_data_mgr::download_repo::BuildUpdateStatus> = missing
            .iter()
            .map(|build| wows_data_mgr::download_repo::BuildUpdateStatus {
                build: build.build,
                version: build.version.clone().unwrap_or_default(),
            })
            .collect();

        game_data_cache::download(
            |this: &mut Self| &mut this.cache,
            base,
            builds,
            proxy,
            &cx.entity(),
            cx,
            |this, outcome, cx| {
                let fetched =
                    matches!(&outcome, game_data_cache::CacheOutcome::Downloaded { fetched, .. } if *fetched > 0);
                this.cache_job_finished(outcome, cx);
                // What the listing could not read, it can now: which replays
                // have a preview and which builds are missing are both decided as
                // the directory is walked, so it is walked again.
                if fetched {
                    this.replay_inspector.update(cx, |view, cx| view.relist(cx));
                }
            },
        );
    }

    /// Adopts a language: saved, applied to the catalogue every `t!` reads,
    /// and pushed into the tabs that translate their own rows so they are
    /// rebuilt in it rather than waiting for a restart.
    pub(crate) fn set_locale(&mut self, code: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.settings_mut() else { return };
        if settings.locale.as_deref() == Some(code.as_str()) {
            return;
        }
        settings.locale = Some(code.clone());
        settings_store::save(keys::LOCALE, &code, cx);
        // `rust_i18n` keeps its locale per crate, so this crate's own chrome
        // and the shared strings are set separately.
        rust_i18n::set_locale(&code);
        wows_toolkit_viewmodel::set_locale(&code);

        self.replay_inspector.update(cx, |view, cx| view.set_locale(Some(code), cx));
        // Placeholders are stored on their input states, so the ones the
        // reader sees are rewritten rather than left in the old language.
        self.refresh_placeholders(window, cx);
        cx.notify();
    }

    /// Rewrites every placeholder this tab owns in the current language.
    ///
    /// A placeholder is held by its `InputState`, not re-read per frame, so a
    /// language change has to push the new text in.
    fn refresh_placeholders(&self, window: &mut Window, cx: &mut Context<Self>) {
        let pairs: [(&Entity<InputState>, &str); 3] = [
            // The cache directory's placeholder is a path, not a phrase, so a
            // language change leaves it alone.
            (&self.wows_dir_input, "ui.settings.wows.directory_hint"),
            (&self.twitch_channel_input, "ui.settings.twitch.monitored_channel"),
            (&self.proxy_input, "ui.settings.app.proxy_url_hint"),
        ];
        for (input, key) in pairs {
            let text = t!(key).into_owned();
            input.update(cx, |state, cx| state.set_placeholder(text, window, cx));
        }
    }

    /// Hands the Stats roundup the build's art, once, so its achievements
    /// draw their own icons rather than a generic glyph.
    ///
    /// The same `LoadedGameData` the replay inspector already opened; this
    /// never starts a load of its own.
    /// Asks once whether a newer result mapping has been published for the build
    /// now loaded, and writes it if so.
    ///
    /// Battle results are read through that mapping; a port that only ever read
    /// the file decoded last year's keys against this year's results. The commit
    /// row is shared with the egui app, so whichever app checks first spares the
    /// other the request.
    /// Drops the commit the published mappings were last checked at, and checks
    /// again.
    fn forget_constants_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings_mut() {
            settings.constants_commit = None;
        }
        settings_store::save(keys::CONSTANTS_FILE_COMMIT, &None::<String>, cx);
        self.constants_checked = false;
        self.poll_constants_check(window, cx);
    }

    fn poll_constants_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.constants_checked {
            return;
        }
        let GameDataStatus::Ready(loaded) = self.replay_inspector.read(cx).game_data_status() else {
            return;
        };
        let Some(known) = self.settings().map(|settings| settings.constants_commit.clone()) else { return };
        self.constants_checked = true;

        let build = loaded.build();
        let checked = crate::constants::check_latest(build, known, cx);

        cx.spawn_in(window, async move |this, cx| {
            let outcome = checked.await;
            let _ = this.update_in(cx, |this, window, cx| match outcome {
                crate::constants::Checked::Written { build, commit } => {
                    tracing::info!(build, "constants: the newest result mapping was written");
                    if let Some(settings) = this.settings_mut() {
                        settings.constants_commit = Some(commit.clone());
                    }
                    // Stored as a nullable string, which is what the egui app
                    // reads: absent means never checked.
                    settings_store::save(keys::CONSTANTS_FILE_COMMIT, &Some(commit), cx);
                    crate::toast::info(t!("ui.replay.constants_latest_written").into_owned(), window, cx);
                    this.replay_inspector.update(cx, |view, cx| view.reparse_open_replays(window, cx));
                }
                crate::constants::Checked::UpToDate => {}
                crate::constants::Checked::Failed(reason) => {
                    // Said once, as the egui app says it once: the mappings that
                    // are cached still read, so this is a notice rather than a
                    // failure to work.
                    tracing::warn!(%reason, "constants: the published mappings could not be checked");
                    crate::toast::warn(
                        t!("ui.replay.constants_check_failed", reason = reason).into_owned(),
                        window,
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    /// Tidies the game-data cache once a session.
    ///
    /// After the settings have landed, because where the cache is is one of them.
    /// Reads the capture-point layouts a tactics board picks its modes from.
    ///
    /// Once a session, off the database both apps write them to: a board asked
    /// for before this lands offers its maps with no modes, which is what a
    /// machine that has never indexed a replay offers anyway.
    fn poll_cap_layouts(&mut self, cx: &mut Context<Self>) {
        if self.cap_layouts_requested {
            return;
        }
        let Some(pool) = settings_store::pool(cx) else { return };
        let Some(runtime) = runtime::runtime(cx) else { return };
        self.cap_layouts_requested = true;

        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    runtime.handle().block_on(async move {
                        wows_replay_insights::cap_layout::CapLayoutDb::load_from_db(&pool).await
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.cap_layouts = read;
                cx.notify();
            });
        })
        .detach();
    }

    fn poll_cache_maintenance(&mut self, cx: &mut Context<Self>) {
        if self.cache_maintained {
            return;
        }
        let Some(base) = self.cache_base() else { return };
        self.cache_maintained = true;
        game_data_cache::maintain(base, cx);
    }

    fn poll_stats_game_data(&mut self, cx: &mut Context<Self>) {
        if self.stats_game_data_requested {
            return;
        }
        let GameDataStatus::Ready(loaded) = self.replay_inspector.read(cx).game_data_status() else {
            return;
        };
        self.stats_game_data_requested = true;
        self.stats.update(cx, |stats, cx| stats.set_game_data(loaded.vfs(), cx));
    }

    /// Forwards the replay inspector's preloaded game data to the Armor
    /// Viewer pane the first time BOTH it has reached `GameDataStatus::Ready`
    /// AND the Armor Viewer tab has been opened, so `ArmorViewerPane::
    /// load_game_data` -- which reads the ~167 MiB `content/assets.bin` and
    /// builds the ship catalog/icons -- runs exactly once per session, lazily
    /// on first visit to the tab, with the SAME `Arc<LoadedGameData>` the
    /// replay inspector already loaded (never a second `GameDataCache`/VFS/
    /// `GameParams` load for the same build). A user who never opens the
    /// Armor Viewer never pays that cost. Called from the `cx.observe`
    /// subscription set up in `Self::new` (which fires on every
    /// replay-inspector notification), from the tab bar's `on_click` handler
    /// (`Self::render`) whenever the Armor Viewer tab is selected, and,
    /// redundantly but harmlessly, from `apply_settings` in case the
    /// observer's first notification races the settings load.
    fn poll_armor_game_data(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.poll_stats_game_data(cx);
        self.poll_constants_check(window, cx);
        self.poll_cache_maintenance(cx);
        self.poll_cap_layouts(cx);
        if self.armor_game_data_requested {
            return;
        }
        if self.active_tab != AppTab::ArmorViewer {
            return;
        }
        if let GameDataStatus::Ready(loaded) = self.replay_inspector.read(cx).game_data_status() {
            self.armor_game_data_requested = true;
            self.armor_pane.update(cx, |pane, cx| pane.load_game_data(loaded, window, cx));
        }
    }

    /// Store the settings snapshot loaded from the shared config DB. Called
    /// once from `main.rs` after the async load completes; read-only for the
    /// lifetime of the session (no write-back). Also kicks off the replay
    /// inspector's background directory scan and game-data cache, now that
    /// the WoWs directory is known.
    /// The Replay Inspector tab's view, so state the tab owns (its grouping,
    /// for one) can be asserted without reaching through the rendered frame.
    /// Test-only: the app itself reaches the field directly.
    #[cfg(test)]
    pub(crate) fn replay_inspector(&self) -> &Entity<ReplayInspectorView> {
        &self.replay_inspector
    }

    /// The Search tab, so its hover and preview behaviour can be asserted
    /// without reaching through the rendered frame. Test-only.
    #[cfg(test)]
    pub(crate) fn search(&self) -> &Entity<SearchView> {
        &self.search
    }

    /// Starts what the Player Tracker needs from the config database as soon
    /// as it opens: the shared notes, and the Twitch chat poll. Its index
    /// aggregates wait until the tab is first shown
    /// (`PlayerTrackerView::load_index_once`).
    pub fn start_player_tracker(&mut self, pool: sqlx::sqlite::SqlitePool, cx: &mut Context<Self>) {
        self.start_twitch_poll(pool.clone(), cx);
        self.player_tracker.update(cx, |tracker, cx| tracker.load_notes(pool, cx));
    }

    /// Starts the chat poll again, for a credential or channel that has just
    /// changed.
    ///
    /// The running task holds the old one, so dropping it is what stops polling a
    /// channel the reader has moved off. The egui app re-polls on the same two
    /// edits (`task/networking.rs:746`); without this the change waits for a
    /// restart.
    fn restart_twitch_poll(&mut self, cx: &mut Context<Self>) {
        let Some(pool) = settings_store::pool(cx) else { return };
        self._twitch_poll = None;
        self.start_twitch_poll(pool, cx);
    }

    /// Polls the watched channel's chat while a credential is stored.
    ///
    /// The observations go to the shared database, which is where the roster
    /// chip reads them from and where the egui app writes its own. Without a
    /// credential there is nothing to poll and the task is not started: the
    /// chip then shows whatever the other app collected.
    fn start_twitch_poll(&mut self, pool: sqlx::sqlite::SqlitePool, cx: &mut Context<Self>) {
        let Some(settings) = self.settings_mut() else { return };
        let Some(token) = settings.twitch_token.clone() else {
            self.twitch_status = TwitchStatus::Unset;
            return;
        };
        let channel = settings.twitch_channel.clone();
        let proxy = settings.proxy_url.clone();
        let who = token.username().to_string();
        self.twitch_status = TwitchStatus::Checking;
        cx.notify();

        self._twitch_poll = Some(cx.spawn(async move |this, cx| {
            let client = match crate::http::client(&proxy, reqwest::redirect::Policy::default()) {
                Ok(client) => client,
                Err(err) => {
                    tracing::warn!("twitch: no client to poll with: {err}");
                    return;
                }
            };

            let session = match crate::runtime::spawn(cx, {
                let client = client.clone();
                async move { crate::twitch::Session::open(&token, &channel, client).await }
            })
            .await
            {
                Ok(Ok(session)) => session,
                Ok(Err(err)) => {
                    tracing::warn!("twitch: the stored credential is not usable: {err}");
                    let _ = this.update_in(cx, |this, window, cx| this.twitch_refused(window, cx));
                    return;
                }
                Err(err) => {
                    tracing::warn!("twitch: the credential check did not complete: {err}");
                    let _ = this.update_in(cx, |this, window, cx| this.twitch_refused(window, cx));
                    return;
                }
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.twitch_status = TwitchStatus::Accepted { who };
                crate::toast::resolved(STUCK_TWITCH_TOKEN, window, cx);
                cx.notify();
            });
            let session = std::sync::Arc::new(session);

            loop {
                let polled = crate::runtime::spawn(cx, {
                    let session = std::sync::Arc::clone(&session);
                    let pool = pool.clone();
                    async move { session.poll_once(&pool, jiff::Timestamp::now()).await }
                })
                .await;

                match polled {
                    Ok(Ok(seen)) => tracing::debug!("twitch: recorded {seen} chat observation(s)"),
                    // A poll that fails says nothing about the next one: the
                    // channel may simply have gone offline.
                    Ok(Err(err)) => tracing::warn!("twitch: a chat poll failed: {err}"),
                    Err(err) => tracing::warn!("twitch: a chat poll did not complete: {err}"),
                }

                cx.background_executor().timer(crate::twitch::poll_interval()).await;
            }
        }));
    }

    /// Adopts a theme: saved, and applied to what is on screen.
    fn set_theme(&mut self, choice: ThemeChoice, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_setting(keys::THEME, cx, |settings| {
            settings.theme = choice;
            choice
        });
        self.theme = choice;
        theme::apply_egui_theme(choice, self.zoom, window, cx);
    }

    /// Where the replays are, as the listing resolves it.
    ///
    /// `None` until the game directory is set, which is what a board says when
    /// asked to read replays it cannot find.
    fn replays_dir(&self) -> Option<std::path::PathBuf> {
        let wows_dir = self.settings().map(|settings| settings.wows_dir.clone())?;
        if wows_dir.is_empty() {
            return None;
        }
        Some(crate::replay_inspector::browser_view::resolve_replays_dir(std::path::Path::new(&wows_dir)))
    }

    /// The version the install is on, which is what a ship's ranges are read
    /// at. `None` until the game directory is set and its preferences read.
    fn installed_version(&self) -> Option<wowsunpack::data::Version> {
        let wows_dir = self.settings().map(|settings| settings.wows_dir.clone())?;
        if wows_dir.is_empty() {
            return None;
        }
        crate::replay_inspector::load::installed_version(std::path::Path::new(&wows_dir))
    }

    /// Opens a tactics board in a window of its own.
    ///
    /// Its own window rather than a tab, as the egui board is: a board is drawn
    /// beside the battle it is about, not instead of it.
    pub(crate) fn open_tactics_board(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let game_data = self.replay_inspector.read(cx).game_data();
        let layouts = self.cap_layouts.clone();
        let replays_dir = self.replays_dir();
        let version = self.installed_version();
        let board = cx.new(|cx| {
            let mut board = crate::tactics::TacticsBoard::new(game_data, layouts, window, cx);
            board.set_install(replays_dir, version, cx);
            board
        });
        let options =
            crate::window_shell::options(wows_toolkit_config::WindowKind::TacticsBoard, board.read(cx).title(), cx);
        let opened = cx.open_window(options, move |window, cx| {
            crate::window_shell::remember(wows_toolkit_config::WindowKind::TacticsBoard, window, cx);
            // Through a `Shell` so the board's own toasts and dialogs are drawn
            // in its window rather than only in the one the tabs live in.
            let view: AnyView = cx.new(|_cx| crate::window_shell::Shell::new(board)).into();
            cx.new(|cx| gpui_kit::component::Root::new(view, window, cx))
        });
        if let Err(err) = opened {
            tracing::warn!("tactics board: the window could not be opened: {err}");
            crate::toast::failed(t!("ui.tactics.window_failed").into_owned(), window, cx);
        }
    }

    /// Opens the command palette over the window.
    pub(crate) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = Rc::clone(&self.palette_entries);
        self.open_palette_with(entries, window, cx);
    }

    /// Fills the palette with one of the cascading modes and reopens it.
    ///
    /// Read once on entering the mode rather than per keystroke: the palette
    /// filters what it was given as the reader types, and a query per keystroke
    /// against a year of battles is slower than the typing.
    fn enter_palette_mode(&mut self, mode: crate::palette::PaletteMode, window: &mut Window, cx: &mut Context<Self>) {
        if mode == crate::palette::PaletteMode::ArmorShips {
            let entries = self.armor_ship_entries(cx);
            // Its own empty message: a ship list is empty because no build is
            // loaded, which has nothing to do with what is indexed.
            self.open_palette_mode_with(entries, "ui.palette.no_ship_catalogue", window, cx);
            return;
        }

        let Some(pool) = settings_store::pool(cx) else { return };
        let Some(runtime) = runtime::runtime(cx) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    runtime.handle().block_on(async move { crate::palette::mode_entries(mode, &pool).await })
                })
                .await;
            let _ =
                this.update_in(cx, |this, window, cx| this.open_palette_mode_with(read, NOTHING_INDEXED, window, cx));
        })
        .detach();
    }

    /// Every ship the loaded build has armor for, as palette rows.
    ///
    /// Empty when no build is loaded, which the palette says rather than opening
    /// an empty list.
    fn armor_ship_entries(&self, cx: &Context<Self>) -> Vec<PaletteEntry> {
        let Some(catalog) = self.armor_pane.read(cx).catalog() else { return Vec::new() };
        catalog
            .ships()
            .map(|ship| PaletteEntry {
                label: format!("{} -- {}", ship.display_name, crate::armor_viewer::catalog::tier_roman(ship.tier)),
                action: PaletteAction::ViewArmor { param_index: ship.param_index.clone() },
            })
            .collect()
    }

    /// Opens the palette over a mode's rows, or says the mode has none.
    fn open_palette_mode_with(
        &mut self,
        entries: Vec<PaletteEntry>,
        when_empty: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entries.is_empty() {
            crate::toast::warn(t!(when_empty).into_owned(), window, cx);
            return;
        }
        self.open_palette_with(Rc::new(entries), window, cx);
    }

    /// Opens the palette over `entries`.
    fn open_palette_with(&mut self, entries: Rc<Vec<PaletteEntry>>, window: &mut Window, cx: &mut Context<Self>) {
        // Reaching for the palette while one is already up must not stack a
        // second over it. Closed rather than counted: a dialog can go by the
        // Escape key or a click outside it without this hearing, so a flag
        // saying whether one is open would eventually say the wrong thing
        // and refuse to open the palette at all.
        window.close_all_dialogs(cx);

        let palette = self.palette.clone();
        let owner = cx.weak_entity();
        // Focused once, when it is first drawn: the palette is a search field
        // and a reader who opened it is about to type.
        let focus_on_mount = std::rc::Rc::new(std::cell::Cell::new(true));

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let palette = palette.clone();
            let entries = Rc::clone(&entries);
            let owner = owner.clone();
            let focus_on_mount = focus_on_mount.clone();
            dialog.close_button(false).p_0().content(move |content, window, cx| {
                if focus_on_mount.replace(false) {
                    let palette = palette.clone();
                    window.defer(cx, move |window, cx| {
                        let handle = palette.read(cx).focus_handle(cx);
                        handle.focus(window, cx);
                    });
                }
                let owner = owner.clone();
                let taken = Rc::clone(&entries);
                content.child(
                    Command::new(&palette)
                        .bordered(false)
                        .placeholder(t!("ui.palette.placeholder").to_string())
                        .max_h(px(400.))
                        .items(crate::palette::items(&entries))
                        .on_confirm(move |index_path, window, cx| {
                            let Some(entry) = taken.get(index_path.row) else { return };
                            let action = entry.action.clone();
                            let _ = owner.update(cx, |this, cx| this.run_palette_action(action, window, cx));
                            window.close_all_dialogs(cx);
                        }),
                )
            })
        });
    }

    /// Does what a palette entry says.
    pub(crate) fn run_palette_action(&mut self, action: PaletteAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            PaletteAction::GoTo(tab) => {
                self.active_tab = tab;
                self.poll_armor_game_data(window, cx);
                if tab == AppTab::PlayerTracker {
                    self.player_tracker.update(cx, |tracker, cx| tracker.load_index_once(cx));
                }
            }
            PaletteAction::SetTheme(choice) => self.set_theme(choice, window, cx),
            PaletteAction::OpenReplayFile => {
                self.replay_inspector.update(cx, |view, cx| view.open_manually(window, cx));
            }
            PaletteAction::SearchFor(query) => self.run_search(query, window, cx),
            PaletteAction::EnterMode(mode) => self.enter_palette_mode(mode, window, cx),
            PaletteAction::ViewArmor { param_index } => {
                let named = self
                    .armor_pane
                    .read(cx)
                    .catalog()
                    .and_then(|catalog| catalog.ships().find(|ship| ship.param_index == param_index))
                    .map(|ship| ship.display_name.clone())
                    .unwrap_or_else(|| param_index.clone());
                self.active_tab = AppTab::ArmorViewer;
                self.poll_armor_game_data(window, cx);
                self.armor_pane
                    .update(cx, |pane, cx| pane.show_with_hits(param_index, named, Vec::new(), Default::default(), cx));
            }
            PaletteAction::CopyLatestLog => self.copy_latest_log(window, cx),
            PaletteAction::RefreshPersistedData => {
                crate::first_run::confirm_refresh_persisted_data(&cx.entity(), window, cx)
            }
            PaletteAction::IndexAllReplays => self.build_replay_index(crate::replay_index::IndexMode::FillGaps, cx),
            PaletteAction::ImportConstants => {
                self.active_tab = AppTab::ReplayInspector;
                self.replay_inspector.update(cx, |view, cx| view.import_constants(window, cx));
            }
            PaletteAction::ContributeAllReplays(ledger) => {
                self.active_tab = AppTab::ReplayInspector;
                self.replay_inspector.update(cx, |view, cx| view.contribute_all(ledger, window, cx));
            }
            PaletteAction::OpenReplayDirectory => {
                self.active_tab = AppTab::ReplayInspector;
                self.replay_inspector.update(cx, |view, cx| view.open_directory(window, cx));
            }
        }
        cx.notify();
    }

    /// Shows the Search tab holding `query`, and runs it.
    fn run_search(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        self.active_tab = AppTab::Search;
        self.search.update(cx, |search, cx| search.run_query(&query, window, cx));
        cx.notify();
    }

    /// The auto-export controls: whether to write a file per battle, in which
    /// format, and where.
    ///
    /// The directory is shown rather than typed: a path typed into a field is
    /// only checked when it loses focus, and one that does not exist reads as
    /// working until the first battle fails to write.
    fn render_auto_export_row(&self, replay: &ReplaySettings, cx: &mut Context<Self>) -> AnyElement {
        let enabled = replay.auto_export_data;
        let chosen = replay.auto_export_format;
        let directory = replay.auto_export_path.clone();
        // A directory that is not there cannot be written to, and the reader
        // has no other way to find that out until a battle is lost.
        let missing = !directory.is_empty() && !std::path::Path::new(&directory).is_dir();

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x_4()
                    .gap_y_2()
                    .items_center()
                    .child(
                        Checkbox::new("auto-export-data")
                            .label(t!("ui.settings.replay.auto_export_data").to_string())
                            .checked(enabled)
                            .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                let checked = *checked;
                                this.edit_replay_settings(cx, |replay| replay.auto_export_data = checked);
                            })),
                    )
                    .child(h_flex().gap_2().children(EXPORT_FORMATS.map(|format| {
                        selectable(
                            ("auto-export-format", format as usize),
                            chosen == format,
                            Button::new(("auto-export-format-button", format as usize))
                                .label(format.as_str().to_string())
                                .compact()
                                .selected(chosen == format)
                                .disabled(!enabled)
                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                    this.edit_replay_settings(cx, |replay| replay.auto_export_format = format);
                                })),
                        )
                    }))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("auto-export-choose")
                            .label(t!("ui.buttons.choose").to_string())
                            .compact()
                            .disabled(!enabled)
                            .on_click(cx.listener(|this, _event, _window, cx| this.choose_export_directory(cx))),
                    )
                    .child(
                        div()
                            .id("settings-auto-export-path")
                            .test_support()
                            .text_sm()
                            .when(missing, |path| path.text_color(rgb(crate::theme::semantic().error)))
                            .child(if directory.is_empty() {
                                t!("ui.settings.not_set").into_owned()
                            } else {
                                directory
                            }),
                    ),
            )
            .into_any_element()
    }

    /// Asks for the directory the per-battle exports are written to.
    fn choose_export_directory(&mut self, cx: &mut Context<Self>) {
        let asked = crate::dialog::pick_folder(&t!("ui.settings.replay.export_path_hint"));
        cx.spawn(async move |this, cx| {
            let Some(directory) = asked.await else { return };
            let directory = directory.to_string_lossy().into_owned();
            let _ = this.update(cx, |this, cx| {
                this.edit_replay_settings(cx, |replay| replay.auto_export_path = directory);
            });
        })
        .detach();
    }

    /// Twitch would not take the stored credential.
    ///
    /// Said out loud rather than only logged: the credential goes stale on its
    /// own schedule, and nothing else on screen would show it.
    fn twitch_refused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.twitch_status = TwitchStatus::Refused;
        crate::toast::stuck(STUCK_TWITCH_TOKEN, t!("ui.messages.twitch_token_invalid").to_string(), window, cx);
        cx.notify();
    }

    /// What the paste button says: what Twitch made of the stored credential,
    /// and what the last paste made of the clipboard. The egui settings tab
    /// names the same four states.
    /// The glyph beside the token button, which is what the egui tab draws
    /// next to the same label.
    fn twitch_status_glyph(&self) -> &'static str {
        if matches!(self.twitch_paste, Some(Err(_))) {
            return crate::icons::X_CIRCLE;
        }
        match self.twitch_status {
            TwitchStatus::Unset => crate::icons::WARNING,
            // Nothing has been proven either way yet, so nothing is claimed.
            TwitchStatus::Checking => crate::icons::CLOCK,
            TwitchStatus::Accepted { .. } => crate::icons::CHECK_CIRCLE,
            TwitchStatus::Refused => crate::icons::X_CIRCLE,
        }
    }

    /// The tone that glyph is drawn in.
    fn twitch_status_tone(&self) -> u32 {
        if matches!(self.twitch_paste, Some(Err(_))) {
            return crate::theme::semantic().error;
        }
        match self.twitch_status {
            TwitchStatus::Unset => crate::theme::semantic().warn,
            TwitchStatus::Checking => crate::theme::semantic().text_dim,
            TwitchStatus::Accepted { .. } => crate::theme::semantic().ok,
            TwitchStatus::Refused => crate::theme::semantic().error,
        }
    }

    fn twitch_paste_label_key(&self) -> &'static str {
        if matches!(self.twitch_paste, Some(Err(_))) {
            return "ui.settings.twitch.paste_token_invalid";
        }
        match self.twitch_status {
            TwitchStatus::Unset => "ui.settings.twitch.paste_token_no_token",
            TwitchStatus::Checking => "ui.settings.twitch.paste_token_unvalidated",
            TwitchStatus::Accepted { .. } => "ui.settings.twitch.paste_token_valid",
            TwitchStatus::Refused => "ui.settings.twitch.paste_token_invalid",
        }
    }

    /// Adopts the session statistics read from the config database. The
    /// expected-values table loaded alongside them also reaches the replay
    /// inspector, which rates each replay's players against it.
    pub fn apply_session_stats(&mut self, data: SessionData, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(table) = data.personal_rating.clone() {
            self.replay_inspector.update(cx, |view, cx| view.set_personal_rating(table, cx));
        }
        self.stats.update(cx, |stats, cx| stats.apply_session(data, window, cx));
        cx.notify();
    }

    pub fn apply_settings(&mut self, settings: GpuiSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom = settings.zoom;
        self.theme = settings.theme;
        let channel = settings.twitch_channel.clone();
        self.twitch_channel_input.update(cx, |state, cx| state.set_value(channel, window, cx));
        self.zoom_slider =
            cx.new(|_| SliderState::new().min(MIN_ZOOM).max(MAX_ZOOM).step(0.05).default_value(settings.zoom));
        let collab_name = settings.collab_display_name.clone();
        let collab_display_name = collab_name.clone();
        self.collab_name_input.update(cx, |state, cx| state.set_value(collab_name, window, cx));
        let cache_dir = settings.game_data_cache_dir.clone();
        let game_data_cache_dir = cache_dir.clone();
        let auto_dump_game_data = settings.auto_dump_game_data;
        let data_sharing = settings.data_sharing;
        self.cache_dir_input.update(cx, |state, cx| state.set_value(cache_dir, window, cx));
        // Whatever is known about the cache belongs to the directory the old
        // settings named, which these may not.
        self.forget_cache_findings(cx);
        let wows_dir = settings.wows_dir.clone();
        let debug_mode = settings.debug_mode;
        let replay_settings = settings.replay.clone();
        let auto_load_latest_replay = settings.auto_load_latest_replay;
        let locale = settings.locale.clone();
        self.debug_mode = debug_mode;
        self.replay_inspector.update(cx, |view, cx| {
            let settings = InspectorSettings {
                wows_dir,
                game_data_cache_dir,
                auto_dump_game_data,
                data_sharing,
                debug_mode,
                replay_settings,
                auto_load_latest_replay,
                locale,
                collab_display_name,
            };
            view.apply_settings(settings, window, cx)
        });
        // The tracker watches the same install for a battle in progress, and
        // resolves its roster against the game data the replay inspector has
        // already opened rather than a second copy.
        let wows_dir = settings.wows_dir.clone();
        let proxy_url = settings.proxy_url.clone();
        self.watch_live_matches(&wows_dir, proxy_url, cx);
        let game_data = self.replay_inspector.read(cx).game_data();
        self.search.update(cx, |search, cx| search.set_game_data(game_data, cx));
        self.armor_pane.update(cx, |pane, cx| pane.apply_armor_defaults(settings.armor_defaults.as_ref(), cx));
        // The shared strings follow the saved language, and the combo shows
        // it, both from the moment the settings land.
        if let Some(code) = settings.locale.as_deref() {
            rust_i18n::set_locale(code);
            wows_toolkit_viewmodel::set_locale(code);
        }
        let language_ix = language_index(settings.locale.as_deref());
        self.language_select
            .update(cx, |state, cx| state.set_selected_index(Some(IndexPath::new(language_ix)), window, cx));

        // Seed the text fields so the tab opens showing what is saved.
        self.wows_dir_input.update(cx, |state, cx| state.set_value(settings.wows_dir.clone(), window, cx));
        self.proxy_input.update(cx, |state, cx| state.set_value(settings.proxy_url.clone(), window, cx));

        let unpacker_dir = settings.wows_dir.clone();
        let output_dir = settings.output_dir.clone();
        self.unpacker.update(cx, |unpacker, cx| {
            unpacker.apply_settings(unpacker_dir, window, cx);
            unpacker.set_output_dir(output_dir, window, cx);
        });
        self.poll_armor_game_data(window, cx);
        self.settings = SettingsState::Loaded(Box::new(settings));
    }

    /// Record that the DB load failed. Called once from `main.rs` in place of
    /// `apply_settings` when either the DB open or the settings load errors.
    pub fn mark_settings_failed(&mut self, reason: String) {
        self.settings = SettingsState::Failed(reason);
    }

    /// The global Ctrl+Shift+D handler (`Self::render`'s `on_key_down`):
    /// flips `debug_mode` and pushes the new value into the replay inspector,
    /// matching the egui app's `app.rs:1898-1909` toggle.
    fn toggle_debug_mode(&mut self, cx: &mut Context<Self>) {
        self.debug_mode = !self.debug_mode;
        let debug_mode = self.debug_mode;
        self.replay_inspector.update(cx, |view, cx| view.set_debug_mode(debug_mode, cx));
        cx.notify();
    }
}

/// The formats a battle can be auto-exported in, in the order the egui
/// combo lists them.
const EXPORT_FORMATS: [ReplayExportFormat; 3] =
    [ReplayExportFormat::Json, ReplayExportFormat::Csv, ReplayExportFormat::Cbor];

/// One settings section: its glyph, name and purpose over a card holding the
/// controls, which is the shape the egui tab draws with `section_header` and
/// `ui.group`.
fn settings_section(
    glyph: &'static str,
    title: String,
    description: String,
    border: Hsla,
    body: impl IntoElement,
) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .child(crate::icons::icon(glyph).text_color(crate::theme::icon_accent()))
                        .child(div().text_sm().font_weight(FontWeight::BOLD).child(title)),
                )
                .child(div().text_xs().text_color(crate::theme::text_dim()).child(description)),
        )
        .child(
            div()
                .w_full()
                .rounded(SECTION_RADIUS)
                .border_1()
                .border_color(border)
                .bg(crate::theme::surface())
                .p_3()
                .child(body),
        )
}

/// The layout every section's controls share: one column, labels beside the
/// control so the whole tab reads down a single edge.
fn settings_form() -> Form {
    v_form().label_layout(Axis::Horizontal).label_width(SETTINGS_LABEL_WIDTH).small()
}

impl App {
    fn render_zoom_row(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(crate::ui::boxed(px(160.), crate::ui::SELECT_SMALL_HEIGHT).child(Slider::new(&self.zoom_slider)))
            .child(div().text_sm().w(px(40.)).child(format!("{:.2}", self.zoom)))
            .child(Button::new("reset-zoom").label(t!("ui.buttons.reset").to_string()).compact().on_click(cx.listener(
                |this, _event: &ClickEvent, window, cx| {
                    this.zoom = DEFAULT_ZOOM;
                    theme::apply_egui_theme(this.theme, this.zoom, window, cx);
                    settings_store::save(keys::ZOOM_FACTOR, &this.zoom, cx);
                    this.zoom_slider.update(cx, |slider, slider_cx| {
                        slider.set_value(DEFAULT_ZOOM, window, slider_cx);
                    });
                    cx.notify();
                },
            )))
    }

    /// Opens a search result in the Replay Inspector and shows that tab, so
    /// the replay the user asked for is what they are looking at.
    fn on_search_event(
        &mut self,
        _search: &Entity<SearchView>,
        event: &SearchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SearchEvent::OpenReplay(path) => self.open_replay_from_search(path.clone(), window, cx),
            SearchEvent::RenderReplay(path) => {
                let path = path.clone();
                self.active_tab = AppTab::ReplayInspector;
                self.replay_inspector.update(cx, |view, cx| view.render_replay(path, window, cx));
                cx.notify();
            }
        }
    }

    /// Opens `path` in the Replay Inspector and brings that tab forward.
    ///
    /// A replay cannot be parsed without the game data its build came from,
    /// so with no WoWs directory set the user is moved to the inspector,
    /// which is where that state is explained, rather than to a tab that
    /// would sit empty with no reason given.
    pub(crate) fn open_replay_from_search(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replay_inspector.update(cx, |view, cx| view.open_replay(path, window, cx));
        self.active_tab = AppTab::ReplayInspector;
        cx.notify();
    }

    fn on_wows_dir_edited(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let path = state.read(cx).value().trim().to_string();
        self.apply_wows_dir(path, window, cx);
    }

    /// Saved on blur or Enter rather than per keystroke, so a half-typed URL
    /// never reaches the database.
    fn on_proxy_edited(&mut self, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let url = state.read(cx).value().trim().to_string();
        let Some(settings) = self.settings_mut() else { return };
        if settings.proxy_url == url {
            return;
        }
        settings.proxy_url = url.clone();
        settings_store::save(keys::PROXY_URL, &url, cx);
    }

    /// Adopts the channel to watch once the edit has settled, and polls it.
    ///
    /// The field was seeded from the row and never written back, so a channel
    /// typed here was watched for the session and forgotten. The poll is restarted
    /// with it, since the running one is reading the old channel.
    fn on_twitch_channel_edited(&mut self, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let channel = state.read(cx).value().trim().to_string();
        let Some(settings) = self.settings_mut() else { return };
        if settings.twitch_channel == channel {
            return;
        }
        settings.twitch_channel = channel.clone();
        settings_store::save(twitch_keys::MONITORED_CHANNEL, &channel, cx);
        self.restart_twitch_poll(cx);
    }

    /// What the last credential paste did. Test-only.
    #[cfg(test)]
    pub(crate) fn twitch_paste_outcome(&self) -> Option<Result<String, String>> {
        self.twitch_paste.clone()
    }

    /// The stored Twitch credential. Test-only.
    #[cfg(test)]
    pub(crate) fn stored_twitch_token(&self) -> Option<&TwitchToken> {
        match &self.settings {
            SettingsState::Loaded(settings) => settings.twitch_token.as_ref(),
            _ => None,
        }
    }

    /// The loaded settings, or `None` while they are still being read or
    /// after the read failed.
    fn settings(&self) -> Option<&GpuiSettings> {
        match &self.settings {
            SettingsState::Loaded(settings) => Some(settings),
            _ => None,
        }
    }

    fn settings_mut(&mut self) -> Option<&mut GpuiSettings> {
        match &mut self.settings {
            SettingsState::Loaded(settings) => Some(settings),
            _ => None,
        }
    }

    /// Applies one edit to the in-memory snapshot and persists it.
    ///
    /// Edits take effect immediately and are written as they happen, as the
    /// egui settings tab does; there is no apply step to forget.
    /// Reads a Twitch credential off the clipboard and stores it.
    ///
    /// The credential is pasted rather than obtained through a browser flow,
    /// which is what the egui app does; what it reports back is which account
    /// the credential belongs to, or which part of the paste was wrong.
    fn paste_twitch_token(&mut self, cx: &mut Context<Self>) {
        let pasted = cx.read_from_clipboard().and_then(|item| item.text());
        let Some(pasted) = pasted else {
            self.twitch_paste = Some(Err(t!("ui.collab.clipboard_empty").into_owned()));
            cx.notify();
            return;
        };

        match pasted.trim().parse::<TwitchToken>() {
            Ok(token) => {
                let who = token.username().to_string();
                self.edit_setting(twitch_keys::TOKEN, cx, |settings| {
                    settings.twitch_token = Some(token.clone());
                    token
                });
                self.twitch_paste = Some(Ok(who));
                self.restart_twitch_poll(cx);
            }
            Err(err) => self.twitch_paste = Some(Err(err.to_string())),
        }
        cx.notify();
    }

    /// Adopts a new cache directory once the edit has settled.
    ///
    /// The measurement belongs to the old directory, so it is dropped rather
    /// than shown against the new one.
    fn on_cache_dir_edited(&mut self, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let dir = state.read(cx).value().trim().to_string();
        let Some(settings) = self.settings_mut() else { return };
        if settings.game_data_cache_dir == dir {
            return;
        }
        settings.game_data_cache_dir = dir.clone();
        settings_store::save(keys::GAME_DATA_CACHE_DIR, &dir, cx);
        self.forget_cache_findings(cx);
    }

    /// Walks the replay directory and indexes what is in it.
    ///
    /// The whole directory rather than what changed: this is the control for
    /// building an index that is not there, or rebuilding one whose rows an
    /// older parse got wrong.
    pub(crate) fn build_replay_index(&mut self, mode: crate::replay_index::IndexMode, cx: &mut Context<Self>) {
        if self.index_cancel.is_some() {
            return;
        }
        let Some(settings) = self.settings() else { return };
        let wows_dir = settings.wows_dir.clone();
        if wows_dir.is_empty() {
            return;
        }
        let Some(pool) = settings_store::pool(cx) else { return };
        let Some(runtime) = runtime::runtime(cx) else { return };
        let Some(game_data) = self.replay_inspector.read(cx).game_data() else { return };

        let root = std::path::Path::new(&wows_dir).join("replays");
        let cancel = Arc::new(AtomicBool::new(false));
        self.index_cancel = Some(Arc::clone(&cancel));
        self.index_outcome = None;
        self.index_progress = Some(crate::replay_index::IndexProgress::default());
        cx.notify();

        let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded();
        cx.spawn(async move |this, cx| {
            use futures::StreamExt as _;
            while let Some(step) = progress_rx.next().await {
                if this
                    .update(cx, |this: &mut Self, cx| {
                        this.index_progress = Some(step);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        self._index_build = Some(cx.spawn(async move |this, cx| {
            let built = cx
                .background_spawn(async move {
                    crate::replay_index::build_index(&runtime, &pool, &root, &game_data, &cancel, mode, |step| {
                        let _ = progress_tx.unbounded_send(step);
                    })
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.index_cancel = None;
                this.index_progress = None;
                let (said, report): (String, fn(String) -> JobReport) = match built {
                    Ok(progress) => {
                        let said = if progress.skipped > 0 {
                            t!(
                                "ui.settings.index.built_with_skipped",
                                indexed = progress.indexed,
                                skipped = progress.skipped,
                                failed = progress.failed
                            )
                            .into_owned()
                        } else {
                            t!("ui.settings.index.built", indexed = progress.indexed, failed = progress.failed)
                                .into_owned()
                        };
                        let report: fn(String) -> JobReport =
                            if progress.failed > 0 { JobReport::warn } else { JobReport::ok };
                        (said, report)
                    }
                    Err(err) => (err.to_string(), JobReport::failed),
                };
                this.index_outcome = Some(said.clone());
                // Said as well as written to the Settings line: an index started
                // from the palette finishes while the reader is somewhere else.
                this.job_said.push(report(said));
                cx.notify();
            });
        }));
    }

    /// Adopts a new session display name once the edit has settled.
    fn on_collab_name_edited(&mut self, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let name = state.read(cx).value().trim().to_string();
        let Some(settings) = self.settings_mut() else { return };
        if settings.collab_display_name == name {
            return;
        }
        settings.collab_display_name = name.clone();
        settings_store::save(keys::COLLAB_DISPLAY_NAME, &name, cx);
    }

    /// Drops everything known about the cache, so the next draw asks again.
    ///
    /// The stale and damaged lists name builds under a particular directory;
    /// carrying them across a change would offer to repair builds that are
    /// not there.
    fn forget_cache_findings(&mut self, cx: &mut Context<Self>) {
        self.cache.forget_stats();
        self.cache.updates.clear();
        self.cache.repair.clear();
        self.cache.failure = None;
        // A fire-chance section that found no geometry found none in the old
        // directory. Kept, it would stay failed until a restart even once the
        // data it needed has been downloaded into the new one.
        wows_replay_insights::fire_chance::sections::clear_fire_section_failures();
        cx.notify();
    }

    /// Where the cache is, as the settings currently point.
    fn cache_base(&self) -> Option<PathBuf> {
        let settings = self.settings()?;
        game_data_cache::base(&settings.game_data_cache_dir)
    }

    fn proxy_url(&self) -> String {
        self.settings().map(|settings| settings.proxy_url.clone()).unwrap_or_default()
    }

    /// Folds a finished cache job back into the tab.
    ///
    /// A clean check or validation is what lets the next check stop at the
    /// tip, so the commit is saved only when nothing needs doing; saving it
    /// after a run that found work would skip the re-check.
    fn cache_job_finished(&mut self, outcome: game_data_cache::CacheOutcome, cx: &mut Context<Self>) {
        match outcome {
            game_data_cache::CacheOutcome::Checked { tip, updates } => {
                let clean = updates.is_empty();
                // A clean result said nothing at all before: the spinner stopped
                // and the reader was left to guess. The egui app reports both
                // answers (`app.rs:2175`).
                self.job_said.push(if clean {
                    JobReport::ok(t!("ui.messages.game_data_up_to_date").into_owned())
                } else {
                    JobReport::warn(t!("ui.messages.game_data_updates_available", count = updates.len()).into_owned())
                });
                self.cache.updates = updates;
                if clean {
                    self.remember_cache_tip(tip, cx);
                }
            }
            game_data_cache::CacheOutcome::Validated { tip, repair } => {
                let clean = repair.is_empty();
                self.job_said.push(if clean {
                    JobReport::ok(t!("ui.messages.game_data_cache_valid").into_owned())
                } else {
                    JobReport::warn(t!("ui.messages.game_data_cache_invalid", count = repair.len()).into_owned())
                });
                self.cache.repair = repair;
                if clean {
                    self.remember_cache_tip(tip, cx);
                }
            }
            game_data_cache::CacheOutcome::Downloaded { fetched, failed } => {
                if fetched > 0 {
                    // What was fetched is no longer stale or damaged, and the
                    // cache is a different size than it was.
                    self.cache.updates.clear();
                    self.cache.repair.clear();
                    self.cache.forget_stats();
                }
                self.job_said.push(if failed.is_empty() {
                    JobReport::ok(t!("ui.messages.game_data_builds_downloaded", count = fetched).into_owned())
                } else {
                    JobReport::failed(t!("ui.messages.game_data_download_failed").into_owned())
                });
                if !failed.is_empty() {
                    self.cache.failure = Some(t!("ui.messages.game_data_download_failed").into_owned());
                }
            }
            // The offer that asked for a plan reads it itself; nothing else asks
            // for one.
            game_data_cache::CacheOutcome::Planned { .. } => {}
            game_data_cache::CacheOutcome::Failed(reason) => {
                self.job_said.push(JobReport::failed(reason));
            }
        }
        cx.notify();
    }

    fn remember_cache_tip(&mut self, tip: String, cx: &mut Context<Self>) {
        self.cache.tip = Some(tip.clone());
        if let Some(settings) = self.settings_mut() {
            settings.game_data_repo_commit = Some(tip.clone());
        }
        settings_store::save(keys::GAME_DATA_REPO_COMMIT, &Some(tip), cx);
    }

    /// The replay index: building it, and how far a build has got.
    ///
    /// Refused without a game directory, since the replays are under it.
    fn render_index_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let has_dir = self.settings().is_some_and(|settings| !settings.wows_dir.is_empty());
        let running = self.index_cancel.is_some();
        let progress = self.index_progress;
        let outcome = self.index_outcome.clone();

        settings_section(
            crate::icons::DATABASE,
            t!("ui.settings.index.heading").into_owned(),
            t!("ui.settings.index.description").into_owned(),
            cx.theme().border,
            settings_form().child(
                field().label(String::new()).child(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    Button::new("index-build")
                                        .label(t!("ui.settings.index.build").to_string())
                                        .compact()
                                        .disabled(!has_dir || running)
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.build_replay_index(crate::replay_index::IndexMode::FillGaps, cx)
                                        })),
                                )
                                // For rows an older parse decoded through
                                // constants that did not fit: nothing else can
                                // correct them (the egui app's Re-index All).
                                .child(
                                    Button::new("index-rebuild")
                                        .label(t!("ui.settings.index.rebuild").to_string())
                                        .compact()
                                        .disabled(!has_dir || running)
                                        .tooltip(t!("ui.settings.index.rebuild_hint").to_string())
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.build_replay_index(crate::replay_index::IndexMode::RefreshAll, cx)
                                        })),
                                )
                                .when(running, |this| {
                                    this.child(Spinner::new()).child(
                                        Button::new("index-stop")
                                            .label(t!("ui.buttons.stop").to_string())
                                            .compact()
                                            .on_click(cx.listener(|this, _event, _window, cx| {
                                                if let Some(cancel) = &this.index_cancel {
                                                    cancel.store(true, Ordering::Relaxed);
                                                }
                                                cx.notify();
                                            })),
                                    )
                                }),
                        )
                        .children(progress.map(|step| {
                            div()
                                .text_xs()
                                .text_color(crate::theme::text_dim())
                                .child(if step.skipped > 0 {
                                    t!(
                                        "ui.settings.index.progress_skipped",
                                        done = step.done,
                                        total = step.total,
                                        skipped = step.skipped,
                                        failed = step.failed
                                    )
                                    .to_string()
                                } else {
                                    t!(
                                        "ui.settings.index.progress",
                                        done = step.done,
                                        total = step.total,
                                        failed = step.failed
                                    )
                                    .to_string()
                                })
                                .into_any_element()
                        }))
                        .children(outcome.map(|text| {
                            div().text_xs().text_color(crate::theme::text_dim()).child(text).into_any_element()
                        })),
                ),
            ),
        )
        .into_any_element()
    }

    /// The game-data cache: what it holds, and the maintenance that keeps it
    /// matching the published repository.
    ///
    /// Everything below the directory needs a cache that is actually there,
    /// so a directory holding no builds shows the toggle and the path and
    /// stops. The measurement is taken once and kept until something changes
    /// it, because it walks every stored object.
    fn render_cache_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let auto_dump = self.settings().is_some_and(|settings| settings.auto_dump_game_data);
        let border = cx.theme().border;

        let mut form = settings_form()
            .child(
                field().label(String::new()).child(
                    Checkbox::new("cache-auto-dump")
                        .label(t!("ui.settings.wows.cache.auto_dump").to_string())
                        .checked(auto_dump)
                        .on_click(cx.listener(move |this, checked: &bool, _window, cx| {
                            let on = *checked;
                            this.edit_setting(keys::AUTO_DUMP_GAME_DATA, cx, |settings| {
                                settings.auto_dump_game_data = on;
                                on
                            });
                        })),
                ),
            )
            .child(
                field().label(t!("ui.settings.wows.cache.directory_label").to_string()).child(
                    h_flex()
                        .gap_2()
                        .child(div().flex_1().child(Input::new(&self.cache_dir_input).id("cache-dir").small().w_full()))
                        .child(
                            Button::new("cache-dir-browse")
                                .icon(IconName::FolderOpen)
                                .label(t!("ui.settings.wows.cache.browse").to_string())
                                .compact()
                                .on_click(
                                    cx.listener(|this, _event, window, cx| this.browse_for_cache_dir(window, cx)),
                                ),
                        ),
                ),
            );

        let Some(base) = self.cache_base() else {
            return settings_section(
                crate::icons::ARCHIVE,
                t!("ui.settings.wows.cache.heading").into_owned(),
                t!("ui.settings.wows.cache.description").into_owned(),
                border,
                form,
            )
            .into_any_element();
        };

        // Measured once per directory, off the UI thread; until it lands the
        // section shows only what does not depend on it.
        if let Some(generation) = self.cache.wants_measure() {
            game_data_cache::measure(base.clone(), generation, cx, |this, measured, generation, cx| {
                this.cache.measured(measured, generation);
                cx.notify();
            });
        }
        let stats = self.cache.stats;

        if let Some(stats) = stats.filter(|stats| stats.version_count > 0) {
            let body = self.render_cache_body(base, stats, cx);
            form = form.child(field().label(String::new()).child(body));
        }

        settings_section(
            crate::icons::ARCHIVE,
            t!("ui.settings.wows.cache.heading").into_owned(),
            t!("ui.settings.wows.cache.description").into_owned(),
            border,
            form,
        )
        .into_any_element()
    }

    /// What a cache holding at least one build offers: its size, the jobs
    /// that check it, and whatever those jobs found.
    fn render_cache_body(
        &mut self,
        base: PathBuf,
        stats: wows_data_mgr::dump::CacheStats,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let busy = self.cache.busy();
        let running = self.cache.running.is_some();

        let summary = h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .child(
                div().text_sm().child(
                    t!(
                        "ui.settings.wows.cache.stats",
                        size = humansize::format_size(stats.total_bytes, humansize::BINARY),
                        count = stats.version_count,
                    )
                    .to_string(),
                ),
            )
            .child({
                let folder = base.clone();
                Button::new("cache-open-folder")
                    .label(t!("ui.settings.wows.cache.open_folder").to_string())
                    .compact()
                    .on_click(move |_event, _window, _cx| open_directory(&folder))
            })
            // Nothing to prune while there is one build: the newest is kept.
            .when(stats.version_count > 1, |this| {
                let dir = base.clone();
                this.child(
                    Button::new("cache-delete-old")
                        .label(t!("ui.settings.wows.cache.delete_old").to_string())
                        .compact()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.delete_old_cache_versions(dir.clone(), cx)
                        })),
                )
            });

        let jobs = h_flex()
            .gap_2()
            .items_center()
            .child({
                let dir = base.clone();
                Button::new("cache-check-updates")
                    .label(t!("ui.settings.wows.cache.check_updates").to_string())
                    .compact()
                    .disabled(busy)
                    .on_click(
                        cx.listener(move |this, _event, _window, cx| this.check_cache_for_updates(dir.clone(), cx)),
                    )
            })
            .child({
                let dir = base.clone();
                Button::new("cache-validate")
                    .label(t!("ui.settings.wows.cache.validate").to_string())
                    .compact()
                    .disabled(busy)
                    .tooltip(t!("ui.settings.wows.cache.validate_tooltip").to_string())
                    .on_click(cx.listener(move |this, _event, _window, cx| this.validate_cache(dir.clone(), cx)))
            })
            .when(running, |this| this.child(Spinner::new()));

        let progress = self.cache.progress.filter(|step| step.total > 0).map(|step| {
            div()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(format!("{} / {}", step.done, step.total))
                .into_any_element()
        });

        let updates = (!self.cache.updates.is_empty()).then(|| {
            let count = self.cache.updates.len();
            let dir = base.clone();
            h_flex()
                .gap_2()
                .items_center()
                .child(div().text_sm().child(t!("ui.settings.wows.cache.updates_available", count = count).to_string()))
                .child(
                    Button::new("cache-update-all")
                        .label(t!("ui.settings.wows.cache.update_all").to_string())
                        .compact()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            let builds = this.cache.updates.clone();
                            this.fetch_cache_builds(dir.clone(), builds, cx);
                        })),
                )
                .into_any_element()
        });

        let repair = (!self.cache.repair.is_empty()).then(|| {
            let count = self.cache.repair.len();
            let dir = base.clone();
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(crate::theme::semantic().error))
                        .child(t!("ui.settings.wows.cache.repair_needed", count = count).to_string()),
                )
                .child(
                    Button::new("cache-repair")
                        .label(t!("ui.settings.wows.cache.repair").to_string())
                        .compact()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            let builds = this.cache.repair.clone();
                            this.fetch_cache_builds(dir.clone(), builds, cx);
                        })),
                )
                .into_any_element()
        });

        let failure = self.cache.failure.clone().map(|reason| {
            div().text_xs().text_color(rgb(crate::theme::semantic().error)).child(reason).into_any_element()
        });

        v_flex()
            .gap_2()
            .child(summary)
            .child(jobs)
            .children(progress)
            .children(updates)
            .children(repair)
            .children(failure)
            .into_any_element()
    }

    fn browse_for_cache_dir(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let asked = crate::dialog::pick_folder("Game data cache directory");
        cx.spawn(async move |this, cx| {
            let Some(picked) = asked.await else { return };
            let path = picked.to_string_lossy().into_owned();
            let _ = this.update_in(cx, |this, window, cx| {
                this.cache_dir_input.update(cx, |state, cx| state.set_value(path.clone(), window, cx));
                if let Some(settings) = this.settings_mut() {
                    settings.game_data_cache_dir = path.clone();
                }
                settings_store::save(keys::GAME_DATA_CACHE_DIR, &path, cx);
                this.forget_cache_findings(cx);
            });
        })
        .detach();
    }

    /// Prunes every cached build but the newest.
    ///
    /// Run off the UI thread: it removes whole build directories, which on a
    /// large cache is thousands of files.
    fn delete_old_cache_versions(&mut self, base: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let deleted = cx.background_spawn(async move { wows_data_mgr::dump::delete_old_versions(&base) }).await;
            let _ = this.update(cx, |this, cx| {
                if deleted > 0 {
                    this.forget_cache_findings(cx);
                }
            });
        })
        .detach();
    }

    fn check_cache_for_updates(&mut self, base: PathBuf, cx: &mut Context<Self>) {
        let known_tip = self.settings().and_then(|settings| settings.game_data_repo_commit.clone());
        let proxy = self.proxy_url();
        let view = cx.entity();
        game_data_cache::check_for_updates(
            |this: &mut Self| &mut this.cache,
            base,
            known_tip,
            proxy,
            &view,
            cx,
            |this, outcome, cx| this.cache_job_finished(outcome, cx),
        );
    }

    fn validate_cache(&mut self, base: PathBuf, cx: &mut Context<Self>) {
        let proxy = self.proxy_url();
        let view = cx.entity();
        game_data_cache::validate(
            |this: &mut Self| &mut this.cache,
            base,
            proxy,
            &view,
            cx,
            |this, outcome, cx| this.cache_job_finished(outcome, cx),
        );
    }

    fn fetch_cache_builds(
        &mut self,
        base: PathBuf,
        builds: Vec<wows_data_mgr::download_repo::BuildUpdateStatus>,
        cx: &mut Context<Self>,
    ) {
        if builds.is_empty() {
            return;
        }
        let proxy = self.proxy_url();
        let view = cx.entity();
        game_data_cache::download(
            |this: &mut Self| &mut this.cache,
            base,
            builds,
            proxy,
            &view,
            cx,
            |this, outcome, cx| this.cache_job_finished(outcome, cx),
        );
    }

    fn edit_setting<T: serde::Serialize>(
        &mut self,
        key: &'static str,
        cx: &mut Context<Self>,
        apply: impl FnOnce(&mut GpuiSettings) -> T,
    ) {
        let Some(settings) = self.settings_mut() else { return };
        let value = apply(settings);
        settings_store::save(key, &value, cx);
        cx.notify();
    }

    /// Whether unsigned code may load into this process, which is the one
    /// mitigation with a compatibility cost worth a choice.
    ///
    /// Windows only: the policy does not exist elsewhere, and a control for one
    /// that cannot be applied would be a control that does nothing. The change
    /// lands at the next launch, because the policy cannot be lifted once a
    /// process has started, which is what the label says.
    #[cfg(windows)]
    fn render_code_integrity_row(&self, cx: &mut Context<Self>) -> Option<gpui_kit::component::form::Field> {
        use wows_toolkit_hardening::CodeIntegrityPreference;

        let chosen = self.settings()?.code_integrity;
        Some(
            field()
                .label(t!("ui.settings.app.code_integrity").to_string())
                .description(t!("ui.settings.app.code_integrity_tooltip").to_string())
                .child(h_flex().gap_2().children(CodeIntegrityPreference::ALL.map(|choice| {
                    selectable(
                        ("code-integrity", choice as usize),
                        chosen == choice,
                        Button::new(("code-integrity-button", choice as usize))
                            .label(t!(choice.key()).to_string())
                            .compact()
                            .selected(chosen == choice)
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.edit_setting(keys::CODE_INTEGRITY, cx, |settings| {
                                    settings.code_integrity = choice;
                                    choice
                                });
                            })),
                    )
                }))),
        )
    }

    /// Nothing, where the policy does not exist.
    #[cfg(not(windows))]
    fn render_code_integrity_row(&self, _cx: &mut Context<Self>) -> Option<gpui_kit::component::form::Field> {
        None
    }

    /// Adopts what may be shared: saved, and pushed into the replay inspector,
    /// which is what contributes a battle that lands.
    pub(crate) fn adopt_data_sharing(&mut self, mode: DataSharingMode, cx: &mut Context<Self>) {
        self.edit_setting(keys::DATA_SHARING_MODE, cx, |settings| {
            settings.data_sharing = mode;
            mode
        });
        // The older bool beside it, which the egui app reconciles the mode against
        // on load: writing only the mode would have that reconcile undo this
        // choice on its next start.
        settings_store::save(keys::SEND_REPLAY_DATA, &mode.shares_anything(), cx);
        self.replay_inspector.update(cx, |view, _cx| view.set_data_sharing(mode));
    }

    /// Rewrites the whole `ReplaySettings` blob, which is stored as one row,
    /// and pushes it into the replay inspector so its columns follow.
    fn edit_replay_settings(&mut self, cx: &mut Context<Self>, apply: impl FnOnce(&mut ReplaySettings)) {
        let Some(settings) = self.settings_mut() else { return };
        apply(&mut settings.replay);
        let replay = settings.replay.clone();
        settings_store::save(keys::REPLAY_SETTINGS, &replay, cx);
        self.replay_inspector.update(cx, |view, cx| view.set_replay_settings(replay.clone(), cx));
        cx.notify();
    }

    fn browse_for_wows_dir(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let asked = crate::dialog::pick_folder("World of Warships directory");
        cx.spawn(async move |this, cx| {
            let Some(picked) = asked.await else { return };
            let path = picked.to_string_lossy().into_owned();
            let _ = this.update_in(cx, |this, window, cx| {
                this.wows_dir_input.update(cx, |state, cx| state.set_value(path.clone(), window, cx));
                this.apply_wows_dir(path, window, cx);
            });
        })
        .detach();
    }

    /// Adopts a new game directory: saved, then pushed into the tabs that read
    /// it so they reload rather than keep showing the old install.
    fn apply_wows_dir(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        // A directory that is not there empties every tab that reads it, and
        // the egui field says so rather than adopting it silently
        // (`ui/settings_tab.rs`'s `wows_dir_invalid`).
        self.wows_dir_invalid = !path.is_empty() && !std::path::Path::new(&path).join("bin").is_dir();
        if self.wows_dir_invalid {
            crate::toast::stuck(STUCK_WOWS_DIR, t!("ui.messages.wows_dir_invalid").to_string(), window, cx);
            cx.notify();
            return;
        }

        crate::toast::resolved(STUCK_WOWS_DIR, window, cx);

        let Some(settings) = self.settings_mut() else { return };
        if settings.wows_dir == path {
            return;
        }
        settings.wows_dir = path.clone();
        settings_store::save(keys::WOWS_DIR, &path, cx);

        let replay_settings = settings.replay.clone();
        let debug_mode = settings.debug_mode;
        let auto_load = settings.auto_load_latest_replay;
        let locale = settings.locale.clone();
        let proxy_url = settings.proxy_url.clone();
        let game_data_cache_dir = settings.game_data_cache_dir.clone();
        let auto_dump_game_data = settings.auto_dump_game_data;
        let data_sharing = settings.data_sharing;
        let for_unpacker = path.clone();
        let for_tracker = path.clone();
        self.replay_inspector.update(cx, |view, cx| {
            let settings = InspectorSettings {
                wows_dir: path,
                game_data_cache_dir,
                auto_dump_game_data,
                data_sharing,
                debug_mode,
                replay_settings,
                auto_load_latest_replay: auto_load,
                locale,
                // The name is not what changed here; the popover keeps the
                // one it already has.
                collab_display_name: String::new(),
            };
            view.apply_settings(settings, window, cx)
        });
        self.unpacker.update(cx, |unpacker, cx| unpacker.apply_settings(for_unpacker, window, cx));
        self.watch_live_matches(&for_tracker, proxy_url, cx);
        let game_data = self.replay_inspector.read(cx).game_data();
        self.search.update(cx, |search, cx| search.set_game_data(game_data, cx));
        cx.notify();
    }

    /// Points the Player Tracker's live watch at `wows_dir`'s replays, using
    /// the game data the replay inspector just opened for the same install.
    /// A directory with no game data leaves the tracker saying so rather than
    /// polling the previous install.
    fn watch_live_matches(&mut self, wows_dir: &str, proxy_url: String, cx: &mut Context<Self>) {
        let Some(game_data) = self.replay_inspector.read(cx).game_data() else {
            return;
        };
        let replay_dir = std::path::PathBuf::from(wows_dir).join("replays");
        self.player_tracker.update(cx, |tracker, cx| tracker.watch_live_matches(replay_dir, game_data, proxy_url, cx));
    }

    fn render_settings_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = match &self.settings {
            SettingsState::Loading => {
                return v_flex()
                    .p_4()
                    .child(
                        div()
                            .text_sm()
                            .text_color(crate::theme::text_dim())
                            .child(t!("ui.settings.loading").to_string()),
                    )
                    .into_any_element();
            }
            SettingsState::Failed(reason) => {
                return v_flex()
                    .p_4()
                    .gap_1()
                    .child(
                        div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.settings.load_failed").to_string()),
                    )
                    .child(div().text_sm().text_color(crate::theme::text_dim()).child(reason.clone()))
                    .into_any_element();
            }
            SettingsState::Loaded(settings) => settings,
        };

        let check_for_updates = settings.check_for_updates;
        let enable_logging = settings.enable_logging;
        let data_sharing = settings.data_sharing;
        let theme_choice = settings.theme;
        let replay = settings.replay.clone();
        let current_replay_path = settings.current_replay_path.display().to_string();
        // Only the three flags are drawn, so the row itself does not have to
        // outlive the borrow the listeners below need.
        let armor_defaults = settings
            .armor_defaults
            .as_ref()
            .map(|defaults| (defaults.show_plate_edges, defaults.show_waterline, defaults.hull_opaque));
        let border = cx.theme().border;

        let application = settings_section(
            crate::icons::GEAR_FINE,
            t!("ui.settings.app.heading").into_owned(),
            t!("ui.settings.app.description").into_owned(),
            border,
            settings_form()
                .child(
                    field().label(t!("ui.settings.app.language").to_string()).child(
                        crate::ui::boxed(LANGUAGE_COMBO_WIDTH, crate::ui::SELECT_SMALL_HEIGHT).child(
                            Select::new(&self.language_select)
                                .id("settings-language")
                                .accessibility_label(t!("ui.settings.app.language").to_string())
                                .small()
                                .w(LANGUAGE_COMBO_WIDTH)
                                .menu_width(LANGUAGE_COMBO_WIDTH),
                        ),
                    ),
                )
                .child(field().label(t!("ui.settings.app.theme").to_string()).child(h_flex().gap_2().children(
                    ThemeChoice::ALL.map(|choice| {
                        selectable(
                            ("theme-choice", choice as usize),
                            theme_choice == choice,
                            Button::new(("theme-choice-button", choice as usize))
                                .label(choice.label())
                                .compact()
                                .selected(theme_choice == choice)
                                .on_click(cx.listener(move |this, _event, window, cx| {
                                    this.set_theme(choice, window, cx);
                                })),
                        )
                    }),
                )))
                .child(field().label(t!("ui.settings.app.zoom_factor").to_string()).child(self.render_zoom_row(cx)))
                .child(
                    field()
                        .label(t!("ui.settings.app.data_sharing_mode").to_string())
                        .description(data_sharing.description())
                        .child(h_flex().gap_2().children(DataSharingMode::ALL.map(|mode| {
                            selectable(
                                ("data-sharing", mode as usize),
                                data_sharing == mode,
                                Button::new(("data-sharing-button", mode as usize))
                                    .label(mode.label())
                                    .compact()
                                    .selected(data_sharing == mode)
                                    .tooltip(mode.description())
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        this.adopt_data_sharing(mode, cx);
                                    })),
                            )
                        }))),
                )
                .children(self.render_code_integrity_row(cx))
                .child(
                    field()
                        .label(t!("ui.settings.app.proxy_url").to_string())
                        .description(t!("ui.settings.app.proxy_url_hint").to_string())
                        .child(Input::new(&self.proxy_input).id("proxy-url").small().w_full()),
                )
                .child(
                    field().child(
                        v_flex()
                            .gap_2()
                            .child(
                                Checkbox::new("check-for-updates")
                                    .label(t!("ui.settings.app.check_for_updates").to_string())
                                    .checked(check_for_updates)
                                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                        let checked = *checked;
                                        this.edit_setting(keys::CHECK_FOR_UPDATES, cx, |settings| {
                                            settings.check_for_updates = checked;
                                            checked
                                        });
                                    })),
                            )
                            .child(
                                Checkbox::new("enable-logging")
                                    .label(t!("ui.settings.app.enable_logging").to_string())
                                    .checked(enable_logging)
                                    .tooltip(t!("ui.settings.app.enable_logging_tooltip").to_string())
                                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                        let checked = *checked;
                                        this.edit_setting(keys::ENABLE_LOGGING, cx, |settings| {
                                            settings.enable_logging = checked;
                                            checked
                                        });
                                    })),
                            ),
                    ),
                )
                .child(
                    field().child(
                        h_flex().child(
                            Button::new("open-data-dir")
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .child(crate::icons::icon(crate::icons::FOLDER_OPEN))
                                        .child(t!("ui.settings.app.open_data_dir").to_string()),
                                )
                                .compact()
                                .tooltip(t!("ui.settings.app.open_data_dir_tooltip").to_string())
                                .on_click(|_event, _window, _cx| open_data_directory()),
                        ),
                    ),
                ),
        );

        let game = settings_section(
            crate::icons::FOLDER_OPEN,
            t!("ui.settings.wows.heading").into_owned(),
            t!("ui.settings.wows.description").into_owned(),
            border,
            settings_form().child(
                field()
                    .label(t!("ui.settings.wows.directory_hint").to_string())
                    // The tab strip already flags it; the field says it too,
                    // because that is where the path is read and corrected.
                    // The line is always there, empty while the path is good,
                    // so saying so does not move the controls below it.
                    .description(if self.wows_dir_invalid {
                        t!("ui.messages.wows_dir_invalid").into_owned()
                    } else {
                        String::new()
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .when(self.wows_dir_invalid, |this| {
                                        this.text_color(rgb(crate::theme::semantic().error))
                                    })
                                    .child(Input::new(&self.wows_dir_input).id("wows-dir").small().w_full()),
                            )
                            .child(
                                Button::new("wows-dir-browse")
                                    .icon(IconName::FolderOpen)
                                    .label(t!("ui.settings.wows.browse").to_string())
                                    .compact()
                                    .on_click(
                                        cx.listener(|this, _event, window, cx| this.browse_for_wows_dir(window, cx)),
                                    ),
                            ),
                    ),
            ),
        );

        // Read before the cache section, which takes `self` mutably.
        let suppress_ip_warning = settings.suppress_p2p_ip_warning;
        let disable_auto_open = settings.disable_auto_open_session_windows;

        let cache = self.render_cache_section(cx);
        let index = self.render_index_section(cx);

        let session = settings_section(
            crate::icons::USERS,
            t!("ui.settings.session.heading").into_owned(),
            t!("ui.settings.session.description").into_owned(),
            border,
            settings_form()
                .child(
                    field()
                        .label(t!("ui.settings.session.display_name").to_string())
                        .child(Input::new(&self.collab_name_input).id("session-display-name").small().w_full()),
                )
                .child(
                    field().label(String::new()).child(
                        Checkbox::new("session-suppress-ip-warning")
                            .label(t!("ui.settings.session.suppress_ip_warning").to_string())
                            .checked(suppress_ip_warning)
                            .tooltip(t!("ui.settings.session.ip_warning_tooltip").to_string())
                            .on_click(cx.listener(move |this, checked: &bool, _window, cx| {
                                let on = *checked;
                                this.edit_setting(keys::SUPPRESS_P2P_IP_WARNING, cx, |settings| {
                                    settings.suppress_p2p_ip_warning = on;
                                    on
                                });
                            })),
                    ),
                )
                .child(
                    field().label(String::new()).child(
                        Checkbox::new("session-disable-auto-open")
                            .label(t!("ui.settings.session.disable_auto_open").to_string())
                            .checked(disable_auto_open)
                            .tooltip(t!("ui.settings.session.auto_open_tooltip").to_string())
                            .on_click(cx.listener(move |this, checked: &bool, _window, cx| {
                                let on = *checked;
                                this.edit_setting(keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS, cx, |settings| {
                                    settings.disable_auto_open_session_windows = on;
                                    on
                                });
                            })),
                    ),
                ),
        );

        let replay_section = settings_section(
            crate::icons::TABLE,
            t!("ui.settings.replay.heading").into_owned(),
            t!("ui.settings.replay.description").into_owned(),
            border,
            settings_form()
                .child(field().label(t!("ui.settings.replay.current_path").to_string()).child(div().text_sm().child(
                    crate::ui::selectable_text(
                        "settings-current-replay-path",
                        if current_replay_path.is_empty() {
                            t!("ui.settings.not_set").into_owned()
                        } else {
                            current_replay_path
                        },
                    ),
                )))
                .child(
                    field().child(
                        h_flex()
                            .flex_wrap()
                            .gap_x_4()
                            .gap_y_2()
                            .child(
                                Checkbox::new("show-raw-xp")
                                    .label(t!("ui.settings.replay.show_raw_xp").to_string())
                                    .checked(replay.show_raw_xp)
                                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                        let checked = *checked;
                                        this.edit_replay_settings(cx, |replay| replay.show_raw_xp = checked);
                                    })),
                            )
                            .child(
                                Checkbox::new("show-observed-damage")
                                    .label(t!("ui.settings.replay.show_observed_damage").to_string())
                                    .checked(replay.show_observed_damage)
                                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                        let checked = *checked;
                                        this.edit_replay_settings(cx, |replay| replay.show_observed_damage = checked);
                                    })),
                            )
                            .child(
                                Checkbox::new("show-entity-id")
                                    .label(t!("ui.settings.replay.show_entity_id").to_string())
                                    .checked(replay.show_entity_id)
                                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                        let checked = *checked;
                                        this.edit_replay_settings(cx, |replay| replay.show_entity_id = checked);
                                    })),
                            )
                            .child(
                                Checkbox::new("show-heals")
                                    .label(t!("ui.settings.replay.show_heals").to_string())
                                    .checked(replay.show_heals)
                                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                        let checked = *checked;
                                        this.edit_replay_settings(cx, |replay| replay.show_heals = checked);
                                    })),
                            ),
                    ),
                )
                .child(
                    field().description(t!("ui.settings.replay.enable_previews_tooltip").to_string()).child(
                        Checkbox::new("enable-replay-previews")
                            .label(t!("ui.settings.replay.enable_previews").to_string())
                            .checked(replay.enable_replay_previews)
                            .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                                let checked = *checked;
                                this.edit_replay_settings(cx, |replay| replay.enable_replay_previews = checked);
                            })),
                    ),
                )
                .child(
                    field()
                        .label(t!("ui.settings.replay.auto_export_data").to_string())
                        .description(t!("ui.settings.replay.export_path_hint").to_string())
                        .child(self.render_auto_export_row(&replay, cx)),
                ),
        );

        let twitch = settings_section(
            crate::icons::BROADCAST,
            t!("ui.settings.twitch.heading").into_owned(),
            t!("ui.settings.twitch.description").into_owned(),
            border,
            settings_form()
                .child(
                    field().child(
                        h_flex().child(
                            Button::new("twitch-get-token")
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .child(crate::icons::icon(crate::icons::BROWSER))
                                        .child(t!("ui.settings.twitch.get_token").to_string()),
                                )
                                .compact()
                                .tooltip(t!("ui.settings.twitch.get_token_tooltip").to_string())
                                .on_click(|_event, _window, cx: &mut gpui_kit::App| cx.open_url(TWITCH_TOKEN_URL)),
                        ),
                    ),
                )
                .child(
                    field()
                        // Always there, empty until a credential has been
                        // pasted, so reporting one does not move the button
                        // that pasted it.
                        .description(match self.twitch_paste.as_ref() {
                            Some(Ok(who)) => t!("ui.settings.twitch.signed_in_as", who = who).into_owned(),
                            Some(Err(why)) => why.clone(),
                            None => String::new(),
                        })
                        .child(
                            h_flex().child(
                                Button::new("twitch-paste-token")
                                    .child(
                                        h_flex()
                                            .gap_1()
                                            .child(crate::icons::icon(crate::icons::CLIPBOARD_TEXT))
                                            .child(t!(self.twitch_paste_label_key()).to_string())
                                            .child(
                                                div()
                                                    .text_color(rgb(self.twitch_status_tone()))
                                                    .child(crate::icons::icon(self.twitch_status_glyph())),
                                            ),
                                    )
                                    .compact()
                                    .tooltip(t!("ui.settings.twitch.paste_token_tooltip").to_string())
                                    .on_click(cx.listener(|this, _event, _window, cx| this.paste_twitch_token(cx))),
                            ),
                        ),
                )
                .child(
                    field().label(t!("ui.settings.twitch.monitored_channel").to_string()).child(
                        div()
                            .w(TWITCH_CHANNEL_WIDTH)
                            .child(Input::new(&self.twitch_channel_input).id("twitch-channel").small().w_full()),
                    ),
                ),
        );

        // The viewport writes these itself when a pane changes, so the tab
        // reports them rather than offering a second way to set them.
        let armor = settings_section(
            crate::icons::SHIELD,
            t!("ui.settings.armor.heading").into_owned(),
            t!("ui.settings.armor.description").into_owned(),
            border,
            settings_form().child(
                field().child(match armor_defaults {
                    Some((show_plate_edges, show_waterline, hull_opaque)) => h_flex()
                        .gap_4()
                        .child(
                            Checkbox::new("armor-show-plate-edges")
                                .label(t!("ui.armor.plate_edges").to_string())
                                .checked(show_plate_edges)
                                .disabled(true),
                        )
                        .child(
                            Checkbox::new("armor-show-waterline")
                                .label(t!("ui.armor.waterline").to_string())
                                .checked(show_waterline)
                                .disabled(true),
                        )
                        .child(
                            Checkbox::new("armor-hull-opaque")
                                .label(t!("ui.armor.opaque_hull").to_string())
                                .checked(hull_opaque)
                                .disabled(true),
                        )
                        .into_any_element(),
                    None => div()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.settings.armor.no_defaults").to_string())
                        .into_any_element(),
                }),
            ),
        );

        div()
            .id("settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .child(
                v_flex()
                    .gap_6()
                    .p_4()
                    .child(application)
                    .child(game)
                    .child(cache)
                    .child(index)
                    .child(session)
                    .child(replay_section)
                    .child(twitch)
                    .child(armor),
            )
            .into_any_element()
    }
}

/// Shows the toolkit's storage directory in the system file manager.
///
/// The directory is created first: it does not exist until something has
/// been written there, and opening a path that is not there yet reads as a
/// broken button rather than an empty folder.
fn open_data_directory() {
    let Some(dir) = wows_toolkit_config::storage_dir() else {
        tracing::warn!("settings: there is no storage directory to open");
        return;
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        tracing::warn!("settings: the storage directory could not be created: {err}");
        return;
    }
    open_directory(&dir);
}

/// Shows `dir` in the desktop's own file manager.
fn open_directory(dir: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let opener = "explorer.exe";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let opener = "xdg-open";

    let mut command = std::process::Command::new(opener);
    command.arg(dir);
    if let Err(err) = crate::child_process::prepare(&mut command).spawn() {
        tracing::warn!("settings: {} could not be opened: {err}", dir.display());
    }
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Pick up drag changes made directly on the zoom slider (the reset
        // button applies the theme itself, so this only fires for drags).
        let slider_zoom = self.zoom_slider.read(cx).value().start();
        if (slider_zoom - self.zoom).abs() > f32::EPSILON {
            self.zoom = slider_zoom;
            theme::apply_egui_theme(self.theme, self.zoom, window, cx);
            settings_store::save(keys::ZOOM_FACTOR, &self.zoom, cx);
        }

        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        let active_ix = AppTab::ALL.iter().position(|t| *t == self.active_tab).unwrap_or(0);
        let danger = cx.theme().danger;
        let tabs = TabBar::new("app-tabs")
            .selected_index(active_ix)
            .children(AppTab::ALL.iter().map(|t| {
                // A tab that needs looking at says so, which is how the egui
                // strip reports an install it cannot read (`app.rs`'s
                // `alert_tab_style`).
                let attention = *t == AppTab::Settings && self.wows_dir_invalid;
                Tab::new().child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .when(attention, |row| row.text_color(danger))
                        .child(crate::icons::icon(t.glyph()))
                        .child(t.label()),
                )
            }))
            .on_click(cx.listener(|this, ix: &usize, window, cx| {
                this.active_tab = AppTab::ALL[*ix];
                this.poll_armor_game_data(window, cx);
                if this.active_tab == AppTab::PlayerTracker {
                    this.player_tracker.update(cx, |tracker, cx| tracker.load_index_once(cx));
                }
                cx.notify();
            }));

        // What the egui app's menu bar carries (`app.rs:4751`), at the end of
        // the strip rather than in a row of its own: a row holding three items
        // above the tabs is a web page's chrome, and this is a desktop window.
        let app_menu = Button::new("app-menu")
            .ghost()
            .small()
            .icon(IconName::Ellipsis)
            .tooltip(t!("ui.menu.file").to_string())
            .dropdown_menu({
                let opening = cx.entity().downgrade();
                move |menu, _window, _cx| {
                    let opening = opening.clone();
                    menu.item(PopupMenuItem::new(t!("ui.windows.tactics_board").into_owned()).on_click(
                        move |_event, window, cx| {
                            let Some(app) = opening.upgrade() else { return };
                            app.update(cx, |app, cx| app.open_tactics_board(window, cx));
                        },
                    ))
                    .separator()
                    .item(PopupMenuItem::new(t!("ui.menu.check_updates").into_owned()).on_click(
                        move |_event, window, cx| {
                            check_for_update_from_menu(window, cx);
                        },
                    ))
                    .item(PopupMenuItem::new(t!("ui.menu.about").into_owned()).on_click(move |_event, window, cx| {
                        show_about(window, cx);
                    }))
                    .separator()
                    .item(PopupMenuItem::link(t!("ui.buttons.create_issue").into_owned(), ISSUES_URL))
                    .item(PopupMenuItem::link(t!("ui.buttons.discord").into_owned(), DISCORD_URL))
                    .separator()
                    .item(PopupMenuItem::new(t!("ui.menu.quit").into_owned()).on_click(
                        move |_event, _window, cx| {
                            cx.quit();
                        },
                    ))
                }
            });
        let strip = h_flex()
            .flex_none()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).child(tabs))
            .child(div().flex_none().px_1().child(app_menu));

        // A replay dragged onto the window opens, which is what the egui app does
        // with one (`app.rs`'s `ui_file_drag_and_drop`). Only a replay, and only
        // one: the inspector opens a file, not a set.
        let dropped = cx.listener(|this: &mut Self, paths: &gpui_kit::ExternalPaths, window, cx| {
            this.forget_hovering_files(cx);
            let replays: Vec<std::path::PathBuf> =
                paths.paths().iter().filter(|path| is_replay(path)).cloned().collect();
            match replays.as_slice() {
                [] => crate::toast::warn(t!("ui.messages.drop_not_a_replay").into_owned(), window, cx),
                [one] => {
                    this.active_tab = AppTab::ReplayInspector;
                    this.open_replay_from_search(one.clone(), window, cx);
                }
                _ => crate::toast::warn(t!("ui.messages.drop_one_at_a_time").into_owned(), window, cx),
            }
        });

        for report in std::mem::take(&mut self.job_said) {
            cx.defer_in(window, move |_this, window, cx| report.say(window, cx));
        }

        let body = match self.active_tab {
            AppTab::Settings => self.render_settings_tab(cx).into_any_element(),
            AppTab::ReplayInspector => self.replay_inspector.clone().into_any_element(),
            AppTab::ArmorViewer => self.armor_pane.clone().into_any_element(),
            AppTab::Stats => self.stats.clone().into_any_element(),
            AppTab::PlayerTracker => self.player_tracker.clone().into_any_element(),
            AppTab::Search => self.search.clone().into_any_element(),
            AppTab::Unpacker => self.unpacker.clone().into_any_element(),
        };

        // Reproduces the egui app's `app.rs:720-722` bottom-panel notice: a
        // small warn-colored strip flanked by a warning-triangle icon, shown
        // only while `debug_mode` is on. The egui version renders one
        // leading and one trailing "warning" glyph around the text; this
        // mirrors that with rendered `IconName::TriangleAlert` glyphs
        // instead of a unicode character in source.
        let warning_color = cx.theme().warning;
        let debug_notice = h_flex()
            .id("app-debug-notice")
            .test_support()
            .flex_none()
            .gap_1()
            .items_center()
            .justify_center()
            .px_2()
            .py_1()
            .child(Icon::new(IconName::TriangleAlert).text_color(warning_color))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(warning_color)
                    .child(t!("ui.app.debug_build").to_string()),
            )
            .child(Icon::new(IconName::TriangleAlert).text_color(warning_color));

        v_flex()
            .id("app-root")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .on_drop(dropped)
            // What is being dragged, while it is still being dragged: gpui turns
            // an entering file drag into a drag of `ExternalPaths`, so this is
            // where the scrim learns what it would take.
            .on_drag_move(cx.listener(|this, event: &gpui_kit::DragMoveEvent<gpui_kit::ExternalPaths>, _window, cx| {
                this.note_hovering_files(event.drag(cx).paths().to_vec(), cx);
            }))
            // A drag that leaves the window is reported as a file-drop event
            // rather than a mouse one, which only a paint-time listener sees.
            .child({
                let watched = cx.entity().downgrade();
                canvas(
                    |_bounds, _window, _cx| {},
                    move |_bounds, _state, window, _cx| {
                        let watched = watched.clone();
                        window.on_mouse_event::<gpui_kit::FileDropEvent>(move |event, phase, _window, cx| {
                            if phase != gpui_kit::DispatchPhase::Bubble {
                                return;
                            }
                            if !matches!(event, gpui_kit::FileDropEvent::Exited | gpui_kit::FileDropEvent::Ended) {
                                return;
                            }
                            if let Some(app) = watched.upgrade() {
                                app.update(cx, |this, cx| this.forget_hovering_files(cx));
                            }
                        });
                    },
                )
                .absolute()
                .size_0()
            })
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let modifiers = event.keystroke.modifiers;
                if event.is_held || !modifiers.control {
                    return;
                }
                match (modifiers.shift, event.keystroke.key.as_str()) {
                    (true, "d") => this.toggle_debug_mode(cx),
                    // Both, as the egui app takes both.
                    (false, "p") | (false, "k") => this.open_palette(window, cx),
                    _ => {}
                }
            }))
            .child(strip)
            // The rule under the strip, in the tone meant to be seen: it is
            // what separates the chrome from the page rather than two greys
            // meeting.
            .child(div().flex_none().h(px(1.)).bg(theme::border_bright()))
            .child(div().flex_1().min_h(px(0.)).bg(cx.theme().background).child(body))
            .children(self.status_strip(cx))
            .when(self.debug_mode, |this| this.child(debug_notice))
            // Dialogs, sheets and toasts are held by `Root` but drawn by
            // whoever renders the window's own view, so they go last and over
            // everything else.
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
            // Over all of it: a drag is about to become a drop, and what it
            // would do is the only thing worth reading while it hovers.
            .children(self.drop_scrim())
    }
}

#[cfg(test)]
mod tests {
    use wows_data_mgr::download_repo::DownloadPlan;
    use wows_data_mgr::download_repo::RemoteAvailability;
    use wows_data_mgr::download_repo::ResolvedBuild;

    use super::MissingBuild;
    use super::describe_missing_builds;

    fn waiting(build: u32, version: &str, replays: usize) -> MissingBuild {
        MissingBuild { build, version: Some(version.to_owned()), replays }
    }

    fn resolved(build: u32, availability: RemoteAvailability) -> ResolvedBuild {
        ResolvedBuild { requested_build: build, requested_version: None, availability }
    }

    /// Each build is a row of its own, so the reader can leave one out, and each
    /// row says what the repository has for it.
    #[test]
    fn every_build_is_a_row_that_says_what_is_published_for_it() {
        let missing = vec![waiting(7062104, "0.10.5.0", 3), waiting(3747819, "0.6.9.0", 1)];
        let plan = DownloadPlan {
            unique_missing_objects: 412,
            resolved: vec![
                resolved(7062104, RemoteAvailability::Exact),
                resolved(3747819, RemoteAvailability::Unpublished),
            ],
        };

        let rows = describe_missing_builds(&missing, Some(&plan));

        assert_eq!(rows.len(), 2, "one row per build, so one tick per build");
        assert_eq!(rows[0].0, 7062104, "the row carries the build it would fetch");
        assert!(rows[0].1.contains("0.10.5.0"), "got {rows:?}");
        assert!(rows[0].1.contains("3 replay(s)"), "got {rows:?}");
        assert!(rows[0].1.contains("published"), "got {rows:?}");
        assert!(rows[1].1.contains("never published"), "got {rows:?}");
    }

    /// A nearest published build is named, since downloading it may still not
    /// satisfy the replays that asked.
    #[test]
    fn a_nearest_match_names_what_would_be_fetched_instead() {
        let missing = vec![waiting(7062104, "0.10.5.0", 2)];
        let plan = DownloadPlan {
            unique_missing_objects: 9,
            resolved: vec![resolved(
                7062104,
                RemoteAvailability::Nearest { version: "0.10.5.1".to_owned(), build: 7070000 },
            )],
        };

        let rows = describe_missing_builds(&missing, Some(&plan));

        assert!(rows[0].1.contains("0.10.5.1"), "got {rows:?}");
        assert!(rows[0].1.contains("7070000"), "got {rows:?}");
    }

    /// A repository that could not be asked leaves the availability out rather
    /// than claiming one, and the offer still stands.
    #[test]
    fn an_unasked_repository_leaves_the_availability_out() {
        let missing = vec![waiting(7062104, "0.10.5.0", 2)];

        let rows = describe_missing_builds(&missing, None);

        assert!(rows[0].1.contains("0.10.5.0"), "got {rows:?}");
        assert!(rows[0].1.contains("2 replay(s)"), "got {rows:?}");
        assert!(!rows[0].1.contains("published"), "nothing is claimed about what is there: {rows:?}");
    }
}
