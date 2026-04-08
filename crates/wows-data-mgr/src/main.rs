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
    let args = Args::parse();
    let repo_root = find_repo_root()?;
    let data_dir = resolve_data_dir(&args.data_dir)?;
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

        Commands::DumpRendererData { latest, build, version, output } => {
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
