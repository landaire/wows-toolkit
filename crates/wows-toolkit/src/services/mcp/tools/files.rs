//! The files of the game builds on this machine: installed builds and builds in the game data
//! cache, each read through its virtual file system, which has no path above its root.

use std::collections::BTreeMap;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Semaphore;
use wows_data_mgr::builds::BuildsIndex;
use wows_data_mgr::cas_vfs::BuildCas;
use wows_mcp::tool_set::typed_tool;
use wowsunpack::game_data::AssetsBin;
use wowsunpack::vfs::VfsPath;

use crate::services::mcp::tools::Page;
use crate::services::mcp::tools::ReplayTools;
use crate::services::mcp::tools::ToolContext;
use crate::services::mcp::tools::blocking;
use crate::services::mcp::tools::error::ToolError;

pub const BUILDS: &str = "game_builds";
pub const LIST: &str = "game_files_list";
pub const READ: &str = "game_file_read";

/// The file systems opened most recently. Opening an installed build reads its package indexes,
/// which takes a moment and holds their tables in memory, so only a few are kept.
pub struct BuildFiles {
    opened: Mutex<Vec<(u32, VfsPath)>>,
    /// Bounds the files read at once: reading any part of a packaged file decompresses all of it.
    reads: Arc<Semaphore>,
}

impl BuildFiles {
    const KEPT: usize = 2;
    const PARALLEL_READS: usize = 2;

    pub fn new() -> Self {
        Self { opened: Mutex::new(Vec::new()), reads: Arc::new(Semaphore::new(Self::PARALLEL_READS)) }
    }

    async fn vfs(&self, context: &ToolContext, build: u32) -> Result<VfsPath, ToolError> {
        let kept = {
            let opened = self.opened.lock().unwrap_or_else(PoisonError::into_inner);
            opened.iter().find(|(kept, _)| *kept == build).map(|(_, vfs)| vfs.clone())
        };
        if let Some(vfs) = kept {
            return Ok(vfs);
        }

        let context = context.clone();
        let vfs = blocking(move || open_build(&context, build)).await??;
        {
            let mut opened = self.opened.lock().unwrap_or_else(PoisonError::into_inner);
            opened.retain(|(kept, _)| *kept != build);
            opened.insert(0, (build, vfs.clone()));
            opened.truncate(Self::KEPT);
        }

        Ok(vfs)
    }
}

/// `build`'s file system: one the app has loaded, else the install's, else the cache's.
fn open_build(context: &ToolContext, build: u32) -> Result<VfsPath, ToolError> {
    if let Some(loaded) = context.game_data.as_ref().and_then(|cache| cache.loaded_build(build)) {
        return Ok(loaded.vfs().clone());
    }

    if let Some(wows_dir) = &context.wows_dir
        && wows_dir.join("bin").join(build.to_string()).join("idx").is_dir()
    {
        return wowsunpack::game_data::build_game_vfs_for_build(wows_dir, build, AssetsBin::Omit)
            .map_err(|report| ToolError::BuildFiles { build, reason: report.to_string() });
    }

    let Some(dump_base) = context.dump_base.as_deref() else {
        return Err(ToolError::UnknownBuild { build });
    };
    let index = cache_index(dump_base)?;
    let Some(cached) = index.find_by_build(build) else {
        return Err(ToolError::UnknownBuild { build });
    };
    let dump_dir = dump_base.join(&cached.dir);
    let cas = BuildCas::open(&dump_dir).ok_or_else(|| ToolError::BuildFiles {
        build,
        reason: format!("{} has no readable metadata.toml", dump_dir.display()),
    })?;

    Ok(cas.vfs())
}

/// The game data cache's list of builds. A cache that was never written lists none.
fn cache_index(dump_base: &Path) -> Result<BuildsIndex, ToolError> {
    BuildsIndex::load(&dump_base.join("builds.toml")).map_err(|source| ToolError::CacheIndex { source })
}

/// A path inside a build, as the caller wrote it, checked and made relative to the root.
fn vfs_path(text: &str) -> Result<String, ToolError> {
    let normalized = text.replace('\\', "/");
    if normalized.split('/').any(|segment| segment == "..") {
        return Err(ToolError::ParentComponent { path: text.to_owned() });
    }
    // A cached build's file system is backed by the disk, where a drive prefix or a NUL would name
    // something other than a file under its root.
    if normalized.contains([':', '\0']) {
        return Err(ToolError::ForbiddenCharacter { path: text.to_owned() });
    }

    Ok(normalized.trim_matches('/').to_owned())
}

