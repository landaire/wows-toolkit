//! A window's position on the screen, which GTK 4 neither reports nor sets.
//! On X11 the window's X id is asked for its place directly, over a
//! connection of the app's own; on Wayland only the compositor knows it, so
//! nothing is read or set there.

use std::time::Duration;
use std::time::Instant;

use gdk4_x11::X11Surface;
use gtk::glib;
use gtk::prelude::Cast as _;
use gtk::prelude::DisplayExt as _;
use gtk::prelude::NativeExt as _;
use gtk::prelude::SurfaceExt as _;
use gtk::prelude::WidgetExt as _;
use nsh_platform_linux::gtk4 as gtk;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::AtomEnum;
use x11rb::protocol::xproto::ChangeWindowAttributesAux;
use x11rb::protocol::xproto::ConfigureWindowAux;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xproto::EventMask;
use x11rb::protocol::xproto::Window;
use x11rb::rust_connection::RustConnection;

use crate::desktop::error::PositionError;
use crate::shell::window::WindowPosition;

/// The root window's size in device pixels.
#[derive(Debug, Clone, Copy)]
struct ScreenSize {
    width: i32,
    height: i32,
}

/// How far the window manager's frame reaches past the window on the left
/// and above it, in device pixels.
#[derive(Debug, Clone, Copy)]
struct FrameExtents {
    left: u32,
    top: u32,
}

/// An X11 window, with a connection to its display and the screen's root.
struct XWindow {
    connection: RustConnection,
    window: Window,
    root: Window,
    screen: ScreenSize,
}

impl XWindow {
    /// `window`'s X window, `None` when it is not on an X11 display or has
    /// no surface yet.
    fn of(window: &gtk::ApplicationWindow) -> Option<Result<Self, PositionError>> {
        let surface = window.surface()?.downcast::<X11Surface>().ok()?;

        Some(Self::connect(&surface))
    }

    fn connect(surface: &X11Surface) -> Result<Self, PositionError> {
        let display = surface.display().name().to_string();
        let xid = surface.xid();
        let window = Window::try_from(xid).map_err(|_| PositionError::WindowId { xid })?;
        let (connection, screen) =
            x11rb::connect(Some(&display)).map_err(|source| PositionError::Connect { display, source })?;
        let Some(root) = connection.setup().roots.get(screen) else {
            return Err(PositionError::NoScreen { screen });
        };
        let size = ScreenSize { width: i32::from(root.width_in_pixels), height: i32::from(root.height_in_pixels) };
        let root = root.root;

        Ok(Self { connection, window, root, screen: size })
    }

    /// Where the window's frame sits: the window's own place, less the
    /// decorations the window manager drew around it.
    fn outer(&self) -> Result<WindowPosition, PositionError> {
        let inner = self
            .connection
            .translate_coordinates(self.window, self.root, 0, 0)
            .map_err(PositionError::Send)?
            .reply()
            .map_err(PositionError::Reply)?;
        let frame = self.frame_extents()?;

        Ok(WindowPosition {
            x: f32::from(inner.dst_x) - frame.left as f32,
            y: f32::from(inner.dst_y) - frame.top as f32,
        })
    }

    /// The window manager's frame, from `_NET_FRAME_EXTENTS`; none when the
    /// window manager draws none or does not name it.
    fn frame_extents(&self) -> Result<FrameExtents, PositionError> {
        const FRAME_EXTENTS: &[u8] = b"_NET_FRAME_EXTENTS";
        const NONE: FrameExtents = FrameExtents { left: 0, top: 0 };

        let atom = self
            .connection
            .intern_atom(true, FRAME_EXTENTS)
            .map_err(PositionError::Send)?
            .reply()
            .map_err(PositionError::Reply)?
            .atom;
        if atom == x11rb::NONE {
            return Ok(NONE);
        }

        let reply = self
            .connection
            .get_property(false, self.window, atom, AtomEnum::CARDINAL, 0, 4)
            .map_err(PositionError::Send)?
            .reply()
            .map_err(PositionError::Reply)?;
        // Left, right, top, bottom.
        let Some(mut extents) = reply.value32() else {
            return Ok(NONE);
        };
        let (Some(left), Some(_right), Some(top)) = (extents.next(), extents.next(), extents.next()) else {
            return Ok(NONE);
        };

        Ok(FrameExtents { left, top })
    }

