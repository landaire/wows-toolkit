//! The launcher's surfaces driven in the headless shell, with its state set through the launcher's
//! events and test hooks instead of the network, the keyring and this machine's installs.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use tokio::runtime::Runtime;
use wgc_catalog::models::BuildVersion;
use wgc_catalog::models::ChainType;
use wgc_catalog::models::PartName;
use wgc_catalog::update::FileEntry;
use wgc_catalog::update::LatestBuild;
use wgc_catalog::update::PartStatus;
use wgc_install::GameInstall;
use wgc_install::InstallSource;
use wows_toolkit_launcher::AccountKey;
use wows_toolkit_launcher::CatalogCheckState;
use wows_toolkit_launcher::DownloadPhase;
use wows_toolkit_launcher::DownloadState;
use wows_toolkit_launcher::GameState;
use wows_toolkit_launcher::InstallChoice;
use wows_toolkit_launcher::KeyringState;
use wows_toolkit_launcher::LauncherSettings;
use wows_toolkit_launcher::RunnerState;
use wows_toolkit_launcher::SavedAccount;
use wows_toolkit_launcher::WgcLayerState;
use wows_toolkit_launcher::test_support;
use wows_toolkit_launcher::test_support::ScriptedSignIn;
use wows_toolkit_launcher::test_support::TestSession;
use zgui::view;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Toggled;

use crate::interaction_tests::REVIEW_SIZE;
use crate::interaction_tests::returning_reader;
use crate::launcher::model::LauncherModel;
use crate::launcher::model::PlayOrigin;
use crate::launcher::testing;
use crate::services::saved_for_test;
use crate::shell::AppRootProps;
use crate::shell::Shell;

/// The shell, with the launcher's own startup held.
pub(crate) struct LauncherWindow {
    pub(crate) window: HeadlessWindow,
    pub(crate) shell: Shell,
    pub(crate) model: LauncherModel,
    _runtime: Arc<Runtime>,
    /// The fake install the accounts play, removed with the window.
    pub(crate) install: tempfile::TempDir,
}

impl LauncherWindow {
    /// Behaviour tests draw nothing and look at the shell at its review size.
    pub(crate) fn open() -> Self {
        Self::open_with(HeadlessOptions::new(REVIEW_SIZE))
    }

    pub(crate) fn open_drawn(appearance: Appearance) -> Self {
        Self::open_with(HeadlessOptions {
            drawing: Drawing::Device,
            desktop: Some(appearance),
            ..HeadlessOptions::new(REVIEW_SIZE)
        })
    }

    pub(crate) fn open_with(options: HeadlessOptions) -> Self {
        let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
        returning_reader(&runtime);
        testing::hold_startup();

        let shown = Arc::clone(&runtime);
        let mut window = HeadlessWindow::open(options, move || {
            let runtime = Some(Arc::clone(&shown));
            view! { AppRoot(runtime = runtime) }
        })
        .expect("the shell opens headlessly");

        // The app's settings are read on the tokio runtime, in real time rather than the window's
        // clock.
        for _ in 0..20 {
            window.advance(Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(25));
        }

        let shell = crate::interaction_tests::shell();
        let install = tempfile::tempdir().expect("a temporary directory can be made");
        std::fs::write(install.path().join("WorldOfWarships.exe"), b"stub").expect("the stub is written");
        std::fs::create_dir_all(install.path().join("bin/13533378")).expect("the build directory is made");

        Self { window, shell, model: shell.launcher, _runtime: runtime, install }
    }

    /// Adopts `settings`, with every known game already checked so nothing asks a server.
    pub(crate) fn adopt(&mut self, settings: LauncherSettings) {
        self.model.adopt_quietly(settings);
        for game in wgc_catalog::known_games::known_games() {
            self.model.apply(test_support::game_checked(game.game_id, CatalogCheckState::Ready(fresh_build())));
        }
        self.settle();
    }

    pub(crate) fn settle(&mut self) {
        self.window.advance(Duration::from_millis(400));
    }

    pub(crate) fn has(&self, test_id: &str) -> bool {
        self.window.by_test_id(test_id).is_some()
    }

    pub(crate) fn press(&mut self, test_id: &str) {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.click(element).unwrap_or_else(|err| panic!("{test_id} can be pressed: {err}"));
        self.settle();
    }

    fn type_into(&mut self, test_id: &str, text: &str) {
        self.press(test_id);
        self.window.type_text(text);
        self.settle();
    }

    fn enter(&mut self) {
        self.window.press_key(NamedKey::Enter, Modifiers::NONE);
        self.settle();
    }

    /// The Play button's state, as it names it for tests.
    pub(crate) fn play_state(&self) -> String {
        let button = self.window.by_test_id("launcher-play").expect("the Play control is shown");
        self.window.attribute(button, "data-state").expect("the button names its state")
    }

    /// The text of every element with `class`, joined.
    fn text_of(&self, class: &str) -> String {
        self.window.texts_by_class(class).join(" ")
    }

    /// Settings with the launcher on and `accounts` playing the fake install.
    pub(crate) fn enabled_settings(&self, accounts: &[(AccountKey, &str)]) -> LauncherSettings {
        let mut settings = LauncherSettings::default();
        settings.enable();
        for (key, nickname) in accounts {
            settings.accounts.push(SavedAccount {
                key: *key,
                nickname: wgc_auth::Nickname::new(*nickname),
                install: InstallChoice::Path(self.install.path().to_path_buf()),
            });
        }
        settings.default_account = accounts.first().map(|(key, _)| *key);

        settings
    }

    /// The runner search found neither Steam's Proton nor umu-run.
    pub(crate) fn runner_missing(&mut self) {
        let missing = RunnerState::Missing(Arc::new(wgc_launch::RunnerError::NoRunner));
        self.model.update_launcher(|launcher| test_support::set_runner(launcher, missing));
        self.settle();
    }

    /// Finds the fake install with its catalog check answered.
    pub(crate) fn found(&mut self, catalog: CatalogCheckState) {
        let install = wgc_install(self.install.path());
        self.model.apply(test_support::catalog_checked(install.path.clone(), catalog));
        self.model.apply(test_support::installs_found(vec![install]));
        self.settle();
    }

    pub(crate) fn set_session(&mut self, key: AccountKey, session: TestSession) {
        self.model.update_launcher(|launcher| test_support::set_session(launcher, key, session));
        self.settle();
    }

    fn set_game(&mut self, state: GameState) {
        self.model.update_launcher(|launcher| test_support::set_game_state(launcher, state));
        self.settle();
    }

    fn script(&mut self, answer: ScriptedSignIn) {
        self.model.update_launcher(|launcher| test_support::script_sign_in(launcher, answer));
    }

    fn attempts(&self) -> Vec<test_support::SignInAttempt> {
        self.model.with_untracked(|state| test_support::sign_in_attempts(&state.launcher))
    }
}

pub(crate) fn eu() -> AccountKey {
    AccountKey { realm: wgc_auth::AccountRealm::Eu, account_id: wgc_auth::AccountId::new(697_769_785) }
}

pub(crate) fn na() -> AccountKey {
    AccountKey { realm: wgc_auth::AccountRealm::Na, account_id: wgc_auth::AccountId::new(1_034_874_630) }
}

fn asia() -> AccountKey {
    AccountKey { realm: wgc_auth::AccountRealm::Asia, account_id: wgc_auth::AccountId::new(2_011_532_779) }
}

