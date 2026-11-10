//! Headless behavior of the Resource Unpacker tab over a fixture install: a
//! `bin/<build>/idx` index and a `res_packages` volume written here, read
//! through the same loader a real install goes through.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;

use nsh_core::Points;
use nsh_core::Size;

/// The fixture's one build.
const BUILD: &str = "1234567";

/// How long a view is given to go away.
const GONE: std::time::Duration = std::time::Duration::from_secs(5);

/// The volume every fixture file lives in.
const VOLUME: &str = "fixture.pkg";

/// `.idx` files start with this magic, then the format version.
const IDX_MAGIC: u32 = 0x5046_5349;
const IDX_VERSION_V40: u32 = 0x0200_0000;

/// The parent id of a top-level resource.
const ROOT_PARENT_ID: u64 = 0xdbb1_a1d1_b108_b927;

/// One resource of the fixture index: a directory, or a file with its
/// bytes.
struct Resource {
    id: u64,
    parent: u64,
    name: &'static str,
    body: Option<Vec<u8>>,
}

/// A WoWs install laid out as `list_available_builds` and
/// `build_game_vfs_for_build` expect, removed on drop.
struct FixtureInstall {
    root: tempfile::TempDir,
}

impl FixtureInstall {
    fn new(resources: &[Resource]) -> Self {
        let root = tempfile::tempdir().expect("a fixture install directory is created");
        let idx_dir = root.path().join("bin").join(BUILD).join("idx");
        let packages = root.path().join("res_packages");
        std::fs::create_dir_all(&idx_dir).expect("the idx directory is created");
        std::fs::create_dir_all(&packages).expect("the packages directory is created");

        let mut volume = Vec::new();
        let mut file_infos = Vec::new();
        for resource in resources {
            if let Some(body) = &resource.body {
                file_infos.push((resource.id, volume.len() as u64, body.len() as u32));
                volume.extend_from_slice(body);
            }
        }
        std::fs::write(packages.join(VOLUME), &volume).expect("the fixture volume is written");
        std::fs::write(idx_dir.join("fixture.idx"), idx_v40(resources, &file_infos))
            .expect("the fixture idx is written");

        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }
}

