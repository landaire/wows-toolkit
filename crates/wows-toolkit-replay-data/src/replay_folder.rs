//! The folder an install's game writes replays to, as the game sees it.
//!
//! The launcher runs a read-only install from an overlay whose writes land in a separate upper
//! directory, so the replays of such an install are spread over two real directories. While its
//! versioned folders setting is on, the game writes to `replays/<version>` instead of `replays`.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use wgc_launch::InstallFiles;
use wgc_launch::RuntimeDirs;
use wgc_launch::ViewError;
use wows_game_settings::BuildNumber;
use wows_game_settings::GameSettingsStore;
use wows_game_settings::Section;
use wows_game_settings::SettingsFile;
use wows_game_settings::SettingsFiles;
use wowsunpack::data::Version;

use crate::install::last_server_version;
use crate::live_match::ARENA_INFO_FILE;
use crate::live_match::LIVE_STREAM_FILE;
use crate::live_match::LiveSource;
use crate::replay_dir::DirChange;
use crate::replay_dir::is_finished_replay;
use crate::replay_dir::listed_replay;

/// A directory of replays: one the reader named, or the one an install's game writes to.
#[derive(Clone, Debug)]
pub struct ReplayFolder {
    /// The folder as the reader names it. For an install run from an overlay this is under the
    /// install itself and may hold none of the files.
    path: PathBuf,
    install: Option<InstallFolder>,
}

/// What a watch of a replay folder registers, and which of its events matter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchTargets {
    /// Real directories that hold the folder's entries.
    pub contents: Vec<PathBuf>,
    /// Paths not yet created whose creation moves the folder. Watched through their parents.
    pub awaited: Vec<PathBuf>,
    /// Directories in which any new directory may be the folder of a new game version.
    pub version_parents: Vec<PathBuf>,
}

/// A folder's watch, prepared off the thread that starts it.
pub struct WatchPlan {
    pub(crate) targets: WatchTargets,
    pub(crate) merged: Option<MergedView>,
}

/// The finished replays of an overlaid folder as last listed, so an event in either layer becomes
/// the change it made to the folder the game sees.
pub(crate) struct MergedView {
    files: InstallFiles,
    relative: PathBuf,
    known: BTreeMap<OsString, PathBuf>,
}

/// What a filesystem event did to the file it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Seen {
    Created,
    Written,
    Removed,
}

#[derive(Clone, Debug)]
struct InstallFolder {
    install_dir: PathBuf,
    runtime_dirs: Option<RuntimeDirs>,
    files: InstallFiles,
    /// `replays`, or `replays/<version>`.
    relative: PathBuf,
    /// The game's versioned folders setting, when it could be read.
    versioned: Option<bool>,
    /// The overlay record the launcher writes on the first launch from an overlay. `None` without
    /// runtime directories or an install to canonicalize.
    record: Option<PathBuf>,
    /// The content directories whose resolved name differs from the one they are read by.
    respelled: Vec<Respelled>,
    inputs: Inputs,
}

/// A content directory under the name the folder reads it by and under its resolved name, which
/// a ledger or the index may hold some of its replays under.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Respelled {
    named: PathBuf,
    resolved: PathBuf,
}

/// The names of one content directory.
struct LayerNames {
    named: PathBuf,
    resolved: Option<PathBuf>,
}

/// Stamps of the files the folder was resolved from, compared to notice a change without reading
/// them again.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Inputs(Vec<Option<Stamp>>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    path: PathBuf,
    modified: SystemTime,
    len: u64,
}

/// A `replays/<major>.<minor>.<patch>.<n>` folder name, ordered by version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct VersionFolder([u32; 4]);

/// [`SettingsFiles`] over a borrowed view, for reading the replay settings.
struct ViewFiles<'a>(&'a InstallFiles);

impl ReplayFolder {
    /// A directory read as it is.
    pub fn from_dir(dir: PathBuf) -> ReplayFolder {
        ReplayFolder { path: dir, install: None }
    }

    /// The folder the game of the install at `install_dir` writes replays to. `runtime_dirs`
    /// finds the launcher's overlay state. Reads the install's files.
    pub fn from_install(install_dir: &Path, runtime_dirs: Option<&RuntimeDirs>) -> ReplayFolder {
        let install = InstallFolder::resolve(install_dir.to_path_buf(), runtime_dirs.cloned());

        ReplayFolder { path: install.path(), install: Some(install) }
    }

