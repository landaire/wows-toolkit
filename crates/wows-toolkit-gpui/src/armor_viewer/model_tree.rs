//! The Model Viewer sidebar's model listing: the catalogue grouped into
//! folders, gameplay models by nation, kind and sub-kind and every other model
//! by the group the catalogue files it in, with every row's search texts
//! folded once so a search only scores them.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use wowsunpack::data::ResourceLoader;
use wowsunpack::export::building_armor::BuildingArmorIndex;
use wowsunpack::export::model_catalog::BuildingKind;
use wowsunpack::export::model_catalog::GeometryAvailability;
use wowsunpack::export::model_catalog::MiscCategory;
use wowsunpack::export::model_catalog::ModelCatalog;
use wowsunpack::export::model_catalog::ModelEntry;
use wowsunpack::export::model_catalog::ModelGroup;
use wowsunpack::export::model_catalog::ModelGroupKind;
use wowsunpack::export::model_catalog::ModelKind;
use wowsunpack::export::model_catalog::ModelNation;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_catalog::ModelSubKind;
use wowsunpack::export::model_catalog::ModelVariantKind;
use wowsunpack::export::model_library::ModelLibrary;
use wowsunpack::export::part_group::PartGroup;
use wowsunpack::export::ship_parts::ListedMisc;
use wowsunpack::export::ship_parts::MiscParent;
use wowsunpack::export::ship_parts::ShipPartListing;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::translations::translate_exterior;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::MountParam;
use wowsunpack::game_types::GameParamId;

use super::catalog::ShipCatalog;
use super::filter_fields::FilterFields;
use super::model_refs::ShipPartKey;
use super::model_refs::VehicleParamIndex;
use crate::text_filter::FilterKey;
use crate::text_filter::FilterQuery;

/// Every listed catalogue model, grouped for the tree.
pub struct ModelTreeIndex {
    /// The gameplay nations, then one folder per other model group.
    pub folders: Vec<ModelFolder>,
    /// What each skin model replaces, for its row's tooltip. Shared, as a
    /// row hands it to its tooltip on every frame.
    pub skins: HashMap<ModelPath, Arc<[SkinUse]>>,
    /// The hull model each listed building stands under.
    pub building_hulls: HashMap<GameParamId, ModelPath>,
}

/// A folder of the model listing.
pub struct ModelFolder {
    /// Unique among its siblings. A row id joins it to its ancestors' keys.
    pub key: String,
    pub name: FolderName,
    /// Sub-folders in display order.
    pub folders: Vec<ModelFolder>,
    /// The models filed directly in the folder, by stem.
    pub models: Vec<ModelRow>,
}

/// What a folder shows as its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderName {
    Nation {
        /// The GameParams nation of the same name, whose flag the row shows.
        flag: Option<String>,
        /// Its translated name, or the directory name.
        display_name: String,
    },
    /// The `common` directory: models no one nation owns. The game names no
    /// such nation.
    Common,
    Group(ModelGroupKind),
    Kind(ModelKind),
    SubKind(ModelSubKind),
    Building {
        kind: BuildingKind,
        /// The game's name for the kind, where it has one.
        display_name: Option<String>,
    },
    /// A directory, which is the only name the game gives it.
    Directory(String),
}

pub struct ModelRow {
    pub path: ModelPath,
    /// The stem, then the GameParams entries that use the model. A search
    /// does not look at the directories a model sits in, which nearly every
    /// query would match.
    pub filter_fields: FilterFields,
    pub variants: Vec<VariantRow>,
    /// Whether the install ships its geometry; `None` when unknown.
    pub geometry: Option<GeometryAvailability>,
    /// The GameParams buildings this model is the body of, which open with
    /// their guns in place.
    pub buildings: Vec<BuildingRow>,
}

pub struct VariantRow {
    pub kind: ModelVariantKind,
    pub path: ModelPath,
    /// The stem.
    pub filter_key: FilterKey,
    /// A skin's own dead and section variants; empty for the other kinds.
    pub variants: Vec<VariantRow>,
    /// As [`ModelRow::geometry`].
    pub geometry: Option<GeometryAvailability>,
}

/// How a search names one catalogue model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelMatch {
    Miss,
    /// The model itself, whatever its variants are called.
    Name,
    /// Only one or more of its variants.
    Variant,
}

