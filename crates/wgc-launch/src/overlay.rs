//! Writable overlays over read-only installs: a kernel overlayfs inside a
//! bwrap mount namespace, or fuse-overlayfs on the host.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use crate::error::MountError;
use crate::host::HostSearch;

/// Which overlay implementation owns an install's upper directory. Their
/// whiteout and opaque-directory formats differ, so an upper directory stays
/// with the backend that created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayBackend {
    /// `bwrap --overlay`: lives in the game's mount namespace, so it needs no
    /// cleanup and vanishes with the last game process.
    KernelBwrap,
    /// A host mount that the launcher unmounts after the session ends.
    FuseOverlayfs,
}

impl OverlayBackend {
    pub(crate) fn record(self) -> &'static str {
        match self {
            Self::KernelBwrap => "kernel-bwrap",
            Self::FuseOverlayfs => "fuse-overlayfs",
        }
    }

    pub(crate) fn from_record(text: &str) -> Option<Self> {
        match text.trim() {
            "kernel-bwrap" => Some(Self::KernelBwrap),
            "fuse-overlayfs" => Some(Self::FuseOverlayfs),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayLayers {
    /// The read-only install.
    pub lower: PathBuf,
    /// Read-only files laid over the install, below the upper: replacements,
    /// additions and 0/0 character-device whiteouts that hide install files.
    pub delta: Option<PathBuf>,
    /// Game writes, kept across launches.
    pub upper: PathBuf,
    pub work: PathBuf,
    /// The merged view's mount point. Populated only while a game runs, and
    /// for `KernelBwrap` only inside the game's namespace.
    pub merged: PathBuf,
}

/// Host tools a backend runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverlayTools {
    KernelBwrap { bwrap: PathBuf },
    FuseOverlayfs { fuse_overlayfs: PathBuf, fusermount: PathBuf },
}

impl OverlayTools {
    pub(crate) fn backend(&self) -> OverlayBackend {
        match self {
            Self::KernelBwrap { .. } => OverlayBackend::KernelBwrap,
            Self::FuseOverlayfs { .. } => OverlayBackend::FuseOverlayfs,
        }
    }

    pub(crate) fn find(backend: OverlayBackend, search: &HostSearch) -> Option<Self> {
        match backend {
            OverlayBackend::KernelBwrap => {
                search.find_on_path(OsStr::new("bwrap")).map(|bwrap| Self::KernelBwrap { bwrap })
            }
            OverlayBackend::FuseOverlayfs => {
                if !Path::new("/dev/fuse").exists() {
                    return None;
                }
                let fuse_overlayfs = search.find_on_path(OsStr::new("fuse-overlayfs"))?;
                let fusermount = search
                    .find_on_path(OsStr::new("fusermount3"))
                    .or_else(|| search.find_on_path(OsStr::new("fusermount")))?;
                Some(Self::FuseOverlayfs { fuse_overlayfs, fusermount })
            }
        }
    }

    /// Whether this backend can mount `layers` here. For bwrap this mounts
    /// the real layers in a throwaway namespace, which also proves that the
    /// kernel accepts the lower filesystem and unprivileged overlay mounts.
    /// fuse-overlayfs mounts and unmounts the real lower with throwaway upper,
    /// work and merged directories under `scratch_parent`.
    pub(crate) fn probe(&self, layers: &OverlayLayers, scratch_parent: &Path) -> bool {
        match self {
            Self::KernelBwrap { bwrap } => {
                let mut argv = bwrap_prefix(bwrap, layers);
                argv.push(OsString::from("true"));
                let (program, args) = argv.split_first().expect("bwrap argv has a program");
                Command::new(program)
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|status| status.success())
            }
            Self::FuseOverlayfs { fuse_overlayfs, fusermount } => {
                match probe_fuse_mount(
                    &layers.lower,
                    layers.delta.as_deref(),
                    scratch_parent,
                    fuse_overlayfs,
                    fusermount,
                ) {
                    Ok(()) => true,
                    Err(err) => {
                        tracing::info!("fuse-overlayfs probe failed: {err}");
                        false
                    }
                }
            }
        }
    }
}

/// The argv that runs a command inside a mount namespace with `layers`
/// mounted at `layers.merged`; the command follows.
pub(crate) fn bwrap_prefix(bwrap: &Path, layers: &OverlayLayers) -> Vec<OsString> {
    let mut argv = vec![bwrap.as_os_str().to_owned(), "--dev-bind".into(), "/".into(), "/".into()];

    // The first source is the bottom layer.
    for source in std::iter::once(&layers.lower).chain(&layers.delta) {
        argv.push("--overlay-src".into());
        argv.push(source.as_os_str().to_owned());
    }

    argv.extend([
        "--overlay".into(),
        layers.upper.as_os_str().to_owned(),
        layers.work.as_os_str().to_owned(),
        layers.merged.as_os_str().to_owned(),
        "--chdir".into(),
        layers.merged.as_os_str().to_owned(),
        "--".into(),
    ]);

    argv
}

