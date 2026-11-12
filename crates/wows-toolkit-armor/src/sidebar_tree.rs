//! The Model Viewer sidebar's rows: the ships section, a "Models" heading,
//! then the model catalogue.
//!
//! Only expanded folders get their real children. A collapsed folder is
//! marked [`ItemChildren::Collapsed`] so it still draws as a folder, and the
//! sidebar rebuilds when it is expanded. This keeps a rebuild proportional to
//! what is on screen rather than to the 15000-row catalogue.
//!
//! A search is matched once into [`SearchMatches`]. Folders without matches
//! are dropped and the rest show their match counts. A section with few
//! matches has its folders opened; a broad search leaves them closed.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use rust_i18n::t;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::export::model_catalog::BuildingKind;
use wowsunpack::export::model_catalog::GeometryAvailability;
use wowsunpack::export::model_catalog::ModelGroupKind;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_catalog::ModelVariantKind;
use wowsunpack::export::ship_parts::MiscParent;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::GameParamId;

use crate::catalog::HiddenReason;
use crate::catalog::NationGroup;
use crate::catalog::ShipCatalog;
use crate::catalog::ShipEntry;
use crate::catalog::TierRange;
use crate::catalog::species_name;
use crate::catalog::tier_roman;
use crate::filter_fields::FilterFields;
use crate::model_refs::HardpointTarget;
use crate::model_refs::OpenTarget;
use crate::model_refs::ShipPartKey;
use crate::model_refs::ShipPartRef;
use crate::model_refs::VehicleParamIndex;
use crate::model_tree::BuildingRow;
use crate::model_tree::FolderName;
use crate::model_tree::FortificationRow;
use crate::model_tree::HullRow;
use crate::model_tree::ModelFolder;
use crate::model_tree::ModelMatch;
use crate::model_tree::ModelRow;
use crate::model_tree::ModelTreeIndex;
use crate::model_tree::MountGroup;
use crate::model_tree::PartRow;
use crate::model_tree::ShipPartRows;
use crate::model_tree::SkinUse;
use crate::model_tree::VariantRow;
use crate::model_tree::model_kind_label;
use crate::model_tree::model_sub_kind_label;

pub const SEPARATOR_ID: &str = "armor-models-separator";
const MODELS_STATUS_ID: &str = "armor-models-status";
/// The row id prefix of every model folder.
const MODEL_FOLDER_ID: &str = "model-folder";

/// A row's id, kept by the row while the listing changes around it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId(Arc<str>);

impl RowId {
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for RowId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for RowId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for RowId {
    fn from(id: &str) -> Self {
        Self(id.into())
    }
}

impl From<String> for RowId {
    fn from(id: String) -> Self {
        Self(id.into())
    }
}

/// What a row refers to, consulted by the row renderer, the click handler
/// and the context menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKind {
    Nation(String),
    Class(Species),
    Ship {
        param_index: String,
        display_name: String,
    },
    PartFolder,
    ShipPart {
        ship: VehicleParamIndex,
        part: ShipPartKey,
        model: ModelPath,
        /// The `HP_`/`MP_` node shown before the model name.
        node: Option<String>,
    },
    /// The heading between the ships and the models.
    Separator,
    ModelsStatus,
    ModelNation {
        /// GameParams nation key, for the flag.
        flag: Option<String>,
    },
    ModelGroup,
    Model(ModelPath),
    /// The Fortifications folder or one of its kind folders.
    BuildingGroup,
    Building {
        id: GameParamId,
        display_name: String,
        param_name: String,
    },
}

impl RowKind {
    /// Whether the row opens a ship, building or model, rather than being a
    /// folder or heading.
    pub fn opens_on_its_own(&self) -> bool {
        matches!(self, Self::Ship { .. } | Self::Building { .. } | Self::Model(_) | Self::ShipPart { .. })
    }

    /// Whether the row is a folder a click opens or closes.
    pub fn is_folder(&self) -> bool {
        matches!(
            self,
            Self::Nation(_)
                | Self::Class(_)
                | Self::PartFolder
                | Self::ModelNation { .. }
                | Self::ModelGroup
                | Self::BuildingGroup
        )
    }

    /// The model the row opens and the ship part it is opened from.
    pub fn model_selection(&self) -> Option<(ModelPath, Option<ShipPartRef>)> {
        match self {
            Self::Model(model) => Some((model.clone(), None)),
            Self::ShipPart { ship, part, model, .. } => {
                Some((model.clone(), Some(ShipPartRef { vehicle_param_index: ship.clone(), part: part.clone() })))
            }
            _ => None,
        }
    }