impl ModelRow {
    pub fn search(&self, query: &FilterQuery) -> ModelMatch {
        if self.filter_fields.matches(query) {
            return ModelMatch::Name;
        }

        if self.variants.iter().any(|variant| variant.matches(query) || variant.any_variant_matches(query)) {
            return ModelMatch::Variant;
        }

        ModelMatch::Miss
    }
}

impl VariantRow {
    pub fn matches(&self, query: &FilterQuery) -> bool {
        query.score(&self.filter_key).is_some()
    }

    pub fn any_variant_matches(&self, query: &FilterQuery) -> bool {
        self.variants.iter().any(|variant| variant.matches(query))
    }
}

/// The GameParams entries that use each model: ships for their hull models,
/// mounts for theirs. A model is found by their names as well as its own.
/// Also the exteriors that swap each skin model in.
#[derive(Default)]
pub struct ModelReferences {
    keys: HashMap<ModelPath, FilterFields>,
    skins: HashMap<ModelPath, Vec<SkinUse>>,
}

/// A stock model a skin model replaces, and the exteriors that replace it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinUse {
    pub stock: ModelPath,
    /// The exteriors' names in the current locale, or their GameParams names
    /// where the game has none, sorted.
    pub exteriors: Vec<String>,
}

impl ModelReferences {
    pub fn new() -> Self {
        Self { keys: HashMap::new(), skins: HashMap::new() }
    }

    /// The references of the ships in `ships`, which are the ships the
    /// sidebar lists, and of every mount they carry on any hull.
    pub fn build(ships: &ShipCatalog, metadata: &GameMetadataProvider) -> Self {
        let listed: HashMap<&str, &FilterFields> =
            ships.ships().map(|ship| (ship.param_index.as_str(), &ship.filter_fields)).collect();
        let mut references = Self::new();
        let mut mount_keys: HashMap<GameParamId, FilterFields> = HashMap::new();
        // Ships repeat their hull models and mounts across hulls and share
        // mounts with each other; each pairing is added once.
        let mut seen: HashSet<(&str, GameParamId)> = HashSet::new();
        for param in metadata.params() {
            if let Some(exterior) = param.exterior() {
                // The game names an exterior under one of several keys; an
                // unnamed one shows its GameParams name.
                let name = translate_exterior(param, metadata).0.unwrap_or_else(|| param.name().to_string());
                for pair in exterior.peculiarity_models() {
                    references.insert_skin(&pair.replacement, &pair.stock, &name);
                }

                continue;
            }

            let Some(ship_key) = listed.get(param.index()) else {
                continue;
            };
            let Some(vehicle) = param.vehicle() else {
                continue;
            };

            let hulls = vehicle.hull_upgrades().into_iter().flat_map(|hulls| hulls.values());
            let hull_models =
                vehicle.model_path().into_iter().chain(hulls.clone().filter_map(|hull| hull.hull_model_path()));
            for model in hull_models {
                if seen.insert((model, param.id())) {
                    references.insert(model, ship_key);
                }
            }

            let alternatives =
                hulls.clone().flat_map(|hull| hull.alternative_mounts.values()).flat_map(|mounts| mounts.mounts());
            for mount in hulls.flat_map(|hull| hull.all_mount_points()).chain(alternatives) {
                let Some(mount_param) = mount.param() else {
                    continue;
                };
                if !seen.insert((mount.model_path(), mount_param.id())) {
                    continue;
                }

                let key = mount_keys.entry(mount_param.id()).or_insert_with(|| mount_fields(metadata, mount_param));
                references.insert(mount.model_path(), key);
            }
        }

        references
    }

    /// Records that exterior `exterior` shows model `replacement` in place of
    /// `stock`, keeping stocks and exteriors sorted and listed once. Paths
    /// that are not models are left out.
    pub fn insert_skin(&mut self, replacement: &str, stock: &str, exterior: &str) {
        let (Ok(replacement), Ok(stock)) = (ModelPath::parse(replacement), ModelPath::parse(stock)) else {
            return;
        };

        let uses = self.skins.entry(replacement).or_default();
        let idx = match uses.binary_search_by(|skin| skin.stock.cmp(&stock)) {
            Ok(idx) => idx,
            Err(idx) => {
                uses.insert(idx, SkinUse { stock, exteriors: Vec::new() });
                idx
            }
        };
        let exteriors = &mut uses[idx].exteriors;
        if let Err(at) = exteriors.binary_search_by(|name| name.as_str().cmp(exterior)) {
            exteriors.insert(at, exterior.to_string());
        }
    }