    /// Has this connection told when the window is mapped.
    fn watch_mapping(&self) -> Result<(), PositionError> {
        let watched = ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY);
        self.connection.change_window_attributes(self.window, &watched).map_err(PositionError::Send)?;
        self.connection.flush().map_err(PositionError::Send)
    }

    /// Whether the window has been mapped since [`Self::watch_mapping`],
    /// without waiting for it.
    fn mapped(&self) -> Result<bool, PositionError> {
        while let Some(event) = self.connection.poll_for_event().map_err(PositionError::Send)? {
            if let Event::MapNotify(mapped) = event
                && mapped.window == self.window
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Asks the window manager to put the window's frame at `position`. The
    /// window's gravity is GTK's default, north west, under which the place a
    /// client asks for is its frame's.
    fn move_to(&self, position: WindowPosition) -> Result<(), PositionError> {
        let x = position.x.round() as i32;
        let y = position.y.round() as i32;
        self.connection
            .configure_window(self.window, &ConfigureWindowAux::new().x(x).y(y))
            .map_err(PositionError::Send)?;

        self.connection.flush().map_err(PositionError::Send)
    }

    /// Whether a frame placed at `position` would show its corner on the
    /// screen, which a position saved on a monitor since removed would not.
    fn shows(&self, position: WindowPosition) -> bool {
        // Enough of the frame to take hold of.
        const MARGIN: i32 = 48;

        let x = position.x.round() as i32;
        let y = position.y.round() as i32;

        (-MARGIN..self.screen.width - MARGIN).contains(&x) && (-MARGIN..self.screen.height - MARGIN).contains(&y)
    }
}

/// Where `window`'s frame sits on the screen, `None` where that cannot be
/// read: on Wayland, before the window is shown, or when the X display fails.
pub fn outer(window: &gtk::ApplicationWindow) -> Option<WindowPosition> {
    let read = XWindow::of(window)?.and_then(|x_window| x_window.outer());

    match read {
        Ok(position) => Some(position),
        Err(err) => {
            tracing::warn!("window position: {}", crate::shell::chain(&err));
            None
        }
    }
}

/// Puts `window`'s frame at `position` once the window manager has mapped
/// it, on X11 only. A move asked for earlier is overruled by the window
/// manager's own placement as it maps the window.
pub fn restore(window: &gtk::ApplicationWindow, position: WindowPosition) {
    // The map is watched without blocking the main loop: glib 0.22 offers no
    // file descriptor source, so the connection is polled.
    const POLL: Duration = Duration::from_millis(25);
    const GIVE_UP: Duration = Duration::from_secs(5);

    window.connect_realize(move |window| {
        let Some(found) = XWindow::of(window) else {
            return;
        };
        let x_window = match found.and_then(|x_window| x_window.watch_mapping().map(|()| x_window)) {
            Ok(x_window) => x_window,
            Err(err) => {
                tracing::warn!("window position: {}", crate::shell::chain(&err));
                return;
            }
        };

        let started = Instant::now();
        glib::timeout_add_local(POLL, move || {
            let placed = match x_window.mapped() {
                Ok(false) if started.elapsed() < GIVE_UP => return glib::ControlFlow::Continue,
                Ok(false) => {
                    tracing::info!("window position: the window was not mapped in {GIVE_UP:?}, left where it is");
                    return glib::ControlFlow::Break;
                }
                Ok(true) if !x_window.shows(position) => {
                    tracing::info!("window position: {position:?} is off the screen, left to the window manager");
                    return glib::ControlFlow::Break;
                }
                Ok(true) => x_window.move_to(position),
                Err(err) => Err(err),
            };
            if let Err(err) = placed {
                tracing::warn!("window position: {}", crate::shell::chain(&err));
            }

            glib::ControlFlow::Break
        });
    });
}
