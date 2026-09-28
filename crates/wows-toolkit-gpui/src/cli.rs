//! What this process was asked to do on the command line.
//!
//! The egui app takes flags for the renderer ladder (`--cpu-renderer`,
//! `--gpu-adapter`, `--gpu-safe-mode`, `--list-gpus`), for the process
//! mitigations (`--no-hardening`) and for the updater's cleanup step
//! (`finalize-update --replaced`, plus the bare-path form every released version
//! emits). Each names something this port does not have: it renders through
//! gpui's own backend with no adapter selection, applies no mitigations, and has
//! no updater to finalize. A flag that parsed and then did nothing would be
//! worse than one that is refused by name, so this takes none of them yet.
//!
//! What it does take is `--help` and `--version`, and it refuses anything else
//! rather than starting the app as though the argument had been honoured.

use clap::Parser;

#[derive(Debug, Default, Parser)]
#[command(name = "wows-toolkit-gpui", version, about)]
pub struct Cli {}

/// Reads the process arguments.
///
/// Prints and exits on `--help`, `--version`, and on an argument this build does
/// not take. The message goes through [`report_startup_message`] rather than
/// clap's own printing, because a release build has no console of its own: clap
/// would write into a handle nothing reads and the process would exit looking
/// like it had crashed.
pub fn parse() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let title = format!("{} v{}", wows_toolkit_config::APP_NAME, env!("CARGO_PKG_VERSION"));
            report_startup_message(&title, &err.render().to_string(), err.use_stderr());
            std::process::exit(err.exit_code());
        }
    }
}

/// A writer onto the console that launched this process, if there was one.
///
/// Opens `CONOUT$` rather than using the process's stdout handle, so it bypasses
/// redirection: `wows-toolkit-gpui --help > out.txt` shows the message in the
/// console and leaves `out.txt` empty. This is what the egui app does
/// (`wows_toolkit::cli::console_writer`), and the single `AttachConsole` call
/// site, so that call happens at most once per process.
#[cfg(windows)]
fn console_writer() -> Option<std::fs::File> {
    use windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS;
    use windows_sys::Win32::System::Console::AttachConsole;

    // SAFETY: AttachConsole has no preconditions beyond a valid process
    // context. It either attaches to the parent's console or fails, and failure
    // shows up as CONOUT$ not opening either.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };

    std::fs::OpenOptions::new().write(true).open("CONOUT$").ok()
}

#[cfg(not(windows))]
fn console_writer() -> Option<std::fs::File> {
    None
}

/// Puts a startup-time message where the launching context can show it.
///
/// A release build has no console, so a double-click or a drag-and-drop launch
/// that fails to parse would otherwise exit silently with nothing visible
/// anywhere: logging only starts once the window is being opened, which a parse
/// error never reaches.
///
/// Own handle first, parent's console second: a process that has a handle may
/// have had it redirected, and `wows-toolkit-gpui --help > out.txt` should fill
/// the file. Only a launch with no handle at all falls through to the parent's
/// console, which cannot be redirected, and then to a message box. (The egui app
/// goes straight to the console, so its redirect leaves the file empty.)
fn report_startup_message(title: &str, message: &str, is_error: bool) {
    use std::io::Write as _;

    let mut out = std::io::stdout();
    if write!(out, "{message}").is_ok() && out.flush().is_ok() {
        return;
    }

    if let Some(mut console) = console_writer() {
        let _ = write!(console, "{message}");
        return;
    }

    #[cfg(windows)]
    show_message_box(title, message, is_error);

    #[cfg(not(windows))]
    {
        let _ = (title, is_error);
        eprint!("{message}");
    }
}

/// The fallback for a launch with no console to write to.
#[cfg(windows)]
fn show_message_box(title: &str, message: &str, is_error: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_OK;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_SETFOREGROUND;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_TOPMOST;
    use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;

    // MessageBoxW wants NUL-terminated UTF-16, not the Rust form.
    let to_wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() };
    let title = to_wide(title);
    let message = to_wide(message);
    let icon = if is_error { MB_ICONERROR } else { MB_ICONINFORMATION };

    // SAFETY: both buffers are NUL-terminated UTF-16 that outlive this
    // synchronous call, and the owner window is null.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | icon | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flag the egui app takes and this build does not is refused, not
    /// swallowed: the user is told the renderer or the updater is not here yet
    /// rather than left believing the launch honoured it.
    #[test]
    fn a_flag_this_build_does_not_have_is_refused() {
        for flag in ["--cpu-renderer", "--gpu-safe-mode", "--list-gpus", "--no-hardening"] {
            let refused = Cli::try_parse_from(["wows-toolkit-gpui", flag]);
            assert!(refused.is_err(), "{flag} is not taken yet, so it has to be refused");
        }
    }

    /// And a bare launch takes no arguments at all.
    #[test]
    fn a_bare_launch_parses() {
        assert!(Cli::try_parse_from(["wows-toolkit-gpui"]).is_ok());
    }
}
