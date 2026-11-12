//! Builds the sidebar's tree items: the ships section, a "Models" heading,
//! then the model catalogue.
//!
//! Only expanded folders get their real children. A collapsed folder carries
//! one placeholder child so it still draws as a folder, and the sidebar
//! rebuilds when it is expanded. This keeps a rebuild proportional to what is
//! on screen rather than to the 15000-row catalogue.
//!
//! A search is matched once into [`SearchMatches`]. Folders without matches
//! are dropped and the rest show their match counts. A section with few
//! matches has its folders opened; a broad search leaves them closed.

use std::collections::HashMap;
use std::collections::HashSet;

use gpui_kit::SharedString;
use gpui_kit::component::tree::TreeItem;
use rust_i18n::t;

use wowsunpack::export::model_catalog::ModelKind;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_catalog::ModelSubKind;
use wowsunpack::export::model_catalog::ModelVariantKind;
use wowsunpack::export::ship_parts::MiscParent;
use wowsunpack::game_params::types::Species;

use super::catalog::NationGroup;
use super::catalog::ShipCatalog;
use super::catalog::ShipEntry;
use super::catalog::species_name;
use super::catalog::tier_roman;
use super::model_refs::OpenTarget;
use super::model_refs::ShipPartKey;
use super::model_refs::VehicleParamIndex;
use super::model_tree::HullRow;
use super::model_tree::ModelKindGroup;
use super::model_tree::ModelMatch;
use super::model_tree::ModelNationGroup;
use super::model_tree::ModelRow;
use super::model_tree::ModelSubKindGroup;
use super::model_tree::ModelTreeIndex;
use super::model_tree::MountGroup;
use super::model_tree::PartRow;
use super::model_tree::ShipPartRows;
use super::model_tree::VariantRow;
use super::search_key::SearchKey;

pub(super) const SEPARATOR_ID: &str = "armor-models-separator";
const MODELS_STATUS_ID: &str = "armor-models-status";

/// What a tree row id refers to, consulted by the row renderer, the click
/// handler and the context menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RowKind {
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
        node: Option<SharedString>,
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
}

/// Where the model section stands.
pub(super) enum ModelSection<'a> {
    Loading,
    Unavailable,
    Ready(&'a ModelTreeIndex),
}

pub(super) struct TreeSources<'a> {
    pub ships: &'a ShipCatalog,
    pub ship_parts: &'a HashMap<VehicleParamIndex, ShipPartRows>,
    pub models: ModelSection<'a>,
}

/// Which folders are expanded. With `auto` set, every folder a search wants
/// open is opened and recorded in `set`, except those in `closed`.
pub(super) struct Expansion<'a> {
    pub set: &'a mut HashSet<SharedString>,
    /// Folders the user closed during the search.
    pub closed: &'a HashSet<SharedString>,
    pub auto: bool,
}

impl Expansion<'_> {
    fn resolve(&mut self, id: &SharedString, wanted: bool) -> bool {
        if self.auto && wanted && !self.closed.contains(id) {
            self.set.insert(id.clone());
            return true;
        }

        self.set.contains(id)
    }
}

pub(super) struct BuiltTree {
    pub items: Vec<TreeItem>,
    pub rows: HashMap<SharedString, RowKind>,
    /// During a search, the field a row was found by where it is not the
    /// row's own name.
    pub notes: HashMap<SharedString, SharedString>,
}

/// What a search names, worked out once per query and per change to the
/// listed ships or models. Opening or closing a folder during the search
/// reuses it and tests no name again.
pub(super) struct SearchMatches {
    /// Lowercased and non-empty.
    query: String,
    ships: ShipMatches,
    /// `None` until the model catalogue is listed.
    models: Option<ModelMatches>,
}

/// Ships by nation and class, in [`ShipCatalog`] order.
type ShipMatches = MatchGroup<MatchGroup<MatchGroup<ShipMatch>>>;

/// Models by nation, kind and sub-kind, in [`ModelTreeIndex`] order.
type ModelMatches = MatchGroup<MatchGroup<MatchGroup<MatchGroup<ModelMatch>>>>;

