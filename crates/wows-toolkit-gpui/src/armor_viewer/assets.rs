//! Background load of the Armor Viewer's ship data: the shared
//! [`ShipAssets`] (assets.bin + camo DB, reused across every ship the
//! viewer opens), the [`ShipCatalog`] the sidebar/tree will list ships from,
//! and the nation-flag/class-icon [`IconCache`] that sidebar rows key their
//! icons from. Reuses the replay inspector's [`LoadedGameData`] (the same
//! preloaded VFS + `GameMetadataProvider` `spawn_startup_preload` already
//! warms) rather than a second game-data load: `ShipAssets::from_vfs_with_metadata`
//! takes an already-parsed `GameMetadataProvider`, so this never re-parses
//! `GameParams`.
//!
//! **Split for testability.** [`load_armor_assets`] is a plain, synchronous
//! function over `&VfsPath`/`&GameMetadataProvider`/`&SvgRenderer` -- no gpui
//! `App`/`Context` needed -- so it can run directly against a real game
//! install in a `#[test]`, mirroring `replay_inspector::load`'s
//! `parse_replay`/`spawn_parse` split. [`spawn_load_armor_assets`] is the
//! thin gpui wrapper that extracts the VFS/provider/`SvgRenderer` and runs
//! it on `cx.background_spawn`, matching `replay_inspector::table::PlayerTable::new`'s
//! icon-resolution pattern (the `SvgRenderer` gpui hands back from
//! `cx.svg_renderer()` is `Send` and safe to rasterize with off the UI
//! thread, so the whole load -- VFS reads, GameParams grouping, PNG/SVG
//! decode -- runs in one background task with no UI-thread SVG step).

use std::collections::HashSet;
use std::sync::Arc;

use gpui::App;
use gpui::AppContext;
use gpui::SvgRenderer;
use gpui::Task;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::replay_inspector::icons::IconCache;
use crate::replay_inspector::icons::decode_ship_class_svg;
use crate::replay_inspector::load::LoadedGameData;

use super::catalog::ShipCatalog;

/// Class-icon tint: the replay table tints ship-class icons by team/division
/// color (`replay_inspector::columns::player_color_kind_rgb`), but the armor
/// sidebar has no per-row team context, so this renders the icon undyed
/// (`0xffffff` leaves every channel at full intensity in `decode_ship_class_svg`'s
/// tint multiply -- see `icons.rs`), matching the icon's own white source art.
const CLASS_ICON_TINT: u32 = 0xffffff;

/// Reasons the Armor Viewer's ship data failed to load. Wraps `ShipAssets`'
/// `rootcause::Report` errors (the same convention `ReplayLoadError` in
/// `replay_inspector::load` uses for its own `anyhow`/`Report`-returning
/// dependencies) rather than surfacing the raw report type across the
/// background-task boundary.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ArmorAssetsError {
    #[error("failed to load ship assets: {0}")]
    ShipAssets(String),
}

/// One loaded build's Armor Viewer data: the shared ship-export assets, the
/// pre-built ship catalog for the sidebar/tree, and the nation-flag/class-icon
/// cache the sidebar rows will key their icons from. `icons` degrades
/// gracefully per-asset (a missing or undecodable flag/class icon is simply
/// absent from the cache, never a load failure) -- see [`load_catalog_icons`].
pub struct ArmorAssetsBundle {
    pub assets: ShipAssets,
    pub catalog: ShipCatalog,
    pub icons: IconCache,
}

/// Loads `vfs`/`metadata`'s [`ShipAssets`], builds the [`ShipCatalog`] from
/// the same provider, and resolves every nation flag and present ship-class
/// icon the catalog references into `icons`. Synchronous and CPU/IO-bound;
/// callers run it off the UI thread (see [`spawn_load_armor_assets`]).
fn load_armor_assets(
    vfs: &VfsPath,
    metadata: &Arc<GameMetadataProvider>,
    svg_renderer: &SvgRenderer,
) -> Result<ArmorAssetsBundle, ArmorAssetsError> {
    let assets = ShipAssets::from_vfs_with_metadata(vfs, Arc::clone(metadata))
        .map_err(|report| ArmorAssetsError::ShipAssets(format!("{report:?}")))?;

    let catalog = ShipCatalog::build(metadata);

    let mut icons = IconCache::new();
    load_catalog_icons(&catalog, vfs, svg_renderer, &mut icons);

    Ok(ArmorAssetsBundle { assets, catalog, icons })
}

