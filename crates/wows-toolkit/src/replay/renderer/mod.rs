use rust_i18n::t;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use egui::Color32;
use egui::CornerRadius;
use egui::FontId;
use egui::Pos2;
use egui::Rect;
use egui::Stroke;
use egui::TextureHandle;
use egui::Vec2;
use parking_lot::Mutex;

use crate::LocalizedTextResolver;
use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::GameFonts;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::RenderProgress;
use wows_minimap_renderer::RenderStage;
use wows_minimap_renderer::STATS_PANEL_WIDTH;
use wows_minimap_renderer::TEAM_ROSTER_WIDTH;
use wows_minimap_renderer::assets;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::draw_command::ShipConfigVisibility;
use wows_minimap_renderer::map_data::MapInfo;
use wows_minimap_renderer::renderer::RenderOptions;

use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::Version;
use wowsunpack::vfs::VfsPath;

use egui_taffy::AsTuiBuilder as _;
use egui_taffy::TuiBuilderLogic as _;
use egui_taffy::taffy;
use egui_taffy::taffy::prelude::auto;
use egui_taffy::taffy::prelude::length;

use crate::collab::SessionStatus;
use crate::collab::peer::FrameBroadcast;
use crate::data::settings::SavedRenderOptions;
use crate::data::wows_data::SharedWoWsData;
use crate::icons;

use crate::util::controls::CommandGroup;
/// Approximate number of frame snapshots per second of game time.
/// Controls the granularity of seeking in the replay.
const SNAPSHOTS_PER_SECOND: f32 = 1.5;
const PLAYBACK_SPEEDS: [f32; 6] = [1.0, 5.0, 10.0, 20.0, 40.0, 60.0];
use crate::replay::minimap_view::Annotation;
use crate::replay::minimap_view::AnnotationState;
use crate::replay::minimap_view::ENEMY_COLOR;
use crate::replay::minimap_view::FRIENDLY_COLOR;
use crate::replay::minimap_view::MapTransform;
use crate::replay::minimap_view::OverlayState;
use crate::replay::minimap_view::PaintTool;
use crate::replay::minimap_view::ViewportZoomPan;
use crate::replay::minimap_view::collab_annotation_to_local;
use crate::replay::minimap_view::get_my_user_id;
use crate::replay::minimap_view::handle_map_click_ping;
use crate::replay::minimap_view::send_annotation_clear;
use crate::replay::minimap_view::send_annotation_full_sync;
use crate::replay::minimap_view::send_annotation_remove;
use crate::replay::minimap_view::send_annotation_update;
/// Extracted score bar state used for positioning the advantage label.
struct ScoreBarInfo {
    team0_score: i32,
    team1_score: i32,
    team0_timer: Option<String>,
    team1_timer: Option<String>,
    adv_team: i32,
}

/// RGBA image data with dimensions.
#[derive(PartialEq, Eq)]
pub struct RgbaAsset {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

type IconMap = HashMap<String, RgbaAsset>;

/// A GUI-asset set cached per game version. Asset paths are version-aware, so a
/// session that mixes replays from different versions must keep their assets
/// apart. Most versions resolve byte-identical assets, so on load we reuse an
/// existing version's `Arc` when the freshly-loaded set matches — keeping the
/// extra versions nearly free.
#[derive(Default)]
struct VersionedAssets {
    by_version: HashMap<Option<Version>, Arc<IconMap>>,
}

impl VersionedAssets {
    fn get_or_load(&mut self, version: Option<&Version>, load: impl FnOnce() -> IconMap) -> Arc<IconMap> {
        let key = version.copied();
        if let Some(cached) = self.by_version.get(&key) {
            return Arc::clone(cached);
        }
        let fresh = load();
        let arc = match self.by_version.values().find(|existing| ***existing == fresh) {
            Some(identical) => Arc::clone(identical),
            None => Arc::new(fresh),
        };
        self.by_version.insert(key, Arc::clone(&arc));
        arc
    }
}

/// Cached assets shared across renderer instances. Lives in TabState.
/// Icons are keyed by game version; map data by version and map name.
#[derive(Default)]
pub struct RendererAssetCache {
    ship_icons: VersionedAssets,
    plane_icons: VersionedAssets,
    building_icons: VersionedAssets,
    consumable_icons: VersionedAssets,
    ribbon_icons: VersionedAssets,
    subribbon_icons: VersionedAssets,
    death_cause_icons: VersionedAssets,
    powerup_icons: VersionedAssets,
    crew_skill_icons: VersionedAssets,
    modernization_icons: VersionedAssets,
    signal_flag_icons: VersionedAssets,
    game_fonts: HashMap<Option<Version>, GameFonts>,
    maps: HashMap<(Option<Version>, String), CachedMapData>,
    /// Resolved icon source per game-data version. Memoizing keeps the
    /// newest-dump fallback (and its log line) to once per version instead of
    /// once per icon type per frame.
    icon_sources: HashMap<Option<Version>, IconSource>,
}

struct CachedMapData {
    image: Option<Arc<RgbaAsset>>,
    info: Option<MapInfo>,
}

/// Convert a loader's `RgbaImage` map into the cache's `RgbaAsset` map.
fn convert_icons(raw: HashMap<String, image::RgbaImage>) -> IconMap {
    raw.into_iter()
        .map(|(k, img)| {
            let (w, h) = (img.width(), img.height());
            (k, RgbaAsset { data: img.into_raw(), width: w, height: h })
        })
        .collect()
}

/// Where a set of GUI icons is loaded from: a VFS plus the version those icons
/// belong to (also the cache key). Either the replay's own game data or, for old
/// builds that ship no per-file icons, the newest dump on disk.
#[derive(Clone)]
struct IconSource {
    vfs: VfsPath,
    version: Option<Version>,
}

/// Resolve where to load the renderer's GUI icons from.
///
/// Builds new enough to ship per-file icons use their own VFS. Older builds
/// (pre-12.0 clients shipped no `gui/fla/minimap/ship_icons`, so the minimap
/// falls back to plain circles) borrow every icon set from the newest dump on
/// disk, located via the dump cache base -- the parent of this build's own dump
/// dir. The map image and fonts always stay on the replay's own VFS.
fn resolve_icon_source(vfs: &VfsPath, version: Option<&Version>, dump_dir: Option<&std::path::Path>) -> IconSource {
    let own = || IconSource { vfs: vfs.clone(), version: version.copied() };

    // Cheap existence probe (no SVG decode): does this build ship class icons?
    let has_icons = wowsunpack::game_assets::GuiAsset::ShipClassIcon {
        species: wowsunpack::game_params::types::Species::Destroyer,
        state: wowsunpack::game_assets::ShipIconState::Alive,
    }
    .resolve(vfs, version)
    .is_some();
    if has_icons {
        return own();
    }

    match newest_dump_source(dump_dir) {
        Some((vfs, dump_version)) => {
            tracing::info!(
                own_build = version.map(|v| v.build),
                borrowed_build = dump_version.map(|v| v.build),
                "renderer build ships no GUI icons; borrowing icons from newest dump",
            );
            IconSource { vfs, version: dump_version }
        }
        None => own(),
    }
}

/// The VFS and version of the newest dump on disk, located via the dump cache
/// base (the parent of a build's own dump dir). Used to borrow GUI assets for
/// old builds that ship none. Returns None when there's no usable dump.
fn newest_dump_source(dump_dir: Option<&std::path::Path>) -> Option<(VfsPath, Option<Version>)> {
    let base = dump_dir.and_then(|d| d.parent())?;
    let index = wows_data_mgr::builds::BuildsIndex::load(&base.join("builds.toml"));
    let entry = index.builds.iter().max_by_key(|e| e.build)?;
    let vfs_root = base.join(&entry.dir).join("vfs");
    if !vfs_root.exists() {
        return None;
    }
    let vfs = VfsPath::new(wowsunpack::vfs::impls::physical::PhysicalFS::new(&vfs_root));
    let mut parts = entry.version.split('.').filter_map(|p| p.trim().parse::<u32>().ok());
    let version = parts.next().map(|major| Version {
        major,
        minor: parts.next().unwrap_or(0),
        patch: parts.next().unwrap_or(0),
        build: entry.build,
    });
    Some((vfs, version))
}

/// All dump build VFS roots on disk, newest build first. Lets an old replay that
/// ships no TTF borrow a font from the newest build that has one, trying older
/// builds in turn rather than giving up after the single newest.
fn dump_sources_newest_first(dump_dir: Option<&std::path::Path>) -> Vec<VfsPath> {
    let Some(base) = dump_dir.and_then(|d| d.parent()) else {
        return Vec::new();
    };
    let index = wows_data_mgr::builds::BuildsIndex::load(&base.join("builds.toml"));
    let mut entries: Vec<_> = index.builds.iter().collect();
    entries.sort_by(|a, b| b.build.cmp(&a.build));
    entries
        .into_iter()
        .filter_map(|e| {
            let vfs_root = base.join(&e.dir).join("vfs");
            vfs_root.exists().then(|| VfsPath::new(wowsunpack::vfs::impls::physical::PhysicalFS::new(&vfs_root)))
        })
        .collect()
}

impl RendererAssetCache {
    /// Resolve (and memoize) where GUI icons load from for this game data.
    /// Old builds that ship no icons transparently borrow them from the newest
    /// dump on disk; callers pass their own VFS/version/dump dir and never need
    /// to compute the fallback themselves.
    fn icon_source(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> IconSource {
        if let Some(src) = self.icon_sources.get(&version.copied()) {
            return src.clone();
        }
        let src = resolve_icon_source(vfs, version, dump_dir);
        self.icon_sources.insert(version.copied(), src.clone());
        src
    }

    pub fn get_or_load_ship_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.ship_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_ship_icons(&src.vfs, src.version.as_ref()))
        })
    }

    pub fn get_or_load_plane_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.plane_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_plane_icons(&src.vfs, src.version.as_ref()))
        })
    }

    pub fn get_or_load_building_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.building_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_building_icons(&src.vfs, src.version.as_ref()))
        })
    }

    pub fn get_or_load_consumable_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.consumable_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_consumable_icons(&src.vfs, src.version.as_ref()))
        })
    }

    pub fn get_or_load_ribbon_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.ribbon_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_ribbon_icons(
                &src.vfs,
                wowsunpack::game_assets::GuiAssetDir::Ribbons,
                src.version.as_ref(),
            ))
        })
    }

    pub fn get_or_load_subribbon_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.subribbon_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_ribbon_icons(
                &src.vfs,
                wowsunpack::game_assets::GuiAssetDir::SubRibbons,
                src.version.as_ref(),
            ))
        })
    }

    pub fn get_or_load_death_cause_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.death_cause_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_death_cause_icons(&src.vfs, 16, src.version.as_ref()))
        })
    }

    pub fn get_or_load_powerup_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.powerup_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_powerup_icons(&src.vfs, 16, src.version.as_ref()))
        })
    }

    pub fn get_or_load_crew_skill_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        _dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        // Crew skill icons are skill-set specific and ship in every build's own
        // VFS (pre-rework clients keep them under big/small subdirs). Never
        // borrow another build's set the way ship icons do -- a newer build's
        // skills and icon names would not match this replay's skills.
        self.crew_skill_icons.get_or_load(version, || convert_icons(assets::load_crew_skill_icons(vfs, 36, version)))
    }

    pub fn get_or_load_modernization_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.modernization_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_modernization_icons(&src.vfs, 36, src.version.as_ref()))
        })
    }

    pub fn get_or_load_signal_flag_icons(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> Arc<IconMap> {
        let src = self.icon_source(vfs, version, dump_dir);
        self.signal_flag_icons.get_or_load(src.version.as_ref(), || {
            convert_icons(assets::load_signal_flag_icons(&src.vfs, 36, src.version.as_ref()))
        })
    }

    pub fn get_or_load_game_fonts(
        &mut self,
        vfs: &VfsPath,
        version: Option<&Version>,
        dump_dir: Option<&std::path::Path>,
    ) -> GameFonts {
        if let Some(cached) = self.game_fonts.get(&version.copied()) {
            return cached.clone();
        }
        // Old clients ship bitmap-only fonts; borrow a TTF from dump builds
        // (newest first) before falling back to a system font.
        let fallbacks = dump_sources_newest_first(dump_dir);
        let fonts = assets::load_game_fonts_with_fallbacks(vfs, &fallbacks);
        self.game_fonts.insert(version.copied(), fonts.clone());
        fonts
    }

    pub fn get_or_load_map(
        &mut self,
        map_name: &str,
        vfs: &VfsPath,
        version: Option<&Version>,
    ) -> (Option<Arc<RgbaAsset>>, Option<MapInfo>) {
        let key = (version.copied(), map_name.to_string());
        if let Some(cached) = self.maps.get(&key) {
            return (cached.image.clone(), cached.info.clone());
        }
        let map_image = assets::load_map_image(map_name, vfs).map(|img| {
            let rgba = image::DynamicImage::ImageRgb8(img).into_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            Arc::new(RgbaAsset { data: rgba.into_raw(), width: w, height: h })
        });
        let map_info = assets::load_map_info(map_name, vfs);
        self.maps.insert(key, CachedMapData { image: map_image.clone(), info: map_info.clone() });
        (map_image, map_info)
    }
}
pub fn render_options_from_saved(saved: &SavedRenderOptions) -> RenderOptions {
    RenderOptions {
        show_hp_bars: saved.show_hp_bars,
        show_tracers: saved.show_tracers,
        show_torpedoes: saved.show_torpedoes,
        show_planes: saved.show_planes,
        show_smoke: saved.show_smoke,
        show_score: saved.show_score,
        show_timer: saved.show_timer,
        show_kill_feed: saved.show_kill_feed,
        show_player_names: saved.show_player_names,
        show_ship_names: saved.show_ship_names,
        show_capture_points: saved.show_capture_points,
        show_buildings: saved.show_buildings,
        show_weather: saved.show_buildings, // TODO: add show_weather to SavedRenderOptions
        show_camera_direction: saved.show_camera_direction,
        show_consumables: saved.show_consumables,
        show_armament: saved.show_armament,
        show_trails: saved.show_trails,
        show_dead_trails: saved.show_dead_trails,
        show_speed_trails: saved.show_speed_trails,
        show_ship_config: saved.show_ship_config,
        show_dead_ship_names: saved.show_dead_ship_names,
        show_battle_result: saved.show_battle_result,
        show_buffs: saved.show_buffs,
        show_chat: saved.show_chat,
        show_advantage: saved.show_advantage,
        show_score_timer: saved.show_score_timer,
        show_stats_panel: saved.show_stats_panel,
        show_team_rosters: saved.show_team_rosters,
        // UI does its own per-ship filtering in the draw loop, so emit all circles
        ship_config_visibility: ShipConfigVisibility::Filtered(Arc::new(|_| Some(ShipConfigFilter::all_enabled()))),
    }
}

/// Send the current per-ship trail hidden set through the collab channel (if connected).
fn broadcast_trail_overrides(trail_hidden: &HashSet<String>, shared_state: &Arc<Mutex<SharedRendererState>>) {
    let state = shared_state.lock();
    if let Some(ref tx) = state.collab_local_tx {
        let data: Vec<_> = trail_hidden.iter().cloned().collect();
        let _ = tx.send(crate::collab::peer::LocalEvent::TrailOverrides(data));
    }
}

/// Send the current per-ship range overrides through the collab channel (if connected).
fn broadcast_range_overrides(
    overrides: &HashMap<EntityId, ShipConfigFilter>,
    shared_state: &Arc<Mutex<SharedRendererState>>,
) {
    let state = shared_state.lock();
    if let Some(ref tx) = state.collab_local_tx {
        let data: Vec<_> = overrides.iter().map(|(k, v)| (*k, *v)).collect();
        let _ = tx.send(crate::collab::peer::LocalEvent::RangeOverrides(data));
    }
}

fn saved_from_render_options(opts: &RenderOptions) -> SavedRenderOptions {
    SavedRenderOptions {
        show_hp_bars: opts.show_hp_bars,
        show_tracers: opts.show_tracers,
        show_torpedoes: opts.show_torpedoes,
        show_planes: opts.show_planes,
        show_smoke: opts.show_smoke,
        show_score: opts.show_score,
        show_timer: opts.show_timer,
        show_kill_feed: opts.show_kill_feed,
        show_player_names: opts.show_player_names,
        show_ship_names: opts.show_ship_names,
        show_capture_points: opts.show_capture_points,
        show_buildings: opts.show_buildings,
        show_camera_direction: opts.show_camera_direction,
        show_consumables: opts.show_consumables,
        show_dead_ships: false,
        show_dead_ship_names: opts.show_dead_ship_names,
        show_armament: opts.show_armament,
        show_trails: opts.show_trails,
        show_dead_trails: opts.show_dead_trails,
        show_speed_trails: opts.show_speed_trails,
        show_battle_result: opts.show_battle_result,
        show_buffs: opts.show_buffs,
        show_ship_config: opts.show_ship_config,
        // Range filter flags are persisted from annotation state at the call site
        show_self_detection_range: false,
        show_self_main_battery_range: false,
        show_self_secondary_range: false,
        show_self_torpedo_range: false,
        show_self_radar_range: false,
        show_self_hydro_range: false,
        show_chat: opts.show_chat,
        show_advantage: opts.show_advantage,
        show_score_timer: opts.show_score_timer,
        show_stats_panel: opts.show_stats_panel,
        show_team_rosters: opts.show_team_rosters,
        prefer_cpu_encoder: false, // Not part of RenderOptions; set by caller
        video_codec: None,         // Same: caller persists the user's codec choice.
        include_pre_battle: false, // Same: caller persists the user's choice.
    }
}
/// Commands sent from the UI thread to the background playback thread.
pub enum PlaybackCommand {
    Play,
    Pause,
    Seek(GameClock),
    SetSpeed(f32),
    Stop,
}