    /// Adds `key`'s fields to model `model`'s. A path that is not a model
    /// names nothing the sidebar lists.
    pub fn insert(&mut self, model: &str, key: &FilterFields) {
        let Ok(path) = ModelPath::parse(model) else {
            return;
        };

        self.keys.entry(path).or_default().extend(key);
    }

    /// What `model` is found by: its stem, then its references.
    pub fn model_key(&self, model: &ModelPath) -> FilterFields {
        let mut key = FilterFields::new();
        key.push(model.stem());
        if let Some(references) = self.keys.get(model) {
            key.extend(references);
        }

        key
    }
}

/// [`ModelReferences::model_key`] for each part model, built once however
/// many ships carry it.
pub struct PartKeys<'a> {
    references: &'a ModelReferences,
    keys: HashMap<ModelPath, Arc<FilterFields>>,
}

impl<'a> PartKeys<'a> {
    pub fn new(references: &'a ModelReferences) -> Self {
        Self { references, keys: HashMap::new() }
    }

    fn key(&mut self, model: &ModelPath) -> Arc<FilterFields> {
        let references = self.references;
        let key = self.keys.entry(model.clone()).or_insert_with(|| Arc::new(references.model_key(model)));

        Arc::clone(key)
    }
}

/// The folders a ship's mounts are listed under, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MountGroup {
    MainBattery,
    Secondaries,
    AntiAircraft,
    Torpedoes,
    Directors,
    Finders,
    Radars,
    DepthCharges,
    Aircraft,
    Other,
}

impl MountGroup {
    /// The folder for a mount of `group` showing a model of `kind`. The model
    /// kind splits what GameParams files together as fire control.
    pub fn classify(group: PartGroup, kind: Option<ModelKind>) -> Self {
        match (kind, group) {
            (Some(ModelKind::Director), _) => Self::Directors,
            (Some(ModelKind::Finder), _) => Self::Finders,
            (Some(ModelKind::Radar), _) => Self::Radars,
            (Some(ModelKind::Catapult | ModelKind::Aircraft), _) => Self::Aircraft,
            (_, PartGroup::MainBattery) => Self::MainBattery,
            (_, PartGroup::SecondaryBattery) => Self::Secondaries,
            (_, PartGroup::AaGuns) => Self::AntiAircraft,
            (_, PartGroup::Torpedoes) => Self::Torpedoes,
            (_, PartGroup::DepthCharges) => Self::DepthCharges,
            (_, PartGroup::FireControl) => Self::Directors,
            (_, PartGroup::Radar) => Self::Radars,
            (_, _) => Self::Other,
        }
    }
}

/// A ship's parts as its tree row lists them.
#[derive(Default)]
pub struct ShipPartRows {
    /// The hull model and its sections.
    pub hull: Vec<HullRow>,
    /// Non-empty groups in [`MountGroup`] order.
    pub mounts: Vec<MountGroupRows>,
    pub misc: Vec<PartRow>,
    /// Every part's search texts once; ships repeat a mount many times.
    keys: Vec<Arc<FilterFields>>,
}

pub struct MountGroupRows {
    pub group: MountGroup,
    pub parts: Vec<PartRow>,
}

/// The hull model or one of its sections.
pub struct HullRow {
    pub model: ModelPath,
    /// As [`ModelRow::filter_fields`].
    pub filter_fields: Arc<FilterFields>,
    /// As [`ModelRow::geometry`].
    pub geometry: Option<GeometryAvailability>,
}

/// One mount or misc part.
pub struct PartRow {
    pub key: ShipPartKey,
    /// The node it sits at, as shown: `HP_x`, `MP_y`, or the misc node behind
    /// its parent mount or hull section.
    pub node: String,
    pub model: ModelPath,
    /// As [`ModelRow::filter_fields`].
    pub filter_fields: Arc<FilterFields>,
    /// As [`ModelRow::geometry`].
    pub geometry: Option<GeometryAvailability>,
}

