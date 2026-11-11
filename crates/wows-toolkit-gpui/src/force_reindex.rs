//! Forced re-indexing: reading chosen replays again, whatever the index holds
//! for them.
//!
//! A replay is read through the game data of the release it was recorded on,
//! so what can be re-indexed is decided before anything runs: a release this
//! machine can read is used as it is, one the published repository has is
//! downloaded first, and one neither has leaves its replays out.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use jiff::civil::DateTime;
use rust_i18n::t;
use wows_data_mgr::download_repo::RemoteAvailability;
use wows_data_mgr::download_repo::ResolvedBuild;
use wowsunpack::data::Version;

/// The format replay headers record the battle time in.
const HEADER_DATE_FORMAT: &str = "%d.%m.%Y %H:%M:%S";

/// The format the confirmation states a battle time in.
const SAID_DATE_FORMAT: &str = "%Y-%m-%d %H:%M";

/// A build number, which with the version identifies the game data a replay
/// is read through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildNumber(pub u32);

/// The release a replay was recorded on.
///
/// Build numbers are per server, so one number can name two releases whose
/// game data differs; the version is what tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Release {
    /// `major.minor.patch`, with no build.
    pub version: Version,
    pub build: BuildNumber,
}

impl Release {
    /// `major.minor.patch`, as the game-data repository is asked for it.
    pub fn version_said(&self) -> String {
        version_said(&self.version)
    }
}

impl PartialOrd for Release {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Release {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.version, self.build).cmp(&(other.version, other.build))
    }
}

/// What a replay header says about the release it was recorded on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderRelease {
    Named(Release),
    /// A version with no build number, which names no game data to read it
    /// through.
    NoBuild,
    /// No version that parses.
    NoVersion,
}

impl HeaderRelease {
    /// From the version a header's `clientVersionFromExe` parsed into.
    pub fn from_version(version: Option<Version>) -> Self {
        let Some(version) = version else { return Self::NoVersion };
        let Some(build) = version.build else { return Self::NoBuild };

        Self::Named(Release {
            version: Version::base(version.major, version.minor, version.patch),
            build: BuildNumber(build.get()),
        })
    }

    pub fn release(self) -> Option<Release> {
        match self {
            Self::Named(release) => Some(release),
            Self::NoBuild | Self::NoVersion => None,
        }
    }
}

/// One replay that could be re-indexed, as the listing knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexCandidate {
    pub path: PathBuf,
    pub release: HeaderRelease,
    /// When the battle was played. `None` when the header's time does not
    /// parse.
    pub played_at: Option<DateTime>,
}

impl ReindexCandidate {
    /// From what a listing reads out of a replay header.
    pub fn from_listing(path: PathBuf, version: Option<Version>, date_time: &str) -> Self {
        let release = HeaderRelease::from_version(version);
        let played_at = DateTime::strptime(HEADER_DATE_FORMAT, date_time).ok();

        Self { path, release, played_at }
    }
}

/// When the reader is asked before a forced re-index runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    /// A command over a whole listing, a version or a group: the reader has
    /// not seen which replays it covers, so the count and range are put first.
    Always,
    /// Rows the reader chose one by one: asked only when the choice is large,
    /// when game data has to be fetched first, or when some rows cannot be
    /// read.
    WhenNotPlain,
}

/// Which replays a palette command re-indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReindexScope {
    All,
    /// The replays recorded on one `major.minor.patch`, whatever the build.
    Version(Version),
}

impl ReindexScope {
    pub fn select(&self, candidates: Vec<ReindexCandidate>) -> Vec<ReindexCandidate> {
        match self {
            Self::All => candidates,
            Self::Version(version) => candidates
                .into_iter()
                .filter(|candidate| candidate.release.release().is_some_and(|release| release.version == *version))
                .collect(),
        }
    }
}

/// One game version the listing holds, and how many of its replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionCount {
    pub version: Version,
    pub replays: usize,
}

/// Every version the candidates were recorded on, newest first.
///
/// A replay naming no release is left out: it cannot be re-indexed under any
/// version.
pub fn versions(candidates: &[ReindexCandidate]) -> Vec<VersionCount> {
    let mut counted: BTreeMap<Version, usize> = BTreeMap::new();
    for release in candidates.iter().filter_map(|candidate| candidate.release.release()) {
        *counted.entry(release.version).or_default() += 1;
    }

    counted.into_iter().rev().map(|(version, replays)| VersionCount { version, replays }).collect()
}

/// One release the candidates were recorded on, and how many of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildNeed {
    pub release: Release,
    pub replays: usize,
}