/// A single frame's rendering data, shared from background to UI thread.
#[derive(Debug)]
pub struct PlaybackFrame {
    pub replay_id: u64,
    pub commands: Vec<DrawCommand>,
    pub clock: GameClock,
    pub frame_index: usize,
    pub total_frames: usize,
    pub game_duration: f32,
}
/// Probe the encoder status at most once per session and cache the result in
/// egui memory. `check_encoder()` enumerates Vulkan devices and is too
/// expensive to call every frame from the settings popover.
fn cached_encoder_status(ctx: &egui::Context) -> Arc<wows_minimap_renderer::encoder::EncoderStatus> {
    let id = egui::Id::new("wt::encoder_status");
    if let Some(cached) = ctx.data(|d| d.get_temp::<Arc<wows_minimap_renderer::encoder::EncoderStatus>>(id)) {
        return cached;
    }
    let status = Arc::new(wows_minimap_renderer::check_encoder());
    ctx.data_mut(|d| d.insert_temp(id, Arc::clone(&status)));
    status
}

/// Resolve the effective `prefer_cpu` flag for a video export.
///
/// The GUI rule: if the user explicitly picked a codec the GPU can't encode
/// (e.g. AV1, which gpu-video doesn't yet support), silently fall back to CPU
/// instead of erroring. With `CodecChoice::Auto` we honor the user's CPU
/// preference and otherwise let the encoder pick the best GPU codec.
pub fn resolve_prefer_cpu(
    user_prefers_cpu: bool,
    codec_choice: Option<wows_minimap_renderer::VideoCodec>,
    status: &wows_minimap_renderer::encoder::EncoderStatus,
) -> bool {
    if user_prefers_cpu {
        return true;
    }
    if !status.gpu_available() {
        return true;
    }
    match codec_choice {
        Some(codec) => !status.supports(wows_minimap_renderer::EncoderKind::Gpu, codec),
        None => false,
    }
}

/// Player info snapshot captured from BattleController for the armor viewer.
#[derive(Clone, Debug)]
pub struct ReplayPlayerInfo {
    pub entity_id: EntityId,
    pub username: String,
    pub team_id: i64,
    pub vehicle: Arc<wowsunpack::game_params::types::Param>,
    pub ship_display_name: String,
    /// Equipped hull GameParamId from the replay's ShipConfig.
    pub hull_param_id: Option<wowsunpack::game_types::GameParamId>,
}

/// Shared bridge between replay thread and realtime armor viewer windows.
pub struct RealtimeArmorBridge {
    pub players: Vec<ReplayPlayerInfo>,
    /// Resolved shot hits from ShotKills packets, matched to originating salvos.
    pub shot_hits: Vec<ResolvedShotHit>,
    pub last_clock: GameClock,
    /// The entity this bridge tracks (the ship whose armor viewer is open).
    pub target_entity_id: EntityId,
    /// Incremented each time data is cleared (seek/rebuild). Consumers use
    /// this to detect stale state.
    pub generation: u64,
    /// Pre-computed shot timeline for this target ship (entire replay).
    /// Set after the shot extraction pass completes.
    pub shot_timeline: Option<Arc<ShipShotTimeline>>,
}

impl RealtimeArmorBridge {
    pub fn new(target_entity_id: EntityId) -> Self {
        Self {
            players: Vec::new(),
            shot_hits: Vec::new(),
            last_clock: GameClock(0.0),
            target_entity_id,
            generation: 0,
            shot_timeline: None,
        }
    }
}

/// A request from the context menu to open a realtime armor viewer.
pub struct ArmorViewerRequest {
    pub target_entity_id: EntityId,
    pub bridge: Arc<Mutex<RealtimeArmorBridge>>,
    /// Sender for playback commands (seek, etc.) back to the replay thread.
    pub command_tx: mpsc::Sender<PlaybackCommand>,
}

/// Pre-resolved per-player build data for the roster hover popover.
/// Built once per published frame on the playback thread so the UI does no
/// translation or GameParams lookup work; every field is owned and ready to
/// render.
pub struct PlayerBuildDisplay {
    pub captain_name: Option<String>,
    /// Captain skill catalog laid out by tier (row 0 = tier 1, row 1 = tier 2,
    /// etc.). Each row contains every skill available to the captain for the
    /// player's ship species; learned skills are flagged so the popover can
    /// highlight them.
    pub skill_rows: Vec<SkillRow>,
    pub upgrades: Vec<EquipmentDisplay>,
    pub signals: Vec<EquipmentDisplay>,
}

pub struct SkillRow {
    pub tier: Option<u8>,
    pub skills: Vec<SkillDisplay>,
}

pub struct SkillDisplay {
    /// Snake_case slug matching the icon filename in `crew_skill_icons`.
    pub icon_key: String,
    pub name: String,
    pub description: String,
    pub tier: Option<u8>,
    /// True when the player actually took this skill (vs shown for context).
    pub learned: bool,
}

pub struct EquipmentDisplay {
    /// Full `Param::name()` matching the icon filename in
    /// `modernization_icons` / `signal_flag_icons`.
    pub icon_key: String,
    pub name: String,
    pub description: String,
}

/// Raw RGBA asset data loaded on the background thread.
/// Uses Arc to share cached data across renderer instances.
pub struct ReplayRendererAssets {
    pub map_image: Option<Arc<RgbaAsset>>,
    pub ship_icons: Arc<HashMap<String, RgbaAsset>>,
    pub plane_icons: Arc<HashMap<String, RgbaAsset>>,
    pub building_icons: Arc<HashMap<String, RgbaAsset>>,
    pub consumable_icons: Arc<HashMap<String, RgbaAsset>>,
    pub ribbon_icons: Arc<HashMap<String, RgbaAsset>>,
    pub subribbon_icons: Arc<HashMap<String, RgbaAsset>>,
    pub death_cause_icons: Arc<HashMap<String, RgbaAsset>>,
    pub powerup_icons: Arc<HashMap<String, RgbaAsset>>,
    pub crew_skill_icons: Arc<HashMap<String, RgbaAsset>>,
    pub modernization_icons: Arc<HashMap<String, RgbaAsset>>,
    pub signal_flag_icons: Arc<HashMap<String, RgbaAsset>>,
}

/// egui TextureHandles created on the UI thread.
struct RendererTextures {
    map_texture: Option<TextureHandle>,
    ship_icons: HashMap<String, TextureHandle>,
    /// Gold outline textures for detected-teammate highlight, keyed by the same variant keys as ship_icons.
    ship_icon_outlines: HashMap<String, TextureHandle>,
    plane_icons: HashMap<String, TextureHandle>,
    building_icons: HashMap<String, TextureHandle>,
    consumable_icons: HashMap<String, TextureHandle>,
    ribbon_icons: HashMap<String, TextureHandle>,
    subribbon_icons: HashMap<String, TextureHandle>,
    death_cause_icons: HashMap<String, TextureHandle>,
    powerup_icons: HashMap<String, TextureHandle>,
    crew_skill_icons: HashMap<String, TextureHandle>,
    modernization_icons: HashMap<String, TextureHandle>,
    signal_flag_icons: HashMap<String, TextureHandle>,
    silhouette_texture: Option<TextureHandle>,
}

/// Status of the background renderer.
pub enum RendererStatus {
    Loading,
    Ready,
    Error(String),
}

/// State shared between the UI and background threads.
pub struct SharedRendererState {
    pub status: RendererStatus,
    pub frame: Option<PlaybackFrame>,
    pub assets: Option<ReplayRendererAssets>,
    pub playing: bool,
    pub speed: f32,
    pub options: RenderOptions,
    pub show_dead_ships: bool,
    /// Viewport egui context, set by the UI thread on first draw.
    /// Used by the background thread to request repaints after frame updates.
    pub viewport_ctx: Option<egui::Context>,
    /// Pre-parsed timeline events for the entire replay.
    pub(crate) timeline_events: Option<Vec<TimelineEvent>>,
    /// Absolute game clock at which the battle started (after pre-battle countdown).
    /// Used to convert between absolute clock (used by seeking) and elapsed time (used by timeline).
    pub battle_start: GameClock,
    /// Absolute game clock at which the battle ended (from the BattleEnd packet).
    /// None when the replay ends before the result is decided.
    pub battle_end: Option<GameClock>,
    /// Actual game duration from the last packet's clock (may differ from metadata duration).
    pub actual_game_duration: Option<f32>,
    /// The replay owner's player name (from replay metadata).
    pub self_player_name: Option<String>,
    /// The replay owner's entity ID (resolved from draw commands).
    pub self_entity_id: Option<EntityId>,
    /// Game fonts loaded from game files, set by the background thread.
    pub game_fonts: Option<GameFonts>,
    /// Whether game fonts have been registered with the egui context.
    pub game_fonts_registered: bool,
    /// Active armor bridges for realtime armor viewer windows.
    pub armor_bridges: Vec<Arc<Mutex<RealtimeArmorBridge>>>,
    /// Pending requests to open realtime armor viewer windows (consumed by app.rs).
    pub pending_armor_viewers: Vec<ArmorViewerRequest>,
    /// Pre-computed shot timelines per target ship (entire replay).
    /// Set after the shot extraction pass completes.
    pub shot_timelines: Option<HashMap<EntityId, Arc<ShipShotTimeline>>>,
    /// Parsed replay/spectator keybinding groups from `commands.scheme.xml`.
    pub replay_controls: Option<Vec<CommandGroup>>,
    /// When a collab session is active, frames are cloned and sent here for broadcast.
    pub session_frame_tx: Option<std::sync::mpsc::SyncSender<FrameBroadcast>>,
    /// Game client version string from replay metadata (e.g. "0,13,7,0").
    pub game_version: Option<String>,
    /// Assigned collab replay ID when wired to a session.
    pub collab_replay_id: Option<u64>,
    /// True once a ReplayOpened command has been sent for this renderer.
    pub session_announced: bool,
    /// Version of render options last applied from the collab session.
    pub applied_render_options_version: u64,
    /// Version of annotation sync last applied from the collab session.
    pub applied_annotation_sync_version: u64,
    /// Version of per-ship range overrides last applied from the collab session.
    pub applied_range_override_version: u64,
    /// Version of per-ship trail overrides last applied from the collab session.
    pub applied_trail_override_version: u64,
    /// Reference to the collab session state (set by app.rs when wired to a session).
    pub collab_session_state: Option<Arc<Mutex<crate::collab::SessionState>>>,
    /// Receiver for playback frames from the collab peer task (bypasses ROOT event loop).
    pub collab_frame_rx: Option<std::sync::mpsc::Receiver<PlaybackFrame>>,
    /// Channel to send local UI events (cursors, annotations, pings, etc.) to the collab peer task.
    pub collab_local_tx: Option<std::sync::mpsc::Sender<crate::collab::peer::LocalEvent>>,
    /// Channel to send session commands (e.g. ReplayOpened) directly from the
    /// background thread, avoiding cross-window repaint issues.
    pub collab_command_tx: Option<std::sync::mpsc::Sender<crate::collab::SessionCommand>>,
    /// Replay name for collab announcements (set once at creation).
    pub collab_replay_name: Option<String>,
    /// Raw map name for collab announcements (e.g. "spaces/16_OC_bees_to_honey").
    pub collab_map_name: Option<String>,
    /// Map space size in BigWorld units (from MapInfo), used for px->km conversion.
    pub map_space_size: Option<f32>,
    /// Raw self-player ship silhouette (set by playback thread, converted to TextureHandle on UI thread).
    pub self_silhouette_raw: Option<(u32, u32, Vec<u8>)>,

    /// Cancellation signal for in-flight `step_session_to_clock` calls. Set
    /// to `true` when the UI issues a new Seek so the still-running step
    /// from a previous seek bails out immediately instead of finishing its
    /// (potentially long) backward rebuild. The playback thread resets it
    /// to `false` after acting on it.
    pub cancel_step: Arc<AtomicBool>,
    /// Per-player display-ready build snapshots keyed by entity ID.
    /// Refreshed by the playback thread alongside each published frame so the
    /// roster hover popover can render skills, modernizations, and signals
    /// without touching the live BattleController or doing translations on
    /// the UI thread.
    pub player_builds: HashMap<EntityId, Arc<PlayerBuildDisplay>>,
    /// Teams whose builds the popover is allowed to surface. Always includes
    /// the primary recording player's team; an enemy team is included only
    /// when one of the merged replays' recording players is on that team
    /// (mirrors and extends the replay inspector's enemy-build NDA gate).
    pub teams_with_replays: HashSet<i64>,
    /// Captured at launch from `ReplayDependencies::is_debug_mode`. When
    /// true, the build popover unmasks enemy loadouts regardless of which
    /// teams have replays — same override the replay inspector applies.
    pub is_debug_mode: bool,
}

/// The cloneable viewport handle stored in TabState.
/// What kind of video export action is pending behind the GPU warning dialog.
enum PendingVideoExport {
    /// Save to a user-chosen file path.
    SaveToFile {
        output_path: String,
        options: RenderOptions,
        prefer_cpu: bool,
        codec: Option<wows_minimap_renderer::VideoCodec>,
        actual_game_duration: Option<f32>,
        encoder_config: wows_minimap_renderer::EncoderConfig,
        include_pre_battle: bool,
    },
    /// Render to a temporary file and copy to clipboard.
    CopyToClipboard {
        options: RenderOptions,
        prefer_cpu: bool,
        codec: Option<wows_minimap_renderer::VideoCodec>,
        actual_game_duration: Option<f32>,
        encoder_config: wows_minimap_renderer::EncoderConfig,
        include_pre_battle: bool,
    },
}

/// State for the GPU encoder warning dialog.
struct GpuEncoderWarning {
    /// The pending export action to execute if the user clicks "Ok".
    pending_action: PendingVideoExport,
    /// Whether the "Don't show this again" checkbox is checked.
    dont_show_again: bool,
}

pub struct ReplayRendererViewer {
    pub title: Arc<String>,
    pub open: Arc<AtomicBool>,
    shared_state: Arc<Mutex<SharedRendererState>>,
    command_tx: mpsc::Sender<PlaybackCommand>,
    textures: Arc<Mutex<Option<RendererTextures>>>,
    /// When set, the main app loop should save these as default render options.
    pub pending_defaults_save: Arc<Mutex<Option<SavedRenderOptions>>>,
    /// Toast notifications for this renderer viewport.
    toasts: crate::tab_state::SharedToasts,
    /// Whether a video export is currently in progress.
    video_exporting: Arc<AtomicBool>,
    /// Progress of the current video export, updated by the background thread.
    video_export_progress: Arc<Mutex<Option<RenderProgress>>>,
    /// Data needed for video export (cloned from launch params).
    /// `None` for client viewers that don't have their own replay data.
    video_export_data: Option<Arc<VideoExportData>>,
    /// Zoom and pan state for the viewport. Persists across frames.
    zoom_pan: Arc<Mutex<ViewportZoomPan>>,
    /// Overlay controls visibility state.
    overlay_state: Arc<Mutex<OverlayState>>,
    /// Annotation/painting layer state.
    annotation_state: Arc<Mutex<AnnotationState>>,
    /// Local-only pings (shown when not in a collab session).
    local_pings: Arc<Mutex<Vec<shapes::MapPing>>>,
    /// Shared flag for "suppress GPU encoder warning" (persisted in Settings).
    pub suppress_gpu_warning: Arc<AtomicBool>,
    /// Active GPU encoder warning dialog, if any.
    gpu_encoder_warning: Arc<Mutex<Option<GpuEncoderWarning>>>,
    /// User preference: prefer CPU (software) encoder for video export.
    prefer_cpu_encoder: Arc<AtomicBool>,
    /// User preference: codec for video export. `None` means "best available".
    video_codec: Arc<Mutex<Option<wows_minimap_renderer::VideoCodec>>>,
    /// User preference: include the pre-battle phase in exported video.
    include_pre_battle: Arc<AtomicBool>,
    /// Shared window settings tracker for persisting viewport geometry.
    window_settings: crate::tab_state::SharedWindowSettings,
    /// Notify handle to trigger an immediate settings save.
    save_notify: Arc<tokio::sync::Notify>,
}

/// Decrypted bytes for one additional perspective replay merged into the
/// primary. Carried alongside the primary's bytes through every renderer
/// path so the playback thread, video export, and timeline extraction all
/// produce a merged view via
/// [`wows_battle_world::merged::MergedReplays`].
#[derive(Clone)]
pub struct AltReplayBytes {
    pub raw_meta: Vec<u8>,
    pub packet_data: Vec<u8>,
}

