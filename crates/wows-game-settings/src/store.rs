use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use tempfile::NamedTempFile;

use crate::changes::PlannedEdit;
use crate::changes::SettingsChanges;
use crate::changes::plan;
use crate::document::PrefsDocument;
use crate::document::PrefsEdit;
use crate::error::ReadError;
use crate::error::SaveError;
use crate::files::BuildNumber;
use crate::files::SettingsFile;
use crate::files::SettingsFiles;
use crate::files::commit_replacement;
use crate::files::create_backup;
use crate::files::prune_backups;
use crate::files::stage_replacement;
use crate::files::write_atomic;
use crate::guard::GameStopped;
use crate::settings::GameSettings;
use crate::settings::ReplayConfigSource;
use crate::settings::ReplaySettings;
use crate::settings::Section;
use crate::settings::SourceDocuments;
use crate::settings::replays;

/// Reads and saves the settings of one install.
#[derive(Clone, Debug)]
pub struct GameSettingsStore<F> {
    files: F,
    build: Option<BuildNumber>,
}

/// The settings as read, with the file contents a save compares against.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    settings: GameSettings,
    files: LoadedFiles,
}

#[derive(Clone, Debug, PartialEq)]
struct LoadedFiles {
    preferences: Option<LoadedFile>,
    user_preferences: Option<LoadedFile>,
    engine_config: Option<LoadedEngineConfig>,
    game_info: Option<LoadedFile>,
}

#[derive(Clone, Debug, PartialEq)]
struct LoadedFile {
    path: PathBuf,
    document: PrefsDocument,
}

#[derive(Clone, Debug, PartialEq)]
struct LoadedEngineConfig {
    build: BuildNumber,
    source: ReplayConfigSource,
    file: LoadedFile,
}

/// A new file version written beside its target, not yet in place.
struct StagedWrite {
    target: PathBuf,
    temporary: NamedTempFile,
    /// The content `temporary` replaces; `None` when the target is new.
    previous: Option<String>,
    /// The backup this save made of the replaced file.
    backup: Option<PathBuf>,
}

