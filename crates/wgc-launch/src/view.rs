//! The directory a game runs from: the install itself when it is writable,
//! otherwise an overlay that sends every game write to a toolkit-owned upper
//! directory.

use std::ffi::OsString;
use std::io::Write;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::dirs::InstallKey;
use crate::dirs::InstallStateDir;
use crate::dirs::RuntimeDirs;
use crate::error::RelativePathError;
use crate::error::ViewError;
use crate::host::HostSearch;
use crate::overlay::OverlayBackend;
use crate::overlay::OverlayLayers;
use crate::overlay::OverlayTools;
use crate::overlay::markers;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallView {
    Direct { root: PathBuf },
    Overlay(OverlayView),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayView {
    key: InstallKey,
    layers: OverlayLayers,
    tools: OverlayTools,
}

impl InstallView {
    /// Picks the view for the install at `install_root`. Spawns a short
    /// overlay probe mount the first time a read-only install is prepared.
    pub fn prepare(install_root: &Path, dirs: &RuntimeDirs) -> Result<Self, ViewError> {
        Self::prepare_with(install_root, dirs, &HostSearch::from_env())
    }

    pub fn prepare_with(install_root: &Path, dirs: &RuntimeDirs, search: &HostSearch) -> Result<Self, ViewError> {
        let root = canonical_root(install_root)?;
        let plain = dirs.install_state(&root);
        let delta = read_delta_record(&plain.delta_record())?;
        let state = if delta.is_some() { plain.with_delta() } else { plain };
        let record_path = state.backend_record();

        // An install that has an upper directory keeps using it even if it
        // became writable, so its preferences and replays do not vanish.
        let recorded = read_backend_record(&record_path)?;

        if delta.is_none() && recorded.is_none() && probe_writable(&root)? == Writability::Writable {
            return Ok(Self::Direct { root });
        }
        if !cfg!(target_os = "linux") {
            return Err(match delta {
                Some(_) => ViewError::DeltaUnsupported { path: root },
                None => ViewError::ReadOnlyUnsupported { path: root },
            });
        }

        let has_delta = delta.is_some();
        for dir in [state.upper(), state.work(), state.merged()] {
            std::fs::create_dir_all(&dir).map_err(|source| ViewError::StateDir { path: dir, source })?;
        }
        // The stale mount check compares the merged directory with the mount table's resolved paths.
        let state =
            state.canonicalize().map_err(|source| ViewError::StateDir { path: state.dir().to_path_buf(), source })?;
        let layers =
            OverlayLayers { lower: root, delta, upper: state.upper(), work: state.work(), merged: state.merged() };

        // Probe only when choosing: prepare also runs while a game runs, and
        // overlayfs does not support two mounts sharing one upper directory.
        let tools = match recorded {
            Some(backend) => OverlayTools::find(backend, search)
                .ok_or_else(|| ViewError::BackendUnavailable { upper: layers.upper.clone(), recorded: backend })?,
            None => {
                let candidates = [OverlayBackend::KernelBwrap, OverlayBackend::FuseOverlayfs]
                    .into_iter()
                    .filter_map(|backend| OverlayTools::find(backend, search));
                choose_backend(&state, candidates, |tools| tools.probe(&layers, state.dir())).map_err(
                    |err| match err {
                        ViewError::NoOverlayBackend if has_delta => ViewError::NoDeltaBackend,
                        err => err,
                    },
                )?
            }
        };

        Ok(Self::Overlay(OverlayView { key: state.key().clone(), layers, tools }))
    }

    /// Lays the read-only directory `delta` between the install and the
    /// upper in every later view of the install, or stops when `None`.
    /// Views with a delta keep their own upper. Call only while no game
    /// runs from the install.
    pub fn select_delta(install_root: &Path, dirs: &RuntimeDirs, delta: Option<&Path>) -> Result<(), ViewError> {
        let root = canonical_root(install_root)?;
        let state = dirs.install_state(&root);
        let record = state.delta_record();

        let Some(delta) = delta else {
            return match std::fs::remove_file(&record) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(ViewError::StateDir { path: record, source }),
            };
        };

        std::fs::create_dir_all(state.dir())
            .map_err(|source| ViewError::StateDir { path: state.dir().to_path_buf(), source })?;
        std::fs::write(&record, path_to_bytes(delta)).map_err(|source| ViewError::StateDir { path: record, source })
    }

    /// The game stub inside the view, as a host path.
    pub fn game_exe(&self) -> PathBuf {
        self.game_root().join("WorldOfWarships.exe")
    }

    /// The install root as the game sees it. For an overlay this path is
    /// populated only inside the running game's view.
    pub fn game_root(&self) -> &Path {
        match self {
            Self::Direct { root } => root,
            Self::Overlay(overlay) => &overlay.layers.merged,
        }
    }

    /// Install-relative file access for the rest of the toolkit.
    pub fn files(&self) -> InstallFiles {
        match self {
            Self::Direct { root } => InstallFiles::Direct { root: root.clone() },
            Self::Overlay(overlay) => InstallFiles::Overlay {
                lower: overlay.layers.lower.clone(),
                delta: overlay.layers.delta.clone(),
                upper: overlay.layers.upper.clone(),
                backend: overlay.tools.backend(),
            },
        }
    }
}

