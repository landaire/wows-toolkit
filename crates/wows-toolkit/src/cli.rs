use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
use thiserror::Error;

/// Why a replaced-binary path was rejected. This gates a delete driven by an
/// untrusted argument, so each rejection carries the paths that caused it.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FinalizeError {
    #[error("replaced binary {replaced:?} is not in the same directory as {current:?}")]
    DifferentDirectory { replaced: PathBuf, current: PathBuf },
    #[error("replaced binary {replaced:?} does not have a .old extension")]
    UnexpectedName { replaced: PathBuf },
    #[error("replaced binary {replaced:?} does not exist")]
    Missing { replaced: PathBuf },
}

/// Decide whether `replaced` may be deleted by this process.
///
/// The directory check keeps an argument from naming a file elsewhere on the
/// system. The extension check narrows it further: every version that emits
/// this argument produces `<exe>.old`.
pub fn validate_finalize_target(current_exe: &Path, replaced: &Path) -> Result<(), FinalizeError> {
    if current_exe.parent() != replaced.parent() {
        return Err(FinalizeError::DifferentDirectory {
            replaced: replaced.to_path_buf(),
            current: current_exe.to_path_buf(),
        });
    }

    if replaced.extension() != Some("old".as_ref()) {
        return Err(FinalizeError::UnexpectedName { replaced: replaced.to_path_buf() });
    }

    if !replaced.exists() {
        return Err(FinalizeError::Missing { replaced: replaced.to_path_buf() });
    }

    Ok(())
}

/// Subcommand names that must not be mistaken for a legacy path argument.
const SUBCOMMAND_NAMES: &[&str] = &["finalize-update", "help"];

/// Recognise the argument form used by every released version:
/// `wows_toolkit.exe <replaced.exe.old>`.
///
/// Those versions cannot be changed, so this form is accepted for as long as
/// any of them can still update into this binary. Classification is syntactic
/// only; `validate_finalize_target` decides whether the path may be deleted.
pub fn legacy_finalize_target(args: &[OsString]) -> Option<PathBuf> {
    let [_program, single] = args else {
        return None;
    };

    let text = single.to_str()?;
    if text.starts_with('-') || SUBCOMMAND_NAMES.contains(&text) {
        return None;
    }

    Some(PathBuf::from(single))
}

#[derive(Debug, Parser)]
#[command(name = "wows_toolkit", version, about)]
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
pub enum Invocation {
    FinalizeUpdate { replaced: PathBuf },
    Run(Cli),
}

/// Decide what to do with the raw process arguments.
///
/// The legacy check runs before clap because a bare filesystem path is
/// ambiguous with a subcommand name, and resolving that inside clap's grammar
/// would make the contract harder to see and to test.
pub fn resolve<I>(args: I) -> Result<Invocation, clap::Error>
where
    I: IntoIterator<Item = OsString>,
{
    let args: Vec<OsString> = args.into_iter().collect();

    if let Some(replaced) = legacy_finalize_target(&args) {
        return Ok(Invocation::FinalizeUpdate { replaced });
    }

    let cli = Cli::try_parse_from(&args)?;
    match &cli.command {
        Some(Command::FinalizeUpdate { replaced }) => {
            Ok(Invocation::FinalizeUpdate { replaced: replaced.clone() })
        }
        None => Ok(Invocation::Run(cli)),
    }
}

/// Obtain a writer for the console that launched this process.
///
/// Release builds are a windows subsystem binary with no console of their own,
/// so stdout and stderr go nowhere. `AttachConsole` borrows the parent's
/// console and `CONOUT$` opens it directly, which avoids depending on whether
/// the standard handles were inherited.
#[cfg(windows)]
pub fn console_writer() -> Option<std::fs::File> {
    use windows_sys::Win32::System::Console::AttachConsole;
    use windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS;

    // Fails when the parent has no console, in which case CONOUT$ will not
    // open either and the caller gets None.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };

    std::fs::OpenOptions::new().write(true).open("CONOUT$").ok()
}

#[cfg(not(windows))]
pub fn console_writer() -> Option<std::fs::File> {
    None
}