pub(crate) fn wgc_install(path: &Path) -> GameInstall {
    GameInstall {
        path: path.to_path_buf(),
        version: Some(wowsunpack::data::Version {
            major: 15,
            minor: 10,
            patch: 0,
            build: std::num::NonZeroU32::new(13_533_378),
        }),
        build: None,
        source: InstallSource::Wgc { working_dir: path.to_path_buf() },
        product_id: None,
        auto_update: true,
    }
}

pub(crate) fn up_to_date() -> CatalogCheckState {
    CatalogCheckState::Ready(LatestBuild {
        chain_type: ChainType::Install,
        parts: vec![(PartName::Client, PartStatus::UpToDate)],
        downloads: Vec::new(),
        web_seed_threads: None,
    })
}

/// An update whose parts weigh 2.1 GB.
pub(crate) fn update_available() -> CatalogCheckState {
    let mut build = match up_to_date() {
        CatalogCheckState::Ready(build) => build,
        _ => unreachable!("up_to_date is a ready check"),
    };
    build.parts = vec![(
        PartName::Client,
        PartStatus::UpdateAvailable {
            from: BuildVersion::new("15.10.0.13533378".into()),
            to: BuildVersion::new("15.11.0.13600000".into()),
        },
    )];
    build.downloads = vec![wgc_catalog::update::PartDownload {
        part: PartName::Client,
        version_to: BuildVersion::new("15.11.0.13600000".into()),
        info_hash: serde_json::from_value(serde_json::json!("00".repeat(20))).expect("a hash"),
        torrent_urls: vec!["https://example.invalid/client.torrent".into()],
        web_seed_base: Some("https://example.invalid/".into()),
        files: vec![FileEntry { name: "client.pkg".into(), size: 2_100_000_000, unpacked_size: 2_100_000_000 }],
    }];

    CatalogCheckState::Ready(build)
}

fn fresh_build() -> LatestBuild {
    LatestBuild {
        chain_type: ChainType::Install,
        parts: vec![(PartName::Client, PartStatus::NotInstalled { to: BuildVersion::new("15.10.0.13533378".into()) })],
        downloads: Vec::new(),
        web_seed_threads: None,
    }
}

fn downloading(done_bytes: u64, total_bytes: u64) -> DownloadState {
    DownloadState::Running { phase: DownloadPhase::Downloading, done_bytes, total_bytes, current: "client.pkg".into() }
}

/// The launcher row saved last, as JSON.
fn saved_launcher_row() -> Option<serde_json::Value> {
    saved_for_test()
        .into_iter()
        .rev()
        .find(|(key, _)| *key == wows_toolkit_launcher::SETTINGS_KEY)
        .map(|(_, json)| serde_json::from_str(&json).expect("a saved row is JSON"))
}

/// The launcher with one signed-in EU account, Play ready.
fn ready() -> LauncherWindow {
    let mut launcher = LauncherWindow::open();
    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab"), (na(), "Ishmael")]);
    launcher.adopt(settings);
    launcher.found(up_to_date());
    launcher.set_session(eu(), TestSession::SignedIn("Captain_Ahab".into()));
    launcher.set_session(na(), TestSession::SignedIn("Ishmael".into()));

    launcher
}

fn open_settings_drawer(launcher: &mut LauncherWindow) {
    launcher.shell.settings_drawer.open_at(wows_toolkit_viewmodel::settings_drawer::Setting::GameLauncher);
    launcher.settle();
}

#[test]
fn the_launcher_is_hidden_until_the_user_opts_in() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(LauncherSettings::default());

    assert!(!launcher.has("launcher-play-control"));
    open_settings_drawer(&mut launcher);
    assert!(launcher.has("launcher-enabled"), "the opt-in is offered in the drawer");
    assert!(!launcher.has("launcher-installs"), "the other launcher rows wait for it");
}

#[test]
fn a_row_migrated_from_a_signed_in_account_starts_with_the_launcher_off() {
    let mut launcher = LauncherWindow::open();
    let row = r#"{"active_account": {"realm": "Eu", "account_id": 697769785, "nickname": "gapedd"}}"#;
    launcher.adopt(serde_json::from_str(row).expect("the row loads"));

    assert!(!launcher.has("launcher-play-control"));
}

#[test]
fn turning_the_launcher_on_asks_first_and_cancel_keeps_it_off() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(LauncherSettings::default());
    open_settings_drawer(&mut launcher);

    launcher.press("launcher-enabled");
    assert!(launcher.has("launcher-opt-in"), "the notice is shown first");
    assert!(launcher.text_of("wt-launcher__notice-account").starts_with("By using the launcher feature"));
    assert!(!launcher.model.with_untracked(|state| state.is_enabled()), "nothing is on yet");

    launcher.press("launcher-opt-in-cancel");
    assert!(!launcher.has("launcher-opt-in"));
    assert!(!launcher.model.with_untracked(|state| state.is_enabled()));
    assert!(!launcher.has("launcher-play-control"));
}

#[test]
fn continuing_turns_the_launcher_on_and_shows_play() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(LauncherSettings::default());
    open_settings_drawer(&mut launcher);

    launcher.press("launcher-enabled");
    launcher.press("launcher-opt-in-continue");

    assert!(launcher.model.with_untracked(|state| state.is_enabled()));
    assert!(launcher.has("launcher-play-control"));
    let saved = saved_launcher_row().expect("enabling saved the launcher row");
    assert_eq!(saved["consent"]["terms"], 1);
}

#[test]
fn turning_the_launcher_off_drops_the_acceptance() {
    let mut launcher = ready();
    open_settings_drawer(&mut launcher);

    launcher.press("launcher-enabled");

    assert!(!launcher.has("launcher-play-control"));
    let saved = saved_launcher_row().expect("disabling saved the launcher row");
    assert!(saved["consent"].is_null(), "turning it on again asks again");
}

#[test]
fn after_opting_out_operations_are_refused_and_results_land_without_follow_on_work() {
    let launcher = ready();
    launcher.model.disable();

    assert_eq!(launcher.model.run(|_, _, _| ((), false)), None);

    let other = PathBuf::from("/games/Other_World_of_Warships");
    launcher.model.apply(test_support::installs_found(vec![wgc_install(&other)]));
    let installs = launcher.model.with_untracked(|state| state.launcher.installs().to_vec());
    assert!(installs.iter().any(|install| install.path == other));
    assert!(launcher.model.with_untracked(|state| state.launcher.catalog_status(&other).is_none()));
}

#[test]
fn turning_the_launcher_on_again_looks_for_installs_again() {
    let launcher = ready();
    launcher.model.disable();

    launcher.model.enable();

    assert!(launcher.model.with_untracked(|state| state.launcher.is_discovering()));
}

#[test]
fn update_asks_where_the_build_goes_before_downloading() {
    let mut launcher = LauncherWindow::open();
    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab")]);
    launcher.adopt(settings);
    launcher.found(update_available());
    launcher.set_session(eu(), TestSession::SignedIn("Captain_Ahab".into()));
    assert_eq!(launcher.play_state(), "update");
    let install = launcher.install.path().to_path_buf();
    let downloading = |launcher: &LauncherWindow| {
        launcher.model.with_untracked(|state| state.launcher.download_state(&install).is_some())
    };
    wows_toolkit_services::dialog::take_asked();

    launcher.press("launcher-play");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert_eq!(wows_toolkit_services::dialog::take_asked(), [wows_toolkit_services::dialog::Asked::Folder]);
    assert!(!downloading(&launcher), "a dismissed picker starts nothing");

    let elsewhere = tempfile::tempdir().expect("a temporary folder");
    wows_toolkit_services::dialog::pick_next(elsewhere.path().to_path_buf());
    launcher.press("launcher-play");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert!(!downloading(&launcher), "another folder cannot take the install's update");
    let toasts = launcher.window.texts_by_class("lui-toasts").concat();
    let said = rust_i18n::t!("ui.launcher.update_in_place", path = install.display().to_string()).into_owned();
    assert!(toasts.contains(&said), "the message says where the update goes: {toasts:?}");

    // The same folder spelled another way is still the install.
    wows_toolkit_services::dialog::pick_next(install.join("."));
    launcher.press("launcher-play");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert!(downloading(&launcher), "the install's own folder takes the update");
}