/// Data retained for video export. Cloned once at launch time.
struct VideoExportData {
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    alt_replays: Vec<AltReplayBytes>,
    map_name: String,
    replay_name: String,
    game_duration: f32,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
}
/// Create and launch a replay renderer in a background thread.
///
/// Returns a `ReplayRendererViewer` that can be drawn from the UI thread.
///
/// The `asset_cache` is shared across renderer instances to avoid reloading
/// ship/plane icons and map images from the game files on each launch.
#[allow(clippy::too_many_arguments)]
pub fn launch_replay_renderer(
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    alt_replays: Vec<AltReplayBytes>,
    map_name: String,
    replay_name: String,
    game_duration: f32,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    saved_options: &SavedRenderOptions,
    suppress_gpu_warning: Arc<AtomicBool>,
    window_settings: crate::tab_state::SharedWindowSettings,
    save_notify: Arc<tokio::sync::Notify>,
    is_debug_mode: bool,
) -> ReplayRendererViewer {
    let mut initial_options = render_options_from_saved(saved_options);
    // Merged replays automatically swap to the team-roster panel for the
    // session — the rosters are the whole point of having alt perspectives —
    // without overwriting the user's saved default. Single-replay sessions
    // keep whatever the saved options dictate.
    if !alt_replays.is_empty() {
        initial_options.show_stats_panel = false;
        initial_options.show_team_rosters = true;
    }
    let (command_tx, command_rx) = mpsc::channel();
    let shared_state = Arc::new(Mutex::new(SharedRendererState {
        status: RendererStatus::Loading,
        frame: None,
        assets: None,
        playing: false,
        speed: 20.0,
        options: initial_options,
        show_dead_ships: saved_options.show_dead_ships,
        viewport_ctx: None,
        timeline_events: None,
        battle_start: GameClock(0.0),
        battle_end: None,
        actual_game_duration: None,
        self_player_name: None,
        self_entity_id: None,
        game_fonts: None,
        game_fonts_registered: false,
        armor_bridges: Vec::new(),
        pending_armor_viewers: Vec::new(),
        shot_timelines: None,
        replay_controls: None,
        session_frame_tx: None,
        game_version: None,
        collab_replay_id: None,
        session_announced: false,
        applied_render_options_version: 0,
        applied_annotation_sync_version: 0,
        applied_range_override_version: 0,
        applied_trail_override_version: 0,
        collab_session_state: None,
        collab_frame_rx: None,
        collab_local_tx: None,
        collab_command_tx: None,
        collab_replay_name: Some(replay_name.clone()),
        collab_map_name: Some(map_name.clone()),
        map_space_size: None,
        self_silhouette_raw: None,
        cancel_step: Arc::new(AtomicBool::new(false)),
        player_builds: HashMap::new(),
        teams_with_replays: HashSet::new(),
        is_debug_mode,
    }));

    let title = Arc::new(format!("Replay Renderer - {replay_name}"));

    let video_export_data = Arc::new(VideoExportData {
        raw_meta: raw_meta.clone(),
        packet_data: packet_data.clone(),
        alt_replays: alt_replays.clone(),
        map_name: map_name.clone(),
        replay_name,
        game_duration,
        wows_data: wows_data.clone(),
        asset_cache: Arc::clone(&asset_cache),
    });

    let viewer = ReplayRendererViewer {
        title,
        open: Arc::new(AtomicBool::new(true)),
        shared_state: Arc::clone(&shared_state),
        command_tx,
        textures: Arc::new(Mutex::new(None)),
        pending_defaults_save: Arc::new(Mutex::new(None)),
        toasts: Arc::new(parking_lot::Mutex::new(egui_notify::Toasts::default())),
        video_exporting: Arc::new(AtomicBool::new(false)),
        video_export_progress: Arc::new(Mutex::new(None)),
        video_export_data: Some(video_export_data),
        zoom_pan: Arc::new(Mutex::new(ViewportZoomPan::default())),
        overlay_state: Arc::new(Mutex::new(OverlayState::default())),
        annotation_state: Arc::new(Mutex::new({
            let mut ann = AnnotationState::default();
            let filter = saved_options.self_range_filter();
            if filter.any_enabled() {
                ann.pending_self_range_filter = Some(filter);
            }
            ann
        })),
        local_pings: Arc::new(Mutex::new(Vec::new())),
        suppress_gpu_warning,
        gpu_encoder_warning: Arc::new(Mutex::new(None)),
        prefer_cpu_encoder: Arc::new(AtomicBool::new(saved_options.prefer_cpu_encoder)),
        video_codec: Arc::new(Mutex::new(saved_options.video_codec)),
        include_pre_battle: Arc::new(AtomicBool::new(saved_options.include_pre_battle)),
        window_settings,
        save_notify,
    };

    let open = Arc::clone(&viewer.open);

    crate::util::thread::spawn_logged("replay-playback", move || {
        playback_thread(
            raw_meta,
            packet_data,
            alt_replays,
            map_name,
            game_duration,
            wows_data,
            asset_cache,
            shared_state,
            command_rx,
            open,
        );
    });

    viewer
}

/// Create a lightweight client viewer for a collaborative session.
///
/// The client doesn't have its own replay data — it receives rendered frames
/// from the host. The map image is decoded from the PNG sent in SessionInfo.
/// Ship icons, fonts, and other assets are loaded from the local game data.
#[allow(clippy::too_many_arguments)]
pub fn launch_client_renderer(
    replay_name: String,
    map_image_png: Vec<u8>,
    game_version: String,
    saved_options: &SavedRenderOptions,
    suppress_gpu_warning: Arc<AtomicBool>,
    wows_data: Option<&SharedWoWsData>,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
    window_settings: crate::tab_state::SharedWindowSettings,
    save_notify: Arc<tokio::sync::Notify>,
    is_debug_mode: bool,
) -> ReplayRendererViewer {
    let initial_options = render_options_from_saved(saved_options);
    let (_command_tx, _command_rx) = mpsc::channel();

    // Decode PNG to RGBA
    let map_image = image::load_from_memory(&map_image_png).ok().map(|img| {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        Arc::new(RgbaAsset { data: rgba.into_raw(), width: w, height: h })
    });

    // Load icons and fonts from VFS via the shared asset cache.
    let (
        ship_icons,
        plane_icons,
        building_icons,
        consumable_icons,
        ribbon_icons,
        subribbon_icons,
        death_cause_icons,
        powerup_icons,
        crew_skill_icons,
        modernization_icons,
        signal_flag_icons,
        game_fonts,
    ) = if let Some((vfs, version, dump_dir)) = wows_data.map(|d| {
        let guard = d.read();
        (guard.vfs.clone(), guard.version().copied(), guard.dump_dir.clone())
    }) {
        let version = version.as_ref();
        let dump_dir = dump_dir.as_deref();
        let mut cache = asset_cache.lock();
        // Icons auto-borrow from the newest dump for old replays that ship none;
        // fonts stay on the replay's own VFS.
        let si = cache.get_or_load_ship_icons(&vfs, version, dump_dir);
        let pi = cache.get_or_load_plane_icons(&vfs, version, dump_dir);
        let bi = cache.get_or_load_building_icons(&vfs, version, dump_dir);
        let ci = cache.get_or_load_consumable_icons(&vfs, version, dump_dir);
        let ri = cache.get_or_load_ribbon_icons(&vfs, version, dump_dir);
        let sri = cache.get_or_load_subribbon_icons(&vfs, version, dump_dir);
        let di = cache.get_or_load_death_cause_icons(&vfs, version, dump_dir);
        let pwi = cache.get_or_load_powerup_icons(&vfs, version, dump_dir);
        let ski = cache.get_or_load_crew_skill_icons(&vfs, version, dump_dir);
        let mi = cache.get_or_load_modernization_icons(&vfs, version, dump_dir);
        let sfi = cache.get_or_load_signal_flag_icons(&vfs, version, dump_dir);
        let gf = cache.get_or_load_game_fonts(&vfs, version, dump_dir);
        (si, pi, bi, ci, ri, sri, di, pwi, ski, mi, sfi, Some(gf))
    } else {
        (
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            Arc::new(HashMap::new()),
            None,
        )
    };

    let shared_state = Arc::new(Mutex::new(SharedRendererState {
        status: RendererStatus::Loading,
        frame: None,
        assets: Some(ReplayRendererAssets {
            map_image,
            ship_icons,
            plane_icons,
            building_icons,
            consumable_icons,
            ribbon_icons,
            subribbon_icons,
            death_cause_icons,
            powerup_icons,
            crew_skill_icons,
            modernization_icons,
            signal_flag_icons,
        }),
        playing: false,
        speed: 1.0,
        options: initial_options,
        show_dead_ships: saved_options.show_dead_ships,
        viewport_ctx: None,
        timeline_events: None,
        battle_start: GameClock(0.0),
        battle_end: None,
        actual_game_duration: None,
        self_player_name: None,
        self_entity_id: None,
        game_fonts,
        game_fonts_registered: false,
        armor_bridges: Vec::new(),
        pending_armor_viewers: Vec::new(),
        shot_timelines: None,
        replay_controls: None,
        session_frame_tx: None,
        game_version: Some(game_version),
        collab_replay_id: None,
        session_announced: false,
        applied_render_options_version: 0,
        applied_annotation_sync_version: 0,
        applied_range_override_version: 0,
        applied_trail_override_version: 0,
        collab_session_state: None,
        collab_frame_rx: None,
        collab_local_tx: None,
        collab_command_tx: None,
        collab_replay_name: None,
        collab_map_name: None,
        map_space_size: None,
        self_silhouette_raw: None,
        cancel_step: Arc::new(AtomicBool::new(false)),
        player_builds: HashMap::new(),
        teams_with_replays: HashSet::new(),
        is_debug_mode,
    }));

    let title = Arc::new(format!("Collab Viewer - {replay_name}"));

    ReplayRendererViewer {
        title,
        open: Arc::new(AtomicBool::new(true)),
        shared_state: Arc::clone(&shared_state),
        command_tx: _command_tx,
        textures: Arc::new(Mutex::new(None)),
        pending_defaults_save: Arc::new(Mutex::new(None)),
        toasts: Arc::new(parking_lot::Mutex::new(egui_notify::Toasts::default())),
        video_exporting: Arc::new(AtomicBool::new(false)),
        video_export_progress: Arc::new(Mutex::new(None)),
        video_export_data: None,
        zoom_pan: Arc::new(Mutex::new(ViewportZoomPan::default())),
        overlay_state: Arc::new(Mutex::new(OverlayState::default())),
        annotation_state: Arc::new(Mutex::new({
            let mut ann = AnnotationState::default();
            let filter = saved_options.self_range_filter();
            if filter.any_enabled() {
                ann.pending_self_range_filter = Some(filter);
            }
            ann
        })),
        local_pings: Arc::new(Mutex::new(Vec::new())),
        suppress_gpu_warning,
        gpu_encoder_warning: Arc::new(Mutex::new(None)),
        prefer_cpu_encoder: Arc::new(AtomicBool::new(false)),
        video_codec: Arc::new(Mutex::new(None)),
        include_pre_battle: Arc::new(AtomicBool::new(false)),
        window_settings,
        save_notify,
    }
}

mod playback;
use playback::playback_thread;

mod timeline;
pub use timeline::PreExtractedHit;
pub use timeline::ShipShotTimeline;
pub(crate) use timeline::TimelineEvent;
pub(crate) use timeline::TimelineEventKind;
pub(crate) use timeline::event_color;
pub(crate) use timeline::format_timeline_event;

mod video_export;
pub use video_export::BatchEncodeOptions;
pub use video_export::BatchReplayInfo;
pub use video_export::batch_render_to_clipboard;
pub use video_export::batch_render_to_folder;
use video_export::execute_video_export;

mod shapes;
use shapes::*;

mod textures;
use textures::upload_textures;
impl ReplayRendererViewer {
    /// Access the shared renderer state (for polling pending requests, etc.).
    pub fn shared_state(&self) -> &Arc<Mutex<SharedRendererState>> {
        &self.shared_state
    }

    /// The [`egui::ViewportId`] used by this viewer's deferred viewport.
    pub fn viewport_id(&self) -> egui::ViewportId {
        egui::ViewportId::from_hash_of(&*self.title)
    }

