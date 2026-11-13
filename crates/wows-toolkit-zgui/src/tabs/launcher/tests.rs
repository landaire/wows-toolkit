//! The Launcher tab driven in the headless shell, with its state set through the launcher's events.

use std::num::NonZeroU32;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use landaire_ui::Appearance;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use tokio::runtime::Runtime;
use wgc_catalog::models::BuildVersion;
use wgc_catalog::models::ChainType;
use wgc_catalog::models::PartName;
use wgc_catalog::update::LatestBuild;
use wgc_catalog::update::PartStatus;
use wgc_install::GameInstall;
use wgc_install::InstallSource;
use wows_toolkit_launcher::CatalogCheckState;
use wows_toolkit_launcher::DownloadPhase;
use wows_toolkit_launcher::DownloadState;
use wows_toolkit_launcher::LauncherSettings;
use wows_toolkit_launcher::test_support;
use wows_toolkit_services::dialog::Asked;
use wows_toolkit_viewmodel::settings::keys;
use zgui::view;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Toggled;

use crate::interaction_tests::REVIEW_SIZE;
use crate::interaction_tests::returning_reader;
use crate::services::saved_for_test;
use crate::shell::AppRootProps;
use crate::tabs::launcher::model::LauncherModel;
use crate::tabs::launcher::testing;

/// The shell, open on the Launcher tab, with the tab's own startup held.
struct LauncherWindow {
    window: HeadlessWindow,
    model: LauncherModel,
    runtime: Arc<Runtime>,
}

impl LauncherWindow {
    /// Behaviour tests draw nothing and look at the shell at its review size.
    fn open() -> Self {
        Self::open_drawn(Drawing::Recorded, None, REVIEW_SIZE)
    }

    fn open_drawn(drawing: Drawing, desktop: Option<Appearance>, size: WindowSize) -> Self {
        let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
        returning_reader(&runtime);
        testing::hold_startup();

        let options = HeadlessOptions { drawing, desktop, ..HeadlessOptions::new(size) };
        let shown = Arc::clone(&runtime);
        let mut window = HeadlessWindow::open(options, move || {
            let runtime = Some(Arc::clone(&shown));
            view! { AppRoot(runtime = runtime) }
        })
        .expect("the shell opens headlessly");

        // The app's settings are read on the tokio runtime, in real time rather than the window's
        // clock; "Use for toolkit" edits them, so they have to have landed.
        for _ in 0..20 {
            window.advance(Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(25));
        }

        let tab = window.by_test_id("launcher").expect("the strip has a Launcher tab");
        window.click(tab).expect("the tab can be pressed");
        window.advance(Duration::from_millis(300));
        let model = testing::model().expect("the Launcher tab was built");

        Self { window, model, runtime }
    }

    /// Adopts `settings`, with every known game already checked so the tab asks no server.
    fn adopt(&mut self, settings: LauncherSettings) {
        self.model.adopt_quietly(settings);
        for game in wgc_catalog::known_games::known_games() {
            self.model.apply(test_support::game_checked(game.game_id, CatalogCheckState::Ready(fresh_build())));
        }
        self.settle();
    }

    fn settle(&mut self) {
        self.window.advance(Duration::from_millis(400));
    }

    fn has(&self, test_id: &str) -> bool {
        self.window.by_test_id(test_id).is_some()
    }

    fn press(&mut self, test_id: &str) {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.click(element).unwrap_or_else(|err| panic!("{test_id} can be pressed: {err}"));
        self.settle();
    }

    /// Finds `install` with its catalog check already answered, so none is sent.
    fn found(&mut self, installs: Vec<GameInstall>, catalog: impl Fn(&GameInstall) -> CatalogCheckState) {
        for install in &installs {
            self.model.apply(test_support::catalog_checked(install.path.clone(), catalog(install)));
        }
        self.model.apply(test_support::installs_found(installs));
        self.settle();
    }
}