    /// [`Self::from_install`] with the overlay state under the app's data directory.
    pub fn from_app_install(install_dir: &Path) -> ReplayFolder {
        let runtime_dirs = app_runtime_dirs();

        Self::from_install(install_dir, runtime_dirs.as_ref())
    }

    /// The folder as the reader names it.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Resolves the folder again when anything it was resolved from changed. Returns whether the
    /// folder moved.
    pub fn refresh(&mut self) -> bool {
        let Some(install) = &self.install else {
            return false;
        };
        if Inputs::read(&install.files, install.record.as_deref()) == install.inputs {
            return false;
        }

        let resolved = InstallFolder::resolve(install.install_dir.clone(), install.runtime_dirs.clone());
        let path = resolved.path();
        let moved = path != self.path;
        self.path = path;
        self.install = Some(resolved);

        moved
    }

    /// The finished replays the folder holds, each at the real path to read it from.
    pub fn replay_files(&self) -> Vec<PathBuf> {
        match self.overlay() {
            Some((files, relative)) => overlay_replays(files, relative).into_values().collect(),
            None => plain_replays(&self.path),
        }
    }

    /// The real directories that hold the folder's entries, upper first.
    pub fn content_dirs(&self) -> Vec<PathBuf> {
        match &self.install {
            Some(install) => install.files.layer_dirs(&install.relative).unwrap_or_else(|_| vec![self.path.clone()]),
            None => vec![self.path.clone()],
        }
    }

    /// Whether the folder spans the layers of an overlay.
    pub fn is_overlaid(&self) -> bool {
        self.overlay().is_some()
    }

    /// The other paths of the file at `path` in its own directory: the directory under its
    /// resolved name when the folder reads it by another, and the reverse. Empty for a path
    /// outside the folder.
    pub fn spellings(&self, path: &Path) -> Vec<PathBuf> {
        let Some((name, layers, held)) = self.locate(path) else {
            return Vec::new();
        };

        layers[held].paths(name).filter(|other| other != path).collect()
    }

    /// The paths of `path`'s name in the folder's other layers, under each of their names: the
    /// copies the game replaced when it rewrote the file under an overlay. Empty for a folder that
    /// is not overlaid, or a path outside the folder.
    pub fn layer_copies(&self, path: &Path) -> Vec<PathBuf> {
        if !self.is_overlaid() {
            return Vec::new();
        }
        let Some((name, layers, held)) = self.locate(path) else {
            return Vec::new();
        };

        layers.iter().enumerate().filter(|(layer, _)| *layer != held).flat_map(|(_, names)| names.paths(name)).collect()
    }

    /// Every path a ledger or the index may hold the battle at `path` under: its
    /// [`Self::spellings`] and its [`Self::layer_copies`].
    pub fn other_paths(&self, path: &Path) -> Vec<PathBuf> {
        let mut others = self.spellings(path);
        others.extend(self.layer_copies(path));

        others
    }

