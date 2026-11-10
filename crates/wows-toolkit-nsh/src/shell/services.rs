use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;

use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::JsObject;
use nsh_core::prelude::background;
use nsh_core::prelude::blocking;
use nsh_core::prelude::module;

use crate::shell::error::SettingsError;
use crate::shell::error::ShellError;
use crate::shell::error::UpdateError;
use crate::shell::look;
use crate::shell::look::LookRevision;
use crate::shell::look::SharedLook;
use crate::shell::settings::parse_theme;
use crate::shell::settings::theme_name;
use crate::shell::theme;
use crate::shell::update::ReleaseOffer;

/// One language the interface can be read in.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct Language {
    pub code: String,
    /// The language's name in itself, which is how a reader finds their own.
    pub native_name: String,
}

/// One named palette the Settings tab offers.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ThemeOption {
    /// The stored name (`System`, `Dracula`, ...).
    pub id: String,
    pub label: String,
    /// Window, panel, text and accent, as `#rrggbb`.
    pub swatches: Vec<String>,
}

/// The shell's services: language, theme, updates and diagnostics.
#[module]
pub struct Shell;

#[module]
impl Shell {
    /// The catalog text for `key` in the current language, with each
    /// `%{name}` placeholder replaced from `args`. A key the catalog lacks
    /// reads as the key itself, which is how rust-i18n reports it.
    pub fn translate(key: String, args: Option<BTreeMap<String, String>>) -> String {
        let text = rust_i18n::t!(key.as_str()).into_owned();
        let Some(args) = args else {
            return text;
        };

        args.iter().fold(text, |text, (name, value)| text.replace(&format!("%{{{name}}}"), value))
    }

    /// Reads every catalog string in `code` from now on: this crate's and the
    /// shared view-model's, which rust-i18n keeps apart.
    pub fn set_locale(code: String) -> Result<(), SettingsError> {
        if !wt_translations::SUPPORTED_LANGUAGES.iter().any(|language| language.code == code) {
            return Err(SettingsError::UnknownLanguage { value: code });
        }
        rust_i18n::set_locale(&code);
        wows_toolkit_viewmodel::set_locale(&code);
        look::publish_locale(&code);

        Ok(())
    }

    /// The language catalog strings are read in now. The locale is the
    /// process's, so another window may have set it before this one started.
    pub fn locale() -> String {
        rust_i18n::locale().to_string()
    }

    /// The languages the game, and so the toolkit, is translated into.
    pub fn languages() -> Vec<Language> {
        wt_translations::SUPPORTED_LANGUAGES
            .iter()
            .map(|language| Language { code: language.code.to_owned(), native_name: language.native_name.to_owned() })
            .collect()
    }

    /// The palettes, in the order the other front ends list them, previewed
    /// as they draw on a desktop that is dark or not.
    pub fn themes(system_dark: bool) -> Vec<ThemeOption> {
        wows_toolkit_viewmodel::settings::ThemeChoice::ALL
            .into_iter()
            .map(|choice| ThemeOption {
                id: theme_name(choice).to_owned(),
                label: choice.label().to_owned(),
                swatches: theme::swatches(choice, system_dark),
            })
            .collect()
    }

    /// Draws the app in the palette named `name`, and in its appearance, and
    /// has the app's other windows follow.
    pub async fn apply_theme(name: String) -> Result<(), SettingsError> {
        draw_in(&name)?;
        look::publish_theme(&name);

        Ok(())
    }

    /// Draws this window in the palette another window applied, without
    /// announcing it again.
    pub async fn follow_theme(name: String) -> Result<(), SettingsError> {
        draw_in(&name)
    }

    /// The theme and language last applied in any window, once they differ
    /// from `after`: what the app's other windows follow.
    pub async fn next_look(after: LookRevision) -> SharedLook {
        look::next(after).await
    }

    /// Opens `url` in the desktop's browser. Resolves to whether it opened.
    pub async fn open_url(url: String) -> bool {
        nsh_core::prelude::open_url(url).await
    }

    /// Puts `text` on the clipboard.
    pub async fn copy_text(text: String) {
        nsh_core::prelude::set_clipboard(text);
    }

    /// What the last run's crash left behind, removed as it is read so it is
    /// reported once. `None` when the last run did not crash.
    pub fn take_crash_report() -> Option<String> {
        let report = wows_toolkit_viewmodel::logging::take_crash_report();
        // Kept for the session: a bug report copied later still names it.
        if let Some(report) = &report {
            *RECENT_CRASH.lock().unwrap_or_else(PoisonError::into_inner) = Some(report.clone());
        }

        report
    }

