#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The path deliberately holds no locale files: pointed at the real catalogs,
// `i18n!()` needs a stack frame larger than a thread gets by default (see the
// same note in `wows_toolkit`'s lib.rs). `TranslationsBackend` supplies them,
// parsed at startup.
rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

mod app;
mod armor_viewer;
mod child_process;
mod cli;
mod collab;
mod collab_popover;
mod constants;
mod dialog;
mod first_run;
mod game_data_cache;
mod http;
mod icons;
#[cfg(test)]
mod interaction_tests;
mod logging;
mod minimap_preview;
mod notices;
mod palette;
mod personal_rating;
mod player_tracker;
mod preview_hover;
mod render_defaults;
mod replay_index;
mod replay_inspector;
mod replay_renderer;
mod runtime;
mod search;
mod search_pills;
mod settings;
mod settings_store;
mod stats;
mod theme;
mod toast;
mod twitch;
mod ui;
mod unpacker;
mod update;
mod upload;
mod viewport;
mod window_shell;

use app::App;
use gpui_kit::assets::Assets;
use gpui_kit::component::Root;
use gpui_kit::*;
use settings::GpuiSettings;
use wows_toolkit_viewmodel::settings::ThemeChoice;

const DEFAULT_WINDOW_ORIGIN: Point<Pixels> = point(px(200.), px(120.));
const DEFAULT_WINDOW_SIZE: Size<Pixels> = size(px(1200.), px(800.));

