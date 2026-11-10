//! Showing a file in the system file manager.

use std::path::Path;
use std::process::Command;

/// Opens the system file manager at `path`: selected on Windows and macOS,
/// its directory elsewhere. `prepare` decorates the command before it
/// spawns.
pub fn reveal(path: &Path, prepare: impl FnOnce(&mut Command)) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer");
        command.arg(format!("/select,{}", path.display()));
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg("-R").arg(path);
        command
    };
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    };

    prepare(&mut command);
    command.spawn()?;

    Ok(())
}
