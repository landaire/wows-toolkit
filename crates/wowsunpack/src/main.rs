use rootcause::prelude::*;

use pickled::HashableValue;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::fs::{
    self,
};
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::io::stdout;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Instant;
use thread_local::ThreadLocal;
use vfs::VfsPath;
use vfs::impls::overlay::OverlayFS;
use wowsunpack::data::assets_bin_vfs::AssetsBinVfs;
use wowsunpack::data::idx::VfsEntry;
use wowsunpack::data::idx::{
    self,
};
use wowsunpack::data::idx_vfs::IdxVfs;
use wowsunpack::data::serialization;
use wowsunpack::data::wrappers::mmap::MmapPkgSource;
use wowsunpack::export::gltf_export;
use wowsunpack::game_params::convert::game_params_to_pickle;

use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use indicatif::ParallelProgressIterator;
use indicatif::ProgressBar;
use rayon::prelude::*;

/// Utility for interacting with World of Warships game assets
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Game directory. This option can be used instead of pkg_dir / idx_files
    /// and will automatically use the latest version of the game. If none of these
    /// args are provided, the executable's directory is assumed to be the game dir.
    ///
    /// This option will use the latest build of WoWs in the `bin` directory, which
    /// may not necessarily be the latest _playable_ version of the game e.g. when the
    /// game launcher preps an update to the game which has not yet gone live.
    ///
    /// Overrides `--pkg-dir`, `--idx-files`, and `--bin-dir`
    #[clap(short, long)]
    game_dir: Option<PathBuf>,

    /// Directory where pkg files are located. If not provided, this will
    /// default relative to the given idx directory as "../../../../res_packages"
    ///
    /// Ignored if `--game-dir` is specified.
    #[clap(short, long)]
    pkg_dir: Option<PathBuf>,

    /// .idx file(s) or their containing directory.
    ///
    /// Ignored if `--game-dir` is specified.
    #[clap(short, long)]
    idx_files: Vec<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// List files in a directory
    List {
        /// Only show files from assets.bin (not idx/pkg)
        #[clap(long)]
        assets: bool,

        /// Directory name to list
        dir: Option<String>,
    },
    /// Extract files to an output directory
    Extract {
        /// Only extract files from assets.bin (not idx/pkg)
        #[clap(long)]
        assets: bool,

        /// Flatten the file structure when writing files. For example, if the
        /// given output directory is `res` and the target file is `content/GameParams.data`,
        /// the file will normally be written to `res/content/GameParams.data`.
        /// When this flag is set, the file will be written to `res/GameParams.data`
        #[clap(long)]
        flatten: bool,

        /// Do not preserve the matched file path when writing output files. For example,
        /// if `gui/achievements` is passed as a `files` arg and `res_unpacked` is the `out_dir`, it would normally
        /// extract as `res_unpacked/gui/achievements/`. Enabling this option will instead extract as
        /// `res_unpacked/achievements/` -- stripping the matched part which is not part of the filename
        /// or its children.
        #[clap(long)]
        strip_prefix: bool,

        /// Where to extract files to
        #[clap(short, long, default_value = "wowsunpack_extracted")]
        out_dir: PathBuf,

        /// Files to extract. Glob patterns such as `content/**/*.xml` are accepted
        files: Vec<String>,
    },
    /// Write meta information about the game assets to the specified output file.
    /// This may be useful for diffing contents between builds at a glance. Output
    /// data includes file name, size, CRC32, unpacked size, compression info,
    /// and a flag indicating if the file is a directory.
    ///
    /// The output data will always be sorted by filename.
    Metadata {
        #[clap(short, long, default_value_t = MetadataFormat::Plain, value_enum)]
        format: MetadataFormat,

        /// A value of "-" will print to stdout
        #[clap(default_value = "-")]
        out_file: PathBuf,
    },
    /// Special command for directly reading the `content/GameParams.data` file,
    /// converting it to JSON, and writing to the specified output file path.
    GameParams {
        /// Don't pretty-print the JSON (may make serialization/deserialization faster)
        #[clap(short, long)]
        ugly: bool,

        /// Dump the full GameParams file. This causes `--id` to be ignored.
        #[clap(short, long)]
        full: bool,

        /// Print the top-level GameParams IDs to stdout.
        #[clap(long)]
        print_ids: bool,

        /// Which GameParams identifier to dump
        #[clap(long, default_value = "")]
        id: String,

        #[clap(default_value = "GameParams.json")]
        out_file: PathBuf,
    },
    /// Grep files for the given regex. Only prints a binary match.
    Grep {
        /// Only search files from assets.bin (not idx/pkg)
        #[clap(long)]
        assets: bool,

        /// Path filter
        #[clap(long)]
        path: Option<String>,

        /// The pattern to look for
        pattern: String,
    },
    DiffDump {
        out_dir: PathBuf,
    },
    /// Parse and inspect a .geometry model file
    Geometry {
        /// Path to the .geometry file (VFS path by default, disk path with --no-vfs)
        file: PathBuf,

        /// Decode ENCD-compressed vertex/index buffers and print sizes
        #[clap(long)]
        decode: bool,

        /// Read file from disk instead of VFS
        #[clap(long)]
        no_vfs: bool,
    },
    /// Export a ship sub-model to GLB format
    ExportModel {
        /// Path to a .geometry file (VFS path by default, disk path with --no-vfs).
        /// If a matching .visual exists in assets.bin it will be used for material
        /// names, textures, and LOD filtering. Otherwise raw geometry is exported.
        file: PathBuf,

        /// Output file path
        #[arg(short, long, default_value = "output.glb")]
        output: PathBuf,

        /// LOD level (0 = highest detail)
        #[arg(long, default_value = "0")]
        lod: usize,

        /// Skip loading camouflage textures
        #[arg(long)]
        no_textures: bool,

        /// Export the damaged/destroyed hull state (crack geometry instead of patches)
        #[arg(long)]
        damaged: bool,

        /// List available camouflage texture schemes, then exit
        #[arg(long)]
        list_textures: bool,

        /// Read file from disk instead of VFS
        #[clap(long)]
        no_vfs: bool,
    },
    /// Export all sub-models of a ship to a single GLB file.
    /// Each sub-model becomes a separate named object in Blender.
    ExportShip {
        /// Ship name — either a model directory name (e.g. "JSB039_Yamato_1945")
        /// or a translated display name (e.g. "Yamato") for fuzzy lookup
        name: String,

        /// Output file path
        #[arg(short, long, default_value = "output.glb")]
        output: PathBuf,

        /// LOD level (0 = highest detail)
        #[arg(long, default_value = "0")]
        lod: usize,

        /// List available hull upgrades and their components, then exit
        #[arg(long)]
        list_upgrades: bool,

        /// Hull upgrade to use (e.g. "A" for stock, "B" for upgraded).
        /// Accepts a prefix match against upgrade keys.
        #[arg(long)]
        hull: Option<String>,

        /// Skip loading camouflage textures
        #[arg(long)]
        no_textures: bool,

        /// Export the damaged/destroyed hull state (crack geometry instead of patches)
        #[arg(long)]
        damaged: bool,

        /// List available camouflage texture schemes, then exit
        #[arg(long)]
        list_textures: bool,

        /// Enable verbose debug output (UV decoding details, etc.)
        #[arg(long)]
        debug: bool,
    },
    /// Export a complete map/space to a single GLB file (models, terrain, water).
    /// Takes a space directory path (e.g. "spaces/16_OC_bees_to_honey") and loads
    /// models.geometry, models.bin, space.bin, space.settings, and terrain.bin from it.
    ExportMap {
        /// Path to a space directory (e.g. "spaces/16_OC_bees_to_honey")
        space_dir: PathBuf,

        /// Output file path
        #[arg(short, long, default_value = "output.glb")]
        output: PathBuf,

        /// LOD level (0 = highest detail)
        #[arg(long, default_value = "0")]
        lod: usize,

        /// Read files from disk instead of VFS
        #[clap(long)]
        no_vfs: bool,

        /// Terrain decimation step (1=full, 4=default, 8=coarse)
        #[arg(long, default_value = "4")]
        terrain_step: u32,

        /// Skip terrain mesh generation
        #[arg(long)]
        no_terrain: bool,

        /// Skip water plane generation
        #[arg(long)]
        no_water: bool,

        /// Skip vegetation (trees/bushes from forest.bin)
        #[arg(long)]
        no_vegetation: bool,

        /// Vegetation grid cell size in meters for decimation. One tree per species
        /// per cell is kept. Larger values = fewer trees. 0 = no decimation.
        #[arg(long, default_value = "20")]
        vegetation_density: f32,

        /// Skip loading model textures
        #[arg(long)]
        no_textures: bool,

        /// Maximum texture dimension (e.g. 512, 1024). Textures larger than this
        /// are downsampled with box filtering. Reduces GLB file size significantly.
        #[arg(long)]
        max_texture_size: Option<u32>,
    },
    /// Inspect armor model geometry and GameParams thickness data for a ship
    Armor {
        /// Ship name — either a model directory name (e.g. "JSB039_Yamato_1945")
        /// or a translated display name (e.g. "Yamato") for fuzzy lookup
        name: String,

        /// Hull upgrade to use (e.g. "A" for stock, "B" for upgraded)
        #[arg(long)]
        hull: Option<String>,
    },
    /// Parse and inspect an assets.bin (PrototypeDatabase) file
    AssetsBin {
        /// Path to the assets.bin file (VFS path by default, disk path with --no-vfs)
        file: PathBuf,

        /// Filter path entries by name substring
        #[clap(long)]
        filter: Option<String>,

        /// Maximum number of path entries to display
        #[clap(long, default_value = "50")]
        max_paths: usize,

        /// Resolve a path suffix to its prototype location and print record info
        #[clap(long)]
        resolve: Option<String>,

        /// Parse and display a VisualPrototype by path suffix
        #[clap(long)]
        parse_visual: Option<String>,

        /// Parse and display a MaterialPrototype by path suffix
        #[clap(long)]
        parse_material: Option<String>,

        /// Read file from disk instead of VFS
        #[clap(long)]
        no_vfs: bool,
    },
    /// Dump UV coordinate statistics for hull meshes of a ship
    DumpUvs {
        /// Ship name
        name: String,

        /// Hull upgrade to use
        #[arg(long)]
        hull: Option<String>,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, ValueEnum)]
enum MetadataFormat {
    Plain,
    Json,
    Csv,
}