    /// Puts what a bug report needs on the clipboard: this build, the game
    /// build loaded, the result mappings and how their last fetch went, and
    /// the last crash, in the block the Discord channel asks for.
    pub async fn copy_bug_report_info() -> Result<(), SettingsError> {
        let settings = background(async {
            let pool = crate::shell::settings::open().await?;
            crate::shell::settings::load_from(&pool).await
        })
        .await
        .map_err(SettingsError::Task)??;
        let storage = wows_toolkit_config::storage_dir();
        let report = wows_toolkit_viewmodel::bug_report::BugReportInfo {
            constants_fetch: wows_toolkit_viewmodel::bug_report::constants_fetch_status(),
            application_version: env!("CARGO_PKG_VERSION").to_owned(),
            loaded_game_version: crate::game_data::newest_loaded()
                .and_then(|loaded| loaded.version())
                .map(wows_toolkit_viewmodel::bug_report::full_game_version),
            downloaded_constants: wows_toolkit_viewmodel::bug_report::latest_downloaded_constants(storage.as_deref()),
            constants_commit: settings.constants_commit,
            recent_crash: RECENT_CRASH.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        };
        nsh_core::prelude::set_clipboard(report.discord_code_block());

        Ok(())
    }

    /// Where the rotated log files are written.
    pub fn log_directory() -> String {
        wows_toolkit_viewmodel::logging::log_dir().display().to_string()
    }

    /// Puts the newest log file on the clipboard, for a bug report. `None`
    /// when there is no log file to copy.
    pub async fn copy_latest_log() -> Result<Option<LogCopy>, ShellError> {
        let read = blocking(|| {
            let Some(path) = wows_toolkit_viewmodel::logging::newest_log() else {
                return Ok(None);
            };
            let contents =
                std::fs::read_to_string(&path).map_err(|source| ShellError::ReadLog { path: path.clone(), source })?;
            Ok(Some((path, contents)))
        })
        .await
        .map_err(ShellError::Task)??;
        let Some((path, contents)) = read else {
            return Ok(None);
        };

        let copied = LogCopy {
            // The newest log is a file in the log directory, so it has a name.
            file: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            // Saturates: no log file reaches four terabytes.
            kilobytes: u32::try_from(contents.len() / 1024).unwrap_or(u32::MAX),
        };
        nsh_core::prelude::set_clipboard(contents);

        Ok(Some(copied))
    }

    /// Shows the toolkit's storage directory in the desktop's file manager,
    /// creating it first: it does not exist until something is written there,
    /// and a folder that will not open reads as a broken button.
    pub async fn open_data_directory() -> Result<(), ShellError> {
        let dir = wows_toolkit_config::storage_dir().ok_or(ShellError::NoStorageDirectory)?;
        blocking(move || {
            std::fs::create_dir_all(&dir)
                .map_err(|source| ShellError::CreateDirectory { path: dir.clone(), source })?;
            open_in_file_manager(&dir)
        })
        .await
        .map_err(ShellError::Task)?
    }

    /// Asks for one directory. `None` when the reader cancelled.
    pub async fn pick_directory(title: String) -> Result<Option<String>, ShellError> {
        let picked = nsh_core::prelude::pick_paths(OpenDialog::folders().title(title))
            .await
            .map_err(|error| ShellError::Dialog { reason: error.to_string() })?;

        Ok(picked.and_then(|mut paths| paths.pop()).map(|path| path.display().to_string()))
    }

    /// Asks the release feed for a build newer than this one. `None` when
    /// this is the newest.
    pub async fn check_for_update() -> Result<Option<ReleaseOffer>, UpdateError> {
        background(crate::shell::update::check_published()).await.map_err(UpdateError::Task)?
    }
}

/// The crash the last run left, once it has been read this session.
static RECENT_CRASH: Mutex<Option<String>> = Mutex::new(None);

/// The log file put on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct LogCopy {
    pub file: String,
    pub kilobytes: u32,
}

/// Shows `dir` in the desktop's file manager and waits for the opener to hand
/// it off.
fn open_in_file_manager(dir: &Path) -> Result<(), ShellError> {
    let failed = |source| ShellError::OpenDirectory { path: PathBuf::from(dir), source };
    let status = std::process::Command::new("xdg-open").arg(dir).status().map_err(failed)?;
    if status.success() {
        return Ok(());
    }

    Err(ShellError::OpenerFailed { path: PathBuf::from(dir), status })
}

/// Draws the calling window in the palette named `name`, and in its appearance.
fn draw_in(name: &str) -> Result<(), SettingsError> {
    let choice = parse_theme(name)?;
    nsh_core::prelude::set_tokens(theme::tokens(choice));
    nsh_core::prelude::set_color_scheme(theme::appearance(choice));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_filled_from_the_arguments() {
        Shell::set_locale("en".to_owned()).expect("English is supported");
        let args = BTreeMap::from([("file".to_owned(), "a.wowsreplay".to_owned())]);
        assert_eq!(Shell::translate("ui.app.drop_to_load".to_owned(), Some(args)), "Drop to load a.wowsreplay");
    }

    #[test]
    fn an_unsupported_language_is_refused() {
        assert!(Shell::set_locale("xx".to_owned()).is_err());
    }
}
