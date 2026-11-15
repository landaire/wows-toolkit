//! Game settings read and written through an install's file view, and the replay settings a new
//! build inherits from an older one.

use std::io;
use std::path::Path;
use std::path::PathBuf;

use wgc_launch::InstallFiles;
use wows_game_settings::BuildNumber;
use wows_game_settings::GameActivity;
use wows_game_settings::GameSettingsStore;
use wows_game_settings::GameStopped;
use wows_game_settings::ReplayConfigSource;
use wows_game_settings::ReplaySettings;
use wows_game_settings::Section;
use wows_game_settings::SettingsChanges;
use wows_game_settings::SettingsFiles;

use crate::error::ReplayCarryError;
use crate::launcher::DownloadState;
use crate::launcher::Launcher;
use crate::launcher::LauncherEnv;

/// An install's files as [`wows_game_settings`] reads and writes them, so an install run from an
/// overlay is read as the game sees it and written to its upper directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallSettingsFiles(pub InstallFiles);

impl SettingsFiles for InstallSettingsFiles {
    fn read_path(&self, relative: &Path) -> io::Result<Option<PathBuf>> {
        self.0.resolve_read(relative)
    }

    fn write_path(&self, relative: &Path) -> io::Result<PathBuf> {
        self.0.resolve_write(relative)
    }
}

/// What [`carry_replay_settings`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayCarry {
    /// The install has no `bin/<build>` directory.
    NoBuild,
    /// The newest build has no engine config at all, so it has nothing to override.
    NoEngineConfig {
        build: BuildNumber,
    },
    /// The newest build already has its own override.
    Kept {
        build: BuildNumber,
    },
    /// No older build has an override to take the settings from.
    NoSource {
        build: BuildNumber,
    },
    /// The older override holds what the newest build's defaults already say.
    Unchanged {
        from: BuildNumber,
        to: BuildNumber,
    },
    Written {
        from: BuildNumber,
        to: BuildNumber,
    },
}

impl Launcher {
    /// [`Self::carry_replay_settings`] for every known install.
    pub(crate) fn carry_all_replay_settings(&self, env: &LauncherEnv) {
        for install in self.installs() {
            self.carry_replay_settings(&install.path, env);
        }
    }

    /// [`Self::carry_replay_settings`] once a download into `install` has finished, since an
    /// update brings a new `bin/<build>`.
    pub(crate) fn carry_replay_settings_after(&self, install: &Path, state: &DownloadState, env: &LauncherEnv) {
        if matches!(state, DownloadState::Done { .. }) {
            self.carry_replay_settings(install, env);
        }
    }

    /// Runs [`carry_replay_settings`] for the install at `install` off the calling thread. Skipped
    /// without runtime directories, since overlay state cannot be found without them.
    pub(crate) fn carry_replay_settings(&self, install: &Path, env: &LauncherEnv) {
        let Some(dirs) = env.runtime_dirs.clone() else {
            return;
        };

        let gate = self.game_gate();
        let install = install.to_path_buf();
        env.runtime.spawn_blocking(move || {
            let files = match InstallFiles::existing(&install, &dirs) {
                Ok(files) => files,
                // A saved install that is gone has no builds to carry settings between.
                Err(wgc_launch::ViewError::InstallMissing { .. }) => return,
                Err(err) => {
                    tracing::warn!(install = %install.display(), %err, "replay settings: the install view is unreadable");
                    return;
                }
            };

            match carry_replay_settings(&InstallSettingsFiles(files), &gate) {
                Ok(ReplayCarry::Written { from, to }) => {
                    tracing::info!(install = %install.display(), %from, %to, "replay settings: carried to the new build");
                }
                Ok(_) => {}
                // The next discovery or download retries once the game has exited.
                Err(ReplayCarryError::GameRunning(_)) => {
                    tracing::debug!(install = %install.display(), "replay settings: the game is running");
                }
                Err(err) => {
                    tracing::warn!(install = %install.display(), %err, "replay settings: not carried to the new build");
                }
            }
        });
    }
}

/// The `bin/<build>` directories of the install, oldest first.
pub fn installed_builds(files: &InstallFiles) -> io::Result<Vec<BuildNumber>> {
    let mut builds: Vec<BuildNumber> = files
        .list_dir(Path::new("bin"))?
        .into_iter()
        .filter(|entry| entry.path.is_dir())
        .filter_map(|entry| BuildNumber::parse(&entry.name))
        .collect();
    builds.sort();

    Ok(builds)
}