    /// `path`'s file name, the names of each content directory, and the one that holds `path`.
    fn locate<'a>(&self, path: &'a Path) -> Option<(&'a OsStr, Vec<LayerNames>, usize)> {
        let (Some(name), Some(parent)) = (path.file_name(), path.parent()) else {
            return None;
        };
        let respelled = self.install.as_ref().map_or(&[][..], |install| install.respelled.as_slice());
        let layers: Vec<LayerNames> = self
            .content_dirs()
            .into_iter()
            .map(|named| {
                let resolved = respelled.iter().find(|dir| dir.named == named).map(|dir| dir.resolved.clone());
                LayerNames { named, resolved }
            })
            .collect();
        let held = layers.iter().position(|names| names.holds(parent))?;

        Some((name, layers, held))
    }

    /// Where a battle in progress keeps its two halves. A half that does not exist yet is named
    /// where the game will write it: in an overlay every game write lands in the upper directory.
    pub fn live_source(&self) -> LiveSource {
        let Some((files, relative)) = self.overlay() else {
            return LiveSource::in_dir(&self.path);
        };
        let InstallFiles::Overlay { upper, .. } = files else {
            return LiveSource::in_dir(&self.path);
        };

        let resolve = |name: &str| {
            let relative = relative.join(name);
            files.resolve_read(&relative).ok().flatten().unwrap_or_else(|| upper.join(&relative))
        };

        LiveSource { arena_info: resolve(ARENA_INFO_FILE), stream: resolve(LIVE_STREAM_FILE) }
    }

    /// What a watch of the folder registers.
    pub fn watch_targets(&self) -> WatchTargets {
        let Some(install) = &self.install else {
            return WatchTargets { contents: vec![self.path.clone()], ..WatchTargets::default() };
        };

        let overlay = matches!(install.files, InstallFiles::Overlay { .. });
        let versioned = install.versioned == Some(true);
        let mut targets = WatchTargets::default();

        for dir in install.files.layer_dirs(&install.relative).into_iter().flatten() {
            // An install read in place keeps failing on a missing folder, as it always has, unless
            // the game is about to create a new version's folder.
            if dir.is_dir() || !(overlay || versioned) {
                targets.contents.push(dir);
            } else {
                targets.awaited.extend(first_missing(&dir));
            }
        }

        if versioned {
            for dir in install.files.layer_dirs(Path::new("replays")).into_iter().flatten() {
                if dir.is_dir() {
                    targets.version_parents.push(dir);
                } else {
                    targets.awaited.extend(first_missing(&dir));
                }
            }
        }

        // The first launch from an overlay moves the game's writes to the upper directory. Only
        // Linux runs an install from an overlay.
        if cfg!(target_os = "linux")
            && !overlay
            && let Some(record) = &install.record
        {
            targets.awaited.extend(first_missing(record));
        }

        targets.awaited.sort();
        targets.awaited.dedup();

        targets
    }

    /// The watch of the folder, ready to start. Lists the folder; blocks.
    pub fn watch_plan(&self) -> WatchPlan {
        let merged = self.overlay().map(|(files, relative)| MergedView {
            files: files.clone(),
            relative: relative.to_path_buf(),
            known: overlay_replays(files, relative),
        });

        WatchPlan { targets: self.watch_targets(), merged }
    }

    fn overlay(&self) -> Option<(&InstallFiles, &Path)> {
        let install = self.install.as_ref()?;
        match &install.files {
            InstallFiles::Overlay { .. } => Some((&install.files, install.relative.as_path())),
            InstallFiles::Direct { .. } => None,
        }
    }
}

impl WatchTargets {
    /// The directories to register, each once.
    pub fn watched(&self) -> Vec<PathBuf> {
        let parents = self.awaited.iter().filter_map(|path| path.parent()).map(Path::to_path_buf);
        let mut watched: Vec<PathBuf> = Vec::new();
        for dir in self.contents.iter().cloned().chain(parents).chain(self.version_parents.iter().cloned()) {
            if !watched.contains(&dir) {
                watched.push(dir);
            }
        }

        watched
    }

    /// Whether `path` is an entry of the folder. A platform that reports paths under another
    /// spelling than the one watched leaves an entry of a lone content directory recognizable
    /// only by not being under any other watched directory.
    pub(crate) fn holds(&self, path: &Path) -> bool {
        let Some(parent) = path.parent() else {
            return false;
        };
        if self.contents.iter().any(|dir| dir == parent) {
            return true;
        }

        let mut elsewhere = self
            .awaited
            .iter()
            .filter_map(|awaited| awaited.parent())
            .chain(self.version_parents.iter().map(PathBuf::as_path));

        self.contents.len() == 1 && !elsewhere.any(|dir| dir == parent)
    }

    /// Whether creating `path` moves the folder, so it is resolved again.
    pub(crate) fn moved_by(&self, path: &Path) -> bool {
        if self.awaited.iter().any(|awaited| awaited == path) {
            return true;
        }

        path.parent().is_some_and(|parent| self.version_parents.iter().any(|dir| dir == parent)) && path.is_dir()
    }
}

