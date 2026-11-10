//! What the TSX interface reads: the whole viewer as one object, and the
//! pointer tooltip on a channel of its own.

use std::collections::HashSet;

use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use rust_i18n::t;
use wows_replays::analyzer::decoder::HitType;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_armor::scene::ArcSummary;
use wows_toolkit_armor::scene::ArmorScene;
use wows_toolkit_armor::scene::CAST_RANGE_MAX;
use wows_toolkit_armor::scene::CAST_RANGE_MIN;
use wows_toolkit_armor::scene::CAST_RANGE_STEP;
use wows_toolkit_viewmodel::armor::arc::ArcOutcome;
use wows_toolkit_viewmodel::armor::arc::ComparisonVerdict;
use wows_toolkit_viewmodel::armor::arc::StoppingPlate;
use wows_toolkit_viewmodel::armor::camera_perspective::LookMode;
use wows_toolkit_viewmodel::armor::incoming::ServerOutcome;
use wows_toolkit_viewmodel::armor::incoming::group_incoming;
use wows_toolkit_viewmodel::ship_catalog::translate_name;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::gltf_export::CamoOrigin;
use wowsunpack::export::gltf_export::ArmorLegendEntry;
use wowsunpack::export::gltf_export::armor_color_legend;
use wowsunpack::export::gltf_export::thickness_to_color;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::Species;

use super::export::ExportDialogView;
use super::pen::PenPlate;
use super::pen::PenView;
use super::state::BundleState;
use super::state::GpuState;
use super::state::Pane;
use super::state::PaneId;
use super::state::PaneLoad;
use super::state::State;
use super::tree::HullGroupRow;
use super::tree::TreeNames;
use super::tree::ZoneRow;
use super::tree::hex_color;
use super::tree::hull_rows;
use super::tree::zone_rows;

/// Where the ship data stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Phase {
    /// The settings name no World of Warships directory.
    NotConfigured,
    #[default]
    Pending,
    Failed,
    Ready,
}

/// Where the shared device stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum GpuPhase {
    #[default]
    Initializing,
    Ready,
    Failed,
}

/// How a cast shell's run ended, which picks the colour its line is read in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum OutcomeKind {
    Detonated,
    Overpenetrated,
    Ricocheted,
    Shattered,
    Stopped,
    #[default]
    NotSimulated,
}

/// How the locked camera aims.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Look {
    #[default]
    Game,
    ThroughCenter,
}

impl From<LookMode> for Look {
    fn from(mode: LookMode) -> Self {
        match mode {
            LookMode::Game => Self::Game,
            LookMode::ThroughCenter => Self::ThroughCenter,
        }
    }
}

impl From<Look> for LookMode {
    fn from(look: Look) -> Self {
        match look {
            Look::Game => Self::Game,
            Look::ThroughCenter => Self::ThroughCenter,
        }
    }
}

/// Whether this app's simulation agrees with the server about a shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Standing {
    #[default]
    Agrees,
    /// The angle is in the band where the game rolls for a ricochet.
    Deferred,
    Disagrees,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ShotLogLine {
    pub at: String,
    pub outcome: String,
}