impl ShipPartRows {
    pub fn from_listing(listing: &ShipPartListing, catalog: &ModelCatalog, keys: &mut PartKeys<'_>) -> Self {
        let mut groups: BTreeMap<MountGroup, Vec<PartRow>> = BTreeMap::new();
        for mount in &listing.mounts {
            let entry = catalog.owning_entry(&mount.model);
            let kind = entry.and_then(|entry| entry.group().gameplay()).map(|gameplay| gameplay.kind());
            groups.entry(MountGroup::classify(mount.group, kind)).or_default().push(PartRow {
                key: ShipPartKey::Mount(mount.hp.clone()),
                node: mount.hp.to_string(),
                model: mount.model.clone(),
                filter_fields: keys.key(&mount.model),
                geometry: catalog.geometry(&mount.model),
            });
        }

        let misc: Vec<PartRow> = listing
            .miscs
            .iter()
            .map(|misc| PartRow {
                key: misc.key(),
                node: misc_label(misc, listing.hull_sections.first()),
                model: misc.model.clone(),
                filter_fields: keys.key(&misc.model),
                geometry: catalog.geometry(&misc.model),
            })
            .collect();

        let hull: Vec<HullRow> = listing
            .hull_sections
            .iter()
            .map(|model| HullRow {
                model: model.clone(),
                filter_fields: keys.key(model),
                geometry: catalog.geometry(model),
            })
            .collect();

        let mounts: Vec<MountGroupRows> =
            groups.into_iter().map(|(group, parts)| MountGroupRows { group, parts }).collect();
        let mut unique_keys: Vec<Arc<FilterFields>> = hull
            .iter()
            .map(|row| &row.filter_fields)
            .chain(mounts.iter().flat_map(|group| &group.parts).map(|part| &part.filter_fields))
            .chain(misc.iter().map(|part| &part.filter_fields))
            .map(Arc::clone)
            .collect();
        unique_keys.sort_by_key(Arc::as_ptr);
        unique_keys.dedup_by(|a, b| Arc::ptr_eq(a, b));

        Self { hull, mounts, misc, keys: unique_keys }
    }

    pub fn is_empty(&self) -> bool {
        self.hull.is_empty() && self.mounts.is_empty() && self.misc.is_empty()
    }

    /// Whether `query` names any part.
    pub fn any_matches(&self, query: &FilterQuery) -> bool {
        self.keys.iter().any(|key| key.matches(query))
    }
}

impl ModelTreeIndex {
    /// Groups `catalog`'s listed entries. `ships` names the nations whose
    /// directory matches a GameParams nation.
    pub fn build(
        catalog: &ModelCatalog,
        ships: &ShipCatalog,
        references: &ModelReferences,
        mut buildings: ModelBuildings,
        metadata: &GameMetadataProvider,
    ) -> Self {
        let mut nations: Vec<ModelFolder> = Vec::new();
        let mut groups: Vec<ModelFolder> = Vec::new();
        let mut building_hulls = HashMap::new();
        for entry in catalog.listed_entries() {
            let row = model_row(entry, catalog, references, &mut buildings);
            building_hulls.extend(row.buildings.iter().map(|building| (building.id, row.path.clone())));
            let path = folder_path(entry.group(), ships, metadata);
            let (top, rest) = path.split_first().expect("a folder path names its top folder");
            let siblings = if entry.group().kind() == ModelGroupKind::Gameplay { &mut nations } else { &mut groups };
            let mut folder = child_folder(siblings, top);
            for step in rest {
                folder = child_folder(&mut folder.folders, step);
            }
            folder.models.push(row);
        }

        nations.sort_by_cached_key(|folder| folder_order(&folder.name));
        groups.sort_by_cached_key(|folder| folder_order(&folder.name));
        let mut pending: Vec<&mut ModelFolder> = nations.iter_mut().chain(groups.iter_mut()).collect();
        while let Some(folder) = pending.pop() {
            folder.folders.sort_by_cached_key(|folder| folder_order(&folder.name));
            folder.models.sort_by_cached_key(|model| model.path.stem().to_lowercase());
            pending.extend(folder.folders.iter_mut());
        }
        nations.append(&mut groups);

        let skins = references.skins.iter().map(|(skin, uses)| (skin.clone(), Arc::from(uses.as_slice()))).collect();

        Self { folders: nations, skins, building_hulls }
    }

    pub fn model_count(&self) -> usize {
        let mut count = 0;
        let mut pending: Vec<&ModelFolder> = self.folders.iter().collect();
        while let Some(folder) = pending.pop() {
            count += folder.models.len();
            pending.extend(&folder.folders);
        }

        count
    }
}