impl MergedView {
    /// The changes `seen` on `path` made to the folder the game sees.
    pub(crate) fn apply(&mut self, path: &Path, seen: Seen) -> Vec<DirChange> {
        let Some(name) = path.file_name() else {
            return Vec::new();
        };
        // Written throughout a battle, and never a finished replay.
        if name == LIVE_STREAM_FILE || name == ARENA_INFO_FILE {
            return Vec::new();
        }

        if seen == Seen::Written {
            return match self.known.get(name) {
                Some(known) if known == path => vec![DirChange::Changed(path.to_path_buf())],
                _ => Vec::new(),
            };
        }

        // A creation or removal in either layer may be a whiteout or a copy up, so the folder is
        // listed again rather than the event taken at its word.
        let now = overlay_replays(&self.files, &self.relative);
        let changes = merged_changes(&self.known, &now);
        self.known = now;

        changes
    }
}

impl InstallFolder {
    fn resolve(install_dir: PathBuf, runtime_dirs: Option<RuntimeDirs>) -> InstallFolder {
        let files = install_files(&install_dir, runtime_dirs.as_ref());
        let record = runtime_dirs.as_ref().and_then(|dirs| {
            let root = std::fs::canonicalize(&install_dir).ok()?;
            Some(dirs.install_state(&root).backend_record())
        });

        // Stamped before anything is read, so a change made while resolving is seen next time.
        let inputs = Inputs::read(&files, record.as_deref());
        let versioned = versioned_setting(&files);
        let relative = folder_relative(&files, versioned);
        let respelled = respelled_dirs(&files, &relative);

        InstallFolder { install_dir, runtime_dirs, files, relative, versioned, record, respelled, inputs }
    }

    fn path(&self) -> PathBuf {
        self.install_dir.join(&self.relative)
    }
}

impl LayerNames {
    fn holds(&self, dir: &Path) -> bool {
        self.named == dir || self.resolved.as_deref() == Some(dir)
    }

    fn paths<'a>(&'a self, name: &'a OsStr) -> impl Iterator<Item = PathBuf> + 'a {
        std::iter::once(&self.named).chain(&self.resolved).map(move |dir| dir.join(name))
    }
}

impl Inputs {
    fn read(files: &InstallFiles, record: Option<&Path>) -> Inputs {
        let mut paths: Vec<Option<PathBuf>> = vec![record.map(Path::to_path_buf)];
        paths.push(files.resolve_read(Path::new("preferences.xml")).ok().flatten());
        for relative in ["bin", "replays"] {
            paths.extend(files.layer_dirs(Path::new(relative)).into_iter().flatten().map(Some));
        }
        if let Some(build) = newest_build(files) {
            let engine_config = SettingsFile::EngineConfig { build }.relative_path();
            if let Some(res_mods) = engine_config.parent() {
                paths.extend(files.layer_dirs(res_mods).into_iter().flatten().map(Some));
            }
            paths.push(files.resolve_read(&engine_config).ok().flatten());
        }

        Inputs(paths.into_iter().map(|path| path.and_then(stamp)).collect())
    }
}

impl VersionFolder {
    fn parse(name: &OsStr) -> Option<VersionFolder> {
        let mut parts = name.to_str()?.split('.');
        let mut numbers = [0; 4];
        for number in &mut numbers {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            *number = part.parse().ok()?;
        }
        if parts.next().is_some() {
            return None;
        }

        Some(VersionFolder(numbers))
    }

    /// The folder the game names after `version`.
    fn of(version: Version) -> VersionFolder {
        VersionFolder([version.major, version.minor, version.patch, 0])
    }

    fn name(self) -> String {
        let [major, minor, patch, extra] = self.0;
        format!("{major}.{minor}.{patch}.{extra}")
    }
}

impl SettingsFiles for ViewFiles<'_> {
    fn read_path(&self, relative: &Path) -> io::Result<Option<PathBuf>> {
        self.0.resolve_read(relative)
    }

    fn write_path(&self, relative: &Path) -> io::Result<PathBuf> {
        self.0.resolve_write(relative)
    }
}

/// Where the launcher keeps the game runtime and install overlays: under the app's data directory.
/// `None` when the app has no data directory, in which case no overlay can exist.
pub fn app_runtime_dirs() -> Option<RuntimeDirs> {
    wows_toolkit_config::storage_dir().map(|dir| RuntimeDirs::new(&dir))
}