fn wgc_install(path: &Path) -> GameInstall {
    GameInstall {
        path: path.to_path_buf(),
        version: Some(wowsunpack::data::Version { major: 14, minor: 3, patch: 0, build: NonZeroU32::new(9_876_543) }),
        build: None,
        source: InstallSource::Wgc { working_dir: path.to_path_buf() },
        product_id: None,
        auto_update: true,
    }
}

fn up_to_date() -> CatalogCheckState {
    CatalogCheckState::Ready(LatestBuild {
        chain_type: ChainType::Install,
        parts: vec![(PartName::Client, PartStatus::UpToDate)],
        downloads: Vec::new(),
        web_seed_threads: None,
    })
}

fn update_available() -> CatalogCheckState {
    CatalogCheckState::Ready(LatestBuild {
        chain_type: ChainType::Install,
        parts: vec![(
            PartName::Client,
            PartStatus::UpdateAvailable {
                from: BuildVersion::new("14.3.0.9876543".into()),
                to: BuildVersion::new("14.4.0.9912000".into()),
            },
        )],
        downloads: Vec::new(),
        web_seed_threads: None,
    })
}

fn fresh_build() -> LatestBuild {
    LatestBuild {
        chain_type: ChainType::Install,
        parts: vec![(PartName::Client, PartStatus::NotInstalled { to: BuildVersion::new("14.4.0.9912000".into()) })],
        downloads: Vec::new(),
        web_seed_threads: None,
    }
}

fn session(expires_in: Duration) -> wgc_auth::StoredSession {
    wgc_auth::StoredSession {
        realm: wgc_auth::AccountRealm::Eu,
        account_id: wgc_auth::AccountId::new(1),
        nickname: wgc_auth::Nickname::new("Captain_Ahab"),
        tokens: wgc_auth::TokenPair {
            access_token: wgc_auth::AccessToken::new("token".into()),
            exchange_code: wgc_auth::ExchangeCode::from_raw("code".into()),
        },
        expiry: wgc_auth::TokenExpiry::KnownAt(SystemTime::now() + expires_in),
    }
}

fn downloading(done_bytes: u64, total_bytes: u64) -> DownloadState {
    DownloadState::Running {
        phase: DownloadPhase::Downloading,
        done_bytes,
        total_bytes,
        current: "wows_client.pkg".into(),
    }
}

/// Settings with Proton configured, so no install asks for it.
fn configured() -> LauncherSettings {
    LauncherSettings {
        proton_binary: Some(PathBuf::from("/opt/proton/proton")),
        proton_prefix: Some(PathBuf::from("/var/lib/wows-prefix")),
        ..LauncherSettings::default()
    }
}

/// The launcher row saved last, as JSON.
fn saved_launcher_row() -> Option<serde_json::Value> {
    saved_for_test()
        .into_iter()
        .rev()
        .find(|(key, _)| *key == wows_toolkit_launcher::SETTINGS_KEY)
        .map(|(_, json)| serde_json::from_str(&json).expect("a saved row is JSON"))
}

#[test]
fn the_tab_waits_for_its_settings() {
    let launcher = LauncherWindow::open();

    assert!(launcher.has("launcher-loading"));
    assert!(!launcher.has("launcher-show-sign-in"));
}

#[test]
fn signing_in_shows_the_form_with_every_realm() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    assert!(!launcher.has("launcher-loading"));
    assert!(!launcher.has("launcher-username"), "the form waits to be asked for");

    launcher.press("launcher-show-sign-in");

    for id in ["launcher-username", "launcher-password", "launcher-sign-in"] {
        assert!(launcher.has(id), "{id} is shown");
    }
    for index in 0..wows_toolkit_launcher::REALMS.len() {
        assert!(launcher.has(&format!("launcher-realm-button-{index}")));
    }
}

