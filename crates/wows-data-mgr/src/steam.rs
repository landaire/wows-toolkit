//! Read the depot manifests the public branch currently publishes, by shelling
//! out to the `steamroom` CLI.
//!
//! Manifest IDs come from public app info and need no account. Reading a build
//! number back out of a depot needs the depot key, which needs an account that
//! licenses the app.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use crate::manifest::AppId;
use crate::manifest::CLIENT_DEPOT_ID;
use crate::manifest::CONTENT_DEPOT_ID;
use crate::manifest::DepotId;
use crate::manifest::DepotManifest;
use crate::manifest::GameVersionEntry;
use crate::manifest::LOCALIZATION_DEPOT_ID;
use crate::manifest::ManifestId;

/// The executable this module drives.
const STEAMROOM: &str = "steamroom";

/// Why a Steam query produced no usable answer.
#[derive(Debug, thiserror::Error)]
pub enum SteamQueryError {
    #[error("{STEAMROOM} not found on PATH; install it with `cargo install steamroom-cli`")]
    SteamroomMissing,
    #[error("failed to run `{STEAMROOM} {subcommand}`")]
    Spawn {
        subcommand: &'static str,
        #[source]
        source: std::io::Error,
    },
    /// `code` is absent when a signal ended the process.
    #[error("`{STEAMROOM} {subcommand}` failed{}: {stderr}", match .code {
        Some(code) => format!(" with exit code {code}"),
        None => String::new(),
    })]
    CommandFailed { subcommand: &'static str, code: Option<i32>, stderr: String },
    #[error("`{STEAMROOM} {subcommand}` returned data that is not valid JSON")]
    Malformed {
        subcommand: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("the public branch of app {} publishes no manifest for the client depot {}", app.0, depot.0)]
    NoClientDepot { app: AppId, depot: DepotId },
    #[error("depot {} manifest {} holds no bin/<build> directory", depot.0, manifest.0)]
    NoBuildDirectory { depot: DepotId, manifest: ManifestId },
}

/// How the depot listing authenticates.
///
/// Listing a depot's filenames needs its key, and Steam only hands that to an
/// account that licenses the app. Public app info needs neither, so this is
/// passed only to the commands that read depot content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteamLogin {
    /// The account named in `.steam-user`, reusing whatever token `steamroom`
    /// has saved for it.
    Account(String),
    /// A token cached by the local Steam client.
    SteamClientToken,
}

impl SteamLogin {
    fn args(&self) -> Vec<OsString> {
        match self {
            Self::Account(user) => vec![OsString::from("--username"), OsString::from(user)],
            Self::SteamClientToken => vec![OsString::from("--use-steam-token")],
        }
    }
}

/// Whether `steamroom` may stop and ask for a password.
///
/// A prompt is fine for a command the user ran to log in, and stalls a dump
/// that was only refreshing a pin on the side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginPrompt {
    Allow,
    Deny,
}

/// Read the Steam account name the repo already saved for DepotDownloader,
/// falling back to whatever the local Steam client cached.
pub fn resolve_login(repo_root: &Path, override_user: Option<&str>) -> SteamLogin {
    if let Some(user) = override_user {
        return SteamLogin::Account(user.to_string());
    }
    match std::fs::read_to_string(repo_root.join(".steam-user")) {
        Ok(saved) if !saved.trim().is_empty() => SteamLogin::Account(saved.trim().to_string()),
        _ => SteamLogin::SteamClientToken,
    }
}

pub fn find_steamroom() -> Result<&'static str, SteamQueryError> {
    if Command::new(STEAMROOM).arg("--help").output().is_ok() {
        return Ok(STEAMROOM);
    }
    Err(SteamQueryError::SteamroomMissing)
}

/// The manifest each depot is published under on the public branch.
pub fn fetch_public_manifests(steamroom: &str, app: AppId) -> Result<BTreeMap<DepotId, ManifestId>, SteamQueryError> {
    let stdout = run(steamroom, "manifests", &manifests_args(app), LoginPrompt::Deny)?;
    parse_manifest_listing(&stdout)
}

/// Every build the depot's file listing holds a `bin/<build>/` directory for.
///
/// Pinning `--manifest` to the one about to be recorded is what keeps the
/// branch moving mid-run from attributing one build's number to another's
/// manifest.
pub fn discover_builds(
    steamroom: &str,
    app: AppId,
    client: &DepotManifest,
    login: &SteamLogin,
    prompt: LoginPrompt,
) -> Result<BTreeSet<u32>, SteamQueryError> {
    let stdout = run(steamroom, "files", &files_args(app, client, login), prompt)?;
    let builds = builds_from_file_listing(&stdout);
    if builds.is_empty() {
        return Err(SteamQueryError::NoBuildDirectory { depot: client.depot_id, manifest: client.manifest_id.clone() });
    }
    Ok(builds)
}