    pub fn draw(&self, ctx: &egui::Context) {
        // Store the parent context so the background thread can request repaints.
        // Deferred viewports repaint as part of their parent's paint cycle.
        {
            let mut state = self.shared_state.lock();
            if state.viewport_ctx.is_none() {
                state.viewport_ctx = Some(ctx.clone());
            }
        }

        let shared_state = self.shared_state.clone();
        let command_tx = self.command_tx.clone();
        let window_open = self.open.clone();
        let textures_arc = self.textures.clone();
        let pending_save = self.pending_defaults_save.clone();
        let toasts = self.toasts.clone();
        let video_exporting = self.video_exporting.clone();
        let video_export_progress = self.video_export_progress.clone();
        let video_export_data = self.video_export_data.clone();
        let zoom_pan_arc = self.zoom_pan.clone();
        let overlay_state_arc = self.overlay_state.clone();
        let annotation_arc = self.annotation_state.clone();
        let local_pings_arc = self.local_pings.clone();
        let suppress_gpu_warning = self.suppress_gpu_warning.clone();
        let gpu_encoder_warning = self.gpu_encoder_warning.clone();
        let prefer_cpu_encoder = self.prefer_cpu_encoder.clone();
        let video_codec = self.video_codec.clone();
        let include_pre_battle = self.include_pre_battle.clone();
        let window_settings = self.window_settings.clone();
        let save_notify = self.save_notify.clone();
        let parent_ctx = ctx.clone();
        let viewport_id = egui::ViewportId::from_hash_of(&*self.title);

        // Apply persisted window size if available.
        let builder = egui::ViewportBuilder::default().with_title(&*self.title).with_min_inner_size([400.0, 450.0]);
        let builder = window_settings
            .lock()
            .settings
            .get(&crate::tab_state::WindowKind::ReplayRenderer)
            .map(|s| s.apply_to_builder(builder.clone(), [800.0, 900.0]))
            .unwrap_or_else(|| builder.with_inner_size([800.0, 900.0]));

        ctx.show_viewport_deferred(
            viewport_id,
            builder,
            move |viewport_ui, _class| {
                let ctx = viewport_ui.ctx().clone();
                if !window_open.load(Ordering::Relaxed) || crate::app::mitigate_wgpu_mem_leak(&ctx) {
                    return;
                }

                let mut repaint = false;

                let mut state = shared_state.lock();

                // Pull latest frame from collab channel before any status checks
                // so the Loading->Ready transition sees the first frame immediately.
                if let Some(rx) = state.collab_frame_rx.take() {
                    while let Ok(frame) = rx.try_recv() {
                        state.frame = Some(frame);
                        repaint = true;
                    }
                    state.collab_frame_rx = Some(rx);
                }

                // Register game fonts with egui on the first frame.
                // set_fonts() doesn't take effect until the next frame, so we
                // track whether we just registered to avoid using them too early.
                let mut fonts_just_registered = false;
                if !state.game_fonts_registered {
                    let mut font_defs = ctx.fonts(|r| r.definitions().clone());
                    register_game_fonts(&mut font_defs, state.game_fonts.as_ref());
                    ctx.set_fonts(font_defs);
                    state.game_fonts_registered = true;
                    fonts_just_registered = true;
                }

                // For client renderers: transition Loading->Ready once fonts are
                // effective (registered on a prior frame) and a frame has arrived.
                if matches!(state.status, RendererStatus::Loading)
                    && state.frame.is_some()
                    && !fonts_just_registered
                {
                    tracing::debug!("Renderer: Loading->Ready (fonts effective, frame available)");
                    state.status = RendererStatus::Ready;
                }

                let status_is_loading = matches!(state.status, RendererStatus::Loading);
                let status_error = match &state.status {
                    RendererStatus::Error(e) => Some(e.clone()),
                    _ => None,
                };
                let has_assets = state.assets.is_some();
                let playing = state.playing;
                let speed = state.speed;
                let options = state.options.clone();
                let show_dead_ships = state.show_dead_ships;
                let battle_start = state.battle_start;
                let battle_end = state.battle_end;
                let actual_game_duration = state.actual_game_duration;
                let frame_data =
                    state.frame.as_ref().map(|f| (f.frame_index, f.total_frames, f.clock, f.game_duration));
                // Resolve self entity ID from draw commands (once)
                let self_entity_id = if state.self_entity_id.is_some() {
                    state.self_entity_id
                } else if let Some(ref frame) = state.frame {
                    let eid = frame.commands.iter().find_map(|cmd| {
                        if let DrawCommand::Ship { entity_id, is_self: true, .. } = cmd {
                            Some(*entity_id)
                        } else {
                            None
                        }
                    });
                    if eid.is_some() {
                        state.self_entity_id = eid;
                    }
                    eid
                } else {
                    None
                };

                drop(state);

                // Apply pending render options / annotation sync from collab session (version-based).
                // Session lifecycle events (Started, Ended, etc.) are polled by app.rs.
                {
                    let mut state = shared_state.lock();
                    if let Some(ref session_state_arc) = state.collab_session_state.clone() {
                        let s = session_state_arc.lock();
                            if s.render_options_version > state.applied_render_options_version {
                                if let Some(ref opts) = s.current_render_options {
                                    state.options.show_hp_bars = opts.show_hp_bars;
                                    state.options.show_tracers = opts.show_tracers;
                                    state.options.show_torpedoes = opts.show_torpedoes;
                                    state.options.show_planes = opts.show_planes;
                                    state.options.show_smoke = opts.show_smoke;
                                    state.options.show_score = opts.show_score;
                                    state.options.show_timer = opts.show_timer;
                                    state.options.show_kill_feed = opts.show_kill_feed;
                                    state.options.show_player_names = opts.show_player_names;
                                    state.options.show_ship_names = opts.show_ship_names;
                                    state.options.show_capture_points = opts.show_capture_points;
                                    state.options.show_buildings = opts.show_buildings;
                                    state.options.show_camera_direction = opts.show_camera_direction;
                                    state.options.show_consumables = opts.show_consumables;
                                    state.options.show_armament = opts.show_armament;
                                    state.options.show_trails = opts.show_trails;
                                    state.options.show_dead_trails = opts.show_dead_trails;
                                    state.options.show_speed_trails = opts.show_speed_trails;
                                    state.options.show_ship_config = opts.show_ship_config;
                                    state.options.show_dead_ship_names = opts.show_dead_ship_names;
                                    state.options.show_battle_result = opts.show_battle_result;
                                    state.options.show_buffs = opts.show_buffs;
                                    state.options.show_chat = opts.show_chat;
                                    state.options.show_advantage = opts.show_advantage;
                                    state.options.show_score_timer = opts.show_score_timer;
                                    state.show_dead_ships = opts.show_dead_ships;
                                }
                                state.applied_render_options_version = s.render_options_version;
                            }
                            if s.annotation_sync_version > state.applied_annotation_sync_version {
                                if let Some(ref sync) = s.current_annotation_sync {
                                    let mut ann = annotation_arc.lock();
                                    ann.annotations = sync.annotations.iter().cloned().map(collab_annotation_to_local).collect();
                                    ann.annotation_owners = sync.owners.clone();
                                    ann.annotation_ids = sync.ids.clone();
                                }
                                state.applied_annotation_sync_version = s.annotation_sync_version;
                            }
                            if s.range_override_version > state.applied_range_override_version {
                                if let Some(ref overrides) = s.current_range_overrides {
                                    let mut ann = annotation_arc.lock();
                                    ann.ship_range_overrides.clear();
                                    for &(eid, filter) in overrides {
                                        ann.ship_range_overrides.insert(eid, filter);
                                    }
                                    // Enable show_ship_config if any overrides are present
                                    if !ann.ship_range_overrides.is_empty() && !state.options.show_ship_config {
                                        state.options.show_ship_config = true;
                                    }
                                }
                                state.applied_range_override_version = s.range_override_version;
                            }
                            if s.trail_override_version > state.applied_trail_override_version {
                                if let Some(ref hidden) = s.current_trail_hidden {
                                    let mut ann = annotation_arc.lock();
                                    ann.trail_hidden_ships.clear();
                                    for name in hidden {
                                        ann.trail_hidden_ships.insert(name.clone());
                                    }
                                    // Enable show_trails if there are still some visible trails
                                    if !ann.trail_hidden_ships.is_empty() && !state.options.show_trails {
                                        state.options.show_trails = true;
                                    }
                                }
                                state.applied_trail_override_version = s.trail_override_version;
                            }
                        }
                }

                // Apply pending self range filter once self entity ID is known
                if let Some(self_eid) = self_entity_id {
                    let mut ann = annotation_arc.lock();
                    if let Some(filter) = ann.pending_self_range_filter.take() {
                        ann.ship_range_overrides.insert(self_eid, filter);
                        // Ensure show_ship_config is enabled
                        let mut state = shared_state.lock();
                        if !state.options.show_ship_config {
                            state.options.show_ship_config = true;
                        }
                    }
                }

                // Upload textures on first ready frame
                {
                    let mut tex_guard = textures_arc.lock();
                    if tex_guard.is_none() && has_assets {
                        let state = shared_state.lock();
                        if let Some(assets) = &state.assets {
                            // Send silhouette to collab peers if available at initial upload time.
                            if let Some((w, h, data)) = state.self_silhouette_raw.as_ref()
                                && let Some(ref tx) = state.collab_local_tx {
                                    let _ = tx.send(crate::collab::peer::LocalEvent::SelfSilhouette {
                                        data: data.clone(),
                                        width: *w,
                                        height: *h,
                                    });
                                }
                            *tex_guard = Some(upload_textures(&ctx, assets, state.self_silhouette_raw.as_ref()));
                        }
                    }
                    // Lazy silhouette upload: raw data may arrive after initial texture upload
                    if let Some(textures) = tex_guard.as_mut()
                        && textures.silhouette_texture.is_none() {
                            let state = shared_state.lock();
                            if let Some((w, h, data)) = state.self_silhouette_raw.as_ref() {
                                let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
                                textures.silhouette_texture = Some(ctx.load_texture("stats_silhouette", image, egui::TextureOptions::LINEAR));
                                // Send to collab peers so web clients can render the stats panel silhouette.
                                if let Some(ref tx) = state.collab_local_tx {
                                    let _ = tx.send(crate::collab::peer::LocalEvent::SelfSilhouette {
                                        data: data.clone(),
                                        width: *w,
                                        height: *h,
                                    });
                                }
                            }
                        }
                }

                // ── Annotation toolbar ──
                if !status_is_loading {
                    egui::Panel::top("replay_annotation_toolbar").show_inside(viewport_ui, |ui| {
                        let locked = shared_state
                            .lock()
                            .collab_session_state
                            .as_ref()
                            .map(|ss| ss.lock().permissions.annotations_locked)
                            .unwrap_or(false);
                        let mut ann = annotation_arc.lock();
                        let tex_guard = textures_arc.lock();
                        let ship_icons = tex_guard.as_ref().map(|t| &t.ship_icons);
                        let result = wt_collab_egui::toolbar::draw_annotation_toolbar(
                            ui,
                            &mut ann,
                            ship_icons,
                            locked,
                        );
                        drop(tex_guard);
                        if result.did_clear {
                            send_annotation_clear(&shared_state.lock().collab_local_tx, None);
                        }
                        if result.did_undo {
                            send_annotation_full_sync(&shared_state.lock().collab_command_tx, &ann, None);
                        }
                    });
                }

                let (show_stats_panel, show_team_rosters) = {
                    let opts = &shared_state.lock().options;
                    let loading = status_is_loading;
                    // Stats panel and team rosters share the same gutter; if a
                    // legacy settings file has both on, team rosters win.
                    let team = !loading && opts.show_team_rosters;
                    let stats = !loading && opts.show_stats_panel && !team;
                    (stats, team)
                };

                egui::CentralPanel::default().show_inside(viewport_ui, |ui| {
                    if status_is_loading {
                        ui.centered_and_justified(|ui| {
                            ui.spinner();
                            ui.label(t!("ui.renderer.loading"));
                        });
                        ctx.request_repaint();
                        return;
                    }

                    if let Some(err) = status_error {
                        ui.colored_label(Color32::RED, format!("Error: {}", err));
                        return;
                    }

                    // Canvas layout: team rosters reserve a left+right gutter and
                    // replace the self-perspective stats panel. When rosters are
                    // off, the stats panel takes its own right-side strip.
                    let roster_gutter = if show_team_rosters { TEAM_ROSTER_WIDTH as f32 } else { 0.0 };
                    let stats_strip = if show_stats_panel && !show_team_rosters {
                        STATS_PANEL_WIDTH as f32
                    } else {
                        0.0
                    };
                    let map_x_offset = roster_gutter;
                    let canvas_w = MINIMAP_SIZE as f32 + roster_gutter * 2.0 + stats_strip;
                    let logical_canvas = Vec2::new(canvas_w, CANVAS_HEIGHT as f32);
                    let available = ui.available_size();
                    let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());
                    let map_w = if show_stats_panel || show_team_rosters {
                        Some(MINIMAP_SIZE as f32)
                    } else {
                        None
                    };
                    // Pass zoom=1.0 so the canvas (and therefore HUD/panels)
                    // stays fit-scaled regardless of the slider. Map content
                    // still zooms via the MapTransform's zoom factor below;
                    // HUD-text overlays (PreBattleCountdown, BattleResultOverlay)
                    // scale themselves explicitly off transform.zoom.
                    let layout = compute_canvas_layout(available, logical_canvas, 1.0, response.rect.min, map_w);
                    let window_scale = layout.window_scale;

                    // Zoom/pan input handling
                    let zoom_changed = {
                        let mut zp = zoom_pan_arc.lock();
                        handle_viewport_zoom_pan(
                            &ctx,
                            &response,
                            &mut zp,
                            &layout,
                            logical_canvas,
                            &ZoomPanConfig {
                                allow_left_drag_pan: true,
                                hud_height: HUD_HEIGHT as f32,
                                handle_tool_yaw: true,
                                map_width: map_w,
                                map_x_offset,
                            },
                            Some(&mut annotation_arc.lock()),
                            false,
                        )
                    };

                    // Build transform for this frame
                    let zp = zoom_pan_arc.lock();
                    // HUD elements like the score bar are emitted in canvas-space
                    // starting at x=0. When rosters are on, hud_width is the full
                    // gutter+map+gutter area so the score bar spans across all of it.
                    // The stats-panel strip (when also on) is excluded.
                    let hud_w = MINIMAP_SIZE as f32 + roster_gutter * 2.0;
                    let transform = MapTransform {
                        origin: layout.origin,
                        window_scale,
                        zoom: zp.zoom,
                        pan: zp.pan,
                        hud_height: HUD_HEIGHT as f32,
                        canvas_height: CANVAS_HEIGHT as f32,
                        canvas_width: canvas_w,
                        hud_width: hud_w,
                        map_x_offset,
                    };
                    let current_zoom = zp.zoom;
                    drop(zp);

                    // Cursor icon based on tool / zoom state
                    if response.hovered() {
                        let cursor = {
                            let ann = annotation_arc.lock();
                            let base = annotation_cursor_icon(&ann, &response, &transform);
                            if base.is_some() {
                                base
                            } else {
                                // No annotation cursor — check if hovering over a ship
                                let near_ship = response.hover_pos().is_some_and(|hp| {
                                    let state = shared_state.lock();
                                    if let Some(ref frame) = state.frame {
                                        frame.commands.iter().any(|cmd| {
                                            if let DrawCommand::Ship { pos, player_name: Some(_), .. } = cmd {
                                                let sp = transform.minimap_to_screen(pos);
                                                hp.distance(sp) < 30.0
                                            } else {
                                                false
                                            }
                                        })
                                    } else {
                                        false
                                    }
                                });
                                if near_ship {
                                    Some(egui::CursorIcon::PointingHand)
                                } else {
                                    None
                                }
                            }
                        };
                        if let Some(c) = cursor {
                            if response.dragged() && c == egui::CursorIcon::Grab {
                                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                            } else {
                                ctx.set_cursor_icon(c);
                            }
                        } else if current_zoom > 1.01 {
                            if response.dragged() {
                                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                            } else {
                                ctx.set_cursor_icon(egui::CursorIcon::Grab);
                            }
                        }
                    }

                    // Request repaint if zoom/pan changed while paused
                    if zoom_changed && !playing {
                        repaint = true;
                    }

                    // Draw dark background
                    painter.rect_filled(response.rect, CornerRadius::ZERO, Color32::from_rgb(20, 25, 35));

                    // Clipped painter for map-region content (below HUD).
                    // Clip to MINIMAP_SIZE when any side panel is on, so map content
                    // doesn't bleed into the stats strip or roster gutters.
                    let map_clip_width = if show_stats_panel || show_team_rosters {
                        Some(MINIMAP_SIZE as f32)
                    } else {
                        None
                    };
                    let map_clip = compute_map_clip_rect(&layout, HUD_HEIGHT as f32, map_clip_width, map_x_offset);
                    let map_painter = painter.with_clip_rect(map_clip);

                    let tex_guard = textures_arc.lock();
                    if let Some(ref textures) = *tex_guard {
                        // Draw map background texture
                        draw_map_background(&map_painter, &transform, textures.map_texture.as_ref().map(|t| t.id()));

                        // Draw grid overlay (A-J / 1-10)
                        draw_grid(&map_painter, &transform, &GridStyle {
                            grid_color: Color32::from_rgba_unmultiplied(255, 255, 255, 64),
                            label_color: Color32::from_rgba_unmultiplied(255, 255, 255, 180),
                            line_width: 1.0,
                            label_font: game_font(11.0 * window_scale),
                        });

                        // Draw current frame's commands, filtered by UI-local options
                        let (trail_hidden_ships, ship_range_overrides) = {
                            let ann = annotation_arc.lock();
                            (ann.trail_hidden_ships.clone(), ann.ship_range_overrides.clone())
                        };
                        let state = shared_state.lock();
                        if let Some(ref frame) = state.frame {
                            // Collect alive ship entity IDs for filtering config circles
                            let alive_ships: HashSet<EntityId> = frame
                                .commands
                                .iter()
                                .filter_map(|cmd| {
                                    if let DrawCommand::Ship { entity_id, .. } = cmd {
                                        Some(*entity_id)
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            // Separate HUD and map commands so HUD draws on unclipped painter
                            let mut placed_labels: Vec<Rect> = Vec::new();
                            for cmd in &frame.commands {
                                // Stats commands are rendered in the egui SidePanel, not here
                                if show_stats_panel && cmd.is_stats() {
                                    continue;
                                }
                                if !should_draw_command(cmd, &options, show_dead_ships) {
                                    continue;
                                }
                                // Apply per-ship trail filter
                                if let DrawCommand::PositionTrail { player_name, .. } = cmd
                                    && let Some(name) = player_name
                                        && trail_hidden_ships.contains(name) {
                                            continue;
                                        }
                                // Apply per-ship config circle filter (only show if explicitly enabled via right-click, never for dead ships)
                                if let DrawCommand::ShipConfigCircle { entity_id, kind, .. } = cmd {
                                    if !alive_ships.contains(entity_id) {
                                        continue;
                                    }
                                    let enabled = if let Some(filter) = ship_range_overrides.get(entity_id) {
                                        filter.is_enabled(kind)
                                    } else {
                                        false // hidden by default; must enable per-ship via right-click
                                    };
                                    if !enabled {
                                        continue;
                                    }
                                }
                                let is_hud = cmd.is_hud();
                                let cmd_shapes = draw_command_to_shapes(cmd, &transform, textures, &ctx, &options, &mut placed_labels, &LocalizedTextResolver);
                                let target_painter = if is_hud { &painter } else { &map_painter };
                                for shape in cmd_shapes {
                                    target_painter.add(shape);
                                }
                            }

                            // Stats panel + team rosters: both ride on `is_stats()` commands,
                            // dispatched in a single unzoomed pass aligned to the canvas.
                            if show_stats_panel || show_team_rosters {
                                let stats_transform = MapTransform {
                                    origin: layout.origin,
                                    window_scale,
                                    zoom: 1.0,
                                    pan: Vec2::ZERO,
                                    hud_height: HUD_HEIGHT as f32,
                                    canvas_height: CANVAS_HEIGHT as f32,
                                    canvas_width: canvas_w,
                                    hud_width: hud_w,
                                    map_x_offset,
                                };
                                let shared_tex = make_shared_textures(textures);
                                let label_opts = make_label_opts(&options);
                                let mut stats_placed = Vec::new();
                                let mut hover_regions: Vec<wt_collab_egui::draw_commands::ConsumableHoverRegion> = Vec::new();
                                let mut player_build_regions: Vec<wt_collab_egui::draw_commands::PlayerBuildHoverRegion> = Vec::new();
                                for cmd in &frame.commands {
                                    if !cmd.is_stats() {
                                        continue;
                                    }
                                    let cmd_shapes = wt_collab_egui::draw_commands::draw_command_to_shapes(
                                        cmd, &stats_transform, &shared_tex, &ctx, &label_opts,
                                        Some(&mut stats_placed), &LocalizedTextResolver,
                                        Some(&mut hover_regions),
                                        Some(&mut player_build_regions),
                                    );
                                    for shape in cmd_shapes {
                                        painter.add(shape);
                                    }
                                }
                                for (idx, region) in hover_regions.iter().enumerate() {
                                    let id = egui::Id::new(("roster_consumable_hover", idx));
                                    let hover_resp = ui.interact(region.rect, id, egui::Sense::hover());
                                    let tex = textures.consumable_icons.get(&region.icon_key);
                                    hover_resp.on_hover_ui(|ui| {
                                        roster_consumable_tooltip(ui, region, tex);
                                    });
                                }
                                // Resolve build snapshots up front: the outer
                                // `state` lock is still held across this block,
                                // and parking_lot Mutex isn't reentrant, so we
                                // can't relock it inside the hover closure.
                                // Same reason we copy `teams_with_replays` here
                                // instead of borrowing it inside the loop.
                                let build_snapshots: Vec<Option<Arc<PlayerBuildDisplay>>> = player_build_regions
                                    .iter()
                                    .map(|r| state.player_builds.get(&r.entity_id).cloned())
                                    .collect();
                                let teams_with_replays = state.teams_with_replays.clone();
                                let debug_mode = state.is_debug_mode;
                                for (idx, region) in player_build_regions.iter().enumerate() {
                                    // Only attach the build popover when the
                                    // row's team is represented by one of our
                                    // replays (always for own team, for the
                                    // enemy team only when an alt-replay is
                                    // from someone on that side) — unless
                                    // debug mode is on, which unmasks
                                    // everything just like the inspector.
                                    if !debug_mode && !teams_with_replays.contains(&region.team_id) {
                                        continue;
                                    }
                                    // Stable ID across frames so the pinned
                                    // popup state survives layout reflows.
                                    let id = egui::Id::new(("roster_player_build", region.entity_id));
                                    let resp = ui.interact(region.rect, id, egui::Sense::click());
                                    let snapshot = build_snapshots[idx].clone();
                                    // Click toggles a pinned, interactive
                                    // popup the user can hover inside; the
                                    // popup closes when the user clicks
                                    // outside it.
                                    egui::Popup::from_toggle_button_response(&resp)
                                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                        .show(|ui| {
                                            roster_player_build_tooltip(ui, region, snapshot.as_ref(), textures);
                                        });
                                    // While not pinned, fall back to a transient
                                    // hover tooltip so the build is still
                                    // discoverable without committing a click.
                                    if !egui::Popup::is_id_open(&ctx, id) {
                                        let snapshot = snapshot.clone();
                                        resp.on_hover_ui(|ui| {
                                            roster_player_build_tooltip(ui, region, snapshot.as_ref(), textures);
                                        });
                                    }
                                }
                            }

                            // Hover tooltip for TeamAdvantage
                            let ws = transform.window_scale;
                            // Find ScoreBar to compute advantage label position
                            let score_bar_info: Option<ScoreBarInfo> = frame.commands.iter().find_map(|cmd| {
                                if let DrawCommand::ScoreBar { team0, team1, team0_timer, team1_timer, advantage, .. } = cmd {
                                    let adv_team = advantage.as_ref().map(|(_, t)| *t as i32).unwrap_or(-1);
                                    Some(ScoreBarInfo {
                                        team0_score: *team0,
                                        team1_score: *team1,
                                        team0_timer: team0_timer.clone(),
                                        team1_timer: team1_timer.clone(),
                                        adv_team,
                                    })
                                } else {
                                    None
                                }
                            });
                            for cmd in &frame.commands {
                                if let DrawCommand::TeamAdvantage { level, color, breakdown } = cmd {
                                    let Some(adv_level) = level else {
                                        break;
                                    };
                                    let label = adv_level.label().to_string();
                                    let canvas_w = transform.screen_hud_width();
                                    let bar_height = HUD_HEIGHT as f32 * ws;
                                    let bar_origin = transform.hud_pos(0.0, 0.0);

                                    // Recompute cursor positions matching ScoreBar rendering
                                    let score_font = game_font(14.0 * ws);
                                    let timer_font = game_font(12.0 * ws);
                                    let adv_font = game_font(11.0 * ws);
                                    let adv_color = color_from_rgb(*color);
                                    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.clone(), adv_font, adv_color));
                                    let text_w = galley.size().x;
                                    let text_h = galley.size().y;

                                    let (t0_end_x, t1_start_x) = if let Some(ScoreBarInfo { team0_score: t0_score, team1_score: t1_score, team0_timer: ref t0_timer, team1_timer: ref t1_timer, .. }) = score_bar_info {
                                        let t0_text = format!("{}", t0_score);
                                        let t0g = ctx.fonts_mut(|f| f.layout_no_wrap(t0_text, score_font.clone(), Color32::WHITE));
                                        let mut t0_end = bar_origin.x + 8.0 * ws + t0g.size().x;
                                        if let Some(timer) = t0_timer {
                                            let tg = ctx.fonts_mut(|f| f.layout_no_wrap(timer.clone(), timer_font.clone(), Color32::WHITE));
                                            t0_end = t0_end + 4.0 * ws + tg.size().x;
                                        }
                                        let t1_text = format!("{}", t1_score);
                                        let t1g = ctx.fonts_mut(|f| f.layout_no_wrap(t1_text, score_font, Color32::WHITE));
                                        let mut t1_start = bar_origin.x + canvas_w - t1g.size().x - 8.0 * ws;
                                        if let Some(timer) = t1_timer {
                                            let tg = ctx.fonts_mut(|f| f.layout_no_wrap(timer.clone(), timer_font, Color32::WHITE));
                                            t1_start = t1_start - tg.size().x - 4.0 * ws;
                                        }
                                        (t0_end, t1_start)
                                    } else {
                                        let half = canvas_w / 2.0;
                                        (bar_origin.x + half, bar_origin.x + half)
                                    };

                                    let adv_team = score_bar_info.as_ref().map(|s| s.adv_team).unwrap_or(-1);
                                    let x = if adv_team == 0 {
                                        t0_end_x + 6.0 * ws
                                    } else {
                                        t1_start_x - text_w - 6.0 * ws
                                    };
                                    let y = bar_origin.y + (bar_height - text_h) / 2.0;
                                    let label_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(text_w, text_h));
                                    let resp = ui.interact(
                                        label_rect,
                                        egui::Id::new("advantage_tooltip"),
                                        egui::Sense::hover(),
                                    );
                                    resp.on_hover_ui(|ui| {
                                        let bd = breakdown;
                                        let fmt_contrib = |val: (f32, f32)| -> String {
                                            let diff = val.0 - val.1;
                                            if diff > 0.0 {
                                                format!("+{:.1}", diff)
                                            } else if diff < 0.0 {
                                                format!("{:.1}", diff)
                                            } else {
                                                "0".to_string()
                                            }
                                        };
                                        let is_nonzero = |val: (f32, f32)| val.0 != 0.0 || val.1 != 0.0;
                                        ui.label(egui::RichText::new(t!("ui.renderer.advantage.breakdown").as_ref()).strong());
                                        ui.separator();
                                        if bd.team_eliminated {
                                            ui.label(t!("ui.renderer.advantage.team_eliminated"));
                                        } else {
                                            egui::Grid::new("adv_grid").num_columns(2).show(ui, |ui| {
                                                if is_nonzero(bd.score_projection) {
                                                    ui.label(t!("ui.renderer.advantage.score_projection"));
                                                    ui.label(fmt_contrib(bd.score_projection));
                                                    ui.end_row();
                                                }
                                                if is_nonzero(bd.fleet_power) {
                                                    ui.label(t!("ui.renderer.advantage.fleet_power"));
                                                    ui.label(fmt_contrib(bd.fleet_power));
                                                    ui.end_row();
                                                }
                                                if is_nonzero(bd.strategic_threat) {
                                                    ui.label(t!("ui.renderer.advantage.strategic_threat"));
                                                    ui.label(fmt_contrib(bd.strategic_threat));
                                                    ui.end_row();
                                                }
                                                ui.separator();
                                                ui.separator();
                                                ui.end_row();
                                                ui.label(egui::RichText::new(t!("ui.renderer.advantage.total").as_ref()).strong());
                                                ui.label(egui::RichText::new(fmt_contrib(bd.total)).strong());
                                                ui.end_row();
                                            });
                                            if !bd.hp_data_reliable {
                                                ui.small(t!("ui.renderer.advantage.hp_incomplete"));
                                            }
                                        }
                                    });
                                    break;
                                }
                            }
                        }
                        drop(state);

                        // ─── Render annotations on map ───────────────────────────
                        {
                            let ann_state = annotation_arc.lock();
                            let map_space = shared_state.lock().map_space_size;
                            for ann in &ann_state.annotations {
                                render_annotation(ann, &transform, textures, &map_painter, map_space);
                            }
                            // Draw selection highlight
                            for &sel in &ann_state.selected_indices {
                                if sel < ann_state.annotations.len() {
                                    render_selection_highlight(&ann_state.annotations[sel], &transform, &map_painter);
                                }
                            }
                            // Render tool preview (ghost shape at cursor)
                            if let Some(cursor_pos) = response.hover_pos() {
                                let minimap_pos = transform.screen_to_minimap(cursor_pos);
                                render_tool_preview(
                                    &ann_state.active_tool,
                                    minimap_pos,
                                    ann_state.paint_color,
                                    ann_state.stroke_width,
                                    &transform,
                                    textures,
                                    &map_painter,
                                    map_space,
                                );
                            }
                        }

                        // ─── Render remote cursors (collab session) ──────────
                        let collab_ss = shared_state.lock().collab_session_state.clone();
                        if let Some(ref ss_arc) = collab_ss {
                            let s = ss_arc.lock();
                            draw_remote_cursors(&s.cursors, s.my_user_id, &map_painter, &transform);

                            // ─── Render map pings (ripple effects) ──────────────
                            let ping_views: Vec<MapPing> = s.pings.iter().map(|p| MapPing {
                                pos: p.pos,
                                color: p.color,
                                time: p.time,
                            }).collect();
                            drop(s);
                            if draw_pings(&ping_views, &map_painter, &transform) {
                                repaint = true;
                                let mut s = ss_arc.lock();
                                s.pings.retain(|p| p.time.elapsed().as_secs_f32() < PING_DURATION);
                            }
                        }
                    }

                    // ─── Render local pings (when not in a collab session) ───
                    {
                        let mut lp = local_pings_arc.lock();
                        if !lp.is_empty()
                            && draw_pings(&lp, &map_painter, &transform) {
                                repaint = true;
                                let now = web_time::Instant::now();
                                lp.retain(|p| now.duration_since(p.time).as_secs_f32() < PING_DURATION);
                            }
                    }

                    // ─── Send local cursor to collab session ────────────────
                    {
                        let cursor_pos = response.hover_pos().map(|p| {
                            let mp = transform.screen_to_minimap(p);
                            [mp.x, mp.y]
                        });
                        if let Some(ref tx) = shared_state.lock().collab_local_tx {
                            let _ = tx.send(crate::collab::peer::LocalEvent::CursorPosition(cursor_pos));
                        }
                    }

                    drop(tex_guard);

                    // ─── Active tool indicator ───────────────────────────────────
                    {
                        let ann = annotation_arc.lock();
                        if let Some(label) = tool_label(&ann.active_tool) {
                            let text_pos = Pos2::new(response.rect.left() + 8.0, response.rect.top() + 8.0);
                            painter.text(
                                text_pos,
                                egui::Align2::LEFT_TOP,
                                format!("{} (right-click to cancel)", label),
                                FontId::proportional(13.0),
                                Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                            );
                        }
                    }

                    // ─── Annotation input handling ────────────────────────────────
                    {
                        let mut ann = annotation_arc.lock();
                        let tool_active = !matches!(ann.active_tool, PaintTool::None);

                        // When a tool is active, clear any selection
                        if tool_active {
                            ann.clear_selection();
                        }

                        // Right-click: open context menu or cancel tool
                        if response.secondary_clicked() {
                            if tool_active {
                                ann.active_tool = PaintTool::None;
                            } else {
                                let click_pos = response.interact_pointer_pos().unwrap_or(response.rect.center());
                                ann.context_menu_pos = click_pos;

                                // Detect nearest ship to right-click position
                                ann.context_menu_ship = None;
                                let state = shared_state.lock();
                                if let Some(ref frame) = state.frame {
                                    let mut best_dist = 30.0_f32; // max click distance in screen px
                                    for cmd in &frame.commands {
                                        if let DrawCommand::Ship { pos, entity_id, player_name: Some(name), .. } = cmd {
                                            let screen_pos = transform.minimap_to_screen(pos);
                                            let dist = click_pos.distance(screen_pos);
                                            if dist < best_dist {
                                                best_dist = dist;
                                                ann.context_menu_ship = Some((*entity_id, name.clone()));
                                            }
                                        }
                                    }
                                }
                                ann.show_context_menu = true;
                            }
                        }

                        // Tool shortcuts (Ctrl+1..7, Ctrl+M)
                        handle_tool_shortcuts(&ctx, &mut ann);

                        // Show shortcut overlay while Ctrl is held
                        draw_shortcut_overlay(&ctx, egui::Id::new("replay_shortcut_overlay"));

                        // Escape key: cancel tool or deselect
                        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                            if tool_active {
                                ann.active_tool = PaintTool::None;
                            } else {
                                ann.clear_selection();
                            }
                        }

                        // Delete/Backspace to delete selected annotations
                        if !tool_active
                            && ann.has_selection()
                            && ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
                        {
                            ann.save_undo();
                            let mut indices: Vec<usize> = ann.selected_indices.iter().copied().collect();
                            indices.sort_unstable_by(|a, b| b.cmp(a)); // remove from end first
                            let state = shared_state.lock();
                            for idx in indices {
                                if idx < ann.annotations.len() {
                                    let id = ann.annotation_ids[idx];
                                    ann.annotations.remove(idx);
                                    ann.annotation_ids.remove(idx);
                                    ann.annotation_owners.remove(idx);
                                    send_annotation_remove(&state.collab_local_tx, id, None);
                                }
                            }
                            drop(state);
                            ann.clear_selection();
                        }

                        // [ and ] to adjust stroke width when a tool is active
                        if tool_active {
                            if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                                ann.stroke_width = (ann.stroke_width - 1.0).clamp(1.0, 8.0);
                            }
                            if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                                ann.stroke_width = (ann.stroke_width + 1.0).clamp(1.0, 8.0);
                            }
                        }

                        // Left-click/drag tool actions
                        if tool_active {
                            // Drawing tools: PlacingShip, Freehand, Eraser, Line, Circle, Rect, Triangle
                            let result = handle_tool_interaction(&mut ann, &response, &transform);

                            if result.new_annotation.is_some() || result.erase_index.is_some() {
                                ann.save_undo();
                            }
                            if let Some(a) = result.new_annotation {
                                let id: u64 = rand::random();
                                let state = shared_state.lock();
                                let my_user_id = get_my_user_id(&state.collab_session_state);
                                ann.annotations.push(a);
                                ann.annotation_ids.push(id);
                                ann.annotation_owners.push(my_user_id);
                                send_annotation_update(&state.collab_local_tx, &ann, ann.annotations.len() - 1, None);
                            }
                            if let Some(idx) = result.erase_index {
                                let id = ann.annotation_ids[idx];
                                ann.annotations.remove(idx);
                                ann.annotation_ids.remove(idx);
                                ann.annotation_owners.remove(idx);
                                send_annotation_remove(&shared_state.lock().collab_local_tx, id, None);
                            }
                        } else {
                            // No tool active: select/move/rotate annotations
                            let sm = handle_annotation_select_move(&mut ann, &response, &transform);

                            // Sync to collab after rotation stopped or annotation moved
                            if let Some(idx) = sm.rotation_stopped_index {
                                send_annotation_update(&shared_state.lock().collab_local_tx, &ann, idx, None);
                            }
                            for &idx in &sm.moved_indices {
                                send_annotation_update(&shared_state.lock().collab_local_tx, &ann, idx, None);
                            }

                            // Click on empty space -> ping
                            if sm.selected_by_click && !ann.has_selection()
                                && let Some(click_pos) = response.hover_pos().map(|p| transform.screen_to_minimap(p)) {
                                    let state = shared_state.lock();
                                    handle_map_click_ping(
                                        click_pos,
                                        &mut local_pings_arc.lock(),
                                        &state.collab_session_state,
                                        &state.collab_local_tx,
                                    );
                                }
                        }

                        // Ctrl+Z to undo
                        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
                            ann.undo();
                            send_annotation_full_sync(&shared_state.lock().collab_command_tx, &ann, None);
                        }

                        if response.clicked() {
                            repaint = true;
                        }
                    }

