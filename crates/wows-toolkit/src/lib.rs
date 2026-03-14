#![warn(clippy::all, rust_2018_idioms)]
#![allow(clippy::blocks_in_conditions)]

rust_i18n::i18n!("../wt-translations/translations", fallback = "en", backend = FileBackend::try_load());

mod app;
mod armor_viewer;
pub mod collab;
pub(crate) mod data;
pub(crate) mod db;
#[cfg(feature = "mod_manager")]
mod mod_manager;
pub(crate) mod replay;
mod tab_state;
mod task;
mod twitch;
mod ui;
pub(crate) mod util;
pub mod viewport_3d;
pub use app::WowsToolkitApp;
pub use db::load_main_window_settings;
pub const APP_NAME: &str = "WoWs Toolkit";
pub(crate) use egui_phosphor::regular as icons;

/// Force the `i18n!()` lazy static to initialize. The generated initializer
/// requires a large stack frame in debug builds, so call this from a thread
/// with an explicit (larger) stack size.
pub fn init_i18n() {
    // Any `t!()` call triggers the lazy init.
    let _ = rust_i18n::t!("meta.language_name");
}

/// [`TextResolver`] implementation that uses `t!()` from rust-i18n.
pub(crate) struct LocalizedTextResolver;

impl wt_translations::TextResolver for LocalizedTextResolver {
    fn resolve(&self, text: &wt_translations::TranslatableText) -> String {
        let key = text.key();
        rust_i18n::t!(key).into()
    }
}

/// Backend that loads translations from TOML files on disk next to the
/// executable. Checked first by `t!()`; missing keys fall through to the
/// compiled-in translations. Data is loaded once at first `t!()` call.
pub(crate) struct FileBackend {
    translations: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
}

impl FileBackend {
    /// Attempt to load `translations/*.toml` from the exe directory.
    /// Returns an empty backend if the directory doesn't exist.
    pub fn try_load() -> Self {
        let translations_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("translations")));

        let mut all = std::collections::HashMap::new();
        if let Some(dir) = translations_dir {
            load_toml_dir(&dir, &mut all);
        }
        Self { translations: all }
    }
}

fn load_toml_dir(
    dir: &std::path::Path,
    out: &mut std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml")
            && let Some(locale) = path.file_stem().and_then(|s| s.to_str())
            && let Some(flat) = load_locale_file(&path)
        {
            out.insert(locale.to_string(), flat);
        }
    }
}

/// Load and flatten a single locale TOML file. Kept as a separate non-inlined
/// function so the large `toml::Table` intermediate lives in its own stack frame
/// (reused across files) rather than accumulating in the caller's frame.
#[inline(never)]
fn load_locale_file(path: &std::path::Path) -> Option<std::collections::HashMap<String, String>> {
    let content = std::fs::read_to_string(path).ok()?;
    let table: Box<toml::Table> = Box::new(content.parse().ok()?);
    let mut flat = std::collections::HashMap::new();
    flatten_toml("", &table, &mut flat);
    Some(flat)
}

fn flatten_toml(prefix: &str, table: &toml::Table, out: &mut std::collections::HashMap<String, String>) {
    for (k, v) in table {
        let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match v {
            toml::Value::String(s) => {
                out.insert(key, s.clone());
            }
            toml::Value::Table(t) => {
                flatten_toml(&key, t, out);
            }
            _ => {}
        }
    }
}

/// App data directory, matching eframe's `storage_dir()` layout so existing
/// data is found after removing the `persistence` feature.
///
/// - Windows: `%APPDATA%\APP_NAME\data`
/// - macOS:   `~/Library/Application Support/APP_NAME`
/// - Linux:   `$XDG_DATA_HOME/app_name` or `~/.local/share/app_name`
pub fn storage_dir() -> Option<std::path::PathBuf> {
    use egui::os::OperatingSystem as OS;
    use std::path::PathBuf;
    match OS::from_target_os() {
        OS::Nix => std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home::home_dir().map(|p| p.join(".local").join("share")))
            .map(|p| p.join(APP_NAME.to_lowercase().replace(|c: char| c.is_ascii_whitespace(), ""))),
        OS::Mac => home::home_dir().map(|p| {
            p.join("Library").join("Application Support").join(APP_NAME.replace(|c: char| c.is_ascii_whitespace(), "-"))
        }),
        OS::Windows => {
            // %APPDATA% = roaming appdata, same as eframe's FOLDERID_RoamingAppData
            std::env::var_os("APPDATA").map(PathBuf::from).map(|p| p.join(APP_NAME).join("data"))
        }
        _ => None,
    }
}

impl rust_i18n::Backend for FileBackend {
    fn available_locales(&self) -> Vec<&str> {
        self.translations.keys().map(|s| s.as_str()).collect()
    }

    fn translate(&self, locale: &str, key: &str) -> Option<&str> {
        self.translations.get(locale).and_then(|m| m.get(key).map(|s| s.as_str()))
    }
}