/// Assemble a manifest entry from the depots the public branch publishes.
/// Content and localization are absent for old builds, which is not an error.
pub fn entry_from_public_manifests(
    app: AppId,
    version: &str,
    depots: &BTreeMap<DepotId, ManifestId>,
) -> Result<GameVersionEntry, SteamQueryError> {
    let pin = |depot: DepotId| depots.get(&depot).map(|id| DepotManifest::new(depot, id.clone()));
    let client = pin(CLIENT_DEPOT_ID).ok_or(SteamQueryError::NoClientDepot { app, depot: CLIENT_DEPOT_ID })?;

    Ok(GameVersionEntry {
        version: version.to_string(),
        client,
        content: pin(CONTENT_DEPOT_ID),
        localization: pin(LOCALIZATION_DEPOT_ID),
    })
}

fn manifests_args(app: AppId) -> Vec<OsString> {
    vec![
        OsString::from("manifests"),
        OsString::from("--app"),
        OsString::from(app.0.to_string()),
        OsString::from("--format"),
        OsString::from("json"),
    ]
}

fn files_args(app: AppId, client: &DepotManifest, login: &SteamLogin) -> Vec<OsString> {
    let mut args = login.args();
    args.extend([
        OsString::from("files"),
        OsString::from("--app"),
        OsString::from(app.0.to_string()),
        OsString::from("--depot"),
        OsString::from(client.depot_id.0.to_string()),
        OsString::from("--manifest"),
        OsString::from(&client.manifest_id.0),
        OsString::from("--format"),
        OsString::from("plain"),
    ]);
    args
}

