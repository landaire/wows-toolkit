//! The per-launch folder and its size bounds.

use std::fs::File;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use crate::error::CaptureError;
use crate::game::LaunchId;

/// Launch folders kept, the new one included.
const KEEP_LAUNCHES: usize = 8;

/// A size limit in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ByteLimit(pub(super) u64);

impl ByteLimit {
    pub(super) const fn mib(count: u64) -> Self {
        Self(count * 1024 * 1024)
    }

    const fn kib(count: u64) -> Self {
        Self(count * 1024)
    }
}

/// Time since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct UnixTime(Duration);

impl UnixTime {
    /// The wall-clock time of `instant`.
    pub(super) fn of(instant: Instant) -> Result<Self, CaptureError> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|source| CaptureError::Clock { source })?;

        Ok(Self(now.saturating_sub(instant.elapsed())))
    }

    pub(super) fn millis(self) -> u128 {
        self.0.as_millis()
    }
}

/// Bytes every file of one launch folder may still take, shared by the capture threads.
#[derive(Debug, Clone)]
pub(super) struct LaunchBudget(Arc<AtomicU64>);

impl LaunchBudget {
    pub(super) fn new(limit: ByteLimit) -> Self {
        Self(Arc::new(AtomicU64::new(limit.0)))
    }

    /// Takes `len` bytes, or nothing when fewer remain.
    fn take(&self, len: u64) -> bool {
        self.0.try_update(Ordering::Relaxed, Ordering::Relaxed, |left| left.checked_sub(len)).is_ok()
    }
}

/// The files of a launch folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CaptureFile {
    /// Times, processes and the capture's own warnings.
    Capture,
    Stacks,
    Syscalls,
    Sockets,
    Dns,
    /// The client's memory map, to place the stack addresses of Windows modules.
    Maps,
    PythonLog,
    DebugLog,
}

impl CaptureFile {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Capture => "capture.txt",
            Self::Stacks => "stacks.txt",
            Self::Syscalls => "syscalls.tsv",
            Self::Sockets => "sockets.tsv",
            Self::Dns => "dns.tsv",
            Self::Maps => "maps.txt",
            Self::PythonLog => "python.log",
            Self::DebugLog => "debug.log",
        }
    }

    pub(super) fn limit(self) -> ByteLimit {
        match self {
            Self::Capture => ByteLimit::kib(256),
            Self::Stacks => ByteLimit::mib(8),
            Self::Syscalls => ByteLimit::mib(4),
            Self::Sockets => ByteLimit::mib(2),
            Self::Dns => ByteLimit::kib(512),
            Self::Maps | Self::PythonLog | Self::DebugLog => ByteLimit::mib(1),
        }
    }
}

/// A sink that takes whole records while both its own limit and the launch budget allow, then
/// notes once that it dropped the rest.
#[derive(Debug)]
pub(super) struct BoundedWriter<W> {
    inner: W,
    file_left: u64,
    launch: LaunchBudget,
    truncated: bool,
}

impl<W> BoundedWriter<W>
where
    W: Write,
{
    pub(super) fn new(inner: W, file: ByteLimit, launch: LaunchBudget) -> Self {
        Self { inner, file_left: file.0, launch, truncated: false }
    }

    pub(super) fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        const TRUNCATED: &[u8] = b"[size limit reached; later records dropped]\n";

        if self.truncated {
            return Ok(());
        }

        let len = bytes.len() as u64;
        if len <= self.file_left && self.launch.take(len) {
            self.file_left -= len;
            return self.inner.write_all(bytes);
        }

        self.truncated = true;
        self.inner.write_all(TRUNCATED)
    }
}

/// An open file of a launch folder.
#[derive(Debug)]
pub(super) struct CaptureWriter {
    path: PathBuf,
    writer: BoundedWriter<File>,
}

impl CaptureWriter {
    pub(super) fn record(&mut self, bytes: &[u8]) -> Result<(), CaptureError> {
        self.writer.record(bytes).map_err(|source| CaptureError::Write { path: self.path.clone(), source })
    }
}

/// `<root>/<unix seconds>-<launch>`, one per captured launch.
#[derive(Debug)]
pub(super) struct LaunchFolder {
    path: PathBuf,
}

