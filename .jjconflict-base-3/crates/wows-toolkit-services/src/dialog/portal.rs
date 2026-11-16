//! The desktop's file chooser, asked over D-Bus through the XDG desktop portal.

use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use ashpd::desktop::ResponseError;
use ashpd::desktop::file_chooser::FileFilter;
use ashpd::desktop::file_chooser::SelectedFiles;
use ashpd::zbus;
use ashpd::zbus::fdo;

use super::Asked;
use super::Filter;
use super::Request;

/// Why the portal gave no answer.
#[derive(Debug, thiserror::Error)]
pub enum PortalError {
    #[error("the session D-Bus could not be reached ({source})")]
    NoBus {
        #[source]
        source: ashpd::Error,
    },

    #[error("no desktop portal offers a file chooser ({source})")]
    NoFileChooser {
        #[source]
        source: ashpd::Error,
    },

    #[error("the desktop portal failed ({source})")]
    Request {
        #[source]
        source: ashpd::Error,
    },

    #[error("the desktop portal answered without a file")]
    NoFile,

    #[error("the desktop portal answered with {uri}, which is not a local file")]
    NotLocal { uri: String },
}

impl PortalError {
    /// Whether there is no file chooser portal to ask, as opposed to one that
    /// was asked and failed.
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::NoBus { .. } | Self::NoFileChooser { .. })
    }
}

impl From<ashpd::Error> for PortalError {
    fn from(source: ashpd::Error) -> Self {
        if let ashpd::Error::PortalNotFound(_) = source {
            return Self::NoFileChooser { source };
        }
        // ashpd hands a failed call back wrapped, and a failed connection bare.
        let bus = match &source {
            ashpd::Error::Zbus(bus) | ashpd::Error::Portal(ashpd::PortalError::ZBus(bus)) => bus,
            _ => return Self::Request { source },
        };

        if no_bus(bus) {
            Self::NoBus { source }
        } else if no_file_chooser(bus) {
            Self::NoFileChooser { source }
        } else {
            Self::Request { source }
        }
    }
}

/// Whether `bus` says there is no session bus, or that it went away under a
/// connection made before.
fn no_bus(bus: &zbus::Error) -> bool {
    match bus {
        zbus::Error::Address(_) | zbus::Error::Connection(..) => true,
        zbus::Error::InputOutput(io) => matches!(
            io.kind(),
            ErrorKind::BrokenPipe
                | ErrorKind::NotConnected
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::UnexpectedEof
        ),
        _ => false,
    }
}

/// Whether `bus` is the bus refusing a call to a name or interface nobody
/// offers, which is what a desktop without a file chooser portal answers.
fn no_file_chooser(bus: &zbus::Error) -> bool {
    const MISSING: [&str; 4] = [
        "org.freedesktop.DBus.Error.ServiceUnknown",
        "org.freedesktop.DBus.Error.NameHasNoOwner",
        "org.freedesktop.DBus.Error.UnknownInterface",
        "org.freedesktop.DBus.Error.UnknownMethod",
    ];

    match bus {
        zbus::Error::MethodError(name, ..) => MISSING.contains(&name.as_str()),
        zbus::Error::FDO(standard) => matches!(
            **standard,
            fdo::Error::ServiceUnknown(_)
                | fdo::Error::NameHasNoOwner(_)
                | fdo::Error::UnknownInterface(_)
                | fdo::Error::UnknownMethod(_)
        ),
        _ => false,
    }
}

pub(super) async fn ask(request: &Request) -> Result<Option<PathBuf>, PortalError> {
    let title = request.title.as_deref();
    let filters = request.filter.map(file_filter);
    let start = request.start.as_deref();

    let sent = match &request.asked {
        Asked::File | Asked::Folder => {
            SelectedFiles::open_file()
                .title(title)
                .modal(true)
                .multiple(false)
                .directory(request.asked == Asked::Folder)
                .filters(filters)
                .current_folder::<&Path>(start)?
                .send()
                .await?
        }
        Asked::Save { file_name } => {
            SelectedFiles::save_file()
                .title(title)
                .modal(true)
                .current_name(file_name.as_str())
                .filters(filters)
                .current_folder::<&Path>(start)?
                .send()
                .await?
        }
    };
    let selected = match sent.response() {
        Ok(selected) => selected,
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let Some(uri) = selected.uris().first() else {
        return Err(PortalError::NoFile);
    };
    let not_local = || PortalError::NotLocal { uri: uri.as_str().to_owned() };
    let url = url::Url::parse(uri.as_str()).map_err(|_| not_local())?;
    let path = url.to_file_path().map_err(|()| not_local())?;

    Ok(Some(path))
}

fn file_filter(filter: Filter) -> FileFilter {
    filter
        .extensions
        .iter()
        .fold(FileFilter::new(filter.label), |accepted, extension| accepted.glob(&format!("*.{extension}")))
}