/// Every release the candidates were recorded on, newest first.
pub fn builds(candidates: &[ReindexCandidate]) -> Vec<BuildNeed> {
    let mut counted: BTreeMap<Release, usize> = BTreeMap::new();
    for release in candidates.iter().filter_map(|candidate| candidate.release.release()) {
        *counted.entry(release).or_default() += 1;
    }

    counted.into_iter().rev().map(|(release, replays)| BuildNeed { release, replays }).collect()
}

/// The repository could not be asked about some releases, so whether their
/// game data exists is not known.
#[derive(Debug, thiserror::Error)]
#[error("the game-data repository could not be reached for {} release(s)", releases.len())]
pub struct RepositoryUnreachable {
    pub releases: Vec<Release>,
}

/// The releases among `missing` the repository publishes data for, from its
/// answer to a plan request.
///
/// Matched on build and version together, as each was asked. A release the
/// repository could not be asked about fails the whole answer: calling it
/// unobtainable would tell the reader something untrue about their data.
pub fn fetchable_from_plan(
    missing: &[Release],
    resolved: &[ResolvedBuild],
) -> Result<BTreeSet<Release>, RepositoryUnreachable> {
    let mut fetchable = BTreeSet::new();
    let mut unreachable = Vec::new();

    for release in missing {
        let version = release.version_said();
        let answer = resolved.iter().find(|resolved| {
            resolved.requested_build == release.build.0
                && resolved.requested_version.as_deref() == Some(version.as_str())
        });
        match answer.map(|resolved| &resolved.availability) {
            Some(RemoteAvailability::Exact | RemoteAvailability::Nearest { .. }) => {
                fetchable.insert(*release);
            }
            Some(RemoteAvailability::Unreachable) => unreachable.push(*release),
            // Not published, or not answered for: nothing to fetch.
            Some(RemoteAvailability::Unpublished) | None => {}
        }
    }

    if !unreachable.is_empty() {
        return Err(RepositoryUnreachable { releases: unreachable });
    }

    Ok(fetchable)
}

/// What can be fetched for the releases this machine cannot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetchable {
    /// The releases the repository publishes.
    Releases(BTreeSet<Release>),
    /// No game-data cache directory is set, so nothing can be fetched.
    NoCacheDirectory,
}

impl Fetchable {
    fn contains(&self, release: &Release) -> bool {
        match self {
            Self::Releases(releases) => releases.contains(release),
            Self::NoCacheDirectory => false,
        }
    }
}

/// Why releases in a summary cannot be obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnobtainableCause {
    NotPublished,
    NoCacheDirectory,
}

/// The oldest and newest battle among the replays a pass reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayedRange {
    pub oldest: DateTime,
    pub newest: DateTime,
}

/// What a forced re-index will do, as the reader is asked to confirm it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexSummary {
    /// The replays that will be read, with the release each needs.
    pub replays: Vec<(PathBuf, Release)>,
    /// `None` when no included replay's time parses.
    pub played: Option<PlayedRange>,
    /// Included replays whose time does not parse, which `played` does not
    /// cover.
    pub undated: usize,
    /// Releases downloaded before the pass, newest first.
    pub downloads: Vec<BuildNeed>,
    /// Releases nothing can supply, newest first. Their replays are skipped.
    pub unobtainable: Vec<BuildNeed>,
    pub unobtainable_cause: UnobtainableCause,
    /// Replays whose header names a version but no build, which are skipped.
    pub no_build: usize,
    /// Replays whose header names no version that parses, which are skipped.
    pub no_version: usize,
}

impl ReindexSummary {
    /// Replays left out of the pass.
    pub fn skipped(&self) -> usize {
        self.unobtainable.iter().map(|need| need.replays).sum::<usize>() + self.no_build + self.no_version
    }

    /// Whether the pass reads everything asked for with nothing fetched first.
    pub fn is_plain(&self) -> bool {
        self.downloads.is_empty() && self.skipped() == 0
    }

    /// Whether the pass may run without putting the summary to the reader.
    pub fn runs_unasked(&self, confirm: Confirm) -> bool {
        // A choice past this size is more than the reader picked row by row.
        const QUIET_LIMIT: usize = 50;

        confirm == Confirm::WhenNotPlain && self.is_plain() && self.replays.len() <= QUIET_LIMIT
    }

    /// The replays to read once the `failed` releases could not be
    /// downloaded, and how many that drops.
    pub fn readable_after(&self, failed: &BTreeSet<Release>) -> (Vec<PathBuf>, usize) {
        let mut dropped = 0;
        let mut kept = Vec::with_capacity(self.replays.len());
        for (path, release) in &self.replays {
            if failed.contains(release) {
                dropped += 1;
                continue;
            }
            kept.push(path.clone());
        }

        (kept, dropped)
    }