/// One pane of the dock.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct PaneView {
    pub id: u32,
    /// The frame surface to show it in.
    pub surface: String,
    /// One-based, as "Pane 2 of 3" reads.
    pub number: u32,
    pub active: bool,
    pub ship: Option<String>,
    pub param_index: Option<String>,
    /// The ship being loaded.
    pub loading: Option<String>,
    pub load_error: Option<String>,
    pub has_armor: bool,
    /// Bumped when the pane starts showing another ship.
    pub ship_generation: u32,
    pub export_confirm: bool,
    pub hidden_plates: u32,
    pub health: Option<f32>,
    pub hits: u32,
    pub shot_log: Vec<ShotLogLine>,
    pub readout: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct GapsView {
    pub shown: bool,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct SplashZoneView {
    pub zone: String,
    pub box_name: String,
    pub thickness: Option<f32>,
    pub max_hp: Option<f32>,
    pub direct: bool,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct SplashView {
    pub shown: bool,
    pub boxes_shown: bool,
    pub has_data: bool,
    pub zones_reached: u32,
    pub zones: Vec<SplashZoneView>,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct TrajectoryView {
    pub shown: bool,
    pub count: u32,
    pub continue_on_ricochet: bool,
    pub cast_range: f32,
}

/// One cast arc.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct ArcView {
    pub number: u32,
    pub color: String,
    pub heading: String,
    pub summary: String,
    pub range: f32,
    pub isolating_plates: bool,
    pub isolating_zones: bool,
    pub outcome: String,
    pub outcome_kind: OutcomeKind,
    pub can_step_down: bool,
    pub can_step_up: bool,
}

/// A selectable entry with a key and a label.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ChoiceView {
    pub key: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ModuleView {
    pub component: String,
    /// `key` is the component name, which is also its tooltip.
    pub options: Vec<ChoiceView>,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct LodView {
    pub lod: u32,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CamoView {
    pub id: u32,
    pub name: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CamoGroupView {
    /// The i18n key of the group's heading.
    pub heading: String,
    pub schemes: Vec<CamoView>,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CamoPickerView {
    pub selected: Option<u32>,
    pub own: Vec<CamoView>,
    pub groups: Vec<CamoGroupView>,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct HullView {
    pub opaque: bool,
    pub groups: Vec<HullGroupRow>,
    pub upgrades: Vec<ChoiceView>,
    pub modules: Vec<ModuleView>,
    pub camo: Option<CamoPickerView>,
    pub lods: Vec<LodView>,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ModeView {
    pub name: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct RingsView {
    /// The ship names camera modes at all.
    pub available: bool,
    pub shown: bool,
    pub modes: Vec<ModeView>,
    pub fov: f32,
    pub height: f32,
    pub zoom_path: bool,
    pub zoom_path_at_fov: bool,
    pub zoom_path_at_max_fov: bool,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct PerspectiveView {
    pub enabled: bool,
    pub fov: f32,
    pub look: Look,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct LightingView {
    pub enabled: bool,
    pub flat: f32,
    pub intensity: f32,
    pub azimuth: f32,
    pub elevation: f32,
    pub rim: f32,
    pub specular: f32,
    pub shininess: f32,
    pub ambient_color: String,
    pub key_color: String,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct DisplayView {
    pub legend_visible: bool,
    pub plate_edges: bool,
    pub waterline: bool,
    pub waterline_opacity: f32,
    pub zero_mm: bool,
    pub ship_center: bool,
    pub armor_opacity: f32,
    pub marker_opacity: f32,
    /// Hits are drawn, which is when the marker opacity applies.
    pub hits_drawn: bool,
    pub roll: f32,
    pub rings: RingsView,
    pub perspective: PerspectiveView,
    pub lighting: LightingView,
}

/// The active pane, as the options rail and the tools read it.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct ActiveView {
    pub pane_id: u32,
    pub has_armor: bool,
    pub ship: Option<String>,
    pub show_hidden: bool,
    pub gaps: GapsView,
    pub splash: SplashView,
    pub trajectory: TrajectoryView,
    pub arcs: Vec<ArcView>,
    pub has_plate_overrides: bool,
    pub zones: Vec<ZoneRow>,
    pub hull: Option<HullView>,
    pub display: DisplayView,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct LegendEntryView {
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct LegendView {
    pub visible: bool,
    pub collapsed: bool,
    pub x: f32,
    pub y: f32,
    pub entries: Vec<LegendEntryView>,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct StripPoint {
    pub x: f32,
    pub y: f32,
}

/// The ship's health over the battle; every position is a 0..1 fraction of
/// the strip, `y` up.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct HealthStripView {
    pub line: Vec<StripPoint>,
    pub hits: Vec<f32>,
    /// Where playback is.
    pub cursor: Option<f32>,
    pub first_seconds: f32,
    pub last_seconds: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct AttackerView {
    /// `None` is every enemy.
    pub entity: Option<u32>,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SimView {
    pub label: String,
    pub standing: Standing,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct IncomingShellView {
    /// `"m:ss  {outcome}"`.
    pub text: String,
    pub sim: Option<SimView>,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct SalvoView {
    pub attacker: String,
    pub clock: String,
    /// What the clock button seeks to.
    pub seconds: f32,
    pub count: String,
    pub shells: Vec<IncomingShellView>,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct IncomingView {
    pub strip: Option<HealthStripView>,
    pub tracked: String,
    pub sim_unchecked: Option<String>,
    pub secondaries: bool,
    pub attackers: Vec<AttackerView>,
    pub salvos: Vec<SalvoView>,
}

/// The whole viewer.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct ArmorView {
    pub status: Phase,
    /// Why the ship data is not there, worded.
    pub status_reason: Option<String>,
    pub gpu: GpuPhase,
    /// Why the device failed, worded.
    pub gpu_error: Option<String>,
    /// Bumped when a replay asks for the viewer to be shown.
    pub reveal_request: u32,
    pub ship_loaded: bool,
    pub mirror_cameras: bool,
    pub sync_options: bool,
    pub stack_panes: bool,
    pub options_open: bool,
    pub pen_check_open: bool,
    pub active_pane: u32,
    pub panes: Vec<PaneView>,
    pub active: ActiveView,
    pub legend: LegendView,
    pub pen: PenView,
    pub incoming: Option<IncomingView>,
    pub export_dialog: Option<ExportDialogView>,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct HoverLayer {
    pub mm: f32,
    pub color: String,
    /// The layer the pointer is on.
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct HoverPlate {
    pub zone: String,
    /// The material key, which the plate commands take.
    pub material: String,
    pub tenths: i32,
    pub mm: f32,
    pub color: String,
    pub zone_label: String,
    pub material_label: String,
    pub hidden: bool,
    /// Every layer of the part; empty for a single-layer part.
    pub layers: Vec<HoverLayer>,
}

/// What the pointer is over.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
#[non_exhaustive]
pub struct HoverView {
    pub pane_id: Option<u32>,
    /// Logical pixels within the pane.
    pub x: f32,
    pub y: f32,
    pub plate: Option<HoverPlate>,
    /// The camera orbit's label, line by line. Replaces the plate tooltip.
    pub ring: Option<Vec<String>>,
}

/// Names the way the game does, given the build.
fn material_label(material: ArmorMaterial, metadata: Option<&GameMetadataProvider>, species: Option<Species>) -> String {
    match metadata {
        Some(metadata) => material.localized_name(metadata, species),
        None => material.english_label(),
    }
}

fn zone_label(zone: &str, metadata: Option<&GameMetadataProvider>) -> String {
    match metadata {
        Some(metadata) => translate_name(metadata, zone),
        None => zone.to_owned(),
    }
}

/// What pane `id`'s pointer is over.
pub(super) fn hover_view(id: PaneId, scene: &ArmorScene, metadata: Option<&GameMetadataProvider>) -> HoverView {
    if let Some(ring) = scene.hovered_ring() {
        return HoverView {
            pane_id: Some(id.raw()),
            x: ring.cursor.x,
            y: ring.cursor.y,
            plate: None,
            ring: Some(ring.label.lines().map(str::to_owned).collect()),
        };
    }
    let Some(hover) = scene.hovered() else { return HoverView::default() };

    let tooltip = &hover.tooltip;
    let species = scene.current_armor().and_then(|armor| armor.species);
    let layers = if tooltip.layers.len() > 1 {
        tooltip
            .layers
            .iter()
            .map(|&mm| HoverLayer {
                mm,
                color: hex_color(thickness_to_color(mm)),
                current: (mm - tooltip.thickness_mm).abs() < 0.05,
            })
            .collect()
    } else {
        Vec::new()
    };
    let plate = HoverPlate {
        zone: hover.key.zone.clone(),
        material: hover.key.material_name.clone(),
        tenths: hover.key.thickness_tenths,
        mm: tooltip.thickness_mm,
        color: hex_color(tooltip.color),
        zone_label: zone_label(&tooltip.zone, metadata),
        material_label: material_label(tooltip.material, metadata, species),
        hidden: scene.plate_visibility().get(&hover.key).copied().unwrap_or(false),
        layers,
    };

    HoverView { pane_id: Some(id.raw()), x: hover.cursor.x, y: hover.cursor.y, plate: Some(plate), ring: None }
}

/// The whole viewer as the interface reads it.
pub(super) fn armor_view(state: &State) -> ArmorView {
    let metadata = state.bundle().map(|bundle| bundle.assets.metadata());
    let (status, status_reason) = match &state.bundle {
        BundleState::NotConfigured => (Phase::NotConfigured, None),
        BundleState::NotStarted | BundleState::Loading => (Phase::Pending, None),
        BundleState::Ready(_) => (Phase::Ready, None),
        BundleState::Failed(reason) => {
            (Phase::Failed, Some(t!("ui.armor.catalog_failed", reason = reason.clone()).into_owned()))
        }
    };
    let (gpu, gpu_error) = match &state.gpu {
        GpuState::Initializing => (GpuPhase::Initializing, None),
        GpuState::Ready(_) => (GpuPhase::Ready, None),
        GpuState::Failed(reason) => {
            (GpuPhase::Failed, Some(t!("ui.armor.init_failed", reason = reason.clone()).into_owned()))
        }
    };

    let active = state.active_pane();
    let plate = active.scene.last_plate().map(|(zone, mm)| PenPlate { mm, zone: zone_label(zone, metadata) });
    let thickness = plate.as_ref().map(|plate| Millimeters::from(plate.mm));
    let results = match state.bundle() {
        Some(bundle) => state.pen.results(bundle.catalog.ships()),
        None => Vec::new(),
    };
    let pen = PenView {
        ifhe: state.pen.ifhe,
        search: state.pen.search.clone(),
        results,
        plate,
        ships: state.pen.ship_rows(thickness),
        firing_ship: state.pen.firing_ship.clone(),
        firing_shell: state.pen.firing_shell.map(|index| index as u32),
    };

    ArmorView {
        status,
        status_reason,
        gpu,
        gpu_error,
        reveal_request: state.reveal_request,
        ship_loaded: state.ship_loaded,
        mirror_cameras: state.mirror_cameras,
        sync_options: state.sync_options,
        stack_panes: state.stack_panes,
        options_open: state.options_open,
        pen_check_open: state.pen_open,
        active_pane: state.active.raw(),
        panes: state
            .panes
            .iter()
            .enumerate()
            .map(|(index, pane)| pane_view(pane, index, pane.id == state.active))
            .collect(),
        active: active_view(active, metadata, state.legend.visible),
        legend: LegendView {
            visible: state.legend.visible,
            collapsed: state.legend.collapsed,
            x: state.legend.x,
            y: state.legend.y,
            entries: armor_color_legend().iter().map(legend_entry).collect(),
        },
        pen,
        incoming: incoming_view(state),
        export_dialog: state.export.as_ref().map(|dialog| dialog.view()),
    }
}

fn pane_view(pane: &Pane, index: usize, active: bool) -> PaneView {
    let scene = &pane.scene;
    let (loading, load_error) = match &pane.load {
        PaneLoad::Idle => (None, None),
        PaneLoad::Loading { display_name } => (Some(display_name.clone()), None),
        PaneLoad::Failed { display_name, reason } => (
            None,
            Some(t!("ui.armor.ship_load_failed", ship = display_name.clone(), reason = reason.clone()).into_owned()),
        ),
    };
    let hits = scene.hits_drawn();
    let health = scene.hit_health();
    PaneView {
        id: pane.id.raw(),
        surface: pane.id.surface().as_str().to_owned(),
        number: index as u32 + 1,
        active,
        ship: pane.source.as_ref().map(|source| source.display_name.clone()),
        param_index: pane.source.as_ref().map(|source| source.param_index.clone()),
        loading,
        load_error,
        has_armor: scene.has_armor(),
        ship_generation: pane.ship_generation,
        export_confirm: pane.export_confirm,
        hidden_plates: scene.plate_visibility().len() as u32,
        health,
        hits: hits as u32,
        shot_log: scene
            .shot_log()
            .into_iter()
            .map(|line| ShotLogLine { at: line.at, outcome: line.outcome.to_owned() })
            .collect(),
        readout: health.map(|health| {
            t!("ui.armor.realtime_readout", health = format!("{health:.0}"), hits = hits).into_owned()
        }),
    }
}

fn active_view(pane: &Pane, metadata: Option<&GameMetadataProvider>, legend_visible: bool) -> ActiveView {
    let scene = &pane.scene;
    let armor = scene.current_armor();
    let species = armor.and_then(|armor| armor.species);
    let display = scene.display_settings();
    let gaps = scene.gaps();
    let splash = scene.splash_state();
    let trajectory = scene.trajectory_state();
    let names = TreeNames { metadata, species };

    ActiveView {
        pane_id: pane.id.raw(),
        has_armor: scene.has_armor(),
        ship: pane.source.as_ref().map(|source| source.display_name.clone()),
        show_hidden: scene.show_hidden_only(),
        gaps: GapsView { shown: gaps.shown, count: gaps.count as u32 },
        splash: SplashView {
            shown: splash.shown,
            boxes_shown: splash.boxes_shown,
            has_data: splash.has_data,
            zones_reached: splash.zones_reached as u32,
            zones: scene
                .splash_zones()
                .iter()
                .map(|zone| SplashZoneView {
                    zone: zone.zone_name.clone(),
                    box_name: zone.box_name.clone(),
                    thickness: zone.thickness.map(|mm| mm.value()),
                    max_hp: zone.max_hp,
                    direct: zone.is_direct_hit,
                })
                .collect(),
        },
        trajectory: TrajectoryView {
            shown: trajectory.shown,
            count: trajectory.count as u32,
            continue_on_ricochet: trajectory.continue_on_ricochet,
            cast_range: scene.cast_range().value(),
        },
        arcs: scene
            .arcs()
            .iter()
            .enumerate()
            .map(|(index, arc)| arc_view(index, arc, metadata, species))
            .collect(),
        has_plate_overrides: !scene.plate_visibility().is_empty(),
        zones: match armor {
            Some(armor) => zone_rows(
                &armor.zone_part_plates,
                scene.part_visibility(),
                scene.plate_visibility(),
                display.show_zero_mm,
                names,
            ),
            None => Vec::new(),
        },
        hull: armor.map(|_| hull_view(scene)),
        display: display_view(scene, legend_visible),
    }
}

fn arc_view(
    index: usize,
    arc: &ArcSummary,
    metadata: Option<&GameMetadataProvider>,
    species: Option<Species>,
) -> ArcView {
    let range = arc.range.value();
    let step = |by: f32| (range + by).clamp(CAST_RANGE_MIN, CAST_RANGE_MAX);
    let describe_plate = |plate: &Option<StoppingPlate>| match plate {
        Some(plate) => format!(
            "#{} {:.0}mm {}",
            plate.number,
            plate.thickness.value(),
            material_label(plate.material, metadata, species)
        ),
        None => t!("ui.armor.arc_plate_unknown").into_owned(),
    };
    let (outcome, outcome_kind) = match &arc.outcome {
        ArcOutcome::NotSimulated => (t!("ui.armor.arc_no_shell").into_owned(), OutcomeKind::NotSimulated),
        ArcOutcome::Detonated { zone: Some(zone) } => (
            t!("ui.armor.arc_detonated_in", zone = zone_label(zone, metadata)).into_owned(),
            OutcomeKind::Detonated,
        ),
        ArcOutcome::Detonated { zone: None } => {
            (t!("ui.armor.arc_detonated_past_armor").into_owned(), OutcomeKind::Detonated)
        }
        ArcOutcome::Ricocheted { plate } => {
            (t!("ui.armor.arc_ricochet", plate = describe_plate(plate)).into_owned(), OutcomeKind::Ricocheted)
        }
        ArcOutcome::Shattered { plate } => {
            (t!("ui.armor.arc_shatter", plate = describe_plate(plate)).into_owned(), OutcomeKind::Shattered)
        }
        ArcOutcome::Stopped { plate } => {
            (t!("ui.armor.arc_stopped", plate = describe_plate(plate)).into_owned(), OutcomeKind::Stopped)
        }
        ArcOutcome::Overpenetrated { fuse_armed: true } => {
            (t!("ui.armor.arc_overpen").into_owned(), OutcomeKind::Overpenetrated)
        }
        ArcOutcome::Overpenetrated { fuse_armed: false } => {
            (t!("ui.armor.arc_overpen_unarmed").into_owned(), OutcomeKind::Overpenetrated)
        }
    };

    ArcView {
        number: index as u32 + 1,
        color: hex_color(arc.color),
        heading: t!("ui.armor.arc_heading", number = index + 1).into_owned(),
        summary: t!(
            "ui.armor.arc_summary",
            hits = arc.hits,
            armor = format!("{:.0}", arc.total_armor.value()),
            range = format!("{range:.1}"),
        )
        .into_owned(),
        range,
        isolating_plates: arc.isolating_plates,
        isolating_zones: arc.isolating_zones,
        outcome,
        outcome_kind,
        can_step_down: (step(-CAST_RANGE_STEP) - range).abs() >= f32::EPSILON,
        can_step_up: (step(CAST_RANGE_STEP) - range).abs() >= f32::EPSILON,
    }
}

fn hull_view(scene: &ArmorScene) -> HullView {
    let Some(armor) = scene.current_armor() else {
        return HullView {
            opaque: scene.display_settings().hull_opaque,
            groups: Vec::new(),
            upgrades: Vec::new(),
            modules: Vec::new(),
            camo: None,
            lods: Vec::new(),
        };
    };
    let selection = scene.hull_selection();
    let upgrades = armor
        .hull_upgrade_names
        .iter()
        .enumerate()
        .map(|(index, (key, label))| ChoiceView {
            key: key.clone(),
            label: label.clone(),
            // No choice yet is the stock hull, listed first.
            selected: selection.hull.as_ref().map_or(index == 0, |chosen| chosen == key),
        })
        .collect();
    let modules = armor
        .module_alternatives
        .iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|(component, names)| ModuleView {
            component: component.to_string(),
            options: names
                .iter()
                .enumerate()
                .map(|(index, name)| ChoiceView {
                    key: name.clone(),
                    label: format!("{component} {}", (b'A' + index as u8) as char),
                    selected: selection.modules.get(component).map_or(index == 0, |chosen| chosen == name),
                })
                .collect(),
        })
        .collect();
    let selected_camo = scene.selected_camo();
    let camo = (!armor.camo_scheme_infos.is_empty()).then(|| {
        let (own, groups) = camo_groups(&armor.camo_scheme_infos, |id| selected_camo == Some(id));
        CamoPickerView { selected: selected_camo.map(|id| id.0 as u32), own, groups }
    });

    HullView {
        opaque: scene.display_settings().hull_opaque,
        groups: hull_rows(&armor.hull_part_groups, scene.hull_visibility()),
        upgrades,
        modules,
        camo,
        lods: (0..armor.hull_lod_count)
            .map(|lod| LodView { lod: lod as u32, selected: selection.lod == lod })
            .collect(),
    }
}

/// The schemes as the pickers list them: the ship's own first, then the
/// universal, expendable and legacy groups, each sorted by name.
pub(super) fn camo_groups(
    schemes: &[CamoSchemeInfo],
    selected: impl Fn(CamoSchemeId) -> bool,
) -> (Vec<CamoView>, Vec<CamoGroupView>) {
    let sorted = |origin: CamoOrigin| {
        let mut group: Vec<&CamoSchemeInfo> = schemes.iter().filter(|scheme| scheme.origin == origin).collect();
        group.sort_by_key(|scheme| scheme.display_name.to_lowercase());
        group
            .into_iter()
            .map(|scheme| CamoView {
                id: scheme.id.0 as u32,
                name: scheme.display_name.clone(),
                selected: selected(scheme.id),
            })
            .collect::<Vec<_>>()
    };
    let own = sorted(CamoOrigin::ShipSpecific);
    let groups = [
        (CamoOrigin::Universal, "ui.armor.camo_group_universal"),
        (CamoOrigin::Expendable, "ui.armor.camo_group_expendable"),
        (CamoOrigin::LegacyScan, "ui.armor.camo_group_other"),
    ]
    .into_iter()
    .map(|(origin, heading)| CamoGroupView { heading: heading.to_owned(), schemes: sorted(origin) })
    .filter(|group| !group.schemes.is_empty())
    .collect();

    (own, groups)
}

fn display_view(scene: &ArmorScene, legend_visible: bool) -> DisplayView {
    let display = scene.display_settings();
    let rings = scene.camera_rings();
    let modes = scene.camera_modes();
    let perspective = scene.perspective();
    let lighting = scene.lighting();

    DisplayView {
        legend_visible,
        plate_edges: display.show_plate_edges,
        waterline: display.show_waterline,
        waterline_opacity: display.waterline_opacity,
        zero_mm: display.show_zero_mm,
        ship_center: display.show_ship_center,
        armor_opacity: display.armor_opacity,
        marker_opacity: display.marker_opacity,
        hits_drawn: scene.hits_drawn() > 0,
        roll: scene.model_roll_deg(),
        rings: RingsView {
            available: !modes.is_empty(),
            shown: rings.shown,
            modes: modes
                .iter()
                .map(|name| ModeView { name: name.clone(), selected: rings.mode.as_ref() == Some(name) })
                .collect(),
            fov: rings.fov,
            height: rings.height,
            zoom_path: rings.zoom_path,
            zoom_path_at_fov: rings.zoom_path_at_fov,
            zoom_path_at_max_fov: rings.zoom_path_at_max_fov,
        },
        perspective: PerspectiveView {
            enabled: perspective.enabled,
            fov: perspective.camera.fov_deg,
            look: perspective.camera.look_mode.into(),
        },
        lighting: LightingView {
            enabled: lighting.enabled,
            flat: lighting.flat_intensity,
            intensity: lighting.key_intensity,
            azimuth: lighting.azimuth_deg,
            elevation: lighting.elevation_deg,
            rim: lighting.rim_strength,
            specular: lighting.specular_strength,
            shininess: lighting.shininess,
            ambient_color: rgb_hex(lighting.flat_color),
            key_color: rgb_hex(lighting.key_color),
        },
    }
}

fn rgb_hex(color: [f32; 3]) -> String {
    hex_color([color[0], color[1], color[2], 1.0])
}

/// `"#rrggbb"` as a 0..1 colour, `None` for anything else.
pub(super) fn parse_hex(color: &str) -> Option<[f32; 3]> {
    let digits = color.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok().map(|value| value as f32 / 255.0);

    Some([channel(0)?, channel(2)?, channel(4)?])
}

fn legend_entry(entry: &ArmorLegendEntry) -> LegendEntryView {
    let label = if entry.max_mm >= 999.0 {
        format!("{}+ mm", entry.min_mm as u32)
    } else {
        format!("{}-{} mm", entry.min_mm as u32, entry.max_mm as u32)
    };

    LegendEntryView { label, color: hex_color(entry.color) }
}

fn incoming_view(state: &State) -> Option<IncomingView> {
    let incoming = &state.incoming;
    if incoming.is_empty() {
        return None;
    }
    let context = &incoming.context;
    let scene = &state.active_pane().scene;

    let enemies: HashSet<EntityId> = context.attackers.keys().copied().collect();
    let salvos = group_incoming(&context.taken, &incoming.filter, &enemies, &context.main_battery);
    let shells: usize = salvos.iter().map(|salvo| salvo.shells.len()).sum();
    let chosen = incoming.filter.attacker;

    let strip = context.health.as_ref().map(|strip| HealthStripView {
        line: strip.line.iter().map(|&(x, y)| StripPoint { x, y }).collect(),
        hits: strip.hits.clone(),
        cursor: state.playback_at.map(|at| strip.along(at)),
        first_seconds: strip.first.seconds(),
        last_seconds: strip.last.seconds(),
    });
    let mut attackers = vec![AttackerView {
        entity: None,
        label: t!("ui.armor.realtime.all_enemies").into_owned(),
        selected: chosen.is_none(),
    }];
    attackers.extend(context.attackers.iter().map(|(entity, name)| AttackerView {
        entity: Some(entity.raw()),
        label: name.clone(),
        selected: chosen == Some(*entity),
    }));
    let sim_unchecked = scene.sim_unchecked();

    Some(IncomingView {
        strip,
        tracked: t!("ui.armor.realtime.tracked", salvos = salvos.len(), shells = shells).into_owned(),
        sim_unchecked: (sim_unchecked > 0)
            .then(|| t!("ui.armor.realtime.sim_unchecked", count = sim_unchecked).into_owned()),
        secondaries: incoming.filter.secondaries,
        attackers,
        salvos: salvos
            .iter()
            .map(|salvo| SalvoView {
                // A ship the roster did not name reads as its entity.
                attacker: context.attackers.get(&salvo.attacker).cloned().unwrap_or_else(|| {
                    t!("ui.armor.realtime.unknown_attacker", id = salvo.attacker.raw()).into_owned()
                }),
                clock: clock_label(salvo.first_clock),
                seconds: salvo.first_clock.seconds(),
                count: t!("ui.armor.realtime.shells", count = salvo.shells.len()).into_owned(),
                shells: salvo
                    .shells
                    .iter()
                    .map(|shell| IncomingShellView {
                        text: format!("{}  {}", clock_label(shell.clock), hit_label(&shell.hit_type)),
                        sim: scene.sim_verdict(shell.shot_id).map(sim_view),
                    })
                    .collect(),
            })
            .collect(),
    })
}

fn sim_view(verdict: &ComparisonVerdict) -> SimView {
    match verdict {
        ComparisonVerdict::Match => {
            SimView { label: t!("ui.armor.realtime.sim_agrees").into_owned(), standing: Standing::Agrees }
        }
        ComparisonVerdict::RicochetRngDefer { angle, ricochet_start, always_ricochet } => SimView {
            label: t!(
                "ui.armor.realtime.sim_rng_zone",
                angle = format!("{:.1}", angle.value()),
                from = format!("{:.1}", ricochet_start.value()),
                to = format!("{:.1}", always_ricochet.value()),
            )
            .into_owned(),
            standing: Standing::Deferred,
        },
        ComparisonVerdict::Mismatch { sim, server } => SimView {
            label: t!("ui.armor.realtime.sim_disagrees", sim = t!(sim.label_key()), server = t!(server.label_key()))
                .into_owned(),
            standing: Standing::Disagrees,
        },
    }
}

/// Minutes and seconds into the battle.
fn clock_label(clock: GameClock) -> String {
    let seconds = clock.seconds().max(0.0);
    format!("{}:{:02}", (seconds / 60.0).floor() as i32, (seconds % 60.0) as i32)
}

/// What the server said a shell did.
fn hit_label(hit: &HitType) -> String {
    let outcome = ServerOutcome::from_shell_hit_type(&hit.shell_hit);
    match &outcome {
        ServerOutcome::Unknown(raw) => t!(outcome.label_key(), raw = raw.clone()).into_owned(),
        _ => t!(outcome.label_key()).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hex_colour_round_trips() {
        assert_eq!(parse_hex("#ff8000"), Some([1.0, 128.0 / 255.0, 0.0]));
        assert_eq!(rgb_hex([1.0, 128.0 / 255.0, 0.0]), "#ff8000");
        assert_eq!(parse_hex("ff8000"), None);
        assert_eq!(parse_hex("#ff80"), None);
        assert_eq!(parse_hex("#gg8000"), None);
    }

    #[test]
    fn a_clock_reads_as_minutes_and_seconds() {
        assert_eq!(clock_label(GameClock(0.0)), "0:00");
        assert_eq!(clock_label(GameClock(754.6)), "12:34");
        assert_eq!(clock_label(GameClock(-3.0)), "0:00");
    }
}