    /// The hardpoint target the row's checkbox controls, if it has one.
    pub fn hardpoint_target(&self) -> Option<HardpointTarget> {
        match self {
            Self::Model(model) => Some(HardpointTarget::Model(model.clone())),
            Self::ShipPart { ship, part, .. } => {
                Some(HardpointTarget::ShipPart { vehicle_param_index: ship.clone(), part: part.clone() })
            }
            _ => None,
        }
    }
}

/// What a row's tooltip says: its whole name, which the row may cut short,
/// and the model path or GameParams index behind it. A skin row also says
/// what it replaces, from `skin_uses`, and a row the install cannot draw,
/// why.
pub fn row_tooltip(kind: &RowKind, skin_uses: &[SkinUse], missing: Option<GeometryAvailability>) -> Option<String> {
    let mut text = match kind {
        RowKind::Ship { param_index, display_name } => format!("{display_name}\n{param_index}"),
        RowKind::Building { display_name, param_name, .. } => format!("{display_name}\n{param_name}"),
        RowKind::Model(model) | RowKind::ShipPart { node: None, model, .. } => format!("{}\n{model}", model.stem()),
        RowKind::ShipPart { node: Some(node), model, .. } => format!("{node} {}\n{model}", model.stem()),
        RowKind::Nation(_)
        | RowKind::Class(_)
        | RowKind::PartFolder
        | RowKind::Separator
        | RowKind::ModelsStatus
        | RowKind::ModelNation { .. }
        | RowKind::ModelGroup
        | RowKind::BuildingGroup => return None,
    };

    // A line per exterior, so the sentence reads the same in every locale.
    for skin in skin_uses {
        for exterior in &skin.exteriors {
            text.push('\n');
            text.push_str(&t!("ui.models.skin_replaces", model = skin.stock.stem(), exterior = exterior));
        }
    }

    let missing = match missing {
        Some(GeometryAvailability::Absent) => Some(t!("ui.models.geometry_absent")),
        Some(GeometryAvailability::NoGeometry) => Some(t!("ui.models.geometry_none")),
        Some(GeometryAvailability::Present) | None => None,
    };
    if let Some(missing) = missing {
        text.push('\n');
        text.push_str(&missing);
    }

    Some(text)
}

/// Where the model section stands.
pub enum ModelSection<'a> {
    Loading,
    Unavailable,
    Ready(&'a ModelTreeIndex),
}

pub struct TreeSources<'a> {
    pub ships: &'a ShipCatalog,
    pub ship_parts: &'a HashMap<VehicleParamIndex, ShipPartRows>,
    pub models: ModelSection<'a>,
    /// Armored buildings, listed after the ships; empty until listed.
    pub fortifications: &'a [FortificationRow],
    /// Why each ship the catalog normally leaves out is left out, for the
    /// ships listed anyway.
    pub hidden: &'a HashMap<String, HiddenReason>,
}

/// Which folders are expanded. With `auto` set, every folder a search wants
/// open is opened and recorded in `set`, except those in `closed`.
pub struct Expansion<'a> {
    pub set: &'a mut HashSet<RowId>,
    /// Folders the user closed during the search.
    pub closed: &'a HashSet<RowId>,
    pub auto: bool,
}

impl Expansion<'_> {
    fn resolve(&mut self, id: &RowId, wanted: bool) -> bool {
        if self.auto && wanted && !self.closed.contains(id) {
            self.set.insert(id.clone());
            return true;
        }

        self.set.contains(id)
    }
}

/// One row and, for an expanded folder, the rows under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarItem {
    pub id: RowId,
    pub label: String,
    /// A heading or status line, which cannot be selected.
    pub disabled: bool,
    pub children: ItemChildren,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemChildren {
    Leaf,
    /// A folder whose rows are not built.
    Collapsed,
    Expanded(Vec<SidebarItem>),
}

pub struct BuiltTree {
    pub items: Vec<SidebarItem>,
    pub rows: HashMap<RowId, RowKind>,
    /// Dim text drawn after a row's name.
    pub notes: HashMap<RowId, RowNote>,
    /// Model rows the install cannot draw, drawn dimmed.
    pub unavailable: HashMap<RowId, GeometryAvailability>,
}

/// Dim text drawn after a row's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowNote {
    /// During a search, the field the row was found by where it is not the
    /// row's own name. Drawn marked where the search matched it.
    Matched(String),
    /// What the row is, such as a skin or a ship normally hidden.
    Tag(String),
}

impl RowNote {
    pub fn text(&self) -> &str {
        match self {
            Self::Matched(text) | Self::Tag(text) => text,
        }
    }
}

impl std::fmt::Display for RowNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.text())
    }
}

