/// Why a window's position on an X11 screen could not be read or set.
#[derive(Debug, thiserror::Error)]
pub enum PositionError {
    #[error("the X display {display} could not be reached")]
    Connect {
        display: String,
        #[source]
        source: x11rb::errors::ConnectError,
    },
    #[error("the X display reported no screen {screen}")]
    NoScreen { screen: usize },
    #[error("GTK's X window id {xid} is not one the X protocol carries")]
    WindowId { xid: gdk4_x11::XWindow },
    #[error("the X request could not be sent")]
    Send(#[source] x11rb::errors::ConnectionError),
    #[error("the X request failed")]
    Reply(#[source] x11rb::errors::ReplyError),
}
