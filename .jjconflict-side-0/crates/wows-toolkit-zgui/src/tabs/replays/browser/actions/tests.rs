use std::cell::Cell;
use std::collections::BTreeSet;
use std::time::Duration;

use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::SourceKind;
use wows_toolkit_replay_data::force_reindex::version_said;
use wows_toolkit_replay_data::paths::shown_path;
use wows_toolkit_replay_data::replay_dir::DirChange;
use wows_toolkit_replay_data::replay_dir::ScannedReplay;
use wows_toolkit_replay_data::replay_dir::read_replay;
use wows_toolkit_replay_data::simulated_watch;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::replay_folders::FolderHistory;
use wows_toolkit_viewmodel::replay_folders::PinnedFolders;
use wowsunpack::game_types::ArenaId;
use zgui::vocab::Modifiers;

use super::*;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::settle_in_real_time;
use crate::services::GameDataRequest;
use crate::tabs::replays::browser::folders::MADE;

/// Recorded on three releases, oldest first.
const FIXTURES: [&str; 3] = [
    "10000_1736020571_20250101_215420_PVSB018-Ipiranga_41_Conquest.wowsreplay",
    "10000_1767829698_20260107_182737_PJSB018-Yamato-1944_50_Gold_harbor.wowsreplay",
    "10000_1773241614_20260214_215653_PVSB719-Valparaiso_42_Neighbors.wowsreplay",
];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wows-replay-insights/tests/fixtures/replays").join(name)
}

fn scanned(name: &str) -> ScannedReplay {
    read_replay(fixture(name)).expect("the fixture is a replay")
}

/// A directory holding copies of `names`, by its canonical path.
fn replay_dir(names: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    let root = std::fs::canonicalize(dir.path()).expect("the directory resolves");
    for name in names {
        std::fs::copy(fixture(name), root.join(name)).expect("the replay is copied");
    }

    (dir, root)
}

/// The shell, with the replays tab's browser, simulated directory watches, and no kept
/// directories.
fn open() -> (HeadlessWindow, BrowserControl) {
    simulated_watch::install();
    let window = open_shell(HeadlessOptions::new(WindowSize { width: 1200.0, height: 800.0 }));
    let control = MADE.with(Cell::get).expect("the replays tab made its browser");
    control.adopt_pinned_folders(PinnedFolders::default());
    control.adopt_folder_history(FolderHistory::default());

    (window, control)
}

/// A game directory with a build directory for each fixture's build, so their game data counts
/// as on this machine, taken as the settings' own without being saved. Its replays directory
/// holds `replays`.
fn adopt_install(control: BrowserControl, replays: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    let wows = std::fs::canonicalize(dir.path()).expect("the directory resolves").join("wows");
    for name in FIXTURES {
        let build = scanned(name).listed.build.expect("the fixture names its build");
        std::fs::create_dir_all(wows.join("bin").join(build.to_string())).expect("the build directory is made");
    }
    let replays_dir = wows.join("replays");
    std::fs::create_dir_all(&replays_dir).expect("the replays directory is made");
    for name in replays {
        std::fs::copy(fixture(name), replays_dir.join(name)).expect("the replay is copied");
    }

    let services = control.shell().services();
    let (pool, runtime) = index(control);
    let mut settings = runtime.block_on(StartupSettings::load(&pool));
    settings.wows_dir = wows.display().to_string();
    services.settings.adopt_unsaved(settings);
    services.game_data.adopt(GameDataRequest {
        wows_dir: Some(wows),
        cache_dir: String::new(),
        locale: None,
        auto_dump: false,
    });

    dir
}

fn index(control: BrowserControl) -> (sqlx::SqlitePool, Arc<tokio::runtime::Runtime>) {
    let services = control.shell().services();
    let pool = services.settings.pool().expect("the settings database is open");
    let runtime = services.runtime().expect("the runtime is up");

    (pool, runtime)
}

/// Puts the shell's settings over an empty database of the test's own, so what the test counts in
/// the index is only what it wrote. The settings themselves are the shared database's, which
/// answers the first-run questions. Call before anything is indexed.
fn own_index(control: BrowserControl) -> (sqlx::SqlitePool, Arc<tokio::runtime::Runtime>) {
    let services = control.shell().services();
    let (shared, runtime) = index(control);
    let settings = runtime.block_on(StartupSettings::load(&shared));
    let pool = runtime.block_on(wows_toolkit_config::test_pool());
    services.settings.adopt(settings, pool.clone());

    (pool, runtime)
}