/// Run a query and return its stdout, which every caller parses.
///
/// Stdout is always captured for that reason, so under `Allow` stderr is
/// inherited instead: a login prompt writes there, and capturing it would leave
/// the user staring at a terminal that has gone silent waiting for a password.
/// The prompt is then visible rather than buffered, at the cost of stderr being
/// unavailable to quote back in a failure, which the user has already seen.
fn run(
    steamroom: &str,
    subcommand: &'static str,
    args: &[OsString],
    prompt: LoginPrompt,
) -> Result<String, SteamQueryError> {
    let (stdin, stderr) = match prompt {
        LoginPrompt::Allow => (Stdio::inherit(), Stdio::inherit()),
        LoginPrompt::Deny => (Stdio::null(), Stdio::piped()),
    };
    let output = Command::new(steamroom)
        .args(args)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .and_then(|child| child.wait_with_output())
        .map_err(|source| SteamQueryError::Spawn { subcommand, source })?;

    if !output.status.success() {
        return Err(SteamQueryError::CommandFailed {
            subcommand,
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(serde::Deserialize)]
struct ManifestListing {
    depot: u32,
    manifest: String,
}

fn parse_manifest_listing(stdout: &str) -> Result<BTreeMap<DepotId, ManifestId>, SteamQueryError> {
    let listings: Vec<ManifestListing> = serde_json::from_str(stdout)
        .map_err(|source| SteamQueryError::Malformed { subcommand: "manifests", source })?;
    Ok(listings.into_iter().map(|l| (DepotId(l.depot), ManifestId(l.manifest))).collect())
}

/// Scan a depot file listing for the builds it ships.
///
/// A build directory is `bin/<build>/` at the depot root; `bin` deeper in a
/// path is some other directory that happens to share the name. Steam paths
/// arrive with either separator depending on the depot, so both split a
/// component.
fn builds_from_file_listing(stdout: &str) -> BTreeSet<u32> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut components = line.trim().split(['/', '\\']);
            if !components.next()?.eq_ignore_ascii_case("bin") {
                return None;
            }
            components.next()?.parse::<u32>().ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: AppId = AppId(552990);

    fn manifest_id(id: &str) -> ManifestId {
        ManifestId(id.to_string())
    }

    #[test]
    fn a_manifest_listing_maps_every_depot_to_its_pin() {
        let stdout = r#"[
          { "depot": 552991, "manifest": "2381082003725578406" },
          { "depot": 552993, "manifest": "6201496915258390911" },
          { "depot": 552994, "manifest": "3704313530698924538" }
        ]"#;

        let depots = parse_manifest_listing(stdout).unwrap();

        assert_eq!(depots.get(&CLIENT_DEPOT_ID), Some(&manifest_id("6201496915258390911")));
        assert_eq!(depots.get(&CONTENT_DEPOT_ID), Some(&manifest_id("2381082003725578406")));
        assert_eq!(depots.get(&LOCALIZATION_DEPOT_ID), Some(&manifest_id("3704313530698924538")));
    }

    /// The app publishes marker and wallpaper depots this crate has no use for.
    #[test]
    fn depots_the_manifest_does_not_pin_are_carried_but_unused() {
        let stdout = r#"[
          { "depot": 552992, "manifest": "53635004422444341" },
          { "depot": 552993, "manifest": "client" },
          { "depot": 552996, "manifest": "4635489861143683887" }
        ]"#;

        let depots = parse_manifest_listing(stdout).unwrap();
        let entry = entry_from_public_manifests(APP, "15.7.0", &depots).unwrap();

        assert_eq!(entry.client, DepotManifest::new(CLIENT_DEPOT_ID, manifest_id("client")));
        assert_eq!(entry.content, None);
        assert_eq!(entry.localization, None);
    }

    /// Old builds predate the localization depot; that shape is legitimate and
    /// round-trips through the manifest file.
    #[test]
    fn a_branch_without_localization_yields_a_client_and_content_entry() {
        let depots =
            BTreeMap::from([(CLIENT_DEPOT_ID, manifest_id("client")), (CONTENT_DEPOT_ID, manifest_id("content"))]);

        let entry = entry_from_public_manifests(APP, "9.3.1", &depots).unwrap();

        assert_eq!(entry.version, "9.3.1");
        assert_eq!(entry.content, Some(DepotManifest::new(CONTENT_DEPOT_ID, manifest_id("content"))));
        assert_eq!(entry.localization, None);
    }

    /// Without a client depot there is nothing to pin and nothing to read a
    /// build number out of, so this must not degrade into a partial entry.
    #[test]
    fn a_branch_without_a_client_depot_is_an_error() {
        let depots = BTreeMap::from([(CONTENT_DEPOT_ID, manifest_id("content"))]);

        let err = entry_from_public_manifests(APP, "15.7.0", &depots).unwrap_err();

        assert!(matches!(err, SteamQueryError::NoClientDepot { app: APP, depot: CLIENT_DEPOT_ID }));
        assert!(err.to_string().contains("552993"));
    }

    #[test]
    fn malformed_json_names_the_subcommand_that_produced_it() {
        let err = parse_manifest_listing("not json").unwrap_err();

        assert!(matches!(err, SteamQueryError::Malformed { .. }));
        assert!(err.to_string().contains("manifests"));
    }

    #[test]
    fn a_file_listing_yields_the_build_its_bin_directory_names() {
        let stdout = "res_packages/content.pkg\nbin/13015811/bin32/WorldOfWarships32.exe\nbin/13015811/res/x.xml\n";

        assert_eq!(builds_from_file_listing(stdout), BTreeSet::from([13_015_811]));
    }

    /// Steam depot paths use the platform separator.
    #[test]
    fn backslash_separated_paths_are_scanned_too() {
        let stdout = "bin\\13015811\\bin64\\WorldOfWarships64.exe\n";

        assert_eq!(builds_from_file_listing(stdout), BTreeSet::from([13_015_811]));
    }

    /// A depot mid-transition can ship two; reporting both lets the caller
    /// decide rather than silently picking one.
    #[test]
    fn every_build_directory_in_the_listing_is_reported() {
        let stdout = "bin/13015811/x\nbin/12830008/y\nbin/13015811/z\n";

        assert_eq!(builds_from_file_listing(stdout), BTreeSet::from([12_830_008, 13_015_811]));
    }

    /// Paths that merely contain the word must not be read as build directories.
    #[test]
    fn paths_without_a_numeric_build_component_are_ignored() {
        let stdout = "bin/scripts/x\nres/bin/notanumber/y\nbin\n\nreadme.txt\n";

        assert!(builds_from_file_listing(stdout).is_empty());
    }

    #[test]
    fn the_manifests_query_asks_for_json_and_needs_no_login() {
        assert_eq!(
            manifests_args(APP),
            ["manifests", "--app", "552990", "--format", "json"].map(OsString::from).to_vec()
        );
    }

    /// Login flags are global and must precede the subcommand, and the listing
    /// is pinned to the exact manifest being recorded.
    #[test]
    fn the_file_listing_query_pins_the_manifest_and_leads_with_the_login() {
        let client = DepotManifest::new(CLIENT_DEPOT_ID, manifest_id("6201496915258390911"));

        assert_eq!(
            files_args(APP, &client, &SteamLogin::Account("captain".to_string())),
            [
                "--username",
                "captain",
                "files",
                "--app",
                "552990",
                "--depot",
                "552993",
                "--manifest",
                "6201496915258390911",
                "--format",
                "plain",
            ]
            .map(OsString::from)
            .to_vec()
        );
        assert_eq!(files_args(APP, &client, &SteamLogin::SteamClientToken)[0], OsString::from("--use-steam-token"));
    }

    #[test]
    fn a_saved_steam_user_becomes_the_account_to_log_in_as() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".steam-user"), "captain\n").unwrap();

        assert_eq!(resolve_login(dir.path(), None), SteamLogin::Account("captain".to_string()));
        assert_eq!(resolve_login(dir.path(), Some("other")), SteamLogin::Account("other".to_string()));
    }

    /// An absent or blank file is not an account name.
    #[test]
    fn no_saved_user_falls_back_to_the_steam_client_token() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(resolve_login(dir.path(), None), SteamLogin::SteamClientToken);

        std::fs::write(dir.path().join(".steam-user"), "  \n").unwrap();
        assert_eq!(resolve_login(dir.path(), None), SteamLogin::SteamClientToken);
    }
}
