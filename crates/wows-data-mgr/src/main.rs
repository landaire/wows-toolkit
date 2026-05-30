use clap::Parser;
use clap::Subcommand;
use rootcause::prelude::*;
use std::path::PathBuf;

mod detect;
mod download;

use wows_data_mgr::dump;
use wows_data_mgr::manifest;
use wows_data_mgr::registry;

#[derive(Parser)]
#[command(name = "wows-data-mgr", about = "Download and manage World of Warships game data")]
struct Args {
    /// Override the game data directory (default: game_data/ in repo root)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download game data for a specific version via DepotDownloader
    Download {
        /// Download the latest known version
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
        /// Dump for the latest available build
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

        /// Dump directly from this game install directory, skipping the
        /// version manifest and registry. Use together with --build.
        #[arg(long, conflicts_with_all = &["latest", "version"])]
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

    /// Regenerate derived artifacts (rkyv blob, compressed copies) for dumped
    /// builds, deduplicate them into content-addressed storage, then garbage
    /// collect CAS objects no longer referenced by any build. Pass `--no-gc`
    /// to keep orphaned objects around (run `gc` later to reclaim them).
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

    /// Delete content-addressed objects no longer referenced by any dumped
    /// build. This is the only command that removes shared storage.
    Gc {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Print the VFS path globs the dump extracts, one per line. Feed these to
    /// `wowsunpack pkgs` to resolve the minimal set of .pkg files to download.
    RequiredPaths,

    /// Add missing assets (maps, and with --with-gui the gui/ dirs) to an
    /// existing build without re-extracting data it already has. Regenerates the
    /// rkyv blob with the current parser. Only needs gui + spaces_* packages on
    /// disk, not the multi-GiB basecontent package.
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

    /// Fold a legacy `vfs_common/` store into `common/` and relink every build,
    /// healing a dump base where a redump created `common/` while old builds
    /// still reference `vfs_common/`.
    MigrateCas {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Verify that every build in a dump base is internally consistent: its
    /// metadata parses and every referenced content object exists in common/.
    /// Exits non-zero if any build is broken.
    Verify {
        /// Directory containing dumps (same as dump-renderer-data --output)
        #[arg(short, long)]
        output: PathBuf,

        /// Also check that each reconstructed symlink resolves to a readable file
        #[arg(long)]
        check_links: bool,
    },

    /// Copy dumped builds from a local source dump base into a destination
    /// (e.g. the toolkit's data cache), deduplicating against content already
    /// present. The offline equivalent of the toolkit's GitHub download, for
    /// testing cache updates without publishing data.
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

fn main() -> Result<(), Report> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();
    let repo_root = find_repo_root()?;
    let data_dir = resolve_data_dir(&args.data_dir)?;

    // These commands don't need the version manifest, so handle them before
    // loading it (a malformed game_versions.toml must not block them).
    match &args.command {
        Commands::RefreshDerived { output, build, no_gc } => {
            println!("Refreshing derived data...");
            dump::refresh_derived(output, *build)?;
            if !*no_gc {
                println!("Garbage-collecting orphaned CAS objects...");
                dump::gc_cas(output)?;
            } else {
                println!("Skipping garbage collection (--no-gc).");
            }
            return Ok(());
        }
        Commands::Gc { output } => {
            println!("Garbage-collecting orphaned CAS objects...");
            return dump::gc_cas(output);
        }
        Commands::RequiredPaths => {
            for glob in dump::required_path_globs() {
                println!("{glob}");
            }
            return Ok(());
        }
        Commands::CompleteBuild { build, game_dir, output, with_gui } => {
            println!("Completing build {build} from {} (with_gui={with_gui})...", game_dir.display());
            let map_count = dump::complete_build(game_dir, *build, output, *with_gui)?;
            println!("Done: extracted {map_count} map(s) and regenerated derived data.");
            return Ok(());
        }
        Commands::MigrateCas { output } => {
            println!("Merging vfs_common/ into common/ and relinking builds in {}...", output.display());
            let migrated = dump::migrate_cas_dir_name(output)?;
            if migrated {
                println!("Done. Run `verify` to confirm consistency.");
            } else {
                println!("Nothing to migrate (no vfs_common/ present).");
            }
            return Ok(());
        }
        Commands::Verify { output, check_links } => {
            let reports = dump::verify_builds(output, *check_links)?;
            if reports.is_empty() {
                println!("No builds found in {}", output.display());
                return Ok(());
            }
            let mut broken = 0;
            for r in &reports {
                if r.is_ok() {
                    println!("  OK   {} ({} objects)", r.dir, r.referenced);
                } else {
                    broken += 1;
                    if r.metadata_unreadable {
                        println!("  FAIL {} - metadata.toml unreadable", r.dir);
                    } else {
                        println!(
                            "  FAIL {} - {}/{} objects missing, {} broken link(s)",
                            r.dir,
                            r.missing_objects.len(),
                            r.referenced,
                            r.broken_links.len()
                        );
                    }
                }
            }
            let ok = reports.len() - broken;
            println!("\n{ok}/{} builds consistent.", reports.len());
            if broken > 0 {
                bail!("{broken} build(s) inconsistent");
            }
            return Ok(());
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
            return Ok(());
        }
        // An explicit build + game directory dumps without manifest or registry.
        Commands::DumpRendererData { build: Some(b), game_dir: Some(gd), output, force, .. } => {
            let build = *b;
            let version_str = detect::detect_version_at_path(gd, build)
                .attach_with(|| format!("Could not detect version for build {build} at {}", gd.display()))?;
            let dir = dump::dump_dir(output, &version_str, build);
            if *force && dir.exists() {
                println!("Removing existing dump at {}...", dir.display());
                std::fs::remove_dir_all(&dir)?;
            }
            println!("Dumping build {build} ({version_str}) from {}", gd.display());
            let pb = dump::create_progress_bar(gd);
            dump::dump_renderer_data(gd, build, &version_str, output, pb.as_ref(), false)?;
            println!("Dumped to {}", dir.display());
            return Ok(());
        }
        _ => {}
    }

    let manifest = manifest::load_manifest(&repo_root.join("game_versions.toml"))?;
    let mut reg = registry::load_registry(&data_dir.join("versions.toml"));

    match args.command {
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
                return Ok(());
            }

            let entry = manifest.get(target);
            download::download_build(target, entry, &data_dir, &repo_root, username.as_deref())?;

            let version_str = detect::detect_version_for_build(&data_dir, target)?;
            reg.set_downloaded(target, &version_str);
            registry::save_registry(&reg, &data_dir.join("versions.toml"))?;

            if entry.is_none() {
                println!();
                println!("This build is not in game_versions.toml. Add it with:");
                println!();
                println!("[versions.{target}]");
                println!("version = \"{version_str}\"");
                println!("depot_id = 552991");
                println!("manifest_id = \"<look up on SteamDB>\"");
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

                println!("{:<12} {:<10} {:<24} {}", build_str, entry.version, entry.manifest_id, status);
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

        Commands::DumpRendererData { latest, build, version, output, force, game_dir: _ } => {
            let target = if latest {
                let builds = reg.available_builds();
                *builds.last().ok_or_else(|| rootcause::report!("No builds available"))?
            } else if let Some(b) = build {
                b
            } else if let Some(ref v) = version {
                manifest
                    .find_by_version(v)
                    .ok_or_else(|| rootcause::report!("No build found matching version '{v}'"))?
            } else {
                bail!("Specify --latest, --build, or --version");
            };

            let game_dir = reg
                .game_dir_for_build(target, &data_dir)
                .ok_or_else(|| rootcause::report!("Build {target} not available locally"))?;

            let version_str = if let Some(entry) = manifest.get(target) {
                entry.version.clone()
            } else {
                detect::detect_version_at_path(&game_dir, target).unwrap_or_else(|_| "unknown".to_string())
            };

            if force {
                // Remove stale builds.toml entry for this build
                let builds_path = output.join("builds.toml");
                let mut index = wows_data_mgr::builds::BuildsIndex::load(&builds_path);
                if index.find_by_build(target).is_some() {
                    // Find and remove the old directory
                    if let Some(old_entry) = index.find_by_build(target).cloned() {
                        let old_dir = output.join(&old_entry.dir);
                        if old_dir.exists() {
                            println!("Removing old dump at {}...", old_dir.display());
                            std::fs::remove_dir_all(&old_dir)?;
                        }
                    }
                    index.remove_build(target);
                    index.save(&builds_path)?;
                }

                // Also remove the new version dir if it exists
                let existing_dir = dump::dump_dir(&output, &version_str, target);
                if existing_dir.exists() {
                    println!("Removing existing dump at {}...", existing_dir.display());
                    std::fs::remove_dir_all(&existing_dir)?;
                }
            }

            println!("Building VFS from game directory...");
            let pb = dump::create_progress_bar(&game_dir);
            dump::dump_renderer_data(&game_dir, target, &version_str, &output, pb.as_ref(), false)?;
            println!("Dumped renderer data to {}", dump::dump_dir(&output, &version_str, target).display());
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
        | Commands::Gc { .. }
        | Commands::RequiredPaths
        | Commands::Update { .. }
        | Commands::Verify { .. }
        | Commands::MigrateCas { .. }
        | Commands::CompleteBuild { .. } => {
            unreachable!("handled before manifest load")
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
                return Ok(());
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

    Ok(())
}