    /// What the confirmation says, one line each.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![t!("ui.reindex.confirm_count", count = self.replays.len()).into_owned()];

        if let Some(played) = self.played {
            lines.push(
                t!(
                    "ui.reindex.confirm_range",
                    oldest = played.oldest.strftime(SAID_DATE_FORMAT).to_string(),
                    newest = played.newest.strftime(SAID_DATE_FORMAT).to_string()
                )
                .into_owned(),
            );
        }

        if self.undated > 0 {
            lines.push(t!("ui.reindex.confirm_undated", count = self.undated).into_owned());
        }

        if !self.downloads.is_empty() {
            lines.push(t!("ui.reindex.confirm_downloads").into_owned());
            lines.extend(self.downloads.iter().map(build_line));
        }

        if !self.unobtainable.is_empty() {
            let count: usize = self.unobtainable.iter().map(|need| need.replays).sum();
            let said = match self.unobtainable_cause {
                UnobtainableCause::NotPublished => t!("ui.reindex.confirm_unobtainable", count = count),
                UnobtainableCause::NoCacheDirectory => t!("ui.reindex.confirm_no_cache_dir", count = count),
            };
            lines.push(said.into_owned());
            lines.extend(self.unobtainable.iter().map(build_line));
        }

        if self.no_build > 0 {
            lines.push(t!("ui.reindex.confirm_no_build", count = self.no_build).into_owned());
        }

        if self.no_version > 0 {
            lines.push(t!("ui.reindex.confirm_no_version", count = self.no_version).into_owned());
        }

        lines
    }
}

/// Splits the candidates into what a pass reads and what it leaves out.
///
/// `local` are the releases this machine can read; `fetchable` what can be
/// downloaded for the rest. A release in neither is unobtainable.
pub fn summarize(candidates: &[ReindexCandidate], local: &BTreeSet<Release>, fetchable: &Fetchable) -> ReindexSummary {
    let mut replays = Vec::new();
    let mut played: Option<PlayedRange> = None;
    let mut undated = 0;
    let mut no_build = 0;
    let mut no_version = 0;

    for candidate in candidates {
        let release = match candidate.release {
            HeaderRelease::Named(release) => release,
            HeaderRelease::NoBuild => {
                no_build += 1;
                continue;
            }
            HeaderRelease::NoVersion => {
                no_version += 1;
                continue;
            }
        };
        if !local.contains(&release) && !fetchable.contains(&release) {
            continue;
        }

        replays.push((candidate.path.clone(), release));
        let Some(at) = candidate.played_at else {
            undated += 1;
            continue;
        };
        played = Some(match played {
            Some(range) => PlayedRange { oldest: range.oldest.min(at), newest: range.newest.max(at) },
            None => PlayedRange { oldest: at, newest: at },
        });
    }

    let (downloads, unobtainable): (Vec<BuildNeed>, Vec<BuildNeed>) = builds(candidates)
        .into_iter()
        .filter(|need| !local.contains(&need.release))
        .partition(|need| fetchable.contains(&need.release));
    let unobtainable_cause = match fetchable {
        Fetchable::Releases(_) => UnobtainableCause::NotPublished,
        Fetchable::NoCacheDirectory => UnobtainableCause::NoCacheDirectory,
    };

    ReindexSummary { replays, played, undated, downloads, unobtainable, unobtainable_cause, no_build, no_version }
}

/// `major.minor.patch`.
pub fn version_said(version: &Version) -> String {
    format!("{}.{}.{}", version.major, version.minor, version.patch)
}