/// A folder's place among its siblings, which are all of one variant.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FolderOrder {
    /// Groups, kinds and misc categories, by their `ALL` display order.
    Rank(usize),
    /// Other sub-kinds and building kinds, by their declaration order.
    SubKind(ModelSubKind),
    Building(BuildingKind),
    /// Nations and directories, by name.
    Name(String),
    /// The common directory, after the nations.
    Common,
}

fn folder_order(name: &FolderName) -> FolderOrder {
    // Every `ALL` lists each variant, so the fallback never applies.
    let rank = |position: Option<usize>| FolderOrder::Rank(position.unwrap_or(usize::MAX));
    match name {
        FolderName::Nation { display_name, .. } => FolderOrder::Name(display_name.to_lowercase()),
        FolderName::Common => FolderOrder::Common,
        FolderName::Directory(directory) => FolderOrder::Name(directory.to_lowercase()),
        FolderName::Group(group) => rank(ModelGroupKind::ALL.iter().position(|each| each == group)),
        FolderName::Kind(kind) => rank(ModelKind::ALL.iter().position(|each| each == kind)),
        FolderName::SubKind(ModelSubKind::Misc(category)) => {
            rank(MiscCategory::ALL.iter().position(|each| each == category))
        }
        FolderName::SubKind(sub_kind) => FolderOrder::SubKind(sub_kind.clone()),
        FolderName::Building { kind, .. } => FolderOrder::Building(kind.clone()),
    }
}

/// A GameParams `Building` a building model is the body of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildingRow {
    pub id: GameParamId,
    /// The game's name for the building, or its GameParams name.
    pub display_name: String,
    /// The GameParams name, e.g. `PCBX001`.
    pub param_name: String,
}

/// The buildings the viewer can open, by the hull model that is their body.
#[derive(Debug, Default)]
pub struct ModelBuildings(HashMap<ModelPath, Vec<BuildingRow>>);

impl ModelBuildings {
    pub fn new(by_model: HashMap<ModelPath, Vec<BuildingRow>>) -> Self {
        Self(by_model)
    }

    /// Moves out the buildings `model` is the body of.
    fn take(&mut self, model: &ModelPath) -> Vec<BuildingRow> {
        self.0.remove(model).unwrap_or_default()
    }
}

/// Every building in `index`, which holds those with armor in their hull or
/// their gun mounts, by its hull model.
pub fn list_model_buildings(index: &BuildingArmorIndex, metadata: &GameMetadataProvider) -> ModelBuildings {
    let mut by_model: HashMap<ModelPath, Vec<BuildingRow>> = HashMap::new();
    for building in index.buildings() {
        let Some(model) = building.model() else {
            continue;
        };
        let param_name = building.param().name().to_string();
        let display_name = building_display_name(&param_name, metadata);
        by_model.entry(model.clone()).or_default().push(BuildingRow { id: building.id(), display_name, param_name });
    }

    ModelBuildings(by_model)
}

/// The game's name for a building, or its GameParams name.
fn building_display_name(param_name: &str, metadata: &GameMetadataProvider) -> String {
    let key = wowsunpack::data::TranslationKey::new(format!("IDS_{}", param_name.to_uppercase()));
    metadata.localized_name_from_id(&key).unwrap_or_else(|| param_name.to_string())
}

/// A building armored in its own hull, which the Ships section lists under
/// Fortifications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FortificationRow {
    pub id: GameParamId,
    pub kind: BuildingKind,
    /// The game's name for the kind, where it has one.
    pub kind_name: Option<String>,
    /// The game's name for the building, or its GameParams name.
    pub display_name: String,
    /// The GameParams name, e.g. `PCBX001`.
    pub param_name: String,
    /// The name, GameParams name and id.
    pub filter_fields: FilterFields,
    /// Whether the install ships the hull's geometry; `None` when unknown.
    pub geometry: Option<GeometryAvailability>,
}

