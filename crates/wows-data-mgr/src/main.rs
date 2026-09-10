use clap::Parser;
use clap::Subcommand;
use rootcause::prelude::*;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

mod detect;
mod download;
mod steam;

use wows_data_mgr::dump;
use wows_data_mgr::manifest;
use wows_data_mgr::registry;

/// Problem paths described in full per build before the rest are summarised.
/// A build can hit hundreds of them and each message names files.
const NAMED_PROBLEM_PATHS: usize = 3;

#[derive(Parser)]
#[command(name = "wows-data-mgr", about = "Download and manage World of Warships game data")]
struct Args {
    /// Override the game data directory (default: game_data/ in repo root)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    /// Skip materializing the newly dumped build as a symlink tree. The dump
    /// is read through metadata.toml and the shared store either way; the tree
    /// is there for humans browsing it.
    #[arg(long, global = true)]
    no_link: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Clone)]
enum Commands {
    /// Download game data for a specific version via DepotDownloader
    Download {
        /// Download the newest build listed in game_versions.toml
        #[arg(long, conflicts_with_all = &["build", "version"])]
        latest: bool,

        /// Download by build number (e.g. 11965230)
        #[arg(long, conflicts_with_all = &["latest", "version"])]
        build: Option<u32>,

        /// Download by version string (e.g. 15.1 or 15.1.0)
        #[arg(long, conflicts_with_all = &["latest", "build"])]
        version: Option<String>,

        /// Force re-download even if already present
        #[arg(long)]
        force: bool,

        /// Steam username (otherwise reads from .steam-user)
        #[arg(long)]
        username: Option<String>,
    },

    /// List known game versions and their download status
    List,

    /// Detect game versions from downloaded/installed data
    Detect {
        /// Path to scan (default: game_data/builds/)
        path: Option<PathBuf>,
    },

    /// Dump renderer-required game data to a directory for offline use
    DumpRendererData {
        /// Dump the newest build available locally, which need not be one
        /// game_versions.toml already lists
        #[arg(long, conflicts_with_all = &["build", "version"])]
        latest: bool,

        /// Dump by build number (e.g. 11965230)
        #[arg(long, conflicts_with_all = &["latest", "version"])]
        build: Option<u32>,

        /// Dump by version string (e.g. 15.1 or 15.1.0)
        #[arg(long, conflicts_with_all = &["latest", "build"])]
        version: Option<String>,

        /// Output directory (a subdirectory named <version>_<build> will be created)
        #[arg(short, long)]
        output: PathBuf,

        /// Overwrite existing dump for this build
        #[arg(long)]
        force: bool,

        /// Skip refreshing this build's depot pins in game_versions.toml from
        /// the Steam public branch after the dump.
        #[arg(long)]
        no_manifest_update: bool,

        /// Override the game data source with this game install directory.
        /// May be combined with any selector.
        #[arg(long)]
        game_dir: Option<PathBuf>,
    },

    /// Remove a previously dumped build, cleaning up deduplicated storage
    Remove {
        /// Remove by build number
        #[arg(long, conflicts_with = "version")]
        build: Option<u32>,

        /// Remove all builds matching a version string (e.g. 15.1 or 15.1.0)
        #[arg(long, conflicts_with = "build")]
        version: Option<String>,

        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Regenerate derived artifacts (rkyv blob, compressed copies) with GC
    /// enabled by default.
    RefreshDerived {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,

        /// Refresh only this build number (default: all builds)
        #[arg(long)]
        build: Option<u32>,

        /// Skip the automatic post-refresh garbage collection. Orphaned CAS
        /// objects (typically previous versions of replaced rkyv/zst blobs)
        /// stay on disk until `wows-data-mgr gc` runs.
        #[arg(long)]
        no_gc: bool,
    },

    /// Index content a build tree holds that `metadata.toml` never recorded.
    /// Symlinks pointing to an invalid object are skipped.
    Reindex {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,

        /// Reindex only this build number (default: all builds)
        #[arg(long)]
        build: Option<u32>,
    },

    /// Re-materializes a build's filesystem from the `metadata.toml`. i.e.
    /// fixes up the filesystem under a build path.
    Relink {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,

        /// Relink only this build number (default: all builds)
        #[arg(long)]
        build: Option<u32>,
    },

    /// Delete the materialized symlink trees in build directories. Readers
    /// resolve every file through metadata.toml and common/, so the trees are a
    /// browsing convenience. Never removes content objects.
    PruneMaterialized {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,

        /// Only prune this build (default: every build in the base)
        #[arg(long)]
        build: Option<u32>,
    },

    /// Delete content-addressed objects no longer referenced by any dumped
    /// build.
    Gc {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Print the VFS path globs the dump extracts, one per line. Feed these to
    /// `wowsunpack pkgs` to resolve the minimal set of .pkg files to download.
    RequiredPaths,

    /// Add missing assets (maps, and with --with-gui the gui/ dirs) to an
    /// existing build without re-extracting data it already has.
    CompleteBuild {
        /// Build number to complete (must already exist in builds.toml)
        #[arg(long)]
        build: u32,

        /// Game install directory holding bin/<build>/idx and res_packages
        #[arg(long)]
        game_dir: PathBuf,

        /// Output directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,

        /// Also re-extract the gui/ asset dirs (ribbons, achievements, flags, ...)
        #[arg(long)]
        with_gui: bool,
    },

    /// Fold a legacy `vfs_common/` store into `common/` and relink every build.
    MigrateCas {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Verify that every build in a dump base is internally consistent: its
    /// metadata parses and every referenced content object exists in common/,
    /// and with --check-hashes that each object's bytes still hash to its name.
    /// Exits non-zero if any build is broken.
    Verify {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,

        /// Also check that each materialized symlink resolves to a readable
        /// file. A build whose metadata does not claim a tree is complete and
        /// is skipped.
        #[arg(long)]
        check_links: bool,

        /// Also read every referenced object and check its bytes against its
        /// name, catching content that was rewritten in place
        #[arg(long)]
        check_hashes: bool,
    },

    /// Copy dumped builds from a local source dump base into a destination
    /// (e.g. the toolkit's data cache), deduplicating against content already
    /// present.
    Update {
        /// Source dump base to copy from (must contain builds.toml and common/)
        #[arg(long)]
        from: PathBuf,

        /// Destination dump base (the toolkit's data cache)
        #[arg(short, long)]
        output: PathBuf,

        /// Copy only the latest build in the source
        #[arg(long, conflicts_with_all = &["build", "version"])]
        latest: bool,

        /// Copy a single build number
        #[arg(long, conflicts_with_all = &["latest", "version"])]
        build: Option<u32>,

        /// Copy all builds matching a version string (e.g. 15.1 or 15.1.0)
        #[arg(long, conflicts_with_all = &["latest", "build"])]
        version: Option<String>,

        /// Re-copy even if the destination already has the build
        #[arg(long)]
        force: bool,
    },

    /// Refresh game_versions.toml with the depot manifests the Steam public
    /// branch currently publishes.
    ///
    /// The build number is read back out of the client depot's file listing, so
    /// an entry is only written for the build those manifests provably
    /// introduce. Listing a depot needs a Steam account that owns the game.
    UpdateVersions {
        /// Game version the published build is (e.g. 15.7.0). Defaults to what
        /// the local registry records for that build.
        #[arg(long)]
        version: Option<String>,

        /// Re-pin a build game_versions.toml already records. Entries name the
        /// manifest a build was first introduced under, so this replaces that
        /// record with whatever the branch publishes now.
        #[arg(long)]
        force: bool,

        /// Steam username to query as (otherwise reads from .steam-user)
        #[arg(long)]
        steam_user: Option<String>,
    },

    /// Register an existing WoWs installation without downloading
    Register {
        /// Register as the "latest" path — always use whatever builds exist here
        #[arg(long, conflicts_with_all = &["version", "build"])]
        latest: bool,

        /// Version string (e.g. 15.1 or 15.1.0)
        #[arg(long, conflicts_with = "build")]
        version: Option<String>,

        /// Build number (e.g. 11965230)
        #[arg(long, conflicts_with = "version")]
        build: Option<u32>,

        /// Path to the WoWs installation directory
        #[arg(long, required = true)]
        path: PathBuf,
    },
}

/// The dump base and the one build a command created, which is what gets
/// materialized unless `--no-link` says otherwise. Naming a concrete build is
/// what keeps the link step off every other build in the base. Commands that
/// only touch existing builds report nothing: each keeps the materialization
/// its own metadata records, and pruning exists to remove a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LinkTarget {
    output: PathBuf,
    build: u32,
}

fn find_repo_root() -> Result<PathBuf, Report> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("game_versions.toml").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("Could not find repo root (no game_versions.toml found in parent directories)");
        }
    }
}

