//! Starting another program without handing it this process's hardening.

/// Prepares `command` so nothing this process did to itself reaches the child.
///
/// The suppressed error mode is inherited outright, so a genuine load failure
/// in the game or in a relaunched updater would fail silently. That is what the
/// shared helper undoes. No renderer environment is passed on: the desktop
/// front ends choose their adapter in process.
pub fn prepare(command: &mut std::process::Command) -> &mut std::process::Command {
    wows_toolkit_hardening::prepare_child(command, &[])
}

/// Opens the system file manager with `path` selected.
///
/// Best effort: a file manager that is not there, or refuses, leaves a log line rather than an
/// error the caller would have to carry.
pub fn reveal_in_file_manager(path: &std::path::Path) {
    // Explorer splits its command line at commas outside quotes and does not read `\\?\` paths,
    // so the path goes in plain and quoted. A Windows path cannot contain a quote.
    #[cfg(target_os = "windows")]
    let mut command = {
        use std::os::windows::process::CommandExt as _;

        let mut command = std::process::Command::new("explorer");
        command.raw_arg(format!("/select,\"{}\"", crate::paths::without_verbatim_prefix(path).display()));
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg("-R").arg(path);
        command
    };
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    };

    if let Err(err) = prepare(&mut command).spawn() {
        tracing::warn!("{} could not be shown in the file manager: {err}", path.display());
    }
}
