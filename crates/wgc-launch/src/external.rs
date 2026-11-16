//! A game another launcher started, such as Steam's `-applaunch`. It
//! carries no session marker, so its processes are found by what they run:
//! the stub or the client from inside the install.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::session::ProcessId;

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const CLIENT_EXE: &str = "WorldOfWarships64.exe";
const STUB_EXE: &str = "WorldOfWarships.exe";

/// The game processes running from one install outside a toolkit session.
/// Queries scan `/proc` and fail with `ErrorKind::Unsupported` on other
/// platforms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalGame {
    install: Arc<Path>,
}

impl ExternalGame {
    /// Watches the install at `install`, which should be canonical: process
    /// paths are compared to it as text.
    pub fn new(install: &Path) -> Self {
        Self { install: Arc::from(install) }
    }

    pub fn install(&self) -> &Path {
        &self.install
    }

    /// The stub and client processes running from the install.
    pub fn pids(&self) -> std::io::Result<Vec<ProcessId>> {
        scan::install_processes(&self.install)
    }

    pub fn is_running(&self) -> std::io::Result<bool> {
        Ok(!self.pids()?.is_empty())
    }

    /// Blocks until no game process of the install remains.
    pub fn wait_exit(&self) -> std::io::Result<()> {
        while self.is_running()? {
            std::thread::sleep(POLL_INTERVAL);
        }

        Ok(())
    }
}

/// Whether a process with this argv[0] and environment runs the game from
/// `install`. Wine shows a Windows path in argv[0]; Proton also exports
/// `STEAM_COMPAT_INSTALL_PATH` to every process of the game. A toolkit
/// session's processes carry its marker and belong to that session instead.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn runs_from(argv0: &[u8], environ: &[u8], install: &Path) -> bool {
    let name = argv0.rsplit(|byte| *byte == b'/' || *byte == b'\\').next().unwrap_or_default();
    let is_game = [CLIENT_EXE, STUB_EXE].iter().any(|exe| name.eq_ignore_ascii_case(exe.as_bytes()));
    if !is_game {
        return false;
    }

    let mut compat_install = None;
    for var in environ.split(|byte| *byte == 0) {
        if var.starts_with(crate::session::SESSION_ENV.as_bytes())
            && var.get(crate::session::SESSION_ENV.len()) == Some(&b'=')
        {
            return false;
        }
        if let Some(value) = var.strip_prefix(b"STEAM_COMPAT_INSTALL_PATH=") {
            compat_install = Some(value);
        }
    }

    let install_bytes = install.as_os_str().as_encoded_bytes();
    if compat_install.is_some_and(|value| value.strip_suffix(b"/").unwrap_or(value) == install_bytes) {
        return true;
    }

    host_path(argv0).is_some_and(|path| path.starts_with(install))
}

/// The host path of a Wine `Z:` path, or argv[0] itself when it is a host path.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn host_path(argv0: &[u8]) -> Option<PathBuf> {
    let unix: Vec<u8> = match argv0 {
        [drive, b':', rest @ ..] if drive.eq_ignore_ascii_case(&b'z') => {
            rest.iter().map(|byte| if *byte == b'\\' { b'/' } else { *byte }).collect()
        }
        [b'/', ..] => argv0.to_vec(),
        _ => return None,
    };

    String::from_utf8(unix).ok().map(PathBuf::from)
}

#[cfg(target_os = "linux")]
mod scan {
    use std::os::unix::fs::MetadataExt;

    use super::*;

    pub(super) fn install_processes(install: &Path) -> std::io::Result<Vec<ProcessId>> {
        let uid = rustix::process::getuid().as_raw();

        let mut found = Vec::new();
        for entry in std::fs::read_dir("/proc")?.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else {
                continue;
            };
            if !entry.metadata().is_ok_and(|metadata| metadata.uid() == uid) {
                continue;
            }

            // Processes exit between listing and reading; skip them.
            let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            let argv0 = cmdline.split(|byte| *byte == 0).next().unwrap_or_default();
            let Ok(environ) = std::fs::read(entry.path().join("environ")) else {
                continue;
            };
            if super::runs_from(argv0, &environ, install) {
                found.push(ProcessId(pid));
            }
        }

        Ok(found)
    }
}

/// Process tracking reads `/proc`.
#[cfg(not(target_os = "linux"))]
mod scan {
    use super::*;

    pub(super) fn install_processes(_install: &Path) -> std::io::Result<Vec<ProcessId>> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALL: &str = "/home/u/.local/share/Steam/steamapps/common/World of Warships";

    #[test]
    fn steam_game_processes_are_found_by_path_or_compat_install() {
        let install = Path::new(INSTALL);
        let cases: [(&[u8], &[u8], bool); 6] = [
            (
                br"Z:\home\u\.local\share\Steam\steamapps\common\World of Warships\bin\13533378\bin64\WorldOfWarships64.exe",
                b"HOME=/home/u\0",
                true,
            ),
            (
                br"C:\windows\system32\WorldOfWarships.exe",
                b"STEAM_COMPAT_INSTALL_PATH=/home/u/.local/share/Steam/steamapps/common/World of Warships\0",
                true,
            ),
            (br"Z:\games\World_of_Warships\WorldOfWarships.exe", b"HOME=/home/u\0", false),
            (
                br"Z:\home\u\.local\share\Steam\steamapps\common\World of Warships\WorldOfWarships.exe",
                b"WOWS_TOOLKIT_GAME_SESSION=wows\0",
                false,
            ),
            (
                br"Z:\home\u\.local\share\Steam\steamapps\common\World of Warships\WGCheck\WGCheck.exe",
                b"HOME=/home/u\0",
                false,
            ),
            (b"/usr/bin/wineserver", b"STEAM_COMPAT_INSTALL_PATH=/home/u/.local/share/Steam\0", false),
        ];

        for (argv0, environ, expected) in cases {
            assert_eq!(runs_from(argv0, environ, install), expected, "{}", String::from_utf8_lossy(argv0));
        }
    }
}