/// One release's line in the confirmation.
fn build_line(need: &BuildNeed) -> String {
    t!(
        "ui.reindex.build_row",
        version = need.release.version_said(),
        build = need.release.build.0,
        count = need.replays
    )
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header's version, as `clientVersionFromExe` parses it.
    fn header(version: &str, build: u32) -> Option<Version> {
        Version::try_from_client_exe(&format!("{},{build}", version.replace('.', ",")))
    }

    fn candidate(name: &str, version: Option<Version>, date_time: &str) -> ReindexCandidate {
        ReindexCandidate::from_listing(PathBuf::from(name), version, date_time)
    }

    fn release(version: &str, build: u32) -> Release {
        HeaderRelease::from_version(header(version, build)).release().expect("the header names a release")
    }

    fn published(releases: &[Release]) -> Fetchable {
        Fetchable::Releases(releases.iter().copied().collect())
    }

    /// A small listing: two releases installed, one the repository has, one
    /// nobody has, a replay with no readable time, one naming a version with
    /// no build, and one naming no version.
    fn listing() -> Vec<ReindexCandidate> {
        vec![
            candidate("a.wowsreplay", header("14.3.0", 300), "02.04.2025 18:00:00"),
            candidate("b.wowsreplay", header("14.3.0", 300), "01.04.2025 09:30:00"),
            candidate("c.wowsreplay", header("13.5.1", 200), "15.12.2024 20:15:00"),
            candidate("d.wowsreplay", header("12.0.0", 100), "03.01.2023 10:00:00"),
            candidate("e.wowsreplay", header("0.11.0", 50), "01.01.2022 12:00:00"),
            candidate("f.wowsreplay", None, "05.05.2024 05:05:05"),
            candidate("g.wowsreplay", header("12.0.0", 100), "not a time"),
            candidate("h.wowsreplay", header("12.0.0", 0), "05.05.2024 05:05:05"),
        ]
    }

    #[test]
    fn the_summary_counts_reads_downloads_and_skips() {
        let local: BTreeSet<Release> = [release("14.3.0", 300), release("12.0.0", 100)].into();

        let summary = summarize(&listing(), &local, &published(&[release("13.5.1", 200)]));

        let read: Vec<&str> = summary.replays.iter().map(|(path, _)| path.to_str().expect("ascii")).collect();
        assert_eq!(read, ["a.wowsreplay", "b.wowsreplay", "c.wowsreplay", "d.wowsreplay", "g.wowsreplay"]);
        assert_eq!(
            summary.played,
            Some(PlayedRange {
                oldest: jiff::civil::date(2023, 1, 3).at(10, 0, 0, 0),
                newest: jiff::civil::date(2025, 4, 2).at(18, 0, 0, 0),
            }),
            "the range spans only the replays that are read"
        );
        assert_eq!(summary.undated, 1, "a replay with no readable time is counted, not left out of the pass");
        assert_eq!(summary.downloads, [BuildNeed { release: release("13.5.1", 200), replays: 1 }]);
        assert_eq!(summary.unobtainable, [BuildNeed { release: release("0.11.0", 50), replays: 1 }]);
        assert_eq!(summary.no_build, 1);
        assert_eq!(summary.no_version, 1);
        assert_eq!(summary.skipped(), 3);
        assert!(!summary.is_plain());

        assert_eq!(
            summary.lines(),
            [
                t!("ui.reindex.confirm_count", count = 5).into_owned(),
                t!("ui.reindex.confirm_range", oldest = "2023-01-03 10:00", newest = "2025-04-02 18:00").into_owned(),
                t!("ui.reindex.confirm_undated", count = 1).into_owned(),
                t!("ui.reindex.confirm_downloads").into_owned(),
                t!("ui.reindex.build_row", version = "13.5.1", build = 200, count = 1).into_owned(),
                t!("ui.reindex.confirm_unobtainable", count = 1).into_owned(),
                t!("ui.reindex.build_row", version = "0.11.0", build = 50, count = 1).into_owned(),
                t!("ui.reindex.confirm_no_build", count = 1).into_owned(),
                t!("ui.reindex.confirm_no_version", count = 1).into_owned(),
            ]
        );
    }

    /// With no cache directory nothing can be fetched, and the summary says
    /// that rather than that nothing is published.
    #[test]
    fn without_a_cache_directory_the_summary_names_that_as_the_cause() {
        let local: BTreeSet<Release> = [release("14.3.0", 300)].into();

        let summary = summarize(&listing(), &local, &Fetchable::NoCacheDirectory);

        assert_eq!(summary.unobtainable_cause, UnobtainableCause::NoCacheDirectory);
        assert!(summary.downloads.is_empty());
        let unobtainable: usize = summary.unobtainable.iter().map(|need| need.replays).sum();
        assert!(summary.lines().contains(&t!("ui.reindex.confirm_no_cache_dir", count = unobtainable).into_owned()));
    }

    /// Build numbers are per server: the same number on another version is
    /// other game data, and is neither readable nor fetched on the first's
    /// account.
    #[test]
    fn one_build_number_on_two_versions_is_two_releases() {
        let chosen = vec![
            candidate("eu.wowsreplay", header("14.3.0", 300), "02.04.2025 18:00:00"),
            candidate("cn.wowsreplay", header("14.2.0", 300), "02.03.2025 18:00:00"),
        ];
        let local: BTreeSet<Release> = [release("14.3.0", 300)].into();

        let summary = summarize(&chosen, &local, &published(&[]));

        assert_eq!(summary.replays, [(PathBuf::from("eu.wowsreplay"), release("14.3.0", 300))]);
        assert_eq!(summary.unobtainable, [BuildNeed { release: release("14.2.0", 300), replays: 1 }]);
    }

    fn resolved(build: u32, version: &str, availability: RemoteAvailability) -> ResolvedBuild {
        ResolvedBuild { requested_build: build, requested_version: Some(version.to_owned()), availability }
    }

    /// The repository's answer is matched on build and version, so the
    /// release it did not publish is not promised a download because another
    /// release shares its build number.
    #[test]
    fn a_published_release_is_matched_on_build_and_version() {
        let missing = [release("14.3.0", 300), release("14.2.0", 300)];
        let answer = [
            resolved(300, "14.3.0", RemoteAvailability::Exact),
            resolved(300, "14.2.0", RemoteAvailability::Unpublished),
        ];

        let fetchable = fetchable_from_plan(&missing, &answer).expect("the repository answered");

        assert_eq!(fetchable, [release("14.3.0", 300)].into());
    }

    /// A release the repository could not be asked about stops the run
    /// rather than being reported as unobtainable.
    #[test]
    fn an_unreachable_release_fails_the_answer() {
        let missing = [release("14.3.0", 300), release("13.5.1", 200)];
        let answer = [
            resolved(300, "14.3.0", RemoteAvailability::Exact),
            resolved(200, "13.5.1", RemoteAvailability::Unreachable),
        ];

        let refused = fetchable_from_plan(&missing, &answer).expect_err("one release could not be asked about");

        assert_eq!(refused.releases, [release("13.5.1", 200)]);
    }

    #[test]
    fn a_small_readable_selection_runs_unasked_and_a_large_one_does_not() {
        let local: BTreeSet<Release> = [release("14.3.0", 300)].into();
        let small: Vec<ReindexCandidate> = listing().into_iter().take(2).collect();
        let large: Vec<ReindexCandidate> = (0..51)
            .map(|ix| candidate(&format!("{ix}.wowsreplay"), header("14.3.0", 300), "02.04.2025 18:00:00"))
            .collect();

        let small = summarize(&small, &local, &published(&[]));
        let large = summarize(&large, &local, &published(&[]));

        assert!(small.runs_unasked(Confirm::WhenNotPlain), "nothing to fetch and nothing left out needs no question");
        assert!(!small.runs_unasked(Confirm::Always));
        assert!(large.is_plain());
        assert!(!large.runs_unasked(Confirm::WhenNotPlain), "a large choice is put to the reader");
    }

    #[test]
    fn a_failed_download_drops_only_its_replays() {
        let local: BTreeSet<Release> = [release("14.3.0", 300)].into();
        let fetchable = published(&[release("13.5.1", 200), release("12.0.0", 100)]);
        let summary = summarize(&listing(), &local, &fetchable);

        let (kept, dropped) = summary.readable_after(&[release("12.0.0", 100)].into());

        assert_eq!(dropped, 2);
        assert_eq!(kept, [PathBuf::from("a.wowsreplay"), PathBuf::from("b.wowsreplay"), PathBuf::from("c.wowsreplay")]);
    }

    #[test]
    fn versions_are_listed_newest_first_with_their_counts() {
        let listed = versions(&listing());

        let said: Vec<(String, usize)> =
            listed.iter().map(|count| (version_said(&count.version), count.replays)).collect();
        // 13.5.1 sorts above 12.0.0 and 0.11.0 below both, which string order
        // would get wrong. The version with no build cannot be re-indexed and
        // is not counted.
        assert_eq!(
            said,
            [("14.3.0".to_owned(), 2), ("13.5.1".to_owned(), 1), ("12.0.0".to_owned(), 2), ("0.11.0".to_owned(), 1)]
        );
    }

    #[test]
    fn a_version_scope_keeps_every_build_of_that_version() {
        let mut listed = listing();
        listed.push(candidate("i.wowsreplay", header("14.3.0", 301), "03.04.2025 18:00:00"));

        let chosen = ReindexScope::Version(Version::base(14, 3, 0)).select(listed);

        let names: Vec<&str> = chosen.iter().map(|candidate| candidate.path.to_str().expect("ascii")).collect();
        assert_eq!(names, ["a.wowsreplay", "b.wowsreplay", "i.wowsreplay"]);
    }

    #[test]
    fn a_header_tells_no_version_from_no_build() {
        assert_eq!(HeaderRelease::from_version(None), HeaderRelease::NoVersion);
        assert_eq!(HeaderRelease::from_version(header("14.3.0", 0)), HeaderRelease::NoBuild);
        assert_eq!(candidate("x.wowsreplay", None, "yesterday").played_at, None);
    }
}
