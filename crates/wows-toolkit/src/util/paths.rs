use std::path::Component;
use std::path::Path;

/// Whether `path` lies inside `root`, with `root` itself counting as inside.
///
/// Compared component-wise rather than as a string prefix, so a root of
/// `.../replays` does not swallow `.../replays_backup`.
///
/// Both sides are canonicalized when the filesystem resolves both, which
/// settles verbatim (`\\?\`) prefixes, short names and symlinks. When either
/// does not resolve -- a replay deleted since it was listed, most often -- the
/// raw paths are compared instead: canonicalizing only one side would leave two
/// spellings of the same directory looking unrelated.
pub(crate) fn path_is_within(root: &Path, path: &Path) -> bool {
    // An unset directory is not one anything can be inside of, and an empty
    // path has no components, so every path would otherwise match it.
    if root.as_os_str().is_empty() {
        return false;
    }
    match (std::fs::canonicalize(root), std::fs::canonicalize(path)) {
        (Ok(root), Ok(path)) => components_within(&root, &path),
        _ => components_within(root, path),
    }
}

fn components_within(root: &Path, path: &Path) -> bool {
    let mut candidate = path.components();
    for expected in root.components() {
        match candidate.next() {
            Some(actual) if component_eq(&expected, &actual) => {}
            _ => return false,
        }
    }
    true
}

/// Windows reaches the same directory under either case, so its components are
/// folded. ASCII-only: drive letters and the game's own directory names are
/// ASCII, and a full Unicode fold still would not reproduce what the filesystem
/// does.
fn component_eq(a: &Component<'_>, b: &Component<'_>) -> bool {
    if cfg!(windows) { a.as_os_str().eq_ignore_ascii_case(b.as_os_str()) } else { a == b }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Deliberately under a root that does not exist, so `canonicalize` fails
    /// on both sides and the raw comparison is what runs. A test that depended
    /// on real directories would prove nothing about the branch taken for a
    /// replay that has been deleted.
    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\wt-test-not-a-real-dir\replays"
        } else {
            "/wt-test-not-a-real-dir/replays"
        })
    }

    fn under(tail: &str) -> PathBuf {
        root().join(tail)
    }

    #[test]
    fn a_file_in_the_root_is_within_it() {
        assert!(path_is_within(&root(), &under("20260807_120000_PJSD.wowsreplay")));
    }

    #[test]
    fn a_file_in_a_subdirectory_is_within_the_root() {
        assert!(path_is_within(&root(), &under("archive/20260807_120000_PJSD.wowsreplay")));
    }

    #[test]
    fn the_root_is_within_itself() {
        assert!(path_is_within(&root(), &root()));
    }

    /// The reason components are compared instead of string prefixes: a plain
    /// `starts_with` on the rendered path would count this sibling as inside.
    #[test]
    fn a_sibling_sharing_a_name_prefix_is_not_within_the_root() {
        let sibling = root().parent().expect("the test root has a parent").join("replays_backup");
        assert!(!path_is_within(&root(), &sibling.join("20260807_120000_PJSD.wowsreplay")));
    }

    #[test]
    fn an_unrelated_directory_is_not_within_the_root() {
        let elsewhere = if cfg!(windows) {
            r"D:\downloads\someone-elses.wowsreplay"
        } else {
            "/downloads/someone-elses.wowsreplay"
        };
        assert!(!path_is_within(&root(), Path::new(elsewhere)));
    }

    #[test]
    fn a_parent_of_the_root_is_not_within_it() {
        assert!(!path_is_within(&root(), root().parent().expect("the test root has a parent")));
    }

    /// A workspace with no directory configured yet must not make every replay
    /// look like one of the user's own.
    #[test]
    fn nothing_is_within_an_empty_root() {
        assert!(!path_is_within(Path::new(""), &under("20260807_120000_PJSD.wowsreplay")));
    }

    /// Windows reaches one directory under many spellings, and the replays
    /// directory is routinely typed in a different case from how the game
    /// wrote it.
    #[cfg(windows)]
    #[test]
    fn case_does_not_decide_the_answer_on_windows() {
        let shouted = PathBuf::from(r"C:\WT-TEST-NOT-A-REAL-DIR\REPLAYS");
        assert!(path_is_within(&shouted, &under("20260807_120000_PJSD.wowsreplay")));
    }
}
