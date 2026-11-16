//! What happens once the window is open: the settings read, and everything that waits on it.

use std::path::PathBuf;

use rust_i18n::t;
use sqlx::sqlite::SqlitePool;
use wows_toolkit_replay_data::force_reindex::version_said;
use wows_toolkit_replay_data::heap_profile;
use wows_toolkit_replay_data::replay_folder::ReplayFolder;
use wows_toolkit_replay_data::replay_index::IndexMode;
use wows_toolkit_services::constants::Checked;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::bug_report::ConstantsFetchStatus;
use wows_toolkit_viewmodel::bug_report::record_constants_fetch;
use wows_toolkit_viewmodel::first_run::adopt_found_install;
use wows_toolkit_viewmodel::first_run::adopt_system_locale;
use wows_toolkit_viewmodel::settings::keys;
use zgui::reactive::RenderEffect;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Set;
use zgui::task::blocking;
use zgui::task::spawn_detached;
use zgui::task::spawn_local;

use crate::i18n::LanguageCode;
use crate::long_job::long_job;
use crate::services::GameDataRequest;
use crate::services::GameDataState;
use crate::services::Services;
use crate::services::constants::Provenance;
use crate::services::constants::Wanted;
use crate::shell::ReplayRequest;
use crate::shell::Shell;
use crate::shell::crash;
use crate::shell::first_run;
use crate::shell::settings::Setting;
use crate::shell::update;
use crate::shell::update::Asked;

/// The message key a game directory that holds no install stays up under until it is fixed.
pub const STUCK_WOWS_DIR: &str = "wows-dir";

/// Reads the settings, and applies them when they land. Called once, from the shell's body.
pub fn begin(services: Services, shell: Shell) {
    announce_game_data(services, shell);
    maintain_game_data_cache(services);
    check_published_constants(services, shell);
    rebuild_stale_index(services, shell);

    let system_locale = sys_locale::get_locale();
    let loaded = services.on_tokio(async move {
        let pool = wows_toolkit_config::open_db().await?;
        // Before the settings are read, so the language question offers what was saved and the
        // game data loads from the install found.
        if let Err(err) = adopt_system_locale(&pool, system_locale.as_deref()).await {
            tracing::warn!("the machine's language was not saved: {err}");
        }
        let found = async { tokio::task::spawn_blocking(find_install).await.ok().flatten() };
        if let Err(err) = adopt_found_install(&pool, found).await {
            tracing::warn!("the install found was not saved: {err}");
        }
        let settings = StartupSettings::load(&pool).await;
        // The pool comes back so settings edits have somewhere to go.
        Ok::<_, sqlx::Error>((settings, pool))
    });

    spawn_local(async move {
        let loaded = loaded.await;
        heap_profile::mark("startup_settings.loaded");

        match loaded {
            Ok(Ok((settings, pool))) => apply(services, shell, settings, pool),
            // The defaults already applied stay, and the drawer reports the failure instead of
            // loading forever.
            Ok(Err(err)) => {
                tracing::error!("failed to load settings from the config DB: {err:#}");
                services.settings.fail(err.to_string());
            }
            Err(err) => {
                tracing::error!("settings-load task did not complete: {err}");
                services.settings.fail(err.to_string());
            }
        }
    });
}

/// What the game data needs from `settings`.
pub fn game_data_request(settings: &StartupSettings) -> GameDataRequest {
    GameDataRequest {
        wows_dir: (!settings.wows_dir.is_empty()).then(|| PathBuf::from(&settings.wows_dir)),
        cache_dir: settings.game_data_cache_dir.clone(),
        locale: settings.locale.clone().map(LanguageCode::new),
        auto_dump: settings.auto_dump_game_data,
    }
}

/// Whether `wows_dir` could hold an install: set, and with the `bin` directory every install has.
pub fn is_wows_dir_invalid(wows_dir: &str) -> bool {
    !wows_dir.is_empty() && !std::path::Path::new(wows_dir).join("bin").is_dir()
}

/// The first install the launcher's discovery or the installer's default path holds.
fn find_install() -> Option<PathBuf> {
    let standard = PathBuf::from(r"C:\Games\World_of_Warships");

    wgc_install::discover_all(None, &[standard])
        .into_iter()
        .map(|install| install.path)
        .find(|path| path.join("bin").is_dir())
}

