use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::error::PathError;

/// Directories that file path arguments must resolve inside. Paths are
/// canonicalized before the check, so symbolic links that lead out of every
/// root are rejected.
#[derive(Debug, Clone)]
pub struct PathRoots {
    roots: Vec<PathBuf>,
}

impl PathRoots {
    /// A root that does not exist holds no files, so it is left out rather
    /// than failing the whole set.
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let roots = roots.into_iter().filter_map(|root| root.canonicalize().ok()).collect();

        Self { roots }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Resolves `path` to the canonical path of an existing file system entry
    /// inside one of the roots. A relative path is tried against each root
    /// in order.
    pub fn resolve(&self, path: &Path) -> Result<PathBuf, PathError> {
        if path.components().any(|component| matches!(component, Component::ParentDir)) {
            return Err(PathError::ParentComponent { path: path.to_path_buf() });
        }

        if is_network_or_device(path) {
            return Err(PathError::OutsideRoots { path: path.to_path_buf() });
        }

        // One error whether a path outside the roots exists or not, so a caller cannot probe the
        // file system. A Windows path with a root or a drive but not both, such as `\Users` or
        // `D:file`, resolves against a current directory, not a root, so it is refused unread.
        let anchored = path.has_root() || path.components().any(|component| matches!(component, Component::Prefix(_)));
        let canonical = if path.is_absolute() {
            path.canonicalize().map_err(|_| PathError::OutsideRoots { path: path.to_path_buf() })?
        } else if anchored {
            return Err(PathError::OutsideRoots { path: path.to_path_buf() });
        } else {
            self.resolve_relative(path)?
        };

        if !self.roots.iter().any(|root| canonical.starts_with(root)) {
            return Err(PathError::OutsideRoots { path: path.to_path_buf() });
        }

        Ok(canonical)
    }

    fn resolve_relative(&self, path: &Path) -> Result<PathBuf, PathError> {
        let mut last_error = None;
        for root in &self.roots {
            match root.join(path).canonicalize() {
                Ok(canonical) => return Ok(canonical),
                Err(error) => last_error = Some(error),
            }
        }

        let source = last_error.unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound));

        Err(PathError::Unreadable { path: path.to_path_buf(), source })
    }
}

/// Whether `path` names a network share or a device. Canonicalizing or opening one on Windows
/// connects to the named host, sending the user's credentials, before any check could refuse it.
pub fn is_network_or_device(path: &Path) -> bool {
    path.components().any(|component| matches!(component, Component::Prefix(prefix) if !is_disk(prefix.kind())))
}

fn is_disk(prefix: std::path::Prefix<'_>) -> bool {
    matches!(prefix, std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        outside: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("sub/inside.txt"), b"inside").unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();

        Fixture { _dir: dir, root, outside }
    }

    #[test]
    fn resolves_absolute_and_relative_paths_inside_the_root() {
        let fixture = fixture();
        let roots = PathRoots::new([fixture.root.clone()]);
        let expected = fixture.root.join("sub/inside.txt").canonicalize().unwrap();

        assert_eq!(roots.resolve(&fixture.root.join("sub/inside.txt")).unwrap(), expected);
        assert_eq!(roots.resolve(Path::new("sub/inside.txt")).unwrap(), expected);
    }

    #[test]
    fn rejects_parent_components_even_when_they_stay_inside() {
        let fixture = fixture();
        let roots = PathRoots::new([fixture.root.clone()]);

        let escaping = Path::new("../outside/secret.txt");
        assert!(matches!(roots.resolve(escaping), Err(PathError::ParentComponent { .. })));

        let looping = fixture.root.join("sub/../sub/inside.txt");
        assert!(matches!(roots.resolve(&looping), Err(PathError::ParentComponent { .. })));
    }

    #[test]
    fn rejects_absolute_paths_outside_the_root() {
        let fixture = fixture();
        let roots = PathRoots::new([fixture.root.clone()]);

        let result = roots.resolve(&fixture.outside.join("secret.txt"));
        assert!(matches!(result, Err(PathError::OutsideRoots { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_links_that_leave_the_root() {
        let fixture = fixture();
        std::os::unix::fs::symlink(fixture.outside.join("secret.txt"), fixture.root.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(&fixture.outside, fixture.root.join("linkdir")).unwrap();
        let roots = PathRoots::new([fixture.root.clone()]);

        assert!(matches!(roots.resolve(Path::new("link.txt")), Err(PathError::OutsideRoots { .. })));
        assert!(matches!(roots.resolve(Path::new("linkdir/secret.txt")), Err(PathError::OutsideRoots { .. })));
    }

    #[test]
    fn missing_roots_hold_nothing() {
        let fixture = fixture();
        let roots = PathRoots::new([fixture.root.join("absent")]);

        assert!(roots.roots().is_empty());
        let result = roots.resolve(&fixture.root.join("sub/inside.txt"));
        assert!(matches!(result, Err(PathError::OutsideRoots { .. })));
    }

    #[cfg(windows)]
    #[test]
    fn network_and_device_paths_are_refused_unopened() {
        let fixture = fixture();
        let roots = PathRoots::new([fixture.root.clone()]);

        for path in [r"\\attacker.example\share\a.wowsreplay", r"\\?\UNC\attacker.example\share\a", r"\\.\pipe\x"] {
            assert!(matches!(roots.resolve(Path::new(path)), Err(PathError::OutsideRoots { .. })), "{path}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn root_relative_paths_outside_the_root_get_one_error() {
        let fixture = fixture();
        let roots = PathRoots::new([fixture.root.clone()]);
        let secret = fixture.outside.join("secret.txt");
        let mut components = secret.components();
        components.next();
        let root_relative = components.as_path();

        for path in [root_relative.to_path_buf(), root_relative.with_file_name("absent.txt")] {
            assert!(matches!(roots.resolve(&path), Err(PathError::OutsideRoots { .. })), "{}", path.display());
        }
    }
}
