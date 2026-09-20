//! Content search across the game VFS.
//!
//! Mirrors the egui app's `start_content_search`: the query is a regex when it
//! compiles and a literal otherwise, an optional glob narrows which paths are
//! read, and every match reports a short snippet of surrounding text.

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::Arc;
use wowsunpack::vfs::VfsPath;

use super::listing::FileList;

/// One match, with enough context to show a row without re-reading the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSearchHit {
    pub path: String,
    pub vfs_path: VfsPath,
    /// Surrounding text, newlines flattened so the row stays one line.
    pub context: String,
    /// Byte offset of the match within the file.
    pub offset: usize,
}

/// How far a scan has got, in files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchProgress {
    pub scanned: usize,
    pub total: usize,
}

/// Characters of context kept either side of a match.
const CONTEXT_RADIUS: usize = 30;

/// Files between progress reports. The scan is IO-bound and a report per file
/// would swamp the UI on a full install.
const PROGRESS_INTERVAL: usize = 100;

/// Read buffer above which the buffer is released rather than reused, so one
/// huge file does not pin memory for the rest of the scan.
const MAX_RETAINED_BUFFER: usize = 4 * 1024 * 1024;

/// Compiles `query` for searching.
///
/// A query that is valid regex is used as one; anything else is escaped and
/// matched literally, so typing an unbalanced bracket searches for that text
/// instead of failing. `None` only when even the escaped form will not
/// compile, which a literal effectively never is.
pub fn compile_query(query: &str) -> Option<regex::bytes::Regex> {
    regex::bytes::Regex::new(query).or_else(|_| regex::bytes::Regex::new(&regex::escape(query))).ok()
}

/// Which files a scan will read: files only, narrowed by `path_filter` when it
/// is a usable glob. An unparsable filter is ignored rather than matching
/// nothing, matching the egui app.
pub fn files_to_scan(files: &FileList, path_filter: &str) -> Vec<(Arc<PathBuf>, VfsPath)> {
    let glob = (!path_filter.is_empty()).then(|| glob::Pattern::new(path_filter).ok()).flatten();
    files
        .iter()
        .filter(|(path, entry)| {
            entry.is_file().unwrap_or(false) && glob.as_ref().is_none_or(|glob| glob.matches_path(path))
        })
        .cloned()
        .collect()
}

/// Text around a match, as a single line.
///
/// Valid UTF-8 is windowed by characters so a multi-byte character is never
/// split. Binary content falls back to a lossy window around the match, which
/// is why the radius is widened there before the lossy conversion.
pub fn context_snippet(data: &[u8], match_start: usize, match_end: usize, radius: usize) -> String {
    let Ok(text) = std::str::from_utf8(data) else {
        return binary_context_snippet(data, match_start, match_end, radius);
    };

    let match_char_start = text[..match_start].chars().count();
    let match_char_len = text[match_start..match_end].chars().count();

    let chars: Vec<char> = text.chars().collect();
    let start = match_char_start.saturating_sub(radius);
    let end = (match_char_start + match_char_len + radius).min(chars.len());

    flatten(&chars[start..end].iter().collect::<String>())
}

fn binary_context_snippet(data: &[u8], match_start: usize, match_end: usize, radius: usize) -> String {
    let window_start = match_start.saturating_sub(radius * 4);
    let window_end = (match_end + radius * 4).min(data.len());
    let lossy = String::from_utf8_lossy(&data[window_start..window_end]);
    let matched = String::from_utf8_lossy(&data[match_start..match_end]);

    let Some(position) = lossy.find(matched.as_ref()) else {
        return flatten(&matched);
    };
    let chars: Vec<char> = lossy.chars().collect();
    let start = lossy[..position].chars().count().saturating_sub(radius);
    let end = (start + radius + matched.chars().count() + radius).min(chars.len());
    flatten(&chars[start..end].iter().collect::<String>())
}

fn flatten(text: &str) -> String {
    text.replace('\n', " ").replace('\r', "")
}