fn folder_id(control: BrowserControl, root: &Path) -> FolderId {
    control
        .folders
        .with_untracked(|folders| {
            folders.iter().find(|tab| tab.source == FolderSource::Chosen(root.to_path_buf())).map(|tab| tab.id)
        })
        .expect("the directory has a tab")
}

fn resolved_source(control: BrowserControl, id: FolderId) -> Option<SourceId> {
    control.work.sources.with_untracked(|sources| match sources.get(&id) {
        Some(FolderSourceState::Resolved { source, .. }) => Some(*source),
        Some(_) | None => None,
    })
}

/// Lets frames run, and the tokio runtime and the worker threads work, until `done` holds.
fn settle_until(window: &mut HeadlessWindow, what: &str, done: impl Fn(&HeadlessWindow) -> bool) {
    for _ in 0..200 {
        if done(window) {
            return;
        }
        window.advance(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(done(window), "{what}");
}

/// Waits for the directory's own source and for every directory pass to end.
fn settle_indexed(window: &mut HeadlessWindow, control: BrowserControl, id: FolderId) {
    settle_until(window, "the directory's own source is made", |_| resolved_source(control, id).is_some());
    settle_until(window, "the directory passes end", |_| control.work.passes.with_untracked(HashMap::is_empty));
}

fn click(window: &mut HeadlessWindow, id: &str) {
    let element = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
    window.click(element).expect("the element can be clicked");
    window.advance(Duration::from_millis(400));
}

fn label(window: &mut HeadlessWindow, id: &str) -> Option<String> {
    let element = window.by_test_id(id)?;
    window.a11y(element)?.label
}

/// Opening a directory gives it a source of its own, and Search on its tab asks the Search tab
/// for that source rather than the Live one.
#[test]
fn a_chosen_folder_gets_a_source_of_its_own_and_search_scopes_to_it() {
    let (mut window, control) = open();
    let (_dir, root) = replay_dir(&[]);

    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let id = folder_id(control, &root);
    settle_indexed(&mut window, control, id);

    let (pool, runtime) = index(control);
    let sources = runtime.block_on(query::list_sources(&pool)).expect("the sources read");
    let own = sources
        .iter()
        .find(|source| source.root_path.as_deref() == Some(root.as_path()))
        .expect("the directory has a source");
    assert_eq!(own.kind, SourceKind::ImportedDir);
    assert_eq!(resolved_source(control, id), Some(own.id), "the tab holds its source");
    assert_eq!(search_offer(control), Offer::Enabled);

    click(&mut window, "replay-listing-search");
    settle_in_real_time(&mut window);

    let shell = control.shell();
    assert_eq!(shell.selected_untracked(), Some(TabKey::App(AppTab::Search)));
    assert_eq!(
        crate::tabs::search::tests::built().full_query(),
        print_query(&seed::source_scoped(own.id)),
        "the search is over the directory's own source"
    );
}

/// The index button says why it is not offered: no game data to read the replays through, or
/// another pass running.
#[test]
fn the_index_button_says_why_it_is_not_offered() {
    let (mut window, control) = open();

    let without_install = t!("ui.replay.game_data_failed", error = t!("ui.messages.wows_dir_not_set")).into_owned();
    assert_eq!(index_offer(control), Offer::Disabled(without_install.clone()));
    window.advance(Duration::from_millis(100));
    let button = window.by_test_id("replay-listing-index").expect("the header shows the index button");
    let state = window.a11y(button).expect("the button is in the accessibility tree");
    assert!(state.disabled, "the button is disabled");
    assert_eq!(state.label, Some(without_install), "and says why");

    control.work.reindexing.set(true);
    assert_eq!(index_offer(control), Offer::Disabled(t!("ui.reindex.busy_index").into_owned()));
    control.work.reindexing.set(false);
}

/// A palette re-index covers the install's directory and the pinned ones, each replay once. A
/// directory opened once is left out: its replays may be anyone's.
#[test]
fn a_palette_reindex_covers_the_install_and_pinned_folders_once() {
    let (mut window, control) = open();
    let _install = adopt_install(control, &[FIXTURES[2]]);
    control.install_listing().expect("the install has a listing").relist();
    let (_pinned_dir, pinned) = replay_dir(&FIXTURES[..2]);
    let (_opened_dir, opened) = replay_dir(&FIXTURES[..1]);
    control.open_folder(pinned.clone());
    settle_in_real_time(&mut window);
    control.pin_folder(folder_id(control, &pinned), true);
    control.open_folder(opened.clone());
    settle_until(&mut window, "every directory is listed", |_| {
        control.all_listings().iter().map(|listing| listing.files.with_untracked(Vec::len)).sum::<usize>() == 4
    });

    let covered: Vec<PathBuf> = kept_candidates(control).into_iter().map(|candidate| candidate.path).collect();

    let install_dir =
        control.install_listing().and_then(|listing| listing.directory.get_untracked()).expect("the install is listed");
    let expected: BTreeSet<PathBuf> =
        [pinned.join(FIXTURES[0]), pinned.join(FIXTURES[1]), install_dir.join(FIXTURES[2])].into_iter().collect();
    assert_eq!(covered.len(), expected.len(), "each replay once, covered {covered:?}");
    assert_eq!(covered.into_iter().collect::<BTreeSet<_>>(), expected);
}

/// The palette lists the kept replays' game versions newest first, each with its replay count,
/// and choosing one asks first with the count and the dates.
#[test]
fn the_palette_offers_versions_newest_first_and_asks_before_reindexing() {
    let (mut window, control) = open();
    let _install = adopt_install(control, &[]);
    let (_dir, root) = replay_dir(&FIXTURES);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let id = folder_id(control, &root);
    control.pin_folder(id, true);
    settle_indexed(&mut window, control, id);

    window.press_character('k', Modifiers::PRIMARY);
    window.advance(Duration::from_millis(400));
    // Typed rather than clicked: the entry sits below the root list's visible rows.
    window.type_text(&t!("ui.reindex.palette_by_version"));
    window.advance(Duration::from_millis(400));
    window.press_key(zgui::vocab::NamedKey::Enter, Modifiers::NONE);
    window.advance(Duration::from_millis(400));
    assert!(window.by_test_id("palette-mode-reindex-versions").is_some(), "the versions list is shown");

    let mut versions: Vec<_> =
        FIXTURES.iter().map(|name| scanned(name).version.expect("the fixture names its version")).collect();
    versions.sort();
    versions.reverse();
    for (row, version) in versions.iter().enumerate() {
        let expected = t!("ui.reindex.version_row", version = version_said(version), count = 1).into_owned();
        assert_eq!(label(&mut window, &format!("palette-row-{row}")), Some(expected));
    }
    assert!(window.by_test_id("palette-row-3").is_none(), "one row per version");

    window.press_key(zgui::vocab::NamedKey::Enter, Modifiers::NONE);
    settle_until(&mut window, "the re-index asks first", |window| window.by_test_id("force-reindex-summary").is_some());

    let expected = [
        t!("ui.reindex.confirm_count", count = 1).into_owned(),
        t!("ui.reindex.confirm_range", oldest = "2026-02-14 21:56", newest = "2026-02-14 21:56").into_owned(),
    ]
    .join("\n");
    assert_eq!(
        label(&mut window, "force-reindex-summary"),
        Some(expected),
        "a palette command always asks, with the count and dates"
    );
}

/// Indexing a directory's list asks first, saying how many replays its source lacks and where
/// they land, then reads exactly those into the directory's own source.
///
/// The game directory holds no game data, so every read fails; the count of failures at the end
/// is what shows how many were read.
#[test]
fn indexing_a_folder_asks_first_and_reads_only_what_its_source_lacks() {
    let (mut window, control) = open();
    let _install = adopt_install(control, &[]);
    let (_dir, root) = replay_dir(&FIXTURES);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let id = folder_id(control, &root);
    settle_indexed(&mut window, control, id);
    let own = resolved_source(control, id).expect("the directory has its own source");

    let (pool, runtime) = index(control);
    runtime.block_on(async {
        let arena_id = ArenaId::new(4_100_200_300);
        let indexed = ObjectiveMatch {
            arena_id,
            timestamp: jiff::Timestamp::now(),
            map: "Ocean".into(),
            game_mode: "Domination".into(),
            game_mode_id: None,
            game_type: "pvp".into(),
            match_group: "pvp".into(),
            version_build: None,
        };
        query::upsert_match(&pool, &indexed).await.expect("the match inserts");
        let record = ReplayRecord {
            arena_id,
            source_id: own,
            replay_path: root.join(FIXTURES[0]),
            file_mtime: None,
            outcome: MatchOutcome::Win,
            self_account_id: None,
            self_ship_id: None,
            self_survived: None,
            self_damage: None,
            self_kills: None,
            self_pr: None,
            results_available: true,
            indexed_at: jiff::Timestamp::now(),
        };
        query::upsert_record(&pool, &record).await.expect("the record inserts");
    });

    index_selected_folder(control);
    settle_until(&mut window, "indexing asks first", |window| window.by_test_id("force-reindex-summary").is_some());

    let expected = [
        t!("ui.index_folder.confirm_count", count = 2).into_owned(),
        t!("ui.reindex.confirm_range", oldest = "2026-01-07 18:27", newest = "2026-02-14 21:56").into_owned(),
        t!("ui.index_folder.note_folder", dir = shown_path(&root)).into_owned(),
    ]
    .join("\n");
    assert_eq!(label(&mut window, "force-reindex-summary"), Some(expected));

    click(&mut window, "force-reindex-ok");
    settle_until(&mut window, "the pass ends", |_| !control.work.is_busy());
    window.advance(Duration::from_millis(400));

    let said = t!("ui.index_folder.done", indexed = 0, failed = 2, skipped = 0).into_owned();
    assert!(window.by_label(&said).is_some(), "the end of the pass is said with its counts: {said}");
}

/// A directory the Live source is rooted at, such as an earlier install's replays directory, is
/// neither written into nor searched as a chosen directory: the Live source would take its
/// replays.
#[test]
fn a_folder_at_the_live_root_is_kept_out_of_the_live_source() {
    let (mut window, control) = open();
    let (pool, runtime) = own_index(control);
    let (_dir, root) = replay_dir(&[]);
    runtime
        .block_on(query::ensure_default_source(&pool, &root, jiff::Timestamp::now()))
        .expect("the live source is made");

    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let id = folder_id(control, &root);
    settle_until(&mut window, "the source lookup ends", |_| {
        control
            .work
            .sources
            .with_untracked(|sources| matches!(sources.get(&id), Some(FolderSourceState::Unavailable(_))))
    });

    let refused = control.work.sources.with_untracked(|sources| match sources.get(&id) {
        Some(FolderSourceState::Unavailable(err)) => {
            matches!(**err, SourceUnavailable::Scope(ScopeSourceError::LiveRoot { .. }))
        }
        Some(_) | None => false,
    });
    assert!(refused, "the directory has no source of its own");
    assert!(matches!(search_offer(control), Offer::Disabled(_)), "and offers no search");
    let found = runtime.block_on(SourceScope::Folder(root).existing(&pool)).expect("the sources read");
    assert_eq!(found, None, "nor does a search over it find the Live one");
}

/// A battle landing in a pinned directory is written under that directory's source; one landing
/// in the install's is what the Live source is made for.
#[test]
fn a_replay_landing_in_a_pinned_folder_is_indexed_under_its_own_source() {
    let (mut window, control) = open();
    let (pool, runtime) = own_index(control);
    let install = adopt_install(control, &[]);
    let (_pinned_dir, pinned) = replay_dir(&[]);
    control.adopt_pinned_folders(PinnedFolders(vec![pinned.clone()]));
    settle_in_real_time(&mut window);
    let id = folder_id(control, &pinned);
    settle_indexed(&mut window, control, id);
    let live = || runtime.block_on(query::live_source_id(&pool)).expect("the live source reads");
    assert_eq!(live(), None, "nothing has made the Live source yet");

    let landed = pinned.join("landed.wowsreplay");
    std::fs::copy(fixture(FIXTURES[0]), &landed).expect("the replay is copied");
    simulated_watch::report(&pinned, DirChange::Appeared(landed));
    settle_in_real_time(&mut window);

    let sources = runtime.block_on(query::list_sources(&pool)).expect("the sources read");
    let own = sources.iter().find(|source| source.root_path.as_deref() == Some(pinned.as_path()));
    assert_eq!(own.map(|source| source.kind), Some(SourceKind::ImportedDir), "the directory has its own source");
    assert_eq!(live(), None, "the battle did not go to the Live source");

    let main_dir = std::fs::canonicalize(install.path()).expect("the directory resolves").join("wows").join("replays");
    let played = main_dir.join("played.wowsreplay");
    std::fs::copy(fixture(FIXTURES[0]), &played).expect("the replay is copied");
    simulated_watch::report(&main_dir, DirChange::Appeared(played));
    settle_until(&mut window, "the install's battle is indexed", |_| live().is_some());
}
