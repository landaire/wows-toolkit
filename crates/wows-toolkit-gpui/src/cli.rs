//! What this process was asked to do on the command line.
//!
//! `finalize-update --replaced <path>` is what an update spawns to delete the
//! executable it replaced, and the bare-path form is the same thing as every
//! released version emits it: those versions cannot be changed, so the form is
//! accepted for as long as one of them can still update into this binary.
//!
//! The egui app also takes flags for the renderer ladder (`--cpu-renderer`,
//! `--gpu-adapter`, `--gpu-safe-mode`, `--list-gpus`) and for the process
//! mitigations (`--no-hardening`). Each names something this port does not have:
//! it renders through gpui's own backend with no adapter selection and applies no
//! mitigations. A flag that parsed and then did nothing would be worse than one
//! that is refused by name, so this takes none of them yet.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;

#[derive(Debug, Default, Parser)]
#[command(name = "wows-toolkit-gpui", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Delete the binary this process replaced during an update.
    FinalizeUpdate {
        #[arg(long)]
        replaced: PathBuf,
    },
}

/// What this process was asked to do.
#[derive(Debug)]
pub enum Invocation {
    /// Delete the named binary, then carry on and open the window.
    FinalizeUpdate(PathBuf),
    /// Open the window.
    Run,
}

/// Subcommand names a bare path must not be mistaken for.
const SUBCOMMANDS: &[&str] = &["finalize-update", "help"];

/// The argument form every released version emits: the app, and the path of the
/// binary it replaced.
///
/// Recognised before clap, because a bare filesystem path is ambiguous with a
/// subcommand name and resolving that inside clap's grammar would hide the
/// contract. Syntactic only: `update::finalize` decides whether the path may
/// actually be deleted.
fn legacy_finalize_target(args: &[OsString]) -> Option<PathBuf> {
    let [_program, single] = args else { return None };
    let text = single.to_str()?;
    if text.starts_with('-') || SUBCOMMANDS.contains(&text) {
        return None;
    }
    Some(PathBuf::from(single))
}

/// Reads the process arguments.
///
/// Prints and exits on `--help`, `--version`, and on an argument this build does
/// not take. The message goes through [`report_startup_message`] rather than
/// clap's own printing, because a release build has no console of its own: clap
/// would write into a handle nothing reads and the process would exit looking
/// like it had crashed.
pub fn parse() -> Invocation {
    let args: Vec<OsString> = std::env::args_os().collect();
    if let Some(replaced) = legacy_finalize_target(&args) {
        return Invocation::FinalizeUpdate(replaced);
    }

    match Cli::try_parse_from(&args) {
        Ok(cli) => match cli.command {
            Some(Command::FinalizeUpdate { replaced }) => Invocation::FinalizeUpdate(replaced),
            None => Invocation::Run,
        },
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
    /// swallowed: the user is told the renderer is not here yet rather than left
    /// believing the launch honoured it.
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

    /// The cleanup step is taken both ways: as this build spawns it, and as every
    /// released version spawns it, which is a bare path.
    #[test]
    fn the_finalize_step_is_taken_in_both_forms() {
        let named = Cli::try_parse_from(["wows-toolkit-gpui", "finalize-update", "--replaced", "old.exe"])
            .expect("the subcommand parses");
        assert!(matches!(named.command, Some(Command::FinalizeUpdate { .. })));

        let legacy = legacy_finalize_target(&["app".into(), "C:/x/wows_toolkit.exe.old".into()]);
        assert_eq!(legacy, Some(PathBuf::from("C:/x/wows_toolkit.exe.old")));
    }

    /// A subcommand name and a flag are not paths, whatever they look like.
    #[test]
    fn a_subcommand_or_a_flag_is_not_a_legacy_path() {
        assert_eq!(legacy_finalize_target(&["app".into(), "finalize-update".into()]), None);
        assert_eq!(legacy_finalize_target(&["app".into(), "--help".into()]), None);
        assert_eq!(legacy_finalize_target(&["app".into()]), None, "a bare launch names nothing");
    }
}