                    // ─── Playback keyboard shortcuts ──────────────────────────────
                    {
                        // Space: toggle play/pause
                        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                            if playing {
                                let _ = command_tx.send(PlaybackCommand::Pause);
                                shared_state.lock().playing = false;
                            } else {
                                let _ = command_tx.send(PlaybackCommand::Play);
                                shared_state.lock().playing = true;
                            }
                        }

                        // Up/Down arrows: change playback speed
                        {
                            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                                let current = shared_state.lock().speed;
                                if let Some(&next) = PLAYBACK_SPEEDS.iter().find(|&&s| s > current + 0.1) {
                                    let _ = command_tx.send(PlaybackCommand::SetSpeed(next));
                                    shared_state.lock().speed = next;
                                    toasts.lock().info(format!("{:.0}x", next));
                                }
                            }
                            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                                let current = shared_state.lock().speed;
                                if let Some(&next) = PLAYBACK_SPEEDS.iter().rev().find(|&&s| s < current - 0.1) {
                                    let _ = command_tx.send(PlaybackCommand::SetSpeed(next));
                                    shared_state.lock().speed = next;
                                    toasts.lock().info(format!("{:.0}x", next));
                                }
                            }
                        }

                        if let Some((_frame_idx, _total_frames, clock_secs, game_dur)) = frame_data {
                            // Left/Right arrows: seek +/-10 seconds
                            let shift = ctx.input(|i| i.modifiers.shift);
                            if !shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                                let target = (clock_secs - 10.0).max(GameClock(0.0));
                                shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                let _ = command_tx.send(PlaybackCommand::Seek(target));
                            }
                            if !shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                                let target = (clock_secs + 10.0).min(GameClock(game_dur));
                                shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                let _ = command_tx.send(PlaybackCommand::Seek(target));
                            }

                            // Shift+Left/Right: skip to prev/next timeline event
                            let elapsed = clock_secs.to_elapsed(battle_start);
                            if shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                                let state = shared_state.lock();
                                if let Some(ref events) = state.timeline_events
                                    && let Some(event) = events.iter().rev().find(|e| e.clock < elapsed - 0.5) {
                                        let seek_clock = event.clock.to_absolute(battle_start);
                                        let desc = format_timeline_event(event);
                                        drop(state);
                                        shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                        let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                        toasts.lock().info(desc);
                                    }
                            }
                            if shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                                let state = shared_state.lock();
                                if let Some(ref events) = state.timeline_events
                                    && let Some(event) = events.iter().find(|e| e.clock > elapsed) {
                                        let seek_clock = event.clock.to_absolute(battle_start);
                                        let desc = format_timeline_event(event);
                                        drop(state);
                                        shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                        let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                        toasts.lock().info(desc);
                                    }
                            }
                        }
                    }

                    // ─── Context menu (egui::Area at cursor) ─────────────────────
                    {
                        let show_menu = annotation_arc.lock().show_context_menu;
                        if show_menu {
                            let menu_pos = annotation_arc.lock().context_menu_pos;
                            let menu_resp = egui::Area::new(ui.id().with("paint_context_menu"))
                                .order(egui::Order::Foreground)
                                .fixed_pos(menu_pos)
                                .interactable(true)
                                .show(&ctx, |ui| {
                                    let frame = egui::Frame::NONE
                                        .fill(Color32::from_gray(30))
                                        .corner_radius(CornerRadius::same(6))
                                        .inner_margin(egui::Margin::same(8))
                                        .stroke(Stroke::new(1.0, Color32::from_gray(80)));
                                    frame.show(ui, |ui| {
                                        ui.set_min_width(160.0);
                                        let mut ann = annotation_arc.lock();

                                        // ── Annotation tools ──
                                        let tex_guard = textures_arc.lock();
                                        let ship_icons_ref = tex_guard.as_ref().map(|t| &t.ship_icons);
                                        let menu_result = wt_collab_egui::toolbar::draw_annotation_menu_common(
                                            ui,
                                            &mut ann,
                                            ship_icons_ref,
                                        );
                                        drop(tex_guard);
                                        if menu_result.did_clear {
                                            send_annotation_clear(&shared_state.lock().collab_local_tx, None);
                                        }
                                        if menu_result.did_undo {
                                            send_annotation_full_sync(&shared_state.lock().collab_command_tx, &ann, None);
                                        }

                                        // ── Ship-specific options (shown when right-clicking a ship) ──
                                        if let Some((ship_eid, ref ship_name)) = ann.context_menu_ship.clone() {
                                            ui.separator();
                                            ui.label(egui::RichText::new(ship_name.as_str()).small());

                                            // Per-ship trail toggle
                                            let mut show_trail = !ann.trail_hidden_ships.contains(ship_name);
                                            if ui.checkbox(&mut show_trail, t!("ui.renderer.context.show_trail")).changed() {
                                                if show_trail {
                                                    ann.trail_hidden_ships.remove(ship_name);
                                                    // If global trails are off, turn them on
                                                    if !shared_state.lock().options.show_trails {
                                                        shared_state.lock().options.show_trails = true;
                                                    }
                                                } else {
                                                    ann.trail_hidden_ships.insert(ship_name.clone());
                                                    // If all trails are now hidden, turn off global
                                                    let state = shared_state.lock();
                                                    if let Some(ref frame) = state.frame {
                                                        let all_hidden = frame.commands.iter().all(|cmd| {
                                                            if let DrawCommand::PositionTrail {
                                                                player_name: Some(name),
                                                                ..
                                                            } = cmd
                                                            {
                                                                ann.trail_hidden_ships.contains(name)
                                                            } else {
                                                                true
                                                            }
                                                        });
                                                        if all_hidden && state.options.show_trails {
                                                            drop(state);
                                                            shared_state.lock().options.show_trails = false;
                                                        }
                                                    }
                                                }
                                                broadcast_trail_overrides(&ann.trail_hidden_ships, &shared_state);
                                                repaint = true;
                                            }

                                            // Disable all other trails
                                            if ui.button(t!("ui.renderer.context.disable_other_trails")).clicked() {
                                                let state = shared_state.lock();
                                                if let Some(ref frame) = state.frame {
                                                    for cmd in &frame.commands {
                                                        if let DrawCommand::PositionTrail {
                                                            player_name: Some(name),
                                                            ..
                                                        } = cmd
                                                            && name != ship_name {
                                                                ann.trail_hidden_ships.insert(name.clone());
                                                            }
                                                    }
                                                }
                                                ann.trail_hidden_ships.remove(ship_name);
                                                if !state.options.show_trails {
                                                    drop(state);
                                                    shared_state.lock().options.show_trails = true;
                                                }
                                                broadcast_trail_overrides(&ann.trail_hidden_ships, &shared_state);
                                                ann.show_context_menu = false;
                                                repaint = true;
                                            }

                                            // Per-type range toggles for this ship
                                            let mut filter =
                                                ann.ship_range_overrides.get(&ship_eid).copied().unwrap_or_default();
                                            ui.label(egui::RichText::new(t!("ui.renderer.context.ranges").as_ref()).small());
                                            let mut range_changed = false;
                                            range_changed |= ui.checkbox(&mut filter.detection, t!("ui.renderer.context.detection")).changed();
                                            range_changed |= ui.checkbox(&mut filter.main_battery, t!("ui.renderer.context.main_battery")).changed();
                                            range_changed |= ui.checkbox(&mut filter.secondary_battery, t!("ui.renderer.context.secondary")).changed();
                                            range_changed |= ui.checkbox(&mut filter.torpedo, t!("ui.renderer.context.torpedo")).changed();
                                            range_changed |= ui.checkbox(&mut filter.radar, t!("ui.renderer.context.radar")).changed();
                                            range_changed |= ui.checkbox(&mut filter.hydro, t!("ui.renderer.context.hydro")).changed();
                                            let all_on = filter == ShipConfigFilter::all_enabled();
                                            if !all_on && ui.button(t!("ui.renderer.context.enable_all")).clicked() {
                                                filter = ShipConfigFilter::all_enabled();
                                                range_changed = true;
                                            } else if all_on && ui.button(t!("ui.renderer.context.disable_all")).clicked() {
                                                filter = ShipConfigFilter::default();
                                                range_changed = true;
                                            }
                                            if range_changed {
                                                if !filter.any_enabled() {
                                                    ann.ship_range_overrides.remove(&ship_eid);
                                                } else {
                                                    ann.ship_range_overrides.insert(ship_eid, filter);
                                                }
                                                // Auto-enable global when turning on any range
                                                if filter.any_enabled() {
                                                    let mut state = shared_state.lock();
                                                    if !state.options.show_ship_config {
                                                        state.options.show_ship_config = true;
                                                    }
                                                }
                                                // Auto-disable global when no ship has any range enabled
                                                if ann.ship_range_overrides.is_empty() {
                                                    let mut state = shared_state.lock();
                                                    if state.options.show_ship_config {
                                                        state.options.show_ship_config = false;
                                                    }
                                                }
                                                broadcast_range_overrides(&ann.ship_range_overrides, &shared_state);
                                                repaint = true;
                                            }

                                            // Disable all other ships' ranges
                                            if ui.button(t!("ui.renderer.context.disable_other_ranges")).clicked() {
                                                let keys: Vec<EntityId> = ann
                                                    .ship_range_overrides
                                                    .keys()
                                                    .filter(|k| **k != ship_eid)
                                                    .copied()
                                                    .collect();
                                                for k in keys {
                                                    ann.ship_range_overrides.remove(&k);
                                                }
                                                broadcast_range_overrides(&ann.ship_range_overrides, &shared_state);
                                                ann.show_context_menu = false;
                                                repaint = true;
                                            }

                                            // Enable ranges for all alive ships
                                            if ui.button(t!("ui.renderer.context.enable_all_ranges")).clicked() {
                                                let state = shared_state.lock();
                                                if let Some(ref frame) = state.frame {
                                                    for cmd in &frame.commands {
                                                        if let DrawCommand::Ship { entity_id, .. } = cmd {
                                                            ann.ship_range_overrides
                                                                .insert(*entity_id, ShipConfigFilter::all_enabled());
                                                        }
                                                    }
                                                }
                                                if !state.options.show_ship_config {
                                                    drop(state);
                                                    shared_state.lock().options.show_ship_config = true;
                                                }
                                                broadcast_range_overrides(&ann.ship_range_overrides, &shared_state);
                                                ann.show_context_menu = false;
                                                repaint = true;
                                            }

                                            // ── Realtime Armor Viewer ──
                                            ui.separator();
                                            if ui.button(wt_translations::icon_t(icons::SHIELD, &t!("ui.renderer.context.show_armor"))).clicked() {
                                                let mut new_bridge = RealtimeArmorBridge::new(ship_eid);
                                                let mut state = shared_state.lock();
                                                // Attach pre-computed shot timeline if available
                                                if let Some(ref timelines) = state.shot_timelines {
                                                    new_bridge.shot_timeline = timelines.get(&ship_eid).cloned();
                                                }
                                                let bridge = Arc::new(Mutex::new(new_bridge));
                                                state.armor_bridges.push(bridge.clone());
                                                state.pending_armor_viewers.push(ArmorViewerRequest {
                                                    target_entity_id: ship_eid,
                                                    bridge,
                                                    command_tx: command_tx.clone(),
                                                });
                                                // Seek to current position to populate the new bridge
                                                let current_clock = state
                                                    .frame
                                                    .as_ref()
                                                    .map(|f| f.clock)
                                                    .unwrap_or(GameClock(0.0));
                                                drop(state);
                                                shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                                let _ = command_tx.send(PlaybackCommand::Seek(current_clock));
                                                ann.show_context_menu = false;
                                                repaint = true;
                                            }
                                        }
                                    });
                                });

                            // Close menu on click outside (but not if a sub-popup like color picker is open)
                            let menu_rect = menu_resp.response.rect;
                            let any_popup = ctx.any_popup_open();
                            let clicked_outside = !any_popup
                                && ctx.input(|i| {
                                    i.pointer.any_click()
                                        && i.pointer.interact_pos().is_some_and(|p| !menu_rect.contains(p))
                                });
                            if clicked_outside {
                                annotation_arc.lock().show_context_menu = false;
                            }
                        }
                    }

                    // ─── Selection edit popup ─────────────────────────────────────
                    {
                        let ann = annotation_arc.lock();
                        let sel_info = ann.single_selected().and_then(|idx| {
                            if idx < ann.annotations.len() {
                                let bounds = annotation_screen_bounds(&ann.annotations[idx], &transform);
                                Some((idx, bounds))
                            } else {
                                None
                            }
                        });
                        drop(ann);

                        if let Some((sel_idx, bounds)) = sel_info {
                            let collab_tx = shared_state.lock().collab_local_tx.clone();
                            let map_space = shared_state.lock().map_space_size;
                            draw_annotation_edit_popup(
                                &ctx,
                                ui.id().with("annotation_edit_popup"),
                                &annotation_arc,
                                sel_idx,
                                bounds,
                                map_space,
                                &collab_tx,
                                None,
                            );
                        }
                    }

                    // ─── Overlay controls (video-player style) ───────────────────

                    // Video export progress bar overlay
                    if video_exporting.load(Ordering::Relaxed) {
                        let progress_text = if let Some(p) = video_export_progress.lock().clone() {
                            let pct = if p.total > 0 { p.current as f32 / p.total as f32 } else { 0.0 };
                            let label = match p.stage {
                                RenderStage::Encoding => t!("ui.renderer.encoding"),
                                RenderStage::Muxing => t!("ui.renderer.muxing"),
                            };
                            Some((pct, format!("{} ({}/{})", label, p.current, p.total)))
                        } else {
                            None
                        };

                        egui::Area::new(ui.id().with("video_export_progress"))
                            .order(egui::Order::Foreground)
                            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 8.0))
                            .interactable(false)
                            .show(&ctx, |ui| {
                                egui::Frame::new()
                                    .fill(Color32::from_rgba_unmultiplied(30, 30, 30, 200))
                                    .corner_radius(CornerRadius::same(4))
                                    .inner_margin(egui::Margin::symmetric(12, 6))
                                    .show(ui, |ui| {
                                        ui.set_width(300.0);
                                        if let Some((pct, label)) = progress_text {
                                            ui.add(egui::ProgressBar::new(pct).text(label));
                                        } else {
                                            ui.horizontal(|ui| {
                                                ui.spinner();
                                                ui.label(t!("ui.renderer.preparing_export"));
                                            });
                                        }
                                    });
                            });
                        repaint = true;
                    }

                    // Track mouse activity for fade
                    let now = ctx.input(|i| i.time);
                    let any_mouse_activity =
                        ctx.input(|i| i.pointer.velocity().length() > 0.5 || i.pointer.any_click());
                    {
                        let mut ov = overlay_state_arc.lock();
                        if any_mouse_activity {
                            ov.last_activity = now;
                        }
                    }

                    // Check if any popup is open (keeps overlay visible, e.g. settings or speed)
                    let any_popup_open = egui::Popup::is_any_open(&ctx);

                    // Compute overlay opacity
                    let elapsed = now - overlay_state_arc.lock().last_activity;
                    let overlay_rect = ctx.memory(|mem| mem.area_rect(ui.id().with("controls_overlay")));
                    let hover_pos = ctx.input(|i| i.pointer.hover_pos());
                    let overlay_hovered = overlay_rect.is_some_and(|r| hover_pos.is_some_and(|p| r.contains(p)));
                    let opacity = if overlay_hovered || any_popup_open || elapsed < 2.0 {
                        1.0_f32
                    } else if elapsed < 2.5 {
                        (1.0 - ((elapsed - 2.0) / 0.5) as f32).max(0.0)
                    } else {
                        0.0
                    };

                    // Request repaint during fade animation
                    if opacity > 0.0 && opacity < 1.0 {
                        repaint = true;
                    }

                    // Only render overlay when visible (so it doesn't block canvas input when hidden)
                    if opacity > 0.0 {
                        let bg_alpha = (180.0 * opacity) as u8;
                        let text_alpha = (255.0 * opacity) as u8;

                        egui::Area::new(ui.id().with("controls_overlay"))
                            .order(egui::Order::Foreground)
                            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -8.0))
                            .interactable(true)
                            .show(&ctx, |ui| {
                                // Apply faded text color
                                ui.visuals_mut().override_text_color =
                                    Some(Color32::from_rgba_unmultiplied(255, 255, 255, text_alpha));
                                ui.visuals_mut().widgets.inactive.bg_fill =
                                    Color32::from_rgba_unmultiplied(60, 60, 60, bg_alpha);
                                ui.visuals_mut().widgets.hovered.bg_fill =
                                    Color32::from_rgba_unmultiplied(80, 80, 80, bg_alpha);

                                let frame = egui::Frame::NONE
                                    .fill(Color32::from_black_alpha(bg_alpha))
                                    .corner_radius(CornerRadius::same(6))
                                    .inner_margin(egui::Margin::same(8));
                                frame.show(ui, |ui| {
                                    let overlay_width = (response.rect.width() - 32.0).max(200.0);
                                    ui.set_width(overlay_width);

                                    // Prevent egui text layout from wrapping to minimal width
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                                    let row_style = taffy::Style {
                                        display: taffy::Display::Flex,
                                        flex_direction: taffy::FlexDirection::Row,
                                        align_items: Some(taffy::AlignItems::Center),
                                        gap: length(4.0),
                                        size: taffy::Size { width: taffy::prelude::percent(1.0), height: auto() },
                                        ..Default::default()
                                    };
                                    let grow_style = taffy::Style {
                                        flex_grow: 1.0,
                                        flex_shrink: 1.0,
                                        min_size: taffy::Size { width: length(60.0), height: auto() },
                                        ..Default::default()
                                    };
                                    let fixed_style = taffy::Style { flex_shrink: 0.0, ..Default::default() };

                                    let mut settings_btn_opt: Option<egui::Response> = None;
                                    let mut timeline_btn_opt: Option<egui::Response> = None;

                                    egui_taffy::tui(ui, ui.id().with("overlay_tui"))
                                        .reserve_available_width()
                                        .style(row_style)
                                        .show(|tui| {
                                            // Jump to start
                                            {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::SKIP_BACK));
                                                if btn.on_hover_text(t!("ui.renderer.controls.jump_to_start")).clicked() {
                                                    shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                                    let _ = command_tx.send(PlaybackCommand::Seek(GameClock(0.0)));
                                                }
                                            }

                                            // Skip to previous event
                                            if let Some((_fi, _tf, clock_secs, _gd)) = frame_data {
                                                let elapsed = clock_secs.to_elapsed(battle_start);
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::REWIND));
                                                if btn.on_hover_text(t!("ui.renderer.controls.previous_event")).clicked() {
                                                    let state = shared_state.lock();
                                                    if let Some(ref events) = state.timeline_events
                                                        && let Some(event) =
                                                            events.iter().rev().find(|e| e.clock < elapsed - 0.5)
                                                        {
                                                            let seek_clock = event.clock.to_absolute(battle_start);
                                                            let desc = format_timeline_event(event);
                                                            drop(state);
                                                            shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                                            let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                                            toasts.lock().info(desc);
                                                        }
                                                }
                                            }

                                            // Back 10 seconds
                                            if let Some((_fi, _tf, clock_secs, _gd)) = frame_data {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::CLOCK_COUNTER_CLOCKWISE));
                                                if btn.on_hover_text(t!("ui.renderer.controls.back_10s")).clicked() {
                                                    let target = (clock_secs - 10.0).max(GameClock(0.0));
                                                    shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                                    let _ = command_tx.send(PlaybackCommand::Seek(target));
                                                }
                                            }

                                            // Play/Pause
                                            if playing {
                                                if tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::PAUSE))
                                                    .on_hover_text(t!("ui.renderer.controls.pause"))
                                                    .clicked()
                                                {
                                                    let _ = command_tx.send(PlaybackCommand::Pause);
                                                    shared_state.lock().playing = false;
                                                }
                                            } else if tui
                                                .tui()
                                                .style(fixed_style.clone())
                                                .ui_add(egui::Button::new(icons::PLAY))
                                                .on_hover_text(t!("ui.renderer.controls.play"))
                                                .clicked()
                                            {
                                                let _ = command_tx.send(PlaybackCommand::Play);
                                                shared_state.lock().playing = true;
                                            }

                                            // Forward 10 seconds
                                            if let Some((_fi, _tf, clock_secs, game_dur)) = frame_data {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::CLOCK_CLOCKWISE));
                                                if btn.on_hover_text(t!("ui.renderer.controls.forward_10s")).clicked() {
                                                    let target = (clock_secs + 10.0).min(GameClock(game_dur));
                                                    shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                                    let _ = command_tx.send(PlaybackCommand::Seek(target));
                                                }
                                            }

                                            // Skip to next event
                                            if let Some((_fi, _tf, clock_secs, _gd)) = frame_data {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::FAST_FORWARD));
                                                if btn.on_hover_text(t!("ui.renderer.controls.next_event")).clicked() {
                                                    let elapsed = clock_secs.to_elapsed(battle_start);
                                                    let state = shared_state.lock();
                                                    if let Some(ref events) = state.timeline_events
                                                        && let Some(event) =
                                                            events.iter().find(|e| e.clock > elapsed)
                                                        {
                                                            let seek_clock = event.clock.to_absolute(battle_start);
                                                            let desc = format_timeline_event(event);
                                                            drop(state);
                                                            shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                                            let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                                            toasts.lock().info(desc);
                                                        }
                                                }
                                            }

                                            // Jump to end
                                            if let Some((_fi, _tf, _clock_secs, game_dur)) = frame_data {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::SKIP_FORWARD));
                                                if btn.on_hover_text(t!("ui.renderer.controls.jump_to_end")).clicked() {
                                                    shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                                    let _ = command_tx.send(PlaybackCommand::Seek(GameClock(game_dur)));
                                                }
                                            }

                                            // Seek slider (flex_grow: 1.0 — fills remaining space)
                                            if let Some((_frame_idx, _total_frames, clock_secs, game_dur)) = frame_data
                                            {
                                                let mut seek_time = clock_secs.seconds();
                                                let mut seek_changed = false;
                                                tui.tui().style(grow_style.clone()).ui(|ui| {
                                                    ui.spacing_mut().slider_width = ui.available_width();
                                                    let slider = egui::Slider::new(&mut seek_time, 0.0..=game_dur)
                                                        .show_value(false);
                                                    let resp = ui.add(slider);
                                                    seek_changed = resp.changed();

                                                    let rect = resp.rect;
                                                    // egui insets the slider thumb travel by its handle radius
                                                    // (default Rect handle: height/2.5 scaled by the 0.75 aspect).
                                                    let handle_r = rect.height() / 2.5 * 0.75;
                                                    // Fade the ticks with the rest of the controls.
                                                    let tick_color =
                                                        egui::Color32::from_rgba_unmultiplied(225, 225, 225, (255.0 * opacity) as u8);
                                                    let painter = ui.painter();
                                                    let start_x =
                                                        clock_tick_x(battle_start.seconds(), game_dur, rect, handle_r);
                                                    painter.vline(start_x, rect.y_range(), egui::Stroke::new(1.5, tick_color));
                                                    if let Some(end) = battle_end {
                                                        let end_x = clock_tick_x(end.seconds(), game_dur, rect, handle_r);
                                                        painter.vline(end_x, rect.y_range(), egui::Stroke::new(1.5, tick_color));
                                                    }
                                                });
                                                if seek_changed {
                                                    shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                                    let _ = command_tx.send(PlaybackCommand::Seek(GameClock(seek_time)));
                                                }

                                                let elapsed_secs = clock_secs.to_elapsed(battle_start).seconds().max(0.0) as u32;
                                                let mins = elapsed_secs / 60;
                                                let secs = elapsed_secs % 60;
                                                tui.tui()
                                                    .style(fixed_style.clone())
                                                    .label(format!("{:02}:{:02}", mins, secs));
                                            }

                                            // Speed selector
                                            let mut current_speed = speed;
                                            tui.tui().style(fixed_style.clone()).ui(|ui| {
                                                egui::ComboBox::from_id_salt("overlay_speed")
                                                    .selected_text(format!("{:.0}x", current_speed))
                                                    .width(60.0)
                                                    .show_ui(ui, |ui| {
                                                        for s in PLAYBACK_SPEEDS {
                                                            if ui
                                                                .selectable_value(
                                                                    &mut current_speed,
                                                                    s,
                                                                    format!("{:.0}x", s),
                                                                )
                                                                .changed()
                                                            {
                                                                let _ = command_tx.send(PlaybackCommand::SetSpeed(s));
                                                                shared_state.lock().speed = s;
                                                            }
                                                        }
                                                    });
                                            });

                                            tui.tui()
                                                .style(fixed_style.clone())
                                                .ui_add(egui_taffy::widgets::TaffySeparator::default());

                                            // Settings button
                                            let btn = tui.tui().style(fixed_style.clone()).ui_add(egui::Button::new(
                                                egui::RichText::new(icons::GEAR_FINE).size(18.0),
                                            ));
                                            settings_btn_opt = Some(btn);

                                            // Timeline button
                                            let btn = tui.tui().style(fixed_style.clone()).ui_add(egui::Button::new(
                                                egui::RichText::new(icons::LIST_BULLETS).size(18.0),
                                            ));
                                            timeline_btn_opt = Some(btn);
                                            // Save as Video / Clipboard buttons — hidden for client
                                            // viewers and while a collab session is active.
                                            let session_is_active = shared_state.lock().collab_session_state.as_ref()
                                                .map(|ss| matches!(ss.lock().status, SessionStatus::Active | SessionStatus::Starting))
                                                .unwrap_or(false);
                                            if !session_is_active
                                            && let Some(ref video_export_data) = video_export_data {
                                            {
                                                let is_exporting = video_exporting.load(Ordering::Relaxed);
                                                let has_warning = gpu_encoder_warning.lock().is_some();
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .enabled_ui(!is_exporting && !has_warning)
                                                    .ui_add(egui::Button::new(
                                                        egui::RichText::new(icons::FLOPPY_DISK).size(18.0),
                                                    ));
                                                if btn.on_hover_text(t!("ui.renderer.controls.save_as_video")).clicked() {
                                                    let mut opts = options.clone();
                                                    // Apply per-ship range overrides for video export
                                                    let overrides = annotation_arc.lock().ship_range_overrides.clone();
                                                    if !overrides.is_empty() {
                                                        // The exported video has no UI-side per-ship filter, so the
                                                        // overrides drive visibility directly and the config gate must
                                                        // be open for any circles to render.
                                                        opts.show_ship_config = true;
                                                        opts.ship_config_visibility = ShipConfigVisibility::Filtered(Arc::new(move |eid| {
                                                            overrides.get(&eid).copied()
                                                        }));
                                                    }
                                                    let default_name = format!("{}.mp4", video_export_data.replay_name);
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .set_file_name(&default_name)
                                                        .add_filter("MP4 Video", &["mp4"])
                                                        .save_file()
                                                    {
                                                        let status = cached_encoder_status(&ctx);
                                                        let codec_pref = *video_codec.lock();
                                                        let prefer_cpu = resolve_prefer_cpu(prefer_cpu_encoder.load(Ordering::Relaxed), codec_pref, &status);
                                                        let action = PendingVideoExport::SaveToFile {
                                                            output_path: path.to_string_lossy().to_string(),
                                                            options: opts,
                                                            prefer_cpu,
                                                            codec: codec_pref,
                                                            actual_game_duration,
                                                            encoder_config: wows_minimap_renderer::EncoderConfig::default(),
                                                            include_pre_battle: include_pre_battle.load(Ordering::Relaxed),
                                                        };
                                                        if prefer_cpu || status.gpu_available() || suppress_gpu_warning.load(Ordering::Relaxed) {
                                                            execute_video_export(action, video_export_data, &toasts, &video_exporting, &video_export_progress);
                                                        } else {
                                                            *gpu_encoder_warning.lock() = Some(GpuEncoderWarning {
                                                                pending_action: action,
                                                                dont_show_again: false,
                                                            });
                                                        }
                                                    }
                                                }
                                            }

                                            // Render Video to Clipboard button
                                            {
                                                let is_exporting = video_exporting.load(Ordering::Relaxed);
                                                let has_warning = gpu_encoder_warning.lock().is_some();
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .enabled_ui(!is_exporting && !has_warning)
                                                    .ui_add(egui::Button::new(
                                                        egui::RichText::new(icons::CLIPBOARD).size(18.0),
                                                    ));
                                                if btn.on_hover_text(t!("ui.renderer.controls.copy_to_clipboard")).clicked() {
                                                    let mut opts = options.clone();
                                                    // Apply per-ship range overrides for video export
                                                    let overrides = annotation_arc.lock().ship_range_overrides.clone();
                                                    if !overrides.is_empty() {
                                                        // The exported video has no UI-side per-ship filter, so the
                                                        // overrides drive visibility directly and the config gate must
                                                        // be open for any circles to render.
                                                        opts.show_ship_config = true;
                                                        opts.ship_config_visibility = ShipConfigVisibility::Filtered(Arc::new(move |eid| {
                                                            overrides.get(&eid).copied()
                                                        }));
                                                    }
                                                    let status = wows_minimap_renderer::check_encoder();
                                                    let codec_pref = *video_codec.lock();
                                                    let prefer_cpu = resolve_prefer_cpu(prefer_cpu_encoder.load(Ordering::Relaxed), codec_pref, &status);
                                                    let action = PendingVideoExport::CopyToClipboard {
                                                        options: opts,
                                                        prefer_cpu,
                                                        codec: codec_pref,
                                                        actual_game_duration,
                                                        encoder_config: wows_minimap_renderer::EncoderConfig::default(),
                                                        include_pre_battle: include_pre_battle.load(Ordering::Relaxed),
                                                    };
                                                    if prefer_cpu || status.gpu_available() || suppress_gpu_warning.load(Ordering::Relaxed) {
                                                        execute_video_export(action, video_export_data, &toasts, &video_exporting, &video_export_progress);
                                                    } else {
                                                        *gpu_encoder_warning.lock() = Some(GpuEncoderWarning {
                                                            pending_action: action,
                                                            dont_show_again: false,
                                                        });
                                                    }
                                                }
                                            }
                                            } // end if video_export_data
                                             // end if !session_is_active

                                            tui.tui()
                                                .style(fixed_style.clone())
                                                .ui_add(egui_taffy::widgets::TaffySeparator::default());

                                            // Zoom slider
                                            {
                                                tui.tui().style(fixed_style.clone()).label(icons::MAGNIFYING_GLASS);

                                                let mut zp = zoom_pan_arc.lock();
                                                let mut zoom_val = zp.zoom;
                                                let slider = egui::Slider::new(&mut zoom_val, 1.0..=10.0_f32)
                                                    .logarithmic(true)
                                                    .max_decimals(1)
                                                    .suffix("x");
                                                if tui.tui().style(fixed_style.clone()).ui_add(slider).changed() {
                                                    let old_zoom = zp.zoom;
                                                    let center_x = zp.pan.x + MINIMAP_SIZE as f32 / 2.0;
                                                    let center_y = zp.pan.y + MINIMAP_SIZE as f32 / 2.0;
                                                    let minimap_cx = center_x / old_zoom;
                                                    let minimap_cy = center_y / old_zoom;
                                                    zp.pan.x = minimap_cx * zoom_val - MINIMAP_SIZE as f32 / 2.0;
                                                    zp.pan.y = minimap_cy * zoom_val - MINIMAP_SIZE as f32 / 2.0;
                                                    zp.zoom = zoom_val;
                                                    zp.pan.x = zp.pan.x.max(0.0);
                                                    zp.pan.y = zp.pan.y.max(0.0);
                                                }
                                                if tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(t!("ui.buttons.reset")))
                                                    .clicked()
                                                {
                                                    zp.zoom = 1.0;
                                                    zp.pan = Vec2::ZERO;
                                                }
                                            }

                                        });

                                    let settings_btn = settings_btn_opt.unwrap();

                                    // Settings popup (inside frame, outside horizontal)
                                    egui::Popup::from_toggle_button_response(&settings_btn)
                                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                        .show(|ui| {
                                            ui.set_min_width(180.0);
                                            let scroll_out = egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(ui, |ui| {
                                            let mut opts = options.clone();
                                            let mut show_dead = show_dead_ships;
                                            let mut changed = false;

                                            // ── Ship Settings ──
                                            ui.label(egui::RichText::new(t!("ui.renderer.settings.ship_settings").as_ref()).small().strong());
                                            ui.indent("ship_settings", |ui| {
                                                changed |= ui.checkbox(&mut opts.show_armament, t!("ui.renderer.settings.armament")).changed();
                                                changed |= ui.checkbox(&mut show_dead, t!("ui.renderer.settings.dead_ships")).changed();
                                                changed |= ui
                                                    .checkbox(&mut opts.show_dead_ship_names, t!("ui.renderer.settings.dead_ship_names"))
                                                    .changed();
                                                changed |= ui.checkbox(&mut opts.show_hp_bars, "HP Bars").changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_player_names, t!("ui.renderer.settings.player_names")).changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_ship_config, t!("ui.renderer.settings.ship_ranges")).changed();

                                                // Self ship range toggles
                                                if let Some(self_eid) = self_entity_id {
                                                    let mut filter = annotation_arc.lock().ship_range_overrides
                                                        .get(&self_eid).copied().unwrap_or_default();
                                                    let mut self_changed = false;
                                                    ui.indent("self_ranges", |ui| {
                                                        ui.label(egui::RichText::new(t!("ui.renderer.settings.self_ship_ranges").as_ref()).small());
                                                        self_changed |= ui.checkbox(&mut filter.detection, t!("ui.renderer.context.detection")).changed();
                                                        self_changed |= ui.checkbox(&mut filter.main_battery, t!("ui.renderer.context.main_battery")).changed();
                                                        self_changed |= ui.checkbox(&mut filter.secondary_battery, t!("ui.renderer.context.secondary")).changed();
                                                        self_changed |= ui.checkbox(&mut filter.torpedo, t!("ui.renderer.context.torpedo")).changed();
                                                        self_changed |= ui.checkbox(&mut filter.radar, t!("ui.renderer.context.radar")).changed();
                                                        self_changed |= ui.checkbox(&mut filter.hydro, t!("ui.renderer.context.hydro")).changed();
                                                    });
                                                    if self_changed {
                                                        let mut ann = annotation_arc.lock();
                                                        if !filter.any_enabled() {
                                                            ann.ship_range_overrides.remove(&self_eid);
                                                        } else {
                                                            ann.ship_range_overrides.insert(self_eid, filter);
                                                        }
                                                        // Auto-enable global show_ship_config when any range is on
                                                        if filter.any_enabled() && !opts.show_ship_config {
                                                            opts.show_ship_config = true;
                                                            changed = true;
                                                        }
                                                        // Auto-disable global when no ship has any range enabled
                                                        if ann.ship_range_overrides.is_empty() && opts.show_ship_config {
                                                            opts.show_ship_config = false;
                                                            changed = true;
                                                        }
                                                        broadcast_range_overrides(&ann.ship_range_overrides, &shared_state);

                                                        repaint = true;
                                                    }
                                                }

                                                changed |=
                                                    ui.checkbox(&mut opts.show_ship_names, t!("ui.renderer.settings.ship_names")).changed();
                                                changed |= ui
                                                    .checkbox(&mut opts.show_camera_direction, t!("ui.renderer.settings.camera_direction"))
                                                    .changed();
                                            });

                                            // ── Trail Settings ──
                                            ui.label(egui::RichText::new(t!("ui.renderer.settings.trail_settings").as_ref()).small().strong());
                                            ui.indent("trail_settings", |ui| {
                                                changed |= ui.checkbox(&mut opts.show_trails, t!("ui.renderer.settings.heat_trail")).changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_speed_trails, t!("ui.renderer.settings.speed_trails")).changed();
                                                changed |= ui
                                                    .checkbox(&mut opts.show_dead_trails, t!("ui.renderer.settings.dead_ship_trails"))
                                                    .changed();
                                            });

                                            // ── Map Settings ──
                                            ui.label(egui::RichText::new(t!("ui.renderer.settings.map_settings").as_ref()).small().strong());
                                            ui.indent("map_settings", |ui| {
                                                changed |= ui.checkbox(&mut opts.show_buildings, t!("ui.renderer.settings.buildings")).changed();
                                                changed |= ui
                                                    .checkbox(&mut opts.show_capture_points, t!("ui.renderer.settings.capture_points"))
                                                    .changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_consumables, t!("ui.renderer.settings.consumables")).changed();
                                                changed |= ui.checkbox(&mut opts.show_planes, t!("ui.renderer.settings.planes")).changed();
                                                changed |= ui.checkbox(&mut opts.show_smoke, t!("ui.renderer.settings.smoke")).changed();
                                                changed |= ui.checkbox(&mut opts.show_torpedoes, t!("ui.renderer.settings.torpedoes")).changed();
                                                changed |= ui.checkbox(&mut opts.show_tracers, t!("ui.renderer.settings.tracers")).changed();
                                            });

                                            // ── HUD Settings ──
                                            ui.label(egui::RichText::new(t!("ui.renderer.settings.hud_settings").as_ref()).small().strong());
                                            ui.indent("hud_settings", |ui| {
                                                changed |= ui
                                                    .checkbox(&mut opts.show_battle_result, t!("ui.renderer.settings.battle_result"))
                                                    .changed();
                                                changed |= ui.checkbox(&mut opts.show_buffs, t!("ui.renderer.settings.buff_counters")).changed();
                                                changed |= ui.checkbox(&mut opts.show_chat, t!("ui.renderer.settings.chat_label")).changed();
                                                changed |= ui.checkbox(&mut opts.show_kill_feed, t!("ui.renderer.settings.kill_feed")).changed();
                                                changed |= ui.checkbox(&mut opts.show_score, t!("ui.renderer.settings.score_label")).changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_score_timer, t!("ui.renderer.settings.score_timers")).changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_advantage, t!("ui.renderer.settings.team_advantage")).changed();
                                                changed |= ui.checkbox(&mut opts.show_timer, t!("ui.renderer.settings.timer")).changed();
                                                // Stats panel and team rosters compete for the same gutter
                                                // real estate, so they're mutually exclusive: toggling either
                                                // on clears the other.
                                                if ui.checkbox(&mut opts.show_stats_panel, "Stats Panel").changed() {
                                                    if opts.show_stats_panel {
                                                        opts.show_team_rosters = false;
                                                    }
                                                    changed = true;
                                                }
                                                if ui.checkbox(&mut opts.show_team_rosters, "Team Rosters").changed() {
                                                    if opts.show_team_rosters {
                                                        opts.show_stats_panel = false;
                                                    }
                                                    changed = true;
                                                }
                                            });

                                            // ── Export Settings ──
                                            ui.label(egui::RichText::new(t!("ui.renderer.settings.export_settings").as_ref()).small().strong());
                                            ui.indent("export_settings", |ui| {
                                                let mut cpu = prefer_cpu_encoder.load(Ordering::Relaxed);
                                                if ui.checkbox(&mut cpu, t!("ui.renderer.settings.prefer_cpu")).on_hover_text(t!("ui.renderer.settings.prefer_cpu_tooltip")).changed() {
                                                    prefer_cpu_encoder.store(cpu, Ordering::Relaxed);
                                                }
                                                let mut pre_battle = include_pre_battle.load(Ordering::Relaxed);
                                                if ui.checkbox(&mut pre_battle, t!("ui.renderer.settings.include_pre_battle")).on_hover_text(t!("ui.renderer.settings.include_pre_battle_tooltip")).changed() {
                                                    include_pre_battle.store(pre_battle, Ordering::Relaxed);
                                                }
                                                let status = cached_encoder_status(ui.ctx());
                                                let default_codec = status.best_codec(cpu);
                                                let mut current = *video_codec.lock();
                                                ui.label(t!("ui.renderer.settings.codec"));
                                                ui.indent("codec_choice", |ui| {
                                                    let auto_label = format!(
                                                        "{} ({})",
                                                        t!("ui.renderer.settings.codec_auto"),
                                                        default_codec.display_name(),
                                                    );
                                                    if ui.selectable_label(current.is_none(), auto_label).clicked() {
                                                        current = None;
                                                    }
                                                    for codec in wows_minimap_renderer::VideoCodec::ALL {
                                                        if !status.supported_codecs().any(|c| c == codec) {
                                                            continue;
                                                        }
                                                        if ui.selectable_label(current == Some(codec), codec.display_name()).clicked() {
                                                            current = Some(codec);
                                                        }
                                                    }
                                                });
                                                *video_codec.lock() = current;
                                            });

                                            if changed {
                                                let mut state = shared_state.lock();
                                                // Broadcast diffs to collab peers if connected.
                                                if let Some(ref tx) = state.collab_local_tx {
                                                    use crate::collab::protocol::collab_render_options_from_render_options;
                                                    let old = collab_render_options_from_render_options(&state.options, state.show_dead_ships);
                                                    let new = collab_render_options_from_render_options(&opts, show_dead);
                                                    for (field, value) in old.diff(&new) {
                                                        let _ = tx.send(crate::collab::peer::LocalEvent::DisplayToggle(field, value));
                                                    }
                                                }
                                                state.options = opts.clone();
                                                state.show_dead_ships = show_dead;
                                            }
                                            (opts, show_dead)
                                            });

                                            let (opts, show_dead) = scroll_out.inner;
                                            ui.separator();
                                            if ui.button(t!("ui.renderer.settings.save_defaults")).clicked() {
                                                let mut saved = saved_from_render_options(&opts);
                                                saved.show_dead_ships = show_dead;
                                                saved.prefer_cpu_encoder = prefer_cpu_encoder.load(Ordering::Relaxed);
                                                saved.video_codec = *video_codec.lock();
                                                saved.include_pre_battle = include_pre_battle.load(Ordering::Relaxed);
                                                // Persist self range flags from annotation overrides
                                                if let Some(self_eid) = self_entity_id {
                                                    let ann = annotation_arc.lock();
                                                    let filter = ann.ship_range_overrides
                                                        .get(&self_eid).copied().unwrap_or_default();
                                                    saved.set_self_range_filter(&filter);
                                                }
                                                *pending_save.lock() = Some(saved);
                                            }
                                        });

                                    // Timeline popup
                                    let timeline_btn = timeline_btn_opt.unwrap();
                                    egui::Popup::from_toggle_button_response(&timeline_btn)
                                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                        .frame(
                                            egui::Frame::popup(ui.style())
                                                .fill(ui.style().visuals.window_fill.gamma_multiply(0.5)),
                                        )
                                        .show(|ui| {
                                            ui.set_width(280.0);
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new(t!("ui.renderer.settings.event_timeline").as_ref()).strong());
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(egui::Align::Center),
                                                    |ui| {
                                                        let state = shared_state.lock();
                                                        if let Some(events) = &state.timeline_events
                                                            && ui.small_button(t!("ui.buttons.copy")).clicked() {
                                                                let text: String = events
                                                                    .iter()
                                                                    .map(format_timeline_event)
                                                                    .collect::<Vec<_>>()
                                                                    .join("\n");
                                                                ui.ctx().copy_text(text);
                                                            }
                                                    },
                                                );
                                            });
                                            ui.separator();

                                            let state = shared_state.lock();
                                            if let Some(events) = &state.timeline_events {
                                                if events.is_empty() {
                                                    ui.label("No significant events detected.");
                                                } else {
                                                    egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                                                        ui.set_width(ui.available_width());
                                                        for event in events {
                                                            let mins = event.clock.seconds() as u32 / 60;
                                                            let secs = event.clock.seconds() as u32 % 60;
                                                            let timestamp = format!("{:02}:{:02}", mins, secs);

                                                            let row = ui.horizontal(|ui| {
                                                                let mut clicked = ui.small_button(&timestamp).clicked();

                                                                let (color, text, hover) = match &event.kind {
                                                                    TimelineEventKind::HealthLost {
                                                                        ship_name, player_name, is_friendly,
                                                                        percent_lost, old_hp, new_hp, max_hp,
                                                                    } => {
                                                                        let pct = (percent_lost * 100.0) as u32;
                                                                        (
                                                                            event_color(*is_friendly),
                                                                            format!("{} -{}% HP", ship_name, pct),
                                                                            format!("{} ({})\n{:.0}/{:.0} -> {:.0}/{:.0} HP",
                                                                                ship_name, player_name, old_hp, max_hp, new_hp, max_hp),
                                                                        )
                                                                    }
                                                                    TimelineEventKind::Death {
                                                                        ship_name, player_name, is_friendly,
                                                                        killer_ship, killer_player,
                                                                    } => {
                                                                        let hover = if killer_ship.is_empty() {
                                                                            format!("{} ({})", ship_name, player_name)
                                                                        } else {
                                                                            format!("{} ({})\nKilled by {} ({})",
                                                                                ship_name, player_name, killer_ship, killer_player)
                                                                        };
                                                                        (
                                                                            event_color(*is_friendly),
                                                                            format!("{} destroyed", ship_name),
                                                                            hover,
                                                                        )
                                                                    }
                                                                    TimelineEventKind::CapContested {
                                                                        cap_label, owner_is_friendly,
                                                                    } => (
                                                                        event_color(*owner_is_friendly),
                                                                        format!("{} contested", cap_label),
                                                                        String::new(),
                                                                    ),
                                                                    TimelineEventKind::CapFlipped {
                                                                        cap_label, capturer_is_friendly,
                                                                    } => (
                                                                        event_color(*capturer_is_friendly),
                                                                        format!("{} captured", cap_label),
                                                                        String::new(),
                                                                    ),
                                                                    TimelineEventKind::CapBeingCaptured {
                                                                        cap_label, capturer_is_friendly,
                                                                    } => (
                                                                        event_color(*capturer_is_friendly),
                                                                        format!("{} being captured", cap_label),
                                                                        String::new(),
                                                                    ),
                                                                    TimelineEventKind::RadarUsed {
                                                                        ship_name, player_name, is_friendly,
                                                                    } => (
                                                                        event_color(*is_friendly),
                                                                        format!("{} used radar", ship_name),
                                                                        format!("{} ({})", ship_name, player_name),
                                                                    ),
                                                                    TimelineEventKind::AdvantageChanged {
                                                                        label, is_friendly,
                                                                    } => (event_color(*is_friendly), label.clone(), String::new()),
                                                                    TimelineEventKind::Disconnected {
                                                                        ship_name, player_name, is_friendly,
                                                                    } => (
                                                                        event_color(*is_friendly),
                                                                        format!("{} disconnected", ship_name),
                                                                        format!("{} ({})", ship_name, player_name),
                                                                    ),
                                                                };

                                                                let label_resp = ui.add(
                                                                    egui::Label::new(
                                                                        egui::RichText::new(text).color(color),
                                                                    )
                                                                    .selectable(false)
                                                                    .sense(egui::Sense::click()),
                                                                );
                                                                if !hover.is_empty() {
                                                                    label_resp.clone().on_hover_text(&hover);
                                                                }
                                                                if label_resp.hovered() {
                                                                    ui.ctx().set_cursor_icon(
                                                                        egui::CursorIcon::PointingHand,
                                                                    );
                                                                }
                                                                clicked |= label_resp.clicked();
                                                                clicked
                                                            });
                                                            if row.inner {
                                                                shared_state.lock().cancel_step.store(true, Ordering::Relaxed);
                                                                let _ =
                                                                    command_tx.send(PlaybackCommand::Seek(event.clock.to_absolute(battle_start)));
                                                            }
                                                        }
                                                    });
                                                }
                                            } else {
                                                ui.spinner();
                                                ui.label("Parsing events...");
                                            }
                                        });

                                });
                            });
                    }

                    // GPU encoder warning dialog
                    if gpu_encoder_warning.lock().is_some() {
                        let mut close_dialog = false;
                        let mut proceed = false;

                        egui::Window::new("GPU Video Encoder Unavailable")
                            .collapsible(false)
                            .resizable(false)
                            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                            .show(&ctx, |ui| {
                                ui.label(
                                    "Could not find a supported GPU video encoder. \
                                     Video export will fall back to CPU encoding, \
                                     which will be significantly slower."
                                );
                                ui.add_space(8.0);
                                let mut warning = gpu_encoder_warning.lock();
                                if let Some(w) = warning.as_mut() {
                                    ui.checkbox(&mut w.dont_show_again, "Don't show this again");
                                }
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if ui.button("Ok").clicked() {
                                        proceed = true;
                                        close_dialog = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        close_dialog = true;
                                    }
                                });
                            });

                        if close_dialog {
                            let warning = gpu_encoder_warning.lock().take();
                            if let Some(w) = warning {
                                if w.dont_show_again {
                                    suppress_gpu_warning.store(true, Ordering::Relaxed);
                                }
                                if proceed
                                    && let Some(ref video_export_data) = video_export_data {
                                        execute_video_export(w.pending_action, video_export_data, &toasts, &video_exporting, &video_export_progress);
                                    }
                            }
                        }
                    }

                    toasts.lock().show(&ctx);
                });


                if ctx.input(|i| i.viewport().close_requested()) {
                    // Capture window geometry before closing.
                    {
                        let info = ctx.input(|i| i.viewport().clone());
                        window_settings.lock().settings.insert(
                            crate::tab_state::WindowKind::ReplayRenderer,
                            crate::tab_state::WindowSettings::from_viewport_info(&info, None),
                        );
                        save_notify.notify_one();
                    }
                    window_open.store(false, Ordering::Relaxed);
                    let _ = command_tx.send(PlaybackCommand::Stop);
                    // Unregister viewport sink.
                    let state = shared_state.lock();
                    if let Some(ref session_state) = state.collab_session_state
                        && let Some(replay_id) = state.collab_replay_id {
                            session_state.lock().viewport_sinks.remove(&replay_id);
                        }
                    drop(state);
                    ctx.request_repaint();
                } else if status_is_loading {
                    // Keep the viewport alive while loading so it notices the
                    // Loading->Ready transition. Only repaint this viewport —
                    // do NOT wake the parent, which causes event-loop starvation.
                    ctx.request_repaint();
                } else if playing || repaint {
                    // Repaint both this viewport AND the parent so sibling
                    // viewports (e.g. armor viewer) also update in realtime.
                    ctx.request_repaint();
                    parent_ctx.request_repaint();
                }
            },
        );
    }
}