/// What a search names, worked out once per query and per change to the
/// listed ships or models. Opening or closing a folder during the search
/// reuses it and tests no name again.
pub struct SearchMatches {
    /// Inactive when only the tiers narrow the listing.
    query: FilterQuery,
    ships: ShipMatches,
    /// `None` until the model catalogue is listed.
    models: Option<ModelMatches>,
    /// Buildings in [`TreeSources::fortifications`] order. The tiers do not
    /// apply to them.
    fortifications: MatchGroup<bool>,
}

/// Ships by nation and class, in [`ShipCatalog`] order.
type ShipMatches = MatchGroup<MatchGroup<MatchGroup<ShipMatch>>>;

/// The top model folders, in [`ModelTreeIndex`] order.
type ModelMatches = MatchGroup<FolderMatches>;

impl SearchMatches {
    /// `None` for an inactive `query` and every tier, which lists
    /// everything. The tiers filter only ships; models are filtered by the
    /// query alone.
    pub fn from_query(sources: &TreeSources<'_>, query: &FilterQuery, tiers: TierRange) -> Option<Self> {
        if !query.is_active() && tiers.is_all() {
            return None;
        }

        let found = |ship: &ShipEntry| ship_match(ship, sources.ship_parts, query, tiers);
        let ships = sources
            .ships
            .nations
            .iter()
            .map(|nation| nation.classes.iter().map(|class| class.ships.iter().map(found).collect()).collect())
            .collect();

        let models = match sources.models {
            ModelSection::Ready(index) if query.is_active() => {
                Some(index.folders.iter().map(|folder| FolderMatches::new(folder, query)).collect())
            }
            ModelSection::Ready(_) | ModelSection::Loading | ModelSection::Unavailable => None,
        };

        let fortifications = sources.fortifications.iter().map(|row| row.filter_fields.matches(query)).collect();

        Some(Self { query: query.clone(), ships, models, fortifications })
    }
}

/// The matches in one model folder, in its listing order.
struct FolderMatches {
    count: usize,
    folders: Vec<FolderMatches>,
    models: Vec<ModelMatch>,
}

impl FolderMatches {
    /// Folders nest at most three deep, so this recursion stays shallow.
    fn new(folder: &ModelFolder, query: &FilterQuery) -> Self {
        let folders: Vec<FolderMatches> = folder.folders.iter().map(|folder| Self::new(folder, query)).collect();
        let models: Vec<ModelMatch> = folder.models.iter().map(|model| model.search(query)).collect();
        let count = folders.iter().map(MatchCount::match_count).sum::<usize>()
            + models.iter().map(MatchCount::match_count).sum::<usize>();

        Self { count, folders, models }
    }
}

/// How a search names one ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShipMatch {
    Miss,
    /// The ship itself, whatever its parts are called.
    Name,
    /// Only one or more of its parts.
    Parts,
}

fn ship_match(
    ship: &ShipEntry,
    ship_parts: &HashMap<VehicleParamIndex, ShipPartRows>,
    query: &FilterQuery,
    tiers: TierRange,
) -> ShipMatch {
    if !tiers.contains(ship.tier) {
        return ShipMatch::Miss;
    }

    if ship.filter_fields.matches(query) {
        return ShipMatch::Name;
    }

    let parts = ship_parts.get(ship.param_index.as_str());
    if parts.is_some_and(|parts| parts.any_matches(query)) {
        return ShipMatch::Parts;
    }

    ShipMatch::Miss
}

/// The matches under one folder, in its listing order.
struct MatchGroup<T> {
    count: usize,
    members: Vec<T>,
}

impl<T> FromIterator<T> for MatchGroup<T>
where
    T: MatchCount,
{
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let members: Vec<T> = iter.into_iter().collect();
        let count = members.iter().map(MatchCount::match_count).sum();

        Self { count, members }
    }
}

/// How many listed ships or models a search names under a node.
trait MatchCount {
    fn match_count(&self) -> usize;
}

impl MatchCount for ShipMatch {
    fn match_count(&self) -> usize {
        match self {
            ShipMatch::Miss => 0,
            ShipMatch::Name | ShipMatch::Parts => 1,
        }
    }
}

impl MatchCount for ModelMatch {
    fn match_count(&self) -> usize {
        match self {
            ModelMatch::Miss => 0,
            ModelMatch::Name | ModelMatch::Variant => 1,
        }
    }
}

impl<T> MatchCount for MatchGroup<T> {
    fn match_count(&self) -> usize {
        self.count
    }
}

impl MatchCount for FolderMatches {
    fn match_count(&self) -> usize {
        self.count
    }
}

impl MatchCount for bool {
    fn match_count(&self) -> usize {
        usize::from(*self)
    }
}