fn resolve_data_dir(args_data_dir: &Option<PathBuf>) -> Result<PathBuf, Report> {
    if let Some(dir) = args_data_dir {
        Ok(dir.clone())
    } else {
        let repo_root = find_repo_root()?;
        Ok(repo_root.join("game_data"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DumpDataSource {
    Supplied(PathBuf),
    Registry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingDumpPolicy {
    Preserve,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DumpVersionPlan {
    UseManifest(String),
    ValidateSourceThenUseManifest(String),
    DetectFromSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DumpRendererPlan {
    target: u32,
    version: DumpVersionPlan,
    source: DumpDataSource,
    output: PathBuf,
    existing_dump: ExistingDumpPolicy,
}

/// Which build a dump was asked for. The three selectors are mutually
/// exclusive at the CLI, so they are one choice rather than three flags that
/// could all be set at once.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DumpSelector<'a> {
    /// The newest build the source holds.
    Latest,
    Build(u32),
    Version(&'a str),
}

impl<'a> DumpSelector<'a> {
    fn from_args(latest: bool, build: Option<u32>, version: Option<&'a str>) -> Result<Self, Report> {
        match (latest, build, version) {
            (true, _, _) => Ok(Self::Latest),
            (false, Some(build), _) => Ok(Self::Build(build)),
            (false, None, Some(version)) => Ok(Self::Version(version)),
            (false, None, None) => bail!("Specify --latest, --build, or --version"),
        }
    }
}

/// What a dump was asked to produce, before anything is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DumpRequest<'a> {
    selector: DumpSelector<'a>,
    game_dir: Option<&'a Path>,
    output: &'a Path,
    force: bool,
}

fn resolve_dump_renderer_plan(
    manifest: &manifest::GameVersionManifest,
    request: &DumpRequest<'_>,
    local_builds: &[u32],
) -> Result<DumpRendererPlan, Report> {
    let target = match request.selector {
        // The newest build present, not the newest the manifest knows: a dump
        // reads local data, and a build too new to be pinned yet is exactly the
        // one worth dumping. Resolving through the manifest instead only ever
        // named builds this then failed to find locally.
        DumpSelector::Latest => local_builds.iter().copied().max().ok_or_else(|| {
            rootcause::report!(
                "No game builds available locally. Pass --game-dir <install>, or register one with \
                 `register --latest <path>`."
            )
        })?,
        DumpSelector::Build(build) => build,
        DumpSelector::Version(version) => manifest
            .find_by_version(version)
            .ok_or_else(|| rootcause::report!("No build found matching version '{version}'"))?,
    };

    let manifest_version = manifest.get(target).map(|entry| entry.version.clone());
    let version = match (manifest_version, request.force) {
        (Some(version), true) => DumpVersionPlan::ValidateSourceThenUseManifest(version),
        (Some(version), false) => DumpVersionPlan::UseManifest(version),
        (None, _) => DumpVersionPlan::DetectFromSource,
    };

    Ok(DumpRendererPlan {
        target,
        version,
        source: request.game_dir.map_or(DumpDataSource::Registry, |path| DumpDataSource::Supplied(path.to_path_buf())),
        output: request.output.to_path_buf(),
        existing_dump: if request.force { ExistingDumpPolicy::Replace } else { ExistingDumpPolicy::Preserve },
    })
}

impl DumpRendererPlan {
    /// What this dump materializes: the build the selector resolved to, never
    /// the raw `--build` argument, which is absent for `--latest`/`--version`.
    fn link_target(&self) -> LinkTarget {
        LinkTarget { output: self.output.clone(), build: self.target }
    }
}

/// The builds a dump could read, which is what `--latest` picks from.
fn local_builds_for_dump(game_dir: Option<&Path>, data_dir: &Path) -> Result<Vec<u32>, Report> {
    if let Some(dir) = game_dir {
        return Ok(wowsunpack::game_data::list_available_builds(dir)
            .attach_with(|| format!("No valid game builds found at {}", dir.display()))?);
    }

    let mut builds = registry::load_registry(&data_dir.join("versions.toml")).available_builds();
    // `game_dir_for_build` falls back to this directory for a build with no
    // registry entry, so a build downloaded before a lost registry write is
    // dumpable by number. `--latest` has to see the same set or it would not
    // offer one it can already dump.
    builds.extend(downloaded_build_dirs(&data_dir.join("builds")));
    builds.sort_unstable();
    builds.dedup();
    Ok(builds)
}

/// Build numbers named by a directory under `builds/`.
fn downloaded_build_dirs(builds_dir: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(builds_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .collect()
}

/// Returns the version the dump was written under.
fn execute_dump_renderer_plan(plan: DumpRendererPlan, data_dir: &Path) -> Result<String, Report> {
    let game_dir = match plan.source {
        DumpDataSource::Supplied(path) => path,
        DumpDataSource::Registry => {
            let registry = registry::load_registry(&data_dir.join("versions.toml"));
            registry
                .game_dir_for_build(plan.target, data_dir)
                .ok_or_else(|| rootcause::report!("Build {} not available locally", plan.target))?
        }
    };
    let version = match plan.version {
        DumpVersionPlan::UseManifest(version) => version,
        DumpVersionPlan::ValidateSourceThenUseManifest(version) => {
            detect_source_version(&game_dir, plan.target)?;
            version
        }
        DumpVersionPlan::DetectFromSource => detect_source_version(&game_dir, plan.target)?,
    };

    if plan.existing_dump == ExistingDumpPolicy::Replace {
        remove_existing_dump(&plan.output, plan.target, &version)?;
    }

    println!("Dumping build {} ({version}) from {}", plan.target, game_dir.display());
    let progress = dump::create_progress_bar(&game_dir);
    dump::dump_renderer_data(&game_dir, plan.target, &version, &plan.output, progress.as_ref(), false)?;
    println!("Dumped renderer data to {}", dump::dump_dir(&plan.output, &version, plan.target).display());
    Ok(version)
}

fn detect_source_version(game_dir: &Path, target: u32) -> Result<String, Report> {
    detect::detect_version_at_path(game_dir, target)
        .attach_with(|| format!("Could not detect version for build {target} at {}", game_dir.display()))
}

fn remove_existing_dump(output: &Path, target: u32, version: &str) -> Result<(), Report> {
    let builds_path = output.join("builds.toml");
    let mut index = wows_data_mgr::builds::BuildsIndex::load(&builds_path);
    if let Some(old_entry) = index.find_by_build(target).cloned() {
        let old_dir = output.join(&old_entry.dir);
        if old_dir.exists() {
            println!("Removing old dump at {}...", old_dir.display());
            std::fs::remove_dir_all(&old_dir)?;
        }
        index.remove_build(target);
        index.save(&builds_path)?;
    }

    let existing_dir = dump::dump_dir(output, version, target);
    if existing_dir.exists() {
        println!("Removing existing dump at {}...", existing_dir.display());
        std::fs::remove_dir_all(&existing_dir)?;
    }
    Ok(())
}

/// Where a refresh's version label comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VersionSource {
    /// Read out of the game data that was just dumped.
    Detected(String),
    /// Typed on the command line, so it is a claim to check rather than a fact.
    Asserted(String),
    /// Whatever the local registry records for the build.
    Registry,
}

/// What a manifest refresh needs that the caller already knows.
struct ManifestRefresh<'a> {
    repo_root: &'a Path,
    login: steam::SteamLogin,
    prompt: steam::LoginPrompt,
    /// The build the caller already dumped. Steam must agree its manifests
    /// introduce that build, or nothing is written.
    expected_build: Option<u32>,
    version: VersionSource,
    /// Re-pin a build the manifest already records. Entries name the manifest a
    /// build was first introduced under, and a depot re-push moves the current
    /// pins without moving the build, so refreshing one by default would
    /// overwrite that record with a later manifest.
    overwrite: ExistingPinPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingPinPolicy {
    Keep,
    Replace,
}

/// What a refresh did.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RefreshOutcome {
    Pinned { build: u32, version: String },
    AlreadyPinned { build: u32, version: String },
}

impl RefreshOutcome {
    fn report(&self) -> String {
        match self {
            Self::Pinned { build, version } => format!("Pinned build {build} ({version}) in game_versions.toml"),
            Self::AlreadyPinned { build, version } => {
                format!("Build {build} ({version}) is already pinned in game_versions.toml")
            }
        }
    }
}

/// Settle the version label for a build, rejecting a claim the registry
/// contradicts. Nothing else checks a typed `--version`, and it is written
/// verbatim.
fn resolve_refresh_version(
    source: &VersionSource,
    build: u32,
    reg: &registry::LocalRegistry,
) -> Result<String, Report> {
    let known = reg.get(build).map(|entry| entry.version.as_str());
    match source {
        VersionSource::Detected(version) => Ok(version.clone()),
        VersionSource::Asserted(version) => match known {
            Some(known) if known != version => bail!(
                "build {build} is version {known} locally, not {version}; pass the version that build actually is"
            ),
            _ => Ok(version.clone()),
        },
        VersionSource::Registry => known.map(str::to_string).ok_or_else(|| {
            rootcause::report!(
                "Build {build} is not in the local registry; pass --version to say which game version it is"
            )
        }),
    }
}

/// Reconcile the build Steam publishes with the one the caller expected.
///
/// The client depot ships more than one `bin/<build>`: the live build plus the
/// one it replaced, which the game retains. Only the newest is the build that
/// manifest introduces, which is what an entry here records, so membership is
/// not enough. Pinning a retained older build would file the current release's
/// manifest IDs under the previous release's number.
fn resolve_published_build(published: &BTreeSet<u32>, expected: Option<u32>) -> Result<u32, Report> {
    let newest = *published
        .iter()
        .next_back()
        .ok_or_else(|| rootcause::report!("the Steam public branch client depot holds no build directory"))?;

    match expected {
        Some(build) if build == newest => Ok(build),
        Some(build) => bail!(
            "the Steam public branch introduces build {newest}, not {build} (its client depot holds {}). Its \
             manifests pin {newest}, so recording them under {build} would misattribute them.",
            published.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
        ),
        None => Ok(newest),
    }
}

/// A build the manifest already pins, when the caller named one up front.
///
/// Checking before any Steam query is what keeps an archival dump of an old
/// build from paying for an app-info call and a full client depot listing only
/// to be told its build is not the one being published.
fn already_pinned(refresh: &ManifestRefresh<'_>, pinned: &manifest::GameVersionManifest) -> Option<RefreshOutcome> {
    if refresh.overwrite == ExistingPinPolicy::Replace {
        return None;
    }
    let build = refresh.expected_build?;
    let entry = pinned.get(build)?;
    Some(RefreshOutcome::AlreadyPinned { build, version: entry.version.clone() })
}

/// Pin a build's depot manifests in `game_versions.toml` from the public branch.
fn refresh_manifest_entry(
    refresh: &ManifestRefresh<'_>,
    pinned: &manifest::GameVersionManifest,
    reg: &registry::LocalRegistry,
) -> Result<RefreshOutcome, Report> {
    if let Some(outcome) = already_pinned(refresh, pinned) {
        return Ok(outcome);
    }

    let steamroom = steam::find_steamroom()?;
    let depots = steam::fetch_public_manifests(steamroom, manifest::WOWS_APP_ID)?;
    let client = depots
        .get(&manifest::CLIENT_DEPOT_ID)
        .map(|id| manifest::DepotManifest::new(manifest::CLIENT_DEPOT_ID, id.clone()))
        .ok_or_else(|| {
            rootcause::report!("The Steam public branch publishes no client depot for app {}", manifest::WOWS_APP_ID.0)
        })?;

    let published = steam::discover_builds(steamroom, manifest::WOWS_APP_ID, &client, &refresh.login, refresh.prompt)?;
    let build = resolve_published_build(&published, refresh.expected_build)?;

    let version = resolve_refresh_version(&refresh.version, build, reg)?;
    if refresh.overwrite == ExistingPinPolicy::Keep
        && let Some(entry) = pinned.get(build)
    {
        return Ok(RefreshOutcome::AlreadyPinned { build, version: entry.version.clone() });
    }

    let entry = steam::entry_from_public_manifests(manifest::WOWS_APP_ID, &version, &depots)?;
    manifest::save_entry(&refresh.repo_root.join("game_versions.toml"), build, &entry)?;
    Ok(RefreshOutcome::Pinned { build, version })
}

fn unknown_build_manifest_entry(build: u32, version: &str) -> String {
    format!(
        "[versions.{build}]\nversion = \"{version}\"\nclient_depot_id = 552993\nclient_manifest_id = \"<look up on SteamDB>\""
    )
}

fn main() -> Result<(), Report> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();
    let repo_root = find_repo_root()?;
    let data_dir = resolve_data_dir(&args.data_dir)?;

    let created = run(&args, &repo_root, &data_dir)?;

    if !args.no_link
        && let Some(target) = created
    {
        let repointed = dump::relink_builds(&target.output, Some(target.build), dump::TreeSync::AddAndRepoint)?;
        let total: usize = repointed.values().map(Vec::len).sum();
        println!("Materialized {total} link(s) across {} build(s).", repointed.len());
    }
    Ok(())
}

fn run(args: &Args, repo_root: &Path, data_dir: &Path) -> Result<Option<LinkTarget>, Report> {
    // These commands don't need the version manifest, so handle them before
    // loading it (a malformed game_versions.toml must not block them).
    match &args.command {
        Commands::RefreshDerived { output, build, no_gc } => {
            println!("Refreshing derived data...");
            let summary = dump::refresh_derived(output, *build)?;
            if *no_gc {
                println!("Skipping garbage collection (--no-gc).");
            } else if summary.safe_to_gc() {
                println!("Garbage-collecting orphaned CAS objects...");
                dump::gc_cas(output)?;
            } else {
                let damaged: usize = summary.damaged.values().map(Vec::len).sum();
                println!(
                    "Skipping garbage collection: {} build(s) reference {damaged} object(s) missing from the store, \
                     {} build(s) failed to refresh, and {} build(s) have a vfs/ tree their metadata does not record. \
                     Run `relink` to record and repoint those trees, `verify --check-hashes` to inspect the rest, \
                     restore the store, then `gc`.",
                    summary.damaged.len(),
                    summary.failed.len(),
                    summary.stale_trees.len()
                );
            }
            return Ok(None);
        }
        Commands::Reindex { output, build } => {
            let reports = dump::reindex_builds(output, *build)?;
            if reports.is_empty() {
                println!("Every file in every build tree is already indexed.");
                return Ok(None);
            }
            for (dir, report) in &reports {
                println!("  {dir} - indexed {} file(s), {} unbacked", report.added.len(), report.unbacked.len());
                for path in report.unbacked.iter().take(NAMED_PROBLEM_PATHS) {
                    println!("         unbacked: {path}");
                }
                let rest = report.unbacked.len().saturating_sub(NAMED_PROBLEM_PATHS);
                if rest > 0 {
                    println!("         and {rest} more unbacked file(s)");
                }
            }
            let added: usize = reports.values().map(|r| r.added.len()).sum();
            println!("Indexed {added} file(s) across {} build(s).", reports.len());
            return Ok(None);
        }
        Commands::Relink { output, build } => {
            let repointed = dump::relink_builds(output, *build, dump::TreeSync::Full)?;
            if repointed.is_empty() {
                println!("Every build tree already matches its metadata.");
                return Ok(None);
            }
            for (dir, paths) in &repointed {
                println!("  {dir} - repointed {} link(s)", paths.len());
            }
            let total: usize = repointed.values().map(Vec::len).sum();
            println!("Repointed {total} link(s) across {} build(s).", repointed.len());
            return Ok(None);
        }
        Commands::PruneMaterialized { output, build } => {
            let pruned = dump::prune_materialized_trees(output, *build)?;
            if pruned.is_empty() {
                println!("No materialized trees to prune.");
                return Ok(None);
            }
            for (dir, removed) in &pruned {
                println!("  {dir} - removed {removed} link(s)");
            }
            let total: usize = pruned.values().sum();
            println!("Removed {total} link(s) across {} build(s).", pruned.len());
            return Ok(None);
        }
        Commands::Gc { output } => {
            println!("Garbage-collecting orphaned CAS objects...");
            dump::gc_cas(output)?;
            return Ok(None);
        }
        Commands::RequiredPaths => {
            for glob in dump::required_path_globs() {
                println!("{glob}");
            }
            return Ok(None);
        }
        Commands::CompleteBuild { build, game_dir, output, with_gui } => {
            println!("Completing build {build} from {} (with_gui={with_gui})...", game_dir.display());
            let map_count = dump::complete_build(game_dir, *build, output, *with_gui)?;
            println!("Done: extracted {map_count} map(s) and regenerated derived data.");
            return Ok(None);
        }
        Commands::MigrateCas { output } => {
            println!("Merging vfs_common/ into common/ and relinking builds in {}...", output.display());
            let migrated = dump::migrate_cas_dir_name(output)?;
            if migrated {
                println!("Done. Run `verify` to confirm consistency.");
            } else {
                println!("Nothing to migrate (no vfs_common/ present).");
            }
            return Ok(None);
        }
        Commands::Verify { output, check_links, check_hashes } => {
            let reports = dump::verify_builds(output, *check_links, *check_hashes)?;
            if reports.is_empty() {
                println!("No builds found in {}", output.display());
                return Ok(None);
            }
            let mut broken = 0;
            let mut corrupt_hashes = std::collections::BTreeSet::new();
            for r in &reports {
                if r.is_ok() {
                    println!("  OK   {} ({} objects)", r.dir, r.referenced);
                } else {
                    broken += 1;
                    if r.metadata_unreadable {
                        println!("  FAIL {} - metadata.toml unreadable", r.dir);
                    } else {
                        // Without --check-hashes nothing read the bytes, so
                        // "0 corrupt" would assert an audit that never ran.
                        // That is the reassurance 77 corrupt objects hid
                        // behind for months.
                        let corrupt = if *check_hashes {
                            format!("{} corrupt", r.corrupt_objects.len())
                        } else {
                            "hashes not checked".to_string()
                        };
                        println!(
                            "  FAIL {} - {}/{} objects missing, {corrupt}, {} broken link(s), {} un-indexed file(s)",
                            r.dir,
                            r.missing_objects.len(),
                            r.referenced,
                            r.broken_links.len(),
                            r.unindexed.len()
                        );
                        for path in r.unindexed.iter().take(NAMED_PROBLEM_PATHS) {
                            println!("         un-indexed: {path}");
                        }
                        let rest = r.unindexed.len().saturating_sub(NAMED_PROBLEM_PATHS);
                        if rest > 0 {
                            println!("         and {rest} more un-indexed file(s)");
                        }
                        for corrupt in r.corrupt_objects.iter().take(NAMED_PROBLEM_PATHS) {
                            println!("         {corrupt}");
                        }
                        let rest = r.corrupt_objects.len().saturating_sub(NAMED_PROBLEM_PATHS);
                        if rest > 0 {
                            println!("         and {rest} more corrupt object(s)");
                        }
                    }
                }
                corrupt_hashes.extend(r.corrupt_objects.iter().map(|c| c.hash.clone()));
            }
            let ok = reports.len() - broken;
            println!("\n{ok}/{} builds consistent.", reports.len());
            if !corrupt_hashes.is_empty() {
                println!(
                    "{} distinct corrupt object(s) across the store; re-publishing the affected builds is the \
                     only fix.",
                    corrupt_hashes.len()
                );
            }
            if broken > 0 {
                bail!("{broken} build(s) inconsistent");
            }
            return Ok(None);
        }
        Commands::Update { from, output, latest, build, version, force } => {
            let selector = if *latest {
                dump::SyncSelector::Latest
            } else if let Some(b) = build {
                dump::SyncSelector::Build(*b)
            } else if let Some(v) = version {
                dump::SyncSelector::Version(v.clone())
            } else {
                dump::SyncSelector::All
            };

            println!("Syncing from {} into {}...", from.display(), output.display());
            let synced = dump::sync_from_local(from, output, &selector, *force)?;
            for s in &synced {
                let status = if s.copied { "copied" } else { "already present" };
                println!("  {} (build {}) - {status}", s.version, s.build);
            }
            let copied = synced.iter().filter(|s| s.copied).count();
            println!("Done: {copied} copied, {} up to date.", synced.len() - copied);
            return Ok(None);
        }
        _ => {}
    }

    let manifest = manifest::load_manifest(&repo_root.join("game_versions.toml"))?;
    if let Commands::DumpRendererData { latest, build, version, output, force, game_dir, no_manifest_update } =
        &args.command
    {
        let local_builds = local_builds_for_dump(game_dir.as_deref(), data_dir)?;
        let request = DumpRequest {
            selector: DumpSelector::from_args(*latest, *build, version.as_deref())?,
            game_dir: game_dir.as_deref(),
            output,
            force: *force,
        };
        let plan = resolve_dump_renderer_plan(&manifest, &request, &local_builds)?;
        let target = plan.link_target();
        let dumped_version = execute_dump_renderer_plan(plan, data_dir)?;

        if !*no_manifest_update {
            let refresh = ManifestRefresh {
                repo_root,
                login: steam::resolve_login(repo_root, None),
                // A password prompt here would stall a dump that has already
                // done its work.
                prompt: steam::LoginPrompt::Deny,
                expected_build: Some(target.build),
                version: VersionSource::Detected(dumped_version),
                overwrite: ExistingPinPolicy::Keep,
            };
            let reg = registry::load_registry(&data_dir.join("versions.toml"));
            match refresh_manifest_entry(&refresh, &manifest, &reg) {
                Ok(outcome) => println!("{}", outcome.report()),
                Err(e) => eprintln!(
                    "Warning: left game_versions.toml unchanged, could not verify the Steam pins for build {}: {e}",
                    target.build
                ),
            }
        }
        return Ok(Some(target));
    }

    let mut reg = registry::load_registry(&data_dir.join("versions.toml"));

    match args.command.clone() {
        Commands::Download { latest, build, version, force, username } => {
            let target = if latest {
                manifest.latest_build().ok_or_else(|| rootcause::report!("No versions in game_versions.toml"))?
            } else if let Some(b) = build {
                b
            } else if let Some(ref v) = version {
                manifest
                    .find_by_version(v)
                    .ok_or_else(|| rootcause::report!("No build found matching version '{v}'"))?
            } else {
                bail!("Specify --latest, --build, or --version");
            };

            if !force && reg.has_build(target) {
                println!("Build {target} already available. Use --force to re-download.");
                return Ok(None);
            }

            let entry = manifest.get(target);
            download::download_build(target, entry, data_dir, repo_root, username.as_deref())?;

            let version_str = detect::detect_version_for_build(data_dir, target)?;
            reg.set_downloaded(target, &version_str);
            registry::save_registry(&reg, &data_dir.join("versions.toml"))?;

            if entry.is_none() {
                println!();
                println!("This build is not in game_versions.toml. Add it with:");
                println!();
                println!("{}", unknown_build_manifest_entry(target, &version_str));
            }
        }

        Commands::List => {
            if let Some(ref latest) = reg.latest_path {
                println!("Latest path: {}", latest.display());
                if let Ok(builds) = wowsunpack::game_data::list_available_builds(latest) {
                    println!("  builds: {:?}", builds);
                }
                println!();
            }

            println!("{:<12} {:<10} {:<24} STATUS", "BUILD", "VERSION", "MANIFEST");
            println!("{}", "-".repeat(72));

            let mut builds: Vec<_> = manifest.versions.keys().collect();
            builds.sort();

            for build_str in builds {
                let entry = &manifest.versions[build_str];
                let build: u32 = build_str.parse().unwrap_or(0);
                // Content represents game data; client is required when content is absent.
                let display_manifest = entry.content.as_ref().unwrap_or(&entry.client);
                let status = if let Some(local) = reg.get(build) {
                    if let Some(ref path) = local.path {
                        format!("{} (registered)", path.display())
                    } else if let Some(ref ts) = local.downloaded_at {
                        format!("downloaded ({ts})")
                    } else {
                        "downloaded".to_string()
                    }
                } else {
                    "not available".to_string()
                };

                println!("{:<12} {:<10} {:<24} {}", build_str, entry.version, display_manifest.manifest_id.0, status);
            }

            // Also show registry entries not in the manifest
            for (build_str, local) in &reg.builds {
                if !manifest.versions.contains_key(build_str) {
                    let status = if let Some(ref path) = local.path {
                        format!("{} (registered)", path.display())
                    } else {
                        "downloaded (not in manifest)".to_string()
                    };
                    println!("{:<12} {:<10} {:<24} {}", build_str, local.version, "-", status);
                }
            }
        }

        Commands::UpdateVersions { version, steam_user, force } => {
            let refresh = ManifestRefresh {
                repo_root,
                login: steam::resolve_login(repo_root, steam_user.as_deref()),
                // Run on purpose, so logging in is part of the job.
                prompt: steam::LoginPrompt::Allow,
                expected_build: None,
                version: version.map_or(VersionSource::Registry, VersionSource::Asserted),
                overwrite: if force { ExistingPinPolicy::Replace } else { ExistingPinPolicy::Keep },
            };
            println!("{}", refresh_manifest_entry(&refresh, &manifest, &reg)?.report());
        }

        Commands::Detect { path } => {
            let scan_path = path.unwrap_or_else(|| data_dir.join("builds"));
            let detected = detect::detect_all_versions(&scan_path)?;
            if detected.is_empty() {
                println!("No game builds found in {}", scan_path.display());
            } else {
                for (build, version) in &detected {
                    println!("Build {build}: version {version}");
                    reg.set_downloaded(*build, version);
                }
                registry::save_registry(&reg, &data_dir.join("versions.toml"))?;
                println!("\nRegistry updated.");
            }
        }

        Commands::Remove { build, version, output } => {
            let index = wows_data_mgr::builds::BuildsIndex::load(&output.join("builds.toml"));

            if let Some(target_build) = build {
                println!("Removing build {target_build}...");
                dump::remove_build(&output, target_build)?;
                println!("Build {target_build} removed.");
            } else if let Some(ref version_query) = version {
                let matches = index.find_by_version(version_query);
                if matches.is_empty() {
                    bail!("No builds found matching version '{version_query}'");
                }
                let builds_to_remove: Vec<u32> = matches.iter().map(|e| e.build).collect();
                for b in &builds_to_remove {
                    println!("Removing build {b}...");
                    dump::remove_build(&output, *b)?;
                    println!("Build {b} removed.");
                }
            } else {
                bail!("Specify either --build or --version");
            }
        }

        Commands::RefreshDerived { .. }
        | Commands::Reindex { .. }
        | Commands::Relink { .. }
        | Commands::PruneMaterialized { .. }
        | Commands::Gc { .. }
        | Commands::RequiredPaths
        | Commands::Update { .. }
        | Commands::Verify { .. }
        | Commands::MigrateCas { .. }
        | Commands::CompleteBuild { .. } => {
            unreachable!("handled before manifest load")
        }

        Commands::DumpRendererData { .. } => {
            unreachable!("handled after manifest load")
        }

        Commands::Register { latest, version, build, path } => {
            if !path.exists() {
                bail!("Path does not exist: {}", path.display());
            }

            if latest {
                // Validate it looks like a WoWs install
                let builds = wowsunpack::game_data::list_available_builds(&path)
                    .attach_with(|| format!("No valid game builds found at {}", path.display()))?;

                if builds.is_empty() {
                    bail!("No builds found in {}/bin/", path.display());
                }

                reg.latest_path = Some(path.clone());
                registry::save_registry(&reg, &data_dir.join("versions.toml"))?;

                println!("Registered {} as latest path", path.display());
                println!("Currently available builds: {:?}", builds);
                return Ok(None);
            }

            let builds = wowsunpack::game_data::list_available_builds(&path)
                .attach_with(|| format!("No valid game builds found at {}", path.display()))?;

            if builds.is_empty() {
                bail!("No builds found in {}/bin/", path.display());
            }

            let target_builds = if let Some(b) = build {
                if !builds.contains(&b) {
                    bail!("Build {b} not found at {}. Available: {:?}", path.display(), builds);
                }
                vec![b]
            } else if let Some(ref v) = version {
                let mut matched = Vec::new();
                for &b in &builds {
                    if let Ok(detected) = detect::detect_version_at_path(&path, b)
                        && manifest::version_matches(&detected, v)
                    {
                        matched.push(b);
                    }
                }
                if matched.is_empty() {
                    bail!("No builds matching version '{v}' found at {}", path.display());
                }
                matched
            } else {
                builds
            };

            for b in target_builds {
                let version_str = detect::detect_version_at_path(&path, b).unwrap_or_else(|_| "unknown".to_string());
                reg.set_registered(b, &version_str, &path);
                println!("Registered build {b} (version {version_str}) at {}", path.display());
            }

            registry::save_registry(&reg, &data_dir.join("versions.toml"))?;
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::Args;
    use super::DumpDataSource;
    use super::DumpRequest;
    use super::DumpSelector;
    use super::DumpVersionPlan;
    use super::ExistingDumpPolicy;
    use super::ExistingPinPolicy;
    use super::ManifestRefresh;
    use super::RefreshOutcome;
    use super::VersionSource;
    use super::already_pinned;
    use super::local_builds_for_dump;
    use super::registry;
    use super::resolve_dump_renderer_plan;
    use super::resolve_published_build;
    use super::resolve_refresh_version;
    use super::steam;
    use super::unknown_build_manifest_entry;
    use clap::Parser;
    use std::collections::BTreeSet;
    use std::path::Path;
    use wows_data_mgr::manifest::DepotId;
    use wows_data_mgr::manifest::GameVersionManifest;
    use wows_data_mgr::manifest::ManifestId;

    #[test]
    fn game_dir_accepts_each_dump_selector() {
        for args in [
            &["wows-data-mgr", "dump-renderer-data", "--output", "out", "--game-dir", r"G:\game", "--latest"][..],
            &["wows-data-mgr", "dump-renderer-data", "--output", "out", "--game-dir", r"G:\game", "--version", "15.7"]
                [..],
            &[
                "wows-data-mgr",
                "dump-renderer-data",
                "--output",
                "out",
                "--game-dir",
                r"G:\game",
                "--build",
                "13015711",
            ][..],
        ] {
            assert!(Args::try_parse_from(args).is_ok());
        }
    }

    #[test]
    fn game_dir_plan_resolves_each_selector_from_the_manifest() {
        let manifest = manifest_fixture();

        for (selector, expected_build, expected_version) in [
            (DumpSelector::Latest, 13_100_000, "15.8.0"),
            (DumpSelector::Build(13_015_711), 13_015_711, "15.7.0"),
            (DumpSelector::Version("15.7"), 13_015_712, "15.7.1"),
        ] {
            let plan = resolve_dump_renderer_plan(&manifest, &request(selector, true, false), &LOCAL_BUILDS).unwrap();

            assert_eq!(plan.target, expected_build);
            assert_eq!(plan.version, DumpVersionPlan::UseManifest(expected_version.to_string()));
        }
    }

    #[test]
    fn game_dir_plan_bypasses_registry_and_keeps_the_shared_cleanup_policy() {
        let manifest = manifest_fixture();
        let override_plan =
            resolve_dump_renderer_plan(&manifest, &request(DumpSelector::Version("15.7"), true, true), &LOCAL_BUILDS)
                .unwrap();
        let registry_plan =
            resolve_dump_renderer_plan(&manifest, &request(DumpSelector::Version("15.7"), false, true), &LOCAL_BUILDS)
                .unwrap();

        assert_eq!(override_plan.source, DumpDataSource::Supplied(Path::new("supplied-game").to_path_buf()));
        assert_eq!(registry_plan.source, DumpDataSource::Registry);
        assert_eq!(override_plan.target, registry_plan.target);
        assert_eq!(override_plan.output, registry_plan.output);
        assert_eq!(override_plan.existing_dump, ExistingDumpPolicy::Replace);
        assert_eq!(override_plan.existing_dump, registry_plan.existing_dump);
    }

    #[test]
    fn forced_game_dir_plan_validates_source_before_using_the_manifest_version() {
        let manifest = manifest_fixture();

        let plan =
            resolve_dump_renderer_plan(&manifest, &request(DumpSelector::Version("15.7"), true, true), &LOCAL_BUILDS)
                .unwrap();

        assert_eq!(plan.version, DumpVersionPlan::ValidateSourceThenUseManifest("15.7.1".to_string()));
    }

    #[test]
    fn unknown_build_guidance_uses_a_parseable_client_manifest_pair() {
        let suggestion = unknown_build_manifest_entry(13_015_799, "15.7.9");

        let manifest: GameVersionManifest = toml::from_str(&suggestion).unwrap();
        let entry = manifest.get(13_015_799).unwrap();
        assert_eq!(entry.version, "15.7.9");
        assert_eq!(entry.client.depot_id, DepotId(552993));
        assert_eq!(entry.client.manifest_id, ManifestId("<look up on SteamDB>".to_string()));
        assert_eq!(entry.content, None);
        assert_eq!(entry.localization, None);
    }

    /// The request a test resolves, varying only what it is asserting about.
    pub(super) fn request(selector: DumpSelector<'_>, supplied_game_dir: bool, force: bool) -> DumpRequest<'_> {
        DumpRequest {
            selector,
            game_dir: supplied_game_dir.then(|| Path::new("supplied-game")),
            output: Path::new("dump-output"),
            force,
        }
    }

    /// Builds a dump could read. Its newest matches the manifest's newest, so
    /// tests that predate local-build resolution keep asserting the same thing.
    pub(super) const LOCAL_BUILDS: [u32; 4] = [12_830_008, 13_015_711, 13_015_712, 13_100_000];

    /// A build too new to be pinned yet is the one worth dumping, and is what
    /// forced `--latest` off the manifest. Resolving through the manifest named
    /// 13_100_000, which the dump would then have to find locally anyway.
    #[test]
    fn latest_dumps_the_newest_build_present_locally_not_the_newest_pinned() {
        let manifest = manifest_fixture();
        let local = [13_015_711, 13_200_000];

        let plan = resolve_dump_renderer_plan(&manifest, &request(DumpSelector::Latest, true, false), &local).unwrap();

        assert_eq!(plan.target, 13_200_000);
        assert_eq!(plan.version, DumpVersionPlan::DetectFromSource);
    }

    /// Every `--latest` dump already had to find its build locally, so an empty
    /// source is the one case this can fail on, and it must say what to do.
    #[test]
    fn latest_without_a_local_build_says_how_to_supply_one() {
        let manifest = manifest_fixture();

        let err = resolve_dump_renderer_plan(&manifest, &request(DumpSelector::Latest, false, false), &[]).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("--game-dir"), "{message}");
        assert!(message.contains("register --latest"), "{message}");
    }

    /// The live client depot ships the current build and the one it replaced
    /// (measured: 13015811 and 13187581 on the public branch in September 2026),
    /// so requiring a single build directory would never resolve.
    #[test]
    fn an_unqualified_refresh_pins_the_newest_build_the_depot_ships() {
        assert_eq!(resolve_published_build(&BTreeSet::from([13_100_000]), None).unwrap(), 13_100_000);
        assert_eq!(resolve_published_build(&BTreeSet::from([13_015_811, 13_187_581]), None).unwrap(), 13_187_581);
    }

    /// A dump pins the build it dumped or nothing.
    #[test]
    fn a_dump_pins_only_the_build_the_branch_currently_introduces() {
        assert_eq!(resolve_published_build(&BTreeSet::from([13_187_581]), Some(13_187_581)).unwrap(), 13_187_581);

        let err = resolve_published_build(&BTreeSet::from([13_187_581]), Some(13_100_000)).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("13187581") && message.contains("13100000"), "{message}");
    }

    /// Dumping the previous release must not file the current release's
    /// manifests under it. The depot retains that build's directory, so
    /// membership alone would have accepted it.
    #[test]
    fn a_build_the_depot_merely_retains_is_not_pinnable() {
        let published = BTreeSet::from([13_015_811, 13_187_581]);

        let err = resolve_published_build(&published, Some(13_015_811)).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("introduces build 13187581"), "{message}");
        assert!(message.contains("misattribute"), "{message}");
    }

    /// Discovery guarantees a non-empty set, but the rule must not depend on it.
    #[test]
    fn an_empty_listing_pins_nothing() {
        assert!(resolve_published_build(&BTreeSet::new(), None).is_err());
        assert!(resolve_published_build(&BTreeSet::new(), Some(13_187_581)).is_err());
    }

    /// An entry records the manifest a build was first introduced under. A
    /// depot re-push moves the branch's current pins without moving the build,
    /// so re-running a dump days after release must not overwrite that record.
    #[test]
    fn a_build_already_pinned_is_left_alone() {
        let refresh = refresh_fixture(Some(13_015_711), ExistingPinPolicy::Keep);

        let outcome = already_pinned(&refresh, &manifest_fixture());

        assert_eq!(outcome, Some(RefreshOutcome::AlreadyPinned { build: 13_015_711, version: "15.7.0".to_string() }));
    }

    /// The short circuit runs before any Steam query, so an archival dump of an
    /// old build costs neither an app-info call nor a client depot listing.
    #[test]
    fn an_unpinned_build_and_a_forced_refresh_both_reach_steam() {
        assert_eq!(
            already_pinned(&refresh_fixture(Some(13_200_000), ExistingPinPolicy::Keep), &manifest_fixture()),
            None
        );
        assert_eq!(
            already_pinned(&refresh_fixture(Some(13_015_711), ExistingPinPolicy::Replace), &manifest_fixture()),
            None
        );
        // Without a build named up front there is nothing to check yet.
        assert_eq!(already_pinned(&refresh_fixture(None, ExistingPinPolicy::Keep), &manifest_fixture()), None);
    }

    /// A typed --version is written verbatim and nothing else checks it, so a
    /// stale one would label whichever build the branch happens to publish.
    #[test]
    fn an_asserted_version_the_registry_contradicts_is_rejected() {
        let mut reg = registry::LocalRegistry::default();
        reg.set_downloaded(13_015_711, "15.7.0");

        let err =
            resolve_refresh_version(&VersionSource::Asserted("15.6.0".to_string()), 13_015_711, &reg).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("15.7.0") && message.contains("15.6.0"), "{message}");

        // Agreeing, or a build the registry has never seen, both pass.
        assert_eq!(
            resolve_refresh_version(&VersionSource::Asserted("15.7.0".to_string()), 13_015_711, &reg).unwrap(),
            "15.7.0"
        );
        assert_eq!(
            resolve_refresh_version(&VersionSource::Asserted("15.8.0".to_string()), 13_200_000, &reg).unwrap(),
            "15.8.0"
        );
    }

    /// A version read out of the dumped game data outranks the registry.
    #[test]
    fn a_detected_version_is_taken_as_given() {
        let mut reg = registry::LocalRegistry::default();
        reg.set_downloaded(13_015_711, "stale");

        assert_eq!(
            resolve_refresh_version(&VersionSource::Detected("15.7.0".to_string()), 13_015_711, &reg).unwrap(),
            "15.7.0"
        );
        assert_eq!(resolve_refresh_version(&VersionSource::Registry, 13_015_711, &reg).unwrap(), "stale");
        assert!(resolve_refresh_version(&VersionSource::Registry, 13_200_000, &reg).is_err());
    }

    /// `game_dir_for_build` falls back to `builds/<n>` for a build with no
    /// registry entry, so `--latest` must offer one it can already dump.
    #[test]
    fn latest_sees_a_downloaded_build_the_registry_never_recorded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("builds/13200000")).unwrap();
        std::fs::create_dir_all(dir.path().join("builds/not-a-build")).unwrap();
        std::fs::write(dir.path().join("builds/13300000"), b"a file, not a build").unwrap();

        let builds = local_builds_for_dump(None, dir.path()).unwrap();

        assert_eq!(builds, vec![13_200_000]);
    }

    fn refresh_fixture(expected_build: Option<u32>, overwrite: ExistingPinPolicy) -> ManifestRefresh<'static> {
        ManifestRefresh {
            repo_root: Path::new("repo"),
            login: steam::SteamLogin::SteamClientToken,
            prompt: steam::LoginPrompt::Deny,
            expected_build,
            version: VersionSource::Registry,
            overwrite,
        }
    }

    pub(super) fn manifest_fixture() -> GameVersionManifest {
        toml::from_str(
            r#"
            [versions.13015711]
            version = "15.7.0"
            client_depot_id = 552993
            client_manifest_id = "client-15.7.0"

            [versions.13015712]
            version = "15.7.1"
            client_depot_id = 552993
            client_manifest_id = "client-15.7.1"

            [versions.13100000]
            version = "15.8.0"
            client_depot_id = 552993
            client_manifest_id = "client-15.8.0"
            "#,
        )
        .unwrap()
    }
}

#[cfg(test)]
mod link_flag_tests {
    use super::DumpRequest;
    use super::DumpSelector;
    use super::LinkTarget;
    use super::resolve_dump_renderer_plan;
    use super::tests::LOCAL_BUILDS;
    use super::tests::manifest_fixture;
    use std::path::Path;
    use std::path::PathBuf;

    /// The link step materializes the build the dump resolved to. Reading the
    /// raw `--build` argument instead left it absent for `--latest` and
    /// `--version`, and an absent build means every build in the base.
    #[test]
    fn a_dump_links_the_build_its_selector_resolved_to() {
        let manifest = manifest_fixture();

        for (selector, expected) in [
            (DumpSelector::Latest, 13_100_000),
            (DumpSelector::Build(13_015_711), 13_015_711),
            (DumpSelector::Version("15.7"), 13_015_712),
        ] {
            let plan = resolve_dump_renderer_plan(
                &manifest,
                &DumpRequest { selector, game_dir: None, output: Path::new("out"), force: false },
                &LOCAL_BUILDS,
            )
            .unwrap();

            assert_eq!(plan.link_target(), LinkTarget { output: PathBuf::from("out"), build: expected });
        }
    }
}
