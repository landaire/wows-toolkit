//! Manual frame-cost experiment for the replay listing at large N.
//!
//! Drives the real `build_file_listing_ungrouped` / `build_file_listing_grouped`
//! code through `egui_kittest` with rows scanned from a real replay corpus, and
//! times whole frames per mode (ungrouped / ship / date), tree state
//! (collapsed / expanded) and scale. A micro pass then times the stages the
//! grouped path repeats every frame, to attribute where the time goes.
//!
//! Run:
//!   cargo test -p wows-toolkit --release listing_frame_costs -- --ignored --nocapture
//!
//! Env:
//!   REPLAY_CACHE   replay corpus root       (default ~/dev/replay_cache)
//!   WOWS_BUILDS    extracted game data root (default ~/dev/wows-replay-data)
//!   EXP_BUILD_DIR  one extracted build dir  (default WOWS_BUILDS/0.11.10_6474624)
//!   EXP_MAX_SCAN   cap on scanned files

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use parking_lot::RwLock;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::vfs::VfsPath;

use super::GroupedListing;
use super::listing_row;
use super::listing_row::ListedReplay;
use crate::app::ToolkitTabViewer;
use crate::data::build_data::BuildAssets;
use crate::data::build_data::BuildData;
use crate::data::build_data::SharedBuildData;
use crate::data::constants::ConstantsFit;
use crate::db::index::rows::DivisionMate;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::RowSummary;
use crate::db::index::rows::WorkspaceId;
use crate::tab_state::TabState;
use crate::ui::replay_parser::workspace::workspace_group_salt;
use crate::ui::replay_parser::workspace::workspace_leaf_salt;

type Row = (PathBuf, Arc<ListedReplay>);

fn env_path(var: &str, default: &str) -> PathBuf {
    match std::env::var(var) {
        Ok(v) => PathBuf::from(v),
        Err(_) => {
            let home = std::env::var("HOME").expect("HOME is set");
            PathBuf::from(home).join(default)
        }
    }
}

fn scan_listed(root: &Path, cap: usize) -> Vec<Row> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wowsreplay") {
            continue;
        }
        let Ok(blob) = wows_replays::ReplayFile::read_meta_blob(path) else { continue };
        let Ok(meta) = wows_replays::ReplayMetaRef::from_slice(&blob) else { continue };
        out.push((path.to_path_buf(), Arc::new(ListedReplay::from_meta_ref(&meta))));
        if out.len() >= cap {
            break;
        }
    }
    out
}

fn build_provider(build_dir: &Path) -> (Arc<GameMetadataProvider>, VfsPath) {
    let dump = wows_data_mgr::Dump::open(build_dir);
    assert!(dump.has_game_files(), "no game files in {}", build_dir.display());
    let vfs = dump.vfs();
    let cached = dump.derived_path("game_params.rkyv").and_then(|p| wowsunpack::game_params::cache::load(&p));
    let provider = match cached {
        Some(params) => {
            GameMetadataProvider::from_params_with_vfs(params, &vfs).expect("cached params build a provider")
        }
        None => GameMetadataProvider::from_vfs(&vfs).expect("game params load from vfs"),
    };
    let mo = dump.derived_path("translations/en/LC_MESSAGES/global.mo");
    match mo
        .ok_or_else(|| "no derived translations path".to_string())
        .and_then(|p| std::fs::File::open(p).map_err(|e| e.to_string()))
        .and_then(|f| gettext::Catalog::parse(f).map_err(|e| e.to_string()))
    {
        Ok(catalog) => provider.set_translations(catalog),
        Err(e) => println!("warning: no translations ({e}); ship names degrade to spectator label"),
    }
    (Arc::new(provider), vfs)
}

fn shared_build_data(provider: Arc<GameMetadataProvider>, vfs: VfsPath) -> SharedBuildData {
    Arc::new(RwLock::new(Box::new(BuildData {
        assets: BuildAssets::default(),
        vfs,
        game_metadata: Some(provider),
        game_constants: Arc::new(wows_replays::game_constants::DEFAULT_GAME_CONSTANTS.clone()),
        replay_constants: Arc::new(RwLock::new(serde_json::Value::Null)),
        constants_fit: ConstantsFit::Mismatched,
        full_version: None,
        patch_version: 0,
        build_number: 0,
        replays_dir: PathBuf::new(),
        build_dir: PathBuf::new(),
        dump_dir: None,
    })))
}