/// Builds the tree. `search` is `None` when listing everything.
pub fn build_tree(
    sources: &TreeSources<'_>,
    search: Option<&SearchMatches>,
    expansion: &mut Expansion<'_>,
) -> BuiltTree {
    let mut builder = Builder {
        search,
        rows: HashMap::new(),
        notes: HashMap::new(),
        unavailable: HashMap::new(),
        expansion,
        sources,
    };
    let mut items = Vec::new();

    let ship_matches = search.map(|search| &search.ships);
    for (nation, matches) in paired(&sources.ships.nations, members(ship_matches)) {
        if shown(matches) {
            items.push(builder.ship_nation(nation, matches));
        }
    }

    let fortification_matches = search.map(|search| search.fortifications.members.as_slice());
    let fortifications: Vec<(&FortificationRow, Option<&bool>)> =
        paired(sources.fortifications, fortification_matches).filter(|(_, found)| shown(*found)).collect();
    if !fortifications.is_empty() {
        items.push(builder.fortifications(&fortifications));
    }

    builder.rows.insert(SEPARATOR_ID.into(), RowKind::Separator);
    items.push(SidebarItem {
        id: SEPARATOR_ID.into(),
        label: t!("ui.models.heading").into_owned(),
        disabled: true,
        children: ItemChildren::Leaf,
    });

    match sources.models {
        ModelSection::Loading => items.push(builder.status_row(t!("ui.models.loading").into_owned())),
        ModelSection::Unavailable => items.push(builder.status_row(t!("ui.models.unavailable").into_owned())),
        ModelSection::Ready(index) => {
            let model_matches = search.and_then(|search| search.models.as_ref());
            for (folder, matches) in paired(&index.folders, members(model_matches)) {
                if shown(matches) {
                    items.push(builder.model_folder(MODEL_FOLDER_ID, folder, matches));
                }
            }
        }
    }

    BuiltTree { items, rows: builder.rows, notes: builder.notes, unavailable: builder.unavailable }
}

/// The rows that stand for `open`, the first listed of which is
/// highlighted. A building stands under Fortifications when its hull is
/// armored, and under its hull model.
pub fn open_row_ids(open: &OpenTarget, index: Option<&ModelTreeIndex>) -> Vec<RowId> {
    let mut ids = vec![open_row_id(open)];
    if let (OpenTarget::Building(building), Some(index)) = (open, index)
        && let Some(hull) = index.building_hulls.get(building)
    {
        ids.push(model_building_row_id(hull, *building));
    }

    ids
}

/// The row id `open` is drawn on.
pub fn open_row_id(open: &OpenTarget) -> RowId {
    match open {
        OpenTarget::Ship(ship) => ship_row_id(ship.as_str()),
        OpenTarget::Model { model, from_ship: None } => model_row_id(model),
        OpenTarget::Model { from_ship: Some(part), .. } => part_row_id(&part.vehicle_param_index, &part.part),
        OpenTarget::Building(id) => building_row_id(*id),
    }
}

pub fn ship_row_id(param_index: &str) -> RowId {
    format!("armor-ship-{param_index}").into()
}

pub fn building_row_id(id: GameParamId) -> RowId {
    format!("armor-building-{id}").into()
}

/// A building's row under its hull model, apart from its row under
/// Fortifications.
pub fn model_building_row_id(model: &ModelPath, id: GameParamId) -> RowId {
    format!("model-building-{model}-{id}").into()
}

pub fn model_row_id(model: &ModelPath) -> RowId {
    format!("model-{model}").into()
}

pub fn part_row_id(ship: &VehicleParamIndex, part: &ShipPartKey) -> RowId {
    match part {
        ShipPartKey::HullSection(model) => format!("armor-part-{ship}-hull-{model}").into(),
        ShipPartKey::Mount(hp) => format!("armor-part-{ship}-hp-{hp}").into(),
        ShipPartKey::Misc { node, parent: MiscParent::Mount(hp) } => {
            format!("armor-part-{ship}-mp-hp-{hp}-{node}").into()
        }
        ShipPartKey::Misc { node, parent: MiscParent::HullSection(section) } => {
            format!("armor-part-{ship}-mp-hull-{section}-{node}").into()
        }
    }
}

fn hidden_reason_label(reason: HiddenReason) -> String {
    let key = match reason {
        HiddenReason::ClanRental => "ui.armor.hidden_ship.clan_rental",
        HiddenReason::OtherClass => "ui.armor.hidden_ship.other_class",
    };

    t!(key).into_owned()
}

fn model_group_label(group: ModelGroupKind) -> String {
    let key = match group {
        ModelGroupKind::Gameplay => "ui.models.group.gameplay",
        ModelGroupKind::Building => "ui.models.group.building",
        ModelGroupKind::Character => "ui.models.group.character",
        ModelGroupKind::Location => "ui.models.group.location",
        ModelGroupKind::Port => "ui.models.group.port",
        ModelGroupKind::Style => "ui.models.group.style",
        ModelGroupKind::Other => "ui.models.group.other",
    };

    t!(key).into_owned()
}