#[test]
fn play_reads_the_default_account_when_ready() {
    let launcher = ready();

    assert_eq!(launcher.play_state(), "play");
    assert_eq!(launcher.text_of("wt-play__realm"), "EU");
    assert!(launcher.text_of("wt-play__nickname").contains("Captain_Ahab"));
}

#[test]
fn the_play_control_follows_the_launcher_through_its_states() {
    let mut launcher = LauncherWindow::open();
    let mut settings = launcher.enabled_settings(&[]);
    settings.accounts.clear();
    launcher.adopt(settings.clone());
    assert_eq!(launcher.play_state(), "set-up-game", "no install");

    settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab")]);
    launcher.adopt(settings);
    launcher.found(up_to_date());
    launcher.runner_missing();
    assert_eq!(launcher.play_state(), "set-up-runner", "no Proton on Linux");
    // Looking again after the user installed one.
    launcher.model.update_launcher(|launcher| test_support::set_runner(launcher, RunnerState::Unknown));

    let settings = launcher.enabled_settings(&[]);
    launcher.adopt(LauncherSettings { accounts: Vec::new(), default_account: None, ..settings });
    assert_eq!(launcher.play_state(), "set-up-game", "no account and no game directory");

    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab")]);
    launcher.adopt(settings);
    launcher.set_session(eu(), TestSession::Restoring);
    assert_eq!(launcher.play_state(), "restoring");
    launcher.set_session(eu(), TestSession::Expired);
    assert_eq!(launcher.play_state(), "sign-in-again");
    launcher.set_session(eu(), TestSession::SignedIn("Captain_Ahab".into()));
    assert_eq!(launcher.play_state(), "play");

    launcher.found(update_available());
    assert_eq!(launcher.play_state(), "update");
    assert!(launcher.text_of("wt-play__label").contains("GB"), "the update names its size");

    let path = launcher.install.path().to_path_buf();
    launcher.model.apply(test_support::download_reported(path.clone(), downloading(41, 100)));
    launcher.settle();
    assert_eq!(launcher.play_state(), "updating");
    assert!(launcher.text_of("wt-play__label").contains("41%"));
    assert!(launcher.has("app-status-work"), "the download keeps its status bar job");

    launcher.model.apply(test_support::download_reported(path.clone(), DownloadState::Paused));
    launcher.settle();
    assert_eq!(launcher.play_state(), "paused");

    launcher.model.apply(test_support::download_reported(path.clone(), DownloadState::Done { skipped_deltas: 0 }));
    launcher.found(up_to_date());
    launcher.set_game(GameState::Starting { account: Some(eu()), install: path.clone() });
    assert_eq!(launcher.play_state(), "starting");
    launcher.set_game(GameState::Running { account: Some(eu()), install: path.clone() });
    assert_eq!(launcher.play_state(), "running");
    assert_eq!(launcher.text_of("wt-play__realm"), "EU");

    let replay = wows_replays::ReplayFile::from_decrypted_parts(
        crate::tabs::player_tracker::tests::ARENA_INFO.as_bytes().to_vec(),
        Vec::new(),
    )
    .expect("the arena info parses");
    let live = wows_toolkit_viewmodel::player_tracker::live::LiveMatch::from_meta(&replay.meta).expect("it has a date");
    launcher.shell.live.seed(live);
    launcher.settle();
    assert_eq!(launcher.play_state(), "in-battle");
}

#[test]
fn no_accounts_with_an_install_offers_to_add_one() {
    let mut launcher = LauncherWindow::open();
    let settings = launcher.enabled_settings(&[]);
    launcher.adopt(settings);
    launcher.shell.apply_wows_dir(launcher.install.path().to_string_lossy().into_owned());
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);

    assert_eq!(launcher.play_state(), "add-account");
    launcher.press("launcher-play");
    assert!(launcher.has("launcher-sign-in-dialog"));

    launcher.shell.apply_wows_dir(String::new());
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
}

#[test]
fn a_narrow_window_keeps_only_the_glyph_and_the_realm() {
    let mut launcher = LauncherWindow::open_with(HeadlessOptions::new(WindowSize { width: 900.0, height: 700.0 }));
    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab")]);
    launcher.adopt(settings);
    launcher.found(up_to_date());
    launcher.set_session(eu(), TestSession::SignedIn("Captain_Ahab".into()));

    assert!(!launcher.has("launcher-play-label"));
    assert_eq!(launcher.text_of("wt-play__realm"), "EU");
}

#[test]
fn the_popover_lists_the_install_the_accounts_and_the_ways_to_the_rest() {
    let mut launcher = ready();
    launcher.set_session(na(), TestSession::Expired);

    launcher.press("launcher-play-menu");

    for id in [
        "launcher-install-line",
        "launcher-install-status",
        "launcher-account-eu-697769785",
        "launcher-account-na-1034874630",
        "launcher-account-na-1034874630-sign-in",
        "launcher-add-account",
        "launcher-open-game-settings",
        "launcher-open-setup",
        "launcher-open-replay",
        "launcher-play-safe-mode",
    ] {
        assert!(launcher.has(id), "{id} is shown");
    }
    assert_eq!(launcher.text_of("wt-launcher-install-status"), "15.10.0   Up to date");
}

#[test]
fn choosing_an_account_in_the_popover_makes_it_the_default() {
    let mut launcher = ready();
    launcher.press("launcher-play-menu");

    launcher.press("launcher-account-na-1034874630-choose");

    assert_eq!(launcher.model.with_untracked(|state| state.settings.as_ref().unwrap().default_account), Some(na()));
    assert_eq!(launcher.text_of("wt-play__realm"), "NA");
}

#[test]
fn accounts_cannot_be_switched_while_the_game_runs() {
    let mut launcher = ready();
    let install = launcher.install.path().to_path_buf();
    launcher.set_game(GameState::Running { account: Some(eu()), install });

    launcher.press("launcher-play-menu");

    assert!(launcher.has("launcher-close-to-switch"));
    launcher.press("launcher-account-na-1034874630-choose");
    assert_eq!(launcher.model.with_untracked(|state| state.settings.as_ref().unwrap().default_account), Some(eu()));
}