#[test]
fn a_two_factor_challenge_asks_for_the_code() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());

    launcher.model.update_launcher(|state| {
        test_support::begin_sign_in(state, wgc_auth::AccountRealm::Eu, "captain".into());
    });
    launcher.model.apply(test_support::two_factor_required("challenge".into()));
    launcher.settle();

    for id in ["launcher-code", "launcher-backup-code", "launcher-submit-code"] {
        assert!(launcher.has(id), "{id} is shown");
    }
    assert!(!launcher.has("launcher-username"));
}

/// The words beside the backup code and safe mode boxes tick and untick them, as the boxes do.
#[test]
fn the_checkbox_labels_toggle_their_boxes() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    launcher.found(vec![wgc_install(Path::new("/games/World_of_Warships"))], |_| up_to_date());
    launcher.model.update_launcher(|state| {
        test_support::begin_sign_in(state, wgc_auth::AccountRealm::Eu, "captain".into());
    });
    launcher.model.apply(test_support::two_factor_required("challenge".into()));
    launcher.settle();

    let checked = |launcher: &mut LauncherWindow, id: &str| {
        let element = launcher.window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
        launcher.window.a11y(element).and_then(|state| state.checked)
    };
    for (label, control) in
        [("launcher-backup-code-label", "launcher-backup-code"), ("launcher-safe-mode-label", "launcher-safe-mode")]
    {
        assert_eq!(checked(&mut launcher, control), Some(Toggled::False), "{control} starts unticked");
        launcher.press(label);
        assert_eq!(checked(&mut launcher, control), Some(Toggled::True), "{label} ticks {control}");
        launcher.press(label);
        assert_eq!(checked(&mut launcher, control), Some(Toggled::False), "{label} unticks {control}");
    }
}

#[test]
fn a_refused_sign_in_says_why_on_the_form() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    launcher.press("launcher-show-sign-in");

    launcher.model.update_launcher(|state| {
        test_support::begin_sign_in(state, wgc_auth::AccountRealm::Na, "captain".into());
    });
    launcher.model.apply(test_support::sign_in_failed("invalid credentials".into()));
    launcher.settle();

    assert!(launcher.has("launcher-login-error"));
    assert!(launcher.has("launcher-sign-in"), "the form stays up for another try");
}

#[test]
fn a_captcha_notice_returns_to_the_form_on_retry() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());

    launcher.model.update_launcher(|state| {
        test_support::begin_sign_in(state, wgc_auth::AccountRealm::Eu, "captain".into());
    });
    launcher.model.apply(test_support::captcha_required());
    launcher.settle();
    assert!(launcher.has("launcher-captcha"));

    launcher.press("launcher-captcha-retry");

    assert!(!launcher.has("launcher-captcha"));
    assert!(launcher.has("launcher-username"), "retry goes straight to the form");
}

#[test]
fn a_signed_in_session_names_the_account_and_is_saved() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());

    launcher.model.apply(test_support::signed_in(session(Duration::from_secs(3 * 3600)), true));
    launcher.settle();

    assert!(launcher.has("launcher-session"));
    assert!(launcher.has("launcher-sign-out"));
    assert!(!launcher.has("launcher-show-sign-in"));
    let saved = saved_launcher_row().expect("the sign-in saved the launcher row");
    assert_eq!(saved["active_account"]["nickname"], "Captain_Ahab");
}

#[test]
fn no_installs_says_so() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());

    launcher.found(Vec::new(), |_| up_to_date());

    assert!(launcher.has("launcher-no-installs"));
}

#[test]
fn an_install_offers_launch_and_use_for_toolkit() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());

    launcher.found(vec![wgc_install(Path::new("/games/World_of_Warships"))], |_| update_available());

    for id in ["launcher-install-0", "launcher-recheck-0", "launcher-launch-0", "launcher-use-0"] {
        assert!(launcher.has(id), "{id} is shown");
    }
    assert!(launcher.has("launcher-catalog"));
    assert!(!launcher.has("launcher-no-installs"));
    assert!(!launcher.has("launcher-set-up-proton-0"), "Proton is configured");
}

