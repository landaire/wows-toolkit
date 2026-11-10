//! The app's GTK windows: the main one, and the tactics board and popped-out
//! replay renderers, each an nsh view running the same bundle in a window of
//! its own. This binary owns them so the shell can scale them (the UI zoom)
//! and remember each kind's geometry in the row every front end shares.

mod error;
mod position;
mod zoom;

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use gtk::gdk;
use gtk::gio::prelude::ActionMapExt as _;
use gtk::gio::prelude::ApplicationExt as _;
use gtk::gio::prelude::ApplicationExtManual as _;
use gtk::gio::prelude::FileExt as _;
use gtk::glib;
use gtk::prelude::Cast as _;
use gtk::prelude::EventControllerExt as _;
use gtk::prelude::GtkApplicationExt as _;
use gtk::prelude::GtkWindowExt as _;
use gtk::prelude::ObjectExt as _;
use gtk::prelude::StaticType as _;
use gtk::prelude::WidgetExt as _;
use nsh_platform_linux as platform;
use platform::gtk4 as gtk;
use wows_toolkit_config::WindowKind;

use crate::desktop::zoom::ZoomBin;
use crate::renderer::ReplayRenderer;
use crate::shell::DEFAULT_ZOOM;
use crate::shell::ZoomFactor;
use crate::shell::window::WINDOW_PARAM;
use crate::shell::window::WindowGeometry;
use crate::shell::window::WindowPosition;
use crate::shell::window::WindowRequest;
use crate::shell::window::WindowSize;

/// What every window is made from, kept on the GTK thread.
struct Desktop {
    app: gtk::Application,
    bundle: platform::Bundle,
    options: platform::AppOptions,
    /// Every open window's zoom, which applies to all of them.
    zooms: RefCell<Vec<glib::WeakRef<ZoomBin>>>,
    /// The zoom last applied, which a window opened later starts at.
    zoom: Cell<ZoomFactor>,
    /// The open tactics board's window: asked for again, it comes forward
    /// rather than a second board opening on the one shared board.
    tactics: RefCell<Option<glib::WeakRef<gtk::ApplicationWindow>>>,
    /// Set as the main window closes: a window asked for while the others
    /// close would be missed by that sweep and keep the app running.
    closing: Cell<bool>,
}

thread_local! {
    static DESKTOP: RefCell<Option<Rc<Desktop>>> = const { RefCell::new(None) };
}

fn running() -> Option<Rc<Desktop>> {
    DESKTOP.with_borrow(Option::clone)
}

/// Runs `bundle` in the main window until every window closes.
pub fn run(bundle: platform::Bundle, options: platform::AppOptions) -> glib::ExitCode {
    prefer_gl_on_nvidia();
    glib::set_prgname(Some(&options.app_id));
    let app = gtk::Application::builder()
        .application_id(&options.app_id)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let start = RefCell::new(Some((bundle, options)));
    app.connect_activate(move |app| {
        let Some((bundle, options)) = start.borrow_mut().take() else {
            return;
        };
        let desktop = Rc::new(Desktop {
            app: app.clone(),
            bundle,
            options,
            zooms: RefCell::default(),
            zoom: Cell::new(saved_zoom()),
            tactics: RefCell::default(),
            closing: Cell::new(false),
        });
        DESKTOP.with_borrow_mut(|held| *held = Some(Rc::clone(&desktop)));
        install_app_shortcuts(app);
        open_main(&desktop);
    });

    // GTK must not parse this process's own arguments.
    app.run_with_args::<&str>(&[])
}

fn open_main(desktop: &Desktop) {
    // What the other front ends use.
    const DEFAULT_SIZE: WindowSize = WindowSize { width: 1200.0, height: 800.0 };
    const MIN_SIZE: WindowSize = WindowSize { width: 640.0, height: 480.0 };

    let window = new_window(desktop, &desktop.options.title, WindowKind::Main, DEFAULT_SIZE);
    window.set_size_request(MIN_SIZE.width.round() as i32, MIN_SIZE.height.round() as i32);
    let view = new_view(desktop.options.clone());
    handle_open_window(&view);
    report_hovered_files(&view);
    // Closing the main window closes the app: the board and renderers are
    // beside it, not on their own.
    window.connect_close_request(|main| {
        if let Some(desktop) = running() {
            desktop.closing.set(true);
        }
        if let Some(app) = main.application() {
            for window in app.windows().into_iter().filter(|window| window != main.upcast_ref::<gtk::Window>()) {
                window.close();
            }
        }
        glib::Propagation::Proceed
    });
    show(&window, view, desktop, WindowKind::Main);
}