/// Six accounts in every row state, the default one with a nickname far wider than the popover.
/// Returns the rows' test ids.
fn crowded(launcher: &mut LauncherWindow) -> Vec<String> {
    let key = |realm, id| AccountKey { realm, account_id: wgc_auth::AccountId::new(id) };
    let ready = key(wgc_auth::AccountRealm::Na, 1_034_874_630);
    let unencrypted = key(wgc_auth::AccountRealm::Asia, 2_051_681_179);
    let expired = key(wgc_auth::AccountRealm::Eu, 697_769_786);
    let locked = key(wgc_auth::AccountRealm::Eu, 697_769_787);
    let memory_only = key(wgc_auth::AccountRealm::Asia, 2_051_681_180);
    let long = "gapedd_with_a_nickname_far_too_long_for_any_popover_row";

    let settings = launcher.enabled_settings(&[
        (eu(), long),
        (ready, "Ishmael"),
        (unencrypted, "Starbuck"),
        (expired, "Queequeg"),
        (locked, "Tashtego"),
    ]);
    launcher.adopt(settings);
    launcher.found(up_to_date());
    launcher.set_session(eu(), TestSession::SignedIn(long.into()));
    let left = Duration::from_secs(((13 * 24 + 22) * 60 + 30) * 60);
    launcher.set_session(ready, TestSession::SignedInUntil("Ishmael".into(), std::time::SystemTime::now() + left));
    launcher.set_session(unencrypted, TestSession::SignedInUnencrypted("Starbuck".into()));
    launcher.set_session(expired, TestSession::Expired);
    launcher.set_session(locked, TestSession::KeyringLocked);
    launcher.model.update_launcher(|launcher| launcher.reset_login(wows_toolkit_launcher::LoginTarget::NewAccount));
    let session = test_support::session(memory_only, "Flask");
    launcher.model.apply(test_support::signed_in(session, wows_toolkit_launcher::SessionStorage::MemoryOnly));
    launcher.settle();

    [("eu", eu()), ("na", ready), ("asia", unencrypted), ("eu", expired), ("eu", locked), ("asia", memory_only)]
        .into_iter()
        .map(|(realm, key)| format!("launcher-account-{realm}-{}", key.account_id.get()))
        .collect()
}

/// Where every row's status and menu fall outside the popover, as messages.
fn outside_the_popover(launcher: &LauncherWindow, rows: &[String]) -> Vec<String> {
    let bounds = |id: &str| {
        let element = launcher.window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
        launcher.window.bounds(element).unwrap_or_else(|| panic!("{id} has a box"))
    };
    let popover = bounds("launcher-popover");

    rows.iter()
        .flat_map(|row| [format!("{row}-status"), format!("{row}-actions")])
        .filter_map(|id| {
            let part = bounds(&id);
            let inside = part.size.width.0 > 0.0 && popover.contains_rect(part);
            (!inside).then(|| format!("{id} at {part:?} is outside the popover at {popover:?}"))
        })
        .collect()
}

#[test]
fn each_account_row_keeps_its_status_and_menu_inside_the_popover() {
    let mut launcher = LauncherWindow::open();
    let rows = crowded(&mut launcher);
    launcher.press("launcher-play-menu");

    let mut outside = outside_the_popover(&launcher, &rows);
    // The longest statuses are German, Russian and Polish.
    let locale = launcher.shell.services().locale;
    for code in ["de", "ru", "pl"] {
        locale.choose(crate::i18n::LanguageCode::new(code));
        launcher.settle();
        outside.extend(outside_the_popover(&launcher, &rows).into_iter().map(|said| format!("{code}: {said}")));
    }
    locale.choose(crate::i18n::LanguageCode::new("en"));

    assert!(outside.is_empty(), "{outside:#?}");
}

#[test]
fn the_account_menu_makes_an_account_the_default_and_removes_one() {
    let mut launcher = ready();
    launcher.press("launcher-play-menu");

    launcher.press("launcher-account-na-1034874630-actions");
    launcher.press("launcher-account-na-1034874630-make-default");
    assert_eq!(launcher.model.with_untracked(|state| state.settings.as_ref().unwrap().default_account), Some(na()));

    launcher.press("launcher-account-eu-697769785-actions");
    launcher.press("launcher-account-eu-697769785-forget");
    let saved = launcher.model.with_untracked(|state| state.settings.as_ref().unwrap().accounts.clone());
    assert_eq!(saved.iter().map(|account| account.key).collect::<Vec<_>>(), [na()]);
}

#[test]
fn signing_out_keeps_the_account_listed() {
    let mut launcher = ready();
    launcher.press("launcher-play-menu");

    launcher.press("launcher-account-eu-697769785-actions");
    launcher.press("launcher-account-eu-697769785-sign-out");

    let saved = launcher.model.with_untracked(|state| state.settings.as_ref().unwrap().accounts.clone());
    assert_eq!(saved.iter().map(|account| account.key).collect::<Vec<_>>(), [eu(), na()]);
    let status = launcher.model.with_untracked(|state| state.launcher.account_status(eu()));
    assert_eq!(status, wows_toolkit_launcher::AccountStatus::SignedOut);
    assert!(launcher.has("launcher-account-eu-697769785-sign-in"), "the row offers to sign in again");
}

#[test]
fn each_signed_in_row_says_when_its_session_ends() {
    let mut launcher = ready();
    let left = Duration::from_secs(((3 * 24 + 4) * 60 + 30) * 60);
    launcher.set_session(eu(), TestSession::SignedInUntil("Captain_Ahab".into(), std::time::SystemTime::now() + left));
    launcher.set_session(na(), TestSession::SignedInUntil("Ishmael".into(), std::time::SystemTime::UNIX_EPOCH));
    launcher.press("launcher-play-menu");

    assert_eq!(launcher.window.texts_by_class("wt-account-row__expiry"), ["Expires in 3d 4h", "Session expired"]);
}

#[test]
fn an_open_row_turns_expired_when_the_deadline_passes() {
    let mut launcher = ready();
    let left = Duration::from_secs(3);
    launcher.set_session(eu(), TestSession::SignedInUntil("Captain_Ahab".into(), std::time::SystemTime::now() + left));
    launcher.press("launcher-play-menu");
    let before = launcher.window.texts_by_class("wt-account-row__expiry");

    std::thread::sleep(left + Duration::from_millis(200));
    launcher.window.advance(Duration::from_secs(60));

    assert_eq!(before, ["Expires in less than a minute", "Expiry unknown"]);
    assert_eq!(launcher.window.texts_by_class("wt-account-row__expiry"), ["Session expired", "Expiry unknown"]);
    assert!(launcher.window.texts_by_class("wt-launcher__warn").contains(&"Session expired".to_owned()));
}

#[test]
fn without_an_install_the_popover_opens_the_setup_on_its_game_step() {
    let mut launcher = LauncherWindow::open();
    let mut settings = launcher.enabled_settings(&[]);
    settings.accounts.clear();
    launcher.adopt(settings);
    launcher.model.apply(test_support::installs_found(Vec::new()));
    launcher.settle();
    launcher.press("launcher-play-menu");
    assert!(launcher.has("launcher-install-none"));

    launcher.press("launcher-install-set-up");

    assert!(!launcher.has("launcher-popover"), "the popover closes");
    assert!(launcher.has("launcher-setup-game"));
    assert!(launcher.has("launcher-setup-choose-folder"));
}

#[test]
fn without_an_install_the_popover_adds_a_folder() {
    let mut launcher = LauncherWindow::open();
    let mut settings = launcher.enabled_settings(&[]);
    settings.accounts.clear();
    launcher.adopt(settings);
    launcher.model.apply(test_support::installs_found(Vec::new()));
    launcher.settle();
    launcher.press("launcher-play-menu");

    let folder = launcher.install.path().to_path_buf();
    wows_toolkit_services::dialog::pick_next(folder.clone());
    launcher.press("launcher-install-add");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);

    let manual = launcher.model.with_untracked(|state| state.settings.as_ref().unwrap().manual_installs.clone());
    assert_eq!(manual, [folder]);
}