/// A building kind the game has no name for: its GameParams species, or
/// one of the groups no species names.
fn building_kind_label(kind: &BuildingKind) -> String {
    let species = match kind {
        BuildingKind::Weapon => return t!("ui.models.building.weapon").into_owned(),
        BuildingKind::Unassigned => return t!("ui.models.building.unassigned").into_owned(),
        BuildingKind::Unspecified => return t!("ui.models.building.unspecified").into_owned(),
        BuildingKind::Unrecognized(species) => return species.clone(),
        BuildingKind::Complex => "Complex",
        BuildingKind::CoastalArtillery => "CoastalArtillery",
        BuildingKind::AntiAircraft => "AntiAircraft",
        BuildingKind::AirBase => "AirBase",
        BuildingKind::Military => "Military",
        BuildingKind::SensorTower => "SensorTower",
        BuildingKind::RayTower => "RayTower",
        BuildingKind::Generator => "Generator",
        BuildingKind::SpaceStation => "SpaceStation",
    };

    species.to_string()
}

fn folder_label(name: &FolderName) -> String {
    match name {
        FolderName::Nation { display_name, .. } => display_name.clone(),
        FolderName::Common => t!("ui.models.common").into_owned(),
        FolderName::Group(group) => model_group_label(*group),
        FolderName::Kind(kind) => model_kind_label(*kind),
        FolderName::SubKind(sub_kind) => model_sub_kind_label(sub_kind),
        FolderName::Building { kind, display_name } => {
            display_name.clone().unwrap_or_else(|| building_kind_label(kind))
        }
        FolderName::Directory(directory) => directory.clone(),
    }
}

pub fn mount_group_label(group: MountGroup) -> String {
    let key = match group {
        MountGroup::MainBattery => "ui.models.part.main_battery",
        MountGroup::Secondaries => "ui.models.part.secondaries",
        MountGroup::AntiAircraft => "ui.models.part.anti_aircraft",
        MountGroup::Torpedoes => "ui.models.part.torpedoes",
        MountGroup::Directors => "ui.models.part.directors",
        MountGroup::Finders => "ui.models.part.finders",
        MountGroup::Radars => "ui.models.part.radars",
        MountGroup::DepthCharges => "ui.models.part.depth_charges",
        MountGroup::Aircraft => "ui.models.part.aircraft",
        MountGroup::Other => "ui.models.part.other",
    };

    t!(key).into_owned()
}

fn variant_label(kind: &ModelVariantKind, path: &ModelPath) -> String {
    match kind {
        ModelVariantKind::Dead => t!("ui.models.variant.dead").into_owned(),
        ModelVariantKind::Section(suffix) => suffix.clone(),
        ModelVariantKind::Skin => path.stem().to_string(),
    }
}

struct Builder<'q, 'e, 's, 'd> {
    search: Option<&'q SearchMatches>,
    rows: HashMap<RowId, RowKind>,
    notes: HashMap<RowId, RowNote>,
    unavailable: HashMap<RowId, GeometryAvailability>,
    expansion: &'e mut Expansion<'s>,
    sources: &'q TreeSources<'d>,
}

impl Builder<'_, '_, '_, '_> {
    fn opens_ships(&self) -> bool {
        self.search.is_some_and(|search| opens(&search.ships))
    }

    /// Whether a search opens the ships it names only by their parts. Each
    /// opened ship adds a row per matching part, so fewer ships are opened
    /// than other folders.
    fn opens_ship_parts(&self) -> bool {
        const SHIP_PARTS_OPEN_LIMIT: usize = 20;

        self.search.is_some_and(|search| search.ships.match_count() <= SHIP_PARTS_OPEN_LIMIT)
    }

    fn opens_models(&self) -> bool {
        self.search.and_then(|search| search.models.as_ref()).is_some_and(opens)
    }

    /// A folder row. `children` runs only when the folder is expanded.
    fn folder(
        &mut self,
        id: RowId,
        label: impl Into<String>,
        kind: RowKind,
        wanted: bool,
        children: impl FnOnce(&mut Self) -> Vec<SidebarItem>,
    ) -> SidebarItem {
        self.rows.insert(id.clone(), kind);
        let label = label.into();
        let children = match self.expansion.resolve(&id, wanted) {
            true => ItemChildren::Expanded(children(self)),
            false => ItemChildren::Collapsed,
        };

        SidebarItem { id, label, disabled: false, children }
    }

    fn leaf(&mut self, id: RowId, label: impl Into<String>, kind: RowKind) -> SidebarItem {
        self.rows.insert(id.clone(), kind);
        SidebarItem { id, label: label.into(), disabled: false, children: ItemChildren::Leaf }
    }

