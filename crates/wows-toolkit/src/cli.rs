use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;

/// Why a replaced-binary path was rejected. This gates a delete driven by an
/// untrusted argument, so each rejection carries the paths that caused it.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FinalizeError {
    #[error("replaced binary {replaced:?} is not in the same directory as {current:?}")]
    DifferentDirectory { replaced: PathBuf, current: PathBuf },
    #[error("replaced binary {replaced:?} does not have a .old extension")]
    UnexpectedName { replaced: PathBuf },
    #[error("replaced binary {replaced:?} does not exist")]
    Missing { replaced: PathBuf },
}

/// Decide whether `replaced` may be deleted by this process.
///
/// The directory check keeps an argument from naming a file elsewhere on the
/// system. The extension check narrows it further: every version that emits
/// this argument produces `<exe>.old`.
pub fn validate_finalize_target(current_exe: &Path, replaced: &Path) -> Result<(), FinalizeError> {
    if current_exe.parent() != replaced.parent() {
        return Err(FinalizeError::DifferentDirectory {
            replaced: replaced.to_path_buf(),
            current: current_exe.to_path_buf(),
        });
    }

    if replaced.extension() != Some("old".as_ref()) {
        return Err(FinalizeError::UnexpectedName { replaced: replaced.to_path_buf() });
    }

    if !replaced.exists() {
        return Err(FinalizeError::Missing { replaced: replaced.to_path_buf() });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::File;

    use tempfile::tempdir;

    #[test]
    fn accepts_sibling_dot_old_file_that_exists() {
        let dir = tempdir().expect("tempdir");
        let current = dir.path().join("wows_toolkit.exe");
        let replaced = dir.path().join("wows_toolkit.exe.old");
        File::create(&replaced).expect("create replaced");

        assert_eq!(validate_finalize_target(&current, &replaced), Ok(()));
    }

    #[test]
    fn rejects_path_in_another_directory() {
        let dir = tempdir().expect("tempdir");
        let other = tempdir().expect("other tempdir");
        let current = dir.path().join("wows_toolkit.exe");
        let replaced = other.path().join("wows_toolkit.exe.old");
        File::create(&replaced).expect("create replaced");

        assert_eq!(
            validate_finalize_target(&current, &replaced),
            Err(FinalizeError::DifferentDirectory { replaced, current })
        );
    }

    #[test]
    fn rejects_path_without_dot_old_extension() {
        let dir = tempdir().expect("tempdir");
        let current = dir.path().join("wows_toolkit.exe");
        let replaced = dir.path().join("something.exe");
        File::create(&replaced).expect("create replaced");

        assert_eq!(
            validate_finalize_target(&current, &replaced),
            Err(FinalizeError::UnexpectedName { replaced })
        );
    }

    #[test]
    fn rejects_path_that_does_not_exist() {
        let dir = tempdir().expect("tempdir");
        let current = dir.path().join("wows_toolkit.exe");
        let replaced = dir.path().join("wows_toolkit.exe.old");

        assert_eq!(
            validate_finalize_target(&current, &replaced),
            Err(FinalizeError::Missing { replaced })
        );
    }
}