/// Every building armored in its own hull, by kind then name as `index`
/// lists them. One armored only through its gun mounts is listed under
/// Models > Buildings alone.
pub fn list_fortifications(
    index: &BuildingArmorIndex,
    catalog: &ModelCatalog,
    metadata: &GameMetadataProvider,
) -> Vec<FortificationRow> {
    index
        .buildings()
        .iter()
        .filter(|building| building.hull_has_armor())
        .map(|building| {
            let param = building.param();
            let kind = param.kind().clone();
            let kind_name = kind.translation_key().and_then(|key| metadata.localized_name_from_id(&key));
            let display_name = building_display_name(param.name(), metadata);

            let mut filter_fields = FilterFields::new();
            filter_fields.push(display_name.clone());
            filter_fields.push(param.name());
            filter_fields.push(building.id().to_string());

            // A building without a hull model has nothing to draw.
            let geometry = match building.model() {
                Some(model) => catalog.geometry(model),
                None => Some(GeometryAvailability::Absent),
            };

            FortificationRow {
                id: building.id(),
                kind,
                kind_name,
                display_name,
                param_name: param.name().to_string(),
                filter_fields,
                geometry,
            }
        })
        .collect()
}

/// Lists the parts of every ship in `ships` on its default hull, the first
/// hull upgrade by name, which is the hull a ship opens on. The sidebar does
/// not see a pane's hull selection, so a mount only an upgraded hull carries
/// is not listed. A ship whose parts cannot be listed has no entry, and its
/// row stays a plain ship row.
pub fn list_all_ship_parts(
    ships: &ShipCatalog,
    metadata: &GameMetadataProvider,
    library: &ModelLibrary,
    references: &ModelReferences,
) -> HashMap<VehicleParamIndex, ShipPartRows> {
    let no_overrides = HashMap::new();
    let mut keys = PartKeys::new(references);
    let mut parts = HashMap::new();
    for ship in ships.ships() {
        let Some(param) = metadata.game_param_by_index(&ship.param_index) else {
            continue;
        };
        let Some(vehicle) = param.vehicle() else {
            continue;
        };

        // `None` is the default hull, as for a ship load with no hull selected.
        match library.list_ship_parts(vehicle, None, &no_overrides) {
            Ok(listing) => {
                let rows = ShipPartRows::from_listing(&listing, library.catalog(), &mut keys);
                parts.insert(VehicleParamIndex::new(ship.param_index.clone()), rows);
            }
            Err(report) => {
                tracing::debug!("model viewer: the parts of {} could not be listed: {report:?}", ship.param_index);
            }
        }
    }

    parts
}

/// What a mount is found by: the game's name for it, where it has one, and
/// its GameParams index, name and id.
fn mount_fields(metadata: &GameMetadataProvider, param: &MountParam) -> FilterFields {
    let name_key = wowsunpack::data::TranslationKey::new(format!("IDS_{}", param.name().to_uppercase()));
    let mut key = FilterFields::new();
    if let Some(name) = metadata.localized_name_from_id(&name_key) {
        key.push(name);
    }
    key.push(param.index());
    key.push(param.name());
    key.push(param.id().to_string());

    key
}

/// A misc node behind the mount or hull section carrying it: `HP_x MP_y`,
/// `Bow MP_y`, or `MP_y` on the ship model itself. Sections and mounts repeat
/// node names, so the bare node does not tell the rows apart.
fn misc_label(misc: &ListedMisc, ship_model: Option<&ModelPath>) -> String {
    match &misc.parent {
        MiscParent::Mount(hp) => format!("{hp} {}", misc.node),
        MiscParent::HullSection(section) if Some(section) == ship_model => misc.node.to_string(),
        MiscParent::HullSection(section) => {
            let suffix = ship_model
                .and_then(|ship| section.stem().strip_prefix(ship.stem()))
                .and_then(|rest| rest.strip_prefix('_'))
                .unwrap_or(section.stem());
            format!("{suffix} {}", misc.node)
        }
    }
}

