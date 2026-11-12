//! Extracting VFS entries to disk.
//!
//! Mirrors the egui app's `extract_files`: queued directories expand to their
//! files, and each file is written under the output directory at its own VFS
//! path. Unlike that version, an IO failure is reported rather than panicking
//! the worker, so a read-only destination or a full disk surfaces in the UI.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::io::Read as _;
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractProgress {
    pub written: usize,
    pub total: usize,
    /// The file that was just written, which is what the egui progress bar
    /// carries as its text.
    pub file: String,
}

impl ExtractProgress {
    /// Fraction complete, for a progress bar. An empty queue reads as
    /// finished rather than dividing by zero.
    pub fn fraction(&self) -> f32 {
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

/// This file decoded to JSON, when it is a prototype the decoder handles and
/// the decode succeeds. `None` for every other file, which is then written
/// as it is stored.
fn decode_prototype(file: &VfsPath) -> Option<String> {
    let prototype = super::viewer::decodable_prototype(&file.filename())?;
    let mut bytes = Vec::new();
    file.open_file().and_then(|mut reader| Ok(reader.read_to_end(&mut bytes)?)).ok()?;
    wowsunpack::models::decode_prototype_to_json(&bytes, prototype).ok()
}

/// The game's resource directory. A VFS path is relative to it, so an
/// extraction writes under it and the output mirrors an install's own layout.
pub const EXTRACT_ROOT: &str = "res";

/// Where an extraction writes, given the directory the user chose.
pub fn extract_root(output_dir: &Path) -> PathBuf {
    output_dir.join(EXTRACT_ROOT)
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

/// Whether an extraction rewrites decodable assets.bin prototypes as JSON.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrototypeOutput {
    /// Write every file exactly as it is stored.
    #[default]
    Raw,
    /// Write a decodable prototype as `<name>.json`, and every other file
    /// raw. A prototype the decoder rejects is written raw rather than
    /// skipped, so the extraction never silently loses a file.
    DecodeToJson,
}

/// Writes every file in `files` under `output_dir`, reporting progress to
/// `on_progress` after each one. Returns early on the first IO failure.
///
/// `should_stop` is polled before each file so a cancel takes effect without
/// waiting for the whole queue.
pub fn extract_files(
    files: &[VfsPath],
    output_dir: &Path,
    prototypes: PrototypeOutput,
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

        if prototypes == PrototypeOutput::DecodeToJson
            && let Some(json) = decode_prototype(file)
        {
            let json_path = destination.with_file_name(format!("{}.json", file.filename()));
            fs::write(&json_path, json).map_err(|source| ExtractError::Write { path: json_path.clone(), source })?;
            written += 1;
            on_progress(ExtractProgress { written, total, file: json_path.display().to_string() });
            continue;
        }

        let mut reader =
            file.open_file().map_err(|source| ExtractError::ReadSource { path: file.as_str().to_string(), source })?;
        let mut writer = fs::File::create(&destination)
            .map_err(|source| ExtractError::CreateFile { path: destination.clone(), source })?;
        io::copy(&mut reader, &mut writer)
            .map_err(|source| ExtractError::Write { path: destination.clone(), source })?;

        written += 1;
        on_progress(ExtractProgress { written, total, file: destination.display().to_string() });
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
    fn an_extraction_writes_under_the_chosen_directorys_res_folder() {
        assert_eq!(extract_root(Path::new("/out")), PathBuf::from("/out/res"));
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
        let outcome =
            extract_files(&files, &out, PrototypeOutput::Raw, |progress| seen.push(progress), || false).unwrap();

        assert_eq!(outcome, ExtractOutcome::Completed { written: 3 });
        assert_eq!(fs::read_to_string(out.join("res/content/a.xml")).unwrap(), "<a/>");
        assert_eq!(fs::read_to_string(out.join("res/content/b.txt")).unwrap(), "bee");
        assert_eq!(fs::read_to_string(out.join("res/top.txt")).unwrap(), "top");
        let last = seen.last().expect("progress was reported");
        assert_eq!((last.written, last.total), (3, 3));
        assert!(
            seen.iter().all(|report| !report.file.is_empty()),
            "every report names the file it wrote, got {:?}",
            seen.iter().map(|report| report.file.as_str()).collect::<Vec<_>>()
        );
        let _ = fs::remove_dir_all(&out);
    }

    #[test]
    fn progress_counts_finished_files_and_ends_at_the_total() {
        let root = fixture();
        let out = temp_dir("progress");
        let files = expand_to_files(&[root.join("res").unwrap()]);

        let mut seen = Vec::new();
        extract_files(&files, &out, PrototypeOutput::Raw, |progress| seen.push(progress), || false).unwrap();

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
            PrototypeOutput::Raw,
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
    fn asking_for_json_still_writes_an_ordinary_file_as_it_is_stored() {
        let root = fixture();
        let out = temp_dir("decode-passthrough");
        let files = expand_to_files(&[root.join("res/top.txt").unwrap()]);

        let outcome = extract_files(&files, &out, PrototypeOutput::DecodeToJson, |_| {}, || false).unwrap();

        assert_eq!(outcome, ExtractOutcome::Completed { written: 1 });
        assert_eq!(fs::read_to_string(out.join("res/top.txt")).unwrap(), "top");
        assert!(!out.join("res/top.txt.json").exists(), "a text file names no prototype to decode");
        let _ = fs::remove_dir_all(&out);
    }

    #[test]
    fn an_empty_queue_reports_a_complete_fraction_rather_than_dividing_by_zero() {
        let at = |written, total| ExtractProgress { written, total, file: String::new() }.fraction();
        assert_eq!(at(0, 0), 1.0);
        assert_eq!(at(1, 4), 0.25);
    }
}