/// The install's files as the game sees them. An install the launcher ran from an overlay is read
/// through it; any other is read in place. Either way under the name the reader gave the install,
/// so the paths found match those already listed, indexed and sent.
pub fn install_files(install_dir: &Path, runtime_dirs: Option<&RuntimeDirs>) -> InstallFiles {
    let direct = InstallFiles::Direct { root: install_dir.to_path_buf() };
    let Some(dirs) = runtime_dirs else {
        return direct;
    };

    match InstallFiles::existing(install_dir, dirs) {
        Ok(files) => files,
        // Nothing to read either way; the reader reports the missing folder.
        Err(ViewError::InstallMissing { .. }) => direct,
        // The launcher refuses to start the game until the overlay state is readable again, so
        // the install itself is what the game last wrote to.
        Err(err) => {
            tracing::warn!(install = %install_dir.display(), %err, "the install's overlay state is unreadable");
            direct
        }
    }
}

/// The `replays` directory in each layer of the install at `install_dir` as the app's launcher
/// left it, upper first. Every replay folder of the install is in one of them.
pub fn app_install_replay_dirs(install_dir: &Path) -> Vec<PathBuf> {
    let replays = Path::new("replays");
    let files = install_files(install_dir, app_runtime_dirs().as_ref());

    // `replays` is a plain relative path, which every view takes.
    files.layer_dirs(replays).unwrap_or_else(|_| vec![install_dir.join(replays)])
}

/// The content directories of `relative` whose layer resolves to another name than the one it is
/// read by.
fn respelled_dirs(files: &InstallFiles, relative: &Path) -> Vec<Respelled> {
    let roots: Vec<&PathBuf> = match files {
        InstallFiles::Direct { root } => vec![root],
        InstallFiles::Overlay { lower, delta, upper, .. } => {
            std::iter::once(upper).chain(delta).chain(std::iter::once(lower)).collect()
        }
    };

    roots
        .into_iter()
        .filter_map(|root| {
            let resolved = std::fs::canonicalize(root).ok()?;
            (resolved != *root).then(|| Respelled { named: root.join(relative), resolved: resolved.join(relative) })
        })
        .collect()
}

/// The highest `bin/<build>` directory the install shows.
fn newest_build(files: &InstallFiles) -> Option<BuildNumber> {
    files
        .list_dir(Path::new("bin"))
        .ok()?
        .into_iter()
        .filter(|entry| entry.path.is_dir())
        .filter_map(|entry| BuildNumber::parse(&entry.name))
        .max()
}

/// The versioned folders setting of the newest build. `None` when it cannot be read.
fn versioned_setting(files: &InstallFiles) -> Option<bool> {
    let build = newest_build(files)?;
    let store = GameSettingsStore::new(ViewFiles(files), Some(build));
    match store.read_replays() {
        Ok(Section::Available(replays)) => replays.versioned.known().copied(),
        Ok(Section::Unavailable(_)) => None,
        Err(err) => {
            tracing::debug!(%err, "the replay settings are unreadable");
            None
        }
    }
}

/// `replays/<version>` while the game writes there, else `replays`. With the setting unknown,
/// the folder of the version the game last ran is taken when it exists.
fn folder_relative(files: &InstallFiles, versioned: Option<bool>) -> PathBuf {
    let replays = PathBuf::from("replays");
    let last_run = last_run_version(files);

    let chosen = match versioned {
        Some(false) => None,
        // The game names the folder after the version it runs, which is newer than the one
        // `preferences.xml` names until the game has rewritten it.
        Some(true) => {
            // An unreadable `replays` holds no version folder to choose; the watch reports it.
            let entries = files.list_dir(&replays).unwrap_or_default();
            let existing = entries
                .into_iter()
                .filter(|entry| entry.path.is_dir())
                .filter_map(|entry| VersionFolder::parse(&entry.name));
            existing.chain(last_run).max()
        }
        None => last_run.filter(|folder| {
            let relative = replays.join(folder.name());
            files.resolve_read(&relative).ok().flatten().is_some_and(|path| path.is_dir())
        }),
    };

    match chosen {
        Some(folder) => replays.join(folder.name()),
        None => replays,
    }
}

