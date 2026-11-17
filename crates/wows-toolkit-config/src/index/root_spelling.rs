//! Spellings of one directory. A path reached through a symbolic link or a bind mount names the
//! same directory as its resolved path, and rows stored under one spelling must be found under
//! the other. A path that does not resolve, as on a drive not mounted, is matched only as written.

use std::path::Path;
use std::path::PathBuf;

/// `stored` or the ancestor of it that names the same directory as `chosen`, spelled as in
/// `stored`. `None` when none does or `chosen` does not resolve.
pub fn spelling_within(chosen: &Path, stored: &Path) -> Option<PathBuf> {
    let chosen = std::fs::canonicalize(chosen).ok()?;

    stored
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .find(|ancestor| std::fs::canonicalize(ancestor).is_ok_and(|resolved| resolved == chosen))
        .map(Path::to_path_buf)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A real directory `real/install/replays` and a link `link` to `real`.
    fn linked_install() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let real = tmp.path().join("real");
        std::fs::create_dir_all(real.join("install/replays")).expect("the install is made");
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("the link is made");

        (tmp, real, link)
    }

    #[test]
    fn a_linked_spelling_finds_the_stored_ancestor() {
        let (_tmp, real, link) = linked_install();
        let stored = real.join("install/replays");

        assert_eq!(spelling_within(&link.join("install"), &stored), Some(real.join("install")));
        assert_eq!(spelling_within(&real.join("install"), &link.join("install/replays")), Some(link.join("install")));
    }

    #[test]
    fn another_directory_or_an_unresolved_one_finds_nothing() {
        let (tmp, real, _link) = linked_install();
        let stored = real.join("install/replays");
        let other = tmp.path().join("other");
        std::fs::create_dir(&other).expect("the other directory is made");
        let unmounted = tmp.path().join("unmounted/install");

        assert_eq!(spelling_within(&other, &stored), None);
        assert_eq!(spelling_within(&unmounted, &stored), None);
        assert_eq!(spelling_within(&real.join("install"), &unmounted.join("replays")), None);
    }
}