/// Gives the newest build of the install an engine config override holding the replay settings
/// of the newest older build that has one. The game reads them from `bin/<build>/res_mods`, so a
/// new build otherwise starts from the game's defaults. Writes only while `activity` reports the
/// game stopped.
pub fn carry_replay_settings<A>(files: &InstallSettingsFiles, activity: &A) -> Result<ReplayCarry, ReplayCarryError>
where
    A: GameActivity + ?Sized,
{
    let builds = installed_builds(&files.0).map_err(|source| ReplayCarryError::ListBuilds { source })?;
    let Some((&newest, older)) = builds.split_last() else {
        return Ok(ReplayCarry::NoBuild);
    };

    let target = GameSettingsStore::new(files.clone(), Some(newest));
    let Section::Available(current) = target.read_replays()? else {
        return Ok(ReplayCarry::NoEngineConfig { build: newest });
    };
    if current.source == ReplayConfigSource::Override {
        return Ok(ReplayCarry::Kept { build: newest });
    }

    let Some(source) = newest_override(files, older)? else {
        return Ok(ReplayCarry::NoSource { build: newest });
    };

    let changes = SettingsChanges {
        replays_kept: source.kept.known().copied().filter(|kept| current.kept.known() != Some(kept)),
        versioned_replays: source
            .versioned
            .known()
            .copied()
            .filter(|versioned| current.versioned.known() != Some(versioned)),
        ..SettingsChanges::default()
    };
    if changes.is_empty() {
        return Ok(ReplayCarry::Unchanged { from: source.build, to: newest });
    }

    // Held across the read, so the snapshot the save compares against is one the game cannot
    // rewrite underneath it.
    let stopped = GameStopped::check(activity)?;
    let snapshot = target.read()?;
    target.save(&snapshot, &changes, stopped)?;

    Ok(ReplayCarry::Written { from: source.build, to: newest })
}

/// The replay settings of the newest of `older` whose engine config is an override.
fn newest_override(
    files: &InstallSettingsFiles,
    older: &[BuildNumber],
) -> Result<Option<ReplaySettings>, ReplayCarryError> {
    for &build in older.iter().rev() {
        let store = GameSettingsStore::new(files.clone(), Some(build));
        if let Section::Available(settings) = store.read_replays()?
            && settings.source == ReplayConfigSource::Override
        {
            return Ok(Some(settings));
        }
    }

    Ok(None)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::fs;

    use wgc_launch::OverlayBackend;

    use super::*;
    use crate::game::GameGate;

    fn engine_config(kept: u32, versioned: bool) -> String {
        format!(
            "<engine_config.xml>\n  <replays>\n    <dirPath>replays</dirPath>\n    <versioned>{versioned}</versioned>\n    <maxReplaysToSave>{kept}</maxReplaysToSave>\n  </replays>\n</engine_config.xml>\n"
        )
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn override_path(build: u32) -> PathBuf {
        ["bin", &build.to_string(), "res_mods", "engine_config.xml"].iter().collect()
    }

    fn default_path(build: u32) -> PathBuf {
        ["bin", &build.to_string(), "res", "engine_config.xml"].iter().collect()
    }

    /// A newer build takes the settings of the newest older override, wherever it lives, and the
    /// new override lands in the upper directory.
    #[test]
    fn the_newest_build_takes_the_newest_older_override_in_an_overlay() {
        let tmp = tempfile::tempdir().unwrap();
        let lower = tmp.path().join("lower");
        let upper = tmp.path().join("upper");
        for build in [98, 99, 100, 101] {
            write(&lower.join(default_path(build)), &engine_config(30, false));
        }
        // A mod's override in the install, and a newer one the toolkit saved under the overlay.
        write(&lower.join(override_path(98)), &engine_config(50, false));
        write(&upper.join(override_path(99)), &engine_config(200, true));
        let files = InstallSettingsFiles(InstallFiles::Overlay {
            lower: lower.clone(),
            upper: upper.clone(),
            backend: OverlayBackend::KernelBwrap,
        });
        let gate = GameGate::new();

        let carried = carry_replay_settings(&files, &gate).unwrap();

        assert_eq!(carried, ReplayCarry::Written { from: BuildNumber(99), to: BuildNumber(101) });
        assert!(!lower.join(override_path(101)).exists(), "the install itself is not written");
        insta::assert_snapshot!(fs::read_to_string(upper.join(override_path(101))).unwrap());
        assert_eq!(carry_replay_settings(&files, &gate).unwrap(), ReplayCarry::Kept { build: BuildNumber(101) });
    }

    /// An override equal to the new build's defaults leaves the new build without an override.
    #[test]
    fn an_override_matching_the_defaults_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        write(&root.join(default_path(100)), &engine_config(30, false));
        write(&root.join(override_path(100)), &engine_config(30, false));
        write(&root.join(default_path(101)), &engine_config(30, false));
        let files = InstallSettingsFiles(InstallFiles::Direct { root: root.clone() });

        let carried = carry_replay_settings(&files, &GameGate::new()).unwrap();

        assert_eq!(carried, ReplayCarry::Unchanged { from: BuildNumber(100), to: BuildNumber(101) });
        assert!(!root.join(override_path(101)).exists());
    }
}