fn roster_consumable_tooltip(
    ui: &mut egui::Ui,
    region: &wt_collab_egui::draw_commands::ConsumableHoverRegion,
    icon: Option<&TextureHandle>,
) {
    use wows_minimap_renderer::draw_command::ChargeCount;

    ui.set_max_width(280.0);
    ui.horizontal(|ui| {
        if let Some(tex) = icon {
            ui.image((tex.id(), Vec2::splat(48.0)));
        }
        ui.vertical(|ui| {
            let name =
                if region.display_name.is_empty() { region.icon_key.clone() } else { region.display_name.clone() };
            ui.label(egui::RichText::new(name).strong());
            let charges_line = match region.total_charges {
                ChargeCount::Unlimited => "Charges: inf".to_string(),
                ChargeCount::Finite(total) => {
                    let remaining = total.saturating_sub(region.charges_used);
                    format!("Charges: {} / {}", remaining, total)
                }
            };
            ui.label(charges_line);
        });
    });
    if region.work_time_secs > 0.0 || region.reload_time_secs > 0.0 {
        ui.label(format!("Duration: {:.0}s   Cooldown: {:.0}s", region.work_time_secs, region.reload_time_secs));
    }
    if let Some(remaining) = region.active_remaining_secs {
        ui.label(format!("Active: {:.0}s remaining", remaining));
    }
    if !region.description.is_empty() {
        ui.separator();
        ui.label(&region.description);
    }
}