/// Deterministic stand-in for an index row; every listed file gets one, like a
/// fully indexed directory.
fn synth_summary(i: usize) -> RowSummary {
    let outcome = match i % 5 {
        0 | 1 => MatchOutcome::Win,
        2 | 3 => MatchOutcome::Loss,
        _ => MatchOutcome::Draw,
    };
    let in_division = i.is_multiple_of(6);
    RowSummary {
        outcome,
        self_damage: Some(20_000 + (i as u64 * 7919) % 180_000),
        self_kills: Some((i % 4) as i64),
        self_survived: Some(i.is_multiple_of(3)),
        self_pr: Some(600.0 + (i % 1400) as f64),
        division_id: in_division.then_some(i as i64),
        division_mates: if in_division {
            vec![DivisionMate { player_name: format!("mate_{}", i % 97), clan: "CLAN".to_string() }]
        } else {
            Vec::new()
        },
        results_available: true,
        file_mtime: Some(1_700_000_000),
    }
}

/// Multiply the corpus by cloning rows under unique paths, keeping realistic
/// strings, dates and ship ids so grouping shapes stay true to the data.
fn replicate(rows: &[Row], factor: usize) -> Vec<Row> {
    let mut out = Vec::with_capacity(rows.len() * factor);
    for k in 0..factor {
        for (path, listed) in rows {
            let path = if k == 0 { path.clone() } else { path.with_extension(format!("{k}.wowsreplay")) };
            out.push((path, Arc::clone(listed)));
        }
    }
    out
}

#[derive(Clone, Copy)]
enum ListingMode {
    Ungrouped,
    Ship,
    Date,
}

impl ListingMode {
    fn label(self) -> &'static str {
        match self {
            ListingMode::Ungrouped => "ungrouped",
            ListingMode::Ship => "ship",
            ListingMode::Date => "date",
        }
    }
}

struct FrameStats {
    frames: usize,
    avg: Duration,
    min: Duration,
    max: Duration,
}

fn time_mode(rows: &[Row], shared: &SharedBuildData, mode: ListingMode, expanded: bool) -> FrameStats {
    let mut ts = TabState::default();
    ts.world_of_warships_data = Some(Arc::clone(shared));
    ts.live_workspace.set_replay_files(Some(rows.iter().cloned().collect()));
    ts.live_workspace
        .set_row_summaries(rows.iter().enumerate().map(|(i, (p, _))| (p.clone(), synth_summary(i))).collect());
    // Collapsed runs leave the flag unset so the listing's own large-directory
    // default collapses every group on the first frame. Expanded runs mark it
    // done, which leaves egui_ltreeview's native default: every dir open.
    ts.live_workspace.replay_listing_collapse_defaulted = expanded;

    let mut harness = egui_kittest::Harness::builder().with_size(egui::Vec2::new(1600.0, 1000.0)).build_ui(|ui| {
        let mut viewer = ToolkitTabViewer { tab_state: &mut ts };
        match mode {
            ListingMode::Ungrouped => viewer.build_file_listing_ungrouped(ui, WorkspaceId::LIVE),
            ListingMode::Ship => viewer.build_file_listing_grouped(ui, WorkspaceId::LIVE, GroupedListing::Ship),
            ListingMode::Date => viewer.build_file_listing_grouped(ui, WorkspaceId::LIVE, GroupedListing::Date),
        }
    });

    // First frames absorb one-time work (collapse default, font atlas).
    for _ in 0..2 {
        harness.step();
    }

    let budget = Duration::from_secs(8);
    let mut times = Vec::new();
    let started = Instant::now();
    while times.len() < 8 && (times.len() < 2 || started.elapsed() < budget) {
        let t = Instant::now();
        harness.step();
        times.push(t.elapsed());
    }
    let total: Duration = times.iter().sum();
    FrameStats {
        frames: times.len(),
        avg: total / times.len() as u32,
        min: *times.iter().min().expect("at least two frames measured"),
        max: *times.iter().max().expect("at least two frames measured"),
    }
}

