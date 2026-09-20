//! Extracting VFS entries to disk.
//!
//! Mirrors the egui app's `extract_files`: queued directories expand to their
//! files, and each file is written under the output directory at its own VFS
//! path. Unlike that version, an IO failure is reported rather than panicking
//! the worker, so a read-only destination or a full disk surfaces in the UI.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use wowsunpack::vfs::VfsPath;

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("could not create the directory {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not create the file {path}")]
    CreateFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read {path} out of the game data")]
    ReadSource {
        path: String,
        #[source]
        source: wowsunpack::vfs::VfsError,
    },
    #[error("could not write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// How far an extraction has got. `written` counts files finished, so it
/// reaches `total` exactly when the run is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractProgress {
    pub written: usize,
    pub total: usize,
}

impl ExtractProgress {
    /// Fraction complete, for a progress bar. An empty queue reads as
    /// finished rather than dividing by zero.
    pub fn fraction(self) -> f32 {
        if self.total == 0 { 1.0 } else { self.written as f32 / self.total as f32 }
    }
}

/// Expands `queued` into the files it covers, walking directories.
///
/// An unreadable directory contributes nothing rather than failing the run:
/// the queue is a user selection, and one bad node should not cancel the rest.
pub fn expand_to_files(queued: &[VfsPath]) -> Vec<VfsPath> {
    let mut pending: Vec<VfsPath> = queued.to_vec();
    let mut files = Vec::new();
    while let Some(entry) = pending.pop() {
        if entry.is_file().unwrap_or(false) {
            files.push(entry);
        } else if let Ok(children) = entry.read_dir() {
            pending.extend(children);
        }
    }
    files
}

/// The on-disk path `file` extracts to: its VFS path, minus the leading
/// slash, rebased under `output_dir`.
pub fn destination(output_dir: &Path, file: &VfsPath) -> PathBuf {
    output_dir.join(file.as_str().trim_start_matches('/'))
}

/// The outcome of a run that did not fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractOutcome {
    /// Every queued file was written.
    Completed { written: usize },
    /// A stop was requested partway. `written` is what actually reached disk,
    /// which is what the UI must report rather than the queue length.
    Stopped { written: usize },
}

impl ExtractOutcome {
    pub fn written(self) -> usize {
        match self {
            Self::Completed { written } | Self::Stopped { written } => written,
        }
    }
}