fn load_idx_file(path: PathBuf) -> Result<idx::IdxFile, Report> {
    let file_data = std::fs::read(&path).context("Failed to read idx file")?;
    Ok(idx::parse(&file_data)?)
}

/// Read file data from disk (if `no_vfs`) or from the VFS.
fn read_file_data(path: &Path, no_vfs: bool, vfs: Option<&VfsPath>) -> Result<Vec<u8>, Report> {
    if no_vfs {
        return Ok(std::fs::read(path).context("Failed to read file from disk")?);
    }

    let Some(vfs) = vfs else {
        bail!(
            "No VFS available. Use --game-dir to specify a game install, \
             or --no-vfs to read from disk."
        );
    };

    let vfs_path = path.to_string_lossy().replace('\\', "/");
    let mut data = Vec::new();
    vfs.join(&vfs_path)
        .context("VFS path error")?
        .open_file()
        .context_with(|| format!("File not found in VFS: '{vfs_path}'"))?
        .read_to_end(&mut data)?;
    Ok(data)
}

/// Add entries from an AssetsBinVfs to the IDX file tree so list/extract can see them.
fn add_vfs_entries_to_file_tree(
    assets_vfs: &AssetsBinVfs,
    file_tree: &mut HashMap<String, VfsEntry>,
) -> HashSet<String> {
    let mut assets_bin_paths = HashSet::new();

    // Add directory entries (skip root "/").
    for dir_path in assets_vfs.dirs() {
        if dir_path != "/" {
            let path = dir_path.to_string();
            assets_bin_paths.insert(path.clone());
            file_tree.entry(path).or_insert(VfsEntry::Directory);
        }
    }

    // Add file entries with stub FileInfo/Volume.
    let stub_volume = idx::Volume { volume_id: 0, filename: String::new() };
    for (file_path, size) in assets_vfs.files() {
        let path = file_path.to_string();
        assets_bin_paths.insert(path.clone());
        file_tree.entry(path).or_insert(VfsEntry::File {
            file_info: idx::FileInfo {
                resource_id: 0,
                volume_id: 0,
                offset: 0,
                compression_info: 0,
                size: size as u32,
                crc32: 0,
                unpacked_size: size as u32,
                padding: 0,
            },
            volume: stub_volume.clone(),
        });
    }

    assets_bin_paths
}