fn entry(vfs: &VfsPath, build: u32, path: &str) -> Result<VfsPath, ToolError> {
    let joined = if path.is_empty() { Ok(vfs.clone()) } else { vfs.join(path) };
    let entry = joined.map_err(|_| ToolError::NotFound { build, path: path.to_owned() })?;
    if !entry.exists().unwrap_or(false) {
        return Err(ToolError::NotFound { build, path: path.to_owned() });
    }

    Ok(entry)
}

/// Arguments for `game_builds`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildsArgs {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BuildsAnswer {
    /// Newest first.
    pub builds: Vec<KnownBuild>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KnownBuild {
    pub build: u32,
    /// The game version, when the game data cache records it.
    pub version: Option<String>,
    /// In the game install's `bin` directory.
    pub installed: bool,
    /// In the game data cache.
    pub cached: bool,
}

/// Arguments for `game_files_list`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListArgs {
    /// A build from `game_builds`.
    pub build: u32,

    /// A directory inside the build, such as `gui/fla` or `content`. The root when absent.
    #[serde(default)]
    pub path: Option<String>,

    /// Entries to skip, for the next page.
    #[serde(default)]
    pub offset: Option<u32>,

    /// Entries to return, 1 to 1000. Default 200.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListAnswer {
    pub build: u32,
    pub path: String,
    /// Every entry of the directory, before paging.
    pub total: usize,
    /// Sorted by name.
    pub entries: Vec<FileEntry>,
    /// The offset of the next page, when there are more entries.
    pub next_offset: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FileEntry {
    pub name: String,
    /// The path to pass to `game_files_list` or `game_file_read`.
    pub path: String,
    pub kind: EntryKind,
    /// In bytes, for a file.
    pub size: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
}