impl OverlayView {
    pub fn key(&self) -> &InstallKey {
        &self.key
    }

    pub fn layers(&self) -> &OverlayLayers {
        &self.layers
    }

    pub fn backend(&self) -> OverlayBackend {
        self.tools.backend()
    }

    pub(crate) fn tools(&self) -> &OverlayTools {
        &self.tools
    }
}

/// Resolves install-relative paths such as `preferences.xml` or
/// `replays/tempArenaInfo.json` to real files, applying overlay whiteouts.
/// Reads are valid at any time. Writes are valid only while no game runs:
/// the game rewrites its preferences on exit, and overlayfs does not allow
/// changing the upper directory under a mounted overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallFiles {
    Direct { root: PathBuf },
    Overlay { lower: PathBuf, delta: Option<PathBuf>, upper: PathBuf, backend: OverlayBackend },
}

impl InstallFiles {
    /// The files of the install at `install_root` as its last prepared view
    /// left them, without probing or creating overlay state. An install
    /// never run from an overlay is read in place. Install paths keep the
    /// caller's spelling of `install_root`, so they match paths the caller
    /// stored; the resolved root only finds the overlay state.
    pub fn existing(install_root: &Path, dirs: &RuntimeDirs) -> Result<Self, ViewError> {
        let root = canonical_root(install_root)?;
        let plain = dirs.install_state(&root);
        let delta = read_delta_record(&plain.delta_record())?;
        let state = if delta.is_some() { plain.with_delta() } else { plain };
        let named = install_root.to_path_buf();

        Ok(match read_backend_record(&state.backend_record())? {
            Some(backend) => Self::Overlay { lower: named, delta, upper: state.upper(), backend },
            None => Self::Direct { root: named },
        })
    }

    /// The file a reader should open, or `None` if it does not exist in the
    /// merged view.
    pub fn resolve_read(&self, rel: &Path) -> std::io::Result<Option<PathBuf>> {
        check_relative(rel)?;
        let (lower, delta, upper) = match self {
            Self::Direct { root } => {
                let path = root.join(rel);
                return Ok(path.symlink_metadata().is_ok().then_some(path));
            }
            Self::Overlay { lower, delta, upper, .. } => (lower, delta, upper),
        };

        // Each layer above the install may hide what lies below it.
        for layer in std::iter::once(upper).chain(delta) {
            let Some(below_visible) = upper_ancestors_allow_lower(layer, rel) else {
                return Ok(None);
            };

            let path = layer.join(rel);
            if markers::is_whiteout(&path) {
                return Ok(None);
            }
            if path.symlink_metadata().is_ok() {
                return Ok(Some(path));
            }
            if !below_visible {
                return Ok(None);
            }
        }

        let lower_path = lower.join(rel);
        Ok(lower_path.symlink_metadata().is_ok().then_some(lower_path))
    }