fn roster_player_build_tooltip(
    ui: &mut egui::Ui,
    region: &wt_collab_egui::draw_commands::PlayerBuildHoverRegion,
    display: Option<&Arc<PlayerBuildDisplay>>,
    textures: &RendererTextures,
) {
    ui.set_max_width(440.0);

    let header = match &region.clan_tag {
        Some(tag) => format!("[{tag}] {}    {}", region.player_name, region.ship_name),
        None => format!("{}    {}", region.player_name, region.ship_name),
    };
    ui.label(egui::RichText::new(header).strong());

    let Some(display) = display else {
        ui.separator();
        ui.weak("Build data not available for this player.");
        return;
    };

    if let Some(name) = display.captain_name.as_deref() {
        ui.label(egui::RichText::new(format!("Captain: {name}")).italics());
    }

    ui.separator();
    draw_skill_grid(ui, &display.skill_rows, &textures.crew_skill_icons);
    draw_build_section(ui, "Upgrades", &display.upgrades, &textures.modernization_icons);
    draw_build_section(ui, "Signals", &display.signals, &textures.signal_flag_icons);
}

/// Render learned captain skills as a tier grid: one row per tier, point
/// cost labels the row at the left. Each icon carries a hover tooltip
/// with the skill's name, point cost, and description.
fn draw_skill_grid(ui: &mut egui::Ui, rows: &[SkillRow], icons: &HashMap<String, TextureHandle>) {
    if rows.is_empty() {
        return;
    }
    ui.label(egui::RichText::new("Skills").strong().small());
    const ICON_SIZE: f32 = 32.0;
    for row in rows {
        ui.horizontal(|ui| {
            ui.add_sized(
                [14.0, ICON_SIZE],
                egui::Label::new(
                    egui::RichText::new(row.tier.map(|t| t.to_string()).unwrap_or_default()).weak().small(),
                ),
            );
            for skill in &row.skills {
                let Some(tex) = icons.get(&skill.icon_key) else {
                    tracing::warn!(icon_key = %skill.icon_key, "missing skill icon");
                    continue;
                };
                let cost = skill.tier.map(|t| format!(" ({t} pt)")).unwrap_or_default();
                let tooltip = if skill.description.is_empty() {
                    format!("{}{}", skill.name, cost)
                } else {
                    format!("{}{}\n\n{}", skill.name, cost, skill.description)
                };
                // Dim skills the player didn't take so the taken ones stand out.
                let tint =
                    if skill.learned { Color32::WHITE } else { Color32::from_rgba_unmultiplied(255, 255, 255, 55) };
                ui.add(egui::Image::new((tex.id(), Vec2::splat(ICON_SIZE))).tint(tint)).on_hover_text(tooltip);
            }
        });
    }
}