/// Writes every file in `files` under `output_dir`, reporting progress to
/// `on_progress` after each one. Returns early on the first IO failure.
///
/// `should_stop` is polled before each file so a cancel takes effect without
/// waiting for the whole queue.
pub fn extract_files(
    files: &[VfsPath],
    output_dir: &Path,
    mut on_progress: impl FnMut(ExtractProgress),
    should_stop: impl Fn() -> bool,
) -> Result<ExtractOutcome, ExtractError> {
    let total = files.len();
    let mut created: HashSet<PathBuf> = HashSet::new();

    let mut written = 0usize;
    for file in files {
        if should_stop() {
            return Ok(ExtractOutcome::Stopped { written });
        }

        let destination = destination(output_dir, file);
        let parent = destination.parent().unwrap_or(output_dir).to_path_buf();
        if created.insert(parent.clone()) {
            fs::create_dir_all(&parent).map_err(|source| ExtractError::CreateDir { path: parent.clone(), source })?;
        }

        let mut reader =
            file.open_file().map_err(|source| ExtractError::ReadSource { path: file.as_str().to_string(), source })?;
        let mut writer = fs::File::create(&destination)
            .map_err(|source| ExtractError::CreateFile { path: destination.clone(), source })?;
        io::copy(&mut reader, &mut writer)
            .map_err(|source| ExtractError::Write { path: destination.clone(), source })?;

        written += 1;
        on_progress(ExtractProgress { written, total });
    }

    Ok(ExtractOutcome::Completed { written })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use wowsunpack::vfs::MemoryFS;

    fn fixture() -> VfsPath {
        let root: VfsPath = MemoryFS::new().into();
        root.join("res/content").unwrap().create_dir_all().unwrap();
        for (path, body) in [("res/content/a.xml", "<a/>"), ("res/content/b.txt", "bee"), ("res/top.txt", "top")] {
            root.join(path).unwrap().create_file().unwrap().write_all(body.as_bytes()).unwrap();
        }
        root
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wt-gpui-extract-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn expanding_a_directory_yields_every_file_beneath_it() {
        let root = fixture();
        let mut names: Vec<String> =
            expand_to_files(&[root.join("res").unwrap()]).iter().map(|f| f.as_str().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["/res/content/a.xml", "/res/content/b.txt", "/res/top.txt"]);
    }

    #[test]
    fn expanding_a_file_yields_just_that_file() {
        let root = fixture();
        let files = expand_to_files(&[root.join("res/top.txt").unwrap()]);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].as_str(), "/res/top.txt");
    }

    #[test]
    fn expanding_a_missing_entry_contributes_nothing_rather_than_failing() {
        let root = fixture();
        assert!(expand_to_files(&[root.join("res/nope").unwrap()]).is_empty());
    }

    #[test]
    fn destination_mirrors_the_vfs_path_under_the_output_directory() {
        let root = fixture();
        let file = root.join("res/content/a.xml").unwrap();
        assert_eq!(destination(Path::new("/out"), &file), PathBuf::from("/out/res/content/a.xml"));
    }

    #[test]
    fn extraction_writes_every_file_with_its_directory_structure_and_contents() {
        let root = fixture();
        let out = temp_dir("writes");
        let files = expand_to_files(&[root.join("res").unwrap()]);

        let mut seen = Vec::new();
        let outcome = extract_files(&files, &out, |progress| seen.push(progress), || false).unwrap();

        assert_eq!(outcome, ExtractOutcome::Completed { written: 3 });
        assert_eq!(fs::read_to_string(out.join("res/content/a.xml")).unwrap(), "<a/>");
        assert_eq!(fs::read_to_string(out.join("res/content/b.txt")).unwrap(), "bee");
        assert_eq!(fs::read_to_string(out.join("res/top.txt")).unwrap(), "top");
        assert_eq!(seen.last().copied(), Some(ExtractProgress { written: 3, total: 3 }));
        let _ = fs::remove_dir_all(&out);
    }

    #[test]
    fn progress_counts_finished_files_and_ends_at_the_total() {
        let root = fixture();
        let out = temp_dir("progress");
        let files = expand_to_files(&[root.join("res").unwrap()]);

        let mut seen = Vec::new();
        extract_files(&files, &out, |progress| seen.push(progress), || false).unwrap();

        assert_eq!(seen.iter().map(|p| p.written).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(seen.iter().all(|p| p.total == 3));
        let _ = fs::remove_dir_all(&out);
    }

    #[test]
    fn a_stop_request_ends_the_run_before_the_next_file() {
        let root = fixture();
        let out = temp_dir("stop");
        let files = expand_to_files(&[root.join("res").unwrap()]);
        let stop = AtomicBool::new(false);

        let mut reported = 0usize;
        let outcome = extract_files(
            &files,
            &out,
            |_| {
                reported += 1;
                stop.store(true, Ordering::Relaxed);
            },
            || stop.load(Ordering::Relaxed),
        )
        .unwrap();

        assert_eq!(reported, 1, "the stop is observed before the second file");
        assert_eq!(outcome, ExtractOutcome::Stopped { written: 1 }, "a cancel reports what reached disk");
        let _ = fs::remove_dir_all(&out);
    }

    #[test]
    fn an_empty_queue_reports_a_complete_fraction_rather_than_dividing_by_zero() {
        assert_eq!(ExtractProgress { written: 0, total: 0 }.fraction(), 1.0);
        assert_eq!(ExtractProgress { written: 1, total: 4 }.fraction(), 0.25);
    }
}