    /// Where a writer should put `rel`. For an overlay this is in the upper
    /// directory; parent directories are created and whiteouts cleared.
    pub fn resolve_write(&self, rel: &Path) -> std::io::Result<PathBuf> {
        check_relative(rel)?;
        let (upper, backend) = match self {
            Self::Direct { root } => {
                let target = root.join(rel);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                return Ok(target);
            }
            Self::Overlay { upper, backend, .. } => (upper, *backend),
        };

        let mut dir = upper.clone();
        let parents: Vec<_> = rel.parent().map(|parent| parent.components().collect()).unwrap_or_default();
        for component in parents {
            dir.push(component);
            if markers::is_whiteout(&dir) {
                markers::clear_whiteout(&dir)?;
                std::fs::create_dir(&dir)?;
                markers::mark_opaque(&dir, backend)?;
            } else if !dir.is_dir() {
                std::fs::create_dir(&dir)?;
            }
        }

        let target = upper.join(rel);
        markers::clear_whiteout(&target)?;

        Ok(target)
    }

    /// Entries of the directory `rel` in the merged view, sorted by name.
    pub fn list_dir(&self, rel: &Path) -> std::io::Result<Vec<InstallDirEntry>> {
        check_relative(rel)?;
        let (lower, delta, upper) = match self {
            Self::Direct { root } => {
                let mut entries = read_entries(&root.join(rel))?;
                entries.sort_by(|a, b| a.name.cmp(&b.name));
                return Ok(entries);
            }
            Self::Overlay { lower, delta, upper, .. } => (lower, delta, upper),
        };

        let mut hidden = std::collections::HashSet::new();
        let mut merged = std::collections::BTreeMap::new();
        let mut lower_visible = true;
        // Each layer above the install may hide entries of those below it.
        for layer in std::iter::once(upper).chain(delta) {
            let Some(below_visible) = upper_ancestors_allow_lower(layer, rel) else {
                lower_visible = false;
                break;
            };

            let layer_dir = layer.join(rel);
            if markers::is_whiteout(&layer_dir) {
                lower_visible = false;
                break;
            }

            for entry in read_entries(&layer_dir)? {
                if let Some(name) = markers::hidden_name(&entry.name) {
                    hidden.insert(name.to_owned());
                    continue;
                }
                if markers::is_marker_name(&entry.name) {
                    continue;
                }
                if markers::is_whiteout(&entry.path) {
                    hidden.insert(entry.name.clone());
                    continue;
                }
                if !hidden.contains(&entry.name) {
                    merged.entry(entry.name.clone()).or_insert(entry);
                }
            }

            if !below_visible || (layer_dir.is_dir() && markers::is_opaque(&layer_dir)) {
                lower_visible = false;
                break;
            }
        }

        if lower_visible {
            for entry in read_entries(&lower.join(rel))? {
                if !hidden.contains(&entry.name) {
                    merged.entry(entry.name.clone()).or_insert(entry);
                }
            }
        }

        Ok(merged.into_values().collect())
    }