/// Applies the settings read at startup.
pub fn apply(services: Services, shell: Shell, settings: StartupSettings, pool: SqlitePool) {
    shell.appearance.zoom.set(settings.zoom);
    shell.appearance.theme.set(settings.theme);
    let debug_mode_chosen = shell.take_debug_mode_before_load();
    shell.debug_mode.set(debug_mode_chosen.unwrap_or(settings.debug_mode));
    if let Some(code) = settings.locale.clone() {
        services.locale.choose(LanguageCode::new(code));
    }

    check_wows_dir(shell, settings.wows_dir.clone());
    services.game_data.adopt(game_data_request(&settings));

    let check_for_updates = settings.check_for_updates;
    let answered = first_run::Answered::of(&settings);
    services.settings.adopt(settings, pool);
    services.session.read_saved();
    if let Some(on) = debug_mode_chosen {
        shell.save_debug_mode(on);
    }

    // Before anything else the app does with the settings: what may be shared is the reader's to
    // answer.
    first_run::ask(services, shell, answered);
    crash::report_last_crash(shell);
    if check_for_updates {
        update::check_for_update(services, shell, Asked::AtStartup);
    }
    shell.restart_twitch_poll();
    shell.start_mcp_at_launch();
}

/// Marks the game directory read at startup when it holds no install, off the UI thread: the
/// directory may be on a share that is slow to answer.
pub(crate) fn check_wows_dir(shell: Shell, wows_dir: String) {
    let check = shell.settings_drawer.begin_wows_dir_check();
    // Detached: started once the settings land, outside any component. The check generation
    // drops the answer once the shell is gone or a directory has been looked at since.
    spawn_detached(async move {
        let invalid = blocking(move || is_wows_dir_invalid(&wows_dir)).await;
        if shell.settings_drawer.is_latest_wows_dir_check(check) != Some(true) {
            return;
        }

        shell.settings_drawer.wows_dir_invalid.set(invalid);
        if invalid {
            warn_wows_dir_invalid(shell);
        }
    });
}

/// Puts up the message that stays until the game directory is fixed or its button opens the
/// setting.
pub fn warn_wows_dir_invalid(shell: Shell) {
    let open = shell.settings_drawer;
    shell.toasts().show(
        landaire_ui::Notice::warn(t!("ui.messages.wows_dir_invalid").into_owned())
            .keyed(landaire_ui::ToastKey::new(STUCK_WOWS_DIR))
            .sticky()
            .action_keeping_open(t!("ui.settings.panel.open").into_owned(), move || {
                open.open_at(Setting::WowsDirectory);
            }),
    );
}

/// Says when the game data has loaded, or why it did not.
fn announce_game_data(services: Services, shell: Shell) {
    let announcing = RenderEffect::new(move |previous: Option<Option<bool>>| {
        let finished = match services.game_data.state() {
            GameDataState::Ready(_) => Some(true),
            GameDataState::Failed(reason) => {
                // Said once per failure, not again when something else reads the state.
                if previous != Some(Some(false)) {
                    shell.toasts().failed(reason);
                }
                Some(false)
            }
            GameDataState::NoInstall | GameDataState::Loading { .. } => None,
        };
        if finished == Some(true) && previous != Some(Some(true)) {
            shell.toasts().ok(t!("ui.messages.game_data_loaded").into_owned());
        }

        finished
    });
    on_cleanup_local(move || drop(announcing));
}

/// Tidies the game data cache once a session, as soon as a cache directory is set.
fn maintain_game_data_cache(services: Services) {
    let maintaining = RenderEffect::new(move |maintained: Option<bool>| {
        // Returns before reading the settings, so the effect stops running once it has started the
        // work.
        if maintained == Some(true) {
            return true;
        }
        let base = services
            .settings
            .read(|settings| wows_toolkit_services::game_data_cache::base(&settings.game_data_cache_dir))
            .flatten();
        let Some(base) = base else { return false };
        spawn_detached(long_job(move || wows_toolkit_services::game_data_cache::maintain(&base)));
        true
    });
    on_cleanup_local(move || drop(maintaining));
}

/// Once a session, as soon as the game data and the settings are in: asks whether the published
/// result mappings have moved since they were last written, and writes the newest over the
/// installed build's. A mapping a year old reads this year's results wrongly. Then brings the
/// installed build's mapping up to date, generating it from the client when nothing published
/// fits it.
fn check_published_constants(services: Services, shell: Shell) {
    let checking = RenderEffect::new(move |started: Option<bool>| {
        if started == Some(true) {
            return true;
        }
        let GameDataState::Ready(loaded) = services.game_data.state() else { return false };
        let Some(known) = services.settings.read(|settings| settings.constants_commit.clone()) else { return false };

        let installed =
            Wanted { build: loaded.build(), release: loaded.version().map(|version| version_said(&version)) };
        let checked = services.on_tokio(wows_toolkit_services::constants::check_latest(loaded.build(), known));
        spawn_local(async move {
            let outcome = checked.await.unwrap_or_else(|err| Checked::Failed(err.to_string()));
            constants_checked(services, shell, outcome);
            // After the check, which writes the newest published mapping whatever build it is for.
            update_installed_constants(shell, installed).await;
        });
        true
    });
    on_cleanup_local(move || drop(checking));
}