/// A mounted fuse-overlayfs. Not unmounted on drop: a game that outlives
/// the toolkit keeps its files, and the next mount clears a stale one.
#[derive(Debug)]
pub(crate) struct FuseMount {
    merged: PathBuf,
    fusermount: PathBuf,
}

impl FuseMount {
    pub(crate) fn mount(layers: &OverlayLayers, fuse_overlayfs: &Path, fusermount: &Path) -> Result<Self, MountError> {
        let mount = Self { merged: layers.merged.clone(), fusermount: fusermount.to_path_buf() };
        if is_mountpoint(&layers.merged).map_err(MountError::MountTable)? {
            tracing::info!("clearing stale overlay mount at {}", layers.merged.display());
            mount.unmount()?;
        }

        let options = fuse_overlayfs_options(layers)?;

        // fuse-overlayfs forks into the background once the mount is ready.
        let mut command = Command::new(fuse_overlayfs);
        command.arg("-o").arg(options).arg(&layers.merged);
        run_tool(fuse_overlayfs, command)?;

        Ok(mount)
    }

    pub(crate) fn unmount(&self) -> Result<(), MountError> {
        let mut command = Command::new(&self.fusermount);
        command.arg("-u").arg(&self.merged);
        if run_tool(&self.fusermount, command).is_ok() {
            return Ok(());
        }

        // A process outside the session may still hold a file open; detach lazily.
        let mut command = Command::new(&self.fusermount);
        command.arg("-u").arg("-z").arg(&self.merged);
        run_tool(&self.fusermount, command)
    }
}

/// The upper and work directories must share a filesystem, so all three
/// throwaway directories live under `scratch_parent`.
fn probe_fuse_mount(
    lower: &Path,
    delta: Option<&Path>,
    scratch_parent: &Path,
    fuse_overlayfs: &Path,
    fusermount: &Path,
) -> Result<(), MountError> {
    let scratch = tempfile::Builder::new()
        .prefix(".overlay-probe-")
        .tempdir_in(scratch_parent)
        .map_err(|source| MountError::ProbeDir { path: scratch_parent.to_path_buf(), source })?;
    let layers = OverlayLayers {
        lower: lower.to_path_buf(),
        delta: delta.map(Path::to_path_buf),
        upper: scratch.path().join("upper"),
        work: scratch.path().join("work"),
        merged: scratch.path().join("merged"),
    };
    for dir in [&layers.upper, &layers.work, &layers.merged] {
        std::fs::create_dir(dir).map_err(|source| MountError::ProbeDir { path: dir.clone(), source })?;
    }

    FuseMount::mount(&layers, fuse_overlayfs, fusermount)?.unmount()
}