#[test]
#[cfg(target_os = "linux")]
fn an_install_without_proton_offers_to_set_it_up() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(LauncherSettings::default());

    launcher.found(vec![wgc_install(Path::new("/games/World_of_Warships"))], |_| up_to_date());

    assert!(launcher.has("launcher-set-up-proton-0"));
}

#[test]
fn a_running_download_shows_its_progress_and_a_status_bar_job() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    let path = PathBuf::from("/games/World_of_Warships");
    launcher.found(vec![wgc_install(&path)], |_| update_available());

    launcher.model.apply(test_support::download_reported(path.clone(), downloading(3 << 30, 40 << 30)));
    launcher.settle();

    for id in ["launcher-progress-0", "launcher-pause-0", "launcher-cancel-0", "app-status-work"] {
        assert!(launcher.has(id), "{id} is shown");
    }

    launcher.model.apply(test_support::download_reported(path, DownloadState::Done { skipped_deltas: 0 }));
    launcher.window.advance(Duration::from_secs(1));

    assert!(!launcher.has("launcher-progress-0"));
    assert!(!launcher.has("app-status-work"), "the job leaves the status bar with the download");
}

/// The window closing takes the download's status bar job with it. Its handle writes the jobs
/// signal as it drops, which hangs if that happens while the tab's state is being disposed of.
#[test]
fn closing_the_window_during_a_download_finishes() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    let path = PathBuf::from("/games/World_of_Warships");
    launcher.found(vec![wgc_install(&path)], |_| up_to_date());
    launcher.model.apply(test_support::download_reported(path, downloading(1 << 30, 40 << 30)));
    launcher.settle();
    assert!(launcher.has("app-status-work"));

    drop(launcher);
}

#[test]
fn a_paused_download_offers_resume() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    let path = PathBuf::from("/games/World_of_Warships");
    launcher.found(vec![wgc_install(&path)], |_| up_to_date());

    launcher.model.apply(test_support::download_reported(path, DownloadState::Paused));
    launcher.settle();

    assert!(launcher.has("launcher-resume-0"));
    assert!(!launcher.has("launcher-pause-0"));
    assert!(!launcher.has("app-status-work"), "a paused download is not running work");
}

#[test]
fn adding_an_install_asks_for_a_folder() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    wows_toolkit_services::dialog::take_asked();

    launcher.press("launcher-add-install");

    assert_eq!(wows_toolkit_services::dialog::take_asked(), vec![Asked::Folder]);
}

#[test]
fn browsing_for_the_proton_binary_asks_for_a_file() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    wows_toolkit_services::dialog::take_asked();

    launcher.press("launcher-proton-binary");
    launcher.press("launcher-proton-prefix");

    assert_eq!(wows_toolkit_services::dialog::take_asked(), vec![Asked::File, Asked::Folder]);
}

#[test]
fn use_for_toolkit_sets_the_game_directory() {
    let install = tempfile::tempdir().expect("a temporary directory can be made");
    std::fs::create_dir(install.path().join("bin")).expect("the install has a bin directory");
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    launcher.found(vec![wgc_install(install.path())], |_| up_to_date());

    launcher.press("launcher-use-0");

    let wanted = serde_json::to_string(&install.path().to_string_lossy()).expect("a path serializes");
    assert!(
        saved_for_test().iter().any(|(key, json)| *key == keys::WOWS_DIR && *json == wanted),
        "the game directory was saved"
    );

    // The tests share one settings database, and the directory is removed with `install`: a
    // later shell would read it back and fail to load its game data.
    crate::interaction_tests::settle_in_real_time(&mut launcher.window);
    launcher.runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
        wows_toolkit_config::queries::delete_setting(&pool, keys::WOWS_DIR).await.expect("the setting is removed");
    });
}

