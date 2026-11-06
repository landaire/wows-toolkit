//! Upload stored scripts to the `scripts` release of wows-replay-data with
//! the GitHub CLI, which must be logged in with permission to edit releases.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::builds::BuildsIndex;
use crate::error::ScriptsError;
use crate::scripts::INDEX_FILE;
use crate::scripts::ScriptsEntry;
use crate::scripts::ScriptsIndex;

/// Repository whose release holds the published scripts.
pub const RELEASE_REPO: &str = "landaire/wows-replay-data";

/// Tag of the release holding the published scripts.
pub const RELEASE_TAG: &str = "scripts";

/// What publishing one build did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
    Uploaded(ScriptsEntry),
    /// The release already holds a zip with the same digest and size.
    AlreadyPublished,
    /// `builds.toml` does not list the build, so its version is unknown and
    /// nothing was uploaded.
    NotDumped,
}

/// Upload each of `builds`' stored scripts.zip that the release does not
/// already hold byte for byte, updating the release index after each one. The
/// version recorded is the one `builds.toml` in `output_base` lists; a build
/// it does not list is skipped.
pub fn publish(output_base: &Path, builds: &[u32]) -> Result<Vec<(u32, PublishOutcome)>, ScriptsError> {
    let dumped = BuildsIndex::load(&output_base.join("builds.toml"));
    let mut index = published_index()?;

    let mut outcomes = Vec::new();
    for &build in builds {
        let zip = super::local_zip(output_base, build).ok_or(ScriptsError::NotStored { build })?;
        let Some(dumped) = dumped.find_by_build(build) else {
            outcomes.push((build, PublishOutcome::NotDumped));
            continue;
        };
        let version = dumped.version.clone();
        let (sha256, size) = super::digest_file(&zip)?;

        if index.get(build).is_some_and(|published| published.sha256 == sha256 && published.size == size) {
            outcomes.push((build, PublishOutcome::AlreadyPublished));
            continue;
        }

        upload(&zip)?;
        let entry = ScriptsEntry { version, sha256, size };
        index.builds.insert(build, entry.clone());
        // Uploaded after each zip so the index never lists a zip the release
        // lacks, and an interrupted run loses at most the last entry.
        index.save(output_base)?;
        upload(&super::scripts_dir(output_base).join(INDEX_FILE))?;
        outcomes.push((build, PublishOutcome::Uploaded(entry)));
    }
    index.save(output_base)?;

    Ok(outcomes)
}

/// The release index, creating the release when it does not exist.
fn published_index() -> Result<ScriptsIndex, ScriptsError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Release {
        tag_name: String,
    }

    #[derive(Deserialize)]
    struct ReleaseAssets {
        assets: Vec<Asset>,
    }

    #[derive(Deserialize)]
    struct Asset {
        name: String,
    }

    let releases: Vec<Release> = gh_json(["release", "list", "--limit", "1000", "--json", "tagName"])?;
    if !releases.iter().any(|release| release.tag_name == RELEASE_TAG) {
        gh([
            "release",
            "create",
            RELEASE_TAG,
            "--title",
            "Client scripts",
            "--notes",
            "Client scripts.zip per game build. index.toml lists each build's version, sha256 and size.",
        ])?;
        return Ok(ScriptsIndex::default());
    }

    let release: ReleaseAssets = gh_json(["release", "view", RELEASE_TAG, "--json", "assets"])?;
    if !release.assets.iter().any(|asset| asset.name == INDEX_FILE) {
        // `--clobber` deletes the old index before uploading the new one, so
        // a failed upload can leave zips no index describes.
        let has_zips = release
            .assets
            .iter()
            .any(|asset| asset.name.strip_suffix(".zip").is_some_and(|stem| stem.parse::<u32>().is_ok()));
        if has_zips {
            return Err(ScriptsError::ReleaseWithoutIndex { tag: RELEASE_TAG });
        }
        return Ok(ScriptsIndex::default());
    }

    let text = gh(["release", "download", RELEASE_TAG, "--pattern", INDEX_FILE, "--output", "-"])?;

    ScriptsIndex::parse(&format!("release {RELEASE_TAG} {INDEX_FILE}"), &text)
}

/// Upload `path` as a release asset named after its file name.
fn upload(path: &Path) -> Result<(), ScriptsError> {
    gh([
        OsStr::new("release"),
        OsStr::new("upload"),
        OsStr::new(RELEASE_TAG),
        path.as_os_str(),
        OsStr::new("--clobber"),
    ])?;

    Ok(())
}

fn gh_json<T, I, S>(args: I) -> Result<T, ScriptsError>
where
    T: serde::de::DeserializeOwned,
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let stdout = gh(args)?;

    serde_json::from_str(&stdout).map_err(|source| ScriptsError::GhJson { source })
}

/// Run `gh` against [`RELEASE_REPO`], returning its standard output.
fn gh<I, S>(args: I) -> Result<String, ScriptsError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<S> = args.into_iter().collect();
    let output = Command::new("gh")
        .args(&args)
        .args(["--repo", RELEASE_REPO])
        .output()
        .map_err(|source| ScriptsError::GhSpawn { source })?;
    let command = || args.iter().map(|arg| arg.as_ref().to_string_lossy()).collect::<Vec<_>>().join(" ");
    if !output.status.success() {
        return Err(ScriptsError::Gh {
            command: command(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    String::from_utf8(output.stdout).map_err(|source| ScriptsError::GhUtf8 { command: command(), source })
}