    fn status_row(&mut self, label: String) -> SidebarItem {
        SidebarItem { disabled: true, ..self.leaf(MODELS_STATUS_ID.into(), label, RowKind::ModelsStatus) }
    }

    /// Records the field of `fields` the search found row `id` by best,
    /// unless it is the first, which is the name the row shows.
    fn note(&mut self, id: &RowId, fields: &FilterFields) {
        let Some(search) = self.search.filter(|search| search.query.is_active()) else {
            return;
        };
        let Some(field) = fields.best(&search.query) else {
            return;
        };
        if fields.fields().next() == Some(field) {
            return;
        }

        self.notes.insert(id.clone(), RowNote::Matched(field.to_string()));
    }

    /// Records row `id` as one the install cannot draw. An unknown
    /// availability is taken as present.
    fn mark_geometry(&mut self, id: &RowId, geometry: Option<GeometryAvailability>) {
        match geometry {
            Some(missing @ (GeometryAvailability::Absent | GeometryAvailability::NoGeometry)) => {
                self.unavailable.insert(id.clone(), missing);
            }
            // Drawn as usual.
            Some(GeometryAvailability::Present) | None => {}
        }
    }

    fn ship_nation(
        &mut self,
        nation: &NationGroup,
        matches: Option<&MatchGroup<MatchGroup<ShipMatch>>>,
    ) -> SidebarItem {
        let id: RowId = format!("armor-nation-{}", nation.nation).into();
        let label = counted(nation.display_name.clone(), matches);
        let wanted = self.opens_ships();
        self.folder(id, label, RowKind::Nation(nation.nation.clone()), wanted, |b| {
            let mut items = Vec::new();
            for (class, matches) in paired(&nation.classes, members(matches)) {
                if !shown(matches) {
                    continue;
                }

                let id: RowId = format!("armor-class-{}-{:?}", nation.nation, class.species).into();
                let label = counted(species_name(&class.species), matches);
                items.push(b.folder(id, label, RowKind::Class(class.species), wanted, |b| {
                    paired(&class.ships, members(matches))
                        .filter(|(_, found)| shown(*found))
                        .map(|(ship, found)| b.ship_row(ship, found.copied()))
                        .collect()
                }));
            }

            items
        })
    }

    /// The Fortifications folder, with a folder per building kind. `rows`
    /// are the listed buildings, in kind order.
    fn fortifications(&mut self, rows: &[(&FortificationRow, Option<&bool>)]) -> SidebarItem {
        const FORTIFICATIONS_ID: &str = "armor-fortifications";

        // The tiers do not filter buildings, so only a search counts them.
        let searching = self.search.is_some_and(|search| search.query.is_active());
        let with_count = |label: String, count: usize| if searching { format!("{label} ({count})") } else { label };
        let label = with_count(t!("ui.armor.fortifications").into_owned(), rows.len());
        let wanted = self.opens_ships();
        self.folder(FORTIFICATIONS_ID.into(), label, RowKind::BuildingGroup, wanted, |b| {
            rows.chunk_by(|(left, _), (right, _)| left.kind == right.kind)
                .map(|group| {
                    let (first, _) = group[0];
                    let id: RowId = format!("{FORTIFICATIONS_ID}/{:?}", first.kind).into();
                    let name = first.kind_name.clone().unwrap_or_else(|| building_kind_label(&first.kind));
                    b.folder(id, with_count(name, group.len()), RowKind::BuildingGroup, wanted, |b| {
                        group.iter().map(|(row, _)| b.building_row(row)).collect()
                    })
                })
                .collect()
        })
    }

    fn building_row(&mut self, row: &FortificationRow) -> SidebarItem {
        let id = building_row_id(row.id);
        let kind = RowKind::Building {
            id: row.id,
            display_name: row.display_name.clone(),
            param_name: row.param_name.clone(),
        };
        self.note(&id, &row.filter_fields);
        self.mark_geometry(&id, row.geometry);

        self.leaf(id, row.display_name.clone(), kind)
    }

    /// `found` is `None` when listing everything.
    fn ship_row(&mut self, ship: &ShipEntry, found: Option<ShipMatch>) -> SidebarItem {
        let id = ship_row_id(&ship.param_index);
        let label = format!("{} {}", tier_roman(ship.tier), ship.display_name);
        let kind = RowKind::Ship { param_index: ship.param_index.clone(), display_name: ship.display_name.clone() };
        if found == Some(ShipMatch::Name) {
            self.note(&id, &ship.filter_fields);
        }
        // A search note says more than the tag, which stands in otherwise.
        if let Some(reason) = self.sources.hidden.get(&ship.param_index) {
            self.notes.entry(id.clone()).or_insert_with(|| RowNote::Tag(hidden_reason_label(*reason)));
        }

        let vehicle = VehicleParamIndex::new(ship.param_index.clone());
        let ship_parts = self.sources.ship_parts;
        let Some(parts) = ship_parts.get(&vehicle).filter(|parts| !parts.is_empty()) else {
            return self.leaf(id, label, kind);
        };

        // A search that names only parts opens the ship to list them; one
        // that names the ship leaves its parts folded.
        let part_query = match found {
            Some(ShipMatch::Parts) => self.search.map(|search| &search.query),
            Some(ShipMatch::Name | ShipMatch::Miss) | None => None,
        };
        let wanted = part_query.is_some() && self.opens_ship_parts();
        self.folder(id, label, kind, wanted, |b| b.ship_parts(&vehicle, parts, part_query))
    }