#[test]
fn extra_args_are_saved_when_the_field_is_left() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());

    launcher.press("launcher-extra-args");
    launcher.window.type_text("-console");
    launcher.press("launcher-env-vars");

    let saved = saved_launcher_row().expect("leaving the field saved the launcher row");
    assert_eq!(saved["extra_args"], serde_json::json!(["-console"]));
}

/// An option field holds two lines when short and grows to show every line it is given.
#[test]
fn an_option_field_grows_with_its_lines() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    let height = |launcher: &LauncherWindow| {
        let field = launcher.window.by_test_id("launcher-env-vars").expect("the field is shown");
        launcher.window.bounds(field).expect("it is laid out").size.height.0
    };
    let short = height(&launcher);

    launcher.press("launcher-env-vars");
    for line in ["A=1", "B=2", "C=3", "D=4"] {
        launcher.window.type_text(line);
        launcher.window.press_key(NamedKey::Enter, Modifiers::NONE);
    }
    launcher.settle();

    let tall = height(&launcher);
    assert!(tall > short + 30.0, "four lines take more room than two: {short} then {tall}");
}

/// Each Proton Browse button sits after the path it sets, not across the page from it.
#[test]
fn the_proton_browse_buttons_follow_their_paths() {
    let mut launcher = LauncherWindow::open();
    launcher.adopt(configured());
    let bounds = |id: &str| {
        let element = launcher.window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
        launcher.window.bounds(element).expect("it is laid out")
    };

    let column = bounds("launcher-extra-args");
    for browse in ["launcher-proton-binary", "launcher-proton-prefix"] {
        let button = bounds(browse);
        let offset = button.origin.x.0 - column.origin.x.0;
        assert!(offset < 400.0, "{browse} sits {offset}px into a column {}px wide", column.size.width.0);
    }
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the tab"]
fn the_launcher_renders_in_its_main_states() {
    let shots = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots");
    for (appearance, suffix) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
        let mut launcher = LauncherWindow::open_drawn(Drawing::Device, Some(appearance), REVIEW_SIZE);
        launcher.adopt(configured());
        let wows = PathBuf::from("/games/World_of_Warships");
        let pts = PathBuf::from("/games/World_of_Warships_PT");
        launcher.found(vec![wgc_install(&wows), wgc_install(&pts)], |install| {
            if install.path == pts { update_available() } else { up_to_date() }
        });

        let path = shots.join(format!("launcher-signed-out-{suffix}.png"));
        launcher.window.save_png(&path).expect("the tab is drawn");
        println!("{}", path.display());

        launcher.model.apply(test_support::signed_in(session(Duration::from_secs(5 * 3600 + 20 * 60)), true));
        launcher.settle();
        let path = shots.join(format!("launcher-signed-in-{suffix}.png"));
        launcher.window.save_png(&path).expect("the tab is drawn");
        println!("{}", path.display());

        launcher.model.apply(test_support::download_reported(pts, downloading(17 << 30, 42 << 30)));
        launcher.window.advance(Duration::from_secs(1));
        let path = shots.join(format!("launcher-installing-{suffix}.png"));
        launcher.window.save_png(&path).expect("the tab is drawn");
        println!("{}", path.display());
        drop(launcher);

        // The whole page at once, with the sign-in form open, for the sections below the fold.
        let tall = WindowSize { width: REVIEW_SIZE.width, height: 1500.0 };
        let mut launcher = LauncherWindow::open_drawn(Drawing::Device, Some(appearance), tall);
        launcher.adopt(LauncherSettings::default());
        launcher.found(vec![wgc_install(&wows)], |_| up_to_date());
        launcher.press("launcher-show-sign-in");
        let path = shots.join(format!("launcher-page-{suffix}.png"));
        launcher.window.save_png(&path).expect("the tab is drawn");
        println!("{}", path.display());
    }
}