impl<F> GameSettingsStore<F>
where
    F: SettingsFiles,
{
    /// `build` selects `bin/<build>` for replay settings; without one they
    /// are unavailable.
    pub fn new(files: F, build: Option<BuildNumber>) -> GameSettingsStore<F> {
        GameSettingsStore { files, build }
    }

    pub fn read(&self) -> Result<Snapshot, ReadError> {
        let engine_config = match self.build {
            Some(build) => self.read_engine_config(build)?,
            None => None,
        };
        let files = LoadedFiles {
            preferences: self.read_file(&SettingsFile::Preferences.relative_path())?,
            user_preferences: self.read_file(&SettingsFile::UserPreferences.relative_path())?,
            engine_config,
            game_info: self.read_file(&SettingsFile::GameInfo.relative_path())?,
        };

        Ok(Snapshot { settings: GameSettings::from_documents(&files.sources(self.build)), files })
    }

    /// The replay settings alone. Reads only the build's engine config, so a damaged
    /// preferences file does not hide them.
    pub fn read_replays(&self) -> Result<Section<ReplaySettings>, ReadError> {
        let engine_config = match self.build {
            Some(build) => self.read_engine_config(build)?,
            None => None,
        };
        let files = LoadedFiles { preferences: None, user_preferences: None, engine_config, game_info: None };

        Ok(replays(&files.sources(self.build)))
    }

    /// Writes `changes` and returns the settings read back.
    ///
    /// Each file is read again first. A value the game rewrote since
    /// `snapshot` is kept unless this save also changes it, which is refused
    /// as `ChangedOnDisk`. Every edit is checked and every new file staged
    /// before any file is replaced.
    pub fn save<H>(
        &self,
        snapshot: &Snapshot,
        changes: &SettingsChanges,
        _stopped: GameStopped<H>,
    ) -> Result<Snapshot, SaveError> {
        let current = self.read()?;
        let user_preferences = current.files.user_preferences.as_ref().map(|loaded| &loaded.document);
        let mut edits = plan(changes, &current.settings, user_preferences)?;
        edits.retain(|planned| {
            let target =
                current.files.file(planned.file).and_then(|loaded| edit_target(&loaded.document, &planned.edit));
            target != Some(edit_value(&planned.edit))
        });
        if edits.is_empty() {
            return Ok(current);
        }

        for planned in &edits {
            let before = snapshot.files.file(planned.file).map(|loaded| edit_target(&loaded.document, &planned.edit));
            let Some(loaded) = current.files.file(planned.file) else {
                return Err(SaveError::ChangedOnDisk { path: planned.file.relative_path() });
            };
            if before != Some(edit_target(&loaded.document, &planned.edit)) {
                return Err(SaveError::ChangedOnDisk { path: loaded.path.clone() });
            }
        }

        let mut updated = current.files.clone();
        let mut touched: Vec<SettingsFile> = Vec::new();
        for PlannedEdit { file, edit } in &edits {
            let Some(loaded) = updated.file_mut(*file) else {
                return Err(SaveError::ChangedOnDisk { path: file.relative_path() });
            };
            loaded.document.set(edit).map_err(|source| SaveError::Edit { path: loaded.path.clone(), source })?;
            if !touched.contains(file) {
                touched.push(*file);
            }
        }

        let mut staged: Vec<StagedWrite> = Vec::with_capacity(touched.len());
        for file in touched {
            // Both exist: the edit loop above found the file in each.
            let (Some(read), Some(next)) = (current.files.file(file), updated.file(file)) else {
                continue;
            };
            match self.stage(file, read, current.files.replaces_existing(file), next) {
                Ok(write) => staged.push(write),
                Err(error) => {
                    remove_backups(staged.iter().filter_map(|write| write.backup.as_deref()));
                    return Err(error);
                }
            }
        }

        commit(staged)?;

        Ok(self.read()?)
    }

    /// Writes `next` to a temporary file beside its target and, when it
    /// replaces an existing file, backs that file up.
    fn stage(
        &self,
        file: SettingsFile,
        read: &LoadedFile,
        replaces_existing: bool,
        next: &LoadedFile,
    ) -> Result<StagedWrite, SaveError> {
        let relative = file.relative_path();
        let target =
            self.files.write_path(&relative).map_err(|source| SaveError::Write { path: relative.clone(), source })?;

        // What the target itself holds, for rollback. Under an overlay, or for
        // a new engine config override, it may not exist although the file
        // that was read does.
        let previous = if target == read.path {
            Some(read.document.text().to_owned())
        } else {
            match fs::read_to_string(&target) {
                Ok(text) => Some(text),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(source) => return Err(SaveError::Write { path: target, source }),
            }
        };

        let temporary = stage_replacement(&target, next.document.text())
            .map_err(|source| SaveError::Write { path: target.clone(), source })?;

        let backup = if replaces_existing {
            let backup = create_backup(&target, read.document.text())
                .map_err(|source| SaveError::Backup { path: target.clone(), source })?;
            Some(backup)
        } else {
            None
        };

        Ok(StagedWrite { target, temporary, previous, backup })
    }

    fn read_engine_config(&self, build: BuildNumber) -> Result<Option<LoadedEngineConfig>, ReadError> {
        let file = SettingsFile::EngineConfig { build };
        if let Some(loaded) = self.read_file(&file.relative_path())? {
            return Ok(Some(LoadedEngineConfig { build, source: ReplayConfigSource::Override, file: loaded }));
        }

        let Some(fallback) = file.fallback_path() else {
            return Ok(None);
        };
        let loaded = self.read_file(&fallback)?;

        Ok(loaded.map(|loaded| LoadedEngineConfig { build, source: ReplayConfigSource::GameDefault, file: loaded }))
    }

    fn read_file(&self, relative: &Path) -> Result<Option<LoadedFile>, ReadError> {
        let resolved =
            self.files.read_path(relative).map_err(|source| ReadError::Io { path: relative.to_owned(), source })?;
        let Some(path) = resolved else {
            return Ok(None);
        };

        let bytes = fs::read(&path).map_err(|source| ReadError::Io { path: path.clone(), source })?;
        let Ok(text) = String::from_utf8(bytes) else {
            return Err(ReadError::NotUtf8 { path });
        };
        let document = PrefsDocument::parse(text).map_err(|source| ReadError::Parse { path: path.clone(), source })?;

        Ok(Some(LoadedFile { path, document }))
    }
}

impl Snapshot {
    pub fn settings(&self) -> &GameSettings {
        &self.settings
    }