/// The folders an entry is filed in, top first, each with its key.
fn folder_path(group: &ModelGroup, ships: &ShipCatalog, metadata: &GameMetadataProvider) -> Vec<(String, FolderName)> {
    let group_folder = |kind: ModelGroupKind| (format!("{kind:?}"), FolderName::Group(kind));
    let directory = |name: &str| (name.to_string(), FolderName::Directory(name.to_string()));
    match group {
        ModelGroup::Gameplay(gameplay) => {
            let kind = (format!("{:?}", gameplay.kind()), FolderName::Kind(gameplay.kind()));
            let mut path = vec![nation_folder(gameplay.nation(), ships), kind];
            if let Some(sub_kind) = gameplay.sub_kind() {
                path.push((format!("{sub_kind:?}"), FolderName::SubKind(sub_kind.clone())));
            }

            path
        }
        ModelGroup::Building(kind) => {
            let display_name = kind.translation_key().and_then(|key| metadata.localized_name_from_id(&key));
            let folder = (format!("{kind:?}"), FolderName::Building { kind: kind.clone(), display_name });
            vec![group_folder(ModelGroupKind::Building), folder]
        }
        ModelGroup::Character(nation) => vec![group_folder(ModelGroupKind::Character), nation_folder(nation, ships)],
        ModelGroup::Location(scene) | ModelGroup::Port(scene) => {
            let mut path = vec![group_folder(group.kind()), directory(scene.category().as_str())];
            if let Some(sub_category) = scene.sub_category() {
                path.push(directory(sub_category.as_str()));
            }

            path
        }
        ModelGroup::Style(style) => vec![group_folder(ModelGroupKind::Style), directory(style.as_str())],
        ModelGroup::Other(Some(other)) => vec![group_folder(ModelGroupKind::Other), directory(other.as_str())],
        ModelGroup::Other(None) => vec![group_folder(ModelGroupKind::Other)],
    }
}

/// The folder for model directory `nation`, named and flagged after the
/// GameParams nation of the same name where there is one. The common
/// directory is not a nation and is named as [`FolderName::Common`].
fn nation_folder(nation: &ModelNation, ships: &ShipCatalog) -> (String, FolderName) {
    const COMMON: &str = "common";

    let key = format!("nation-{nation}");
    if nation.as_str() == COMMON {
        return (key, FolderName::Common);
    }

    let ship_nation = ships.nations.iter().find(|group| same_nation(nation.as_str(), &group.nation));
    let name = FolderName::Nation {
        flag: ship_nation.map(|group| group.nation.clone()),
        display_name: ship_nation.map_or_else(|| nation.as_str().to_string(), |group| group.display_name.clone()),
    };

    (key, name)
}

/// The folder of `siblings` keyed `key`, added when absent.
fn child_folder<'a>(siblings: &'a mut Vec<ModelFolder>, (key, name): &(String, FolderName)) -> &'a mut ModelFolder {
    let idx = match siblings.iter().position(|folder| folder.key == *key) {
        Some(idx) => idx,
        None => {
            siblings.push(ModelFolder {
                key: key.clone(),
                name: name.clone(),
                folders: Vec::new(),
                models: Vec::new(),
            });
            siblings.len() - 1
        }
    };

    &mut siblings[idx]
}

fn model_row(
    entry: &ModelEntry,
    catalog: &ModelCatalog,
    references: &ModelReferences,
    buildings: &mut ModelBuildings,
) -> ModelRow {
    let variants = entry
        .variants()
        .iter()
        .map(|variant| {
            // A skin is its own catalogue entry with its own wreck and hull
            // sections; its skins are left out so the nesting ends here.
            let variants = match variant.kind {
                ModelVariantKind::Skin => catalog
                    .entry(&variant.path)
                    .into_iter()
                    .flat_map(ModelEntry::variants)
                    .filter(|nested| nested.kind != ModelVariantKind::Skin)
                    .map(|nested| variant_row(&nested.kind, &nested.path, Vec::new(), catalog))
                    .collect(),
                ModelVariantKind::Dead | ModelVariantKind::Section(_) => Vec::new(),
            };
            variant_row(&variant.kind, &variant.path, variants, catalog)
        })
        .collect();

    // A building model is also found by the `Building` params using it, and
    // by the names and ids of the buildings listed under it.
    let mut filter_fields = references.model_key(entry.path());
    for building in entry.building_params() {
        filter_fields.push(building.name());
    }
    let buildings = buildings.take(entry.path());
    for building in &buildings {
        filter_fields.push(building.display_name.clone());
        filter_fields.push(building.param_name.clone());
        filter_fields.push(building.id.to_string());
    }

    ModelRow {
        path: entry.path().clone(),
        filter_fields,
        variants,
        geometry: catalog.geometry(entry.path()),
        buildings,
    }
}

fn variant_row(
    kind: &ModelVariantKind,
    path: &ModelPath,
    variants: Vec<VariantRow>,
    catalog: &ModelCatalog,
) -> VariantRow {
    VariantRow {
        kind: kind.clone(),
        path: path.clone(),
        filter_key: FilterKey::new(path.stem()),
        variants,
        geometry: catalog.geometry(path),
    }
}

