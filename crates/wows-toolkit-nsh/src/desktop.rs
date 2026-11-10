//! The main window: an nsh view in a GTK window this binary owns, so the
//! shell can scale its text (the UI zoom) and remember the window's geometry
//! in the row every front end shares.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use nsh_platform_linux as platform;
use platform::gtk4 as gtk;
use gtk::gio::prelude::ActionMapExt as _;
use gtk::gio::prelude::ApplicationExt as _;
use gtk::gio::prelude::ApplicationExtManual as _;
use gtk::prelude::EventControllerExt as _;
use gtk::prelude::GtkApplicationExt as _;
use gtk::prelude::GtkWindowExt as _;
use gtk::prelude::ObjectExt as _;
use gtk::prelude::WidgetExt as _;
use gtk::glib;
use wows_toolkit_nsh::shell::DEFAULT_ZOOM;
use wows_toolkit_nsh::shell::window::MainWindowGeometry;

/// The host call the shell makes when the zoom setting changes.
const ZOOM_CALL: &str = "shell.zoom";

/// The size the window opens at when nothing is remembered, in points.
const DEFAULT_SIZE: (i32, i32) = (1400, 1000);

/// How long the window's size has to hold before it is written. GTK reports
/// every step of a drag; the other front ends write geometry on a one second
/// tick for the same reason.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(1);

thread_local! {
    /// The view, for the zoom handler, which hears the app on the engine
    /// thread and applies the scale on the GTK one.
    static VIEW: RefCell<Option<Rc<platform::AppView>>> = const { RefCell::new(None) };
}

/// Runs `bundle` in the main window until it closes.
pub fn run(bundle: platform::Bundle, options: platform::AppOptions) -> glib::ExitCode {
    prefer_gl_on_nvidia();
    glib::set_prgname(Some(&options.app_id));
    let app = gtk::Application::builder()
        .application_id(&options.app_id)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let bundle = RefCell::new(Some(bundle));
    app.connect_activate(move |app| {
        let Some(bundle) = bundle.borrow_mut().take() else {
            return;
        };
        open(app, bundle, options.clone());
    });

    // GTK must not parse this process's own arguments.
    app.run_with_args::<&str>(&[])
}

fn open(app: &gtk::Application, bundle: platform::Bundle, options: platform::AppOptions) {
    let remembered = wows_toolkit_nsh::shell::window::remembered();
    let size = remembered
        .and_then(|remembered| remembered.inner_size_points)
        .map(|[width, height]| (width.round() as i32, height.round() as i32))
        .filter(|(width, height)| *width > 0 && *height > 0)
        .unwrap_or(DEFAULT_SIZE);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(&options.title)
        .default_width(size.0)
        .default_height(size.1)
        .build();
    if remembered.is_some_and(|remembered| remembered.maximized) {
        window.maximize();
    }
    if remembered.is_some_and(|remembered| remembered.fullscreen) {
        window.fullscreen();
    }

    let view = platform::AppView::new(options);
    window.set_child(Some(&view.widget()));
    // The saved zoom applies before the first frame, so the window does not
    // open at one size and jump to another once the settings are read.
    view.set_font_scale(Some(font_scale(saved_zoom())));
    handle_zoom(&view);
    view.load(platform::App::Js(bundle));
    VIEW.with_borrow_mut(|held| *held = Some(Rc::clone(&view)));

    remember_geometry(&window);
    install_shortcuts(app, &window);

    let keep = RefCell::new(Some(view));
    window.connect_destroy(move |_| {
        VIEW.with_borrow_mut(|held| *held = None);
        if let Some(view) = keep.borrow_mut().take() {
            view.stop();
        }
    });
    window.present();
}

/// The zoom factor the shared settings hold, or the default every front end
/// uses when none was saved or the database cannot be read yet.
fn saved_zoom() -> f32 {
    wows_toolkit_config::load_startup_setting::<f32>(wows_toolkit_viewmodel::settings::keys::ZOOM_FACTOR)
        .ok()
        .flatten()
        .unwrap_or(DEFAULT_ZOOM)
}