/// Serializes the v0x40 index layout `wowsunpack::data::idx` parses: a
/// header, the table counts and pointers (relative to offset 16), the
/// resource, file info and volume tables, then their strings, each pointed
/// at relative to its own entry.
fn idx_v40(resources: &[Resource], file_infos: &[(u64, u64, u32)]) -> Vec<u8> {
    const META_OFFSET: usize = 16;
    const META_SIZE: usize = 40;
    const RESOURCE_SIZE: usize = 32;
    const FILE_INFO_SIZE: usize = 48;
    const VOLUME_SIZE: usize = 24;

    let resources_at = META_OFFSET + META_SIZE;
    let file_infos_at = resources_at + resources.len() * RESOURCE_SIZE;
    let volumes_at = file_infos_at + file_infos.len() * FILE_INFO_SIZE;
    let strings_at = volumes_at + VOLUME_SIZE;

    let mut strings = Vec::new();
    let mut string_offsets = Vec::new();
    for name in resources.iter().map(|resource| resource.name).chain(std::iter::once(VOLUME)) {
        string_offsets.push(strings_at + strings.len());
        strings.extend_from_slice(name.as_bytes());
        strings.push(0);
    }

    let mut out = Vec::new();
    for word in [IDX_MAGIC, IDX_VERSION_V40, 0, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    for count in [resources.len() as u32, file_infos.len() as u32, 1, 0] {
        out.extend_from_slice(&count.to_le_bytes());
    }
    for pointer in [resources_at, file_infos_at, volumes_at] {
        out.extend_from_slice(&((pointer - META_OFFSET) as u64).to_le_bytes());
    }

    for (at, resource) in resources.iter().enumerate() {
        let entry = resources_at + at * RESOURCE_SIZE;
        for value in [0, (string_offsets[at] - entry) as u64, resource.id, resource.parent] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    for (resource_id, offset, size) in file_infos {
        for value in [*resource_id, 1, *offset, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for value in [*size, 0, *size, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    let volume_name_at = string_offsets[resources.len()];
    for value in [VOLUME.len() as u64, (volume_name_at - volumes_at) as u64, 1] {
        out.extend_from_slice(&value.to_le_bytes());
    }

    out.extend_from_slice(&strings);
    out
}

/// `GameParams.data` holding `{"": {"a": 1}}`: a protocol 2 pickle in a
/// zlib stream of one stored block, its bytes reversed as the game ships
/// them.
fn game_params_data() -> Vec<u8> {
    let pickle: &[u8] = b"\x80\x02}(X\x00\x00\x00\x00}(X\x01\x00\x00\x00aK\x01uu.";

    let mut zlib = vec![0x78, 0x01, 0x01];
    let len = pickle.len() as u16;
    zlib.extend_from_slice(&len.to_le_bytes());
    zlib.extend_from_slice(&(!len).to_le_bytes());
    zlib.extend_from_slice(pickle);
    let (mut a, mut b) = (1u32, 0u32);
    for byte in pickle {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    zlib.reverse();
    zlib
}

/// A 2x2 opaque red PNG.
fn png_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("the PNG header is written");
        writer.write_image_data(&[255, 0, 0, 255].repeat(4)).expect("the PNG pixels are written");
    }
    bytes
}

/// /content/a.xml, /content/GameParams.data, /gui/logo.png,
/// /gui/notes.txt, /scripts (empty).
fn fixture_resources() -> Vec<Resource> {
    vec![
        Resource { id: 1, parent: ROOT_PARENT_ID, name: "content", body: None },
        Resource { id: 2, parent: 1, name: "a.xml", body: Some(b"<root>needle in content</root>".to_vec()) },
        Resource { id: 3, parent: 1, name: "GameParams.data", body: Some(game_params_data()) },
        Resource { id: 4, parent: ROOT_PARENT_ID, name: "gui", body: None },
        Resource { id: 5, parent: 4, name: "logo.png", body: Some(png_bytes()) },
        Resource { id: 6, parent: 4, name: "notes.txt", body: Some(b"plain notes".to_vec()) },
        Resource { id: 7, parent: ROOT_PARENT_ID, name: "scripts", body: None },
    ]
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

/// The settings directory of this process: the override takes effect once
/// per process, so the tests share it and take turns.
static STORAGE: OnceLock<tempfile::TempDir> = OnceLock::new();

static TURN: Mutex<()> = Mutex::new(());

/// Points the shared settings at `wows_dir` and `output_dir`, holding the
/// settings until the returned guard drops.
fn configure_settings(wows_dir: Option<&Path>, output_dir: Option<&Path>) -> MutexGuard<'static, ()> {
    let turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let storage = STORAGE.get_or_init(|| tempfile::tempdir().expect("a settings directory is created"));
    wows_toolkit_config::storage_override::set(storage.path().to_owned());
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime starts");
    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
        let wows_dir = wows_dir.map(|dir| dir.display().to_string()).unwrap_or_default();
        let output_dir = output_dir.map(|dir| dir.display().to_string()).unwrap_or_default();
        wows_toolkit_config::queries::set_setting(&pool, "wows_dir", &wows_dir).await.expect("wows_dir is saved");
        wows_toolkit_config::queries::set_setting(&pool, "output_dir", &output_dir).await.expect("output_dir is saved");
        // The first-run questions and the startup update check are the
        // shell's tests' to exercise; here they would stand over the tab.
        for key in ["build_consent_window_shown", "replay_consent_prompt_shown", "language_selection_shown"] {
            wows_toolkit_config::queries::set_setting_raw(&pool, key, "true").await.expect("a question is answered");
        }
        wows_toolkit_config::queries::set_setting_raw(&pool, "check_for_updates", "false")
            .await
            .expect("the startup update check is off");
    });

    turn
}

fn start() -> nsh_testing::Headless {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    app
}

/// Every interaction runs in one app: the settings override and the
/// engine are per process, and the steps build on each other.
#[test]
fn the_unpacker_browses_filters_queues_extracts_searches_views_and_exports() {
    let install = FixtureInstall::new(&fixture_resources());
    let output = tempfile::tempdir().expect("an output directory is created");
    let _turn = configure_settings(Some(install.path()), Some(output.path()));

    let mut app = start();
    app.press("role:tab[Resource Unpacker]").unwrap();
    app.wait_for("Extraction Queue").unwrap();

    // The package pane lists the root; assets.bin has no archive to open.
    app.wait_for("label:content").unwrap();
    app.wait_for("label:gui").unwrap();
    app.wait_for("3 items").unwrap();
    app.press("Assets.bin").unwrap();
    app.wait_for("text:/^Failed to load: content\\/assets.bin could not be read/").unwrap();
    app.press("Packages").unwrap();

    // The extraction directory comes from the shared settings.
    let output_text = output.path().display().to_string();
    assert_eq!(app.screen().input_text("label:Extract to"), Ok(Some(output_text.as_str())));

    // A single build shows no version selector.
    assert_eq!(app.screen().count("label:Version:"), Ok(0), "one build offers no choice");

    // Tree navigation lists the selected folder; the breadcrumbs follow.
    app.press("role:treeitem[gui]").unwrap();
    app.wait_for("label:logo.png").unwrap();
    app.wait_for("2 items").unwrap();

    // Each folder row shows one icon: open while expanded, closed otherwise.
    let screen = app.screen();
    let rows = screen.count("role:treeitem").unwrap();
    let closed = screen.count("id:unpacker-folder").unwrap();
    let open = screen.count("id:unpacker-folder-open").unwrap();
    assert_eq!(closed + open, rows, "one folder icon per row: {closed} closed, {open} open, {rows} rows");
    assert!(
        open > 0 && closed > 0,
        "the expanded root is open and its collapsed folders closed: {closed} closed, {open} open"
    );

    // Queueing from the listing feeds the queue panel and the Extract label.
    app.wait_for("label:Queue this file: notes.txt").unwrap();
    app.press("label:Queue this file: notes.txt").unwrap();
    app.wait_for("1 queued").unwrap();
    app.wait_for("res/gui/notes.txt").unwrap();
    app.press("label:Remove from queue: res/gui/notes.txt").unwrap();
    app.wait_for("0 queued").unwrap();

    // A filter of three characters searches every file by path.
    app.type_text("label:Filter files...", ".xml").unwrap();
    app.wait_for(r"label:/^\/content\/a\.xml$/").unwrap();
    app.wait_for("1 items").unwrap();
    app.clear("label:Filter files...").unwrap();
    app.wait_for("label:logo.png").unwrap();

    // "Queue All Files" queues the listed files and leaves directories.
    app.press("label:Go to Directory: res").unwrap();
    app.wait_for("3 items").unwrap();
    app.press("label:Queue All Files (Packages)").unwrap();
    app.wait_for("0 queued").unwrap();
    app.press("role:treeitem[gui]").unwrap();
    app.wait_for("label:logo.png").unwrap();
    app.press("label:Queue All Files (Packages)").unwrap();
    app.wait_for("2 queued").unwrap();
    app.wait_for("Extract 2 Items").unwrap();

    // Extraction writes under the destination's res directory.
    app.press("Extract 2 Items").unwrap();
    app.wait_for("Extracted 2 files").unwrap();
    assert_eq!(std::fs::read(output.path().join("res/gui/notes.txt")).unwrap(), b"plain notes");
    assert_eq!(std::fs::read(output.path().join("res/gui/logo.png")).unwrap(), png_bytes());
    app.press("Clear All").unwrap();
    app.wait_for("0 queued").unwrap();

    // The folder tree collapses to a rail and comes back.
    app.press("label:Collapse Folder Tree").unwrap();
    app.wait_for_gone("label:Filter files...", GONE).unwrap();
    app.press("label:Expand Folder Tree").unwrap();
    app.wait_for("label:Filter files...").unwrap();

    // The queue dock collapses to a rail and comes back.
    app.press("label:Collapse Panel").unwrap();
    app.wait_for_gone("Clear All", GONE).unwrap();
    app.press("label:Expand Panel").unwrap();
    app.wait_for("Clear All").unwrap();

    // A content search opens its own results panel; a hit opens the file,
    // queues it, or reveals it in the tree.
    app.type_text("label:Search in files...", "needle").unwrap();
    app.press("label:Search Packages").unwrap();
    app.wait_for("Search: needle").unwrap();
    app.wait_for(r"label:/^\/content\/a\.xml$/").unwrap();
    app.wait_for("1 hits, 4 of 4 files").unwrap();

    app.press("label:Actions for /content/a.xml").unwrap();
    app.press("label:Queue").unwrap();
    app.wait_for("1 queued").unwrap();

    app.press("label:Actions for /content/a.xml").unwrap();
    app.press("label:Show in Package Tree").unwrap();
    app.wait_for("label:GameParams.data").unwrap();

    app.press("label:Actions for /content/a.xml").unwrap();
    app.press("label:View Contents").unwrap();
    app.wait_for("label:Close content/a.xml").unwrap();
    app.press("label:Close content/a.xml").unwrap();
    app.wait_for_gone("label:Close content/a.xml", GONE).unwrap();

    // The minimal export decodes the parameters the toolkit understands;
    // the fixture's tree holds none, so the file is an empty list or the
    // status says why it could not be decoded.
    let minimal = output.path().join("MinGameParams.json");
    app.on_pick_save_path(Some(minimal.clone())).unwrap();
    app.press("Export GameParams").unwrap();
    app.press("label:Whole tree Minimal JSON").unwrap();
    app.wait_for("text:/^(Parameters written|could not decode the parameters of build 1234567)$/").unwrap();
    if app.screen().count("Parameters written") == Ok(1) {
        let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&minimal).unwrap()).unwrap();
        assert!(written.is_array(), "the minimal export is the decoded parameter list");
    }

    // The GameParams export writes the decoded tree, whole or its base.
    let whole = output.path().join("GameParams.json");
    app.on_pick_save_path(Some(whole.clone())).unwrap();
    app.press("Export GameParams").unwrap();
    app.press("label:Whole tree JSON").unwrap();
    app.wait_for("Parameters written").unwrap();
    let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&whole).unwrap()).unwrap();
    assert_eq!(written, serde_json::json!({ "": { "a": 1 } }));

    let base = output.path().join("BaseParams.json");
    app.on_pick_save_path(Some(base.clone())).unwrap();
    app.press("Export GameParams").unwrap();
    app.press("label:Base parameters only JSON").unwrap();
    app.wait_for("Parameters written").unwrap();
    let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&base).unwrap()).unwrap();
    assert_eq!(written, serde_json::json!({ "a": 1 }));
}

#[test]
fn without_a_game_directory_the_panes_say_so_and_extraction_is_refused() {
    let _turn = configure_settings(None, None);

    let mut app = start();
    app.press("role:tab[Resource Unpacker]").unwrap();
    app.wait_for("Extraction Queue").unwrap();

    app.wait_for("No game data loaded. Set your World of Warships directory in Settings.").unwrap();
    assert_eq!(app.screen().count("label:Filter files..."), Ok(0), "the filter appears once a VFS is loaded");
    app.wait_for("Tick a file or folder in the listing to queue it").unwrap();
    assert_eq!(app.screen().enabled("Extract"), Ok(false), "nothing is queued and no destination is set");
    assert_eq!(app.screen().enabled("Clear All"), Ok(false));
    assert_eq!(app.screen().enabled("Export GameParams"), Ok(false), "no build is loaded");
    assert_eq!(app.screen().count("Cancel"), Ok(0), "cancel belongs to a run in progress");
}
