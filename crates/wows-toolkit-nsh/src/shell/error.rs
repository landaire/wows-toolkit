use std::path::PathBuf;

/// Why a settings read or write did not complete.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("the settings database could not be opened")]
    Open(#[source] sqlx::Error),
    #[error("setting {key} could not be read")]
    Read {
        key: &'static str,
        #[source]
        source: wows_toolkit_config::queries::SettingError,
    },
    #[error("the armor viewer defaults could not be read")]
    ArmorDefaults(#[source] sqlx::Error),
    #[error("setting {key} could not be written")]
    Write {
        key: &'static str,
        #[source]
        source: sqlx::Error,
    },
    #[error("the value given for {key} is not a {expected}")]
    Value {
        key: &'static str,
        expected: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("{value:?} is not a theme")]
    UnknownTheme { value: String },
    #[error("{value:?} is not a supported language")]
    UnknownLanguage { value: String },
    #[error("{value:?} is not a data sharing mode")]
    UnknownSharingMode { value: String },
    #[error("{value:?} is not an export format")]
    UnknownExportFormat { value: String },
    #[error("the zoom factor {value} is outside {min}..={max}")]
    ZoomOutOfRange { value: f64, min: f32, max: f32 },
    #[error("the settings task did not complete")]
    Task(#[source] tokio::task::JoinError),
}

/// Why an update check did not complete. Each reads with its whole cause
/// chain, which is what the error window shows and a bug report needs.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("{}", chain(source))]
    Settings {
        #[from]
        source: SettingsError,
    },
    #[error("{}", chain(source))]
    Http {
        #[from]
        source: wows_toolkit_viewmodel::http::HttpError,
    },
    #[error("{}", chain(source))]
    Check {
        #[from]
        source: wows_toolkit_viewmodel::update::UpdateCheckError,
    },
    #[error("the update check task did not complete")]
    Task(#[source] tokio::task::JoinError),
}

/// Why a shell service failed.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("there is no storage directory")]
    NoStorageDirectory,
    #[error("{path} could not be created")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} could not be opened in the file manager")]
    OpenDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the file manager did not open {path}: xdg-open exited with {status}")]
    OpenerFailed { path: PathBuf, status: std::process::ExitStatus },
    #[error("no log file was found beside the executable")]
    NoLogFile,
    #[error("{path} could not be read")]
    ReadLog {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the folder dialog could not be shown: {reason}")]
    Dialog { reason: String },
    #[error("the shell task did not complete")]
    Task(#[source] tokio::task::JoinError),
}

/// Why the main window's geometry was not written.
#[derive(Debug, thiserror::Error)]
pub enum WindowError {
    #[error("no runtime could be started to write the window geometry")]
    Runtime(#[source] std::io::Error),
    #[error("the settings database could not be opened")]
    Open(#[source] sqlx::Error),
    #[error("the window geometry could not be written")]
    Write(#[source] sqlx::Error),
}

/// Why a pasted Twitch credential was not stored.
#[derive(Debug, thiserror::Error)]
pub enum PasteError {
    #[error("{0}")]
    Parse(#[from] wows_toolkit_viewmodel::twitch::TokenParseError),
    #[error("{0}")]
    Settings(#[from] SettingsError),
}

/// Why a replay could not be contributed.
#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("{}", chain(source))]
    Settings {
        #[from]
        source: SettingsError,
    },
    #[error("{}", chain(source))]
    Parse {
        #[from]
        source: crate::game_data::GameDataError,
    },
    #[error("the contribution task did not complete")]
    Task(#[source] tokio::task::JoinError),
}

/// `error` and each error under it on one line, joined by `: `, for a message
/// shown in a single line of the interface. A cause that repeats the text
/// above it, as a transparent wrapper's does, is shown once.
pub fn sentence(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut previous = text.clone();
    let mut cause = error.source();
    while let Some(next) = cause {
        let line = next.to_string();
        if line != previous {
            text.push_str(": ");
            text.push_str(&line);
        }
        previous = line;
        cause = next.source();
    }

    text
}

/// `error` and each error under it, one per line.
pub fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        text.push_str("\n  caused by: ");
        text.push_str(&next.to_string());
        cause = next.source();
    }

    text
}
