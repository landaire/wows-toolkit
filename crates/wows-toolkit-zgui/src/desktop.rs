//! Handing a directory or a link to the desktop: its file manager, its browser.
//!
//! zgui has no opener of its own, so this asks the platform's: the shell's `open` verb on
//! Windows, `open` on macOS and `xdg-open` elsewhere, each of which takes a directory or a URL.

use std::ffi::OsStr;
use std::path::Path;

/// Shows `dir` in the desktop's file manager.
pub fn open_directory(dir: &Path) {
    run_opener(dir.as_os_str());
}

/// Opens `url` in the desktop's browser.
pub fn open_url(url: &str) {
    run_opener(OsStr::new(url));
}

/// Shows the toolkit's storage directory in the file manager.
///
/// The directory is created first: it does not exist until something has been written there, and
/// opening a path that is not there yet reads as a broken button rather than an empty folder.
pub fn open_data_directory() {
    let Some(dir) = wows_toolkit_config::storage_dir() else {
        tracing::warn!("there is no storage directory to open");
        return;
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        tracing::warn!("the storage directory could not be created: {err}");
        return;
    }

    open_directory(&dir);
}

/// `explorer.exe` reads commas in its command line as argument separators, which cuts URLs such
/// as wows-numbers' `/player/<id>,<name>` and paths that contain one, so the shell is asked
/// directly. It may block on shell extensions, so it runs off the UI thread, in the
/// single-threaded COM apartment some of those extensions require.
#[cfg(windows)]
fn run_opener(target: &OsStr) {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED;
    use windows_sys::Win32::System::Com::COINIT_DISABLE_OLE1DDE;
    use windows_sys::Win32::System::Com::CoInitializeEx;
    use windows_sys::Win32::System::Com::CoUninitialize;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide: Vec<u16> = target.encode_wide().chain(std::iter::once(0)).collect();
    let shown = target.to_string_lossy().into_owned();
    let spawned = std::thread::Builder::new().name("shell-open".into()).spawn(move || {
        let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();

        // SAFETY: the thread is fresh, so it has no apartment yet; a failure only means the call
        // below runs without one.
        let initialized =
            unsafe { CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32) }
                >= 0;

        // SAFETY: both strings are NUL-terminated UTF-16 that outlive this synchronous call. The
        // owner window, parameters and working directory are null, which ShellExecuteW accepts.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Values above 32 are success; the rest are error codes.
        if result as usize <= 32 {
            tracing::warn!("{shown} could not be opened: shell error {}", result as usize);
        }
        if initialized {
            // SAFETY: balances the successful CoInitializeEx on this thread.
            unsafe { CoUninitialize() };
        }
    });
    if let Err(err) = spawned {
        tracing::warn!("the shell could not be asked to open a target: {err}");
    }
}

#[cfg(not(windows))]
fn run_opener(target: &OsStr) {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";

    let mut command = std::process::Command::new(opener);
    command.arg(target);
    if let Err(err) = wows_toolkit_services::child_process::prepare(&mut command).spawn() {
        tracing::warn!("{} could not be opened: {err}", target.to_string_lossy());
    }
}