#[test]
fn each_account_row_marks_its_realm_with_a_regional_globe() {
    let mut launcher = LauncherWindow::open();
    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab"), (na(), "Queequeg"), (asia(), "Starbuck")]);
    launcher.adopt(settings);
    launcher.found(up_to_date());
    launcher.press("launcher-play-menu");

    for (id, icon, name) in [
        ("launcher-account-eu-697769785-realm", "earth-europe", "Europe"),
        ("launcher-account-na-1034874630-realm", "earth-americas", "North America"),
        ("launcher-account-asia-2011532779-realm", "earth-asia", "Asia"),
    ] {
        let globe = launcher.window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
        assert_eq!(launcher.window.attribute(globe, "data-icon").as_deref(), Some(icon), "{id}");
        assert_eq!(launcher.window.a11y(globe).and_then(|state| state.label).as_deref(), Some(name), "{id}");
    }
}

#[test]
fn the_realm_picker_offers_eu_na_and_asia_only() {
    let mut launcher = ready();
    launcher.press("launcher-play-menu");
    launcher.press("launcher-add-account");

    for realm in ["eu", "na", "asia"] {
        assert!(launcher.has(&format!("launcher-realm-{realm}")), "{realm} is offered");
    }
    assert!(!launcher.has("launcher-realm-ru"), "RU is not offered");
}

#[test]
fn adding_an_account_with_two_factor_saves_it() {
    let mut launcher = ready();
    launcher.press("launcher-play-menu");
    launcher.press("launcher-add-account");
    launcher.script(ScriptedSignIn::TwoFactor);
    let asia = AccountKey { realm: wgc_auth::AccountRealm::Asia, account_id: wgc_auth::AccountId::new(2_051_681_179) };
    launcher.script(ScriptedSignIn::WrongCode);
    launcher.script(ScriptedSignIn::Succeed(test_support::session(asia, "Starbuck")));

    launcher.press("launcher-realm-asia");
    launcher.type_into("launcher-email", "starbuck@example.com");
    launcher.type_into("launcher-password", "hunter2");
    launcher.enter();
    assert!(launcher.has("launcher-code"), "the challenge asks for the code");

    launcher.type_into("launcher-code", "123456");
    launcher.enter();
    assert!(launcher.has("launcher-login-error"), "a wrong code keeps the challenge");
    assert!(launcher.has("launcher-code"));

    launcher.type_into("launcher-code", "654321");
    launcher.enter();

    assert!(!launcher.has("launcher-sign-in-dialog"), "the dialog closes once signed in");
    let attempts = launcher.attempts();
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[0].realm, wgc_auth::AccountRealm::Asia);
    assert_eq!(attempts[0].email, "starbuck@example.com");
    assert_eq!(attempts[2].code.as_deref(), Some("654321"));
    let saved = saved_launcher_row().expect("the sign-in saved the launcher row");
    let names: Vec<&str> =
        saved["accounts"].as_array().unwrap().iter().filter_map(|a| a["nickname"].as_str()).collect();
    assert_eq!(names, ["Captain_Ahab", "Ishmael", "Starbuck"]);
}

#[test]
fn enter_signs_in_only_with_a_plausible_email_and_a_password() {
    let mut launcher = ready();
    launcher.press("launcher-play-menu");
    launcher.press("launcher-add-account");

    launcher.type_into("launcher-password", "hunter2");
    launcher.enter();
    assert!(launcher.attempts().is_empty(), "no email");

    launcher.type_into("launcher-email", "not-an-email");
    launcher.enter();
    assert!(launcher.attempts().is_empty(), "not an email");

    launcher.script(ScriptedSignIn::Fail(wrong_password()));
    launcher.type_into("launcher-email", "@example.com");
    launcher.enter();
    assert_eq!(launcher.attempts().len(), 1, "a plausible email and a password sign in");
    assert!(launcher.has("launcher-login-error"), "and the refusal is said on the form");
}

fn wrong_password() -> wgc_auth::AuthError {
    wgc_auth::AuthError::InvalidCredentials { parameters: wgc_auth::CredentialParameters::UsernameOrPassword }
}

/// Signs a new account in, with WGNI answering `refusal`.
fn refused_sign_in(refusal: wgc_auth::AuthError) -> LauncherWindow {
    let mut launcher = ready();
    launcher.press("launcher-play-menu");
    launcher.press("launcher-add-account");
    launcher.script(ScriptedSignIn::Fail(refusal));
    launcher.type_into("launcher-email", "ahab@example.com");
    launcher.type_into("launcher-password", "hunter2");
    launcher.enter();

    launcher
}

#[test]
fn a_wrong_password_says_so_in_words() {
    let launcher = refused_sign_in(wrong_password());

    assert_eq!(launcher.text_of("wt-launcher__error"), "Wrong email or password.");
}

#[test]
fn an_unclassified_refusal_keeps_the_report_out_of_the_form() {
    let detail = "status 418 <html>teapot</html>";
    let launcher = refused_sign_in(wgc_auth::AuthError::BadResponse { detail: detail.into() });

    let shown = launcher.text_of("wt-launcher__error");
    assert_eq!(shown, "Sign-in failed. The log has the details.");
    for leaked in [detail, "unexpected response from WGNI", "WGNI"] {
        assert!(!shown.contains(leaked), "{leaked:?} leaked into {shown:?}");
    }
}

#[test]
fn every_sign_in_failure_has_a_sentence() {
    for failure in wows_toolkit_launcher::SignInFailure::ALL {
        let key = wows_toolkit_launcher::sign_in_failure_key(failure);
        let rendered = rust_i18n::t!(key).into_owned();
        assert_ne!(rendered, key, "{failure:?} has no catalog entry");
    }
}

#[test]
fn enter_in_the_code_field_does_nothing_while_it_is_empty() {
    let mut launcher = ready();
    launcher.press("launcher-play-menu");
    launcher.press("launcher-add-account");
    launcher.script(ScriptedSignIn::TwoFactor);
    launcher.type_into("launcher-email", "ahab@example.com");
    launcher.type_into("launcher-password", "hunter2");
    launcher.enter();

    launcher.press("launcher-code");
    launcher.enter();

    assert_eq!(launcher.attempts().len(), 1, "only the first sign-in was sent");
}

#[test]
fn the_keyring_is_read_only_once_the_popover_opens() {
    let mut launcher = LauncherWindow::open();
    let store = testing::store();
    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab")]);
    launcher.adopt(settings);
    launcher.found(up_to_date());
    assert_eq!(launcher.play_state(), "play", "an unread session does not hold Play back");
    assert_eq!(store.reads(), 0, "nothing reads the keyring before a session is needed");

    store.close();
    launcher.press("launcher-play-menu");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert!(launcher.has("launcher-keyring-hint"), "the hint is up while the read waits");
    assert_eq!(store.reads(), 1);

    store.open();
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert!(!launcher.has("launcher-keyring-hint"), "the hint goes once the keyring answers");
    assert!(launcher.has("launcher-account-eu-697769785-sign-in"), "no saved session means signing in");
}

#[test]
fn play_waits_for_an_unread_session_beside_the_keyring_hint() {
    let mut launcher = LauncherWindow::open();
    let store = testing::store();
    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab")]);
    launcher.adopt(settings);
    launcher.found(up_to_date());

    store.close();
    launcher.press("launcher-play");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert_eq!(store.reads(), 1);
    assert_eq!(launcher.play_state(), "restoring", "the press waits on the read");
    assert!(launcher.has("launcher-keyring-hint"), "the hint is beside the Play control");

    store.open();
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert!(!launcher.has("launcher-keyring-hint"), "the hint goes once the keyring answers");
    assert_eq!(launcher.play_state(), "sign-in-again", "the store held no session");
    assert!(launcher.has("launcher-sign-in-dialog"), "the waiting press asks for a sign-in");
}