/// Opens the window `request` asks for, or brings the open board forward.
fn open_secondary(desktop: &Desktop, request: WindowRequest) {
    // What the other front ends use. Below the minimum, the renderer's
    // transport wraps into itself.
    const DEFAULT_SIZE: WindowSize = WindowSize { width: 900.0, height: 700.0 };
    const MIN_SIZE: WindowSize = WindowSize { width: 480.0, height: 420.0 };

    if request == WindowRequest::TacticsBoard
        && let Some(open) = desktop.tactics.borrow().as_ref().and_then(glib::WeakRef::upgrade)
    {
        open.present();
        return;
    }

    let kind = request.kind();
    let window = new_window(desktop, &request.title(), kind, DEFAULT_SIZE);
    window.set_size_request(MIN_SIZE.width.round() as i32, MIN_SIZE.height.round() as i32);

    // A channel of its own, so the main window's handlers and events stay
    // with the main window.
    let view = new_view(platform::AppOptions { host: Default::default(), ..desktop.options.clone() });
    let mut params = serde_json::Map::new();
    params.insert(
        WINDOW_PARAM.to_owned(),
        // Its paths were read from the app's JSON, so they are text.
        serde_json::to_value(&request).expect("a request read from JSON writes back to JSON"),
    );
    // Read by the app as it starts, which `show` does.
    view.host().set_params(params);
    release_before_closing(&window, &view);
    match request {
        WindowRequest::TacticsBoard => *desktop.tactics.borrow_mut() = Some(window.downgrade()),
        // The window owns the viewport handed to it, including when it closes
        // before its app took the viewport up.
        WindowRequest::ReplayRenderer { renderer, .. } => {
            window.connect_destroy(move |_| ReplayRenderer::close(renderer));
        }
    }
    show(&window, view, desktop, kind);
}

/// A window placed where windows of `kind` were last left, `default` in size
/// the first time.
fn new_window(desktop: &Desktop, title: &str, kind: WindowKind, default: WindowSize) -> gtk::ApplicationWindow {
    let remembered = crate::shell::window::remembered(kind);
    let size = remembered
        .and_then(|remembered| remembered.inner_size_points)
        .map(|[width, height]| WindowSize { width, height })
        .filter(|size| size.width.round() >= 1.0 && size.height.round() >= 1.0)
        .unwrap_or(default);

    let window = gtk::ApplicationWindow::builder()
        .application(&desktop.app)
        .title(title)
        .default_width(size.width.round() as i32)
        .default_height(size.height.round() as i32)
        .build();
    if remembered.is_some_and(|remembered| remembered.maximized) {
        window.maximize();
    }
    if remembered.is_some_and(|remembered| remembered.fullscreen) {
        window.fullscreen();
    }
    if let Some(remembered) = remembered.filter(|remembered| !remembered.maximized && !remembered.fullscreen)
        && let Some([x, y]) = remembered.outer_position_pixels
    {
        position::restore(&window, WindowPosition { x, y });
    }
    window
}

/// A view for a new window.
fn new_view(options: platform::AppOptions) -> Rc<platform::AppView> {
    let view = platform::AppView::new(options);
    // The zoom scales text with everything else, and stands in for the
    // system's text size as it does in the other front ends.
    view.set_font_scale(Some(1.0));
    handle_zoom(&view);
    view
}

