#[path = "directory_stats.rs"]
mod directory_stats;
#[path = "force_reindex_flow.rs"]
mod force_reindex_flow;
#[path = "settings_drawer.rs"]
mod settings_drawer;
#[path = "tab_underline.rs"]
mod tab_underline;

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
use gpui_kit::component::slider::SliderEvent;
use gpui_kit::component::slider::SliderState;
use gpui_kit::component::tab::Tab;
use gpui_kit::component::tab::TabBar;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rootcause::hooks::builtin_hooks::report_formatter::DefaultReportFormatter;
use rust_i18n::t;
use std::rc::Rc;

use crate::armor_viewer::ArmorViewerPane;
use crate::game_data_cache;
use crate::palette::PaletteAction;
use crate::player_tracker::PlayerTrackerEvent;
use crate::player_tracker::PlayerTrackerView;
use crate::replay_inspector::GameDataStatus;
use crate::replay_inspector::InspectorSettings;
use crate::replay_inspector::MissingBuild;
use crate::replay_inspector::ReplayInspectorView;
use crate::replay_inspector::view::ConstantsUnfit;
use crate::replay_inspector::view::GameDataMissing;
use crate::replay_inspector::view::ReplaySettingsChanged;
use crate::zoom::ResetZoom;
use crate::zoom::ZoomFactor;
use crate::zoom::ZoomIn;
use crate::zoom::ZoomOut;

pub(crate) use settings_drawer::Setting;

#[cfg(test)]
pub(crate) use settings_drawer::SettingsSection;

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

/// Whether a dragged or dropped path is a replay.
fn is_replay(path: &std::path::Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("wowsreplay")
}

/// What dropping a set of paths on the window would do.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DropIntent {
    OpenReplay(PathBuf),
    /// Lists the directory in a folder tab of the Replay Inspector.
    OpenFolder(PathBuf),
    NotAReplay,
    TooMany,
}

/// What a drop of `paths` would do, read from their names alone. `None` when
/// no path names a replay, and [`folder_drop`] has to look at them.
fn replay_drop(paths: &[PathBuf]) -> Option<DropIntent> {
    let replays: Vec<&PathBuf> = paths.iter().filter(|path| is_replay(path)).collect();
    match replays.as_slice() {
        [one] => Some(DropIntent::OpenReplay((*one).clone())),
        [_, _, ..] => Some(DropIntent::TooMany),
        [] => None,
    }
}

/// What a drop of `paths` that names no replay would do. Queries the
/// filesystem per path, which can block on an offline share, so it runs off
/// the UI thread.
fn folder_drop(paths: &[PathBuf]) -> DropIntent {
    let folders: Vec<&PathBuf> = paths.iter().filter(|path| path.is_dir()).collect();
    match folders.as_slice() {
        [one] => DropIntent::OpenFolder((*one).clone()),
        [_, _, ..] => DropIntent::TooMany,
        [] => DropIntent::NotAReplay,
    }
}