impl LaunchFolder {
    /// Creates the folder for `launch`, first removing the oldest ones past the kept count.
    pub(super) fn create(root: &Path, launch: LaunchId, started: UnixTime) -> Result<Self, CaptureError> {
        prune(root);

        let name = FolderName { started_secs: started.0.as_secs(), launch: launch.number() };
        let path = root.join(name.to_string());
        std::fs::create_dir_all(&path).map_err(|source| CaptureError::CreateFolder { path: path.clone(), source })?;

        Ok(Self { path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn open(&self, file: CaptureFile, budget: &LaunchBudget) -> Result<CaptureWriter, CaptureError> {
        let path = self.path.join(file.name());
        let created = File::create(&path).map_err(|source| CaptureError::CreateFile { path: path.clone(), source })?;

        Ok(CaptureWriter { path, writer: BoundedWriter::new(created, file.limit(), budget.clone()) })
    }
}

/// The name of a launch folder. Ordered by start time, then launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FolderName {
    started_secs: u64,
    launch: u64,
}

impl FolderName {
    fn parse(name: &str) -> Option<Self> {
        let (started, launch) = name.split_once('-')?;

        Some(Self { started_secs: started.parse().ok()?, launch: launch.parse().ok()? })
    }
}

impl std::fmt::Display for FolderName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.started_secs, self.launch)
    }
}

/// Removes the oldest launch folders so the new one keeps the count at `KEEP_LAUNCHES`.
fn prune(root: &Path) {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        // The first capture creates the root.
        Err(err) if err.kind() == io::ErrorKind::NotFound => return,
        Err(err) => {
            tracing::warn!("could not list {}: {err}", root.display());
            return;
        }
    };
    let names: Vec<String> = entries.flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect();

    for name in stale_folders(names.iter().map(String::as_str), KEEP_LAUNCHES - 1) {
        let path = root.join(name);
        if let Err(err) = std::fs::remove_dir_all(&path) {
            tracing::warn!("could not remove {}: {err}", path.display());
        }
    }
}

/// The launch folders among `names` past the newest `keep`. Other names are never selected.
fn stale_folders<'a>(names: impl Iterator<Item = &'a str>, keep: usize) -> Vec<&'a str> {
    let mut folders: Vec<(FolderName, &str)> =
        names.filter_map(|name| FolderName::parse(name).map(|parsed| (parsed, name))).collect();
    folders.sort_unstable_by_key(|(parsed, _)| std::cmp::Reverse(*parsed));

    folders.into_iter().skip(keep).map(|(_, name)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_drops_records_past_its_own_limit() {
        let mut writer = BoundedWriter::new(Vec::new(), ByteLimit(8), LaunchBudget::new(ByteLimit(100)));

        writer.record(b"abcd\n").unwrap();
        writer.record(b"efgh\n").unwrap();
        writer.record(b"ij\n").unwrap();

        insta::assert_snapshot!(String::from_utf8(writer.inner).unwrap(), @r"
        abcd
        [size limit reached; later records dropped]
        ");
    }

    #[test]
    fn writers_share_the_launch_budget() {
        let budget = LaunchBudget::new(ByteLimit(10));
        let mut first = BoundedWriter::new(Vec::new(), ByteLimit(100), budget.clone());
        let mut second = BoundedWriter::new(Vec::new(), ByteLimit(100), budget);

        first.record(b"123456\n").unwrap();
        second.record(b"ab\n").unwrap();
        second.record(b"cd\n").unwrap();
        first.record(b"7\n").unwrap();

        insta::assert_snapshot!(String::from_utf8(first.inner).unwrap(), @r"
        123456
        [size limit reached; later records dropped]
        ");
        insta::assert_snapshot!(String::from_utf8(second.inner).unwrap(), @r"
        ab
        [size limit reached; later records dropped]
        ");
    }

    #[test]
    fn pruning_selects_the_oldest_launch_folders_only() {
        let names = ["1794780376-2", "notes", "1794780376-10", "1794700000-1", "1794790000-1", "12-x", "1794780376"];

        let stale = stale_folders(names.into_iter(), 2);

        insta::assert_debug_snapshot!(stale, @r#"
        [
            "1794780376-2",
            "1794700000-1",
        ]
        "#);
    }
}
