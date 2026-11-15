//! One game session at a time across toolkit processes.

use std::fs::File;
use std::fs::TryLockError;

use crate::dirs::RuntimeDirs;
use crate::error::SessionLockError;
use crate::session;

/// The right to prepare the Wine prefix and an install overlay and to start a session in them.
/// An OS file lock, so one toolkit process holds it at a time and the OS releases it when that
/// process exits. [`crate::GameProcess`] keeps it until the session ends.
#[derive(Debug)]
pub struct SessionLock {
    #[allow(dead_code, reason = "held for its lock, never read")]
    file: File,
}

impl SessionLock {
    /// Takes the lock without waiting. A toolkit process that exited released its lock, so this
    /// then checks that none of its sessions still run.
    pub fn acquire(dirs: &RuntimeDirs) -> Result<SessionLock, SessionLockError> {
        let lock = Self::lock(dirs)?;

        // One shared prefix and one game client: a second session would join the first one's
        // wineserver and see the wrong install.
        if cfg!(target_os = "linux") {
            let running = session::running_session_pids().map_err(SessionLockError::Scan)?;
            if !running.is_empty() {
                return Err(SessionLockError::SessionRunning { pids: running });
            }
        }

        Ok(lock)
    }

    /// [`Self::acquire`] without the check for running sessions.
    pub(crate) fn lock(dirs: &RuntimeDirs) -> Result<SessionLock, SessionLockError> {
        let root = dirs.root();
        std::fs::create_dir_all(root)
            .map_err(|source| SessionLockError::CreateDir { path: root.to_path_buf(), source })?;

        let path = dirs.session_lock();
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| SessionLockError::Open { path: path.clone(), source })?;
        match file.try_lock() {
            Ok(()) => Ok(SessionLock { file }),
            Err(TryLockError::WouldBlock) => Err(SessionLockError::Held { path }),
            Err(TryLockError::Error(source)) => Err(SessionLockError::Lock { path, source }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_lock_refuses_a_second_holder_until_released() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = RuntimeDirs::new(tmp.path());

        let held = SessionLock::lock(&dirs).unwrap();
        assert!(matches!(SessionLock::lock(&dirs), Err(SessionLockError::Held { .. })));

        drop(held);
        SessionLock::lock(&dirs).unwrap();
    }
}
