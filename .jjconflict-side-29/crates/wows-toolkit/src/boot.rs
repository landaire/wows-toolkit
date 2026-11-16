//! Detect a launch that never reached a frame, and drop to a safer render mode
//! when one is found.
//!
//! A driver deadlock during device creation produces no panic and no log, so
//! the panic hook cannot see it. A marker file can: a process killed by an
//! impatient user leaves the filesystem in exactly the state a hung one does,
//! and that is the signal worth acting on either way.
//!
//! Both files are plain files rather than database rows because startup can
//! hang before the database is opened.

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use thiserror::Error;
use tracing::warn;

use crate::gpu::probe::AdapterFingerprint;
use crate::gpu::select::RenderMode;

/// Present between the start of a launch and its first presented frame.
const MARKER_FILE: &str = "render_boot.marker";
/// The mode last attempted, and the hardware it was attempted on.
const MODE_FILE: &str = "render_mode.txt";

#[derive(Debug, Error)]
pub enum BootError {
    #[error("no storage directory is available for the startup marker")]
    NoStorageDir,
    #[error("failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The mode last attempted and the hardware it was attempted on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderState {
    mode: RenderMode,
    fingerprint: AdapterFingerprint,
}

fn read_state(dir: &Path) -> Option<RenderState> {
    let contents = std::fs::read_to_string(dir.join(MODE_FILE)).ok()?;
    let mut lines = contents.lines();
    let mode = RenderMode::from_token(lines.next()?.trim())?;
    let fingerprint = AdapterFingerprint::new(lines.next()?.trim());
    Some(RenderState { mode, fingerprint })
}

fn write_state(dir: &Path, state: &RenderState) -> Result<(), BootError> {
    let path = dir.join(MODE_FILE);
    let contents = format!("{}\n{}\n", state.mode.as_token(), state.fingerprint);
    std::fs::write(&path, contents).map_err(|source| BootError::Write { path, source })
}

/// What a recorded mode is valid for.
///
/// The app version is part of it because a mode records how a particular render
/// stack behaved. Shipping a new wgpu, or a fix to this module, has to start
/// over at the best mode rather than inherit a demotion the old build earned.
fn state_key(fingerprint: &AdapterFingerprint) -> AdapterFingerprint {
    AdapterFingerprint::new(format!("{} {fingerprint}", env!("CARGO_PKG_VERSION")))
}

/// Pick the mode to launch at, given what the previous launch recorded.
///
/// A recorded mode whose fingerprint no longer matches describes a machine that
/// no longer exists, so new hardware or a driver update starts over at the best
/// mode rather than inheriting a demotion earned by different hardware.
fn decide_mode(dir: &Path, fingerprint: &AdapterFingerprint) -> RenderMode {
    let Some(state) = read_state(dir).filter(|state| &state.fingerprint == fingerprint) else {
        return RenderMode::FIRST;
    };
    if dir.join(MARKER_FILE).exists() {
        // The recorded mode was attempted and never presented a frame. The
        // terminal mode stays put: WARP has nothing to fall back to.
        state.mode.next().unwrap_or(RenderMode::CpuRenderer)
    } else {
        state.mode
    }
}

fn write_marker(dir: &Path, mode: RenderMode, fingerprint: &AdapterFingerprint) -> Result<(), BootError> {
    std::fs::create_dir_all(dir).map_err(|source| BootError::Write { path: dir.to_path_buf(), source })?;
    write_state(dir, &RenderState { mode, fingerprint: fingerprint.clone() })?;

    let path = dir.join(MARKER_FILE);
    let mut file = std::fs::File::create(&path).map_err(|source| BootError::Write { path: path.clone(), source })?;
    file.write_all(mode.as_token().as_bytes()).map_err(|source| BootError::Write { path: path.clone(), source })?;
    // The marker is worthless if it is still in the page cache when the
    // machine locks up, which is one of the failures it exists to catch.
    file.sync_all().map_err(|source| BootError::Write { path, source })
}

fn clear_marker(dir: &Path) -> Result<(), BootError> {
    let path = dir.join(MARKER_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(BootError::Write { path, source }),
    }
}

fn storage_dir() -> Result<PathBuf, BootError> {
    crate::storage_dir().ok_or(BootError::NoStorageDir)
}