impl SearchMatches {
    /// `None` for an empty `query`, which lists everything. `query` is
    /// already lowercased.
    pub(super) fn from_query(sources: &TreeSources<'_>, query: &str) -> Option<Self> {
        if query.is_empty() {
            return None;
        }

        let ships = sources
            .ships
            .nations
            .iter()
            .map(|nation| {
                nation
                    .classes
                    .iter()
                    .map(|class| class.ships.iter().map(|ship| ship_match(ship, sources.ship_parts, query)).collect())
                    .collect()
            })
            .collect();

        let models = match sources.models {
            ModelSection::Ready(index) => Some(
                index
                    .nations
                    .iter()
                    .map(|nation| {
                        nation
                            .kinds
                            .iter()
                            .map(|kind| {
                                kind.sub_kinds
                                    .iter()
                                    .map(|sub_kind| sub_kind.models.iter().map(|model| model.search(query)).collect())
                                    .collect()
                            })
                            .collect()
                    })
                    .collect(),
            ),
            ModelSection::Loading | ModelSection::Unavailable => None,
        };

        Some(Self { query: query.to_string(), ships, models })
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

fn ship_match(ship: &ShipEntry, ship_parts: &HashMap<VehicleParamIndex, ShipPartRows>, query: &str) -> ShipMatch {
    if ship.search_key.matches(query) {
        return ShipMatch::Name;
    }

    let parts = ship_parts.get(ship.param_index.as_str());
    if parts.is_some_and(|parts| parts.any_matches(query)) {
        return ShipMatch::Parts;
    }

    ShipMatch::Miss
}

/// The matches under one folder, in its listing order.
pub(super) struct MatchGroup<T> {
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

/// Builds the tree. `search` is `None` when listing everything.
pub(super) fn build_tree(
    sources: &TreeSources<'_>,
    search: Option<&SearchMatches>,
    expansion: &mut Expansion<'_>,
) -> BuiltTree {
    let mut builder = Builder { search, rows: HashMap::new(), notes: HashMap::new(), expansion, sources };
    let mut items = Vec::new();

    let ship_matches = search.map(|search| &search.ships);
    for (nation, matches) in paired(&sources.ships.nations, ship_matches) {
        if shown(matches) {
            items.push(builder.ship_nation(nation, matches));
        }
    }

    builder.rows.insert(SEPARATOR_ID.into(), RowKind::Separator);
    items.push(TreeItem::new(SEPARATOR_ID, t!("ui.models.heading").into_owned()).disabled(true));

    match sources.models {
        ModelSection::Loading => items.push(builder.status_row(t!("ui.models.loading").into_owned())),
        ModelSection::Unavailable => items.push(builder.status_row(t!("ui.models.unavailable").into_owned())),
        ModelSection::Ready(index) => {
            let model_matches = search.and_then(|search| search.models.as_ref());
            for (nation, matches) in paired(&index.nations, model_matches) {
                if shown(matches) {
                    items.push(builder.model_nation(nation, matches));
                }
            }
        }
    }

    BuiltTree { items, rows: builder.rows, notes: builder.notes }
}

/// The row id `open` is drawn on.
pub(super) fn open_row_id(open: &OpenTarget) -> SharedString {
    match open {
        OpenTarget::Ship(ship) => ship_row_id(ship.as_str()),
        OpenTarget::Model { model, from_ship: None } => model_row_id(model),
        OpenTarget::Model { from_ship: Some(part), .. } => part_row_id(&part.vehicle_param_index, &part.part),
    }
}

pub(super) fn ship_row_id(param_index: &str) -> SharedString {
    format!("armor-ship-{param_index}").into()
}

pub(super) fn model_row_id(model: &ModelPath) -> SharedString {
    format!("model-{model}").into()
}

pub(super) fn part_row_id(ship: &VehicleParamIndex, part: &ShipPartKey) -> SharedString {
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

pub(super) fn model_kind_label(kind: ModelKind) -> String {
    let key = match kind {
        ModelKind::Ship => "ui.models.kind.ship",
        ModelKind::Gun => "ui.models.kind.gun",
        ModelKind::Director => "ui.models.kind.director",
        ModelKind::Finder => "ui.models.kind.finder",
        ModelKind::Radar => "ui.models.kind.radar",
        ModelKind::Catapult => "ui.models.kind.catapult",
        ModelKind::Aircraft => "ui.models.kind.aircraft",
        ModelKind::Projectile => "ui.models.kind.projectile",
        ModelKind::Misc => "ui.models.kind.misc",
        ModelKind::Cap => "ui.models.kind.cap",
        ModelKind::Other => "ui.models.kind.other",
    };

    t!(key).into_owned()
}

pub(super) fn model_sub_kind_label(sub_kind: &ModelSubKind) -> String {
    let key = match sub_kind {
        ModelSubKind::Battleship => "ui.models.sub_kind.battleship",
        ModelSubKind::Cruiser => "ui.models.sub_kind.cruiser",
        ModelSubKind::Destroyer => "ui.models.sub_kind.destroyer",
        ModelSubKind::AircraftCarrier => "ui.models.sub_kind.aircraft_carrier",
        ModelSubKind::Submarine => "ui.models.sub_kind.submarine",
        ModelSubKind::Auxiliary => "ui.models.sub_kind.auxiliary",
        ModelSubKind::MainBattery => "ui.models.sub_kind.main_battery",
        ModelSubKind::SecondaryBattery => "ui.models.sub_kind.secondary_battery",
        ModelSubKind::AntiAircraft => "ui.models.sub_kind.anti_aircraft",
        ModelSubKind::TorpedoTubes => "ui.models.sub_kind.torpedo_tubes",
        ModelSubKind::DepthChargeLauncher => "ui.models.sub_kind.depth_charge_launcher",
        ModelSubKind::MissileLauncher => "ui.models.sub_kind.missile_launcher",
        ModelSubKind::SearchRadar => "ui.models.sub_kind.search_radar",
        ModelSubKind::FireControlRadar => "ui.models.sub_kind.fire_control_radar",
        ModelSubKind::Fighter => "ui.models.sub_kind.fighter",
        ModelSubKind::Bomber => "ui.models.sub_kind.bomber",
        ModelSubKind::DiveBomber => "ui.models.sub_kind.dive_bomber",
        ModelSubKind::Scout => "ui.models.sub_kind.scout",
        ModelSubKind::Torpedo => "ui.models.sub_kind.torpedo",
        ModelSubKind::Bomb => "ui.models.sub_kind.bomb",
        ModelSubKind::Rocket => "ui.models.sub_kind.rocket",
        ModelSubKind::DepthCharge => "ui.models.sub_kind.depth_charge",
        ModelSubKind::Mine => "ui.models.sub_kind.mine",
        ModelSubKind::Missile => "ui.models.sub_kind.missile",
        ModelSubKind::Jato => "ui.models.sub_kind.jato",
        ModelSubKind::Shell => "ui.models.sub_kind.shell",
        // The directory name is the only name the game gives it.
        ModelSubKind::Unrecognized(directory) => return directory.clone(),
    };

    t!(key).into_owned()
}

pub(super) fn mount_group_label(group: MountGroup) -> String {
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
    rows: HashMap<SharedString, RowKind>,
    notes: HashMap<SharedString, SharedString>,
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
        id: SharedString,
        label: impl Into<SharedString>,
        kind: RowKind,
        wanted: bool,
        children: impl FnOnce(&mut Self) -> Vec<TreeItem>,
    ) -> TreeItem {
        self.rows.insert(id.clone(), kind);
        let item = TreeItem::new(id.clone(), label);
        if !self.expansion.resolve(&id, wanted) {
            return item.child(TreeItem::new(format!("{id}/placeholder"), ""));
        }

        item.children(children(self)).expanded(true)
    }

    fn leaf(&mut self, id: SharedString, label: impl Into<SharedString>, kind: RowKind) -> TreeItem {
        self.rows.insert(id.clone(), kind);
        TreeItem::new(id, label)
    }

    fn status_row(&mut self, label: String) -> TreeItem {
        self.leaf(MODELS_STATUS_ID.into(), label, RowKind::ModelsStatus).disabled(true)
    }

    /// Records the field of `key` the search found row `id` by, unless it is
    /// the first, which is the name the row shows.
    fn note(&mut self, id: &SharedString, key: &SearchKey) {
        let Some(search) = self.search else {
            return;
        };
        let Some(field) = key.matched_field(&search.query) else {
            return;
        };
        if key.fields().next() == Some(field) {
            return;
        }

        self.notes.insert(id.clone(), field.to_string().into());
    }

    fn ship_nation(&mut self, nation: &NationGroup, matches: Option<&MatchGroup<MatchGroup<ShipMatch>>>) -> TreeItem {
        let id: SharedString = format!("armor-nation-{}", nation.nation).into();
        let label = counted(nation.display_name.clone(), matches);
        let wanted = self.opens_ships();
        self.folder(id, label, RowKind::Nation(nation.nation.clone()), wanted, |b| {
            let mut items = Vec::new();
            for (class, matches) in paired(&nation.classes, matches) {
                if !shown(matches) {
                    continue;
                }

                let id: SharedString = format!("armor-class-{}-{:?}", nation.nation, class.species).into();
                let label = counted(species_name(&class.species), matches);
                items.push(b.folder(id, label, RowKind::Class(class.species), wanted, |b| {
                    paired(&class.ships, matches)
                        .filter(|(_, found)| shown(*found))
                        .map(|(ship, found)| b.ship_row(ship, found.copied()))
                        .collect()
                }));
            }

            items
        })
    }

    /// `found` is `None` when listing everything.
    fn ship_row(&mut self, ship: &ShipEntry, found: Option<ShipMatch>) -> TreeItem {
        let id = ship_row_id(&ship.param_index);
        let label = format!("{} {}", tier_roman(ship.tier), ship.display_name);
        let kind = RowKind::Ship { param_index: ship.param_index.clone(), display_name: ship.display_name.clone() };
        if found == Some(ShipMatch::Name) {
            self.note(&id, &ship.search_key);
        }

        let vehicle = VehicleParamIndex::new(ship.param_index.clone());
        let ship_parts = self.sources.ship_parts;
        let Some(parts) = ship_parts.get(&vehicle).filter(|parts| !parts.is_empty()) else {
            return self.leaf(id, label, kind);
        };

        // A search that names only parts opens the ship to list them; one
        // that names the ship leaves its parts folded.
        let part_query = match found {
            Some(ShipMatch::Parts) => self.search.map(|search| search.query.as_str()),
            Some(ShipMatch::Name | ShipMatch::Miss) | None => None,
        };
        let wanted = part_query.is_some() && self.opens_ship_parts();
        self.folder(id, label, kind, wanted, |b| b.ship_parts(&vehicle, parts, part_query))
    }

    /// `query` lists only the parts it names, in opened folders; `None` lists
    /// every part.
    fn ship_parts(&mut self, ship: &VehicleParamIndex, parts: &ShipPartRows, query: Option<&str>) -> Vec<TreeItem> {
        let listed = |key: &SearchKey| query.is_none_or(|query| key.matches(query));
        let wanted = query.is_some() && self.opens_ship_parts();
        let mut items = Vec::new();

        let hull: Vec<&HullRow> = parts.hull.iter().filter(|row| listed(&row.search_key)).collect();
        if !hull.is_empty() {
            let id: SharedString = format!("armor-part-{ship}-group-hull").into();
            items.push(self.folder(id, t!("ui.models.part.hull").into_owned(), RowKind::PartFolder, wanted, |b| {
                hull.iter().map(|row| b.hull_row(ship, row)).collect()
            }));
        }

        for group in &parts.mounts {
            let rows: Vec<&PartRow> = group.parts.iter().filter(|part| listed(&part.search_key)).collect();
            if rows.is_empty() {
                continue;
            }

            let id: SharedString = format!("armor-part-{ship}-group-{:?}", group.group).into();
            items.push(self.folder(id, mount_group_label(group.group), RowKind::PartFolder, wanted, |b| {
                rows.iter().map(|part| b.part_row(ship, part)).collect()
            }));
        }

        let misc: Vec<&PartRow> = parts.misc.iter().filter(|part| listed(&part.search_key)).collect();
        if !misc.is_empty() {
            let id: SharedString = format!("armor-part-{ship}-group-misc").into();
            items.push(self.folder(id, t!("ui.models.part.misc").into_owned(), RowKind::PartFolder, wanted, |b| {
                misc.iter().map(|part| b.part_row(ship, part)).collect()
            }));
        }

        items
    }

    fn hull_row(&mut self, ship: &VehicleParamIndex, row: &HullRow) -> TreeItem {
        let part = ShipPartKey::HullSection(row.model.clone());
        let id = part_row_id(ship, &part);
        let kind = RowKind::ShipPart { ship: ship.clone(), part, model: row.model.clone(), node: None };
        self.note(&id, &row.search_key);

        self.leaf(id, row.model.stem().to_string(), kind)
    }

    fn part_row(&mut self, ship: &VehicleParamIndex, part: &PartRow) -> TreeItem {
        let id = part_row_id(ship, &part.key);
        let kind = RowKind::ShipPart {
            ship: ship.clone(),
            part: part.key.clone(),
            model: part.model.clone(),
            node: Some(part.node.as_str().to_string().into()),
        };
        self.note(&id, &part.search_key);

        self.leaf(id, format!("{} {}", part.node, part.model.stem()), kind)
    }

    fn model_nation(
        &mut self,
        nation: &ModelNationGroup,
        matches: Option<&MatchGroup<MatchGroup<MatchGroup<ModelMatch>>>>,
    ) -> TreeItem {
        let id: SharedString = format!("model-nation-{}", nation.nation).into();
        let kind = RowKind::ModelNation { flag: nation.ship_nation.clone() };
        let label = counted(nation.display_name.clone(), matches);
        let wanted = self.opens_models();
        self.folder(id, label, kind, wanted, |b| {
            paired(&nation.kinds, matches)
                .filter(|(_, found)| shown(*found))
                .map(|(kind, found)| b.model_kind(nation, kind, found))
                .collect()
        })
    }

    fn model_kind(
        &mut self,
        nation: &ModelNationGroup,
        kind: &ModelKindGroup,
        matches: Option<&MatchGroup<MatchGroup<ModelMatch>>>,
    ) -> TreeItem {
        let id: SharedString = format!("model-kind-{}-{:?}", nation.nation, kind.kind).into();
        let label = counted(model_kind_label(kind.kind), matches);
        let wanted = self.opens_models();
        self.folder(id, label, RowKind::ModelGroup, wanted, |b| {
            let mut items = Vec::new();
            for (sub_kind, matches) in paired(&kind.sub_kinds, matches) {
                if !shown(matches) {
                    continue;
                }

                match &sub_kind.sub_kind {
                    Some(name) => items.push(b.model_sub_kind(nation, kind, sub_kind, name, matches)),
                    None => items.extend(b.model_rows(&sub_kind.models, matches)),
                }
            }

            items
        })
    }

    fn model_sub_kind(
        &mut self,
        nation: &ModelNationGroup,
        kind: &ModelKindGroup,
        sub_kind: &ModelSubKindGroup,
        name: &ModelSubKind,
        matches: Option<&MatchGroup<ModelMatch>>,
    ) -> TreeItem {
        let id: SharedString = format!("model-sub-{}-{:?}-{name:?}", nation.nation, kind.kind).into();
        let label = counted(model_sub_kind_label(name), matches);
        let wanted = self.opens_models();
        self.folder(id, label, RowKind::ModelGroup, wanted, |b| b.model_rows(&sub_kind.models, matches))
    }

    fn model_rows(&mut self, models: &[ModelRow], matches: Option<&MatchGroup<ModelMatch>>) -> Vec<TreeItem> {
        paired(models, matches)
            .filter(|(_, found)| shown(*found))
            .map(|(model, found)| self.model_row(model, found.copied()))
            .collect()
    }

    /// `found` is `None` when listing everything.
    fn model_row(&mut self, model: &ModelRow, found: Option<ModelMatch>) -> TreeItem {
        let id = model_row_id(&model.path);
        let label = model.path.stem().to_string();
        let kind = RowKind::Model(model.path.clone());
        if found == Some(ModelMatch::Name) {
            self.note(&id, &model.search_key);
        }

        if model.variants.is_empty() {
            return self.leaf(id, label, kind);
        }

        // A search that names only a variant opens the model to show it; one
        // that names the model leaves its variants folded.
        let base_matches = found.is_none_or(|found| found == ModelMatch::Name);
        let wanted = self.opens_models() && !base_matches;
        self.folder(id, label, kind, wanted, |b| b.variant_rows(&model.variants, base_matches))
    }

    /// `show_all` lists every variant, for a parent the search names itself.
    /// Variants are tested here rather than in [`SearchMatches`]: only those
    /// of a model being drawn are, so the cost follows the rows built.
    fn variant_rows(&mut self, variants: &[VariantRow], show_all: bool) -> Vec<TreeItem> {
        let query = self.search.map_or("", |search| search.query.as_str());
        let mut items = Vec::new();
        for variant in variants {
            let matches = variant.matches(query);
            if !show_all && !matches && !variant.any_variant_matches(query) {
                continue;
            }

            let id = model_row_id(&variant.path);
            let label = variant_label(&variant.kind, &variant.path);
            let kind = RowKind::Model(variant.path.clone());
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
fn paired<'a, I, M>(
    items: &'a [I],
    matches: Option<&'a MatchGroup<M>>,
) -> impl Iterator<Item = (&'a I, Option<&'a M>)> {
    let mut members = matches.map(|group| group.members.iter());
    items.iter().map(move |item| (item, members.as_mut().and_then(Iterator::next)))
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
#[cfg(test)]
pub(super) mod fixture {
    use std::collections::HashMap;

    use wowsunpack::export::hardpoint::HardpointName;
    use wowsunpack::export::model_catalog::ModelCatalog;
    use wowsunpack::export::model_catalog::ModelPath;
    use wowsunpack::export::part_group::PartGroup;
    use wowsunpack::export::ship_parts::ListedMisc;
    use wowsunpack::export::ship_parts::ListedMount;
    use wowsunpack::export::ship_parts::MiscParent;
    use wowsunpack::export::ship_parts::ShipPartListing;
    use wowsunpack::game_params::types::PeculiarityModel;
    use wowsunpack::game_params::types::Species;

    use super::super::catalog::ClassGroup;
    use super::super::catalog::NationGroup;
    use super::super::catalog::ShipCatalog;
    use super::super::catalog::ShipEntry;
    use super::super::model_refs::VehicleParamIndex;
    use super::super::model_tree::ModelReferences;
    use super::super::model_tree::PartKeys;
    use super::super::model_tree::ShipPartRows;
    use super::super::search_key::SearchKey;

    pub const IOWA_INDEX: &str = "PASB018";
    pub const IOWA: &str = "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model";
    pub const IOWA_TURRET: &str = "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model";
    const IOWA_SKIN: &str = "content/gameplay/usa/ship/battleship/ASB128_Iowa_Halloween/ASB128_Iowa_Halloween.model";
    const SECONDARY: &str = "content/gameplay/usa/gun/secondary/AGS010_5in38_Mk32_Mod12/AGS010_5in38_Mk32_Mod12.model";
    const DIRECTOR: &str = "content/gameplay/usa/director/AD001_Director_Mk37/AD001_Director_Mk37.model";
    const BOLLARD: &str = "content/gameplay/usa/misc/AM2061/AM2061.model";
    const YAMATO: &str = "content/gameplay/japan/ship/battleship/JSB018_Yamato_1944/JSB018_Yamato_1944.model";

    pub fn path(raw: &str) -> ModelPath {
        ModelPath::parse(raw).expect("a model path")
    }

    /// A ship as `ShipCatalog::build` lists it, with real GameParams data.
    fn ship(param_index: &str, short_name: &str, full_name: &str, param_name: &str, id: u64, tier: u32) -> ShipEntry {
        ShipEntry {
            param_index: param_index.to_string(),
            display_name: short_name.to_string(),
            search_name: short_name.to_lowercase(),
            search_key: key(&[short_name, full_name, param_index, param_name, &id.to_string()]),
            tier,
        }
    }

    fn key(fields: &[&str]) -> SearchKey {
        fields.iter().map(|field| field.to_string()).collect()
    }

    pub fn ships() -> ShipCatalog {
        let nation = |nation: &str, display_name: &str, ships: Vec<ShipEntry>| NationGroup {
            nation: nation.to_string(),
            display_name: display_name.to_string(),
            classes: vec![ClassGroup { species: Species::Battleship, ships }],
        };
        let yamato = ship("PJSB018", "Yamato", "Yamato", "PJSB018_Yamato_1944", 4276041424, 10);
        let south_carolina = ship("PASB001", "S. Carolina", "South Carolina", "PASB001_Michigan_1916", 4293867504, 3);
        let iowa = ship(IOWA_INDEX, "Iowa", "Iowa", "PASB018_Iowa_1944", 4276041712, 9);

        ShipCatalog {
            nations: vec![nation("Japan", "Japan", vec![yamato]), nation("USA", "U.S.A.", vec![south_carolina, iowa])],
        }
    }

    /// The ships' hull models and Iowa's mounts, with the game's names for
    /// the guns.
    pub fn references(ships: &ShipCatalog) -> ModelReferences {
        let ship_key = |index: &str| {
            ships.ships().find(|ship| ship.param_index == index).map(|ship| ship.search_key.clone()).expect("listed")
        };
        let mut references = ModelReferences::new();
        references.insert(IOWA, &ship_key(IOWA_INDEX));
        references.insert(YAMATO, &ship_key("PJSB018"));
        references
            .insert(IOWA_TURRET, &key(&["406 mm/50 Mk.7 in a turret", "PAGM034", "PAGM034_16in50_Mk7", "4258916336"]));
        references.insert(
            SECONDARY,
            &key(&["127 mm/38 Mk.12 on a Mk.32 mount", "PAGS010", "PAGS010_5in38_Mk32_Mod12", "4283885552"]),
        );
        references.insert(DIRECTOR, &key(&["PAD001", "PAD001_Director_Mk37", "4293063664"]));

        references
    }

    pub fn catalog() -> ModelCatalog {
        let paths = [
            IOWA,
            "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Bow.model",
            "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_MidBack.model",
            "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_MidFront.model",
            "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Stern.model",
            IOWA_SKIN,
            "content/gameplay/usa/ship/battleship/ASB128_Iowa_Halloween/ASB128_Iowa_Halloween_dead.model",
            IOWA_TURRET,
            "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7_dead.model",
            SECONDARY,
            DIRECTOR,
            BOLLARD,
            YAMATO,
            "content/gameplay/japan/gun/main/JGM178_460mm_45_Type94/JGM178_460mm_45_Type94.model",
            "content/gameplay/events/director/XD006_Lazarus_Director/XD006_Lazarus_Director.model",
        ];
        let skins = [PeculiarityModel { stock: IOWA.to_string(), replacement: IOWA_SKIN.to_string() }];

        ModelCatalog::from_paths(paths.map(path), &skins)
    }

    /// Iowa's parts; Yamato lists none.
    pub fn ship_parts(
        catalog: &ModelCatalog,
        references: &ModelReferences,
    ) -> HashMap<VehicleParamIndex, ShipPartRows> {
        let mount = |hp: &str, model: &str, group: PartGroup| ListedMount {
            hp: HardpointName::new(hp),
            model: path(model),
            species: None,
            group,
        };
        let listing = ShipPartListing {
            hull_sections: catalog
                .entry(&path(IOWA))
                .map(|entry| std::iter::once(entry.path().clone()).chain(entry.sections().cloned()).collect())
                .expect("Iowa is catalogued"),
            mounts: vec![
                mount("HP_AGM_1", IOWA_TURRET, PartGroup::MainBattery),
                mount("HP_AGM_2", IOWA_TURRET, PartGroup::MainBattery),
                mount("HP_AGS_1", SECONDARY, PartGroup::SecondaryBattery),
                mount("HP_AD_1", DIRECTOR, PartGroup::FireControl),
            ],
            miscs: vec![ListedMisc {
                node: HardpointName::new("MP_AM2061_1"),
                parent: MiscParent::HullSection(path(
                    "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Bow.model",
                )),
                model: path(BOLLARD),
            }],
        };

        let rows = ShipPartRows::from_listing(&listing, catalog, &mut PartKeys::new(references));

        HashMap::from([(VehicleParamIndex::new(IOWA_INDEX), rows)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Data {
        ships: ShipCatalog,
        index: ModelTreeIndex,
        parts: HashMap<VehicleParamIndex, ShipPartRows>,
    }

    fn data() -> Data {
        rust_i18n::set_locale("en");
        let ships = fixture::ships();
        let catalog = fixture::catalog();
        let references = fixture::references(&ships);
        let index = ModelTreeIndex::build(&catalog, &ships, &references);
        let parts = fixture::ship_parts(&catalog, &references);

        Data { ships, index, parts }
    }

    /// A catalogue far larger than a search opens: 3000 main battery guns in
    /// each of two nations.
    fn large_data() -> Data {
        rust_i18n::set_locale("en");
        let ships = fixture::ships();
        let guns = [("usa", "AGM"), ("japan", "JGM")].into_iter().flat_map(|(nation, prefix)| {
            (0..3000).map(move |idx| {
                fixture::path(&format!(
                    "content/gameplay/{nation}/gun/main/{prefix}{idx:04}_Gun/{prefix}{idx:04}_Gun.model"
                ))
            })
        });
        let catalog = wowsunpack::export::model_catalog::ModelCatalog::from_paths(guns, &[]);
        let index = ModelTreeIndex::build(&catalog, &ships, &super::super::model_tree::ModelReferences::new());

        Data { ships, index, parts: HashMap::new() }
    }

    fn build(data: &Data, query: &str, set: &mut HashSet<SharedString>, auto: bool) -> BuiltTree {
        let sources =
            TreeSources { ships: &data.ships, ship_parts: &data.parts, models: ModelSection::Ready(&data.index) };
        let search = SearchMatches::from_query(&sources, query);

        build_tree(&sources, search.as_ref(), &mut Expansion { set, closed: &HashSet::new(), auto })
    }

    fn is_placeholder(item: &TreeItem) -> bool {
        item.id.ends_with("/placeholder")
    }

    /// One line per row, indented by depth; a collapsed folder ends in `+`.
    fn outline(items: &[TreeItem]) -> Vec<String> {
        let mut lines = Vec::new();
        let mut pending: Vec<(&TreeItem, usize)> = items.iter().rev().map(|item| (item, 0)).collect();
        while let Some((item, depth)) = pending.pop() {
            let collapsed = item.children.iter().any(is_placeholder);
            let marker = if collapsed { " +" } else { "" };
            lines.push(format!("{}{}{marker}", "  ".repeat(depth), item.label));
            if !collapsed {
                pending.extend(item.children.iter().rev().map(|child| (child, depth + 1)));
            }
        }

        lines
    }

    fn collapsed_folders(items: &[TreeItem]) -> Vec<SharedString> {
        let mut collapsed = Vec::new();
        let mut pending: Vec<&TreeItem> = items.iter().collect();
        while let Some(item) = pending.pop() {
            if item.children.iter().any(is_placeholder) {
                collapsed.push(item.id.clone());
            }
            pending.extend(&item.children);
        }

        collapsed
    }

    fn expand_all(data: &Data) -> BuiltTree {
        let mut set = HashSet::new();
        loop {
            let built = build(data, "", &mut set, false);
            let collapsed = collapsed_folders(&built.items);
            if collapsed.is_empty() {
                return built;
            }
            set.extend(collapsed);
        }
    }

    fn search(data: &Data, query: &str) -> Vec<String> {
        outline(&build(data, query, &mut HashSet::new(), true).items)
    }

    #[test]
    fn folders_start_collapsed_with_ships_above_the_models_heading() {
        let data = data();

        let built = build(&data, "", &mut HashSet::new(), true);

        assert_eq!(outline(&built.items), ["Japan +", "U.S.A. +", "Models", "events +", "Japan +", "U.S.A. +"]);
        assert_eq!(built.rows.get(SEPARATOR_ID), Some(&RowKind::Separator));
    }

    #[test]
    fn ships_list_their_parts_and_models_nest_by_nation_kind_and_sub_kind() {
        let data = data();

        let built = expand_all(&data);

        let expected = [
            "Japan",
            "  Battleship",
            "    X Yamato",
            "U.S.A.",
            "  Battleship",
            "    III S. Carolina",
            "    IX Iowa",
            "      Hull",
            "        ASB028_Iowa_1945",
            "        ASB028_Iowa_1945_Bow",
            "        ASB028_Iowa_1945_MidBack",
            "        ASB028_Iowa_1945_MidFront",
            "        ASB028_Iowa_1945_Stern",
            "      Main battery",
            "        HP_AGM_1 AGM034_16in50_Mk7",
            "        HP_AGM_2 AGM034_16in50_Mk7",
            "      Secondaries",
            "        HP_AGS_1 AGS010_5in38_Mk32_Mod12",
            "      Directors",
            "        HP_AD_1 AD001_Director_Mk37",
            "      Misc",
            "        Bow MP_AM2061_1 AM2061",
            "Models",
            "events",
            "  Directors",
            "    XD006_Lazarus_Director",
            "Japan",
            "  Ships",
            "    Battleships",
            "      JSB018_Yamato_1944",
            "  Guns",
            "    Main battery",
            "      JGM178_460mm_45_Type94",
            "U.S.A.",
            "  Ships",
            "    Battleships",
            "      ASB028_Iowa_1945",
            "        Bow",
            "        MidBack",
            "        MidFront",
            "        Stern",
            "        ASB128_Iowa_Halloween",
            "          Wreck",
            "  Guns",
            "    Main battery",
            "      AGM034_16in50_Mk7",
            "        Wreck",
            "    Secondary battery",
            "      AGS010_5in38_Mk32_Mod12",
            "  Directors",
            "    AD001_Director_Mk37",
            "  Miscellaneous",
            "    AM2061",
        ];
        assert_eq!(outline(&built.items), expected);

        let turret = fixture::path(fixture::IOWA_TURRET);
        let mount = built.rows.get(&part_row_id(
            &VehicleParamIndex::new(fixture::IOWA_INDEX),
            &ShipPartKey::Mount(wowsunpack::export::hardpoint::HardpointName::new("HP_AGM_1")),
        ));
        assert!(
            matches!(mount, Some(RowKind::ShipPart { model, .. }) if *model == turret),
            "the mount row opens the turret model"
        );
    }

    #[test]
    fn a_search_filters_ships_and_models_and_opens_the_folders_holding_matches() {
        let data = data();

        assert_eq!(
            search(&data, "iowa"),
            [
                "U.S.A. (1)",
                "  Battleship (1)",
                "    IX Iowa +",
                "Models",
                "U.S.A. (1)",
                "  Ships (1)",
                "    Battleships (1)",
                "      ASB028_Iowa_1945 +",
            ]
        );
        assert_eq!(
            search(&data, "yamato"),
            [
                "Japan (1)",
                "  Battleship (1)",
                "    X Yamato",
                "Models",
                "Japan (1)",
                "  Ships (1)",
                "    Battleships (1)",
                "      JSB018_Yamato_1944"
            ]
        );
    }

    #[test]
    fn a_search_matches_model_names_and_opens_a_model_to_a_matching_variant() {
        let data = data();

        assert_eq!(search(&data, "lazarus"), ["Models", "events (1)", "  Directors (1)", "    XD006_Lazarus_Director"]);
        assert_eq!(
            search(&data, "mk7_dead"),
            ["Models", "U.S.A. (1)", "  Guns (1)", "    Main battery (1)", "      AGM034_16in50_Mk7", "        Wreck"]
        );
        assert_eq!(
            search(&data, "stern"),
            [
                "U.S.A. (1)",
                "  Battleship (1)",
                "    IX Iowa",
                "      Hull",
                "        ASB028_Iowa_1945_Stern",
                "Models",
                "U.S.A. (1)",
                "  Ships (1)",
                "    Battleships (1)",
                "      ASB028_Iowa_1945",
                "        Stern",
            ]
        );
        assert_eq!(
            search(&data, "halloween_dead"),
            [
                "Models",
                "U.S.A. (1)",
                "  Ships (1)",
                "    Battleships (1)",
                "      ASB028_Iowa_1945",
                "        ASB128_Iowa_Halloween",
                "          Wreck",
            ]
        );
    }

    #[test]
    fn a_search_naming_only_directories_matches_no_model() {
        let data = data();

        for query in ["content", "gameplay", "events/director", "gun/main", "battleship/asb028"] {
            assert_eq!(search(&data, query), ["Models"], "{query}");
        }
    }

    fn note(built: &BuiltTree, id: &SharedString) -> Option<String> {
        built.notes.get(id).map(|note| note.to_string())
    }

    #[test]
    fn a_search_finds_ships_and_their_hull_models_by_game_params_index_name_and_id() {
        let data = data();
        let iowa = ship_row_id(fixture::IOWA_INDEX);
        let iowa_model = model_row_id(&fixture::path(fixture::IOWA));

        for (query, field) in [("pasb018", "PASB018"), ("iowa_1944", "PASB018_Iowa_1944"), ("4276041712", "4276041712")]
        {
            let built = build(&data, query, &mut HashSet::new(), true);

            assert_eq!(
                outline(&built.items),
                [
                    "U.S.A. (1)",
                    "  Battleship (1)",
                    "    IX Iowa +",
                    "Models",
                    "U.S.A. (1)",
                    "  Ships (1)",
                    "    Battleships (1)",
                    "      ASB028_Iowa_1945 +",
                ],
                "{query}"
            );
            assert_eq!(note(&built, &iowa).as_deref(), Some(field), "{query} names the field it matched");
            assert_eq!(note(&built, &iowa_model).as_deref(), Some(field), "{query} on the hull model");
        }
    }

    #[test]
    fn a_search_finds_a_ship_by_its_full_and_its_short_name() {
        let data = data();
        let south_carolina = ship_row_id("PASB001");

        for query in ["south carolina", "s. carolina"] {
            let built = build(&data, query, &mut HashSet::new(), true);

            assert_eq!(
                outline(&built.items),
                ["U.S.A. (1)", "  Battleship (1)", "    III S. Carolina", "Models"],
                "{query}"
            );
        }

        let built = build(&data, "south carolina", &mut HashSet::new(), true);
        assert_eq!(note(&built, &south_carolina).as_deref(), Some("South Carolina"), "the full name is noted");
        let built = build(&data, "s. carolina", &mut HashSet::new(), true);
        assert_eq!(note(&built, &south_carolina), None, "the short name is the row's own");
    }

    #[test]
    fn a_search_finds_models_and_ship_parts_by_stem_and_by_the_mount_using_them() {
        let data = data();
        let turret = model_row_id(&fixture::path(fixture::IOWA_TURRET));
        let iowa = VehicleParamIndex::new(fixture::IOWA_INDEX);
        let mount =
            part_row_id(&iowa, &ShipPartKey::Mount(wowsunpack::export::hardpoint::HardpointName::new("HP_AGM_1")));
        let turret_rows = [
            "U.S.A. (1)",
            "  Battleship (1)",
            "    IX Iowa",
            "      Main battery",
            "        HP_AGM_1 AGM034_16in50_Mk7",
            "        HP_AGM_2 AGM034_16in50_Mk7",
            "Models",
            "U.S.A. (1)",
            "  Guns (1)",
            "    Main battery (1)",
            "      AGM034_16in50_Mk7 +",
        ];

        for (query, field) in [
            ("agm034", None),
            ("406 mm/50", Some("406 mm/50 Mk.7 in a turret")),
            ("pagm034", Some("PAGM034")),
            ("16in50_mk7", None),
            ("4258916336", Some("4258916336")),
        ] {
            let built = build(&data, query, &mut HashSet::new(), true);

            assert_eq!(outline(&built.items), turret_rows, "{query}");
            assert_eq!(note(&built, &turret).as_deref(), field, "{query} on the model");
            assert_eq!(note(&built, &mount).as_deref(), field, "{query} on the part");
        }

        assert_eq!(
            search(&data, "pad001_director"),
            [
                "U.S.A. (1)",
                "  Battleship (1)",
                "    IX Iowa",
                "      Directors",
                "        HP_AD_1 AD001_Director_Mk37",
                "Models",
                "U.S.A. (1)",
                "  Directors (1)",
                "    AD001_Director_Mk37",
            ],
            "a mount the game gives no name is found by its GameParams name"
        );
    }

    #[test]
    fn a_broad_search_leaves_its_folders_closed_with_their_counts() {
        let data = large_data();
        let mut set = HashSet::new();

        let built = build(&data, "gun", &mut set, true);

        assert_eq!(outline(&built.items), ["Models", "Japan (3000) +", "U.S.A. (3000) +"]);
        assert_eq!(built.rows.len(), 3, "the heading and two nations");
        assert!(set.is_empty(), "nothing was opened for the search");

        set.insert("model-nation-usa".into());
        let built = build(&data, "gun", &mut set, false);

        assert_eq!(
            outline(&built.items),
            ["Models", "Japan (3000) +", "U.S.A. (3000)", "  Guns (3000) +"],
            "opening a folder lists only its own children"
        );
    }

    #[test]
    fn a_narrow_search_in_a_large_catalogue_opens_the_folders_holding_its_matches() {
        let data = large_data();

        assert_eq!(
            search(&data, "agm0042_"),
            ["Models", "U.S.A. (1)", "  Guns (1)", "    Main battery (1)", "      AGM0042_Gun"]
        );
    }

    #[test]
    fn a_folder_closed_during_a_search_stays_closed_until_the_search_changes() {
        let data = data();
        let mut set = HashSet::new();
        build(&data, "agm034", &mut set, true);
        set.remove("armor-nation-USA");
        set.remove("model-nation-usa");

        let built = build(&data, "agm034", &mut set, false);

        assert_eq!(outline(&built.items), ["U.S.A. (1) +", "Models", "U.S.A. (1) +"]);
    }

    /// The real catalogue and every ship's parts. Run the test binary with
    /// `WOWS_DIR=<install> <binary> --ignored real_catalogue`.
    #[test]
    #[ignore = "needs a game install named by WOWS_DIR"]
    fn the_real_catalogue_lists_every_model_and_searches_quickly() {
        use std::sync::Arc;
        use std::time::Instant;

        use wowsunpack::export::model_library::ModelLibrary;
        use wowsunpack::game_data::AssetsBin;
        use wowsunpack::game_params::provider::GameMetadataProvider;

        use super::super::model_tree::ModelReferences;
        use super::super::model_tree::list_all_ship_parts;

        let Some(dir) = std::env::var_os("WOWS_DIR").map(std::path::PathBuf::from) else {
            eprintln!("skipped: WOWS_DIR is not set");
            return;
        };
        let vfs = wowsunpack::game_data::build_game_vfs(&dir, AssetsBin::Omit).expect("game vfs");
        let mut bytes = Vec::new();
        let mut file = vfs.join("content/assets.bin").expect("a path").open_file().expect("assets.bin opens");
        std::io::Read::read_to_end(&mut file, &mut bytes).expect("assets.bin reads");
        let metadata = GameMetadataProvider::from_vfs(&vfs).expect("GameParams");
        let latest_build = std::fs::read_dir(dir.join("bin"))
            .expect("bin lists builds")
            .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
            .max()
            .expect("a build");
        let texts = dir.join(format!("bin/{latest_build}/res/texts/en/LC_MESSAGES/global.mo"));
        let catalog = gettext::Catalog::parse(std::fs::File::open(texts).expect("global.mo opens")).expect("global.mo");
        metadata.set_translations(catalog);
        let library = ModelLibrary::new(vfs.clone(), Arc::new(bytes), &metadata).expect("model library");
        let ships = ShipCatalog::build(&metadata);

        let started = Instant::now();
        let references = ModelReferences::build(&ships, &metadata);
        eprintln!("[measured] model references in {:?}", started.elapsed());

        let started = Instant::now();
        let index = ModelTreeIndex::build(library.catalog(), &ships, &references);
        eprintln!("[measured] index of {} models in {:?}", index.model_count(), started.elapsed());
        assert_eq!(index.model_count(), library.catalog().listed_entries().count());

        let started = Instant::now();
        let parts = list_all_ship_parts(&ships, &metadata, &library, &references);
        let ship_count = ships.ships().count();
        eprintln!("[measured] parts of {}/{ship_count} ships in {:?}", parts.len(), started.elapsed());
        assert!(parts.len() * 10 >= ship_count * 9, "most ships list their parts");

        for nation in &index.nations {
            eprintln!("  {} -> {:?} ({})", nation.nation, nation.ship_nation, nation.display_name);
        }
        for directory in ["usa", "japan", "uk", "panasia", "panamerica", "commonwealth"] {
            let nation = index.nations.iter().find(|nation| nation.nation.as_str() == directory);
            assert!(nation.is_some_and(|nation| nation.ship_nation.is_some()), "{directory} has a flag");
        }

        let data = Data { ships, index, parts };
        let queries =
            ["", "a", "gun", "iowa", "agm", "pasb018", "south carolina", "406 mm/50", "pagm034", "4276041712"];
        for query in queries {
            let sources =
                TreeSources { ships: &data.ships, ship_parts: &data.parts, models: ModelSection::Ready(&data.index) };
            let started = Instant::now();
            let matches = SearchMatches::from_query(&sources, query);
            eprintln!("[measured] {query:?}: matched in {:?}", started.elapsed());
            drop(matches);

            let started = Instant::now();
            let built = build(&data, query, &mut HashSet::new(), true);
            eprintln!("[measured] {query:?}: {} rows built in {:?}", built.rows.len(), started.elapsed());
            let mut notes: Vec<String> = built.notes.iter().map(|(id, note)| format!("{id} [{note}]")).collect();
            notes.sort();
            for note in notes.iter().take(6) {
                eprintln!("    {note}");
            }
        }
        for query in ["a", "gun"] {
            let built = build(&data, query, &mut HashSet::new(), true);
            assert!(built.rows.len() < 100, "{query:?} builds {} rows", built.rows.len());
        }
    }
}