fn show(window: &gtk::ApplicationWindow, view: Rc<platform::AppView>, desktop: &Desktop, kind: WindowKind) {
    // Zoomed before the first frame, so the window does not open at one size
    // and jump to another once the settings are read.
    let zoomed = ZoomBin::new(&view.widget(), desktop.zoom.get());
    desktop.zooms.borrow_mut().push(zoomed.downgrade());
    window.set_child(Some(&zoomed));
    view.load(platform::App::Js(desktop.bundle.clone()));
    remember_geometry(window, kind);
    install_window_shortcuts(window);

    let keep = RefCell::new(Some(view));
    window.connect_destroy(move |_| {
        if let Some(view) = keep.borrow_mut().take() {
            view.stop();
        }
        if let Some(desktop) = running() {
            desktop.zooms.borrow_mut().retain(|zoom| zoom.upgrade().is_some());
        }
    });
    window.present();
}

/// Holds a closing window open until its app has let go of what it opened:
/// a stopped engine runs none of its views' cleanups, and a renderer or board
/// left open would go on drawing for nobody.
fn release_before_closing(window: &gtk::ApplicationWindow, view: &platform::AppView) {
    // The app hears the event as the window is asked to close, and posts back
    // once it has let go. One that stopped answering cannot keep it open.
    const CLOSING_EVENT: &str = "window.closing";
    const RELEASED_POST: &str = "window.released";
    const RELEASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

    let host = view.host();
    let released = Arc::new(AtomicBool::new(false));
    let asked = Rc::new(Cell::new(false));

    let weak = glib::SendWeakRef::from(window.downgrade());
    let answered = Arc::clone(&released);
    host.handle(RELEASED_POST, move |_call| {
        let weak = weak.clone();
        let answered = Arc::clone(&answered);
        glib::MainContext::default().invoke(move || {
            answered.store(true, Ordering::SeqCst);
            if let Some(window) = weak.upgrade() {
                window.close();
            }
        });
    });

    // Connected before the geometry handler, which this one holds back until
    // the app has answered.
    window.connect_close_request(move |window| {
        if released.load(Ordering::SeqCst) {
            return glib::Propagation::Proceed;
        }
        if !asked.replace(true) {
            if let Err(err) = host.send(CLOSING_EVENT, serde_json::Value::Null) {
                tracing::warn!("window close: the app could not be told: {err}");
            }
            let weak = window.downgrade();
            let released = Arc::clone(&released);
            glib::timeout_add_local_once(RELEASE_TIMEOUT, move || {
                released.store(true, Ordering::SeqCst);
                if let Some(window) = weak.upgrade() {
                    window.close();
                }
            });
        }
        glib::Propagation::Stop
    });
}

/// The zoom factor the shared settings hold, or the default every front end
/// uses when none was saved or the database cannot be read yet.
fn saved_zoom() -> ZoomFactor {
    wows_toolkit_config::load_startup_setting::<f32>(wows_toolkit_viewmodel::settings::keys::ZOOM_FACTOR)
        .ok()
        .flatten()
        .map(ZoomFactor::new)
        .unwrap_or(DEFAULT_ZOOM)
}

/// Applies a zoom the app sets to every open window.
fn handle_zoom(view: &platform::AppView) {
    const ZOOM_CALL: &str = "shell.zoom";

    view.host().handle(ZOOM_CALL, |call| {
        let Some(zoom) = call.data().get("zoom").and_then(serde_json::Value::as_f64) else {
            call.reject("shell.zoom takes { zoom: number }");
            return;
        };
        let zoom = ZoomFactor::new(zoom as f32);
        glib::MainContext::default().invoke(move || {
            let Some(desktop) = running() else {
                return;
            };
            desktop.zoom.set(zoom);
            for zoomed in desktop.zooms.borrow().iter().filter_map(glib::WeakRef::upgrade) {
                zoomed.set_zoom(zoom);
            }
        });
        call.resolve(serde_json::Value::Null);
    });
}