/// Resolves one nation-flag icon per distinct nation in `catalog` (keyed
/// `"nation:{nation}"`, matching `IconCache`'s documented key convention)
/// and one ship-class icon per distinct `Species` the catalog's classes
/// carry (keyed by `(species, CLASS_ICON_TINT)`), from `vfs`, caching each
/// into `icons`. An asset absent from this build, or one that fails to
/// decode, is silently skipped -- `IconCache::get`/`get_keyed` returning
/// `None` for it is the sidebar's future signal (Task 4) to fall back to a
/// text label, same as every other icon kind in `replay_inspector::icons`.
fn load_catalog_icons(catalog: &ShipCatalog, vfs: &VfsPath, svg_renderer: &SvgRenderer, icons: &mut IconCache) {
    let mut species_seen: HashSet<Species> = HashSet::new();

    for nation in &catalog.nations {
        if let Some(bytes) = GuiAsset::NationFlag(&nation.nation).read(vfs, None) {
            icons.set_keyed(format!("nation:{}", nation.nation), &bytes);
        }

        for class in &nation.classes {
            if !species_seen.insert(class.species) {
                continue;
            }
            let Some(bytes) =
                GuiAsset::ShipClassIcon { species: class.species, state: ShipIconState::Alive }.read(vfs, None)
            else {
                continue;
            };
            if let Ok(image) = decode_ship_class_svg(svg_renderer, &bytes, CLASS_ICON_TINT) {
                icons.set_image(class.species, CLASS_ICON_TINT, image);
            }
        }
    }
}

/// Kicks off the Armor Viewer's ship-data load on the background executor.
/// `loaded` is expected to come from the same [`GameDataCache`](super::super::replay_inspector::GameDataCache)
/// the replay inspector preloads at startup, so this never triggers a
/// second VFS/`GameParams` load for the same build.
pub fn spawn_load_armor_assets(
    loaded: Arc<LoadedGameData>,
    cx: &App,
) -> Task<Result<ArmorAssetsBundle, ArmorAssetsError>> {
    let svg_renderer = cx.svg_renderer();
    cx.background_spawn(async move { load_armor_assets(loaded.vfs(), loaded.provider(), &svg_renderer) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a local game install: `ShipAssets::from_vfs_with_metadata` reads
    /// `content/assets.bin` off the VFS and `load_catalog_icons` resolves real
    /// nation-flag/class-icon bytes, neither of which can be cheaply
    /// fabricated. Exercises the exact synchronous path
    /// `spawn_load_armor_assets` wraps, against a real install's latest
    /// build (mirrors `replay_inspector::load`'s real-install test, which
    /// resolves a build the same way via `list_available_builds`). Run with:
    ///
    /// ```text
    /// WOWS_ARMOR_VIEWER_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// cargo test -p wows-toolkit-gpui -- --ignored load_armor_assets_against_a_real_install_loads_assets_catalog_and_icons
    /// ```
    #[test]
    #[ignore = "needs a local game install; see the doc comment for the run command"]
    fn load_armor_assets_against_a_real_install_loads_assets_catalog_and_icons() {
        let wows_dir = std::env::var("WOWS_ARMOR_VIEWER_LOAD_TEST_DIR")
            .expect("set WOWS_ARMOR_VIEWER_LOAD_TEST_DIR to a WoWs install directory");
        let wows_dir = std::path::PathBuf::from(wows_dir);

        let available =
            wowsunpack::game_data::list_available_builds(&wows_dir).expect("failed to list installed builds");
        let build = *available.last().expect("expected at least one installed build");

        let vfs = wowsunpack::game_data::build_game_vfs_for_build(&wows_dir, build)
            .expect("failed to build the game VFS for the latest installed build");
        let metadata =
            Arc::new(GameMetadataProvider::from_vfs(&vfs).expect("failed to build GameMetadataProvider from the VFS"));
        let svg_renderer = SvgRenderer::new(Arc::new(()));

        let bundle = load_armor_assets(&vfs, &metadata, &svg_renderer).expect("failed to load armor assets");

        let total_ships: usize = bundle.catalog.nations.iter().flat_map(|n| &n.classes).map(|c| c.ships.len()).sum();
        let total_classes: usize = bundle.catalog.nations.iter().map(|n| n.classes.len()).sum();
        assert!(!bundle.catalog.nations.is_empty(), "expected at least one nation");
        assert!(total_ships > 0, "expected at least one ship");

        tracing::info!(
            build,
            nations = bundle.catalog.nations.len(),
            classes = total_classes,
            ships = total_ships,
            ship_class_icons = bundle.icons.ship_class_count(),
            nation_flag_icons = bundle.icons.keyed_count(),
            "armor catalog: {} nations / {} ships loaded",
            bundle.catalog.nations.len(),
            total_ships
        );
        // `println!` alongside the `tracing::info!` above: the test harness
        // has no tracing subscriber installed, so the log line above is a
        // silent no-op under `cargo test`; this is what actually surfaces the
        // counts under `--nocapture` for manual verification against a real
        // install.
        println!(
            "armor catalog (real install, build {build}): {} nations / {} classes / {} ships / {} ship-class icons / {} nation-flag icons",
            bundle.catalog.nations.len(),
            total_classes,
            total_ships,
            bundle.icons.ship_class_count(),
            bundle.icons.keyed_count()
        );
    }
}
