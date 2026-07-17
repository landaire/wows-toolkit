mod app;
mod settings;
mod theme;

use app::App;
use gpui::*;
use gpui_component::Root;
use gpui_component_assets::Assets;
use settings::GpuiSettings;

const DEFAULT_WINDOW_ORIGIN: Point<Pixels> = point(px(200.), px(120.));
const DEFAULT_WINDOW_SIZE: Size<Pixels> = size(px(1200.), px(800.));

/// Map the persisted main-window geometry to `WindowBounds`, falling back to
/// the hardcoded default whenever a field (or the row itself) is absent.
fn window_bounds_from_settings(saved: Option<wows_toolkit_config::WindowSettings>) -> WindowBounds {
    let default_bounds = Bounds { origin: DEFAULT_WINDOW_ORIGIN, size: DEFAULT_WINDOW_SIZE };
    let Some(saved) = saved else {
        return WindowBounds::Windowed(default_bounds);
    };

    let size = saved.inner_size_points.map(|[w, h]| size(px(w), px(h))).unwrap_or(DEFAULT_WINDOW_SIZE);
    let origin = saved.outer_position_pixels.map(|[x, y]| point(px(x), px(y))).unwrap_or(DEFAULT_WINDOW_ORIGIN);
    let bounds = Bounds { origin, size };

    if saved.fullscreen {
        WindowBounds::Fullscreen(bounds)
    } else if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

fn main() {
    let app = gpui_platform::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_component::init(cx);
        gpui_tokio::init(cx);

        // Read before the window opens: position can only be set at builder
        // time, not via a later viewport/window command.
        let window_bounds = window_bounds_from_settings(wows_toolkit_config::load_main_window_settings());

        let window_options = WindowOptions {
            window_bounds: Some(window_bounds),
            window_min_size: Some(size(px(640.), px(480.))),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            let mut app_entity = None;
            let window = cx
                .open_window(window_options, |window, cx| {
                    theme::apply_egui_dark_theme(settings::DEFAULT_ZOOM, window, cx);
                    let view = cx.new(|cx| App::new(window, cx));
                    app_entity = Some(view.clone());
                    cx.new(|cx| Root::new(view, window, cx))
                })
                .expect("failed to open window");
            let app_entity = app_entity.expect("App entity created inside open_window's build_root_view");

            let loaded = gpui_tokio::Tokio::spawn(cx, async move {
                let pool = wows_toolkit_config::open_db().await?;
                Ok::<GpuiSettings, anyhow::Error>(GpuiSettings::load(&pool).await)
            })
            .await;

            let Ok(Ok(loaded)) = loaded else {
                // No DB yet, or the load failed: keep the hardcoded default
                // zoom/theme already applied above and leave the Settings tab
                // showing its own defaults.
                return;
            };

            let zoom = loaded.zoom;
            let _ = window.update(cx, |_root, window, cx| {
                theme::apply_egui_dark_theme(zoom, window, cx);
            });
            app_entity.update(cx, |app, cx| {
                app.apply_settings(loaded);
                cx.notify();
            });
        })
        .detach();
    });
}