    /// `query` lists only the parts it names, in opened folders; `None` lists
    /// every part.
    fn ship_parts(
        &mut self,
        ship: &VehicleParamIndex,
        parts: &ShipPartRows,
        query: Option<&FilterQuery>,
    ) -> Vec<SidebarItem> {
        let listed = |fields: &FilterFields| query.is_none_or(|query| fields.matches(query));
        let wanted = query.is_some() && self.opens_ship_parts();
        let mut items = Vec::new();

        let hull: Vec<&HullRow> = parts.hull.iter().filter(|row| listed(&row.filter_fields)).collect();
        if !hull.is_empty() {
            let id: RowId = format!("armor-part-{ship}-group-hull").into();
            items.push(self.folder(id, t!("ui.models.part.hull").into_owned(), RowKind::PartFolder, wanted, |b| {
                hull.iter().map(|row| b.hull_row(ship, row)).collect()
            }));
        }

        for group in &parts.mounts {
            let rows: Vec<&PartRow> = group.parts.iter().filter(|part| listed(&part.filter_fields)).collect();
            if rows.is_empty() {
                continue;
            }

            let id: RowId = format!("armor-part-{ship}-group-{:?}", group.group).into();
            items.push(self.folder(id, mount_group_label(group.group), RowKind::PartFolder, wanted, |b| {
                rows.iter().map(|part| b.part_row(ship, part)).collect()
            }));
        }

        let misc: Vec<&PartRow> = parts.misc.iter().filter(|part| listed(&part.filter_fields)).collect();
        if !misc.is_empty() {
            let id: RowId = format!("armor-part-{ship}-group-misc").into();
            items.push(self.folder(id, t!("ui.models.part.misc").into_owned(), RowKind::PartFolder, wanted, |b| {
                misc.iter().map(|part| b.part_row(ship, part)).collect()
            }));
        }

        items
    }

    fn hull_row(&mut self, ship: &VehicleParamIndex, row: &HullRow) -> SidebarItem {
        let part = ShipPartKey::HullSection(row.model.clone());
        let id = part_row_id(ship, &part);
        let kind = RowKind::ShipPart { ship: ship.clone(), part, model: row.model.clone(), node: None };
        self.note(&id, &row.filter_fields);
        self.mark_geometry(&id, row.geometry);

        self.leaf(id, row.model.stem().to_string(), kind)
    }

    fn part_row(&mut self, ship: &VehicleParamIndex, part: &PartRow) -> SidebarItem {
        let id = part_row_id(ship, &part.key);
        let kind = RowKind::ShipPart {
            ship: ship.clone(),
            part: part.key.clone(),
            model: part.model.clone(),
            node: Some(part.node.as_str().to_string()),
        };
        self.note(&id, &part.filter_fields);
        self.mark_geometry(&id, part.geometry);

        self.leaf(id, format!("{} {}", part.node, part.model.stem()), kind)
    }

    /// A model folder under the row id `parent`, with its sub-folders and
    /// then its models.
    fn model_folder(&mut self, parent: &str, folder: &ModelFolder, matches: Option<&FolderMatches>) -> SidebarItem {
        let id: RowId = format!("{parent}/{}", folder.key).into();
        let kind = match &folder.name {
            FolderName::Nation { flag, .. } => RowKind::ModelNation { flag: flag.clone() },
            _ => RowKind::ModelGroup,
        };
        let label = counted(folder_label(&folder.name), matches);
        let wanted = self.opens_models();
        let parent = id.clone();
        self.folder(id, label, kind, wanted, |b| {
            let mut items: Vec<SidebarItem> =
                paired(&folder.folders, matches.map(|matches| matches.folders.as_slice()))
                    .filter(|(_, found)| shown(*found))
                    .map(|(child, found)| b.model_folder(parent.as_str(), child, found))
                    .collect();
            items.extend(b.model_rows(&folder.models, matches.map(|matches| matches.models.as_slice())));

            items
        })
    }