    /// The paths the settings were read from, for display.
    pub fn source_paths(&self) -> impl Iterator<Item = &Path> {
        let files = &self.files;
        [
            files.preferences.as_ref(),
            files.user_preferences.as_ref(),
            files.engine_config.as_ref().map(|loaded| &loaded.file),
            files.game_info.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(|loaded| loaded.path.as_path())
    }
}

impl LoadedFiles {
    fn sources(&self, build: Option<BuildNumber>) -> SourceDocuments<'_> {
        SourceDocuments {
            build,
            preferences: self.preferences.as_ref().map(|loaded| &loaded.document),
            user_preferences: self.user_preferences.as_ref().map(|loaded| &loaded.document),
            engine_config: self.engine_config.as_ref().map(|loaded| (&loaded.file.document, loaded.source)),
            game_info: self.game_info.as_ref().map(|loaded| &loaded.document),
        }
    }

    fn file(&self, file: SettingsFile) -> Option<&LoadedFile> {
        match file {
            SettingsFile::Preferences => self.preferences.as_ref(),
            SettingsFile::UserPreferences => self.user_preferences.as_ref(),
            SettingsFile::EngineConfig { build } => {
                self.engine_config.as_ref().filter(|loaded| loaded.build == build).map(|loaded| &loaded.file)
            }
            SettingsFile::GameInfo => self.game_info.as_ref(),
        }
    }

    fn file_mut(&mut self, file: SettingsFile) -> Option<&mut LoadedFile> {
        match file {
            SettingsFile::Preferences => self.preferences.as_mut(),
            SettingsFile::UserPreferences => self.user_preferences.as_mut(),
            SettingsFile::EngineConfig { build } => {
                self.engine_config.as_mut().filter(|loaded| loaded.build == build).map(|loaded| &mut loaded.file)
            }
            SettingsFile::GameInfo => self.game_info.as_mut(),
        }
    }

    /// Whether writing `file` replaces a file that exists, which then gets a
    /// backup. An engine config read from the build's defaults becomes a new
    /// override instead.
    fn replaces_existing(&self, file: SettingsFile) -> bool {
        match file {
            SettingsFile::EngineConfig { .. } => {
                self.engine_config.as_ref().is_some_and(|loaded| loaded.source == ReplayConfigSource::Override)
            }
            SettingsFile::Preferences | SettingsFile::UserPreferences | SettingsFile::GameInfo => {
                self.file(file).is_some()
            }
        }
    }
}

/// The text an edit replaces, for detecting a rewrite by the game.
fn edit_target<'a>(document: &'a PrefsDocument, edit: &PrefsEdit) -> Option<&'a str> {
    match edit {
        PrefsEdit::SetValue { path, .. } => document.value(path),
        PrefsEdit::SetAttribute { path, name, .. } => document.attribute(path, name),
    }
}

fn edit_value(edit: &PrefsEdit) -> &str {
    match edit {
        PrefsEdit::SetValue { value, .. } | PrefsEdit::SetAttribute { value, .. } => value,
    }
}

/// Moves each staged file into place. When one fails, the files already
/// replaced get their previous content back, so a failed save leaves the
/// install as it was.
fn commit(staged: Vec<StagedWrite>) -> Result<(), SaveError> {
    let backups: Vec<PathBuf> = staged.iter().filter_map(|write| write.backup.clone()).collect();
    let mut replaced: Vec<(PathBuf, Option<String>)> = Vec::with_capacity(staged.len());
    for StagedWrite { target, temporary, previous, .. } in staged {
        if let Err(source) = commit_replacement(temporary, &target) {
            let unrestored: Vec<PathBuf> = replaced
                .into_iter()
                .filter_map(|(path, previous)| restore(&path, previous.as_deref()).err().map(|_| path))
                .collect();

            // The new backups are what recovers the unrestored files.
            if !unrestored.is_empty() {
                return Err(SaveError::PartiallyWritten { path: target, source, unrestored });
            }

            remove_backups(backups.iter().map(PathBuf::as_path));
            return Err(SaveError::Write { path: target, source });
        }

        replaced.push((target, previous));
    }

    // The save is complete. A backup left over by a failed prune is harmless,
    // and the next save prunes it.
    for (target, _) in &replaced {
        let _ = prune_backups(target);
    }

    Ok(())
}

fn restore(target: &Path, previous: Option<&str>) -> io::Result<()> {
    match previous {
        Some(text) => write_atomic(target, text),
        None => fs::remove_file(target),
    }
}

/// Removes the backups of a save that changed nothing. One that cannot be
/// removed is harmless, and the save's own error is the one to report.
fn remove_backups<'a>(backups: impl Iterator<Item = &'a Path>) {
    for backup in backups {
        let _ = fs::remove_file(backup);
    }
}
