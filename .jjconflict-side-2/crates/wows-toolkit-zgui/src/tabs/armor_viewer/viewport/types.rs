//! What the viewport's state hands to the panels around it, and what they hand back.

use std::collections::HashMap;
use std::sync::Arc;

use rust_i18n::t;
use wows_toolkit_armor::assets::ModelAssets;
use wows_toolkit_armor::load_model::ModelLoadRequest;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::scene_load::LoadCancel;
use wows_toolkit_armor::trajectory::ArcColor;
use wows_toolkit_armor::trajectory::ArcId;
use wows_toolkit_armor::upload;
use wows_toolkit_armor::viewport::types::LightingSettings;
use wows_toolkit_viewmodel::armor::arc::ArcOutcome;
use wows_toolkit_viewmodel::armor::arc::ComparisonVerdict;
use wows_toolkit_viewmodel::armor::camera_perspective::CameraPerspective;
use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wowsunpack::ballistics::ImpactResult;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;

/// How the camera orbits are drawn.
///
/// `mode` is `None` until a ship names one, and is corrected whenever the loaded ship changes: a
/// mode name belongs to a ship's own GameParams, so carrying one across ships would draw nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraRingSettings {
    pub shown: bool,
    pub mode: Option<String>,
    /// Where between the field-of-view extremes the camera sits, 0 to 1.
    pub fov: f32,
    /// How far the camera is raised, -1 to 1.
    pub height: f32,
    pub zoom_path: bool,
    pub zoom_path_at_fov: bool,
    pub zoom_path_at_max_fov: bool,
}

/// The camera locked onto one of the ship's own orbits, looking the way the game's camera looks
/// from there.
///
/// A drag turns the eye on its orbit and tilts it rather than orbiting the model, and the wheel
/// moves between the inner and outer orbit rather than pulling the camera back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PerspectiveSettings {
    pub enabled: bool,
    pub camera: CameraPerspective,
}

/// Which of the two isolations an arc is being asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Isolate {
    /// Only the plates the shell crossed.
    Plates,
    /// Every part of the zones it entered.
    Zones,
}

/// One cast arc, as the trajectory section shows it.
#[derive(Clone)]
pub struct ArcSummary {
    pub id: ArcId,
    /// The colour the arc is drawn in, so the marker and the line match.
    pub color: ArcColor,
    pub hits: usize,
    /// The plating the shell crossed, added up regardless of angle.
    pub total_armor: Millimeters,
    pub range: Km,
    /// What it was fired with. `None` for an arc cast before any ship was chosen, which shows the
    /// plates crossed without a shell.
    pub shell: Option<CastShell>,
    pub isolating_plates: bool,
    pub isolating_zones: bool,
    pub outcome: ArcOutcome,
}

/// The ship and shell an arc is fired with.
#[derive(Clone)]
pub struct CastShell {
    pub ship: ComparisonShip,
    /// The shell in the ship's list. `None` fires its armour-piercing shell.
    pub shell_index: Option<usize>,
}

impl CastShell {
    /// Where the fired shell sits in the ship's list: the one chosen, else the ship's
    /// armour-piercing shell, which is the one a trajectory through plating says anything about,
    /// else whatever it carries rather than refusing.
    pub fn fired_index(&self) -> Option<usize> {
        let shells = &self.ship.shells;
        self.shell_index
            .filter(|index| *index < shells.len())
            .or_else(|| shells.iter().position(|shell| shell.ammo_type == AmmoType::AP))
            .or_else(|| (!shells.is_empty()).then_some(0))
    }

    /// The shell fired.
    pub fn shell(&self) -> Option<&ShellInfo> {
        self.ship.shells.get(self.fired_index()?)
    }

    /// The shell's ballistics and how it lands from `range`. `None` for a shell the solver cannot
    /// fire.
    pub(super) fn fire(&self, range: Km) -> Option<(ShellParams, ImpactResult)> {
        let params = ShellParams::from_shell_info(self.shell()?)?;
        let impact = solve_for_range(&params, range.to_meters())?;
        Some((params, impact))
    }

    pub(super) fn is_same(&self, other: &CastShell) -> bool {
        self.ship.param_index == other.ship.param_index && self.shell_index == other.shell_index
    }
}

/// Whether the armor's openings are marked, and how many were found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GapState {
    pub shown: bool,
    pub count: usize,
}

/// Whether clicks burst shells, and what the last burst reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplashState {
    pub shown: bool,
    pub boxes_shown: bool,
    /// Whether this hull ships a splash file at all. The mode is refused without one rather than
    /// placing a burst against nothing.
    pub has_data: bool,
    pub zones_reached: usize,
}

/// How many shells have been cast, and whether a ricochet is followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrajectoryState {
    pub count: usize,
    pub continue_on_ricochet: bool,
}

/// What a pane is currently showing all of.
///
/// Kept in the shared `armor_viewer_defaults` row, which is how a ship opened after one where the
/// reader hid everything opens the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllVisible {
    pub hull: bool,
    pub armor: bool,
}

/// Which layer of a model is in view when it opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpeningLayer {
    /// The armor, with the hull as the saved defaults leave it.
    #[default]
    Armor,
    /// The hull meshes, with the armor loaded but hidden.
    Mesh,
}