fn run() -> Result<(), Report> {
    let mut args = Args::parse();

    let mut game_dir = PathBuf::from(std::env::args().next().expect("failed to get first arg"))
        .parent()
        .expect("failed to get executable parent dir")
        .to_owned();

    if let Some(game_dir_arg) = args.game_dir.take() {
        game_dir = game_dir_arg;
    }

    let mut game_version = None;

    // Try to set up VFS from game directory / idx files. This is best-effort:
    // if no game dir is provided and no idx files exist, vfs will be None.
    let mut vfs: Option<VfsPath> = None;
    let mut file_tree = HashMap::new();
    let mut assets_bin_paths = HashSet::new();

    if args.idx_files.is_empty() {
        let bin_dir = game_dir.join("bin");
        if game_dir.join("WorldOfWarships.exe").exists() {
            let paths = fs::read_dir(&bin_dir).ok();
            if let Some(paths) = paths {
                for path in paths {
                    let Ok(path) = path else { continue };
                    if path.file_type().map(|t| t.is_dir()).unwrap_or(false)
                        && let Ok(version) = path.file_name().to_str().unwrap().parse::<u64>()
                    {
                        match game_version {
                            Some(other_version) => {
                                if other_version < version {
                                    game_version = Some(version);
                                }
                            }
                            None => game_version = Some(version),
                        }
                    }
                }
            }

            if let Some(latest_version) = game_version {
                let latest_version_str = format!("{latest_version}");
                let idx_path = bin_dir.join(latest_version_str.as_str()).join("idx");
                if idx_path.exists() {
                    args.idx_files.push(idx_path);
                }
            }
        }
    }

    if !args.idx_files.is_empty() {
        let resources = Mutex::new(Vec::new());
        let mut paths = Vec::with_capacity(args.idx_files.len());

        for path in args.idx_files {
            if path.is_dir() {
                if let Some(parent) = path.parent()
                    && let Some(stem) = parent.file_stem().and_then(|stem| stem.to_str())
                    && let Some(version) = stem.parse::<u64>().ok()
                {
                    game_version = Some(version);
                }

                for path in fs::read_dir(path)? {
                    let path = path?;
                    if path.file_type()?.is_file() {
                        paths.push(path.path());
                    }
                }
            } else {
                paths.push(path);
            }
        }

        let packages_dir =
            args.pkg_dir.or_else(|| Some(paths[0].parent()?.parent()?.parent()?.parent()?.join("res_packages")));

        paths.into_par_iter().try_for_each(|path| {
            resources.lock().unwrap().push(load_idx_file(path)?);

            Ok::<(), Report>(())
        })?;

        let idx_files = resources.into_inner().unwrap();
        file_tree = idx::build_file_tree(&idx_files);

        // Build VFS for commands that need to read file contents
        if let Some(pkg_dir) = packages_dir.as_ref() {
            let pkg_source = MmapPkgSource::new(pkg_dir);
            let idx_vfs = IdxVfs::new(pkg_source, &idx_files);
            let pkg_vfs = VfsPath::new(idx_vfs);

            // Try to load assets.bin from the PKG VFS and overlay it.
            let mut assets_bin_data = Vec::new();
            let assets_loaded = pkg_vfs
                .join("content/assets.bin")
                .and_then(|p| p.open_file())
                .and_then(|mut f| {
                    f.read_to_end(&mut assets_bin_data)?;
                    Ok(())
                })
                .is_ok();

            if assets_loaded {
                match AssetsBinVfs::new(assets_bin_data) {
                    Ok(assets_vfs) => {
                        // Add assets.bin entries to the file_tree so list/extract can find them.
                        assets_bin_paths = add_vfs_entries_to_file_tree(&assets_vfs, &mut file_tree);

                        let assets_layer = VfsPath::new(assets_vfs);
                        let overlay = OverlayFS::new(&[assets_layer, pkg_vfs]);
                        vfs = Some(VfsPath::new(overlay));
                    }
                    Err(e) => {
                        eprintln!("Warning: failed to parse assets.bin for overlay VFS: {e}");
                        vfs = Some(pkg_vfs);
                    }
                }
            } else {
                vfs = Some(pkg_vfs);
            }
        }
    }

    match args.command {
        Commands::Extract { assets, flatten, files, out_dir, strip_prefix } => {
            let Some(vfs) = &vfs else {
                bail!("Package file loader is unavailable. Check that the pkg_dir exists.");
            };

            let globs = files
                .iter()
                .map(|file_name| glob::Pattern::new(file_name).expect("invalid glob pattern"))
                .collect::<Vec<_>>();

            let files_written = AtomicUsize::new(0);

            // Collect matching entries
            let matching: Vec<(&str, &VfsEntry)> = file_tree
                .iter()
                .filter(|(path, entry)| {
                    // When --assets is set, only include files from assets.bin.
                    if assets && !assets_bin_paths.contains(path.as_str()) {
                        return false;
                    }
                    let path_str = path.as_str();
                    let mut matches = false;
                    for glob in &globs {
                        if glob.matches(path_str) {
                            matches = true;
                            break;
                        }
                    }
                    if !matches {
                        return false;
                    }
                    // Skip directories when flattening
                    if flatten && matches!(entry, VfsEntry::Directory) {
                        return false;
                    }
                    true
                })
                .map(|(p, e)| (p.as_str(), e))
                .collect();

            for (path, entry) in &matching {
                match entry {
                    VfsEntry::File { .. } => {
                        let mut file_data = Vec::new();
                        vfs.join(path)
                            .context("VFS path error")?
                            .open_file()
                            .context_with(|| format!("Failed to open {path}"))?
                            .read_to_end(&mut file_data)?;

                        let out_path = if flatten {
                            let file_name = path.rsplit('/').next().unwrap_or(path);
                            out_dir.join(file_name)
                        } else if strip_prefix {
                            // Strip the matched prefix, keep only the last component
                            let file_name = path.rsplit('/').next().unwrap_or(path);
                            out_dir.join(file_name)
                        } else {
                            out_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
                        };

                        if let Some(parent) = out_path.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(&out_path, &file_data)?;
                        files_written.fetch_add(1, Ordering::Relaxed);
                    }
                    VfsEntry::Directory => {
                        // For directories, extract all children recursively
                        let prefix = if path.is_empty() { String::new() } else { format!("{}/", path) };

                        for (child_path, child_entry) in &file_tree {
                            if !child_path.starts_with(&prefix) {
                                continue;
                            }
                            if matches!(child_entry, VfsEntry::Directory) {
                                continue;
                            }

                            let mut file_data = Vec::new();
                            vfs.join(child_path.as_str())
                                .context("VFS path error")?
                                .open_file()
                                .context_with(|| format!("Failed to open {child_path}"))?
                                .read_to_end(&mut file_data)?;

                            let relative = if strip_prefix {
                                // Strip the matched part
                                child_path.strip_prefix(&prefix).unwrap_or(child_path.as_str())
                            } else {
                                child_path.as_str()
                            };

                            let out_path = out_dir.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
                            if let Some(parent) = out_path.parent() {
                                fs::create_dir_all(parent)?;
                            }
                            fs::write(&out_path, &file_data)?;
                            files_written.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
            println!("Wrote {} files", files_written.load(Ordering::Relaxed));
        }
        Commands::Metadata { format, out_file } => {
            let data = serialization::tree_to_serialized_files(&file_tree);
            let out_file =
                if out_file.to_str().unwrap() != "-" { Some(BufWriter::new(File::create(out_file)?)) } else { None };
            // TODO: use dynamic dispatch with Box<T>
            match format {
                MetadataFormat::Json => {
                    if let Some(out_file) = out_file {
                        serde_json::to_writer(out_file, &data)?;
                    } else {
                        let stdout = stdout().lock();

                        serde_json::to_writer(stdout, &data)?;
                    }
                }
                MetadataFormat::Csv => {
                    if let Some(out_file) = out_file {
                        let mut writer = csv::Writer::from_writer(out_file);

                        for record in data {
                            writer.serialize(record)?;
                        }
                    } else {
                        let mut writer = csv::Writer::from_writer(stdout().lock());

                        for record in data {
                            writer.serialize(record)?;
                        }
                    };
                }
                MetadataFormat::Plain => {
                    if let Some(out_file) = out_file {
                        let mut writer = out_file;

                        for record in data {
                            writeln!(&mut writer, "{}", record.path())?;
                        }
                    } else {
                        let mut writer = stdout().lock();

                        for record in data {
                            writeln!(&mut writer, "{}", record.path())?;
                        }
                    };
                }
            };
        }
        Commands::GameParams { out_file, ugly, id, print_ids: ids, full } => {
            let Some(vfs) = &vfs else {
                bail!("Package file loader is unavailable. Check that the pkg_dir exists.");
            };

            let pickle = load_game_params(vfs)?;

            fn print_ids(params_dict: &BTreeMap<pickled::HashableValue, pickled::Value>) {
                for key in params_dict.keys() {
                    if let HashableValue::String(s) = key {
                        let s = s.inner();
                        if s.is_empty() {
                            println!("(empty string)");
                        } else {
                            println!("{s}");
                        }
                    } else {
                        println!("Non-string Key: {key:?}")
                    }
                }
            }

            let params_dict = if !full {
                match pickle {
                    pickled::Value::Dict(params_dict) => {
                        if ids {
                            print_ids(&params_dict.inner());
                            return Ok(());
                        }

                        let params_dict = params_dict.inner();
                        let Some(dict) = params_dict.get(&HashableValue::String(id.clone().into())) else {
                            bail!("Could not find GameParams ID {id:?}");
                        };

                        dict.clone()
                    }
                    pickled::Value::Tuple(params_tuple) => {
                        if ids {
                            println!("GameParams format does not have IDs");

                            return Ok(());
                        }
                        params_tuple.inner().first().expect("params_list has no items?").clone()
                    }
                    pickled::Value::List(params_list) => {
                        if ids {
                            println!("GameParams format does not have IDs");

                            return Ok(());
                        }
                        params_list.inner().first().expect("params_list has no items?").clone()
                    }
                    _ => {
                        panic!("Unexpected GameParams root element type");
                    }
                }
            } else {
                pickle
            };

            let writer = BufWriter::new(File::create(&out_file)?);
            if ugly {
                let mut serializer = serde_json::Serializer::new(writer);
                params_dict.serialize(&mut serializer)?;
            } else {
                let mut serializer = serde_json::Serializer::pretty(writer);
                params_dict.serialize(&mut serializer)?;
            }

            println!("GameParams written to {out_file:?}");
        }
        Commands::List { assets, dir } => {
            for (path, entry) in &file_tree {
                let matches = dir.as_ref().map(|dir| path.starts_with(dir.as_str())).unwrap_or(true);
                if !matches {
                    continue;
                }

                let from_assets_bin = assets_bin_paths.contains(path.as_str());
                if assets && !from_assets_bin {
                    continue;
                }
                match entry {
                    VfsEntry::File { file_info, .. } => {
                        let tag = if from_assets_bin { "A" } else { "F" };
                        println!("({tag}) {} {} bytes", path, file_info.unpacked_size);
                    }
                    VfsEntry::Directory => {
                        let tag = if from_assets_bin { "A" } else { "D" };
                        println!("({tag}) {path}");
                    }
                }
            }
        }
        Commands::Grep { assets, pattern, path } => {
            let Some(vfs) = &vfs else {
                bail!("Package file loader is unavailable. Check that the pkg_dir exists.");
            };

            let regex = regex::bytes::Regex::new(pattern.as_str())?;

            let glob = path.map(|glob| glob::Pattern::new(glob.as_str()).expect("invalid glob pattern"));

            let files: Vec<(&String, &VfsEntry)> = file_tree
                .iter()
                .filter(|(path, _)| if assets { assets_bin_paths.contains(path.as_str()) } else { true })
                .collect();

            let buffer = ThreadLocal::<RefCell<Vec<u8>>>::new();

            let bar = ProgressBar::new(files.len() as u64);

            files.into_par_iter().progress_with(bar.clone()).for_each(|(path, entry)| {
                if let Some(glob) = &glob
                    && !glob.matches(path.as_str())
                {
                    return;
                }

                if matches!(entry, VfsEntry::Directory) {
                    return;
                }

                let file_info = match entry {
                    VfsEntry::File { file_info, .. } => file_info,
                    _ => return,
                };

                let buffer = buffer.get_or_default();
                let mut buffer = buffer.borrow_mut();

                buffer.clear();

                let bytes_needed = (file_info.unpacked_size as usize).saturating_sub(buffer.capacity());
                if bytes_needed > 0 {
                    buffer.reserve(bytes_needed);
                }

                let Ok(mut file) = vfs.join(path.as_str()).and_then(|p| p.open_file()) else {
                    return;
                };

                if file.read_to_end(&mut buffer).is_err() {
                    return;
                }

                if let Some(matched) = regex.find(buffer.as_slice()) {
                    if let Ok(data) = std::str::from_utf8(matched.as_bytes()) {
                        bar.println(format!("{path} matched: {data}"));
                    } else {
                        bar.println(format!("{path} matched"));
                    }
                }

                // Avoid retaining huge allocations from large files (e.g. GameParams).
                const MAX_RETAINED: usize = 4 * 1024 * 1024;
                if buffer.capacity() > MAX_RETAINED {
                    *buffer = Vec::new();
                }
            });
        }
        Commands::DiffDump { out_dir } => {
            let Some(vfs) = &vfs else {
                bail!("Package file loader is unavailable. Check that the pkg_dir exists.");
            };

            let game_version = game_version.expect("could not determine latest game version");
            std::fs::write(out_dir.join("version.txt"), game_version.to_string())?;

            let file_info_path = out_dir.join("pkg_files");

            // Dump file info
            for file in serialization::tree_to_serialized_files(&file_tree) {
                let file_path_on_disk = file_info_path.join(file.path().replace('/', std::path::MAIN_SEPARATOR_STR));
                let mut dest = file_path_on_disk;
                let mut new_name = dest.file_name().expect("file has no name?").to_os_string();
                new_name.push(".txt");
                dest.set_file_name(new_name);

                if file.is_directory() {
                    continue;
                }

                std::fs::create_dir_all(dest.parent().expect("file has no parent?"))
                    .expect("failed to create parent dir");

                let out_file =
                    BufWriter::new(std::fs::File::create(dest).expect("failed to create file metadata file"));
                serde_json::to_writer_pretty(out_file, &file).expect("failed to serialize file metadata");
            }

            let game_params_path = out_dir.join("game_params");

            // Dump params info
            let pickle = load_game_params(vfs)?;

            // Dump the base params first
            let base_path = game_params_path.join("base");

            let handle_params_from_listish = |params: &pickled::Value| -> Result<(), Report> {
                let pickled::Value::Dict(params) = params else {
                    bail!("Params are not a dictionary");
                };

                let params = params.inner();
                for (key, value) in params.iter() {
                    let key_str = key.to_string_key().expect("key is not stringable");

                    dump_param(&key_str, value, base_path.to_owned());
                }

                Ok(())
            };

            match pickle {
                pickled::Value::Dict(params_dict) => {
                    let params_dict = params_dict.inner();

                    let base_data = params_dict
                        .get(&HashableValue::String("".to_string().into()))
                        .expect("failed to find base GameParams");

                    let base_data = base_data.dict_ref().expect("params are not a dictionary").inner();

                    for (key, value) in base_data.iter() {
                        let key = key.to_string_key().expect("key is not stringable");

                        dump_param(key.as_ref(), value, base_path.to_owned());
                    }

                    for (region, params) in params_dict
                        .iter()
                        .filter(|(key, _value)| key.string_ref().map(|s| !s.inner().is_empty()).unwrap_or_default())
                    {
                        let pickled::Value::Dict(params) = params else {
                            continue;
                        };

                        let region_key = region.to_string_key().expect("could not convert region to string");
                        let region_path = game_params_path.join(region_key.as_ref());

                        let params = params.inner();
                        for (key, value) in params.iter() {
                            let key_str = key.to_string_key().expect("key is not stringable");

                            dump_param(&key_str, value, region_path.to_owned());
                        }
                    }
                }
                pickled::Value::Tuple(params_tuple) => {
                    handle_params_from_listish(
                        params_tuple.inner().first().expect("params tuple does not have any items?"),
                    )?;
                }
                pickled::Value::List(params_list) => {
                    let params = params_list.inner();
                    let params = params.first().expect("params list does not have any items?");

                    handle_params_from_listish(params)?;
                }
                _ => {
                    panic!("Unexpected GameParams root element type");
                }
            };
        }
        Commands::Geometry { file, decode, no_vfs } => {
            let file_data = read_file_data(&file, no_vfs, vfs.as_ref())?;
            run_geometry(&file_data, &file.to_string_lossy(), decode)?;
        }
        Commands::ExportModel { file, output, lod, no_textures, damaged, list_textures, no_vfs } => {
            run_export_model(&ExportModelParams {
                file: &file,
                output: &output,
                lod,
                no_textures,
                damaged,
                list_textures,
                no_vfs,
                vfs: vfs.as_ref(),
            })?;
        }
        Commands::ExportShip { name, output, lod, list_upgrades, hull, no_textures, damaged, list_textures, debug } => {
            let Some(vfs) = &vfs else {
                bail!("VFS required for export-ship. Use --game-dir to specify a game install.");
            };

            run_export_ship(
                vfs,
                &name,
                &output,
                lod,
                &game_dir,
                game_version,
                list_upgrades,
                hull.as_deref(),
                no_textures,
                damaged,
                list_textures,
                debug,
            )?;
        }
        Commands::ExportMap {
            space_dir,
            output,
            lod,
            no_vfs,
            terrain_step,
            no_terrain,
            no_water,
            no_vegetation,
            vegetation_density,
            no_textures,
            max_texture_size,
        } => {
            run_export_map(
                &space_dir,
                &output,
                lod,
                no_vfs,
                vfs.as_ref(),
                terrain_step,
                no_terrain,
                no_water,
                no_vegetation,
                vegetation_density,
                no_textures,
                max_texture_size,
            )?;
        }
        Commands::Armor { name, hull } => {
            let Some(vfs) = &vfs else {
                bail!("VFS required for armor inspection. Use --game-dir to specify a game install.");
            };

            run_armor(vfs, &name, &game_dir, game_version, hull.as_deref())?;
        }
        Commands::DumpUvs { name, hull } => {
            let Some(vfs) = &vfs else {
                bail!("VFS required. Use --game-dir to specify a game install.");
            };
            run_dump_uvs(vfs, &name, &game_dir, game_version, hull.as_deref())?;
        }
        Commands::AssetsBin { file, filter, max_paths, resolve, parse_visual, parse_material, no_vfs } => {
            let file_data = read_file_data(&file, no_vfs, vfs.as_ref())?;
            run_assets_bin(
                &file_data,
                &file.to_string_lossy(),
                filter.as_deref(),
                max_paths,
                resolve.as_deref(),
                parse_visual.as_deref(),
                parse_material.as_deref(),
            )?;
        }
    }

    Ok(())
}

fn param_path(stem: &str, param: &pickled::Value, mut base: PathBuf) -> Option<PathBuf> {
    let value = param.dict_ref()?;
    let value = value.inner();
    let type_info = value.get(&HashableValue::String("typeinfo".to_string().into()))?;
    let type_info = type_info.dict_ref()?.inner();

    let (nation, species, typ) = (
        type_info.get(&HashableValue::String("nation".to_string().into()))?,
        type_info.get(&HashableValue::String("species".to_string().into()))?,
        type_info.get(&HashableValue::String("type".to_string().into()))?,
    );
    if let pickled::Value::String(typ) = typ {
        base = base.join(typ.inner().as_str());
    }
    if let pickled::Value::String(nation) = nation {
        base = base.join(nation.inner().as_str());
    }
    if let pickled::Value::String(species) = species {
        base = base.join(species.inner().as_str());
    }
    base = base.join(format!("{stem}.json"));

    Some(base)
}

fn dump_param(file_stem: &str, value: &pickled::Value, mut out_path: PathBuf) -> Option<()> {
    out_path = param_path(file_stem, value, out_path)?;

    // Dump this file
    let parent = out_path.parent().expect("no parent dir?");
    std::fs::create_dir_all(parent).expect("failed to create parent dir");

    let file = BufWriter::new(std::fs::File::create(out_path).expect("failed to create output file"));

    let mut serializer = serde_json::Serializer::pretty(file);
    value.serialize(&mut serializer).expect("failed to serialize data");

    None
}

fn load_game_params(vfs: &VfsPath) -> Result<pickled::Value, Report> {
    let mut game_params_data: Vec<u8> = Vec::new();
    vfs.join("content/GameParams.data")
        .context("VFS path error")?
        .open_file()
        .context("Could not find GameParams.data in WoWs package")?
        .read_to_end(&mut game_params_data)
        .context("Failed to read GameParams")?;

    let pickle = game_params_to_pickle(game_params_data).context("Failed to deserialize GameParams")?;

    Ok(pickle)
}

fn run_geometry(file_data: &[u8], name: &str, decode: bool) -> Result<(), Report> {
    use wowsunpack::models::geometry;

    let geom = geometry::parse_geometry(file_data)?;

    println!("=== .geometry file: {} ===", name);
    println!("File size: {} bytes", file_data.len());
    println!();

    println!("Vertices mappings: {}", geom.vertices_mapping.len());
    for (i, m) in geom.vertices_mapping.iter().enumerate() {
        println!(
            "  [{i}] id=0x{:08X} buf={} offset={} count={} texelDensity=0x{:04X}",
            m.mapping_id, m.merged_buffer_index, m.items_offset, m.items_count, m.packed_texel_density
        );
    }
    println!();

    println!("Indices mappings: {}", geom.indices_mapping.len());
    for (i, m) in geom.indices_mapping.iter().enumerate() {
        println!(
            "  [{i}] id=0x{:08X} buf={} offset={} count={} texelDensity=0x{:04X}",
            m.mapping_id, m.merged_buffer_index, m.items_offset, m.items_count, m.packed_texel_density
        );
    }
    println!();

    println!("Merged vertices: {}", geom.merged_vertices.len());
    for (i, v) in geom.merged_vertices.iter().enumerate() {
        let encoding = match &v.data {
            geometry::VertexData::Encoded { element_count, .. } => {
                format!("ENCD ({element_count} elements)")
            }
            geometry::VertexData::Raw(_) => "raw".to_string(),
        };
        println!(
            "  [{i}] format=\"{}\" size={} stride={} skinned={} bumped={} encoding={}",
            v.format_name, v.size_in_bytes, v.stride_in_bytes, v.is_skinned, v.is_bumped, encoding
        );

        if decode {
            match v.data.decode() {
                Ok(decoded) => println!("    -> decoded {} bytes of vertex data", decoded.len()),
                Err(e) => println!("    -> decode error: {e:?}"),
            }
        }
    }
    println!();

    println!("Merged indices: {}", geom.merged_indices.len());
    for (i, idx) in geom.merged_indices.iter().enumerate() {
        let encoding = match &idx.data {
            geometry::IndexData::Encoded { element_count, .. } => {
                format!("ENCD ({element_count} elements)")
            }
            geometry::IndexData::Raw(_) => "raw".to_string(),
        };
        println!("  [{i}] size={} indexSize={} encoding={}", idx.size_in_bytes, idx.index_size, encoding);

        if decode {
            match idx.data.decode() {
                Ok(decoded) => println!("    -> decoded {} bytes of index data", decoded.len()),
                Err(e) => println!("    -> decode error: {e:?}"),
            }
        }
    }
    println!();

    println!("Collision models: {}", geom.collision_models.len());
    for (i, cm) in geom.collision_models.iter().enumerate() {
        println!("  [{i}] name=\"{}\" size={}", cm.name, cm.size_in_bytes);
    }
    println!();

    println!("Armor models: {}", geom.armor_models.len());
    for (i, am) in geom.armor_models.iter().enumerate() {
        println!("  [{i}] name=\"{}\" triangles={}", am.name, am.triangles.len());
    }

    Ok(())
}

fn run_assets_bin(
    file_data: &[u8],
    name: &str,
    filter: Option<&str>,
    max_paths: usize,
    resolve: Option<&str>,
    parse_visual: Option<&str>,
    parse_material: Option<&str>,
) -> Result<(), Report> {
    use wowsunpack::models::assets_bin;
    use wowsunpack::models::material;
    use wowsunpack::models::visual;

    let db = assets_bin::parse_assets_bin(file_data)?;

    // If --resolve is given, do a targeted lookup instead of the full dump.
    if let Some(path_suffix) = resolve {
        let self_id_index = db.build_self_id_index();
        let (location, full_path) = db.resolve_path(path_suffix, &self_id_index)?;

        println!("Resolved: {full_path}");
        println!("  blob_index={}, record_index={}", location.blob_index, location.record_index);
        let blob = &db.databases[location.blob_index];
        println!(
            "  blob: magic=0x{:08X}, record_count={}, size={}",
            blob.prototype_magic, blob.record_count, blob.size
        );

        // Print the first 64 bytes of the record as hex
        let item_sizes: &[usize] = &[0x78, 0x70, 0x20, 0x28, 0x70, 0x10, 0x18, 0x10, 0x10, 0x10];
        if location.blob_index < item_sizes.len() {
            let item_size = item_sizes[location.blob_index];
            match db.get_prototype_data(location, item_size) {
                Ok(data) => {
                    let show_len = item_size.min(data.len()).min(128);
                    println!("  item_size=0x{item_size:X} ({item_size} bytes)");
                    println!("  record hex ({show_len} bytes):");
                    for row in 0..show_len.div_ceil(16) {
                        let start = row * 16;
                        let end = (start + 16).min(show_len);
                        let hex: String =
                            data[start..end].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
                        println!("    +0x{start:02X}: {hex}");
                    }
                }
                Err(e) => println!("  error reading prototype data: {e}"),
            }
        }

        return Ok(());
    }

    // --parse-visual: resolve path, extract record, parse and print VisualPrototype
    if let Some(path_suffix) = parse_visual {
        let self_id_index = db.build_self_id_index();
        let (location, full_path) = db.resolve_path(path_suffix, &self_id_index)?;

        if location.blob_index != 1 {
            bail!("Path '{}' resolved to blob {} (not VisualPrototype blob 1)", path_suffix, location.blob_index);
        }

        let record_data =
            db.get_prototype_data(location, visual::VISUAL_ITEM_SIZE).context("Failed to get visual prototype data")?;

        let vp = visual::parse_visual(record_data).context("Failed to parse VisualPrototype")?;

        println!("=== VisualPrototype: {full_path} ===");
        println!("  blob_index={}, record_index={}", location.blob_index, location.record_index);
        vp.print_summary(&db);

        return Ok(());
    }

    // --parse-material: resolve path, extract record, parse and print MaterialPrototype
    if let Some(path_suffix) = parse_material {
        let self_id_index = db.build_self_id_index();
        let (location, full_path) = db.resolve_path(path_suffix, &self_id_index)?;

        if location.blob_index != material::MATERIAL_BLOB_INDEX {
            bail!(
                "Path '{}' resolved to blob {} (not MaterialPrototype blob {})",
                path_suffix,
                location.blob_index,
                material::MATERIAL_BLOB_INDEX
            );
        }

        let record_data = db
            .get_prototype_data(location, material::MATERIAL_ITEM_SIZE)
            .context("Failed to get material prototype data")?;

        let mat = material::parse_material(record_data).context("Failed to parse MaterialPrototype")?;

        let prop_names = material::build_property_name_table();
        println!("=== MaterialPrototype: {full_path} ===");
        println!("  blob_index={}, record_index={}", location.blob_index, location.record_index);
        mat.print_summary(&prop_names);

        return Ok(());
    }

    println!("=== assets.bin (PrototypeDatabase): {} ===", name);
    println!("File size: {} bytes", file_data.len());
    println!();

    println!("Header:");
    println!("  magic:        0x{:08X}", db.header.magic);
    println!("  version:      0x{:08X}", db.header.version);
    println!("  checksum:     0x{:08X}", db.header.checksum);
    println!("  architecture: 0x{:04X}", db.header.architecture);
    println!("  endianness:   0x{:04X}", db.header.endianness);
    println!();

    println!("Strings:");
    println!(
        "  offsetsMap: capacity={}, buckets={} bytes, values={} bytes",
        db.strings.offsets_map.capacity,
        db.strings.offsets_map.buckets.len(),
        db.strings.offsets_map.values.len()
    );
    println!("  string data: {} bytes", db.strings.string_data.len());
    // Show a few sample strings
    let mut sample_count = 0;
    let mut pos = 0;
    while pos < db.strings.string_data.len() && sample_count < 5 {
        if db.strings.string_data[pos] == 0 {
            pos += 1;
            continue;
        }
        if let Some(s) = db.strings.get_string(pos as u32)
            && !s.is_empty()
        {
            println!("    [offset={pos}] \"{s}\"");
            pos += s.len() + 1;
            sample_count += 1;
            continue;
        }
        pos += 1;
    }
    println!();

    println!("ResourceToPrototypeMap:");
    println!(
        "  capacity={}, buckets={} bytes, values={} bytes",
        db.resource_to_prototype_map.capacity,
        db.resource_to_prototype_map.buckets.len(),
        db.resource_to_prototype_map.values.len()
    );
    println!();

    println!("PathsStorage: {} entries", db.paths_storage.len());
    let mut shown = 0;
    for entry in &db.paths_storage {
        let matches = filter.map(|f| entry.name.contains(f)).unwrap_or(true);
        if !matches {
            continue;
        }
        println!("  selfId=0x{:016X} parentId=0x{:016X} name=\"{}\"", entry.self_id, entry.parent_id, entry.name);
        shown += 1;
        if shown >= max_paths {
            let remaining = if let Some(f) = filter {
                db.paths_storage.iter().filter(|e| e.name.contains(f)).count() - shown
            } else {
                db.paths_storage.len() - shown
            };
            if remaining > 0 {
                println!("  ... and {remaining} more (use --max-paths to show more)");
            }
            break;
        }
    }
    println!();

    println!("Databases: {} entries", db.databases.len());
    for (i, entry) in db.databases.iter().enumerate() {
        println!(
            "  [{i}] magic=0x{:08X} checksum=0x{:08X} records={} size={} bytes",
            entry.prototype_magic, entry.prototype_checksum, entry.record_count, entry.size
        );
    }

    Ok(())
}

struct ExportModelParams<'a> {
    file: &'a Path,
    output: &'a Path,
    lod: usize,
    no_textures: bool,
    damaged: bool,
    list_textures: bool,
    no_vfs: bool,
    vfs: Option<&'a VfsPath>,
}

fn run_export_model(params: &ExportModelParams<'_>) -> Result<(), Report> {
    let ExportModelParams { file, output, lod, no_textures, damaged, list_textures, no_vfs, vfs } = *params;
    use wowsunpack::export::gltf_export;
    use wowsunpack::export::ship::build_texture_set;
    use wowsunpack::export::ship::collect_mfm_info;
    use wowsunpack::export::texture;
    use wowsunpack::models::assets_bin;
    use wowsunpack::models::geometry;
    use wowsunpack::models::visual;

    // 1. Load geometry file (from disk or VFS).
    let geom_data = read_file_data(file, no_vfs, vfs)?;
    let geom = geometry::parse_geometry(&geom_data).context("Failed to parse geometry")?;

    let file_str = file.to_string_lossy();
    println!("Geometry: {file_str}");
    println!(
        "  {} vertex buffers, {} index buffers, {} vertices mappings, {} indices mappings",
        geom.merged_vertices.len(),
        geom.merged_indices.len(),
        geom.vertices_mapping.len(),
        geom.indices_mapping.len(),
    );

    // 2. Try to find a matching .visual (requires VFS + assets.bin).
    //    Derive the visual suffix by replacing .geometry with .visual in the filename.
    let visual_suffix = file_str.strip_suffix(".geometry").map(|stem| format!("{stem}.visual"));

    // Load assets.bin into function scope so the borrow for PrototypeDatabase lives long enough.
    let assets_bin_data = if let (Some(vfs), Some(_)) = (vfs, &visual_suffix) {
        let result: Result<Vec<u8>, Report> = (|| {
            let mut buf = Vec::new();
            vfs.join("content/assets.bin")
                .context("VFS path error")?
                .open_file()
                .context("Could not find content/assets.bin in VFS")?
                .read_to_end(&mut buf)?;
            Ok(buf)
        })();
        result.ok()
    } else {
        None
    };

    // Parse assets.bin and try to resolve a matching .visual.
    let db = assets_bin_data.as_deref().map(assets_bin::parse_assets_bin).transpose()?;

    let resolved_visual = if let (Some(db), Some(vis_suffix)) = (&db, &visual_suffix) {
        let self_id_index = db.build_self_id_index();
        match db.resolve_path(vis_suffix, &self_id_index) {
            Ok((vis_location, vis_full_path)) if vis_location.blob_index == 1 => {
                let vis_data = db
                    .get_prototype_data(vis_location, visual::VISUAL_ITEM_SIZE)
                    .context("Failed to get visual prototype data")?;
                let vp = visual::parse_visual(vis_data).context("Failed to parse VisualPrototype")?;
                println!("Visual: {vis_full_path}");
                println!(
                    "  {} render sets, {} LODs, {} nodes",
                    vp.render_sets.len(),
                    vp.lods.len(),
                    vp.nodes.name_ids.len()
                );
                Some(vp)
            }
            _ => None,
        }
    } else {
        None
    };

    // 3. If we have a visual, use the full export pipeline.
    if let (Some(vp), Some(db), Some(vfs)) = (&resolved_visual, &db, vfs) {
        if list_textures {
            let mfm_infos = collect_mfm_info(vp, db);
            let stems: Vec<String> = mfm_infos.iter().map(|i| i.stem.clone()).collect();
            let schemes = texture::discover_texture_schemes(vfs, &stems);
            if schemes.is_empty() {
                println!("No camouflage textures found for this model.");
            } else {
                println!("Available camouflage schemes:");
                for scheme in &schemes {
                    println!("  {scheme}");
                }
            }
            return Ok(());
        }

        let texture_set = if no_textures {
            gltf_export::TextureSet::empty()
        } else {
            let mfm_infos = collect_mfm_info(vp, db);
            build_texture_set(&mfm_infos, vfs)
        };

        let mut out_file = std::fs::File::create(output).context("Failed to create output file")?;
        gltf_export::export_glb(vp, &geom, db, lod, &texture_set, damaged, &mut out_file)
            .context("Failed to export GLB")?;
    } else {
        // 4. Fallback: raw geometry export (no visual available).
        if list_textures {
            println!("No visual found; texture listing not available for raw geometry export.");
            return Ok(());
        }
        println!("No matching .visual found; exporting raw geometry.");

        let mut out_file = std::fs::File::create(output).context("Failed to create output file")?;
        gltf_export::export_geometry_raw(&geom, &mut out_file).context("Failed to export raw geometry GLB")?;
    }

    let file_size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    println!("Exported to {} ({} bytes)", output.display(), file_size);

    Ok(())
}

/// Parse space.settings XML to extract world-space bounds.
///
/// The `<bounds>` element has chunk coordinates (100m per chunk). We convert to
/// world units: `min * 100`, `(max + 1) * 100`.
fn parse_space_bounds(xml: &str) -> Option<gltf_export::SpaceBounds> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let bounds = doc.descendants().find(|n| n.has_tag_name("bounds"))?;

    let attr = |name: &str| -> Option<f32> { bounds.attribute(name)?.parse().ok() };

    let min_x = attr("minX")?;
    let max_x = attr("maxX")?;
    let min_y = attr("minY")?; // row axis → world Z
    let max_y = attr("maxY")?;

    Some(gltf_export::SpaceBounds {
        min_x: min_x * 100.0,
        max_x: (max_x + 1.0) * 100.0,
        min_z: min_y * 100.0,
        max_z: (max_y + 1.0) * 100.0,
    })
}

/// Extract the lightmap shadow DDS path from space.ubersettings XML.
fn parse_lightmap_path(xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let node = doc.descendants().find(|n| n.has_tag_name("lightMapShadow"))?;
    let value = node.descendants().find(|n| n.has_tag_name("value"))?;
    let path = value.text()?.trim();
    if path.is_empty() || path == "null" { None } else { Some(path.to_string()) }
}

#[allow(clippy::too_many_arguments)]
fn run_export_map(
    space_dir: &Path,
    output: &Path,
    lod: usize,
    no_vfs: bool,
    vfs: Option<&VfsPath>,
    terrain_step: u32,
    no_terrain: bool,
    no_water: bool,
    no_vegetation: bool,
    vegetation_density: f32,
    no_textures: bool,
    max_texture_size: Option<u32>,
) -> Result<(), Report> {
    use wowsunpack::export::gltf_export;
    use wowsunpack::export::texture;
    use wowsunpack::models::assets_bin;
    use wowsunpack::models::forest;
    use wowsunpack::models::geometry;
    use wowsunpack::models::merged_models;
    use wowsunpack::models::speedtree;
    use wowsunpack::models::terrain;

    /// Join a directory path with a filename, handling both VFS and disk paths.
    fn space_file(dir: &Path, name: &str, no_vfs: bool) -> PathBuf {
        if no_vfs {
            dir.join(name)
        } else {
            let s = dir.to_string_lossy().replace('\\', "/");
            PathBuf::from(format!("{s}/{name}"))
        }
    }

    let dir_str = space_dir.to_string_lossy();
    println!("Space: {dir_str}");

    // 1. Load models.geometry.
    let geom_path = space_file(space_dir, "models.geometry", no_vfs);
    let geom_data = read_file_data(&geom_path, no_vfs, vfs).context("Failed to load models.geometry")?;
    let geom = geometry::parse_geometry(&geom_data).context("Failed to parse geometry")?;

    println!(
        "  {} vertex buffers, {} index buffers, {} vertices mappings, {} indices mappings",
        geom.merged_vertices.len(),
        geom.merged_indices.len(),
        geom.vertices_mapping.len(),
        geom.indices_mapping.len(),
    );

    // 2. Load models.bin.
    let models_bin_path = space_file(space_dir, "models.bin", no_vfs);
    let models_bin_data = read_file_data(&models_bin_path, no_vfs, vfs).context("Failed to load models.bin")?;

    let merged = merged_models::parse_merged_models(&models_bin_data).context("Failed to parse models.bin")?;

    println!("  {} model prototypes, {} skeletons", merged.models.len(), merged.skeletons.len());

    // 3. Load space.bin for instance transforms.
    let space_bin_path = space_file(space_dir, "space.bin", no_vfs);

    let space = match read_file_data(&space_bin_path, no_vfs, vfs) {
        Ok(data) => match merged_models::parse_space_instances(&data) {
            Ok(s) => {
                println!("  {} instances", s.instances.len());
                Some(s)
            }
            Err(e) => {
                eprintln!("Warning: could not parse space.bin: {e}");
                None
            }
        },
        Err(_) => {
            eprintln!("Warning: space.bin not found; models will be placed at origin.");
            None
        }
    };

    // 4. Optionally load assets.bin for string resolution (VFS only).
    let assets_bin_data = if let Some(vfs) = vfs {
        let result: Result<Vec<u8>, Report> = (|| {
            let mut buf = Vec::new();
            vfs.join("content/assets.bin")
                .context("VFS path error")?
                .open_file()
                .context("Could not find content/assets.bin in VFS")?
                .read_to_end(&mut buf)?;
            Ok(buf)
        })();
        match result {
            Ok(data) => Some(data),
            Err(_) => {
                eprintln!("Warning: could not load assets.bin; material names will be hex IDs.");
                None
            }
        }
    } else {
        None
    };

    let db = assets_bin_data.as_deref().map(assets_bin::parse_assets_bin).transpose()?;

    // 5. Load space.settings for world bounds.
    let bounds = {
        let settings_path = space_file(space_dir, "space.settings", no_vfs);
        match read_file_data(&settings_path, no_vfs, vfs) {
            Ok(data) => {
                let xml = String::from_utf8_lossy(&data);
                match parse_space_bounds(&xml) {
                    Some(b) => {
                        println!("Space bounds: X [{}, {}], Z [{}, {}]", b.min_x, b.max_x, b.min_z, b.max_z);
                        b
                    }
                    None => {
                        eprintln!("Warning: could not parse bounds from space.settings; using defaults.");
                        gltf_export::SpaceBounds { min_x: -1000.0, max_x: 1000.0, min_z: -1000.0, max_z: 1000.0 }
                    }
                }
            }
            Err(_) => {
                eprintln!("Warning: space.settings not found; using default bounds.");
                gltf_export::SpaceBounds { min_x: -1000.0, max_x: 1000.0, min_z: -1000.0, max_z: 1000.0 }
            }
        }
    };

    // 6. Load space.ubersettings for terrain lightmap path.
    let lightmap_path = if !no_textures && !no_terrain {
        let uber_path = space_file(space_dir, "space.ubersettings", no_vfs);
        match read_file_data(&uber_path, no_vfs, vfs) {
            Ok(data) => {
                let xml = String::from_utf8_lossy(&data);
                parse_lightmap_path(&xml)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // 7. Load terrain.bin if terrain is enabled.
    let terrain_data = if !no_terrain {
        let terrain_path = space_file(space_dir, "terrain.bin", no_vfs);
        match read_file_data(&terrain_path, no_vfs, vfs) {
            Ok(data) => match terrain::parse_terrain(&data) {
                Ok(t) => {
                    println!(
                        "Terrain: {}×{} heightmap ({} tiles, tile_size={})",
                        t.width, t.height, t.tiles_per_axis, t.tile_size
                    );
                    Some(t)
                }
                Err(e) => {
                    eprintln!("Warning: could not parse terrain.bin: {e}");
                    None
                }
            },
            Err(_) => {
                eprintln!("Warning: terrain.bin not found; skipping terrain mesh.");
                None
            }
        }
    } else {
        None
    };

    // 7. Build map environment config.
    let sea_level = 0.0f32;
    let terrain_cfg = terrain_data.as_ref().map(|t| gltf_export::TerrainConfig {
        terrain: t,
        bounds: &bounds,
        step: terrain_step,
        sea_level,
        lightmap_path: lightmap_path.clone(),
    });
    let water_cfg = if !no_water { Some(gltf_export::WaterConfig { bounds: &bounds, sea_level }) } else { None };
    let env = gltf_export::MapEnvironment { terrain: terrain_cfg, water: water_cfg };

    // 8. Load vegetation from forest.bin + stsdk files.
    let vegetation_data = if !no_vegetation {
        let forest_path = space_file(space_dir, "forest.bin", no_vfs);
        match read_file_data(&forest_path, no_vfs, vfs) {
            Ok(data) => match forest::parse_forest(&data) {
                Ok(forest_data) => {
                    println!(
                        "  Forest: {} species, {} instances",
                        forest_data.species.len(),
                        forest_data.instances.len()
                    );

                    let mut species_list = Vec::new();
                    for species_path in &forest_data.species {
                        // Load stsdk file for this species.
                        let stsdk_result: Option<gltf_export::VegetationSpecies> = (|| {
                            let stsdk_data = read_file_data(&PathBuf::from(species_path), no_vfs, vfs).ok()?;
                            let mesh = speedtree::parse_stsdk(&stsdk_data).ok()?;

                            // Derive albedo texture path: replace .stsdk with _spt_a.dds
                            let albedo_png = if !no_textures {
                                let base = species_path.trim_end_matches(".stsdk");
                                // Try _spt_a.dds first, then _spt_a.dd0
                                let tex_path = format!("{base}_spt_a.dds");
                                let tex_data = read_file_data(&PathBuf::from(&tex_path), no_vfs, vfs)
                                    .or_else(|_| {
                                        let dd0_path = format!("{base}_spt_a.dd0");
                                        read_file_data(&PathBuf::from(&dd0_path), no_vfs, vfs)
                                    })
                                    .ok()?;
                                texture::dds_to_png_resized(&tex_data, max_texture_size).ok()
                            } else {
                                None
                            };

                            Some(gltf_export::VegetationSpecies { mesh, albedo_png })
                        })();

                        match stsdk_result {
                            Some(species) => species_list.push(species),
                            None => {
                                eprintln!("  Warning: failed to load vegetation species: {species_path}");
                                // Push empty placeholder to keep indices aligned
                                species_list.push(gltf_export::VegetationSpecies {
                                    mesh: speedtree::SpeedTreeMesh {
                                        positions: Vec::new(),
                                        normals: Vec::new(),
                                        uvs: Vec::new(),
                                        indices: Vec::new(),
                                    },
                                    albedo_png: None,
                                });
                            }
                        }
                    }

                    let instances: Vec<(usize, [f32; 3])> = forest_data
                        .instances
                        .iter()
                        .map(|inst| (inst.species_index, [inst.x, inst.y, inst.z]))
                        .collect();

                    Some(gltf_export::VegetationData { species: species_list, instances })
                }
                Err(e) => {
                    eprintln!("Warning: could not parse forest.bin: {e}");
                    None
                }
            },
            Err(_) => {
                eprintln!("Warning: forest.bin not found; skipping vegetation.");
                None
            }
        }
    } else {
        None
    };

    // 9. Build the format-agnostic MapScene.
    let vfs_for_textures = if no_textures { None } else { vfs };
    let scene = gltf_export::build_map_scene(&gltf_export::BuildMapSceneParams {
        merged: &merged,
        geometry: &geom,
        space: space.as_ref(),
        db: db.as_ref(),
        lod,
        vfs: vfs_for_textures,
        env: &env,
        bounds: bounds.clone(),
        max_texture_size,
        vegetation: vegetation_data.as_ref(),
        vegetation_density,
    })
    .context("Failed to build map scene")?;

    // 9. Export to GLB.
    let mut out_file = std::fs::File::create(output).context("Failed to create output file")?;
    gltf_export::export_map_scene_glb(&scene, &mut out_file).context("Failed to export map scene GLB")?;

    let file_size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    println!("Exported to {} ({} bytes)", output.display(), file_size);

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_export_ship(
    vfs: &VfsPath,
    name: &str,
    output: &Path,
    lod: usize,
    game_dir: &Path,
    game_version: Option<u64>,
    list_upgrades: bool,
    hull_selection: Option<&str>,
    no_textures: bool,
    damaged: bool,
    list_textures: bool,
    debug: bool,
) -> Result<(), Report> {
    use wowsunpack::export::ship::ShipAssets;
    use wowsunpack::export::ship::ShipExportOptions;

    let mut assets = ShipAssets::load(vfs)?;

    // Load translations if available.
    if let Some(version) = game_version {
        let mo_path = wowsunpack::game_data::translations_path(game_dir, version as u32);
        if let Ok(data) = std::fs::read(&mo_path)
            && let Ok(catalog) = gettext::Catalog::parse(&*data)
        {
            assets.set_translations(catalog);
        }
    }

    if list_upgrades {
        let upgrades = assets.list_hull_upgrades(name)?;
        if upgrades.is_empty() {
            println!("No hull upgrades found for '{name}'.");
        } else {
            println!("Hull upgrades for '{name}':");
            for (i, upgrade) in upgrades.iter().enumerate() {
                println!("  [{}] {}", i + 1, upgrade.name);
                for (ct, comp, mount_count) in &upgrade.components {
                    if comp != "(none)" {
                        println!("      {ct}: {comp} ({mount_count} mounts)");
                    } else {
                        println!("      {ct}: (none)");
                    }
                }
            }
        }
        return Ok(());
    }

    if list_textures {
        let schemes = assets.list_texture_schemes(name)?;
        if schemes.is_empty() {
            println!("No camouflage textures found for this ship.");
        } else {
            println!("Available camouflage schemes:");
            for scheme in &schemes {
                println!("  {scheme}");
            }
        }
        return Ok(());
    }

    let options = ShipExportOptions {
        lod,
        hull: hull_selection.map(|s| s.to_string()),
        textures: !no_textures,
        damaged,
        ..Default::default()
    };
    let ctx = assets.load_ship(name, &options)?;

    println!(
        "Found {} hull parts, {} mounts ({} unique turrets)",
        ctx.hull_part_names().len(),
        ctx.mount_count(),
        ctx.unique_turret_count()
    );

    let has_armor = ctx.armor_map().is_some() || ctx.hull_splash_bytes().is_some();

    wowsunpack::export::set_debug(debug);
    let mut file = std::fs::File::create(output).context("Failed to create output file")?;
    ctx.export_glb(&mut file)?;

    let file_size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    println!("Exported to {} ({} bytes)", output.display(), file_size);

    if has_armor {
        print_armor_legend();
    }

    Ok(())
}

fn print_armor_legend() {
    use wowsunpack::export::gltf_export;

    println!();
    println!("Armor thickness color scale:");
    println!("     0    mm  light gray (no assigned thickness)");
    for entry in gltf_export::armor_color_legend() {
        println!("  {:>4}–{:>4} mm  {}", entry.min_mm as u32, entry.max_mm as u32, entry.color_name);
    }
}

fn run_armor(
    vfs: &VfsPath,
    name: &str,
    game_dir: &Path,
    game_version: Option<u64>,
    hull_selection: Option<&str>,
) -> Result<(), Report> {
    use wowsunpack::export::ship::ShipAssets;
    use wowsunpack::export::ship::ShipExportOptions;
    use wowsunpack::models::geometry;

    let mut assets = ShipAssets::load(vfs)?;

    // Load translations if available.
    if let Some(version) = game_version {
        let mo_path = wowsunpack::game_data::translations_path(game_dir, version as u32);
        if let Ok(data) = std::fs::read(&mo_path)
            && let Ok(catalog) = gettext::Catalog::parse(&*data)
        {
            assets.set_translations(catalog);
        }
    }

    let options =
        ShipExportOptions { hull: hull_selection.map(|s| s.to_string()), textures: false, ..Default::default() };
    let ctx = assets.load_ship(name, &options)?;
    let info = ctx.info();
    println!("Ship: {} ({})", info.display_name.as_deref().unwrap_or("?"), info.model_dir);

    let armor_map = ctx.armor_map();

    use wowsunpack::export::gltf_export::collision_material_name;
    use wowsunpack::export::gltf_export::zone_from_material_name;

    // Helper to print armor layer info for materials found in a geometry.
    fn print_armor_layers(
        geom_mat_ids: &std::collections::BTreeSet<u8>,
        amap: &std::collections::HashMap<u32, std::collections::BTreeMap<u32, f32>>,
    ) {
        let mut matched = 0usize;
        for &mid in geom_mat_ids {
            if let Some(layers_map) = amap.get(&(mid as u32)) {
                let layers: Vec<f32> = layers_map.values().copied().collect();
                let total: f32 = layers.iter().sum();
                if total > 0.0 {
                    matched += 1;
                    let mat_name = collision_material_name(mid);
                    let zone = zone_from_material_name(mat_name);
                    let hidden = matches!(zone, "Hull" | "SteeringGear" | "Default");
                    let tag = if hidden { "  [HIDDEN]" } else { "" };
                    let idx_str: Vec<String> = layers_map.iter().map(|(k, v)| format!("mi{k}={v:.0}")).collect();
                    if layers.len() == 1 {
                        println!(
                            "      mat {:>3} ({:<20}) = {:>6.1} mm  [{}]{}",
                            mid,
                            mat_name,
                            total,
                            idx_str.join(", "),
                            tag,
                        );
                    } else {
                        let layer_str: Vec<String> = layers.iter().map(|v| format!("{v:.0}")).collect();
                        println!(
                            "      mat {:>3} ({:<20}) = {:>6.1} mm  (layers: [{}])  [{}]{}",
                            mid,
                            mat_name,
                            total,
                            layer_str.join(", "),
                            idx_str.join(", "),
                            tag,
                        );
                    }
                }
            }
        }
        if matched == 0 {
            println!("      (no GameParams thickness entries for these materials)");
        }
    }

    // Parse geometry for each hull part to inspect armor models.
    let mut armor_model_count = 0u32;
    let mut total_tris = 0usize;

    for (part_name, geom_bytes) in ctx.hull_part_names().iter().zip(ctx.hull_geom_bytes()) {
        let geom = geometry::parse_geometry(geom_bytes)?;

        if geom.armor_models.is_empty() {
            continue;
        }

        println!("\nHull part: {part_name}");
        for am in &geom.armor_models {
            armor_model_count += 1;
            total_tris += am.triangles.len();

            // Compute bounding box and collect material IDs.
            let mut bmin = [f32::MAX; 3];
            let mut bmax = [f32::MIN; 3];
            let mut geom_mat_ids = std::collections::BTreeSet::new();
            for tri in &am.triangles {
                geom_mat_ids.insert(tri.material_id);
                for v in &tri.vertices {
                    for i in 0..3 {
                        bmin[i] = bmin[i].min(v[i]);
                        bmax[i] = bmax[i].max(v[i]);
                    }
                }
            }

            println!("  Armor model: \"{}\"", am.name);
            println!("    Triangles: {}", am.triangles.len());
            println!(
                "    Bounding box: ({:.2}, {:.2}, {:.2}) to ({:.2}, {:.2}, {:.2})",
                bmin[0], bmin[1], bmin[2], bmax[0], bmax[1], bmax[2]
            );
            println!("    Materials: {:?}", geom_mat_ids.iter().collect::<Vec<_>>());

            if let Some(amap) = armor_map {
                println!("    GameParams thickness:");
                print_armor_layers(&geom_mat_ids, amap);
            } else {
                println!("    GameParams: no armor data available");
            }

            // Print per-layer bounding boxes for multi-layer materials.
            {
                // Group triangles by (material_id, layer_index) -> (count, bbox_min, bbox_max)
                #[allow(clippy::type_complexity)]
                let mut layer_groups: std::collections::BTreeMap<
                    (u8, u8),
                    (usize, [f32; 3], [f32; 3]),
                > = Default::default();
                for tri in &am.triangles {
                    let key = (tri.material_id, tri.layer_index);
                    let entry = layer_groups.entry(key).or_insert((0, [f32::MAX; 3], [f32::MIN; 3]));
                    entry.0 += 1; // triangle count
                    for v in &tri.vertices {
                        for (i, &coord) in v.iter().enumerate() {
                            entry.1[i] = entry.1[i].min(coord);
                            entry.2[i] = entry.2[i].max(coord);
                        }
                    }
                }

                // Only print if there are any multi-layer materials
                let multi_layer_mats: std::collections::BTreeSet<u8> = layer_groups
                    .keys()
                    .map(|(mid, _)| *mid)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .filter(|mid| layer_groups.keys().filter(|(m, _)| m == mid).count() > 1)
                    .collect();

                if !multi_layer_mats.is_empty() {
                    println!("    Per-layer spatial extent (multi-layer materials):");
                    for &mid in &multi_layer_mats {
                        let mat_name = collision_material_name(mid);
                        for (&(m, layer), &(count, ref lmin, ref lmax)) in &layer_groups {
                            if m != mid {
                                continue;
                            }
                            println!(
                                "      mat {:>3} layer {} ({:<20}): {:>4} tris, Y [{:.2} .. {:.2}], Z [{:.2} .. {:.2}]",
                                mid, layer, mat_name, count, lmin[1], lmax[1], lmin[2], lmax[2],
                            );
                        }
                    }
                }
            }
        }
    }

    // Turret armor models.
    let turret_names = ctx.turret_model_names();
    for (turret_name, geom_bytes) in turret_names.iter().zip(ctx.turret_geom_bytes()) {
        let geom = geometry::parse_geometry(geom_bytes)?;

        if geom.armor_models.is_empty() {
            continue;
        }

        println!("\nTurret model: {turret_name}");
        for am in &geom.armor_models {
            armor_model_count += 1;
            total_tris += am.triangles.len();

            let mut bmin = [f32::MAX; 3];
            let mut bmax = [f32::MIN; 3];
            let mut geom_mat_ids = std::collections::BTreeSet::new();
            for tri in &am.triangles {
                geom_mat_ids.insert(tri.material_id);
                for v in &tri.vertices {
                    for i in 0..3 {
                        bmin[i] = bmin[i].min(v[i]);
                        bmax[i] = bmax[i].max(v[i]);
                    }
                }
            }

            println!("  Armor model: \"{}\"", am.name);
            println!("    Triangles: {}", am.triangles.len());
            println!(
                "    Bounding box: ({:.2}, {:.2}, {:.2}) to ({:.2}, {:.2}, {:.2})",
                bmin[0], bmin[1], bmin[2], bmax[0], bmax[1], bmax[2]
            );
            println!("    Materials: {:?}", geom_mat_ids.iter().collect::<Vec<_>>());

            if let Some(amap) = armor_map {
                println!("    GameParams thickness:");
                print_armor_layers(&geom_mat_ids, amap);
            }
        }
    }

    // Zone classification by collision material ID.
    {
        use wowsunpack::export::gltf_export::zone_from_material_name;
        println!("\nZone classification (by collision material):");

        let mut zone_counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        let all_geom_bytes: Vec<&[u8]> = ctx.hull_geom_bytes().into_iter().chain(ctx.turret_geom_bytes()).collect();
        for geom_bytes in &all_geom_bytes {
            if let Ok(geom) = geometry::parse_geometry(geom_bytes) {
                for am in &geom.armor_models {
                    for tri in &am.triangles {
                        let mat_name = collision_material_name(tri.material_id);
                        let zone = zone_from_material_name(mat_name).to_string();
                        *zone_counts.entry(zone).or_default() += 1;
                    }
                }
            }
        }
        for (zone, count) in &zone_counts {
            println!("  {:>20}: {:>5} triangles", zone, count);
        }
    }

    // Summary.
    if let Some(amap) = armor_map {
        let total_materials = amap.len();
        let multi_layer = amap.values().filter(|v| v.len() > 1).count();
        println!(
            "\nSummary: {} armor triangles across {} model(s), GameParams: {} materials ({} multi-layer)",
            total_tris, armor_model_count, total_materials, multi_layer
        );
    } else {
        println!("\nSummary: {} armor triangles across {} model(s), no GameParams data", total_tris, armor_model_count);
    }

    println!("\nTurret armor triangle materials:");
    let interactive = ctx.interactive_armor_meshes()?;
    for mesh in &interactive {
        if mesh.transform.is_none() {
            continue; // skip hull
        }
        // Key by (material_id, model_index) to show per-layer thickness.
        let mut mat_summary: std::collections::BTreeMap<(u8, u32, String), (usize, f32)> = Default::default();
        for ti in &mesh.triangle_info {
            let entry = mat_summary
                .entry((ti.material_id, ti.model_index, ti.material_name.clone()))
                .or_insert((0, ti.thickness_mm));
            entry.0 += 1;
        }
        println!("  {}:", mesh.name);
        for ((mid, layer, mname), (count, thickness)) in &mat_summary {
            println!("    mat {:>3} layer {} ({:<24}) x{:<4} = {:>6.1} mm", mid, layer, mname, count, thickness);
        }
    }

    Ok(())
}

fn main() -> Result<(), Report> {
    let timestamp = Instant::now();

    run()?;

    println!("Finished in {} seconds", (Instant::now() - timestamp).as_secs_f32());

    Ok(())
}

fn run_dump_uvs(
    vfs: &VfsPath,
    name: &str,
    _game_dir: &Path,
    _game_version: Option<u64>,
    _hull_selection: Option<&str>,
) -> Result<(), Report> {
    use wowsunpack::models::geometry;
    use wowsunpack::models::vertex_format;

    // Try to find and extract .geometry files matching the name from VFS.
    let mut geom_files: Vec<(String, Vec<u8>)> = Vec::new();

    // Instead of walking VFS (slow), try direct path construction.
    // The user provides a directory name like "JSB001_Kawachi_1912".
    // Geometry files are at content/gameplay/{nation}/ship/{type}/{name}/{name}_{part}.geometry
    // But we do not know nation/type. Just search for the name in known locations.
    let nations = [
        "japan",
        "usa",
        "germany",
        "uk",
        "france",
        "ussr",
        "italy",
        "pan_america",
        "pan_asia",
        "europe",
        "netherlands",
        "spain",
        "commonwealth",
    ];
    let ship_types = ["battleship", "cruiser", "destroyer", "aircarrier", "submarine"];
    let parts = ["", "_Bow", "_MidFront", "_MidBack", "_Stern"];

    for nation in &nations {
        for stype in &ship_types {
            for part in &parts {
                let path = format!("content/gameplay/{nation}/ship/{stype}/{name}/{name}{part}.geometry");
                if let Ok(file_path) = vfs.join(&path)
                    && let Ok(mut f) = file_path.open_file()
                {
                    let mut data = Vec::new();
                    if f.read_to_end(&mut data).is_ok() && !data.is_empty() {
                        geom_files.push((path, data));
                    }
                }
            }
        }
    }

    if geom_files.is_empty() {
        bail!(
            "No .geometry files found for '{}'. Try using the full model directory name (e.g. JSB001_Kawachi_1912).",
            name
        );
    }

    println!(
        "Found {} geometry files for '{}':
",
        geom_files.len(),
        name
    );

    for (path, data) in &geom_files {
        println!("=== {} ({} bytes) ===", path, data.len());

        let geom = match geometry::parse_geometry(data) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("  Failed to parse: {}", e);
                continue;
            }
        };

        println!("  Vertex buffers: {}  Index buffers: {}", geom.merged_vertices.len(), geom.merged_indices.len());
        println!("  Vertex mappings: {}  Index mappings: {}", geom.vertices_mapping.len(), geom.indices_mapping.len());
        for (mi, vm) in geom.vertices_mapping.iter().enumerate() {
            println!(
                "    vert_mapping[{}]: id={} buf={} offset={} count={}",
                mi, vm.mapping_id, vm.merged_buffer_index, vm.items_offset, vm.items_count
            );
        }
        for (mi, im) in geom.indices_mapping.iter().enumerate() {
            println!(
                "    idx_mapping[{}]: id={} buf={} offset={} count={}",
                mi, im.mapping_id, im.merged_buffer_index, im.items_offset, im.items_count
            );
        }
        // Check ALL index ranges against vertex buffers
        // Decode index buffer(s) and check max index vs total vertex count
        for (bi, ibuf) in geom.merged_indices.iter().enumerate() {
            let decoded_i = match ibuf.data.decode() {
                Ok(d) => d,
                Err(_) => continue,
            };
            let idx_size = ibuf.index_size as usize;
            let total_indices = decoded_i.len() / idx_size;
            let max_idx: u32 = match idx_size {
                2 => decoded_i.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).max().unwrap_or(0),
                4 => decoded_i.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).max().unwrap_or(0),
                _ => continue,
            };
            println!("    Index buf {}: {} indices, max_idx={}", bi, total_indices, max_idx);
            // Check each index mapping range
            for im in geom.indices_mapping.iter().filter(|m| m.merged_buffer_index as usize == bi) {
                let idx_start = im.items_offset as usize * idx_size;
                let idx_end = idx_start + im.items_count as usize * idx_size;
                if idx_end > decoded_i.len() {
                    continue;
                }
                let idx_slice = &decoded_i[idx_start..idx_end];
                let max_i: u32 = match idx_size {
                    2 => idx_slice.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).max().unwrap_or(0),
                    4 => idx_slice
                        .chunks_exact(4)
                        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .max()
                        .unwrap_or(0),
                    _ => continue,
                };
                let min_i: u32 = match idx_size {
                    2 => idx_slice.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).min().unwrap_or(0),
                    4 => idx_slice
                        .chunks_exact(4)
                        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .min()
                        .unwrap_or(0),
                    _ => continue,
                };
                println!(
                    "      idx_map id={}: offset={} count={} idx_range=[{}, {}]",
                    im.mapping_id, im.items_offset, im.items_count, min_i, max_i
                );
            }
        }
        // For each vertex mapping, show which vertex ranges exist
        for vbuf_idx in 0..geom.merged_vertices.len() {
            let vbuf = &geom.merged_vertices[vbuf_idx];
            let decoded_v = match vbuf.data.decode() {
                Ok(d) => d,
                Err(_) => continue,
            };
            let stride = vbuf.stride_in_bytes as usize;
            let total_verts = decoded_v.len() / stride;
            println!("    Vertex buf {}: {} total verts (format={})", vbuf_idx, total_verts, vbuf.format_name);
            for vm in geom.vertices_mapping.iter().filter(|m| m.merged_buffer_index as usize == vbuf_idx) {
                println!(
                    "      vert_map id={}: offset={} count={} range=[{}, {})",
                    vm.mapping_id,
                    vm.items_offset,
                    vm.items_count,
                    vm.items_offset,
                    vm.items_offset + vm.items_count
                );
            }
        }

        for (buf_idx, vert_proto) in geom.merged_vertices.iter().enumerate() {
            let format = vertex_format::parse_vertex_format(&vert_proto.format_name);
            let stride = vert_proto.stride_in_bytes as usize;

            let uv_attr = format.attributes.iter().find(|a| a.semantic == vertex_format::AttributeSemantic::TexCoord0);
            let uv_attr = match uv_attr {
                Some(a) => a,
                None => {
                    println!("  Buffer {}: format='{}' stride={} (no UV)", buf_idx, vert_proto.format_name, stride);
                    continue;
                }
            };

            let decoded = match vert_proto.data.decode() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("  Buffer {}: decode error: {}", buf_idx, e);
                    continue;
                }
            };

            let count = decoded.len() / stride;
            println!(
                "  Buffer {}: format='{}' stride={} vertices={} uv_offset={}",
                buf_idx, vert_proto.format_name, stride, count, uv_attr.offset
            );

            if count == 0 {
                continue;
            }

            let mut f16_u_min = f32::MAX;
            let mut f16_u_max = f32::MIN;
            let mut f16_v_min = f32::MAX;
            let mut f16_v_max = f32::MIN;
            let mut sn_u_min = f32::MAX;
            let mut sn_u_max = f32::MIN;
            let mut sn_v_min = f32::MAX;
            let mut sn_v_max = f32::MIN;

            let sample_count = count.min(15);
            let mut samples = Vec::new();

            for i in 0..count {
                let off = i * stride + uv_attr.offset;
                if off + 4 > decoded.len() {
                    break;
                }
                let raw = &decoded[off..off + 4];
                let u_bits = u16::from_le_bytes([raw[0], raw[1]]);
                let v_bits = u16::from_le_bytes([raw[2], raw[3]]);

                let u_f16 = half::f16::from_bits(u_bits).to_f32();
                let v_f16 = half::f16::from_bits(v_bits).to_f32();
                f16_u_min = f16_u_min.min(u_f16);
                f16_u_max = f16_u_max.max(u_f16);
                f16_v_min = f16_v_min.min(v_f16);
                f16_v_max = f16_v_max.max(v_f16);

                let u_sn = (u_bits as i16) as f32 / 32767.0;
                let v_sn = (v_bits as i16) as f32 / 32767.0;
                sn_u_min = sn_u_min.min(u_sn);
                sn_u_max = sn_u_max.max(u_sn);
                sn_v_min = sn_v_min.min(v_sn);
                sn_v_max = sn_v_max.max(v_sn);

                if i < sample_count {
                    samples.push((i, u_bits, v_bits, u_f16, v_f16, u_sn, v_sn));
                }
            }

            println!(
                "    float16 range:  u=[{:.6}, {:.6}] v=[{:.6}, {:.6}]  span=({:.4}, {:.4})",
                f16_u_min,
                f16_u_max,
                f16_v_min,
                f16_v_max,
                f16_u_max - f16_u_min,
                f16_v_max - f16_v_min
            );
            println!(
                "    snorm16 range:  u=[{:.6}, {:.6}] v=[{:.6}, {:.6}]  span=({:.4}, {:.4})",
                sn_u_min,
                sn_u_max,
                sn_v_min,
                sn_v_max,
                sn_u_max - sn_u_min,
                sn_v_max - sn_v_min
            );

            println!("    Samples:");
            println!(
                "    {:>5}  {:>6} {:>6}  {:>10} {:>10}  {:>10} {:>10}",
                "idx", "u_raw", "v_raw", "f16_u", "f16_v", "sn16_u", "sn16_v"
            );
            for &(i, ub, vb, uf, vf, us, vs) in &samples {
                println!(
                    "    {:>5}  0x{:04X} 0x{:04X}  {:>10.6} {:>10.6}  {:>10.6} {:>10.6}",
                    i, ub, vb, uf, vf, us, vs
                );
            }
            // Also dump first 5 vertices with BOTH position and UV
            let pos_attr = format.attributes.iter().find(|a| a.semantic == vertex_format::AttributeSemantic::Position);
            if let Some(pa) = pos_attr {
                println!("    Position+UV correlation (first 10 verts):");
                for i in 0..count.min(10) {
                    let poff = i * stride + pa.offset;
                    let uoff = i * stride + uv_attr.offset;
                    if poff + 12 > decoded.len() || uoff + 4 > decoded.len() {
                        break;
                    }
                    let x = f32::from_le_bytes(decoded[poff..poff + 4].try_into().unwrap());
                    let y = f32::from_le_bytes(decoded[poff + 4..poff + 8].try_into().unwrap());
                    let z = f32::from_le_bytes(decoded[poff + 8..poff + 12].try_into().unwrap());
                    let raw = &decoded[uoff..uoff + 4];
                    let u_bits = u16::from_le_bytes([raw[0], raw[1]]);
                    let v_bits = u16::from_le_bytes([raw[2], raw[3]]);
                    let u = half::f16::from_bits(u_bits).to_f32();
                    let v = half::f16::from_bits(v_bits).to_f32();
                    println!("      v[{}]: pos=({:.2}, {:.2}, {:.2}) uv=({:.6}, {:.6})", i, x, y, z, u, v);
                }
                println!();
            }
        }
    }

    Ok(())
}