/// Tells the app which files a drag over the window carries before they land,
/// which nsh's `onFileDragEnter` does not: a motion controller observes the
/// drag without taking it, so nsh's own drop target still receives the drop.
/// The event carries `{ uris }`, the `file:` URIs `onFileDrop` would, and
/// `null` once the drag leaves.
fn report_hovered_files(view: &platform::AppView) {
    const HOVERED_EVENT: &str = "shell.hoveredFiles";

    let host = view.host();
    let motion = gtk::DropControllerMotion::new();
    let entered = host.clone();
    motion.connect_enter(move |motion, _, _| {
        let Some(drop) = motion.drop() else {
            return;
        };
        if !drop.formats().contains_type(gdk::FileList::static_type()) {
            return;
        }

        let watching = motion.downgrade();
        let host = entered.clone();
        glib::spawn_future_local(async move {
            let read = drop.read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT).await;
            // The drag may have left, or another begun, while the list was read.
            if watching.upgrade().and_then(|motion| motion.drop()).as_ref() != Some(&drop) {
                return;
            }

            let list = match read.map(|value| value.get::<gdk::FileList>()) {
                Ok(Ok(list)) => list,
                Ok(Err(err)) => {
                    tracing::debug!("hovered files: not a file list: {err}");
                    return;
                }
                Err(err) => {
                    tracing::debug!("hovered files: unreadable: {err}");
                    return;
                }
            };
            // The same URIs nsh's drop names: local files only.
            let uris: Vec<String> = list
                .files()
                .into_iter()
                .filter(|file| file.path().is_some_and(|path| path.is_absolute()))
                .map(|file| file.uri().to_string())
                .collect();
            if let Err(err) = host.send(HOVERED_EVENT, serde_json::json!({ "uris": uris })) {
                tracing::warn!("hovered files: the app could not be told: {err}");
            }
        });
    });
    motion.connect_leave(move |_| {
        if let Err(err) = host.send(HOVERED_EVENT, serde_json::Value::Null) {
            tracing::warn!("hovered files: the app could not be told: {err}");
        }
    });
    view.widget().add_controller(motion);
}

/// Opens the windows the main window's app asks for.
fn handle_open_window(view: &platform::AppView) {
    const OPEN_WINDOW_CALL: &str = "shell.openWindow";

    view.host().handle(OPEN_WINDOW_CALL, |call| {
        let request = match serde_json::from_value::<WindowRequest>(call.data().clone()) {
            Ok(request) => request,
            Err(err) => {
                call.reject(format!("shell.openWindow: {err}"));
                return;
            }
        };
        glib::MainContext::default().invoke(move || match running() {
            Some(desktop) if !desktop.closing.get() => {
                open_secondary(&desktop, request);
                call.resolve(serde_json::Value::Null);
            }
            _ => call.reject("shell.openWindow: the app is closing"),
        });
    });
}

/// Writes the window's geometry once it has settled after a change, and once
/// more as it closes, so a resize just before quitting is kept.
fn remember_geometry(window: &gtk::ApplicationWindow, kind: WindowKind) {
    // GTK reports every step of a drag; the other front ends write geometry
    // on a one second tick for the same reason.
    const SETTLE: std::time::Duration = std::time::Duration::from_secs(1);

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
                    store(&window, kind);
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
        store(window, kind);
        glib::Propagation::Proceed
    });
}

fn store(window: &gtk::ApplicationWindow, kind: WindowKind) {
    let (width, height) = window.default_size();
    let maximized = window.is_maximized();
    let fullscreen = window.is_fullscreen();
    // The size kept is the window's normal one, so the position kept is too.
    let position = if maximized || fullscreen { None } else { position::outer(window) };
    let geometry = WindowGeometry {
        size: WindowSize { width: width as f32, height: height as f32 },
        position,
        maximized,
        fullscreen,
    };
    if let Err(err) = crate::shell::window::store(kind, geometry) {
        tracing::error!("window geometry: {}", crate::shell::chain(&err));
    }
}

/// GNOME's application shortcuts, as nsh's own runner installs them: Ctrl+Q
/// closes every window, and F11 toggles full screen in the focused one.
fn install_app_shortcuts(app: &gtk::Application) {
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
    app.set_accels_for_action("win.fullscreen", &["F11"]);
}

/// GNOME's window shortcuts: Ctrl+W closes the window, and the full screen
/// toggle F11 runs.
fn install_window_shortcuts(window: &gtk::ApplicationWindow) {
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