/// The version `preferences.xml` says the game last ran.
fn last_run_version(files: &InstallFiles) -> Option<VersionFolder> {
    let path = files.resolve_read(Path::new("preferences.xml")).ok().flatten()?;
    let preferences = std::fs::read_to_string(path).ok()?;
    let version = Version::try_from_client_exe(&last_server_version(&preferences)?)?;

    Some(VersionFolder::of(version))
}

fn plain_replays(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries.flatten().filter_map(|entry| listed_replay(&entry)).collect()
}

/// The finished replays of the overlaid folder `relative`, by name. A folder that cannot be listed
/// holds none, as a plain directory that cannot be read does.
fn overlay_replays(files: &InstallFiles, relative: &Path) -> BTreeMap<OsString, PathBuf> {
    let Ok(entries) = files.list_dir(relative) else {
        return BTreeMap::new();
    };

    entries
        .into_iter()
        .filter(|entry| is_finished_replay(&entry.path))
        .filter(|entry| std::fs::metadata(&entry.path).is_ok_and(|metadata| metadata.is_file()))
        .map(|entry| (entry.name, entry.path))
        .collect()
}

/// What changed between two listings: a file gone or moved to another layer is removed before
/// its new path appears.
fn merged_changes(before: &BTreeMap<OsString, PathBuf>, after: &BTreeMap<OsString, PathBuf>) -> Vec<DirChange> {
    let mut changes = Vec::new();
    for (name, path) in before {
        if after.get(name) != Some(path) {
            changes.push(DirChange::Removed(path.clone()));
        }
    }
    for (name, path) in after {
        if before.get(name) != Some(path) {
            changes.push(DirChange::Appeared(path.clone()));
        }
    }

    changes
}

/// The first component of `path` that does not exist, below its deepest existing ancestor.
fn first_missing(path: &Path) -> Option<PathBuf> {
    let mut missing = path;
    for ancestor in path.ancestors().skip(1) {
        if ancestor.is_dir() {
            return Some(missing.to_path_buf());
        }
        missing = ancestor;
    }

    None
}

fn stamp(path: PathBuf) -> Option<Stamp> {
    let metadata = std::fs::metadata(&path).ok()?;

    Some(Stamp { modified: metadata.modified().ok()?, len: metadata.len(), path })
}

#[cfg(all(test, unix))]
mod tests {
    use std::fmt::Write as _;
    use std::fs;

    use super::*;

    const PREFERENCES: &str = "<root><last_server_version>13, 11, 0, 12668706</last_server_version></root>";

