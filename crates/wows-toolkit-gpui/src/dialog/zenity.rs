//! zenity, for a desktop with no file chooser portal.

use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;

use super::Asked;
use super::Request;

/// Why zenity gave no answer.
#[derive(Debug, thiserror::Error)]
pub enum ZenityError {
    #[error("could not start zenity: {source}")]
    Spawn {
        #[source]
        source: io::Error,
    },

    #[error("zenity ended with {status}: {stderr}")]
    Failed { status: ExitStatus, stderr: String },

    #[error("zenity's thread ended without an answer")]
    NoAnswer,
}

pub(super) async fn ask(request: &Request) -> Result<Option<PathBuf>, ZenityError> {
    let mut command = command(request);
    let (sender, receiver) = futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("zenity".to_owned())
        .spawn(move || {
            let _ = sender.send(command.output());
        })
        .map_err(|source| ZenityError::Spawn { source })?;

    let output = receiver.await.map_err(|_| ZenityError::NoAnswer)?.map_err(|source| ZenityError::Spawn { source })?;

    // zenity exits with 1 when the dialog is cancelled or closed.
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Ok(None),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(ZenityError::Failed { status: output.status, stderr });
        }
    }

    let mut picked = output.stdout;
    if picked.last() == Some(&b'\n') {
        picked.pop();
    }

    Ok(Some(PathBuf::from(OsString::from_vec(picked))))
}

fn command(request: &Request) -> Command {
    let mut command = Command::new("zenity");
    command.args(["--file-selection", "--no-markup"]);
    if let Some(title) = &request.title {
        command.arg(format!("--title={title}"));
    }
    if let Some(filter) = request.filter {
        let globs: Vec<String> = filter.extensions.iter().map(|extension| format!("*.{extension}")).collect();
        command.arg(format!("--file-filter={} | {}", filter.label, globs.join(" ")));
    }

    // A trailing separator makes zenity open the directory rather than select
    // it.
    let start = request.start.as_ref().map(|start| start.join(""));
    let filename = match &request.asked {
        Asked::File => start,
        Asked::Folder => {
            command.arg("--directory");
            start
        }
        Asked::Save { file_name } => {
            // zenity 3 overwrites silently without this; zenity 4 always asks.
            command.args(["--save", "--confirm-overwrite"]);
            // Without a start directory zenity reads a bare name against its
            // own working directory, which is the app's.
            Some(start.unwrap_or_default().join(file_name))
        }
    };
    if let Some(filename) = filename {
        command.arg("--filename").arg(filename);
    }

    command
}