/// The text scale a zoom factor draws at. The zoom factor is the one the other
/// front ends store, whose default is 1.15; this view's sizes are designed at
/// that default, so it is the 1.0 of this scale.
fn font_scale(zoom: f32) -> f32 {
    zoom / DEFAULT_ZOOM
}

fn handle_zoom(view: &platform::AppView) {
    view.host().handle(ZOOM_CALL, |call| {
        let Some(zoom) = call.data().get("zoom").and_then(serde_json::Value::as_f64) else {
            call.reject("shell.zoom takes { zoom: number }");
            return;
        };
        let scale = font_scale(zoom as f32);
        glib::MainContext::default().invoke(move || {
            VIEW.with_borrow(|view| {
                if let Some(view) = view {
                    view.set_font_scale(Some(scale));
                }
            });
        });
        call.resolve(serde_json::Value::Null);
    });
}

/// Writes the window's geometry once it has settled after a change, and once
/// more as it closes, so a resize just before quitting is kept.
fn remember_geometry(window: &gtk::ApplicationWindow) {
    let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::default();
    let schedule = {
        let pending = Rc::clone(&pending);
        move |window: &gtk::ApplicationWindow| {
            if let Some(source) = pending.take() {
                source.remove();
            }
            let weak = window.downgrade();
            let fired = Rc::clone(&pending);
            pending.set(Some(glib::timeout_add_local_once(SETTLE, move || {
                fired.set(None);
                if let Some(window) = weak.upgrade() {
                    store(&window);
                }
            })));
        }
    };
    for property in ["default-width", "default-height", "maximized", "fullscreened"] {
        let schedule = schedule.clone();
        window.connect_notify_local(Some(property), move |window, _| schedule(window));
    }
    window.connect_close_request(move |window| {
        if let Some(source) = pending.take() {
            source.remove();
        }
        store(window);
        glib::Propagation::Proceed
    });
}

fn store(window: &gtk::ApplicationWindow) {
    let (width, height) = window.default_size();
    let geometry = MainWindowGeometry {
        size: [width as f32, height as f32],
        maximized: window.is_maximized(),
        fullscreen: window.is_fullscreen(),
    };
    if let Err(err) = wows_toolkit_nsh::shell::window::store(geometry) {
        tracing::error!("window geometry: {}", wows_toolkit_nsh::shell::chain(&err));
    }
}

/// GNOME's window shortcuts, as nsh's own runner installs them: Ctrl+Q quits,
/// Ctrl+W closes the window, F11 toggles full screen.
fn install_shortcuts(app: &gtk::Application, window: &gtk::ApplicationWindow) {
    let quit = gtk::gio::SimpleAction::new("quit", None);
    let weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            for window in app.windows() {
                window.close();
            }
        }
    });
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<Control>q"]);

    let close = gtk::ShortcutController::new();
    close.set_propagation_phase(gtk::PropagationPhase::Bubble);
    close.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Control>w"),
        Some(gtk::NamedAction::new("window.close")),
    ));
    window.add_controller(close);

    let fullscreen = gtk::gio::SimpleAction::new("fullscreen", None);
    let weak = window.downgrade();
    fullscreen.connect_activate(move |_, _| {
        if let Some(window) = weak.upgrade() {
            window.set_fullscreened(!window.is_fullscreen());
        }
    });
    window.add_action(&fullscreen);
    app.set_accels_for_action("win.fullscreen", &["F11"]);
}

/// Draws with GSK's GL renderer under NVIDIA's driver unless `GSK_RENDERER`
/// chooses, which is what nsh's runner does: its Vulkan frames cost the main
/// thread several times as much there.
fn prefer_gl_on_nvidia() {
    if std::env::var_os("GSK_RENDERER").is_some() || !std::path::Path::new("/proc/driver/nvidia/version").exists() {
        return;
    }
    // SAFETY: called before GTK starts, while this thread is the only one
    // reading the environment.
    unsafe { std::env::set_var("GSK_RENDERER", "ngl") };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_default_zoom_draws_at_the_designed_size() {
        assert!((font_scale(DEFAULT_ZOOM) - 1.0).abs() < f32::EPSILON);
        assert!(font_scale(2.0) > 1.0 && font_scale(0.5) < 1.0);
    }
}