    /// Real directories that hold entries of `rel`, upper first. A file
    /// watcher watches all of them to see changes in the merged view.
    pub fn layer_dirs(&self, rel: &Path) -> std::io::Result<Vec<PathBuf>> {
        check_relative(rel)?;
        Ok(match self {
            Self::Direct { root } => vec![root.join(rel)],
            Self::Overlay { lower, delta, upper, .. } => {
                std::iter::once(upper).chain(delta).chain(std::iter::once(lower)).map(|layer| layer.join(rel)).collect()
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallDirEntry {
    pub name: OsString,
    /// The real file, in whichever layer holds it.
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Writability {
    Writable,
    ReadOnly,
}

/// Mount flags miss ACLs, ownership and read-only bind mounts, so this
/// creates and removes a file instead.
fn probe_writable(root: &Path) -> Result<Writability, ViewError> {
    let probe = root.join(format!(".wows-toolkit-write-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => {
            drop(file);
            std::fs::remove_file(&probe).map_err(|source| ViewError::WriteProbe { path: probe, source })?;
            Ok(Writability::Writable)
        }
        Err(e) if matches!(e.kind(), std::io::ErrorKind::ReadOnlyFilesystem | std::io::ErrorKind::PermissionDenied) => {
            Ok(Writability::ReadOnly)
        }
        Err(source) => Err(ViewError::WriteProbe { path: root.to_path_buf(), source }),
    }
}

/// Walks the parents of `rel` in `upper`. `None`: a parent was deleted, so
/// `rel` does not exist. `Some(false)`: an opaque parent hides the lower layer.
fn upper_ancestors_allow_lower(upper: &Path, rel: &Path) -> Option<bool> {
    let mut lower_visible = true;
    let mut dir = upper.to_path_buf();
    let parents: Vec<_> = rel.parent().map(|parent| parent.components().collect()).unwrap_or_default();
    for component in parents {
        dir.push(component);
        if markers::is_whiteout(&dir) {
            return None;
        }
        match dir.symlink_metadata() {
            Ok(metadata) if metadata.is_dir() => {
                if markers::is_opaque(&dir) {
                    lower_visible = false;
                }
            }
            // A file in the upper layer replaces a lower directory.
            Ok(_) => return None,
            Err(_) => {}
        }
    }

    Some(lower_visible)
}

fn read_entries(dir: &Path) -> std::io::Result<Vec<InstallDirEntry>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        out.push(InstallDirEntry { name: entry.file_name(), path: entry.path() });
    }

    Ok(out)
}

fn check_relative(rel: &Path) -> std::io::Result<()> {
    let plain =
        !rel.as_os_str().is_empty() && rel.components().all(|component| matches!(component, Component::Normal(_)));
    if plain {
        return Ok(());
    }

    Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, RelativePathError { path: rel.to_path_buf() }))
}

fn canonical_root(install_root: &Path) -> Result<PathBuf, ViewError> {
    std::fs::canonicalize(install_root)
        .map_err(|source| ViewError::InstallMissing { path: install_root.to_path_buf(), source })
}

fn read_backend_record(record_path: &Path) -> Result<Option<OverlayBackend>, ViewError> {
    match std::fs::read_to_string(record_path) {
        Ok(text) => OverlayBackend::from_record(&text)
            .map(Some)
            .ok_or_else(|| ViewError::UnknownBackendRecord { path: record_path.to_path_buf(), content: text }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ViewError::StateDir { path: record_path.to_path_buf(), source }),
    }
}

/// The first candidate whose probe mounts, recorded only once its probe succeeded.
fn choose_backend(
    state: &InstallStateDir,
    candidates: impl IntoIterator<Item = OverlayTools>,
    mut probe: impl FnMut(&OverlayTools) -> bool,
) -> Result<OverlayTools, ViewError> {
    let tools = candidates.into_iter().find(|tools| probe(tools)).ok_or(ViewError::NoOverlayBackend)?;

    write_backend_record(state, tools.backend())
        .map_err(|source| ViewError::StateDir { path: state.backend_record(), source })?;

    Ok(tools)
}

/// Replaces the record through a synced temporary file, so a crash leaves the old record or the
/// new one and never a partial one that would orphan the upper directory.
fn write_backend_record(state: &InstallStateDir, backend: OverlayBackend) -> std::io::Result<()> {
    let mut temporary = tempfile::Builder::new().prefix(".overlay-backend-").tempfile_in(state.dir())?;
    temporary.write_all(backend.record().as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(state.backend_record()).map_err(|err| err.error)?;

    // The rename is durable once the directory is synced.
    std::fs::File::open(state.dir())?.sync_all()
}

fn read_delta_record(record_path: &Path) -> Result<Option<PathBuf>, ViewError> {
    let bytes = match std::fs::read(record_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ViewError::StateDir { path: record_path.to_path_buf(), source }),
    };

    let delta = bytes_to_path(bytes)
        .filter(|delta| delta.is_absolute())
        .ok_or_else(|| ViewError::UnreadableDeltaRecord { path: record_path.to_path_buf() })?;

    Ok(Some(delta))
}

#[cfg(unix)]
fn path_to_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_to_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn bytes_to_path(bytes: Vec<u8>) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    Some(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: Vec<u8>) -> Option<PathBuf> {
    String::from_utf8(bytes).ok().map(PathBuf::from)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn candidates() -> [OverlayTools; 2] {
        [
            OverlayTools::KernelBwrap { bwrap: PathBuf::from("/usr/bin/bwrap") },
            OverlayTools::FuseOverlayfs {
                fuse_overlayfs: PathBuf::from("/usr/bin/fuse-overlayfs"),
                fusermount: PathBuf::from("/usr/bin/fusermount3"),
            },
        ]
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn char_whiteout(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        rustix::fs::mknodat(
            rustix::fs::CWD,
            path,
            rustix::fs::FileType::CharacterDevice,
            rustix::fs::Mode::empty(),
            rustix::fs::makedev(0, 0),
        )
        .unwrap();
    }

    /// The layouts the kernel and fuse-overlayfs leave after the game
    /// deletes, replaces and recreates files.
    fn layered() -> (tempfile::TempDir, InstallFiles) {
        let tmp = tempfile::tempdir().unwrap();
        let lower = tmp.path().join("lower");
        let upper = tmp.path().join("upper");
        for rel in [
            "preferences.xml",
            "user_preferences.xml",
            "replays/a.wowsreplay",
            "replays/b.wowsreplay",
            "replays/tempArenaInfo.json",
            "profile/python.log",
            "crashes/old.dmp",
            "res_mods/engine_config.xml",
        ] {
            write(&lower.join(rel), "lower");
        }
        std::fs::create_dir_all(&upper).unwrap();

        write(&upper.join("preferences.xml"), "upper");
        char_whiteout(&upper.join("replays/a.wowsreplay"));
        write(&upper.join("replays/.wh.b.wowsreplay"), "");
        write(&upper.join("replays/c.wowsreplay"), "upper");
        char_whiteout(&upper.join("crashes"));
        write(&upper.join("profile/new.log"), "upper");
        rustix::fs::setxattr(upper.join("profile"), "user.overlay.opaque", b"y", rustix::fs::XattrFlags::empty())
            .unwrap();

        (tmp, InstallFiles::Overlay { lower, delta: None, upper, backend: OverlayBackend::KernelBwrap })
    }

    fn layer_of(tmp: &Path, path: Option<PathBuf>) -> String {
        match path {
            Some(path) => path.strip_prefix(tmp).unwrap().display().to_string(),
            None => "absent".to_owned(),
        }
    }

    #[test]
    fn overlay_reads_resolve_through_whiteouts_and_opaque_dirs() {
        let (tmp, files) = layered();
        let resolved: Vec<(String, String)> = [
            "preferences.xml",
            "user_preferences.xml",
            "replays/a.wowsreplay",
            "replays/b.wowsreplay",
            "replays/c.wowsreplay",
            "replays/tempArenaInfo.json",
            "crashes/old.dmp",
            "profile/python.log",
            "profile/new.log",
            "res_mods/engine_config.xml",
        ]
        .into_iter()
        .map(|rel| (rel.to_owned(), layer_of(tmp.path(), files.resolve_read(Path::new(rel)).unwrap())))
        .collect();

        insta::assert_yaml_snapshot!(resolved);
    }

    #[test]
    fn overlay_listing_merges_layers() {
        let (tmp, files) = layered();
        let listing: Vec<(String, String)> = ["replays", "profile", "crashes"]
            .into_iter()
            .flat_map(|dir| files.list_dir(Path::new(dir)).unwrap())
            .map(|entry| (entry.name.to_string_lossy().into_owned(), layer_of(tmp.path(), Some(entry.path))))
            .collect();

        insta::assert_yaml_snapshot!(listing);
    }

    /// An install with files replaced, added and hidden by a delta layer,
    /// and game writes above it.
    fn with_delta() -> (tempfile::TempDir, InstallFiles) {
        let tmp = tempfile::tempdir().unwrap();
        let lower = tmp.path().join("lower");
        let delta = tmp.path().join("delta");
        let upper = tmp.path().join("upper");
        for rel in ["platform64.dll", "steam_api64.dll", "bin/1/bin64/platform64.dll", "bin/1/bin64/steam_api64.dll"] {
            write(&lower.join(rel), "lower");
        }
        for rel in ["platform64.dll", "wgc_api.dll", "game_info.xml", "bin/1/bin64/platform64.dll"] {
            write(&delta.join(rel), "delta");
        }
        char_whiteout(&delta.join("steam_api64.dll"));
        char_whiteout(&delta.join("bin/1/bin64/steam_api64.dll"));
        char_whiteout(&delta.join("currentrealm.txt"));
        write(&upper.join("currentrealm.txt"), "upper");
        char_whiteout(&upper.join("wgc_api.dll"));

        (tmp, InstallFiles::Overlay { lower, delta: Some(delta), upper, backend: OverlayBackend::FuseOverlayfs })
    }

    #[test]
    fn a_delta_layer_replaces_adds_and_hides_install_files() {
        let (tmp, files) = with_delta();
        let resolved: Vec<(String, String)> = [
            "platform64.dll",
            "steam_api64.dll",
            "wgc_api.dll",
            "game_info.xml",
            "currentrealm.txt",
            "bin/1/bin64/platform64.dll",
            "bin/1/bin64/steam_api64.dll",
        ]
        .into_iter()
        .map(|rel| (rel.to_owned(), layer_of(tmp.path(), files.resolve_read(Path::new(rel)).unwrap())))
        .collect();
        let listing: Vec<(String, String)> = files
            .list_dir(Path::new("bin/1/bin64"))
            .unwrap()
            .into_iter()
            .map(|entry| (entry.name.to_string_lossy().into_owned(), layer_of(tmp.path(), Some(entry.path))))
            .collect();

        insta::assert_yaml_snapshot!((resolved, listing));
    }

    #[test]
    fn a_selected_delta_gets_its_own_upper() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("install");
        std::fs::create_dir_all(&install).unwrap();
        let dirs = RuntimeDirs::new(&tmp.path().join("app"));
        let state = dirs.install_state(&std::fs::canonicalize(&install).unwrap());
        write(&state.with_delta().backend_record(), "fuse-overlayfs");
        let delta = tmp.path().join("delta");

        assert_eq!(InstallFiles::existing(&install, &dirs).unwrap(), InstallFiles::Direct { root: install.clone() });

        InstallView::select_delta(&install, &dirs, Some(&delta)).unwrap();
        assert_eq!(
            InstallFiles::existing(&install, &dirs).unwrap(),
            InstallFiles::Overlay {
                lower: install.clone(),
                delta: Some(delta),
                upper: state.with_delta().upper(),
                backend: OverlayBackend::FuseOverlayfs,
            }
        );

        InstallView::select_delta(&install, &dirs, None).unwrap();
        assert!(matches!(InstallFiles::existing(&install, &dirs).unwrap(), InstallFiles::Direct { .. }));
    }

    /// An install named through a link is read under the link, and its overlay state is found
    /// under the resolved name, so paths stored before the first overlay launch keep matching.
    #[test]
    fn an_install_named_through_a_link_keeps_the_name() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        std::fs::create_dir_all(real.join("install")).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let install = link.join("install");
        let dirs = RuntimeDirs::new(&tmp.path().join("app"));
        let state = dirs.install_state(&std::fs::canonicalize(&install).unwrap());
        write(&state.backend_record(), "kernel-bwrap");

        assert_eq!(
            InstallFiles::existing(&install, &dirs).unwrap(),
            InstallFiles::Overlay {
                lower: install.clone(),
                delta: None,
                upper: state.upper(),
                backend: OverlayBackend::KernelBwrap,
            }
        );
    }

    #[test]
    fn overlay_write_goes_to_upper_and_replaces_deleted_entries() {
        let (tmp, files) = layered();

        let target = files.resolve_write(Path::new("replays/a.wowsreplay")).unwrap();
        std::fs::write(&target, "new").unwrap();
        assert_eq!(files.resolve_read(Path::new("replays/a.wowsreplay")).unwrap(), Some(target));

        let target = files.resolve_write(Path::new("replays/b.wowsreplay")).unwrap();
        std::fs::write(&target, "new").unwrap();
        assert_eq!(files.resolve_read(Path::new("replays/b.wowsreplay")).unwrap(), Some(target));

        // A recreated deleted directory must not bring back the lower files.
        let target = files.resolve_write(Path::new("crashes/new.dmp")).unwrap();
        std::fs::write(&target, "new").unwrap();
        assert_eq!(files.resolve_read(Path::new("crashes/old.dmp")).unwrap(), None);
        assert!(target.starts_with(tmp.path().join("upper")));

        let target = files.resolve_write(Path::new("bin/1/res_mods/engine_config.xml")).unwrap();
        assert_eq!(target, tmp.path().join("upper/bin/1/res_mods/engine_config.xml"));
        assert!(target.parent().unwrap().is_dir());
    }

    #[test]
    fn rejects_paths_that_escape_the_install() {
        let files = InstallFiles::Direct { root: PathBuf::from("/games/wows") };
        for rel in ["../x", "/etc/passwd", "", "a/../b"] {
            assert_eq!(files.resolve_read(Path::new(rel)).unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn writable_install_is_used_directly_and_read_only_is_detected() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(probe_writable(tmp.path()).unwrap(), Writability::Writable);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);

        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let read_only = probe_writable(tmp.path()).unwrap();
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        // Root ignores directory permissions.
        if !rustix::process::geteuid().is_root() {
            assert_eq!(read_only, Writability::ReadOnly);
        }
    }

    #[test]
    fn backend_is_recorded_only_after_its_probe_mounts() {
        let tmp = tempfile::tempdir().unwrap();
        let state = RuntimeDirs::new(tmp.path()).install_state(tmp.path());
        std::fs::create_dir_all(state.dir()).unwrap();

        let mut probed = Vec::new();
        let tools = choose_backend(&state, candidates(), |tools| {
            probed.push((tools.backend(), read_backend_record(&state.backend_record()).unwrap()));
            tools.backend() == OverlayBackend::FuseOverlayfs
        })
        .unwrap();

        assert_eq!(probed, [(OverlayBackend::KernelBwrap, None), (OverlayBackend::FuseOverlayfs, None)]);
        assert_eq!(tools.backend(), OverlayBackend::FuseOverlayfs);
        assert_eq!(read_backend_record(&state.backend_record()).unwrap(), Some(OverlayBackend::FuseOverlayfs));
    }

    #[test]
    fn no_backend_is_recorded_when_every_probe_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let state = RuntimeDirs::new(tmp.path()).install_state(tmp.path());
        std::fs::create_dir_all(state.dir()).unwrap();

        let chosen = choose_backend(&state, candidates(), |_| false);

        assert!(matches!(chosen, Err(ViewError::NoOverlayBackend)));
        assert_eq!(read_backend_record(&state.backend_record()).unwrap(), None);
    }
}
