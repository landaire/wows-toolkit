//! The zgui desktop front end of WoWs Toolkit.
//!
//! # Layout
//!
//! - `services/`: what the whole process shares, reached through [`services::Services`]: the
//!   tokio runtime, the settings snapshot and its writer, the game data, the running jobs and the
//!   interface language. No view code.
//! - `shell/`: the main window's chrome, reached through [`shell::Shell`]: the tab strip, the
//!   status bar, the settings drawer, the command palette, dialogs, messages and file drops.
//! - `tabs/`: one module per tab, dispatched by [`tabs::TabBody`].
//! - `windows/`: windows other than the main one.
//! - `frames`: frames rasterised on the CPU, pushed through a [`frames::FrameSink`] from any thread
//!   and shown by a [`frames::FrameView`].
//! - `preview/`: the minimap preview a replay row plays when hovered, [`preview::HoverPreview`].
//!
//! # Contexts
//!
//! [`shell::AppRoot`] provides [`services::Services`] and [`shell::Shell`] as local contexts.
//! Both are `Copy` handles over signals: read them once in a component's body
//! (`Services::current()`, `Shell::current()`) and move them into closures.
//!
//! - `Services`: `settings`, `game_data`, `jobs`, `locale`, and `on_tokio`.
//! - `Shell`: tab selection and directory stats tabs, `request_replay`/`take_replay_requests`,
//!   `dialogs`, `toasts()`, `copy_text`, `settings_drawer`, `palette`, `set_theme`, `set_zoom`,
//!   `set_locale`, `debug_mode`.
//!
//! # Tabs
//!
//! A tab is a [`shell::tabs::TabKey`]: an [`shell::tabs::AppTab`] or an open directory stats tab.
//! [`tabs::TabBody`] maps the key to the tab's component. The shell builds it the first time the
//! tab is selected and keeps it built, hidden, after that, so a tab keeps its state.
//!
//! Each tab's content finds its own [`shell::tabs::TabChrome`] with `TabChrome::current()`.
//! `set_toggles` puts its side panel toggles in the status bar and `set_mode` names the view a
//! held modifier key switches to. The status bar shows the selected tab's.
//!
//! A tab offers its commands on the palette as it is built: `Shell::palette.offer` for a root
//! command and `offer_mode` for a list the root leads into, with the builders in
//! [`shell::palette::modes`]. An offer lasts as long as the scope that made it. A command meant
//! for a tab that may not be built yet is a `PaletteRun::Request`, which the tab takes with
//! `palette.take_requests`.
//!
//! # Background work
//!
//! Work runs off the UI thread, and its result comes back in a task spawned with `spawn_local`
//! from the component that asked:
//!
//! - network and database work: [`services::Services::on_tokio`], on the shared tokio runtime.
//!   Dropping the returned future aborts the work.
//! - CPU-bound work (parsing, loading game data, walking directories): zgui's `blocking`, or
//!   `background` for a future.
//! - progress: `Services::jobs.start(Job)` puts a job on the status bar until the returned
//!   `JobHandle` is dropped; `JobHandle::report` updates its name and progress. Hold the handle
//!   in the task doing the work.
//!
//! A task is cancelled with the component that spawned it. Re-check what was asked for after the
//! await, since the reader may have asked for something else in the meantime.
//!
//! # Settings
//!
//! `Services::settings.read(|s| ..)` reads the loaded snapshot reactively; `None` until the
//! startup read lands. `settings.edit(key, |s| { s.field = v; v })` changes the snapshot and
//! saves the returned value under `key`; `settings.save(key, &v)` writes a row the snapshot does
//! not hold. Writes run in the background, in order per key.
//!
//! # Windows
//!
//! [`windows::open`] opens a secondary window themed like the main one, with `Services` provided
//! to its content. It has no `Shell`.
//!
//! # Translations
//!
//! Strings come from the shared catalog through `t!`, under the keys the GPUI front end uses. A
//! string read in a reactive hole calls [`i18n::Locale::track`] first so it follows a change of
//! language.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![expect(dead_code, reason = "API used by the tab, palette and settings drawer ports")]
// The path deliberately holds no locale files: `TranslationsBackend` supplies them, parsed at
// startup. Pointed at the real catalogs, `i18n!()` needs a stack frame larger than a thread gets by
// default.
rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

mod cli;
mod desktop;
mod frames;
mod game_art;
mod http;
mod i18n;
mod icons;
#[cfg(test)]
mod interaction_tests;
mod preview;
mod services;
mod shell;
mod tabs;
mod theme;
mod window_shell;
mod windows;
mod zoom;

use std::sync::Arc;

use wows_toolkit_replay_data::heap_profile;
use zgui::view;

use crate::shell::AppRootProps;

/// What the desktop entry and the Wayland compositor know the application as.
const APPLICATION_ID: &str = "io.github.landaire.WoWsToolkit";

/// The smallest the main window may be dragged to.
const MIN_WINDOW_SIZE: (f32, f32) = (640.0, 480.0);