fn main() {
    // Before anything is opened: an argument this build does not take is
    // reported and the process exits, rather than a window appearing as though
    // it had been honoured.
    // The cleanup step first, then the window: the binary this replaced is only
    // deletable once the process that spawned this one has exited, and it has.
    if let cli::Invocation::FinalizeUpdate(replaced) = cli::parse() {
        update::finalize(&replaced);
    }

    // The log file is what a bug report is copied from, so it is started before
    // anything that could fail. The setting is read straight from the database:
    // the app that owns the pool does not exist yet, and a crash here would
    // otherwise go unrecorded. `RUST_LOG` still governs stderr.
    // A database that cannot be read reads the same as one that has never been
    // written here, and both mean logging: a log file the reader did not ask for
    // is a file to delete, where a session that logged nothing is a crash nobody
    // can explain.
    let to_file =
        wows_toolkit_config::load_startup_setting::<bool>(wows_toolkit_viewmodel::settings::keys::ENABLE_LOGGING)
            .unwrap_or(None)
            .unwrap_or(true);
    let _log_guard = logging::init(to_file);
    logging::install_panic_hook();

    // Before any window exists, which is the only moment these are worth
    // applying: extension points attach as a window is created and overlays hook
    // as a swapchain is. Read straight from the database for the same reason the
    // log setting is: the app that owns the pool does not exist yet.
    //
    // The renderer ladder the egui app pins an adapter through has no counterpart
    // here (see `docs/gpui-port-gaps.md`, item 13), so nothing is skipped for the
    // sake of one: the policies are asked for on every launch.
    let report = wows_toolkit_hardening::apply_startup_mitigations(wows_toolkit_hardening::HardeningRequest {
        hardening: wows_toolkit_hardening::Hardening::Apply,
        code_integrity: wows_toolkit_hardening::CodeIntegrityPreference::from_startup_read(
            wows_toolkit_config::load_startup_setting(wows_toolkit_viewmodel::settings::keys::CODE_INTEGRITY),
        ),
    });
    for (mitigation, outcome) in report.entries() {
        tracing::info!("hardening: {mitigation} {outcome}");
    }

    let app = gpui_kit::platform::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_kit::component::init(cx);
        icons::register_font(cx);
        if let Err(err) = runtime::init(cx) {
            tracing::error!("failed to start the tokio runtime: {err}");
        }

        // Read before the window opens: position can only be set at builder
        // time, not via a later viewport/window command.
        let window_bounds = window_shell::bounds_for(
            wows_toolkit_config::load_main_window_settings(),
            Bounds { origin: DEFAULT_WINDOW_ORIGIN, size: DEFAULT_WINDOW_SIZE },
        );

        let window_options = WindowOptions {
            window_bounds: Some(window_bounds),
            window_min_size: Some(size(px(640.), px(480.))),
            // Named and versioned as the egui window is, so two of them side by
            // side are told apart, and a bug report names a build.
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from(format!(
                    "{} v{}",
                    wows_toolkit_config::APP_NAME,
                    env!("CARGO_PKG_VERSION")
                ))),
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            let mut app_entity = None;
            let window = cx
                .open_window(window_options, |window, cx| {
                    // Before the database is read: the desktop's own
                    // preference, which is what the stored default resolves to
                    // anyway.
                    theme::apply_egui_theme(ThemeChoice::default(), settings::DEFAULT_ZOOM, window, cx);
                    window_shell::remember(wows_toolkit_config::WindowKind::Main, window, cx);
                    let view = cx.new(|cx| App::new(window, cx));
                    app_entity = Some(view.clone());
                    cx.new(|cx| Root::new(view, window, cx))
                })
                .inspect_err(|err| tracing::error!("failed to open window: {err}"))
                .expect("failed to open window");
            let app_entity = app_entity.expect("App entity created inside open_window's build_root_view");

            let loaded = runtime::spawn(cx, async move {
                let pool = wows_toolkit_config::open_db().await?;
                let settings = GpuiSettings::load(&pool).await;
                let session = stats::load::SessionData::load(&pool, &settings.proxy_url).await;
                // The pool comes back so settings edits have somewhere to go;
                // the startup read would otherwise close it.
                Ok::<_, anyhow::Error>((settings, session, pool))
            })
            .await;

            let (loaded, session, pool) = match loaded {
                Ok(Ok(loaded)) => loaded,
                Ok(Err(err)) => {
                    // DB open or settings load failed: keep the hardcoded
                    // default zoom/theme already applied above and let the
                    // Settings tab report the failure instead of loading forever.
                    tracing::error!("failed to load settings from the config DB: {err:#}");
                    app_entity.update(cx, |app, cx| {
                        app.mark_settings_failed(err.to_string());
                        cx.notify();
                    });
                    return;
                }
                Err(err) => {
                    tracing::error!("settings-load task did not complete: {err}");
                    app_entity.update(cx, |app, cx| {
                        app.mark_settings_failed(err.to_string());
                        cx.notify();
                    });
                    return;
                }
            };

            // Settings application needs a `Window` (the grouping combo syncs
            // through one), so it runs inside this update rather than beside
            // it. A failure here means no settings reach the app at all --
            // no directory scan, no game-data preload -- so it is logged
            // rather than discarded.
            let zoom = loaded.zoom;
            let theme_choice = loaded.theme;
            let remembered = runtime::spawn(cx, {
                let pool = pool.clone();
                async move { wows_toolkit_config::load_all_window_settings(&pool).await }
            })
            .await;
            let render_defaults = runtime::spawn(cx, {
                let pool = pool.clone();
                async move { render_defaults::load(&pool).await }
            })
            .await;
            if let Err(err) = window.update(cx, |_root, window, cx| {
                settings_store::init(pool.clone(), cx);
                notices::adopt(loaded.suppress_p2p_ip_warning, loaded.suppress_gpu_encoder_warning, cx);
                match render_defaults {
                    Ok(defaults) => render_defaults::adopt(defaults, cx),
                    // Every viewport then opens with what the renderer itself
                    // defaults to, which is what a fresh install does anyway.
                    Err(err) => tracing::error!("the saved renderer defaults could not be read: {err}"),
                }
                match remembered {
                    Ok(remembered) => window_shell::adopt(remembered, cx),
                    // Every window then opens at its default, which is what a
                    // first launch does anyway.
                    Err(err) => tracing::error!("the remembered window geometry could not be read: {err}"),
                }
                // Held for the life of the process: the callback is what writes
                // a resize made in the last second before the app quits.
                window_shell::remember_on_quit(wows_toolkit_config::WindowKind::Main, window.window_handle(), cx)
                    .detach();
                theme::apply_egui_theme(theme_choice, zoom, window, cx);
                // Before anything else the app does with the settings: what may
                // be shared is the reader's to answer, and a battle landing
                // before they have is a battle sent under a default.
                first_run::ask(&app_entity, &loaded, window, cx);
                app_entity.update(cx, |app, cx| {
                    app.report_last_crash(window, cx);
                    if loaded.check_for_updates {
                        app.check_for_update(app::Asked::AtStartup, window, cx);
                    }
                    app.apply_settings(loaded, window, cx);
                    app.apply_session_stats(session, window, cx);
                    app.start_player_tracker(pool, cx);
                    cx.notify();
                });
            }) {
                tracing::error!("settings could not be applied: {err}");
            }
        })
        .detach();
    });
}

/// Every `t!` key this crate names has to exist in the catalog: a missing one
/// does not fail to compile, it silently draws the key itself.
#[cfg(test)]
mod translation_keys {
    use std::path::Path;

    /// Collects the literal keys of every `t!` call under `src`.
    ///
    /// A key is dotted and lower-case, which is how a match inside a doc
    /// comment or a nested macro is told from a real one.
    fn keys_in_source(dir: &Path, keys: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("the source directory is readable").flatten() {
            let path = entry.path();
            if path.is_dir() {
                keys_in_source(&path, keys);
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
                // `format!("` ends in the same three characters, so the macro
                // is only this one when a name does not run into it.
                let preceded_by_name = at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
                if preceded_by_name {
                    continue;
                }
                let Some(end) = source[from..].find('"') else { break };
                let key = &source[from..from + end];
                from += end;
                if key.contains('.') && !key.contains(' ') {
                    keys.push(key.to_string());
                }
            }
        }
    }

    #[test]
    fn every_key_the_port_names_is_in_the_catalog() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut keys = Vec::new();
        keys_in_source(&src, &mut keys);
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