/// The mode this launch should start from.
///
/// Nothing is written yet: which mode actually runs depends on what the present
/// adapters can satisfy, and only the caller knows that.
pub fn planned_mode(fingerprint: &AdapterFingerprint) -> RenderMode {
    let Ok(dir) = storage_dir() else {
        warn!("No storage directory available: render fallback disabled");
        return RenderMode::FIRST;
    };
    decide_mode(&dir, &state_key(fingerprint))
}

/// Record the mode about to run, and write the marker.
///
/// Failing to write it is not fatal. It costs the fallback, leaving the
/// behaviour that existed before it, which beats refusing to start.
pub fn remember_mode(fingerprint: &AdapterFingerprint, mode: RenderMode) {
    let Ok(dir) = storage_dir() else {
        return;
    };
    if let Err(error) = write_marker(&dir, mode, &state_key(fingerprint)) {
        warn!("Failed to write the startup marker, render fallback disabled: {error}");
    }
}

/// The mode this launch is running, for the panic hook.
static ACTIVE_MODE: std::sync::OnceLock<RenderMode> = std::sync::OnceLock::new();

/// The render mode this launch resolved to, once it has been decided.
pub fn active_mode() -> Option<RenderMode> {
    ACTIVE_MODE.get().copied()
}

/// Record the mode this launch is running.
///
/// Separate from `remember_mode`, which writes the on-disk state and is skipped
/// for a run driven by explicit flags. A crash report needs the mode either way.
pub fn set_active_mode(mode: RenderMode) {
    let _ = ACTIVE_MODE.set(mode);
}

/// Frames observed since launch, capped once the marker has been cleared.
static FRAMES: AtomicU32 = AtomicU32::new(0);

/// Call once per painted frame, from a hook eframe only reaches on the paint
/// path.
///
/// The marker clears on the second call, because the first call happens while
/// the first paint is still being assembled. Reaching the second call proves
/// the first one was submitted and presented, which is past instance creation,
/// adapter selection, device creation, surface configuration, and one present.
///
/// Counting anything on the UI-closure path instead would be wrong: egui reruns
/// that closure for multi-pass layout, and eframe runs it for an invisible
/// viewport it never paints, so a hang inside present would still look like a
/// successful launch.
pub fn note_frame() {
    if FRAMES.load(Ordering::Relaxed) > 1 {
        return;
    }
    if FRAMES.fetch_add(1, Ordering::Relaxed) == 1 {
        succeeded();
    }
}