/// The launcher with EU and NA accounts, a Play press for the default EU account waiting on a
/// closed store.
fn waiting_play() -> (LauncherWindow, std::sync::Arc<testing::TestStore>) {
    let mut launcher = LauncherWindow::open();
    let store = testing::store();
    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab"), (na(), "Ishmael")]);
    launcher.adopt(settings);
    launcher.found(up_to_date());

    store.close();
    launcher.press("launcher-play");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert!(launcher.model.pending_play().is_some(), "the press waits on the read");
    (launcher, store)
}

#[test]
fn turning_the_launcher_off_drops_a_waiting_play() {
    let (mut launcher, store) = waiting_play();

    launcher.model.disable();
    store.open();
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    launcher.model.enable();
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);

    assert_eq!(launcher.model.pending_play(), None);
    assert!(!launcher.has("launcher-sign-in-dialog"), "turning it back on is not a press");
}

#[test]
fn a_new_default_account_drops_a_waiting_play() {
    let (mut launcher, store) = waiting_play();

    launcher.model.edit_settings(|settings| {
        settings.set_default_account(na());
    });
    store.open();
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);

    assert_eq!(launcher.model.pending_play(), None);
    assert!(!launcher.has("launcher-sign-in-dialog"), "the old default is not played");
}

#[test]
fn a_new_default_keeps_a_waiting_play_for_a_chosen_account() {
    let mut launcher = LauncherWindow::open();
    let store = testing::store();
    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab"), (na(), "Ishmael")]);
    launcher.adopt(settings);
    launcher.found(up_to_date());

    // "Play as" for the account that is the default when pressed.
    store.close();
    crate::launcher::play::play(launcher.shell, launcher.model, eu(), false, PlayOrigin::Chosen);
    launcher.model.edit_settings(|settings| {
        settings.set_default_account(na());
    });
    store.open();
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);

    assert_eq!(launcher.model.pending_play(), None, "the read answered");
    assert!(launcher.has("launcher-sign-in-dialog"), "the chosen account is still played");
}

#[test]
fn forgetting_the_account_drops_a_waiting_play() {
    let (mut launcher, store) = waiting_play();

    launcher.model.edit_settings(|settings| settings.accounts.retain(|account| account.key != eu()));
    launcher.settle();
    assert_eq!(launcher.model.pending_play(), None, "nothing waits on the forgotten account");
    store.open();
}

#[test]
fn adding_the_first_account_says_the_keyring_may_ask() {
    let mut launcher = LauncherWindow::open();
    let store = testing::store();
    let settings = launcher.enabled_settings(&[]);
    launcher.adopt(settings);
    launcher.found(up_to_date());

    store.close();
    crate::launcher::account::open_sign_in(
        launcher.shell,
        launcher.model,
        wows_toolkit_launcher::LoginTarget::NewAccount,
    );
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert!(launcher.has("launcher-sign-in-dialog"));
    assert_eq!(store.reads(), 1, "the dialog asks the store where a sign-in will be kept");
    assert!(launcher.has("launcher-keyring-hint"), "the hint is up while the store waits");

    store.open();
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    assert!(!launcher.has("launcher-keyring-hint"), "the hint goes once the keyring answers");
}

#[test]
fn without_a_secret_store_the_dialog_and_the_row_say_the_session_is_not_kept() {
    let mut launcher = ready();
    launcher.model.update_launcher(|launcher| launcher.reset_login(wows_toolkit_launcher::LoginTarget::NewAccount));
    launcher.model.apply(test_support::keyring_probed(KeyringState::Unavailable));
    let asia = AccountKey { realm: wgc_auth::AccountRealm::Asia, account_id: wgc_auth::AccountId::new(2_051_681_179) };
    launcher.model.apply(test_support::signed_in_unencrypted(test_support::session(asia, "Starbuck")));
    launcher.settle();

    launcher.press("launcher-play-menu");
    assert!(launcher.has("launcher-account-asia-2051681179-not-saved"));
    launcher.press("launcher-add-account");
    assert!(launcher.has("launcher-keyring-missing"));
    assert!(launcher.has("launcher-keyring-help"));
}

#[test]
fn set_up_game_opens_the_four_step_setup() {
    let mut launcher = LauncherWindow::open();
    let mut settings = launcher.enabled_settings(&[]);
    settings.accounts.clear();
    launcher.adopt(settings);
    launcher.model.apply(test_support::installs_found(Vec::new()));
    launcher.settle();

    launcher.press("launcher-play");

    assert!(launcher.has("launcher-setup"));
    for step in ["game", "proton", "accounts", "replays"] {
        assert!(launcher.has(&format!("launcher-setup-step-{step}")), "{step} is a step");
    }
    assert!(launcher.has("launcher-setup-game"));
    assert!(launcher.has("launcher-setup-none-found"));
    launcher.press("launcher-setup-next");
    assert!(launcher.has("launcher-setup-proton"));
    assert!(launcher.has("launcher-setup-proton-ready"));
    launcher.press("launcher-setup-next");
    assert!(launcher.has("launcher-setup-accounts"));
    assert!(launcher.has("launcher-email"), "the step signs an account in");
    launcher.press("launcher-setup-next");
    assert!(launcher.has("launcher-setup-replays"));
    launcher.press("launcher-setup-next");
    assert!(!launcher.has("launcher-setup"), "Done closes it");
}

#[test]
fn the_setup_signs_in_with_enter_too() {
    let mut launcher = LauncherWindow::open();
    let settings = launcher.enabled_settings(&[]);
    launcher.adopt(settings);
    launcher.press("launcher-play");
    launcher.press("launcher-setup-next");
    launcher.press("launcher-setup-next");
    launcher.script(ScriptedSignIn::Succeed(test_support::session(eu(), "Captain_Ahab")));

    launcher.type_into("launcher-email", "ahab@example.com");
    launcher.type_into("launcher-password", "hunter2");
    launcher.enter();

    assert!(launcher.has("launcher-setup-account-697769785"), "the account joins the list");
}

#[test]
fn the_drawer_lists_the_launcher_rows_once_enabled() {
    let mut launcher = ready();
    open_settings_drawer(&mut launcher);

    for id in [
        "launcher-enabled",
        "launcher-installs",
        "launcher-install-0",
        "launcher-runner-detect",
        "launcher-extra-args",
        "launcher-env-vars",
        "launcher-safe-mode-args",
    ] {
        assert!(launcher.has(id), "{id} is shown");
    }
}

#[test]
fn launch_arguments_are_saved_when_the_field_is_left() {
    let mut launcher = ready();
    open_settings_drawer(&mut launcher);

    launcher.shell.settings_drawer.open_at(wows_toolkit_viewmodel::settings_drawer::Setting::GameLaunchArgs);
    launcher.settle();
    launcher.type_into("launcher-extra-args", "-console");
    launcher.press("launcher-env-vars");

    let saved = saved_launcher_row().expect("leaving the field saved the launcher row");
    assert_eq!(saved["extra_args"], serde_json::json!(["-console"]));
}

#[test]
fn set_up_proton_opens_the_drawer_at_its_row() {
    let mut launcher = LauncherWindow::open();
    let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab")]);
    launcher.adopt(settings);
    launcher.runner_missing();

    launcher.press("launcher-play");

    assert!(launcher.shell.settings_drawer.is_open());
    assert!(launcher.has("launcher-runner-missing"));
    assert!(launcher.has("launcher-runner-detect"));
}