    fn model_rows(&mut self, models: &[ModelRow], matches: Option<&[ModelMatch]>) -> Vec<SidebarItem> {
        paired(models, matches)
            .filter(|(_, found)| shown(*found))
            .map(|(model, found)| self.model_row(model, found.copied()))
            .collect()
    }

    /// `found` is `None` when listing everything.
    fn model_row(&mut self, model: &ModelRow, found: Option<ModelMatch>) -> SidebarItem {
        let id = model_row_id(&model.path);
        let label = model.path.stem().to_string();
        let kind = RowKind::Model(model.path.clone());
        self.mark_geometry(&id, model.geometry);
        if found == Some(ModelMatch::Name) {
            self.note(&id, &model.filter_fields);
        }

        if model.variants.is_empty() && model.buildings.is_empty() {
            return self.leaf(id, label, kind);
        }

        // A search that names only a variant opens the model to show it; one
        // that names the model leaves its variants folded. The buildings it
        // is the body of come first, while the model itself is listed whole.
        let base_matches = found.is_none_or(|found| found == ModelMatch::Name);
        let wanted = self.opens_models() && !base_matches;
        self.folder(id, label, kind, wanted, |b| {
            let mut items = Vec::new();
            if base_matches {
                items.extend(model.buildings.iter().map(|building| b.model_building_row(model, building)));
            }
            items.extend(b.variant_rows(&model.variants, base_matches));

            items
        })
    }

    /// A GameParams building under the model that is its body, which it
    /// cannot be drawn without.
    fn model_building_row(&mut self, model: &ModelRow, building: &BuildingRow) -> SidebarItem {
        let id = model_building_row_id(&model.path, building.id);
        self.mark_geometry(&id, model.geometry);
        let kind = RowKind::Building {
            id: building.id,
            display_name: building.display_name.clone(),
            param_name: building.param_name.clone(),
        };

        self.leaf(id, building.display_name.clone(), kind)
    }

    /// `show_all` lists every variant, for a parent the search names itself.
    /// Variants are tested here rather than in [`SearchMatches`]: only those
    /// of a model being drawn are, so the cost follows the rows built.
    fn variant_rows(&mut self, variants: &[VariantRow], show_all: bool) -> Vec<SidebarItem> {
        let listing = FilterQuery::default();
        let query = self.search.map_or(&listing, |search| &search.query);
        let mut items = Vec::new();
        for variant in variants {
            let matches = variant.matches(query);
            if !show_all && !matches && !variant.any_variant_matches(query) {
                continue;
            }

            let id = model_row_id(&variant.path);
            let label = variant_label(&variant.kind, &variant.path);
            let kind = RowKind::Model(variant.path.clone());
            self.mark_geometry(&id, variant.geometry);
            if variant.kind == ModelVariantKind::Skin {
                self.notes.insert(id.clone(), RowNote::Tag(t!("ui.models.variant.skin").into_owned()));
            }

            if variant.variants.is_empty() {
                items.push(self.leaf(id, label, kind));
                continue;
            }

            let show_nested = show_all || matches;
            let wanted = self.opens_models() && !show_nested;
            items.push(self.folder(id, label, kind, wanted, |b| b.variant_rows(&variant.variants, show_nested)));
        }

        items
    }
}

/// `items` beside their matches; each has `None` when listing everything.
fn paired<'a, I, M>(items: &'a [I], matches: Option<&'a [M]>) -> impl Iterator<Item = (&'a I, Option<&'a M>)> {
    let mut members = matches.map(|matches| matches.iter());
    items.iter().map(move |item| (item, members.as_mut().and_then(Iterator::next)))
}

/// A group's matches in listing order.
fn members<M>(group: Option<&MatchGroup<M>>) -> Option<&[M]> {
    group.map(|group| group.members.as_slice())
}

/// Whether a row is listed. Every row is when listing everything.
fn shown<M>(matches: Option<&M>) -> bool
where
    M: MatchCount,
{
    matches.is_none_or(|matches| matches.match_count() > 0)
}

/// `label`, followed during a search by the number of matches under it.
fn counted<M>(label: impl Into<String>, matches: Option<&M>) -> String
where
    M: MatchCount,
{
    let label = label.into();
    match matches {
        Some(matches) => format!("{label} ({})", matches.match_count()),
        None => label,
    }
}

/// Whether a search opens the folders holding a section's matches. A broad
/// search leaves them closed with their counts shown, so that one query does
/// not build a row for most of the 15000-model catalogue.
fn opens<M>(section: &M) -> bool
where
    M: MatchCount,
{
    const AUTO_OPEN_LIMIT: usize = 200;

    section.match_count() <= AUTO_OPEN_LIMIT
}

/// A small ship catalog, model catalogue and part listing with real paths.
#[cfg(any(test, feature = "test-support"))]
pub mod fixture;

#[cfg(test)]
mod tests;