    fn engine_config(versioned: bool) -> String {
        format!(
            "<engine_config.xml>\n  <replays>\n    <versioned>{versioned}</versioned>\n  </replays>\n</engine_config.xml>\n"
        )
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// An install laid out as the launcher leaves one it ran from an overlay: its own files, and
    /// the game's writes in the upper directory of its state.
    struct Overlaid {
        _tmp: tempfile::TempDir,
        base: PathBuf,
        lower: PathBuf,
        upper: PathBuf,
        dirs: RuntimeDirs,
    }

    impl Overlaid {
        fn new() -> Overlaid {
            let tmp = tempfile::tempdir().unwrap();
            let base = fs::canonicalize(tmp.path()).unwrap();
            let lower = base.join("install");
            fs::create_dir_all(&lower).unwrap();
            let dirs = RuntimeDirs::new(&base.join("app"));
            let state = dirs.install_state(&lower);
            write(&state.backend_record(), "fuse-overlayfs");
            let upper = state.upper();
            fs::create_dir_all(&upper).unwrap();

            Overlaid { _tmp: tmp, base, lower, upper, dirs }
        }

        /// `path` with the upper directory, whose name hashes the temporary path, named `upper`.
        fn shown(&self, path: &Path) -> String {
            match path.strip_prefix(&self.upper) {
                Ok(rest) => Path::new("upper").join(rest).display().to_string(),
                Err(_) => path.strip_prefix(&self.base).unwrap_or(path).display().to_string(),
            }
        }

        fn describe(&self, folder: &ReplayFolder) -> String {
            let mut out = String::new();
            writeln!(out, "path: {}", self.shown(folder.path())).unwrap();
            for path in folder.replay_files() {
                writeln!(out, "replay: {}", self.shown(&path)).unwrap();
            }
            let live = folder.live_source();
            writeln!(out, "arena info: {}", self.shown(&live.arena_info)).unwrap();
            writeln!(out, "stream: {}", self.shown(&live.stream)).unwrap();
            let targets = folder.watch_targets();
            for (role, paths) in [
                ("contents", &targets.contents),
                ("awaited", &targets.awaited),
                ("version parent", &targets.version_parents),
            ] {
                for path in paths {
                    writeln!(out, "{role}: {}", self.shown(path)).unwrap();
                }
            }
            out
        }
    }

    /// With versioned folders on, the newest version folder in either layer is followed, a battle
    /// in progress is read where the game writes it, and the layer that lacks the folder waits
    /// for it.
    #[test]
    fn an_overlaid_install_follows_the_newest_version_folder_in_either_layer() {
        let install = Overlaid::new();
        write(&install.lower.join("preferences.xml"), PREFERENCES);
        write(&install.lower.join("bin/100/res/engine_config.xml"), &engine_config(false));
        write(&install.upper.join("bin/100/res_mods/engine_config.xml"), &engine_config(true));
        write(&install.lower.join("replays/13.11.0.0/a.wowsreplay"), "lower");
        write(&install.upper.join("replays/13.12.0.0/c.wowsreplay"), "upper");
        write(&install.upper.join("replays/13.12.0.0/.wh.d.wowsreplay"), "");
        write(&install.upper.join("replays/13.12.0.0/tempArenaInfo.json"), "{}");

        let folder = ReplayFolder::from_install(&install.lower, Some(&install.dirs));

        insta::assert_snapshot!(install.describe(&folder));
    }

    /// An event in either layer becomes the change it made to the folder the game sees.
    #[test]
    fn layer_events_become_changes_to_the_merged_folder() {
        let install = Overlaid::new();
        write(&install.lower.join("replays/a.wowsreplay"), "lower");
        write(&install.lower.join("replays/b.wowsreplay"), "lower");
        let folder = ReplayFolder::from_install(&install.lower, Some(&install.dirs));
        let Some(mut merged) = folder.watch_plan().merged else {
            panic!("an overlaid folder is merged");
        };
        let lower = install.lower.join("replays");
        let upper = install.upper.join("replays");

        let deleted = upper.join(".wh.a.wowsreplay");
        write(&deleted, "");
        assert_eq!(merged.apply(&deleted, Seen::Created), vec![DirChange::Removed(lower.join("a.wowsreplay"))]);

        let copied = upper.join("b.wowsreplay");
        write(&copied, "upper");
        assert_eq!(
            merged.apply(&copied, Seen::Created),
            vec![DirChange::Removed(lower.join("b.wowsreplay")), DirChange::Appeared(copied.clone())]
        );
        assert_eq!(merged.apply(&copied, Seen::Written), vec![DirChange::Changed(copied.clone())]);

        let stream = upper.join(LIVE_STREAM_FILE);
        write(&stream, "packets");
        assert_eq!(merged.apply(&stream, Seen::Created), Vec::new());

        // Without a whiteout, removing the upper copy uncovers the install's own file.
        fs::remove_file(&copied).unwrap();
        assert_eq!(
            merged.apply(&copied, Seen::Removed),
            vec![DirChange::Removed(copied), DirChange::Appeared(lower.join("b.wowsreplay"))]
        );
    }

    /// A new version's folder moves an install read in place once it is resolved again.
    #[test]
    fn a_refresh_follows_a_new_version_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("preferences.xml"), PREFERENCES);
        write(&root.join("bin/100/res/engine_config.xml"), &engine_config(true));
        fs::create_dir_all(root.join("replays/13.11.0.0")).unwrap();
        let mut folder = ReplayFolder::from_install(root, None);
        assert_eq!(folder.path(), root.join("replays/13.11.0.0"));
        assert!(!folder.refresh(), "nothing changed");

        fs::create_dir_all(root.join("replays/13.12.0.0")).unwrap();
        // Coarse timestamps would otherwise hide the change.
        let later = filetime::FileTime::from_unix_time(4_000_000_000, 0);
        filetime::set_file_mtime(root.join("replays"), later).unwrap();

        assert!(folder.refresh(), "the folder moved");
        assert_eq!(folder.path(), root.join("replays/13.12.0.0"));
    }
}