/// Lays out the game settings fixtures as the fake install.
fn with_preferences(launcher: &LauncherWindow) {
    let root = launcher.install.path();
    std::fs::write(root.join("preferences.xml"), include_str!("fixtures/preferences.xml")).unwrap();
    std::fs::write(root.join("user_preferences.xml"), include_str!("fixtures/user_preferences.xml")).unwrap();
    let res = root.join("bin/13533378/res");
    std::fs::create_dir_all(&res).unwrap();
    std::fs::write(res.join("engine_config.xml"), include_str!("fixtures/engine_config.xml")).unwrap();
}

#[test]
fn game_settings_show_the_files_and_lock_while_the_game_runs() {
    let mut launcher = ready();
    with_preferences(&launcher);

    launcher.press("launcher-play-menu");
    launcher.press("launcher-open-game-settings");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);

    assert!(launcher.has("launcher-game-settings"));
    for id in ["launcher-gs-window-mode", "launcher-gs-resolution", "launcher-gs-battle-fps", "launcher-gs-port-fps"] {
        assert!(launcher.has(id), "{id} is shown");
    }
    assert!(!launcher.has("launcher-game-settings-locked"));

    let install = launcher.install.path().to_path_buf();
    launcher.set_game(GameState::Running { account: Some(eu()), install });
    assert!(launcher.has("launcher-game-settings-locked"));
}

#[test]
fn saving_game_settings_writes_the_changed_value_only() {
    let mut launcher = ready();
    with_preferences(&launcher);
    launcher.press("launcher-play-menu");
    launcher.press("launcher-open-game-settings");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);

    launcher.press("launcher-gs-battle-fps-input");
    launcher.window.type_text("144");
    launcher.press("launcher-game-settings-save");
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);

    assert!(!launcher.has("launcher-game-settings"), "a save closes the dialog");
    let written = std::fs::read_to_string(launcher.install.path().join("preferences.xml")).unwrap();
    assert!(written.contains("<maxFrameRate>\t144\t</maxFrameRate>"), "{written}");
    assert!(written.contains("<maxDockFrameRate>\t75\t</maxDockFrameRate>"), "the rest is kept");
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the launcher"]
fn the_launcher_surfaces_render() {
    let shots = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots");
    let save = |launcher: &mut LauncherWindow, name: &str, suffix: &str| {
        let path = shots.join(format!("launcher-{name}-{suffix}.png"));
        launcher.window.save_png(&path).expect("the window is drawn");
        println!("{}", path.display());
    };
    for (appearance, suffix) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
        let mut launcher = LauncherWindow::open_drawn(appearance);
        let settings = launcher.enabled_settings(&[(na(), "Captain_Ahab"), (eu(), "Queequeg"), (asia(), "Starbuck")]);
        launcher.adopt(settings);
        launcher.found(up_to_date());
        launcher.set_session(na(), TestSession::SignedIn("Captain_Ahab".into()));
        launcher.set_session(eu(), TestSession::Expired);
        save(&mut launcher, "play", suffix);

        launcher.press("launcher-play-menu");
        save(&mut launcher, "popover", suffix);
        launcher.window.press_key(NamedKey::Escape, Modifiers::NONE);
        launcher.settle();

        launcher.found(update_available());
        save(&mut launcher, "update", suffix);

        with_preferences(&launcher);
        launcher.found(up_to_date());
        crate::launcher::game_settings::open(launcher.shell, launcher.model);
        crate::interaction_tests::settle_in_real_time(&mut launcher.window);
        save(&mut launcher, "game-settings", suffix);
        launcher.press("launcher-game-settings-cancel");

        crate::launcher::setup::open(launcher.shell, launcher.model);
        launcher.settle();
        save(&mut launcher, "setup", suffix);
        launcher.press("launcher-setup-skip");

        crate::launcher::account::open_sign_in(
            launcher.shell,
            launcher.model,
            wows_toolkit_launcher::LoginTarget::NewAccount,
        );
        launcher.model.apply(test_support::keyring_probed(KeyringState::Unavailable));
        launcher.settle();
        save(&mut launcher, "add-account", suffix);
        drop(launcher);

        let mut launcher = LauncherWindow::open_drawn(appearance);
        launcher.adopt(LauncherSettings::default());
        crate::launcher::opt_in::ask_to_enable(launcher.shell, launcher.model);
        launcher.settle();
        save(&mut launcher, "opt-in", suffix);
        drop(launcher);

        let mut launcher = LauncherWindow::open_drawn(appearance);
        let store = testing::store();
        let settings = launcher.enabled_settings(&[(eu(), "Captain_Ahab")]);
        launcher.adopt(settings);
        launcher.found(up_to_date());
        store.close();
        launcher.press("launcher-play");
        crate::interaction_tests::settle_in_real_time(&mut launcher.window);
        save(&mut launcher, "keyring-hint", suffix);
        store.open();
    }
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the account popover"]
fn the_account_popover_renders() {
    let shots = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots");
    let save = |launcher: &mut LauncherWindow, name: &str, suffix: &str| {
        let path = shots.join(format!("launcher-popover-{name}-{suffix}.png"));
        launcher.window.save_png(&path).expect("the window is drawn");
        println!("{}", path.display());
    };
    for (appearance, suffix) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
        let mut launcher = LauncherWindow::open_drawn(appearance);
        crowded(&mut launcher);
        launcher.press("launcher-play-menu");
        save(&mut launcher, "accounts", suffix);

        launcher.press("launcher-account-na-1034874630-actions");
        save(&mut launcher, "menu", suffix);
        launcher.window.press_key(NamedKey::Escape, Modifiers::NONE);
        launcher.settle();

        let locale = launcher.shell.services().locale;
        locale.choose(crate::i18n::LanguageCode::new("de"));
        launcher.settle();
        save(&mut launcher, "accounts-de", suffix);
        locale.choose(crate::i18n::LanguageCode::new("en"));
        drop(launcher);

        let mut launcher = LauncherWindow::open_drawn(appearance);
        let mut settings = launcher.enabled_settings(&[]);
        settings.accounts.clear();
        launcher.adopt(settings);
        launcher.model.apply(test_support::installs_found(Vec::new()));
        launcher.settle();
        launcher.press("launcher-play-menu");
        save(&mut launcher, "no-install", suffix);
    }
}

/// The game build of the fake install.
fn fake_build() -> wows_toolkit_launcher::GameBuild {
    wows_toolkit_launcher::GameBuild::new(13_533_378)
}

fn steam_install(path: &Path) -> GameInstall {
    GameInstall {
        source: InstallSource::Steam { app_id: wgc_install::steam::WOWS_STEAM_APP_ID, library: path.to_path_buf() },
        auto_update: false,
        ..wgc_install(path)
    }
}

/// The fake install found as a Steam install whose WGC layer stands at `layer`, with one
/// signed-in account when `signed_in` and the Wargaming login on when `on`.
fn steam_window(launcher: LauncherWindow, signed_in: bool, on: bool, layer: Option<WgcLayerState>) -> LauncherWindow {
    let mut launcher = launcher;
    let path = launcher.install.path().to_path_buf();
    let accounts: &[(AccountKey, &str)] = if signed_in { &[(eu(), "Captain_Ahab")] } else { &[] };
    let mut settings = launcher.enabled_settings(accounts);
    if on {
        settings.wgc_login_installs.push(path.clone());
    }
    launcher.adopt(settings);
    let install = steam_install(&path);
    launcher.model.update_launcher(|state| test_support::set_installs(state, vec![install]));
    if let Some(layer) = layer {
        launcher.model.update_launcher(|state| test_support::set_wgc_layer(state, path, fake_build(), layer));
    }
    if signed_in {
        launcher.set_session(eu(), TestSession::SignedIn("Captain_Ahab".into()));
    }
    launcher.settle();

    launcher
}