/// Files being dragged over the window, with what dropping them would do.
struct HoveringDrop {
    paths: Vec<PathBuf>,
    /// `None` while the paths are being looked at.
    intent: Option<DropIntent>,
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
fn offer_update(
    release: wows_toolkit_viewmodel::update::Release,
    app: WeakEntity<App>,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let asset_url = release.asset_for_windows().map(|asset| asset.browser_download_url.clone());
    window.open_dialog(cx, move |dialog, _window, cx| {
        let app = app.clone();
        let release_url = release.html_url.clone();
        let asset_url = asset_url.clone();
        let can_install = cfg!(target_os = "windows") && asset_url.is_some();
        let border = cx.theme().border;
        let notes_ground = cx.theme().muted;
        let radius = cx.theme().radius;
        dialog
            .title(t!("ui.windows.update_available").into_owned())
            .w(px(680.))
            .child(
                v_flex()
                    .id("update-offer")
                    .gap_3()
                    .child(crate::ui::selectable_text(
                        "update-version",
                        t!("ui.dialogs.update_message", tag = release.tag_name).into_owned(),
                    ))
                    // The notes are the release's own text, so they sit in a
                    // framed box of their own: where they start and stop is
                    // the frame, not a guess from the markdown's headings.
                    .when_some(release.body.clone().filter(|notes| !notes.trim().is_empty()), |body, notes| {
                        body.child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(crate::theme::text_dim())
                                        .child(t!("ui.dialogs.update_notes").into_owned()),
                                )
                                .child(
                                    div()
                                        .id("update-notes-scroll")
                                        .max_h(px(420.))
                                        .overflow_y_scroll()
                                        .p_3()
                                        .rounded(radius)
                                        .border_1()
                                        .border_color(border)
                                        .bg(notes_ground)
                                        .child(
                                            gpui_kit::component::text::TextView::markdown("update-notes", notes)
                                                .selectable(true),
                                        ),
                                ),
                        )
                    })
                    .when(!cfg!(target_os = "windows"), |body| {
                        body.child(
                            div()
                                .text_sm()
                                .text_color(crate::theme::text_dim())
                                .child(t!("ui.dialogs.update_windows_only").into_owned()),
                        )
                    })
                    .when(cfg!(target_os = "windows") && asset_url.is_none(), |body| {
                        body.child(
                            div()
                                .text_sm()
                                .text_color(crate::theme::text_dim())
                                .child(t!("ui.dialogs.update_asset_pending").into_owned()),
                        )
                    }),
            )
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    // A rule over the buttons, so they read as the dialog's
                    // own actions rather than the end of the notes above.
                    .pt_3()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        Button::new("update-view-release")
                            .small()
                            .label(t!("ui.buttons.view_release").into_owned())
                            .on_click(move |_, _, cx| cx.open_url(&release_url)),
                    )
                    .when(can_install, |footer| {
                        footer.child(
                            Button::new("update-install")
                                .primary()
                                .small()
                                .label(t!("ui.buttons.install_update").into_owned())
                                .on_click(move |_, window, cx| {
                                    let (Some(app), Some(url)) = (app.upgrade(), asset_url.clone()) else { return };
                                    window.close_dialog(cx);
                                    app.update(cx, |app, cx| app.install_update(url, window, cx));
                                }),
                        )
                    })
                    .child(
                        Button::new("update-close")
                            .small()
                            .label(t!("ui.buttons.cancel").into_owned())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateStatus {
    Idle,
    Checking(Asked),
    Installing,
}

/// How far the download has got, in the reader's own terms.
fn describe_download(step: crate::update::Downloaded) -> String {
    let read = humansize::format_size(step.read, humansize::BINARY);
    match step.total {
        Some(total) => {
            let total = humansize::format_size(total, humansize::BINARY);
            t!("ui.messages.update_downloading", read = read, total = total).into_owned()
        }
        // A server that did not say how much there is leaves only how much has
        // arrived, which is still worth saying.
        None => t!("ui.messages.update_downloading_unknown", read = read).into_owned(),
    }
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

/// The crash report box's height; the rest of the report scrolls.
pub(crate) const CRASH_REPORT_HEIGHT: f32 = 260.;

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
use crate::settings::GpuiSettings;
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
    /// Stats over the replays in one directory, keyed by its view.
    DirectoryStats(EntityId),
    PlayerTracker,
    Search,
    ArmorViewer,
    Unpacker,
    Launcher,
}

impl AppTab {
    /// Left-to-right order, following the egui app's own dock order
    /// (`app.rs`'s `DockState::new`): replays first. The egui dock opens the
    /// Launcher first; here it follows the Replay Inspector, which the strip
    /// always leads with. `Tab::ModManager` is deliberately absent: the egui
    /// app never adds it to its dock either. Settings are not a tab here but
    /// the drawer the gear at the end of the strip opens.
    pub const ALL: [AppTab; 7] = [
        AppTab::ReplayInspector,
        AppTab::Launcher,
        AppTab::Stats,
        AppTab::PlayerTracker,
        AppTab::Search,
        AppTab::ArmorViewer,
        AppTab::Unpacker,
    ];

    pub const fn label_key(self) -> &'static str {
        match self {
            AppTab::ReplayInspector => "ui.tabs.replay_parser",
            AppTab::Stats | AppTab::DirectoryStats(_) => "ui.tabs.stats",
            AppTab::PlayerTracker => "ui.tabs.player_tracker",
            AppTab::Search => "ui.tabs.search",
            AppTab::ArmorViewer => "ui.tabs.model_viewer",
            AppTab::Unpacker => "ui.tabs.unpacker",
            AppTab::Launcher => "ui.tabs.launcher",
        }
    }

    /// The tab's name in the reader's own language.
    pub fn label(self) -> String {
        t!(self.label_key()).into_owned()
    }

    /// The glyph in front of the label, as the egui tab strip carries
    /// (`app.rs`'s `icon_t` pairs).
    pub fn glyph(self) -> crate::icons::Glyph {
        match self {
            AppTab::ReplayInspector => crate::icons::Glyph::ClipboardList,
            AppTab::Stats | AppTab::DirectoryStats(_) => crate::icons::Glyph::ChartColumn,
            AppTab::PlayerTracker => crate::icons::Glyph::VenetianMask,
            AppTab::Search => crate::icons::Glyph::Search,
            AppTab::ArmorViewer => crate::icons::Glyph::Box,
            AppTab::Unpacker => crate::icons::Glyph::Archive,
            AppTab::Launcher => crate::icons::Glyph::Rocket,
        }
    }
}

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
    /// The tab the status bar's reported modes belong to; see
    /// `status_bar::clear_modes`.
    modes_tab: AppTab,
    /// The line under the selected tab of the strip.
    tab_underline: tab_underline::TabUnderline<AppTab>,
    settings: SettingsState,
    /// The applied zoom. Seeded from `settings.zoom` once the DB load
    /// completes, and written back so it survives a restart the way the egui
    /// slider's does.
    zoom: ZoomFactor,
    /// The saved palette. Seeded from the shared setting, then
    /// changed from the Settings drawer; a zoom change re-applies the theme, so
    /// it has to be kept rather than re-read.
    theme: ThemeChoice,
    theme_preview: Option<ThemeChoice>,
    /// What the last credential paste did, shown beside the button. `None`
    /// before anything has been pasted this session.
    twitch_paste: Option<Result<String, String>>,
    twitch_channel_input: Entity<InputState>,
    /// The chat poll, owned so it stops with the app rather than outliving it.
    _twitch_poll: Option<Task<()>>,
    /// Runs ahead of `zoom` while it is dragged: the zoom applies on release
    /// (see `on_zoom_slider`).
    zoom_slider: Entity<SliderState>,
    /// Between a press on the zoom slider and its release.
    zoom_slider_held: bool,
    /// Replay Inspector tab: the file browser plus the per-replay dock.
    /// Starts its background directory scan once `apply_settings` knows the
    /// WoWs directory.
    replay_inspector: Entity<ReplayInspectorView>,
    directory_stats: Vec<directory_stats::DirectoryStatsTab>,
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
    /// The command state for the currently open palette. Replaced on each open
    /// so a prior query and selection do not leak into the next invocation.
    palette: Entity<CommandState>,
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
    /// The Launcher tab: installs, sign-in, and fresh installs.
    launcher: Entity<crate::launcher::LauncherView>,
    /// Settings drawer text fields. Held so an edit can be read back and the
    /// saved value can be shown when the drawer first renders.
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
    pending_missing_builds: Vec<MissingBuild>,
    download_offer_open: bool,
    offered_builds: std::collections::BTreeSet<u32>,
    /// Whether the published result mappings have been checked this session. The
    /// egui app throttles its own check to one per half hour for the same reason:
    /// the mapping changes when the game does, not while the app is open.
    constants_checked: bool,
    index_revision_checked: bool,
    recent_crash_report: Option<String>,
    update_status: UpdateStatus,
    /// The board already open, so the menu brings it forward rather than
    /// opening another.
    tactics_board: Option<WeakEntity<crate::tactics::TacticsBoard>>,
    /// Held while a board is open: it is what carries what its walk found back
    /// to the copy a later board starts from.
    _board_subscription: Option<Subscription>,
    /// The capture-point layouts a tactics board picks its modes from, read
    /// from the cache both apps write. Empty until the read lands, and empty on
    /// a machine that has never indexed a replay, which is a board with maps
    /// and no modes rather than no board.
    cap_layouts: wows_replay_insights::cap_layout::CapLayoutDb,
    cap_layouts_requested: bool,
    /// Whether the game-data cache has been tidied this session.
    cache_maintained: bool,
    /// The files being dragged over the window, while any are. What the scrim
    /// says is what letting go of them would do.
    hovering_files: Option<HoveringDrop>,
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
    /// The newest Search request scoped to a folder, so an older lookup that
    /// lands later does not replace it.
    scoped_search: Option<Task<()>>,
    /// Backing state for the settings drawer's language combo.
    language_select: Entity<SelectState<SearchableVec<LanguageItem>>>,
    /// Whether the directory in the field is one an install could be in. The
    /// field says so, and the gear that opens the drawer carries the mark.
    wows_dir_invalid: bool,
    settings_drawer: settings_drawer::SettingsDrawer,
    _subscriptions: Vec<Subscription>,
}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let zoom_slider = cx.new(|_| {
            SliderState::new()
                .min(ZoomFactor::MIN.get())
                .max(ZoomFactor::MAX.get())
                .step(0.05)
                .default_value(ZoomFactor::DEFAULT.get())
        });
        let zoom_dragged = cx.subscribe_in(&zoom_slider, window, Self::on_zoom_slider);
        let zoom_set = cx.observe_in(&zoom_slider, window, Self::on_zoom_slider_set);
        Self::listen_for_zoom_shortcut::<ZoomIn>(ZoomFactor::zoomed_in, window, cx);
        Self::listen_for_zoom_shortcut::<ZoomOut>(ZoomFactor::zoomed_out, window, cx);
        Self::listen_for_zoom_shortcut::<ResetZoom>(|_| ZoomFactor::ACTUAL_SIZE, window, cx);
        let replay_inspector = cx.new(|cx| ReplayInspectorView::new(window, cx));
        let armor_pane = cx.new(|cx| ArmorViewerPane::new(window, cx));
        let unpacker = cx.new(|cx| UnpackerView::new(window, cx));
        let stats = cx.new(|cx| StatsView::new(window, cx));
        let player_tracker = cx.new(|cx| PlayerTrackerView::new(window, cx));
        let search = cx.new(|cx| SearchView::new(window, cx));
        let launcher = cx.new(|cx| crate::launcher::LauncherView::new(window, cx));
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
            let crate::replay_inspector::view::ArmorFollowed { at, hits, health } = event;
            let (at, hits, health) = (*at, hits.clone(), *health);
            this.armor_pane.update(cx, |pane, cx| pane.follow_hits(at, hits, health, cx));
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
        // Rows the reader chose in the listing, to be read again.
        let force_reindex = cx.subscribe_in(&replay_inspector, window, |this, view, event, window, cx| {
            let crate::replay_inspector::view::ForceReindexRequested { candidates, confirm } = event;
            let purpose = crate::force_reindex::IndexPurpose::Refresh;
            this.prepare_force_reindex(candidates.clone(), *confirm, purpose, view, window, cx);
        });
        // A folder's list, to have what the index lacks added to it.
        let index_folder = cx.subscribe_in(&replay_inspector, window, |this, _view, event, window, cx| {
            let crate::replay_inspector::view::IndexFolderRequested { scope, candidates } = event;
            this.prepare_folder_index(scope.clone(), candidates.clone(), window, cx);
        });
        // A mapping that does not fit means the commit this app last checked at
        // is no answer: forget it so the newest published one is fetched rather
        // than the repository reported unchanged.
        let constants_unfit =
            cx.subscribe_in(&replay_inspector, window, |this, _view, _event: &ConstantsUnfit, window, cx| {
                this.forget_constants_commit(window, cx);
            });
        // A folder tab asking for its own replays searched: the Search tab is
        // this app's, so the query is run here.
        let search_folder = cx.subscribe_in(&replay_inspector, window, |this, _view, event, window, cx| {
            let crate::replay_inspector::view::SearchFolder(scope) = event;
            this.search_scope(scope.clone(), window, cx);
        });
        // A session starting or stopping reaches the tactics board too: a board
        // is one of the windows a session is on.
        let session_shared = cx.subscribe(&replay_inspector, |this, _view, event, cx| {
            let crate::replay_inspector::view::SessionShared(link) = event;
            this.share_session_with_board(link.clone(), cx);
        });
        // The collab popover's own Tactics Board button. The board is this app's
        // window, so the ask arrives here.
        let board_requested = cx.subscribe_in(
            &replay_inspector,
            window,
            |this, _view, _event: &crate::replay_inspector::view::TacticsBoardRequested, window, cx| {
                this.open_tactics_board(window, cx);
            },
        );
        let autoload_changed = cx.subscribe(
            &replay_inspector,
            |this, _view, event: &crate::replay_inspector::view::AutoloadChanged, _cx| {
                if let Some(settings) = this.settings_mut() {
                    settings.auto_load_latest_replay = event.0;
                }
            },
        );
        // An opened directory's header asks which of its builds have no game
        // data here, which the app offers to fetch.
        let check_game_data = cx.subscribe_in(
            &replay_inspector,
            window,
            |this, _view, event: &crate::replay_inspector::view::CheckGameDataRequested, window, cx| {
                let crate::replay_inspector::view::CheckGameDataRequested(missing) = event;
                if missing.is_empty() {
                    crate::toast::info(t!("ui.replay_folders.game_data_available").into_owned(), window, cx);
                    return;
                }
                // Asked for, so offered again even where it was declined before.
                for build in missing {
                    this.offered_builds.remove(&build.build);
                }
                this.offer_missing_game_data(missing.clone(), window, cx);
            },
        );
        let directory_stats_requested = cx.subscribe_in(
            &replay_inspector,
            window,
            |this, _view, event: &crate::stats::directory::DirectoryStatsRequested, window, cx| {
                this.open_directory_stats(event.0.clone(), window, cx);
            },
        );
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
                let displayed = this.theme_preview.unwrap_or(this.theme);
                if displayed == ThemeChoice::System {
                    theme::apply_egui_theme(displayed, this.zoom, window, cx);
                }
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
        // An install picked in the Launcher becomes the directory every other
        // tab reads, as picking it in Settings does.
        let use_for_toolkit = cx.subscribe_in(&launcher, window, |this, _launcher, event, window, cx| {
            let crate::launcher::UseForToolkit(path) = event;
            let path = path.to_string_lossy().into_owned();
            this.wows_dir_input.update(cx, |state, cx| state.set_value(path.clone(), window, cx));
            this.apply_wows_dir(path, window, cx);
        });
        // Held keys are read as `!is_window_active()` once the window loses
        // focus, which nothing retained would otherwise draw again for.
        let activation = cx.observe_window_activation(window, |_this, window, _cx| window.refresh());
        let modes_reported = crate::status_bar::observe_modes(cx);

        Self {
            theme: ThemeChoice::default(),
            theme_preview: None,
            twitch_paste: None,
            twitch_channel_input,
            _twitch_poll: None,
            active_tab: AppTab::ReplayInspector,
            modes_tab: AppTab::ReplayInspector,
            tab_underline: tab_underline::TabUnderline::default(),
            settings: SettingsState::Loading,
            zoom: ZoomFactor::DEFAULT,
            zoom_slider,
            zoom_slider_held: false,
            replay_inspector,
            directory_stats: Vec::new(),
            debug_mode: false,
            focus_handle,
            armor_pane,
            armor_game_data_requested: false,
            stats_game_data_requested: false,
            twitch_status: TwitchStatus::Unset,
            palette: cx.new(|cx| CommandState::new(window, cx)),
            unpacker,
            stats,
            player_tracker,
            search,
            launcher,
            wows_dir_input,
            proxy_input,
            cache_dir_input,
            cache: game_data_cache::CacheState::default(),
            pending_missing_builds: Vec::new(),
            download_offer_open: false,
            offered_builds: std::collections::BTreeSet::new(),
            constants_checked: false,
            index_revision_checked: false,
            recent_crash_report: None,
            update_status: UpdateStatus::Idle,
            tactics_board: None,
            _board_subscription: None,
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
            scoped_search: None,
            language_select,
            wows_dir_invalid: false,
            settings_drawer: settings_drawer::SettingsDrawer::new(window, cx),
            _subscriptions: vec![
                subscription,
                check_game_data,
                directory_stats_requested,
                autoload_changed,
                show_armor_requested,
                armor_followed,
                armor_seek,
                replay_settings_changed,
                game_data_missing,
                force_reindex,
                index_folder,
                constants_unfit,
                search_folder,
                session_shared,
                board_requested,
                wows_dir_edited,
                proxy_edited,
                twitch_channel_edited,
                appearance_changed,
                cache_dir_edited,
                collab_name_edited,
                search_event,
                tracker_event,
                use_for_toolkit,
                language_chosen,
                activation,
                modes_reported,
                zoom_dragged,
                zoom_set,
            ],
        }
    }

    /// Looks for a newer release, and offers to install one.
    ///
    /// Asked at startup when the setting says to, and from the menu at any time.
    /// A check nobody asked for says nothing when this build is the newest one:
    /// the egui app reports "up to date" only for a manual check too.
    pub(crate) fn check_for_update(&mut self, asked: Asked, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_status == UpdateStatus::Installing {
            return;
        }
        if asked == Asked::ByHand {
            crate::status_bar::working(
                cx.weak_entity().into(),
                crate::status_bar::JobKind::UpdateCheck,
                t!("ui.messages.checking_app_updates").into_owned(),
                None,
                cx,
            );
        }
        if let UpdateStatus::Checking(ref mut pending) = self.update_status {
            if asked == Asked::ByHand {
                *pending = Asked::ByHand;
            }
            return;
        }
        self.update_status = UpdateStatus::Checking(asked);
        let found = crate::update::check(self.proxy_url(), cx);
        cx.spawn_in(window, async move |this, cx| {
            let found = found.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let UpdateStatus::Checking(asked) = this.update_status else { return };
                this.update_status = UpdateStatus::Idle;
                crate::status_bar::finished(cx.entity_id(), crate::status_bar::JobKind::UpdateCheck, cx);
                match found {
                    Ok(Some(release)) => offer_update(release, cx.entity().downgrade(), window, cx),
                    Ok(None) if asked == Asked::ByHand => {
                        crate::toast::ok(t!("ui.messages.app_up_to_date").into_owned(), window, cx);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let reason = format!("{}", error.format_with(&DefaultReportFormatter::ASCII));
                        tracing::warn!("Update check failed: {reason}");
                        if asked == Asked::ByHand {
                            crate::notices::show_error(
                                format!("{}\n\n{reason}", t!("ui.messages.update_check_failed")),
                                window,
                                cx,
                            );
                        }
                    }
                }
            });
        })
        .detach();
    }

    fn install_update(&mut self, asset_url: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_status != UpdateStatus::Idle {
            return;
        }
        self.update_status = UpdateStatus::Installing;
        crate::status_bar::working(
            cx.weak_entity().into(),
            crate::status_bar::JobKind::UpdateDownload,
            t!("ui.messages.update_starting").into_owned(),
            None,
            cx,
        );
        let (reports, mut progress) = futures::channel::mpsc::unbounded();
        let installing = crate::update::install(asset_url, self.proxy_url(), reports, cx);
        cx.spawn_in(window, async move |this, cx| {
            let listen = async {
                while let Some(step) = futures::StreamExt::next(&mut progress).await {
                    let told = cx.update(|_window, cx| {
                        crate::status_bar::working(
                            this.clone().into(),
                            crate::status_bar::JobKind::UpdateDownload,
                            describe_download(step),
                            None,
                            cx,
                        )
                    });
                    if told.is_err() {
                        break;
                    }
                }
            };
            let (installed, ()) = futures::future::join(installing, listen).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.update_status = UpdateStatus::Idle;
                crate::status_bar::finished(cx.entity_id(), crate::status_bar::JobKind::UpdateDownload, cx);
                if let Err(error) = installed {
                    let reason = format!("{}", error.format_with(&DefaultReportFormatter::ASCII));
                    tracing::warn!("Update installation failed: {reason}");
                    crate::notices::show_error(reason, window, cx);
                }
            });
        })
        .detach();
    }

    /// The stats view the directory stats tab `id` shows. An id that names no
    /// open tab shows the session's, so the body and the status bar agree.
    fn shown_stats(&self, id: EntityId) -> Entity<StatsView> {
        self.directory_stats_view(id).unwrap_or_else(|| self.stats.clone())
    }

    /// The tab strip, left to right: every tab once, with each directory's
    /// stats beside the session's.
    fn tab_order(&self) -> Vec<AppTab> {
        let mut tabs = Vec::new();
        for tab in AppTab::ALL {
            tabs.push(tab);
            if tab == AppTab::Stats {
                tabs.extend(self.directory_stats_tabs());
            }
        }

        tabs
    }

    /// The status bar along the bottom of the window: the active tab's side
    /// panel toggles, its session control when it has one, and what is running.
    fn status_strip(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let mut jobs: Vec<crate::status_bar::Job> = Vec::new();
        if let Some(job) = self.cache.running {
            let named = match job {
                game_data_cache::CacheJob::Checking => t!("ui.app.status_cache_checking"),
                game_data_cache::CacheJob::Validating => t!("ui.app.status_cache_validating"),
                game_data_cache::CacheJob::Downloading => t!("ui.app.status_cache_downloading"),
                game_data_cache::CacheJob::Planning => t!("ui.app.status_cache_planning"),
            };
            jobs.push(crate::status_bar::Job {
                name: named.into_owned().into(),
                progress: self.cache.progress.and_then(|at| crate::status_bar::Progress::new(at.done, at.total)),
            });
        }
        if let Some(step) = self.index_progress {
            jobs.push(crate::status_bar::Job {
                name: t!("ui.app.status_indexing").into_owned().into(),
                progress: crate::status_bar::Progress::new(step.done, step.total),
            });
        }
        jobs.extend(crate::status_bar::running(cx));

        let inspector = (self.active_tab == AppTab::ReplayInspector).then(|| self.replay_inspector.clone());
        let toggles = match self.active_tab {
            AppTab::ReplayInspector => self.replay_inspector.read(cx).panel_toggles(&self.replay_inspector, cx),
            AppTab::ArmorViewer => self.armor_pane.read(cx).panel_toggles(&self.armor_pane),
            AppTab::Stats => self.stats.read(cx).panel_toggles(cx),
            AppTab::DirectoryStats(id) => self.shown_stats(id).read(cx).panel_toggles(cx),
            AppTab::Unpacker => self.unpacker.read(cx).panel_toggles(cx),
            AppTab::PlayerTracker => self.player_tracker.read(cx).panel_toggles(cx),
            AppTab::Search | AppTab::Launcher => Vec::new(),
        };
        // A session belongs to the replay workspace it was started from.
        let session = inspector.map(|view| crate::collab_popover::render(&view, cx));

        crate::status_bar::render(toggles, session.into_iter().collect(), jobs, window, cx)
    }

    /// Notes the files now being dragged over the window.
    pub(crate) fn note_hovering_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.hovering_files.as_ref().is_some_and(|hovering| hovering.paths == paths) {
            return;
        }
        let intent = replay_drop(&paths);
        let looked_at = intent.is_none().then(|| paths.clone());
        self.hovering_files = Some(HoveringDrop { paths, intent });
        cx.notify();

        let Some(paths) = looked_at else { return };
        cx.spawn(async move |this, cx| {
            let classified = paths.clone();
            let intent = cx.background_spawn(async move { folder_drop(&classified) }).await;
            let _ = this.update(cx, |this, cx| {
                // The drag has moved on to other files, or left.
                let Some(hovering) = this.hovering_files.as_mut().filter(|hovering| hovering.paths == paths) else {
                    return;
                };
                hovering.intent = Some(intent);
                cx.notify();
            });
        })
        .detach();
    }

    /// Does what dropping files on the window asked for.
    fn apply_drop(&mut self, intent: DropIntent, window: &mut Window, cx: &mut Context<Self>) {
        match intent {
            DropIntent::OpenReplay(path) => self.open_replay_from_search(path, window, cx),
            DropIntent::OpenFolder(path) => {
                self.replay_inspector.update(cx, |view, cx| view.open_folder(path, window, cx));
                self.active_tab = AppTab::ReplayInspector;
                cx.notify();
            }
            DropIntent::NotAReplay => crate::toast::warn(t!("ui.messages.drop_not_a_replay").into_owned(), window, cx),
            DropIntent::TooMany => crate::toast::warn(t!("ui.messages.drop_one_at_a_time").into_owned(), window, cx),
        }
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
        let hovering = self.hovering_files.as_ref()?;
        let named = |path: &PathBuf| path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
        // `None` while a directory is being looked for, which is brief.
        let said = hovering.intent.as_ref().map(|intent| match intent {
            // The drop will say the same, rather than the scrim promising
            // something and the drop refusing it.
            DropIntent::NotAReplay => t!("ui.messages.drop_not_a_replay").into_owned(),
            DropIntent::OpenReplay(path) => t!("ui.app.drop_to_load", file = named(path)).into_owned(),
            DropIntent::OpenFolder(path) => t!("ui.app.drop_to_list", folder = named(path)).into_owned(),
            DropIntent::TooMany => t!("ui.messages.drop_one_at_a_time").into_owned(),
        });

        Some(
            div()
                .id("app-drop-scrim")
                .test_support()
                .when_some(said.clone(), |scrim, said| scrim.aria_label(said))
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                // theme-exempt: white on this overlay's own dark scrim, as the
                // egui overlay is.
                .bg(gpui_kit::black().opacity(0.75))
                .children(said.map(|said| div().text_lg().text_color(gpui_kit::white()).child(said)))
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
        self.show_crash_report(report, window, cx);
    }

    /// The crash dialog. The report is a backtrace dozens of lines long, so it
    /// scrolls inside a box of its own and the dialog keeps a fixed height.
    pub(crate) fn show_crash_report(&mut self, report: String, window: &mut Window, cx: &mut Context<Self>) {
        self.recent_crash_report = Some(report.clone());
        window.open_alert_dialog(cx, move |alert, _window, cx| {
            let copied = report.clone();
            let theme = cx.theme();
            let details = div()
                .id("crash-report-scroll")
                .test_support()
                .max_h(px(CRASH_REPORT_HEIGHT))
                .overflow_y_scroll()
                .p_2()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.muted)
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .child(crate::ui::selectable_text("crash-report", report.trim().to_owned()));
            alert
                .width(px(640.))
                .title(t!("ui.windows.crash_detected").into_owned())
                .description(v_flex().gap_3().child(t!("ui.dialogs.crash_message").into_owned()).child(details))
                .ok_text(t!("ui.buttons.copy").into_owned())
                .show_cancel(true)
                .on_ok(move |_event, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                    crate::toast::ok(t!("ui.stats.copied").into_owned(), window, cx);
                    true
                })
        });
    }

    pub(crate) fn copy_bug_report_info(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The saved commit and a startup crash both arrive with the settings.
        let Some(constants_commit) = self.settings().map(|settings| settings.constants_commit.clone()) else {
            return;
        };
        let loaded_game_version = self
            .replay_inspector
            .read(cx)
            .game_data()
            .and_then(|cache| cache.latest_loaded_version())
            .map(wows_toolkit_viewmodel::bug_report::full_game_version);
        let storage_dir = wows_toolkit_config::storage_dir();
        let downloaded_constants =
            wows_toolkit_viewmodel::bug_report::latest_downloaded_constants(storage_dir.as_deref());
        let report = wows_toolkit_viewmodel::bug_report::BugReportInfo {
            constants_fetch: wows_toolkit_viewmodel::bug_report::constants_fetch_status(),
            application_version: env!("CARGO_PKG_VERSION").to_owned(),
            loaded_game_version,
            downloaded_constants,
            constants_commit,
            recent_crash: self.recent_crash_report.clone(),
        }
        .discord_code_block();

        cx.write_to_clipboard(ClipboardItem::new_string(report));
        crate::toast::ok(t!("ui.messages.bug_report_info_copied").into_owned(), window, cx);
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
        if missing.is_empty() {
            return;
        }
        if self.cache.busy() || self.download_offer_open {
            for build in missing {
                if !self.pending_missing_builds.iter().any(|pending| pending.build == build.build) {
                    self.pending_missing_builds.push(build);
                }
            }
            return;
        }
        let Some(base) = self.cache_base() else {
            let open = self.settings_action(Setting::CacheDirectory, cx);
            crate::toast::warn_with_action(t!("ui.dialogs.download_plan_no_cache_dir").into_owned(), open, window, cx);
            return;
        };

        // What the repository actually publishes for each build, and how much the
        // whole selection would fetch, before the reader is asked to commit to
        // it: an offer to download data that was never published is worse than
        // saying so.
        self.download_offer_open = true;
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
                    // Failed planning leaves every build disabled until a retry
                    // can establish which downloads are available.
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
                // Only builds resolved by the repository can be selected.
                let eligible: std::collections::BTreeSet<u32> = plan
                    .as_ref()
                    .map(|plan| {
                        plan.resolved
                            .iter()
                            .filter(|build| {
                                matches!(
                                    build.availability,
                                    wows_data_mgr::download_repo::RemoteAvailability::Exact
                                        | wows_data_mgr::download_repo::RemoteAvailability::Nearest { .. }
                                )
                            })
                            .map(|build| build.requested_build)
                            .collect()
                    })
                    // No verified plan means no eligible downloads.
                    .unwrap_or_default();
                let ticked = Rc::new(std::cell::RefCell::new(eligible.clone()));
                let entity = cx.entity().downgrade();
                let _ = held.update(cx, move |_root: gpui_kit::AnyView, window, cx| {
                    window.open_dialog(cx, move |dialog, _window, _cx| {
                        let entity = entity.clone();
                        let missing = missing.clone();
                        let base = asked.clone();
                        let proxy = proxy.clone();
                        let ticked = Rc::clone(&ticked);
                        let picking = Rc::clone(&ticked);

                        let closing = entity.clone();
                        let dismissed = missing.clone();
                        dialog
                            .on_close(move |_event, _window, cx| {
                                if let Some(entity) = closing.upgrade() {
                                    entity.update(cx, |this, cx| {
                                        this.download_offer_open = false;
                                        this.offered_builds.extend(dismissed.iter().map(|build| build.build));
                                        cx.notify();
                                    });
                                }
                            })
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
                                            .disabled(!eligible.contains(&build))
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
                                            .disabled(eligible.is_empty())
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
                                                    this.download_offer_open = false;
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
                                            .on_click({
                                                let entity = entity.clone();
                                                let missing = missing.clone();
                                                move |_event, window, cx: &mut gpui_kit::App| {
                                                    if let Some(entity) = entity.upgrade() {
                                                        entity.update(cx, |this, cx| {
                                                            this.download_offer_open = false;
                                                            this.offered_builds
                                                                .extend(missing.iter().map(|build| build.build));
                                                            cx.notify();
                                                        });
                                                    }
                                                    window.close_dialog(cx);
                                                }
                                            }),
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
        wows_toolkit_replay_data::set_locale(&code);

        self.replay_inspector.update(cx, |view, cx| view.set_locale(Some(code.clone()), cx));
        self.stats.update(cx, |view, cx| view.set_locale(cx));
        self.set_directory_stats_locale(cx);
        // Placeholders are stored on their input states, so the ones the
        // reader sees are rewritten rather than left in the old language.
        self.refresh_placeholders(window, cx);
        cx.notify();
    }

    /// Rewrites every placeholder the settings drawer owns in the current
    /// language.
    ///
    /// A placeholder is held by its `InputState`, not re-read per frame, so a
    /// language change has to push the new text in.
    fn refresh_placeholders(&self, window: &mut Window, cx: &mut Context<Self>) {
        let pairs: [(&Entity<InputState>, &str); 4] = [
            // The cache directory's placeholder is a path, not a phrase, so a
            // language change leaves it alone.
            (&self.wows_dir_input, "ui.settings.wows.directory_hint"),
            (&self.twitch_channel_input, "ui.settings.twitch.monitored_channel"),
            (&self.proxy_input, "ui.settings.app.proxy_url_hint"),
            (self.settings_drawer.search(), "ui.settings.panel.search_placeholder"),
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

    fn poll_replay_index_revision(&mut self, cx: &mut Context<Self>) {
        if self.index_revision_checked || self.replay_inspector.read(cx).game_data().is_none() {
            return;
        }
        let Some(wows_dir) = self.settings().map(|settings| settings.wows_dir.clone()) else { return };
        if wows_dir.is_empty() {
            return;
        }
        let Some(pool) = settings_store::pool(cx) else { return };
        let Some(runtime) = runtime::runtime(cx) else { return };
        self.index_revision_checked = true;
        let root = std::path::Path::new(&wows_dir).join("replays");

        cx.spawn(async move |this, cx| {
            let stale = cx
                .background_spawn(async move {
                    runtime.block_on(wows_toolkit_config::index::query::has_stale_replay_records(&pool, &root))
                })
                .await;
            match stale {
                Ok(true) => {
                    let _ = this
                        .update(cx, |this, cx| this.build_replay_index(crate::replay_index::IndexMode::FillGaps, cx));
                }
                Ok(false) => {}
                Err(error) => tracing::warn!(%error, "replay index revision could not be checked"),
            }
        })
        .detach();
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
                    wows_toolkit_viewmodel::bug_report::record_constants_fetch(
                        wows_toolkit_viewmodel::bug_report::ConstantsFetchStatus::Succeeded,
                    );
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
                crate::constants::Checked::UpToDate => {
                    wows_toolkit_viewmodel::bug_report::record_constants_fetch(
                        wows_toolkit_viewmodel::bug_report::ConstantsFetchStatus::Succeeded,
                    );
                }
                crate::constants::Checked::Failed(reason) => {
                    wows_toolkit_viewmodel::bug_report::record_constants_fetch(
                        wows_toolkit_viewmodel::bug_report::ConstantsFetchStatus::Failed(reason.clone()),
                    );
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
                    runtime.handle().block_on(async move { crate::cap_layout_store::load_from_db(&pool).await })
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
        self.stats.update(cx, |stats, cx| stats.set_game_data(loaded.vfs(), loaded.provider().clone(), cx));
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
        self.poll_replay_index_revision(cx);
        self.poll_cache_maintenance(cx);
        self.poll_cap_layouts(cx);
        if self.active_tab == AppTab::ArmorViewer {
            self.request_armor_game_data(window, cx);
        }
    }

    /// Starts the Armor Viewer's catalogue load once the replay inspector's
    /// game data is ready. The palette's ship layer asks for it too, since it
    /// lists the catalogue from any tab.
    fn request_armor_game_data(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.armor_game_data_requested {
            return;
        }
        if let GameDataStatus::Ready(loaded) = self.replay_inspector.read(cx).game_data_status() {
            self.armor_game_data_requested = true;
            self.armor_pane.update(cx, |pane, cx| pane.load_game_data(loaded, window, cx));
        }
    }

    /// Every ship the palette's ship layer offers, starting the catalogue load
    /// it waits on when nothing has asked for it yet.
    pub(crate) fn armor_ship_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> crate::palette::ShipList {
        use crate::palette::ShipList;

        #[cfg(test)]
        if let Some(ships) = cx.try_global::<crate::palette::TestArmorShips>() {
            return ShipList::Ready(ships.rows.clone().into());
        }
        match self.replay_inspector.read(cx).game_data_status() {
            GameDataStatus::Loading { .. } => return ShipList::Pending,
            GameDataStatus::Failed(reason) => return ShipList::Unavailable(reason),
            GameDataStatus::Ready(_) => {}
        }
        self.request_armor_game_data(window, cx);
        match self.armor_pane.read(cx).catalog_status() {
            crate::armor_viewer::pane::CatalogStatus::Pending => ShipList::Pending,
            crate::armor_viewer::pane::CatalogStatus::Ready(catalog) => {
                ShipList::Ready(crate::palette::armor_ship_rows(catalog).into())
            }
            crate::armor_viewer::pane::CatalogStatus::Failed(reason) => ShipList::Unavailable(reason.to_owned()),
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

    /// Test-only: the app itself reaches the field directly.
    #[cfg(test)]
    pub(crate) fn armor_pane(&self) -> &Entity<ArmorViewerPane> {
        &self.armor_pane
    }

    /// The Search tab, so its hover and preview behaviour can be asserted
    /// without reaching through the rendered frame. Test-only.
    #[cfg(test)]
    pub(crate) fn search(&self) -> &Entity<SearchView> {
        &self.search
    }

    /// The Unpacker tab, so a test can hand its panes a VFS. Test-only.
    #[cfg(test)]
    pub(crate) fn unpacker(&self) -> &Entity<UnpackerView> {
        &self.unpacker
    }

    /// The Stats tab, so a test can restore a saved layout into it. Test-only.
    #[cfg(test)]
    pub(crate) fn stats(&self) -> &Entity<StatsView> {
        &self.stats
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
        self.theme_preview = None;
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
        // Brought forward rather than opened again: the egui app toggles one
        // board, and two windows on one would each walk the replays and each
        // write what they found.
        if let Some(open) = self.tactics_board.as_ref().and_then(|handle| handle.upgrade())
            && let Some(window) = open.read(cx).window()
        {
            let _ = window.update(cx, |_root, window, _cx| window.activate_window());
            return;
        }

        // The session this app is in, if any: a board opened while one is
        // running is one of the windows that session is on.
        let link = self.replay_inspector.read(cx).session_link();
        let board = cx.new(|cx| {
            let mut board = crate::tactics::TacticsBoard::new(game_data, layouts, window, cx);
            board.set_install(replays_dir, version, cx);
            if link.is_active() {
                board.set_collab(link, cx);
            }
            board
        });
        self.tactics_board = Some(board.downgrade());
        self._board_subscription =
            Some(cx.subscribe(&board, |this, _board, event: &crate::tactics::LayoutsFound, _cx| {
                let crate::tactics::LayoutsFound(found) = event;
                this.cap_layouts = found.clone();
            }));
        let options = crate::window_shell::options(
            wows_toolkit_config::WindowKind::TacticsBoard,
            crate::tactics::TacticsBoard::title(),
            cx,
        );
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

    /// Hands the tactics board its end of a session.
    ///
    /// Nothing is opened here: a board with no map of its own takes up a peer's
    /// as it follows the session, which is where the boards a joiner is sent
    /// after the handshake turn up.
    fn share_session_with_board(&mut self, link: crate::collab::CollabLink, cx: &mut Context<Self>) {
        if let Some(board) = self.tactics_board.as_ref().and_then(|handle| handle.upgrade()) {
            board.update(cx, |board, cx| board.set_collab(link, cx));
        }
    }

    /// Opens the command palette over the window.
    pub(crate) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Built per opening so the labels follow the current locale.
        let root = crate::palette::RootPalette::new(crate::palette::entries());
        self.open_palette_with(Rc::new(std::cell::RefCell::new(root)), window, cx);
    }

    /// Fills the palette with one of the cascading modes and reopens it.
    fn enter_palette_mode(&mut self, mode: crate::palette::PaletteMode, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |this, cx| this.open_palette_mode(mode, window, cx));
        });
    }

    /// Opens the palette on one of its second-layer lists.
    pub(crate) fn open_palette_mode(
        &mut self,
        mode: crate::palette::PaletteMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = match mode {
            crate::palette::PaletteMode::ArmorShips => {
                crate::palette::ModeSource::Ships(self.armor_ship_list(window, cx))
            }
            crate::palette::PaletteMode::Players => {
                crate::palette::ModeSource::index(crate::palette::IndexList::Players)
            }
            crate::palette::PaletteMode::MyShips => {
                crate::palette::ModeSource::index(crate::palette::IndexList::MyShips)
            }
            crate::palette::PaletteMode::ReindexVersions => {
                let candidates = self.listed_reindex_candidates(cx);
                crate::palette::ModeSource::Versions(crate::force_reindex::versions(&candidates).into())
            }
            crate::palette::PaletteMode::ReplayFolders => {
                let history = self.replay_inspector.read(cx).folder_history();
                let rows: Vec<crate::palette::FolderRow> =
                    history.folders().iter().cloned().map(crate::palette::FolderRow::new).collect();
                crate::palette::ModeSource::Folders(rows.into())
            }
        };

        self.palette = cx.new(|cx| CommandState::new(window, cx));
        let command = self.palette.clone();
        let owner = cx.weak_entity();
        let (inspector, armor_pane) = (self.replay_inspector.clone(), self.armor_pane.clone());
        let mode_view = cx.new(|cx| {
            let mut dialog = crate::palette::ModeDialog::new(command.clone(), owner, mode, source);
            dialog.wait_for_ships(&inspector, &armor_pane, window, cx);
            dialog.check_folders(cx);
            dialog
        });
        mode_view.update(cx, |dialog, cx| dialog.search(String::new(), window, cx));

        window.close_all_dialogs(cx);
        let command = self.palette.clone();
        let focus_on_mount = Rc::new(std::cell::Cell::new(true));
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let command = command.clone();
            let view = mode_view.clone();
            let focus_on_mount = focus_on_mount.clone();
            dialog.close_button(false).p_0().content(move |content, window, cx| {
                if focus_on_mount.replace(false) {
                    let command = command.clone();
                    window.defer(cx, move |window, cx| {
                        command.read(cx).focus_handle(cx).focus(window, cx);
                    });
                }
                content.child(view.clone())
            })
        });
    }

    /// Opens the palette over `root`.
    fn open_palette_with(
        &mut self,
        root: Rc<std::cell::RefCell<crate::palette::RootPalette>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Reaching for the palette while one is already up must not stack a
        // second over it. Closed rather than counted: a dialog can go by the
        // Escape key or a click outside it without this hearing, so a flag
        // saying whether one is open would eventually say the wrong thing
        // and refuse to open the palette at all.
        window.close_all_dialogs(cx);
        self.palette = cx.new(|cx| CommandState::new(window, cx));

        let palette = self.palette.clone();
        let owner = cx.weak_entity();
        // Focused once, when it is first drawn: the palette is a search field
        // and a reader who opened it is about to type.
        let focus_on_mount = std::rc::Rc::new(std::cell::Cell::new(true));

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let palette = palette.clone();
            let root = Rc::clone(&root);
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
                let shown = root.borrow().shown();
                let items = crate::palette::items(&shown, root.borrow().query());
                content.child(
                    Command::new(&palette)
                        .bordered(false)
                        // The command paints its own square background, which
                        // would show past the dialog's rounded corners.
                        .rounded(cx.theme().radius_lg)
                        // Filtered here rather than by the component, which
                        // matches by plain substring.
                        .filterable(false)
                        .placeholder(t!("ui.palette.placeholder").to_string())
                        .max_h(px(400.))
                        .items(items)
                        .on_query({
                            let root = Rc::clone(&root);
                            move |query, window, _cx| {
                                root.borrow_mut().search(query);
                                window.refresh();
                            }
                        })
                        .on_confirm(move |index_path, window, cx| {
                            let Some(entry) = shown.get(index_path.row) else { return };
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
        // Every other entry acts on the page, which the drawer would cover.
        if action != PaletteAction::OpenSettings {
            self.close_settings(window, cx);
        }
        match action {
            PaletteAction::GoTo(tab) => {
                self.active_tab = tab;
                self.poll_armor_game_data(window, cx);
                if tab == AppTab::PlayerTracker {
                    self.player_tracker.update(cx, |tracker, cx| tracker.load_index_once(cx));
                }
            }
            PaletteAction::OpenSettings => self.open_settings(None, window, cx),
            PaletteAction::SetTheme(choice) => self.set_theme(choice, window, cx),
            PaletteAction::OpenReplayFile => {
                self.replay_inspector.update(cx, |view, cx| view.open_manually(window, cx));
            }
            PaletteAction::SearchFor(query) => self.run_search(query, window, cx),
            PaletteAction::EnterMode(mode) => self.enter_palette_mode(mode, window, cx),
            PaletteAction::ViewArmor { param_index, display_name } => {
                self.active_tab = AppTab::ArmorViewer;
                self.poll_armor_game_data(window, cx);
                #[cfg(test)]
                if cx.has_global::<crate::palette::TestArmorShips>() {
                    let opened = param_index.clone();
                    cx.update_global(|ships: &mut crate::palette::TestArmorShips, _| ships.opened.push(opened));
                }
                self.armor_pane.update(cx, |pane, cx| pane.open_ship(param_index, display_name, cx));
            }
            PaletteAction::CopyLatestLog => self.copy_latest_log(window, cx),
            PaletteAction::CopyBugReportInfo => self.copy_bug_report_info(window, cx),
            PaletteAction::RefreshPersistedData => {
                crate::first_run::confirm_refresh_persisted_data(&cx.entity(), window, cx)
            }
            PaletteAction::IndexAllReplays => self.build_replay_index(crate::replay_index::IndexMode::FillGaps, cx),
            PaletteAction::ForceReindex(scope) => self.force_reindex_scope(scope, window, cx),
            PaletteAction::ImportConstants => {
                self.active_tab = AppTab::ReplayInspector;
                self.replay_inspector.update(cx, |view, cx| view.import_constants(window, cx));
            }
            PaletteAction::ContributeAllReplays(ledger) => {
                self.active_tab = AppTab::ReplayInspector;
                self.replay_inspector.update(cx, |view, cx| view.contribute_all(ledger, window, cx));
            }
            PaletteAction::BrowseReplayFolder => {
                self.active_tab = AppTab::ReplayInspector;
                self.replay_inspector.update(cx, |view, cx| view.browse_folder(window, cx));
            }
            PaletteAction::OpenReplayFolder(root) => {
                self.active_tab = AppTab::ReplayInspector;
                self.replay_inspector.update(cx, |view, cx| view.open_recent_folder(root, window, cx));
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

    /// Runs a search over the replays indexed under one source: the Live one
    /// for the install's folder, a chosen folder's own otherwise.
    ///
    /// A source nothing has created yet has nothing indexed, which the reader
    /// is told rather than shown an empty result for.
    fn search_scope(&mut self, scope: crate::replay_index::SourceScope, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pool) = settings_store::pool(cx) else { return };

        self.scoped_search = Some(cx.spawn_in(window, async move |this, cx| {
            let found = crate::runtime::block_on(cx, move || async move { scope.existing(&pool).await }).await;

            let _ = this.update_in(cx, |this, window, cx| match found {
                Ok(Some(source)) => {
                    let query = wows_toolkit_viewmodel::query_bar::seed::source_scoped(source);
                    let text = wows_toolkit_config::index::query_text::print_query(&query);
                    this.run_search(text, window, cx);
                }
                Ok(None) => crate::toast::warn(t!("ui.tabs.search_these_replays_unavailable").into_owned(), window, cx),
                Err(err) => {
                    tracing::warn!("search: the folder's index source could not be read: {err}");
                    crate::toast::failed(t!("ui.index_folder.unreadable_index").into_owned(), window, cx);
                }
            });
        }));
    }

    /// Asks for the directory the per-battle exports are written to.
    fn choose_export_directory(&mut self, cx: &mut Context<Self>) {
        let asked = crate::dialog::pick_folder(&t!("ui.settings.replay.export_path_hint"));
        cx.spawn(async move |this, cx| {
            let Some(directory) = crate::dialog::chosen(asked, &this, cx).await else { return };
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
        let open = self.settings_action(Setting::TwitchToken, cx);
        crate::toast::stuck_with_action(
            STUCK_TWITCH_TOKEN,
            t!("ui.messages.twitch_token_invalid").to_string(),
            open,
            window,
            cx,
        );
        cx.notify();
    }

    /// What the paste button says: what Twitch made of the stored credential,
    /// and what the last paste made of the clipboard. The egui settings tab
    /// names the same four states.
    /// The glyph beside the token button, which is what the egui tab draws
    /// next to the same label.
    fn twitch_status_glyph(&self) -> crate::icons::Glyph {
        if matches!(self.twitch_paste, Some(Err(_))) {
            return crate::icons::Glyph::CircleX;
        }
        match self.twitch_status {
            TwitchStatus::Unset => crate::icons::Glyph::TriangleAlert,
            // Nothing has been proven either way yet, so nothing is claimed.
            TwitchStatus::Checking => crate::icons::Glyph::Clock,
            TwitchStatus::Accepted { .. } => crate::icons::Glyph::CircleCheck,
            TwitchStatus::Refused => crate::icons::Glyph::CircleX,
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
            self.replay_inspector.update(cx, |view, cx| view.set_personal_rating(table.clone(), cx));
        }
        self.stats.update(cx, |stats, cx| stats.apply_session(data, window, cx));
        cx.notify();
    }

    pub fn apply_settings(&mut self, settings: GpuiSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom = settings.zoom;
        self.theme = settings.theme;
        let channel = settings.twitch_channel.clone();
        self.twitch_channel_input.update(cx, |state, cx| state.set_value(channel, window, cx));
        let collab_name = settings.collab_display_name.clone();
        let collab_display_name = collab_name.clone();
        self.collab_name_input.update(cx, |state, cx| state.set_value(collab_name, window, cx));
        self.zoom_slider.update(cx, |slider, cx| slider.set_value(settings.zoom.get(), window, cx));
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
        let opening_layer = settings.model_opening_layer;
        let xray_highlight = settings.model_xray_highlight;
        self.armor_pane.update(cx, |pane, cx| {
            pane.apply_armor_defaults(settings.armor_defaults.as_ref(), cx);
            pane.set_opening_layer(opening_layer, cx);
            pane.set_xray_highlight(xray_highlight, cx);
        });
        // The shared strings follow the saved language, and the combo shows
        // it, both from the moment the settings land.
        if let Some(code) = settings.locale.as_deref() {
            rust_i18n::set_locale(code);
            wows_toolkit_viewmodel::set_locale(code);
            wows_toolkit_replay_data::set_locale(code);
        }
        let language_ix = language_index(settings.locale.as_deref());
        self.language_select
            .update(cx, |state, cx| state.set_selected_index(Some(IndexPath::new(language_ix)), window, cx));

        // Seed the text fields so the tab opens showing what is saved.
        self.wows_dir_input.update(cx, |state, cx| state.set_value(settings.wows_dir.clone(), window, cx));
        self.proxy_input.update(cx, |state, cx| state.set_value(settings.proxy_url.clone(), window, cx));
        self.launcher.update(cx, |launcher, cx| launcher.apply_settings(&settings.proxy_url, window, cx));

        let unpacker_dir = settings.wows_dir.clone();
        let output_dir = settings.output_dir.clone();
        self.unpacker.update(cx, |unpacker, cx| {
            unpacker.apply_settings(unpacker_dir, window, cx);
            unpacker.set_output_dir(output_dir, window, cx);
        });
        self.poll_armor_game_data(window, cx);
        self.settings = SettingsState::Loaded(Box::new(settings));
        self.replay_inspector.update(cx, |view, cx| view.restore_folders(window, cx));
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

impl App {
    /// The zoom slider, the value it shows and its reset.
    pub(crate) fn render_zoom_row(&self, cx: &Context<Self>) -> impl IntoElement {
        // The slider's own value rather than `zoom`: during a drag it is the
        // one that moves.
        let shown = self.zoom_slider.read(cx).value().end();

        h_flex()
            .flex_wrap()
            .gap_2()
            .items_center()
            // The kit draws the thumb 8px left of its value whatever the
            // thumb's size, so the track is inset by that much on each side.
            .child(
                self.probe_zoom_slider(
                    crate::ui::boxed(px(176.), crate::ui::SELECT_SMALL_HEIGHT)
                        .px(px(8.))
                        .child(Slider::new(&self.zoom_slider)),
                ),
            )
            // Wide enough for "2.00" at any zoom, so the reset button holds still.
            .child(
                crate::ui::tabular(div().id("zoom-value").test_support().flex_none().min_w(rems(2.5)).text_sm())
                    .child(format!("{shown:.2}")),
            )
            .child(Button::new("reset-zoom").label(t!("ui.buttons.reset").to_string()).compact().on_click(
                cx.listener(|this, _event: &ClickEvent, window, cx| this.set_zoom(ZoomFactor::DEFAULT, window, cx)),
            ))
    }

    /// Applies `zoom`, saves it, and moves the slider to it. An open drawer
    /// keeps the zoom row where it is drawn.
    fn set_zoom(&mut self, zoom: ZoomFactor, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_slider.update(cx, |slider, cx| slider.set_value(zoom.get(), window, cx));
        if zoom != self.zoom {
            self.anchor_zoom_slider();
            self.zoom = zoom;
            theme::apply_egui_theme(self.theme_preview.unwrap_or(self.theme), zoom, window, cx);
            settings_store::save(keys::ZOOM_FACTOR, &zoom, cx);
        }
        cx.notify();
    }

    /// The zoom applies when a drag ends rather than as it moves. Applying it
    /// rescales the whole window, this slider and the labels beside it
    /// included, so the track moves under a pointer that has not; the value
    /// read back then follows the rescaling instead of the pointer, and every
    /// step reflows the page under it.
    fn on_zoom_slider(
        &mut self,
        _slider: &Entity<SliderState>,
        event: &SliderEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SliderEvent::Change(_) => {
                self.zoom_slider_held = true;
                // For the value shown beside the slider.
                cx.notify();
            }
            SliderEvent::Release(_) => self.end_zoom_drag(window, cx),
        }
    }

    /// Applies the value a drag of the zoom slider ended on. Called for the
    /// slider's own release and for any left release in the window, whichever
    /// comes first.
    fn end_zoom_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.zoom_slider_held {
            return;
        }

        self.zoom_slider_held = false;
        let value = self.zoom_slider.read(cx).value().end();
        self.set_zoom(ZoomFactor::new(value), window, cx);
    }

    /// A value set on the slider other than by the pointer, as an assistive
    /// technology's increment does, applies at once: there is no drag to wait
    /// out.
    fn on_zoom_slider_set(&mut self, slider: Entity<SliderState>, window: &mut Window, cx: &mut Context<Self>) {
        if self.zoom_slider_held {
            return;
        }

        let value = slider.read(cx).value().end();
        if value != self.zoom.get() {
            self.set_zoom(ZoomFactor::new(value), window, cx);
        }
    }

    /// A zoom shortcut: the factor `next` gives from the current one, and a
    /// note saying what it is, since the tab that shows the slider may not
    /// be the one open.
    fn step_zoom(&mut self, next: fn(ZoomFactor) -> ZoomFactor, window: &mut Window, cx: &mut Context<Self>) {
        // The stored zoom replaces whatever is set before the settings are
        // read, and nothing set now could be saved.
        if matches!(self.settings, SettingsState::Loading) {
            return;
        }

        let zoom = next(self.zoom);
        self.set_zoom(zoom, window, cx);

        let said = t!("ui.settings.panel.zoom_changed", factor = format!("{:.1}", zoom.get())).into_owned();
        crate::toast::replacing("zoom", crate::toast::Level::Info, said, window, cx);
    }

    /// Routes the shortcut bound to `A` to [`Self::step_zoom`].
    ///
    /// A global listener rather than one on this view's element: the palette
    /// and the other dialogs `Root` hosts hold focus outside this view, where
    /// an element's listener is not on the dispatch path.
    fn listen_for_zoom_shortcut<A: Action>(
        next: fn(ZoomFactor) -> ZoomFactor,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.weak_entity();
        let handle = window.window_handle();
        let global: &mut gpui_kit::App = cx;
        global.on_action(move |_: &A, cx| {
            let app = app.clone();
            // The window the key went to is being updated until its dispatch
            // returns, and the zoom needs this one.
            cx.defer(move |cx| {
                // A closed window or a dropped view has nothing left to zoom.
                let _ = handle.update(cx, |_, window, cx| app.update(cx, |app, cx| app.step_zoom(next, window, cx)));
            });
        });
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
            // Stays on the Search tab: the render runs in the background and
            // reports through toasts.
            SearchEvent::RenderReplayToClipboard(path) => {
                let paths = vec![path.clone()];
                self.replay_inspector.update(cx, |view, cx| view.render_many_to_clipboard(paths, window, cx));
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
        self.launcher.update(cx, |launcher, _| launcher.set_proxy(&url));
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
        if self.index_busy(cx) {
            self.job_said.push(JobReport::warn(t!("ui.reindex.busy_index").into_owned()));
            cx.notify();
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
        let personal_rating = self.replay_inspector.read(cx).personal_rating().cloned();
        let read_with = personal_rating.clone();

        let root = std::path::Path::new(&wows_dir).join("replays");
        let cancel = self.begin_index_pass(cx);
        self.index_outcome = None;
        self.index_progress = Some(crate::replay_index::IndexProgress::default());
        cx.notify();

        let progress_tx = self.follow_index_progress(cx);

        self._index_build = Some(cx.spawn(async move |this, cx| {
            let built = cx
                .background_spawn(async move {
                    let live =
                        wows_toolkit_config::index::query::ensure_default_source(&pool, &root, jiff::Timestamp::now());
                    let source = runtime.block_on(live)?;
                    crate::replay_index::build_index(
                        &runtime,
                        &pool,
                        source,
                        &root,
                        &game_data,
                        personal_rating.as_deref(),
                        &cancel,
                        mode,
                        |step| {
                            let _ = progress_tx.unbounded_send(step);
                        },
                    )
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.end_index_pass(cx);
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
                // The rows read what the index held before the pass.
                this.replay_inspector.update(cx, |view, cx| {
                    view.index_pass_rated(read_with.as_ref(), cx);
                    view.reload_index_summaries(cx);
                });
                cx.notify();
            });
        }));
    }

    /// Moves the index pass's progress line along as the pass reports, and
    /// hands back where the pass sends its reports.
    fn follow_index_progress(
        &self,
        cx: &mut Context<Self>,
    ) -> futures::channel::mpsc::UnboundedSender<crate::replay_index::IndexProgress> {
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

        progress_tx
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

    fn browse_for_cache_dir(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let asked = crate::dialog::pick_folder("Game data cache directory");
        cx.spawn(async move |this, cx| {
            let Some(picked) = crate::dialog::chosen(asked, &this, cx).await else { return };
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
            let Some(picked) = crate::dialog::chosen(asked, &this, cx).await else { return };
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
            let open = self.settings_action(Setting::WowsDirectory, cx);
            crate::toast::stuck_with_action(
                STUCK_WOWS_DIR,
                t!("ui.messages.wows_dir_invalid").to_string(),
                open,
                window,
                cx,
            );
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

/// One tab's glyph and name.
fn tab_label(tab: AppTab, selected: bool, cx: &gpui_kit::App) -> AnyElement {
    h_flex()
        .gap_1()
        .items_center()
        .when(selected, |row| row.text_color(cx.theme().tab_active_foreground).font_weight(FontWeight::SEMIBOLD))
        .child(crate::icons::icon(tab.glyph()))
        .child(tab.label())
        .into_any_element()
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
        // A hidden tab's views stay alive without rendering, so what they
        // reported is dropped; the new tab's views report as they render.
        if self.modes_tab != self.active_tab {
            self.modes_tab = self.active_tab;
            crate::status_bar::clear_modes(cx);
        }
        // The drawer can shut without a pointer-leave event on the preview tile.
        if !self.settings_drawer.is_open() && self.theme_preview.take().is_some() {
            theme::apply_egui_theme(self.theme, self.zoom, window, cx);
        }

        if !self.cache.busy() && !self.download_offer_open && !self.pending_missing_builds.is_empty() {
            let pending = std::mem::take(&mut self.pending_missing_builds);
            self.offer_missing_game_data(pending, window, cx);
        }
        // Read here rather than at each place `active_tab` is assigned: the
        // tracker holds its live stats lookup until its tab is displayed.
        let tracker_shown = self.active_tab == AppTab::PlayerTracker;
        self.player_tracker.update(cx, |tracker, cx| tracker.set_tab_shown(tracker_shown, cx));
        let tab_order = self.tab_order();
        let active_ix = tab_order.iter().position(|t| *t == self.active_tab).unwrap_or(0);
        let tabs = TabBar::new("app-tabs")
            .selected_index(active_ix)
            .children(tab_order.iter().enumerate().map(|(ix, t)| {
                let selected = *t == self.active_tab;
                Tab::new().relative().child(self.tab_underline.part(*t, ix == 0, selected)).child(match *t {
                    AppTab::DirectoryStats(id) => self.directory_stats_tab_label(id, selected, cx),
                    _ => tab_label(*t, selected, cx),
                })
            }))
            .on_click(cx.listener(move |this, ix: &usize, window, cx| {
                let Some(tab) = tab_order.get(*ix).copied() else { return };
                this.active_tab = tab;
                // The strip is beside the drawer, and a click there is a click
                // away from it.
                this.close_settings(window, cx);
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
            .child(div().flex_none().pl_1().child(self.render_settings_button(cx)))
            .child(div().flex_none().px_1().child(app_menu));

        // A replay dragged onto the window opens, which is what the egui app does
        // with one (`app.rs`'s `ui_file_drag_and_drop`); a directory is listed in
        // a folder tab, as Open Folder does. One at a time: the inspector opens a
        // file or a directory, not a set.
        let dropped = cx.listener(|this: &mut Self, paths: &gpui_kit::ExternalPaths, window, cx| {
            this.forget_hovering_files(cx);
            if let Some(intent) = replay_drop(paths.paths()) {
                this.apply_drop(intent, window, cx);
                return;
            }
            let paths = paths.paths().to_vec();
            cx.spawn_in(window, async move |this, cx| {
                let intent = cx.background_spawn(async move { folder_drop(&paths) }).await;
                let _ = this.update_in(cx, |this, window, cx| this.apply_drop(intent, window, cx));
            })
            .detach();
        });

        for report in std::mem::take(&mut self.job_said) {
            cx.defer_in(window, move |_this, window, cx| report.say(window, cx));
        }

        let settings_drawer = self.render_settings_drawer(window, cx);
        let body = match self.active_tab {
            AppTab::Launcher => self.launcher.clone().into_any_element(),
            AppTab::ReplayInspector => self.replay_inspector.clone().into_any_element(),
            AppTab::ArmorViewer => self.armor_pane.clone().into_any_element(),
            AppTab::Stats => self.stats.clone().into_any_element(),
            AppTab::DirectoryStats(id) => self.shown_stats(id).into_any_element(),
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
                        let released = watched.clone();
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
                        // The slider reports its own release, except when it
                        // left the frame mid-drag, with the drawer shut by a key.
                        window.on_mouse_event::<MouseUpEvent>(move |event, phase, window, cx| {
                            if phase != gpui_kit::DispatchPhase::Bubble || event.button != MouseButton::Left {
                                return;
                            }
                            if let Some(app) = released.upgrade() {
                                app.update(cx, |this, cx| this.end_zoom_drag(window, cx));
                            }
                        });
                    },
                )
                .absolute()
                .size_0()
            })
            // A frame for the views that read the held keys. Dispatch reaches
            // this only while focus is inside the app view, not in a dialog
            // the `Root` layer hosts; the keys themselves are read from the
            // window, so whatever draws next reads them right.
            .on_modifiers_changed(cx.listener(|_this, _event: &gpui_kit::ModifiersChangedEvent, _window, cx| {
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.is_held || this.settings_key(event, window, cx) {
                    return;
                }
                let modifiers = event.keystroke.modifiers;
                if !modifiers.control {
                    return;
                }
                match (modifiers.shift, event.keystroke.key.as_str()) {
                    (true, "d") => this.toggle_debug_mode(cx),
                    // Both, as the egui app takes both. On the Armor Viewer,
                    // ctrl+p goes straight to the ship list.
                    (false, "p") if this.active_tab == AppTab::ArmorViewer => {
                        this.open_palette_mode(crate::palette::PaletteMode::ArmorShips, window, cx)
                    }
                    (false, "p") | (false, "k") => this.open_palette(window, cx),
                    _ => {}
                }
            }))
            .child(strip)
            // The rule under the strip, in the tone meant to be seen: it is
            // what separates the chrome from the page rather than two greys
            // meeting.
            .child(div().flex_none().h(px(1.)).bg(theme::border_bright()))
            // The drawer covers the page below the strip, not the strip, so the
            // tabs and the gear stay where they are while it is open.
            .child(
                div().relative().flex_1().min_h(px(0.)).child(crate::ui::workspace(body, cx)).children(settings_drawer),
            )
            .child(self.status_strip(window, cx))
            .when(self.debug_mode, |this| this.child(debug_notice))
            // Over the rest of the view: a drag is about to become a drop, and
            // what it would do is the only thing worth reading while it hovers.
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

    /// A dropped directory lists it; a replay beside it wins; two of either is
    /// refused; anything else is not a replay.
    #[test]
    fn a_drop_opens_one_replay_or_one_folder() {
        use std::path::PathBuf;

        use super::DropIntent;
        use super::folder_drop;
        use super::replay_drop;

        let folder = tempfile::tempdir().expect("a temporary directory");
        let other = tempfile::tempdir().expect("a second temporary directory");
        let folder_path = folder.path().to_path_buf();
        let replay = PathBuf::from("G:/replays/one.wowsreplay");

        assert_eq!(replay_drop(std::slice::from_ref(&folder_path)), None, "a directory has to be looked at");
        assert_eq!(folder_drop(std::slice::from_ref(&folder_path)), DropIntent::OpenFolder(folder_path.clone()));
        assert_eq!(replay_drop(&[folder_path.clone(), replay.clone()]), Some(DropIntent::OpenReplay(replay.clone())));
        assert_eq!(folder_drop(&[folder_path, other.path().to_path_buf()]), DropIntent::TooMany);
        assert_eq!(replay_drop(&[replay.clone(), replay]), Some(DropIntent::TooMany));
        assert_eq!(folder_drop(&[PathBuf::from("G:/notes.txt")]), DropIntent::NotAReplay);
    }

    #[gpui_kit::test]
    fn manual_update_request_reports_a_pending_startup_failure(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        use gpui_kit::test::TestWindowExt as _;
        let (window, app) = crate::interaction_tests::open_app_in_root(cx);
        cx.update_window(window.into(), |_, window, cx| {
            app.update(cx, |app, cx| {
                // This harness has no network runtime, so discovery must report that failure.
                app.check_for_update(super::Asked::AtStartup, window, cx);
                app.check_for_update(super::Asked::ByHand, window, cx);
            });
        })
        .expect("update check window");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("notice-error-copy", cx);
            let copied = cx.read_from_clipboard().expect("update error copied").text().expect("error text");
            assert!(copied.contains("The update runtime is not available"), "{copied}");
            app.update(cx, |app, _| assert!(app.update_status == super::UpdateStatus::Idle));
        })
        .expect("update error window");
    }

    #[gpui_kit::test]
    fn update_dialog_has_platform_actions_and_can_be_closed(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        use gpui_kit::test::TestWindowExt as _;
        let (window, app) = crate::interaction_tests::open_app_in_root(cx);
        let release = serde_json::from_value(serde_json::json!({
            "tag_name": "v1.0.2",
            "html_url": "https://github.com/landaire/wows-toolkit/releases/tag/v1.0.2",
            "draft": false,
            "prerelease": false,
            "body": "## Changes\nRelease notes",
            "assets": [{
                "name": "wows_toolkit_v1.0.2_windows.zip",
                "browser_download_url": "https://example.invalid/update.zip"
            }]
        }))
        .expect("release response");
        cx.update_window(window.into(), |_, window, cx| {
            super::offer_update(release, app.downgrade(), window, cx);
            window.render_frame(cx);
            assert!(window.try_find("update-view-release").is_some());
            assert_eq!(window.try_find("update-install").is_some(), cfg!(target_os = "windows"));
            window.click("update-close", cx);
            window.render_frame(cx);
            assert!(window.try_find("update-view-release").is_none());
        })
        .expect("update dialog window");
    }

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