impl OpeningLayer {
    pub fn from_show_armor(show_armor: bool) -> Self {
        if show_armor { Self::Armor } else { Self::Mesh }
    }

    pub fn shows_armor(self) -> bool {
        self == Self::Armor
    }

    /// What `scene` opens on. A scene with no armor, or with armor of unknown thickness, opens on
    /// its meshes whatever the setting: there is nothing, or only uniform grey plates, to look at
    /// otherwise.
    pub fn for_scene(self, scene: &LoadedShipArmor) -> Self {
        if scene.has_armor() && !scene.thickness_unknown() { self } else { Self::Mesh }
    }
}

/// Which visibility maps still hold the [`OpeningLayer::Mesh`] seed, untouched by the reader. A
/// seeded map stands for the saved default rather than a choice: `all_visible` reports the default
/// for it, and a reload seeds the reloaded model's parts the same way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshSeed {
    pub(super) armor: bool,
    pub(super) hull: bool,
}

/// What a user-driven change was about, for a pane mirroring its camera or options onto the
/// others. The silent counterparts `set_camera` and `apply_synced` never report one, which keeps a
/// broadcast from echoing back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportChange {
    Camera,
    Settings,
}

/// Which of the loads a pane started into one viewport a result belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadGeneration(pub(super) u64);

/// A load the pane started into one viewport, and the flag that stops it.
#[derive(Clone, Debug)]
pub struct OpeningLoad {
    pub generation: LoadGeneration,
    pub cancel: LoadCancel,
}

/// Which kinds of option the scene on screen has anything for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneFeatures {
    /// A ship: splash boxes, camera orbits, the waterline, camo and hulls.
    pub ship: bool,
    /// Armor plates: their tree, the legend, casts and the plate tools.
    pub armor: bool,
}

/// What a pane's scene was loaded from, which a reload or an export reads again. Set by the pane
/// right before `show_armor`; a reload never replaces it, only a new selection does.
#[derive(Clone)]
pub enum SceneSource {
    Ship { assets: Arc<ShipAssets>, param_index: String, display_name: String },
    Model { assets: Arc<ShipAssets>, models: Arc<ModelAssets>, request: ModelLoadRequest },
}

impl SceneSource {
    pub fn assets(&self) -> &Arc<ShipAssets> {
        match self {
            Self::Ship { assets, .. } | Self::Model { assets, .. } => assets,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Ship { display_name, .. } => display_name,
            Self::Model { request, .. } => request.path.stem(),
        }
    }

    /// The ship's param index, for a ship.
    pub fn param_index(&self) -> Option<&str> {
        match self {
            Self::Ship { param_index, .. } => Some(param_index),
            Self::Model { .. } => None,
        }
    }
}

/// What one pane hands another while "sync options" is on.
///
/// The selected camo is not among them: a `CamoSchemeId` is an index within one ship's own scheme
/// list, so it cannot be reapplied to a pane showing another ship.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncedSettings {
    pub(super) part_visibility: HashMap<(String, String), bool>,
    pub(super) plate_visibility: HashMap<PlateKey, bool>,
    pub(super) hull_visibility: HashMap<String, bool>,
    pub(super) display_settings: upload::DisplaySettings,
    pub(super) lighting: LightingSettings,
    pub(super) camera_rings: CameraRingSettings,
    /// Whether the camera is locked to the ship's orbit, and how it aims.
    pub(super) perspective: PerspectiveSettings,
    pub(super) mesh_seed: MeshSeed,
}

/// What this app's own simulation made of a shell, worded for the log.
///
/// Worded when the comparison is run rather than per frame: a battleship's log runs to hundreds of
/// lines, and each would otherwise translate and format itself again on every draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimAgreement {
    pub label: String,
    pub standing: Agreement,
}

/// Whether the two agree, which is what colours the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agreement {
    Agrees,
    /// The angle falls in the band where the game rolls for a ricochet, so either call is right
    /// and there is nothing to disagree about.
    Deferred,
    Disagrees,
}

/// The reading beside a shell: whether this app's ballistics reach the same answer the server
/// did.
pub fn agreement(verdict: &ComparisonVerdict) -> SimAgreement {
    match verdict {
        ComparisonVerdict::Match => {
            SimAgreement { label: t!("ui.armor.realtime.sim_agrees").into_owned(), standing: Agreement::Agrees }
        }
        ComparisonVerdict::RicochetRngDefer { angle, ricochet_start, always_ricochet } => SimAgreement {
            label: t!(
                "ui.armor.realtime.sim_rng_zone",
                angle = format!("{:.1}", angle.value()),
                from = format!("{:.1}", ricochet_start.value()),
                to = format!("{:.1}", always_ricochet.value()),
            )
            .into_owned(),
            standing: Agreement::Deferred,
        },
        ComparisonVerdict::Mismatch { sim, server } => SimAgreement {
            label: t!("ui.armor.realtime.sim_disagrees", sim = t!(sim.label_key()), server = t!(server.label_key()))
                .into_owned(),
            standing: Agreement::Disagrees,
        },
    }
}