/// Render a labeled wrapping row of equipment icons (upgrades or signals).
/// Each icon carries a name/description hover tooltip; missing icons fall
/// back to a text label so the section still reads correctly.
fn draw_build_section(
    ui: &mut egui::Ui,
    title: &str,
    items: &[EquipmentDisplay],
    icons: &HashMap<String, TextureHandle>,
) {
    if items.is_empty() {
        return;
    }
    ui.label(egui::RichText::new(title).strong().small());
    ui.horizontal_wrapped(|ui| {
        for item in items {
            let Some(tex) = icons.get(&item.icon_key) else {
                tracing::warn!(icon_key = %item.icon_key, section = %title, "missing equipment icon");
                continue;
            };
            let tooltip = if item.description.is_empty() {
                item.name.clone()
            } else {
                format!("{}\n\n{}", item.name, item.description)
            };
            ui.image((tex.id(), Vec2::splat(28.0))).on_hover_text(tooltip);
        }
    });
}

/// Map an absolute clock value (seconds) to an x pixel on a seek-slider rect,
/// inset from both ends by the slider handle radius so the tick lines up with the
/// value position rather than the widget edge. Clamps to [0, 1] of the duration.
fn clock_tick_x(clock_secs: f32, game_dur: f32, rect: egui::Rect, handle_radius: f32) -> f32 {
    let frac = if game_dur > 0.0 { (clock_secs / game_dur).clamp(0.0, 1.0) } else { 0.0 };
    let left = rect.left() + handle_radius;
    let right = rect.right() - handle_radius;
    left + frac * (right - left)
}

#[cfg(test)]
mod tick_tests {
    use super::clock_tick_x;
    use egui::Pos2;
    use egui::Rect;

    fn rect() -> Rect {
        Rect::from_min_max(Pos2::new(100.0, 0.0), Pos2::new(300.0, 10.0))
    }

    #[test]
    fn maps_fraction_with_handle_inset() {
        assert!((clock_tick_x(0.0, 100.0, rect(), 0.0) - 100.0).abs() < 0.01);
        assert!((clock_tick_x(100.0, 100.0, rect(), 0.0) - 300.0).abs() < 0.01);
        assert!((clock_tick_x(50.0, 100.0, rect(), 0.0) - 200.0).abs() < 0.01);
    }

    #[test]
    fn clamps_and_handles_zero_duration() {
        assert!((clock_tick_x(150.0, 100.0, rect(), 0.0) - 300.0).abs() < 0.01);
        assert!((clock_tick_x(-5.0, 100.0, rect(), 0.0) - 100.0).abs() < 0.01);
        assert!((clock_tick_x(10.0, 0.0, rect(), 0.0) - 100.0).abs() < 0.01);
    }

    #[test]
    fn applies_handle_radius_inset() {
        assert!((clock_tick_x(0.0, 100.0, rect(), 10.0) - 110.0).abs() < 0.01);
        assert!((clock_tick_x(100.0, 100.0, rect(), 10.0) - 290.0).abs() < 0.01);
    }
}