fn ready_layer(launcher: &LauncherWindow) -> WgcLayerState {
    test_support::ready_wgc_layer(launcher.install.path().join("layer"), fake_build())
}

fn layer_unavailable() -> WgcLayerState {
    WgcLayerState::Unavailable(Arc::new(wows_toolkit_launcher::WgcLayerError::NoLocalePart))
}

fn layer_downloading() -> WgcLayerState {
    WgcLayerState::Fetching { done_bytes: 39_000_000, total_bytes: 78_000_000 }
}

/// The Settings drawer at the installs row.
fn open_installs(launcher: &mut LauncherWindow) {
    launcher.shell.settings_drawer.open_at(wows_toolkit_viewmodel::settings_drawer::Setting::GameInstalls);
    launcher.settle();
}

/// Whether the switch `test_id` is on, and whether it refuses a click.
fn switch_state(launcher: &mut LauncherWindow, test_id: &str) -> (Option<Toggled>, bool) {
    let element = launcher.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
    let state = launcher.window.a11y(element).unwrap_or_else(|| panic!("{test_id} is in the accessibility tree"));
    (state.checked, state.disabled)
}

const SETTINGS_SWITCH: &str = "launcher-wgc-login-0-switch";

fn uses_wgc_login(launcher: &LauncherWindow) -> bool {
    let path = launcher.install.path().to_path_buf();
    launcher
        .model
        .with_untracked(|state| state.settings.as_ref().is_some_and(|settings| settings.uses_wgc_login(&path)))
}

#[test]
fn a_steam_install_with_the_login_off_plays_through_steam() {
    let launcher = LauncherWindow::open();
    let layer = ready_layer(&launcher);
    let mut launcher = steam_window(launcher, true, false, Some(layer));
    assert_eq!(launcher.play_state(), "play-through-steam");
    assert_eq!(launcher.text_of("wt-play__label"), "Play through Steam");

    open_installs(&mut launcher);
    assert_eq!(switch_state(&mut launcher, SETTINGS_SWITCH), (Some(Toggled::False), false));
    assert_eq!(launcher.text_of("wt-launcher-wgc-status"), "Game Center files ready");

    launcher.press(SETTINGS_SWITCH);
    assert!(uses_wgc_login(&launcher), "the click turns the login on");
    assert_eq!(switch_state(&mut launcher, SETTINGS_SWITCH), (Some(Toggled::True), false));
}

#[test]
fn a_steam_install_with_the_login_on_plays_as_the_account() {
    let launcher = LauncherWindow::open();
    let layer = ready_layer(&launcher);
    let mut launcher = steam_window(launcher, true, true, Some(layer));
    assert_eq!(launcher.play_state(), "play");

    open_installs(&mut launcher);
    assert_eq!(switch_state(&mut launcher, SETTINGS_SWITCH), (Some(Toggled::True), false));

    launcher.press(SETTINGS_SWITCH);
    assert!(!uses_wgc_login(&launcher), "the click turns the login off");
    assert_eq!(launcher.play_state(), "play-through-steam");
}

#[test]
fn without_an_account_the_login_cannot_be_turned_on() {
    let launcher = LauncherWindow::open();
    let layer = ready_layer(&launcher);
    let mut launcher = steam_window(launcher, false, false, Some(layer));
    open_installs(&mut launcher);
    assert_eq!(switch_state(&mut launcher, SETTINGS_SWITCH), (Some(Toggled::False), true));
    assert_eq!(launcher.text_of("wt-launcher-wgc-status"), "Add a Wargaming account to the toolkit first.");
}

#[test]
fn the_login_waits_while_the_layer_is_looked_up() {
    let mut launcher = steam_window(LauncherWindow::open(), true, false, Some(WgcLayerState::Probing));
    open_installs(&mut launcher);
    assert_eq!(switch_state(&mut launcher, SETTINGS_SWITCH), (Some(Toggled::False), true));
    assert_eq!(launcher.text_of("wt-launcher-wgc-status"), "Looking for the Game Center files of this build...");
}

#[test]
fn the_login_says_why_a_build_has_no_layer() {
    let mut launcher = steam_window(LauncherWindow::open(), true, false, Some(layer_unavailable()));
    open_installs(&mut launcher);
    assert_eq!(switch_state(&mut launcher, SETTINGS_SWITCH), (Some(Toggled::False), true));
    assert_eq!(
        launcher.text_of("wt-launcher-wgc-status"),
        "Not available for this build: WG's update service lists no locale package for World of Warships"
    );
}

#[test]
fn the_login_shows_the_layer_download() {
    let mut launcher = steam_window(LauncherWindow::open(), true, true, Some(layer_downloading()));
    open_installs(&mut launcher);
    assert_eq!(switch_state(&mut launcher, SETTINGS_SWITCH), (Some(Toggled::True), false));
    assert_eq!(launcher.text_of("wt-launcher-wgc-status"), "Downloading the Game Center files: 50%");
}

#[test]
fn the_popover_offers_the_login_without_the_explanation() {
    let launcher = LauncherWindow::open();
    let layer = ready_layer(&launcher);
    let mut launcher = steam_window(launcher, true, false, Some(layer));
    launcher.press("launcher-play-menu");
    assert_eq!(switch_state(&mut launcher, "launcher-install-wgc-login-switch"), (Some(Toggled::False), false));
    assert!(!launcher.has("launcher-install-wgc-login-help"), "the popover leaves out the help link");
}

#[test]
fn a_wgc_install_offers_no_login_switch() {
    let mut launcher = ready();
    open_installs(&mut launcher);
    assert!(!launcher.has(SETTINGS_SWITCH));
    assert_eq!(launcher.play_state(), "play");
}

/// Makes the layer state a rendered case starts from.
type LayerFor = fn(&LauncherWindow) -> WgcLayerState;

#[test]
#[ignore = "draws on the graphics device; run by hand to review the Wargaming login switch"]
fn the_wgc_login_switch_renders() {
    let shots = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots");
    let save = |launcher: &mut LauncherWindow, name: &str| {
        let path = shots.join(format!("launcher-wgc-login-{name}.png"));
        launcher.window.save_png(&path).expect("the window is drawn");
        println!("{}", path.display());
    };
    let cases: [(&str, bool, bool, LayerFor); 6] = [
        ("off", true, false, ready_layer),
        ("on", true, true, ready_layer),
        ("needs-sign-in", false, false, ready_layer),
        ("checking", true, false, |_| WgcLayerState::Probing),
        ("unavailable", true, false, |_| layer_unavailable()),
        ("downloading", true, true, |_| layer_downloading()),
    ];
    for (name, signed_in, on, layer) in cases {
        let launcher = LauncherWindow::open_drawn(Appearance::Dark);
        let layer = layer(&launcher);
        let mut launcher = steam_window(launcher, signed_in, on, Some(layer));
        open_installs(&mut launcher);
        save(&mut launcher, name);
    }

    let launcher = LauncherWindow::open_drawn(Appearance::Dark);
    let layer = ready_layer(&launcher);
    let mut launcher = steam_window(launcher, true, false, Some(layer));
    save(&mut launcher, "play-through-steam");
    launcher.press("launcher-play-menu");
    save(&mut launcher, "popover");
}