/// Report a startup-time message (a parse error, `--help`, or `--version`)
/// through whichever channel the launching context can actually show.
///
/// A release build has no console of its own, so a double-click or a
/// drag-and-drop launch that fails to parse would otherwise exit silently
/// with nothing visible anywhere: logging only starts once `eframe::run_native`
/// is reached, which a parse error never gets to. `console_writer` is the
/// single call site for `AttachConsole`, so routing the fallback through here
/// keeps that call to at most once per process.
pub fn report_startup_message(title: &str, message: &str, is_error: bool) {
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

/// Win32 message box fallback for a launch with no console to write to.
#[cfg(windows)]
fn show_message_box(title: &str, message: &str, is_error: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_OK;
    use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;

    // MessageBoxW wants null-terminated UTF-16, not the Rust &str form.
    let to_wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() };
    let title = to_wide(title);
    let message = to_wide(message);
    let icon = if is_error { MB_ICONERROR } else { MB_ICONINFORMATION };

    unsafe {
        MessageBoxW(std::ptr::null_mut(), message.as_ptr(), title.as_ptr(), MB_OK | icon);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsString;
    use std::fs::File;

    use tempfile::tempdir;

    #[test]
    fn accepts_sibling_dot_old_file_that_exists() {
        let dir = tempdir().expect("tempdir");
        let current = dir.path().join("wows_toolkit.exe");
        let replaced = dir.path().join("wows_toolkit.exe.old");
        File::create(&replaced).expect("create replaced");

        assert_eq!(validate_finalize_target(&current, &replaced), Ok(()));
    }

    #[test]
    fn rejects_path_in_another_directory() {
        let dir = tempdir().expect("tempdir");
        let other = tempdir().expect("other tempdir");
        let current = dir.path().join("wows_toolkit.exe");
        let replaced = other.path().join("wows_toolkit.exe.old");
        File::create(&replaced).expect("create replaced");

        assert_eq!(
            validate_finalize_target(&current, &replaced),
            Err(FinalizeError::DifferentDirectory { replaced, current })
        );
    }

    #[test]
    fn rejects_path_without_dot_old_extension() {
        let dir = tempdir().expect("tempdir");
        let current = dir.path().join("wows_toolkit.exe");
        let replaced = dir.path().join("something.exe");
        File::create(&replaced).expect("create replaced");

        assert_eq!(
            validate_finalize_target(&current, &replaced),
            Err(FinalizeError::UnexpectedName { replaced })
        );
    }

    #[test]
    fn rejects_path_that_does_not_exist() {
        let dir = tempdir().expect("tempdir");
        let current = dir.path().join("wows_toolkit.exe");
        let replaced = dir.path().join("wows_toolkit.exe.old");

        assert_eq!(
            validate_finalize_target(&current, &replaced),
            Err(FinalizeError::Missing { replaced })
        );
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn treats_lone_path_argument_as_legacy_form() {
        let parsed = legacy_finalize_target(&args(&["wows_toolkit.exe", "C:\\app\\wows_toolkit.exe.old"]));

        assert_eq!(parsed, Some(PathBuf::from("C:\\app\\wows_toolkit.exe.old")));
    }

    #[test]
    fn bare_launch_is_not_legacy_form() {
        assert_eq!(legacy_finalize_target(&args(&["wows_toolkit.exe"])), None);
    }

    #[test]
    fn flag_is_not_legacy_form() {
        assert_eq!(legacy_finalize_target(&args(&["wows_toolkit.exe", "--help"])), None);
    }

    #[test]
    fn subcommand_name_is_not_legacy_form() {
        assert_eq!(legacy_finalize_target(&args(&["wows_toolkit.exe", "finalize-update"])), None);
    }

    #[test]
    fn multiple_arguments_are_not_legacy_form() {
        assert_eq!(legacy_finalize_target(&args(&["wows_toolkit.exe", "a", "b"])), None);
    }

    #[test]
    fn resolves_legacy_form_to_finalize_update() {
        let resolved = resolve(args(&["wows_toolkit.exe", "C:\\app\\wows_toolkit.exe.old"])).expect("resolve");

        assert!(matches!(
            resolved,
            Invocation::FinalizeUpdate { replaced } if replaced == Path::new("C:\\app\\wows_toolkit.exe.old")
        ));
    }

    #[test]
    fn resolves_subcommand_form_to_finalize_update() {
        let resolved = resolve(args(&[
            "wows_toolkit.exe",
            "finalize-update",
            "--replaced",
            "C:\\app\\wows_toolkit.exe.old",
        ]))
        .expect("resolve");

        assert!(matches!(
            resolved,
            Invocation::FinalizeUpdate { replaced } if replaced == Path::new("C:\\app\\wows_toolkit.exe.old")
        ));
    }

    #[test]
    fn resolves_bare_launch_to_run() {
        let resolved = resolve(args(&["wows_toolkit.exe"])).expect("resolve");

        assert!(matches!(resolved, Invocation::Run(_)));
    }

    #[test]
    fn rejects_unknown_flag() {
        assert!(resolve(args(&["wows_toolkit.exe", "--nonsense"])).is_err());
    }
}