/// Times the work the grouped listing repeats every frame before any node is
/// drawn, mirroring `build_file_listing_grouped` stage by stage.
fn micro_breakdown(rows: &[Row], provider: &GameMetadataProvider) {
    let n = rows.len();
    let map: HashMap<PathBuf, Arc<ListedReplay>> = rows.iter().cloned().collect();
    let summaries: HashMap<PathBuf, RowSummary> =
        rows.iter().enumerate().map(|(i, (p, _))| (p.clone(), synth_summary(i))).collect();

    let t = Instant::now();
    let mut files = map.iter().map(|(x, y)| (x.clone(), y.clone())).collect::<Vec<_>>();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let t_clone_sort = t.elapsed();

    let t = Instant::now();
    let mut ship_groups: HashMap<String, Vec<Row>> = HashMap::new();
    for (path, listed) in &files {
        let ship_name = listing_row::listed_ship_name(listed, provider);
        ship_groups.entry(ship_name).or_default().push((path.clone(), Arc::clone(listed)));
    }
    let t_group_build = t.elapsed();

    let ws = WorkspaceId::LIVE;
    let t = Instant::now();
    let mut leaf_paths: HashMap<egui::Id, PathBuf> = HashMap::new();
    let mut group_child_ids: HashMap<egui::Id, Vec<egui::Id>> = HashMap::new();
    let mut group_paths: HashMap<egui::Id, Vec<PathBuf>> = HashMap::new();
    for (group_name, replays) in &ship_groups {
        let group_id = workspace_group_salt(ws, "ship_group", group_name);
        let mut child_ids = Vec::new();
        let mut grp_paths = Vec::new();
        for (path, _) in replays {
            let id = workspace_leaf_salt(ws, path);
            leaf_paths.insert(id, path.clone());
            child_ids.push(id);
            grp_paths.push(path.clone());
        }
        group_child_ids.insert(group_id, child_ids);
        group_paths.insert(group_id, grp_paths);
    }
    let t_tree_maps = t.elapsed();

    let t = Instant::now();
    let fallback = (leaf_paths.clone(), group_child_ids.clone(), group_paths.clone());
    let t_fallback_clone = t.elapsed();
    drop(fallback);

    let t = Instant::now();
    let mut labels = 0usize;
    for replays in ship_groups.values() {
        let mut counts = super::listing_cache::OutcomeCounts::default();
        for (path, _) in replays {
            if let Some(summary) = summaries.get(path) {
                counts.add(summary.outcome);
            }
        }
        labels += counts.suffix().len();
    }
    let t_win_rate = t.elapsed();
    std::hint::black_box(labels);

    println!("micro n={n} groups={}", ship_groups.len());
    println!("  clone+sort listing vec   {t_clone_sort:>12.3?}");
    println!("  ship group build         {t_group_build:>12.3?}");
    println!("  tree id/path maps        {t_tree_maps:>12.3?}");
    println!("  fallback maps clone      {t_fallback_clone:>12.3?}");
    println!("  win-rate outcome sweep   {t_win_rate:>12.3?}");
}

#[test]
#[ignore = "manual perf experiment against a local replay corpus"]
fn listing_frame_costs() {
    let cache_root = env_path("REPLAY_CACHE", "dev/replay_cache");
    let builds_root = env_path("WOWS_BUILDS", "dev/wows-replay-data");
    let build_dir = match std::env::var("EXP_BUILD_DIR") {
        Ok(v) => PathBuf::from(v),
        Err(_) => builds_root.join("0.11.10_6474624"),
    };
    let cap = std::env::var("EXP_MAX_SCAN").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);

    let t = Instant::now();
    let rows = scan_listed(&cache_root, cap);
    assert!(!rows.is_empty(), "no replays under {}", cache_root.display());
    println!("scanned {} replay headers in {:.2?}", rows.len(), t.elapsed());

    let t = Instant::now();
    let (provider, vfs) = build_provider(&build_dir);
    println!("game data loaded from {} in {:.2?}", build_dir.display(), t.elapsed());

    let distinct_ships: std::collections::HashSet<String> =
        rows.iter().map(|(_, l)| listing_row::listed_ship_name(l, &provider)).collect();
    let distinct_dates: std::collections::HashSet<&str> =
        rows.iter().map(|(_, l)| l.date_time.split(' ').next().unwrap_or(&l.date_time)).collect();
    println!("{} distinct ship names, {} distinct dates", distinct_ships.len(), distinct_dates.len());

    let shared = shared_build_data(Arc::clone(&provider), vfs);

    let scales: Vec<(String, Vec<Row>)> = vec![
        ("2k".to_string(), rows.iter().take(2_000).cloned().collect()),
        (format!("{}k(all)", rows.len() / 1000), rows.clone()),
        (format!("{}k(x4)", rows.len() * 4 / 1000), replicate(&rows, 4)),
        (format!("{}k(x10)", rows.len() * 10 / 1000), replicate(&rows, 10)),
    ];

    println!(
        "{:<12} {:<10} {:<10} {:>6} {:>12} {:>12} {:>12}",
        "scale", "mode", "state", "frames", "avg", "min", "max"
    );
    for (scale_label, scaled) in &scales {
        for (mode, expanded) in [
            (ListingMode::Ungrouped, false),
            (ListingMode::Ship, false),
            (ListingMode::Ship, true),
            (ListingMode::Date, false),
            (ListingMode::Date, true),
        ] {
            let state = match (mode, expanded) {
                (ListingMode::Ungrouped, _) => "-",
                (_, false) => "collapsed",
                (_, true) => "expanded",
            };
            let stats = time_mode(scaled, &shared, mode, expanded);
            println!(
                "{:<12} {:<10} {:<10} {:>6} {:>12.3?} {:>12.3?} {:>12.3?}",
                scale_label,
                mode.label(),
                state,
                stats.frames,
                stats.avg,
                stats.min,
                stats.max
            );
        }
    }

    println!();
    micro_breakdown(&rows, &provider);
    if let Some((_, big)) = scales.last() {
        micro_breakdown(big, &provider);
    }
}