fn main() {
    // First, so the totals cover the whole process. Inert without `dhat-heap`.
    heap_profile::start();
    heap_profile::mark("main.begin");

    // Before anything is opened: an argument this build does not take is reported and the process
    // exits. The update's cleanup step runs first: the binary it replaced is only deletable once
    // the process that spawned this one has exited, and it has.
    let asked = match cli::parse() {
        cli::Invocation::FinalizeUpdate(replaced) => {
            wows_toolkit_services::update::finalize(&replaced);
            cli::Cli::default()
        }
        cli::Invocation::ListGpus(asked) => {
            let title = format!("{} display adapters", wows_toolkit_config::APP_NAME);
            cli::report_startup_message(&title, &cli::adapter_report(&asked), false);
            std::process::exit(0);
        }
        cli::Invocation::Run(asked) => *asked,
    };

    // `ring` and `aws_lc_rs` are both enabled in the dependency graph, so rustls cannot pick a
    // provider by crate features and `ClientConfig::builder()` panics unless one is installed.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // The log file is what a bug report is copied from, so it is started before anything that could
    // fail, with the setting read straight from the database. A database that cannot be read reads
    // the same as one that has never been written, and both mean logging.
    let to_file =
        wows_toolkit_config::load_startup_setting::<bool>(wows_toolkit_viewmodel::settings::keys::ENABLE_LOGGING)
            .unwrap_or(None)
            .unwrap_or(true);
    let _log_guard = wows_toolkit_services::logging::init(to_file, "wows_toolkit_zgui");
    wows_toolkit_services::logging::install_panic_hook();

    // Before any window exists, which is the only moment these are worth applying.
    let report = wows_toolkit_hardening::apply_startup_mitigations(wows_toolkit_hardening::HardeningRequest {
        hardening: asked.hardening(),
        code_integrity: wows_toolkit_hardening::CodeIntegrityPreference::from_startup_read(
            wows_toolkit_config::load_startup_setting(wows_toolkit_viewmodel::settings::keys::CODE_INTEGRITY),
        ),
    });
    for (mitigation, outcome) in report.entries() {
        tracing::info!("hardening: {mitigation} {outcome}");
    }

    let runtime = match wows_toolkit_services::runtime::build() {
        Ok(runtime) => Some(Arc::new(runtime)),
        Err(err) => {
            tracing::error!("failed to start the tokio runtime: {err}");
            None
        }
    };

    // Read before the window opens: a position can only be asked for when it is made.
    let geometry = window_shell::MainWindowGeometry::remembered();
    let title = format!("{} v{}", wows_toolkit_config::APP_NAME, env!("CARGO_PKG_VERSION"));
    let app = zgui::app()
        .with_title(title.clone())
        .with_application_id(APPLICATION_ID)
        .with_min_size(MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)
        .with_zoom(zoom::to_window(wows_toolkit_viewmodel::settings::ZoomFactor::DEFAULT))
        .with_adapter_preference(asked.adapter_preference())
        // Secondary windows hold handles the main window owns, so none outlives it.
        .with_exit_policy(zgui::prelude::ExitPolicy::WhenPrimaryCloses);
    let app = geometry.apply(app);

    heap_profile::mark("app.run");
    let ran = app.run(move || {
        let runtime = runtime.clone();
        view! { AppRoot(runtime = runtime) }
    });
    if let Err(err) = ran {
        tracing::error!("the window could not be run: {err}");
        cli::report_startup_message(&title, &format!("{err}\n"), true);
    }
}

/// Every `t!` key this crate names has to exist in the catalog: a missing one does not fail to
/// compile, it silently draws the key itself.
#[cfg(test)]
mod translation_keys {
    use std::path::Path;
    use std::path::PathBuf;

    /// The literal keys of every `t!` call under `src`. A key is dotted and has no spaces, which is
    /// how a match inside a comment is told from a real one.
    fn keys_in_source(root: &Path) -> Vec<String> {
        let mut keys = Vec::new();
        let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).expect("the source directory is readable").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                    continue;
                }

                let source = std::fs::read_to_string(&path).expect("a source file is readable");
                let bytes = source.as_bytes();
                let mut from = 0;
                while let Some(at) = source[from..].find("t!(\"") {
                    let at = from + at;
                    from = at + 4;
                    // `format!("` ends in the same three characters.
                    let preceded_by_name = at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
                    if preceded_by_name {
                        continue;
                    }
                    let Some(end) = source[from..].find('"') else { break };
                    let key = &source[from..from + end];
                    from += end;
                    if key.contains('.') && !key.contains(' ') {
                        keys.push(key.to_owned());
                    }
                }
            }
        }

        keys
    }

    #[test]
    fn every_key_the_port_names_is_in_the_catalog() {
        let mut keys = keys_in_source(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
        assert!(!keys.is_empty(), "the scan found no keys, so it is not reading the source");

        keys.sort();
        keys.dedup();
        for key in keys {
            let rendered = rust_i18n::t!(&key).into_owned();
            assert_ne!(rendered, key, "no catalog entry for {key}");
            assert!(!rendered.trim().is_empty(), "empty catalog entry for {key}");
        }
    }
}