/// Arguments for `game_file_read`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadArgs {
    /// A build from `game_builds`.
    pub build: u32,

    /// A file inside the build, such as `gui/data/constants/battle.xml`.
    pub path: String,

    /// The byte to start at. Default 0.
    #[serde(default)]
    pub offset: Option<u64>,

    /// Bytes to read, 1 to 1048576. Default 262144.
    #[serde(default)]
    pub length: Option<u32>,

    /// How to return the bytes. Default `auto`: text when they are UTF-8, else base64.
    #[serde(default)]
    pub encoding: Option<Encoding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    Auto,
    Text,
    Base64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadAnswer {
    pub build: u32,
    pub path: String,
    /// The whole file's size in bytes.
    pub size: u64,
    pub offset: u64,
    /// Bytes returned.
    pub length: u64,
    pub encoding: ContentEncoding,
    pub content: String,
    /// The offset to read the rest from, when the file goes on.
    pub next_offset: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContentEncoding {
    Text,
    Base64,
}

pub fn builds_tool() -> Tool {
    typed_tool::<BuildsArgs, BuildsAnswer>(
        BUILDS,
        "The game builds whose files can be read: those installed in the game directory and those in WoWs Toolkit's game data cache.",
    )
}

pub fn list_tool() -> Tool {
    typed_tool::<ListArgs, ListAnswer>(
        LIST,
        "List a directory of a game build's packaged files, as the game reads them. Paginated.",
    )
}

pub fn read_tool() -> Tool {
    typed_tool::<ReadArgs, ReadAnswer>(
        READ,
        "Read part of a file from a game build's packaged files, up to 1 MiB per call, as text or base64. Continue from next_offset for the rest.",
    )
}

pub async fn builds(tools: &ReplayTools, _args: BuildsArgs) -> Result<BuildsAnswer, ToolError> {
    let context = tools.context();
    let builds = blocking(move || {
        let mut known: BTreeMap<u32, KnownBuild> = BTreeMap::new();
        let installed = context
            .wows_dir
            .as_deref()
            .and_then(|dir| wowsunpack::game_data::list_available_builds(dir).ok())
            .unwrap_or_default();
        for build in installed {
            known.insert(build, KnownBuild { build, version: None, installed: true, cached: false });
        }

        let cached = context
            .dump_base
            .as_deref()
            .and_then(|base| BuildsIndex::load(&base.join("builds.toml")).ok())
            .map(|index| index.builds)
            .unwrap_or_default();
        for entry in cached {
            let known = known.entry(entry.build).or_insert(KnownBuild {
                build: entry.build,
                version: None,
                installed: false,
                cached: false,
            });
            known.cached = true;
            known.version = Some(entry.version);
        }

        known.into_values().rev().collect()
    })
    .await?;

    Ok(BuildsAnswer { builds })
}

pub async fn list(tools: &ReplayTools, args: ListArgs) -> Result<ListAnswer, ToolError> {
    let page = Page::new(args.offset, args.limit, 200, 1000)?;
    let path = vfs_path(args.path.as_deref().unwrap_or(""))?;
    let vfs = tools.files.vfs(&tools.context(), args.build).await?;

    let build = args.build;
    let listed = path.clone();
    let entries = blocking(move || -> Result<Vec<FileEntry>, ToolError> {
        let directory = entry(&vfs, build, &listed)?;
        if !directory.is_dir().unwrap_or(false) {
            return Err(ToolError::NotADirectory { path: listed });
        }
        let children = directory
            .read_dir()
            .map_err(|error| ToolError::FileRead { path: listed.clone(), reason: error.to_string() })?;

        let mut entries: Vec<FileEntry> = children
            .map(|child| {
                let is_dir = child.is_dir().unwrap_or(false);
                FileEntry {
                    name: child.filename(),
                    path: child.as_str().trim_start_matches('/').to_owned(),
                    kind: if is_dir { EntryKind::Directory } else { EntryKind::File },
                    size: (!is_dir).then(|| child.metadata().ok().map(|metadata| metadata.len)).flatten(),
                }
            })
            .collect();
        entries.sort_by(|left, right| left.name.cmp(&right.name));

        Ok(entries)
    })
    .await??;

    let total = entries.len();
    let (entries, next_offset) = page.slice(entries);

    Ok(ListAnswer { build, path, total, entries, next_offset })
}

pub async fn read(tools: &ReplayTools, args: ReadArgs) -> Result<ReadAnswer, ToolError> {
    const DEFAULT_LENGTH: u32 = 256 * 1024;
    const MAX_LENGTH: u32 = 1024 * 1024;

    let length = args.length.unwrap_or(DEFAULT_LENGTH);
    if length == 0 || length > MAX_LENGTH {
        return Err(ToolError::InvalidArgument { message: format!("length must be from 1 to {MAX_LENGTH}") });
    }
    let offset = args.offset.unwrap_or(0);
    let encoding = args.encoding.unwrap_or(Encoding::Auto);
    let path = vfs_path(&args.path)?;
    let vfs = tools.files.vfs(&tools.context(), args.build).await?;

    // Moved into the blocking read, which runs on after a cancelled call.
    let permit = Arc::clone(&tools.files.reads)
        .acquire_owned()
        .await
        .map_err(|error| ToolError::Task { reason: error.to_string() })?;

    let build = args.build;
    let read = path.clone();
    let (size, bytes) = blocking(move || -> Result<(u64, Vec<u8>), ToolError> {
        let _permit = permit;
        let file = entry(&vfs, build, &read)?;
        if file.is_dir().unwrap_or(false) {
            return Err(ToolError::IsDirectory { path: read });
        }
        let failed =
            |error: &dyn std::fmt::Display| ToolError::FileRead { path: read.clone(), reason: error.to_string() };
        let size = file.metadata().map_err(|error| failed(&error))?.len;
        let mut reader = file.open_file().map_err(|error| failed(&error))?;
        reader.seek(SeekFrom::Start(offset)).map_err(|error| failed(&error))?;
        let mut bytes = Vec::with_capacity(length as usize);
        reader.take(u64::from(length)).read_to_end(&mut bytes).map_err(|error| failed(&error))?;

        Ok((size, bytes))
    })
    .await??;

    let read_length = bytes.len() as u64;
    let (encoding, content) = match (encoding, String::from_utf8(bytes)) {
        (Encoding::Auto | Encoding::Text, Ok(text)) => (ContentEncoding::Text, text),
        (Encoding::Text, Err(error)) => {
            return Err(ToolError::NotText { path, at: offset + error.utf8_error().valid_up_to() as u64 });
        }
        (Encoding::Auto, Err(error)) => (ContentEncoding::Base64, data_encoding::BASE64.encode(error.as_bytes())),
        (Encoding::Base64, Ok(text)) => (ContentEncoding::Base64, data_encoding::BASE64.encode(text.as_bytes())),
        (Encoding::Base64, Err(error)) => (ContentEncoding::Base64, data_encoding::BASE64.encode(error.as_bytes())),
    };
    let end = offset + read_length;
    let next_offset = (end < size).then_some(end);

    Ok(ReadAnswer { build, path, size, offset, length: read_length, encoding, content, next_offset })
}