/// Brings the installed build's mapping up to date, and has the open replays read again through
/// one that arrived.
async fn update_installed_constants(shell: Shell, installed: Wanted) {
    #[cfg(test)]
    if !crate::tabs::replays::testing::fetches_allowed() {
        return;
    }

    let outcomes = crate::shell::constants::update(shell, vec![installed]).await;
    let arrived = outcomes
        .iter()
        .any(|(_, outcome)| matches!(outcome.provenance(), Some(Provenance::GitHub | Provenance::Generated { .. })));
    if arrived {
        shell.request_replay(ReplayRequest::ReparseOpen);
    }
}

fn constants_checked(services: Services, shell: Shell, outcome: Checked) {
    match outcome {
        Checked::Written { build, commit } => {
            record_constants_fetch(ConstantsFetchStatus::Succeeded);
            tracing::info!(build, "constants: the newest result mapping was written");
            // Stored as a nullable string, which is what the other front ends read.
            services.settings.edit(keys::CONSTANTS_FILE_COMMIT, |settings| {
                settings.constants_commit = Some(commit.clone());
                Some(commit)
            });
            shell.toasts().info(t!("ui.replay.constants_latest_written").into_owned());
            shell.request_replay(ReplayRequest::ReparseOpen);
        }
        Checked::UpToDate => record_constants_fetch(ConstantsFetchStatus::Succeeded),
        Checked::Failed(reason) => {
            record_constants_fetch(ConstantsFetchStatus::Failed(reason.clone()));
            // A notice rather than a failure: the cached mappings still read.
            tracing::warn!(%reason, "constants: the published mappings could not be checked");
            shell.toasts().warn(t!("ui.replay.constants_check_failed", reason = reason).into_owned());
        }
    }
}

/// Once a session, as soon as the game data is in: indexes the install's replays again when rows
/// an older parse wrote are out of date, so an update's fixes reach rows already indexed.
fn rebuild_stale_index(services: Services, shell: Shell) {
    let checking = RenderEffect::new(move |started: Option<bool>| {
        if started == Some(true) {
            return true;
        }
        if services.game_data.cache().is_none() {
            return false;
        }
        let Some(wows_dir) = services.settings.read(|settings| settings.wows_dir.clone()) else { return false };
        let Some(pool) = services.settings.pool() else { return false };
        if wows_dir.is_empty() {
            return true;
        }

        let wows_dir = PathBuf::from(wows_dir);
        spawn_local(async move {
            let dirs = blocking(move || ReplayFolder::from_app_install(&wows_dir).content_dirs()).await;
            let stale = services
                .on_tokio(async move {
                    let rateable_mc = crate::shell::replay_index::cached_rateable_mc();
                    wows_toolkit_replay_data::replay_index::has_stale_records_in(&pool, &dirs, rateable_mc).await
                })
                .await;
            match stale {
                Ok(Ok(true)) => shell.build_replay_index(IndexMode::FillGaps),
                Ok(Ok(false)) => {}
                Ok(Err(err)) => tracing::warn!(%err, "replay index revision could not be checked"),
                Err(err) => tracing::warn!(%err, "replay index revision check did not complete"),
            }
        });
        true
    });
    on_cleanup_local(move || drop(checking));
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use wows_toolkit_services::settings::StartupSettings;
    use wows_toolkit_viewmodel::settings::keys;
    use zgui::reactive::prelude::GetUntracked;
    use zgui::vocab::Modifiers;

    use crate::interaction_tests::open_bare_shell;
    use crate::interaction_tests::shell;
    use crate::services::saved_for_test;

    fn debug_mode_saves() -> Vec<String> {
        saved_for_test().into_iter().filter(|(key, _)| *key == keys::DEBUG_MODE).map(|(_, json)| json).collect()
    }

    /// The bare shell has no runtime, so the read is done here and the save is asked for but never
    /// written to the database the tests share.
    #[test]
    fn the_debug_shortcut_pressed_while_the_settings_load_is_saved_once_they_do() {
        let mut window = open_bare_shell();
        let earlier = debug_mode_saves().len();
        window.press_character('d', Modifiers::PRIMARY | Modifiers::SHIFT);
        window.advance(Duration::from_millis(400));
        assert!(shell().debug_mode.get_untracked(), "the shortcut turns debug mode on");
        assert_eq!(debug_mode_saves().len(), earlier, "nothing is saved before the settings load");

        let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
        let (mut settings, pool) = runtime.block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
            (StartupSettings::load(&pool).await, pool)
        });
        settings.debug_mode = false;
        super::apply(shell().services(), shell(), settings, pool);
        window.advance(Duration::from_millis(400));

        assert!(shell().debug_mode.get_untracked(), "the loaded settings do not undo the shortcut");
        assert_eq!(debug_mode_saves()[earlier..], ["true"], "the choice is saved over the loaded value");
        assert_eq!(shell().services().settings.read_untracked(|settings| settings.debug_mode), Some(true));
    }
}