/// Whether model directory `directory` (`usa`, `uk`, `panasia`) names
/// GameParams nation `ship_nation` (`USA`, `United_Kingdom`, `Pan_Asia`).
fn same_nation(directory: &str, ship_nation: &str) -> bool {
    let folded: String = ship_nation.chars().filter(|c| *c != '_').map(|c| c.to_ascii_lowercase()).collect();

    match directory {
        "uk" => folded == "unitedkingdom",
        _ => folded == directory,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_directories_match_game_params_nations() {
        assert!(same_nation("usa", "USA"));
        assert!(same_nation("uk", "United_Kingdom"));
        assert!(same_nation("panasia", "Pan_Asia"));
        assert!(same_nation("panamerica", "Pan_America"));
        assert!(same_nation("commonwealth", "Commonwealth"));
        assert!(!same_nation("common", "Commonwealth"));
        assert!(!same_nation("events", "USA"));
    }

    /// Run the test binary with `WOWS_DIR=<install> <binary> --ignored default_hull`.
    #[test]
    #[ignore = "needs a game install named by WOWS_DIR"]
    fn ship_parts_follow_the_default_hull() {
        use std::collections::BTreeSet;
        use std::sync::Arc;

        use wowsunpack::export::ship::ShipAssets;
        use wowsunpack::export::ship::ShipExportOptions;
        use wowsunpack::game_data::AssetsBin;

        fn row_keys(rows: &ShipPartRows) -> BTreeSet<ShipPartKey> {
            let hull = rows.hull.iter().map(|row| ShipPartKey::HullSection(row.model.clone()));
            let mounts = rows.mounts.iter().flat_map(|group| &group.parts).map(|part| part.key.clone());
            let miscs = rows.misc.iter().map(|part| part.key.clone());

            hull.chain(mounts).chain(miscs).collect()
        }

        let Some(dir) = std::env::var_os("WOWS_DIR").map(std::path::PathBuf::from) else {
            eprintln!("skipped: WOWS_DIR is not set");
            return;
        };
        let vfs = wowsunpack::game_data::build_game_vfs(&dir, AssetsBin::Omit).expect("game vfs");
        let mut bytes = Vec::new();
        let mut file = vfs.join("content/assets.bin").expect("a path").open_file().expect("assets.bin opens");
        std::io::Read::read_to_end(&mut file, &mut bytes).expect("assets.bin reads");
        let bytes = Arc::new(bytes);
        let metadata = Arc::new(GameMetadataProvider::from_vfs(&vfs).expect("GameParams"));
        let library = ModelLibrary::new(vfs.clone(), Arc::clone(&bytes), &metadata).expect("model library");
        let assets = ShipAssets::from_parts(&vfs, Arc::clone(&metadata), bytes);
        let ships = ShipCatalog::build(&metadata);
        let listed = list_all_ship_parts(&ships, &metadata, &library, &ModelReferences::new());

        let no_overrides = HashMap::new();
        let options = ShipExportOptions { textures: false, ..ShipExportOptions::default() };
        let mut checked = Vec::new();
        for ship in ships.ships() {
            if checked.len() == 5 {
                break;
            }

            let Some(param) = metadata.game_param_by_index(&ship.param_index) else {
                continue;
            };
            let Some(vehicle) = param.vehicle() else {
                continue;
            };
            let Some(rows) = listed.get(&VehicleParamIndex::new(ship.param_index.clone())) else {
                continue;
            };
            let listed_keys = row_keys(rows);

            // Only a ship whose hulls mount different parts shows which hull was listed.
            let hulls_differ = vehicle.hull_upgrades().into_iter().flat_map(|upgrades| upgrades.keys()).any(|hull| {
                library
                    .list_ship_parts(vehicle, Some(hull.as_str()), &no_overrides)
                    .is_ok_and(|listing| listing.keys().collect::<BTreeSet<_>>() != listed_keys)
            });
            if !hulls_differ {
                continue;
            }

            let Ok(loaded) = assets.load_ship_from_vehicle(vehicle, &options) else {
                continue;
            };
            let loaded_keys: BTreeSet<ShipPartKey> = loaded.parts().iter().map(|part| part.key.clone()).collect();
            assert_eq!(listed_keys, loaded_keys, "{} lists the parts it opens with", ship.param_index);
            checked.push(ship.param_index.clone());
        }

        eprintln!("checked {checked:?}");
        assert!(!checked.is_empty(), "some ship mounts different parts on another hull");
    }
}