/// Record that this configuration reached a frame.
pub fn succeeded() {
    let Ok(dir) = storage_dir() else {
        return;
    };
    if let Err(error) = clear_marker(&dir) {
        warn!("Failed to clear the startup marker: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint(value: &str) -> AdapterFingerprint {
        AdapterFingerprint::new(value)
    }

    /// A launch that runs the mode it was asked to run.
    fn launch(dir: &Path, fingerprint: &AdapterFingerprint) -> RenderMode {
        let mode = decide_mode(dir, fingerprint);
        write_marker(dir, mode, fingerprint).unwrap();
        mode
    }

    /// A launch whose adapters could not satisfy the planned mode, so a later
    /// one ran instead. This is what `resolve` does when it skips.
    fn launch_falling_back_to(dir: &Path, fingerprint: &AdapterFingerprint, resolved: RenderMode) -> RenderMode {
        decide_mode(dir, fingerprint);
        write_marker(dir, resolved, fingerprint).unwrap();
        resolved
    }

    #[test]
    fn a_clean_first_launch_starts_at_the_best_mode() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(launch(dir.path(), &fingerprint("nvidia")), RenderMode::PinnedVulkan);
    }

    #[test]
    fn a_launch_that_never_presented_advances_exactly_one_mode() {
        let dir = tempfile::tempdir().unwrap();
        let fp = fingerprint("nvidia");

        assert_eq!(launch(dir.path(), &fp), RenderMode::PinnedVulkan);
        assert_eq!(launch(dir.path(), &fp), RenderMode::PinnedDx12);
        assert_eq!(launch(dir.path(), &fp), RenderMode::PinnedVulkanAlternate);
    }

    #[test]
    fn a_working_configuration_is_reused_without_retrying_earlier_modes() {
        let dir = tempfile::tempdir().unwrap();
        let fp = fingerprint("nvidia");

        launch(dir.path(), &fp);
        launch(dir.path(), &fp);
        assert_eq!(decide_mode(dir.path(), &fp), RenderMode::PinnedVulkanAlternate);

        launch(dir.path(), &fp);
        clear_marker(dir.path()).unwrap();

        assert_eq!(launch(dir.path(), &fp), RenderMode::PinnedVulkanAlternate);
        clear_marker(dir.path()).unwrap();
        assert_eq!(launch(dir.path(), &fp), RenderMode::PinnedVulkanAlternate);
    }

    #[test]
    fn the_terminal_mode_stays_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let fp = fingerprint("nvidia");

        let mut mode = launch(dir.path(), &fp);
        for _ in 0..10 {
            mode = launch(dir.path(), &fp);
        }

        assert_eq!(mode, RenderMode::CpuRenderer);
    }

    #[test]
    fn changed_hardware_discards_a_demotion_earned_by_different_hardware() {
        let dir = tempfile::tempdir().unwrap();
        let old = fingerprint("nvidia");

        launch(dir.path(), &old);
        launch(dir.path(), &old);
        assert_eq!(decide_mode(dir.path(), &old), RenderMode::PinnedVulkanAlternate);

        assert_eq!(decide_mode(dir.path(), &fingerprint("nvidia+amd")), RenderMode::PinnedVulkan);
    }

    #[test]
    fn a_successful_launch_clears_the_marker() {
        let dir = tempfile::tempdir().unwrap();
        let fp = fingerprint("nvidia");

        launch(dir.path(), &fp);
        assert!(dir.path().join(MARKER_FILE).exists());

        clear_marker(dir.path()).unwrap();

        assert!(!dir.path().join(MARKER_FILE).exists());
        assert_eq!(decide_mode(dir.path(), &fp), RenderMode::PinnedVulkan);
    }

    #[test]
    fn clearing_the_marker_twice_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();

        clear_marker(dir.path()).unwrap();
        clear_marker(dir.path()).unwrap();
    }

    #[test]
    fn a_corrupt_mode_file_falls_back_to_the_best_mode() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODE_FILE), "not-a-mode\nnvidia\n").unwrap();
        std::fs::write(dir.path().join(MARKER_FILE), "").unwrap();

        assert_eq!(decide_mode(dir.path(), &fingerprint("nvidia")), RenderMode::PinnedVulkan);
    }

    /// The bug this guards: recording the mode that was *asked* for, rather
    /// than the one that ran, spends one launch per skipped mode re-attempting
    /// an identical configuration. A machine with no pinnable adapters resolves
    /// every pinned mode to the same unpinned config, so recording the request
    /// would hang five times before reaching WARP.
    #[test]
    fn recording_the_mode_that_ran_does_not_re_attempt_skipped_modes() {
        let dir = tempfile::tempdir().unwrap();
        let fp = fingerprint("no-pinnable-adapters");

        assert_eq!(launch_falling_back_to(dir.path(), &fp, RenderMode::UnpinnedHardened), RenderMode::UnpinnedHardened);
        assert_eq!(decide_mode(dir.path(), &fp), RenderMode::Unpinned);

        assert_eq!(launch_falling_back_to(dir.path(), &fp, RenderMode::Unpinned), RenderMode::Unpinned);
        assert_eq!(decide_mode(dir.path(), &fp), RenderMode::CpuRenderer);
    }

    #[test]
    fn a_new_app_version_retries_the_best_mode() {
        let dir = tempfile::tempdir().unwrap();
        let hardware = fingerprint("nvidia");

        launch(dir.path(), &state_key(&hardware));
        launch(dir.path(), &state_key(&hardware));
        assert_eq!(decide_mode(dir.path(), &state_key(&hardware)), RenderMode::PinnedVulkanAlternate);

        // What the next release computes for the same hardware.
        let next_release = AdapterFingerprint::new(format!("99.99.99 {hardware}"));

        assert_eq!(decide_mode(dir.path(), &next_release), RenderMode::PinnedVulkan);
    }

    #[test]
    fn the_state_key_covers_both_hardware_and_version() {
        let key = state_key(&fingerprint("nvidia"));

        assert!(key.as_str().contains(env!("CARGO_PKG_VERSION")));
        assert!(key.as_str().contains("nvidia"));
    }

    #[test]
    fn render_state_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let state = RenderState { mode: RenderMode::UnpinnedHardened, fingerprint: fingerprint("a=1;b=2") };

        write_state(dir.path(), &state).unwrap();

        assert_eq!(read_state(dir.path()), Some(state));
    }
}
