//! 7z-extracts a WG `.dspkg` FULL package into an install directory.
//!
//! `.dspkg` files are 7-zip archives. A FULL package's archive holds the
//! real game files; a DELTA package is an rsync-based diff against a prior
//! version and is not directly 7z-extractable to real files - see
//! [`is_delta_package`]. Delta application is not implemented here.

use std::path::Path;
use std::path::PathBuf;

use sevenz_rust2::ArchiveReader;
use sevenz_rust2::Password;

use crate::error::DownloadError;
use crate::webseed::safe_join;

/// A WG version token: at least three dot-separated numeric components,
/// e.g. `15.8`, `15.8.0`, `15.8.0.0.13090112`.
fn is_wg_version_token(segment: &str) -> bool {
    let parts: Vec<&str> = segment.split('.').collect();
    parts.len() >= 3 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// `true` if `file_name` has a DELTA package's name shape.
///
/// A FULL package name carries exactly one WG version token before its
/// trailing part segment (`wows.na_15.8.0.0.13090112_client.dspkg`); a
/// DELTA package carries two - the new and old versions being diffed
/// (`wows.na_15.8.0.0.13090112_15.7.0.0.13016296_client.dspkg`). Counting
/// `_`-separated tokens that look like a WG version is robust to the part
/// name itself (`client`, `hotfix`, ...) never looking like a version.
pub fn is_delta_package(file_name: &str) -> bool {
    let stem = file_name.strip_suffix(".dspkg").unwrap_or(file_name);
    let version_tokens = stem.split('_').filter(|segment| is_wg_version_token(segment)).count();
    version_tokens >= 2
}

/// 7z-extract a FULL `.dspkg` archive at `dspkg_path` into `dest_dir`,
/// returning the paths of every extracted (non-directory) file.
///
/// `dest_dir` is created if missing. Every archive entry's name is passed
/// through [`safe_join`] before any filesystem write, rejecting `..`,
/// absolute, or drive-prefixed entries as [`DownloadError::UnsafePath`] -
/// a `.dspkg` is downloaded content and its archive entries are just as
/// untrusted as the torrent manifest that named the file itself.
///
/// A DELTA-named package is rejected with
/// [`DownloadError::DeltaUnsupported`] before any extraction is attempted.
pub fn unpack_dspkg(dspkg_path: &Path, dest_dir: &Path) -> Result<Vec<PathBuf>, DownloadError> {
    let file_name = dspkg_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| DownloadError::InvalidDspkgFileName { path: dspkg_path.to_path_buf() })?
        .to_string();

    if is_delta_package(&file_name) {
        return Err(DownloadError::DeltaUnsupported { file: file_name });
    }

    std::fs::create_dir_all(dest_dir).map_err(|source| DownloadError::Io {
        operation: "creating",
        path: dest_dir.to_path_buf(),
        source,
    })?;

    let mut archive = ArchiveReader::open(dspkg_path, Password::empty())
        .map_err(|source| DownloadError::Unpack { file: file_name.clone(), source })?;

    let mut extracted = Vec::new();
    // Recorded separately from the closure's return type (sevenz_rust2::Error
    // cannot carry our DownloadError), then re-raised as the precise
    // UnsafePath variant once for_each_entries has unwound - see below.
    let mut unsafe_path: Option<DownloadError> = None;

    let result = archive.for_each_entries(|entry, reader| {
        let segments: Vec<String> = entry.name().replace('\\', "/").split('/').map(str::to_string).collect();
        let dest = match safe_join(dest_dir, &segments) {
            Ok(dest) => dest,
            Err(err) => {
                unsafe_path = Some(err);
                // Abort the whole archive immediately (an `Err` here
                // unwinds every remaining block, unlike returning
                // `Ok(false)`, which only stops the current block).
                return Err(std::io::Error::other("unsafe entry path, aborting extraction").into());
            }
        };

        if entry.is_directory() {
            std::fs::create_dir_all(&dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&dest)?;
            std::io::copy(reader, &mut out)?;
            extracted.push(dest);
        }
        Ok(true)
    });

    if let Some(err) = unsafe_path {
        return Err(err);
    }
    result.map_err(|source| DownloadError::Unpack { file: file_name.clone(), source })?;

    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_package_name_is_not_delta() {
        assert!(!is_delta_package("wows.na_15.8.0.0.13090112_client.dspkg"));
    }

    #[test]
    fn delta_package_name_is_delta() {
        assert!(is_delta_package("wows.na_15.8.0.0.13090112_15.7.0.0.13016296_client.dspkg"));
    }

    #[test]
    fn other_full_package_names_are_not_delta() {
        assert!(!is_delta_package("wows.na_15.8.0.0.13090112_sdcontent.dspkg"));
        assert!(!is_delta_package("wows.na_15.8.0.0.13090112_locale.dspkg"));
        assert!(!is_delta_package("wows.na_15.8.0.0.13090112_hotfix.dspkg"));
    }

    #[test]
    fn safe_join_guard_rejects_parent_dir_escape() {
        let base = Path::new("/staging");
        let segments = vec!["..".to_string(), "evil.txt".to_string()];
        assert!(matches!(safe_join(base, &segments), Err(DownloadError::UnsafePath { .. })));
    }
}