/// Reads each file in `files` and reports every match.
///
/// A file that cannot be opened or read is skipped: a scan covering the whole
/// install will meet entries it cannot read, and stopping there would hide the
/// matches in every later file.
pub fn scan(
    files: &[(Arc<PathBuf>, VfsPath)],
    pattern: &regex::bytes::Regex,
    mut on_hit: impl FnMut(ContentSearchHit),
    mut on_progress: impl FnMut(SearchProgress),
    should_stop: impl Fn() -> bool,
) {
    let total = files.len();
    on_progress(SearchProgress { scanned: 0, total });

    let mut buffer: Vec<u8> = Vec::new();
    for (index, (path, entry)) in files.iter().enumerate() {
        if should_stop() {
            return;
        }
        if index % PROGRESS_INTERVAL == 0 {
            on_progress(SearchProgress { scanned: index, total });
        }

        buffer.clear();
        let Ok(mut file) = entry.open_file() else { continue };
        if file.read_to_end(&mut buffer).is_err() {
            continue;
        }

        for found in pattern.find_iter(&buffer) {
            on_hit(ContentSearchHit {
                path: path.to_string_lossy().into_owned(),
                vfs_path: entry.clone(),
                context: context_snippet(&buffer, found.start(), found.end(), CONTEXT_RADIUS),
                offset: found.start(),
            });
        }

        if buffer.capacity() > MAX_RETAINED_BUFFER {
            buffer = Vec::new();
        }
    }

    on_progress(SearchProgress { scanned: total, total });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unpacker::listing::build_file_list;
    use std::io::Write as _;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use wowsunpack::vfs::MemoryFS;

    fn fixture() -> VfsPath {
        let root: VfsPath = MemoryFS::new().into();
        root.join("res/content").unwrap().create_dir_all().unwrap();
        for (path, body) in [
            ("res/content/a.xml", "<ship>Yamato</ship>"),
            ("res/content/b.txt", "no match here"),
            ("res/notes.xml", "Yamato again, and Yamato once more"),
        ] {
            root.join(path).unwrap().create_file().unwrap().write_all(body.as_bytes()).unwrap();
        }
        root
    }

    #[test]
    fn a_valid_regex_compiles_as_a_regex() {
        let pattern = compile_query("Yam.to").unwrap();
        assert!(pattern.is_match(b"Yamato"));
    }

    #[test]
    fn an_invalid_regex_falls_back_to_a_literal_search() {
        // Built at runtime so the invalid-regex lint does not reject the
        // literal that this test exists to feed in.
        let query = format!("Yamato{}", '(');
        assert!(regex::bytes::Regex::new(&query).is_err(), "an unclosed group is not valid regex");
        let pattern = compile_query(&query).unwrap();
        assert!(pattern.is_match(b"Yamato("), "the text is matched literally");
        assert!(!pattern.is_match(b"Yamato"), "and the paren is still required");
    }

    #[test]
    fn only_files_are_scanned_and_a_glob_narrows_them() {
        let root = fixture();
        let files = build_file_list(&root);

        assert_eq!(files_to_scan(&files, "").len(), 3, "every file, no filter");
        let xml: Vec<String> =
            files_to_scan(&files, "*.xml").iter().map(|(path, _)| path.to_string_lossy().into_owned()).collect();
        assert_eq!(xml.len(), 2, "both xml files at any depth");
        assert!(xml.iter().all(|path| path.ends_with(".xml")));
    }

    #[test]
    fn an_unparsable_path_filter_is_ignored_rather_than_matching_nothing() {
        let root = fixture();
        let files = build_file_list(&root);
        assert!(glob::Pattern::new("a[").is_err());
        assert_eq!(files_to_scan(&files, "a[").len(), 3, "the bad filter is dropped");
    }

    #[test]
    fn a_scan_reports_every_match_in_every_file() {
        let root = fixture();
        let files = files_to_scan(&build_file_list(&root), "");
        let pattern = compile_query("Yamato").unwrap();

        let mut hits = Vec::new();
        scan(&files, &pattern, |hit| hits.push(hit), |_| {}, || false);

        assert_eq!(hits.len(), 3, "one in a.xml and two in notes.xml");
        assert!(hits.iter().all(|hit| hit.context.contains("Yamato")));
        let mut offsets: Vec<usize> = hits.iter().filter(|h| h.path.ends_with("notes.xml")).map(|h| h.offset).collect();
        offsets.sort();
        assert_eq!(offsets.len(), 2, "both matches in the same file are reported separately");
        assert_ne!(offsets[0], offsets[1], "and at different offsets");
    }

    #[test]
    fn progress_starts_at_zero_and_ends_at_the_total() {
        let root = fixture();
        let files = files_to_scan(&build_file_list(&root), "");
        let pattern = compile_query("Yamato").unwrap();

        let mut seen = Vec::new();
        scan(&files, &pattern, |_| {}, |progress| seen.push(progress), || false);

        assert_eq!(seen.first().copied(), Some(SearchProgress { scanned: 0, total: 3 }));
        assert_eq!(seen.last().copied(), Some(SearchProgress { scanned: 3, total: 3 }));
    }

    #[test]
    fn a_stop_request_ends_the_scan_without_a_final_progress_report() {
        let root = fixture();
        let files = files_to_scan(&build_file_list(&root), "");
        let pattern = compile_query("Yamato").unwrap();
        let stop = AtomicBool::new(true);

        let mut hits = Vec::new();
        let mut seen = Vec::new();
        scan(&files, &pattern, |hit| hits.push(hit), |p| seen.push(p), || stop.load(Ordering::Relaxed));

        assert!(hits.is_empty(), "nothing is read once stopped");
        assert_eq!(seen, vec![SearchProgress { scanned: 0, total: 3 }], "only the opening report");
    }

    #[test]
    fn a_snippet_keeps_context_either_side_and_flattens_newlines() {
        let data = b"alpha\nbeta Yamato gamma\ndelta";
        let start = data.windows(6).position(|w| w == b"Yamato").unwrap();
        let snippet = context_snippet(data, start, start + 6, 6);
        assert!(snippet.contains("Yamato"));
        assert!(!snippet.contains('\n'), "the snippet is one line");
        assert!(snippet.contains("beta"), "context before the match");
        assert!(snippet.contains("gamma"), "context after the match");
    }

    #[test]
    fn a_snippet_of_binary_content_still_yields_the_matched_text() {
        let mut data = vec![0xffu8, 0xfe, 0x00];
        let start = data.len();
        data.extend_from_slice(b"Yamato");
        data.extend_from_slice(&[0x00, 0xff]);
        assert!(std::str::from_utf8(&data).is_err(), "the fixture is not valid UTF-8");

        let snippet = context_snippet(&data, start, start + 6, 4);
        assert!(snippet.contains("Yamato"));
    }

    #[test]
    fn a_snippet_at_the_very_start_does_not_underflow() {
        let data = b"Yamato trails";
        let snippet = context_snippet(data, 0, 6, 30);
        assert!(snippet.starts_with("Yamato"));
    }
}
