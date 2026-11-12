//! What happens once the window is open: the settings read, and everything that waits on it.

use std::path::PathBuf;

use rust_i18n::t;
use sqlx::sqlite::SqlitePool;
use wows_toolkit_replay_data::heap_profile;
use wows_toolkit_services::settings::StartupSettings;
use zgui::reactive::RenderEffect;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Set;
use zgui::task::blocking;
use zgui::task::spawn_detached;
use zgui::task::spawn_local;

use crate::i18n::LanguageCode;
use crate::services::GameDataRequest;
use crate::services::GameDataState;
use crate::services::Services;
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

    let loaded = services.on_tokio(async move {
        let pool = wows_toolkit_config::open_db().await?;
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

/// Applies the settings read at startup.
pub fn apply(services: Services, shell: Shell, settings: StartupSettings, pool: SqlitePool) {
    shell.appearance.zoom.set(settings.zoom);
    shell.appearance.theme.set(settings.theme);
    shell.debug_mode.set(settings.debug_mode);
    if let Some(code) = settings.locale.clone() {
        services.locale.choose(LanguageCode::new(code));
    }

    let invalid = is_wows_dir_invalid(&settings.wows_dir);
    shell.settings_drawer.wows_dir_invalid.set(invalid);
    if invalid {
        warn_wows_dir_invalid(shell);
    }
    services.game_data.adopt(game_data_request(&settings));

    let check_for_updates = settings.check_for_updates;
    let answered = first_run::Answered::of(&settings);
    services.settings.adopt(settings, pool);

    // Before anything else the app does with the settings: what may be shared is the reader's to
    // answer.
    first_run::ask(services, shell, answered);
    crash::report_last_crash(shell);
    if check_for_updates {
        update::check_for_update(services, shell, Asked::AtStartup);
    }
    shell.restart_twitch_poll();
}

/// Puts up the message that stays until the game directory is fixed, with a way to fix it.
pub fn warn_wows_dir_invalid(shell: Shell) {
    let open = shell.settings_drawer;
    shell.toasts().show(
        landaire_ui::Notice::warn(t!("ui.messages.wows_dir_invalid").into_owned())
            .keyed(landaire_ui::ToastKey::new(STUCK_WOWS_DIR))
            .sticky()
            .action(t!("ui.settings.panel.open").into_owned(), move || open.open_at(Setting::WowsDirectory)),
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
        spawn_detached(blocking(move || wows_toolkit_services::game_data_cache::maintain(&base)));
        true
    });
    on_cleanup_local(move || drop(maintaining));
}