fn run_tool(program: &Path, mut command: Command) -> Result<(), MountError> {
    command.env_clear().env("PATH", "/usr/local/bin:/usr/bin:/bin").stdin(Stdio::null());
    let output = command.output().map_err(|source| MountError::Spawn { program: program.to_path_buf(), source })?;
    if !output.status.success() {
        return Err(MountError::Failed {
            program: program.to_path_buf(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(())
}

/// The `-o` argument that hands `layers` to fuse-overlayfs. libfuse splits it at unescaped commas
/// and drops one level of backslashes. fuse-overlayfs then splits `lowerdir` and `upperdir` at
/// unescaped colons and drops a second level, but drops every backslash from `workdir`.
fn fuse_overlayfs_options(layers: &OverlayLayers) -> Result<OsString, MountError> {
    let work = layers.work.as_os_str().as_encoded_bytes();
    if work.contains(&b'\\') {
        return Err(MountError::WorkDirBackslash { path: layers.work.clone() });
    }

    let mut options = Vec::new();
    // fuse-overlayfs lists lower layers top first, split at unescaped colons.
    let mut lower = Vec::new();
    if let Some(delta) = &layers.delta {
        lower.extend(escape_layer_path(delta));
        lower.push(b':');
    }
    lower.extend(escape_layer_path(&layers.lower));
    push_fuse_option(&mut options, "lowerdir", &lower);
    push_fuse_option(&mut options, "upperdir", &escape_layer_path(&layers.upper));
    push_fuse_option(&mut options, "workdir", work);

    Ok(bytes_to_path(options).into_os_string())
}

fn escape_layer_path(path: &Path) -> Vec<u8> {
    let mut escaped = Vec::new();
    for &byte in path.as_os_str().as_encoded_bytes() {
        if matches!(byte, b'\\' | b':') {
            escaped.push(b'\\');
        }
        escaped.push(byte);
    }

    escaped
}

fn push_fuse_option(options: &mut Vec<u8>, key: &str, value: &[u8]) {
    if !options.is_empty() {
        options.push(b',');
    }
    options.extend_from_slice(key.as_bytes());
    options.push(b'=');
    for &byte in value {
        if matches!(byte, b'\\' | b',') {
            options.push(b'\\');
        }
        options.push(byte);
    }
}

/// Whether `path` is a mount point. The mount table names mounts by their resolved paths, so
/// `path` must be canonical.
fn is_mountpoint(path: &Path) -> std::io::Result<bool> {
    let table = std::fs::read_to_string("/proc/self/mountinfo")?;
    Ok(mount_points(&table).any(|mount_point| mount_point == path))
}

/// Mount points in `/proc/self/mountinfo`, with the kernel's octal escapes decoded.
fn mount_points(table: &str) -> impl Iterator<Item = PathBuf> + '_ {
    table.lines().filter_map(|line| line.split(' ').nth(4)).map(unescape_mount_point)
}

fn unescape_mount_point(field: &str) -> PathBuf {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        let escape = bytes
            .get(idx + 1..idx + 4)
            .filter(|digits| bytes[idx] == b'\\' && digits.iter().all(|d| (b'0'..=b'7').contains(d)));
        match escape {
            Some(digits) => {
                out.push(digits.iter().fold(0u8, |acc, digit| acc.wrapping_mul(8).wrapping_add(digit - b'0')));
                idx += 4;
            }
            None => {
                out.push(bytes[idx]);
                idx += 1;
            }
        }
    }

    bytes_to_path(out)
}

#[cfg(unix)]
fn bytes_to_path(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;

    PathBuf::from(OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// Whiteout and opaque-directory markers in an upper directory, in both
/// the kernel's and fuse-overlayfs's formats.
pub(crate) mod markers {
    use std::path::Path;

    const WHITEOUT_PREFIX: &str = ".wh.";
    const OPAQUE_FILE: &str = ".wh..wh..opq";

    /// A deleted lower entry: a 0/0 character device, or fuse-overlayfs's `.wh.<name>`.
    pub(crate) fn is_whiteout(path: &Path) -> bool {
        if is_char_whiteout(path) {
            return true;
        }

        match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => {
                let mut marker = std::ffi::OsString::from(WHITEOUT_PREFIX);
                marker.push(name);
                parent.join(marker).symlink_metadata().is_ok()
            }
            _ => false,
        }
    }

    pub(crate) fn is_marker_name(name: &std::ffi::OsStr) -> bool {
        name.as_encoded_bytes().starts_with(WHITEOUT_PREFIX.as_bytes())
    }

    /// The name a `.wh.<name>` marker hides, if `name` is one.
    pub(crate) fn hidden_name(name: &std::ffi::OsStr) -> Option<&std::ffi::OsStr> {
        if name == OPAQUE_FILE {
            return None;
        }

        name.to_str()?.strip_prefix(WHITEOUT_PREFIX).map(std::ffi::OsStr::new)
    }

    /// An upper directory that hides the lower directory of the same name.
    pub(crate) fn is_opaque(dir: &Path) -> bool {
        dir.join(OPAQUE_FILE).symlink_metadata().is_ok() || has_opaque_xattr(dir)
    }

    /// Removes any whiteout for `path` so a new entry can take its place.
    pub(crate) fn clear_whiteout(path: &Path) -> std::io::Result<()> {
        if is_char_whiteout(path) {
            std::fs::remove_file(path)?;
        }

        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            let mut marker = std::ffi::OsString::from(WHITEOUT_PREFIX);
            marker.push(name);
            match std::fs::remove_file(parent.join(marker)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    /// Whether `path` is a 0/0 character device, the whiteout [`create_whiteout`] makes.
    #[cfg(unix)]
    pub fn is_char_whiteout(path: &Path) -> bool {
        use std::os::unix::fs::FileTypeExt;
        use std::os::unix::fs::MetadataExt;

        path.symlink_metadata().is_ok_and(|metadata| metadata.file_type().is_char_device() && metadata.rdev() == 0)
    }

    #[cfg(not(unix))]
    pub fn is_char_whiteout(_path: &Path) -> bool {
        false
    }

    #[cfg(unix)]
    fn has_opaque_xattr(dir: &Path) -> bool {
        const NAMES: [&str; 3] = ["user.overlay.opaque", "trusted.overlay.opaque", "user.fuseoverlayfs.opaque"];

        NAMES.iter().any(|name| {
            let mut value = [0u8; 4];
            matches!(rustix::fs::lgetxattr(dir, *name, &mut value[..]), Ok(len) if value[..len] == *b"y")
        })
    }

    #[cfg(not(unix))]
    fn has_opaque_xattr(_dir: &Path) -> bool {
        false
    }

    /// Marks a directory the toolkit recreated in place of a whiteout, so
    /// the deleted lower contents stay hidden.
    #[cfg(unix)]
    pub(crate) fn mark_opaque(dir: &Path, backend: super::OverlayBackend) -> std::io::Result<()> {
        match backend {
            super::OverlayBackend::KernelBwrap => {
                rustix::fs::lsetxattr(dir, "user.overlay.opaque", b"y", rustix::fs::XattrFlags::empty())?;
            }
            super::OverlayBackend::FuseOverlayfs => {
                std::fs::write(dir.join(OPAQUE_FILE), b"")?;
                rustix::fs::lsetxattr(dir, "user.fuseoverlayfs.opaque", b"y", rustix::fs::XattrFlags::empty())?;
            }
        }

        Ok(())
    }

    #[cfg(not(unix))]
    pub(crate) fn mark_opaque(_dir: &Path, _backend: super::OverlayBackend) -> std::io::Result<()> {
        Ok(())
    }

    /// Hides `path` of the layers below the one holding it: a 0/0 character
    /// device, which the kernel and fuse-overlayfs both read as a whiteout
    /// and which needs no privilege to create. Only Linux overlays read
    /// whiteouts.
    #[cfg(target_os = "linux")]
    pub fn create_whiteout(path: &Path) -> std::io::Result<()> {
        rustix::fs::mknodat(
            rustix::fs::CWD,
            path,
            rustix::fs::FileType::CharacterDevice,
            rustix::fs::Mode::empty(),
            rustix::fs::makedev(0, 0),
        )?;

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn create_whiteout(_path: &Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mountinfo_mount_points_decode_escapes() {
        let table = "\
22 1 0:21 / / rw,relatime shared:1 - btrfs /dev/nvme0n1p3 rw
789 75 0:3 / /home/u/game\\040data/merged rw,nosuid shared:805 - fuse.fuse-overlayfs fuse-overlayfs rw
";
        let points: Vec<PathBuf> = mount_points(table).collect();
        insta::assert_debug_snapshot!(points);
    }

    #[test]
    fn fuse_overlayfs_options_escape_separators_in_paths() {
        let layers = OverlayLayers {
            lower: PathBuf::from(r"/games/a,b:c\d"),
            delta: None,
            upper: PathBuf::from("/state/up:per"),
            work: PathBuf::from("/state/wo,rk:dir"),
            merged: PathBuf::from("/state/merged"),
        };

        let options = fuse_overlayfs_options(&layers).unwrap();

        assert_eq!(
            options.to_str().unwrap(),
            r"lowerdir=/games/a\,b\\:c\\\\d,upperdir=/state/up\\:per,workdir=/state/wo\,rk:dir"
        );
    }

    #[test]
    fn fuse_overlayfs_cannot_take_a_work_dir_with_a_backslash() {
        let layers = OverlayLayers {
            lower: PathBuf::from("/games/wows"),
            delta: None,
            upper: PathBuf::from("/state/upper"),
            work: PathBuf::from(r"/state/wo\rk"),
            merged: PathBuf::from("/state/merged"),
        };

        assert!(matches!(fuse_overlayfs_options(&layers), Err(MountError::WorkDirBackslash { .. })));
    }

    #[test]
    fn fuse_overlayfs_stacks_the_delta_over_the_install() {
        let layers = OverlayLayers {
            lower: PathBuf::from("/steam/World of Warships"),
            delta: Some(PathBuf::from("/data/wgc-layer/15.10")),
            upper: PathBuf::from("/data/installs/wows/delta/upper"),
            work: PathBuf::from("/data/installs/wows/delta/work"),
            merged: PathBuf::from("/data/installs/wows/delta/merged"),
        };

        assert_eq!(
            fuse_overlayfs_options(&layers).unwrap().to_str().unwrap(),
            "lowerdir=/data/wgc-layer/15.10:/steam/World of Warships,upperdir=/data/installs/wows/delta/upper,workdir=/data/installs/wows/delta/work"
        );
    }
}
