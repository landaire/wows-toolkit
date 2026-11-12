//! Builds the sidebar's tree items: the ships section, a "Models" heading,
//! then the model catalogue.
//!
//! Only expanded folders get their real children. A collapsed folder carries
//! one placeholder child so it still draws as a folder, and the sidebar
//! rebuilds when it is expanded. This keeps a rebuild proportional to what is
//! on screen rather than to the 15000-row catalogue.

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
use super::model_tree::ModelKindGroup;
use super::model_tree::ModelNationGroup;
use super::model_tree::ModelRow;
use super::model_tree::ModelSubKindGroup;
use super::model_tree::ModelTreeIndex;
use super::model_tree::MountGroup;
use super::model_tree::PartRow;
use super::model_tree::ShipPartRows;
use super::model_tree::VariantRow;

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
/// open is opened and recorded in `set`.
pub(super) struct Expansion<'a> {
    pub set: &'a mut HashSet<SharedString>,
    pub auto: bool,
}

impl Expansion<'_> {
    fn resolve(&mut self, id: &SharedString, wanted: bool) -> bool {
        if self.auto && wanted {
            self.set.insert(id.clone());
            return true;
        }

        self.set.contains(id)
    }
}

pub(super) struct BuiltTree {
    pub items: Vec<TreeItem>,
    pub rows: HashMap<SharedString, RowKind>,
}

/// Builds the tree for `query`, which is already lowercased; empty lists
/// everything.
pub(super) fn build_tree(sources: &TreeSources<'_>, query: &str, expansion: &mut Expansion<'_>) -> BuiltTree {
    let mut builder = Builder { query, rows: HashMap::new(), expansion, sources };
    let mut items = Vec::new();

    for nation in &sources.ships.nations {
        if let Some(item) = builder.ship_nation(nation) {
            items.push(item);
        }
    }

    builder.rows.insert(SEPARATOR_ID.into(), RowKind::Separator);
    items.push(TreeItem::new(SEPARATOR_ID, t!("ui.models.heading").into_owned()).disabled(true));

    match sources.models {
        ModelSection::Loading => items.push(builder.status_row(t!("ui.models.loading").into_owned())),
        ModelSection::Unavailable => items.push(builder.status_row(t!("ui.models.unavailable").into_owned())),
        ModelSection::Ready(index) => {
            for nation in &index.nations {
                if let Some(item) = builder.model_nation(nation) {
                    items.push(item);
                }
            }
        }
    }

    BuiltTree { items, rows: builder.rows }
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
    query: &'q str,
    rows: HashMap<SharedString, RowKind>,
    expansion: &'e mut Expansion<'s>,
    sources: &'q TreeSources<'d>,
}

impl Builder<'_, '_, '_, '_> {
    fn searching(&self) -> bool {
        !self.query.is_empty()
    }

    fn ship_matches(&self, ship: &ShipEntry) -> bool {
        !self.searching() || ship.search_name.contains(self.query)
    }

    fn model_matches(&self, model: &ModelRow) -> bool {
        !self.searching() || model.matches(self.query) || model.any_variant_matches(self.query)
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

    fn ship_nation(&mut self, nation: &NationGroup) -> Option<TreeItem> {
        let has_match = nation.classes.iter().flat_map(|class| &class.ships).any(|ship| self.ship_matches(ship));
        if !has_match {
            return None;
        }

        let id: SharedString = format!("armor-nation-{}", nation.nation).into();
        let searching = self.searching();
        Some(self.folder(id, nation.display_name.clone(), RowKind::Nation(nation.nation.clone()), searching, |b| {
            let mut items = Vec::new();
            for class in &nation.classes {
                if !class.ships.iter().any(|ship| b.ship_matches(ship)) {
                    continue;
                }

                let id: SharedString = format!("armor-class-{}-{:?}", nation.nation, class.species).into();
                items.push(b.folder(id, species_name(&class.species), RowKind::Class(class.species), searching, |b| {
                    let query = b.query;
                    let ships = class.ships.iter().filter(|ship| query.is_empty() || ship.search_name.contains(query));
                    ships.map(|ship| b.ship_row(ship)).collect()
                }));
            }

            items
        }))
    }

    fn ship_row(&mut self, ship: &ShipEntry) -> TreeItem {
        let id = ship_row_id(&ship.param_index);
        let label = format!("{} {}", tier_roman(ship.tier), ship.display_name);
        let kind = RowKind::Ship { param_index: ship.param_index.clone(), display_name: ship.display_name.clone() };

        let vehicle = VehicleParamIndex::new(ship.param_index.clone());
        let ship_parts = self.sources.ship_parts;
        let Some(parts) = ship_parts.get(&vehicle).filter(|parts| !parts.is_empty()) else {
            return self.leaf(id, label, kind);
        };

        self.folder(id, label, kind, false, |b| b.ship_parts(&vehicle, parts))
    }

    fn ship_parts(&mut self, ship: &VehicleParamIndex, parts: &ShipPartRows) -> Vec<TreeItem> {
        let mut items = Vec::new();

        if !parts.hull.is_empty() {
            let id: SharedString = format!("armor-part-{ship}-group-hull").into();
            items.push(self.folder(id, t!("ui.models.part.hull").into_owned(), RowKind::PartFolder, false, |b| {
                parts.hull.iter().map(|model| b.hull_row(ship, model)).collect()
            }));
        }

        for group in &parts.mounts {
            let id: SharedString = format!("armor-part-{ship}-group-{:?}", group.group).into();
            items.push(self.folder(id, mount_group_label(group.group), RowKind::PartFolder, false, |b| {
                group.parts.iter().map(|part| b.part_row(ship, part)).collect()
            }));
        }

        if !parts.misc.is_empty() {
            let id: SharedString = format!("armor-part-{ship}-group-misc").into();
            items.push(self.folder(id, t!("ui.models.part.misc").into_owned(), RowKind::PartFolder, false, |b| {
                parts.misc.iter().map(|part| b.part_row(ship, part)).collect()
            }));
        }

        items
    }

    fn hull_row(&mut self, ship: &VehicleParamIndex, model: &ModelPath) -> TreeItem {
        let part = ShipPartKey::HullSection(model.clone());
        let id = part_row_id(ship, &part);
        let kind = RowKind::ShipPart { ship: ship.clone(), part, model: model.clone(), node: None };

        self.leaf(id, model.stem().to_string(), kind)
    }

    fn part_row(&mut self, ship: &VehicleParamIndex, part: &PartRow) -> TreeItem {
        let id = part_row_id(ship, &part.key);
        let kind = RowKind::ShipPart {
            ship: ship.clone(),
            part: part.key.clone(),
            model: part.model.clone(),
            node: Some(part.node.as_str().to_string().into()),
        };

        self.leaf(id, format!("{} {}", part.node, part.model.stem()), kind)
    }

    fn model_nation(&mut self, nation: &ModelNationGroup) -> Option<TreeItem> {
        let has_match = nation
            .kinds
            .iter()
            .flat_map(|kind| &kind.sub_kinds)
            .flat_map(|sub_kind| &sub_kind.models)
            .any(|model| self.model_matches(model));
        if !has_match {
            return None;
        }

        let id: SharedString = format!("model-nation-{}", nation.nation).into();
        let kind = RowKind::ModelNation { flag: nation.ship_nation.clone() };
        let searching = self.searching();
        Some(self.folder(id, nation.display_name.clone(), kind, searching, |b| {
            nation.kinds.iter().filter_map(|kind| b.model_kind(nation, kind)).collect()
        }))
    }

    fn model_kind(&mut self, nation: &ModelNationGroup, kind: &ModelKindGroup) -> Option<TreeItem> {
        let has_match =
            kind.sub_kinds.iter().flat_map(|sub_kind| &sub_kind.models).any(|model| self.model_matches(model));
        if !has_match {
            return None;
        }

        let id: SharedString = format!("model-kind-{}-{:?}", nation.nation, kind.kind).into();
        let searching = self.searching();
        Some(self.folder(id, model_kind_label(kind.kind), RowKind::ModelGroup, searching, |b| {
            let mut items = Vec::new();
            for sub_kind in &kind.sub_kinds {
                match &sub_kind.sub_kind {
                    Some(name) => {
                        if let Some(item) = b.model_sub_kind(nation, kind, sub_kind, name) {
                            items.push(item);
                        }
                    }
                    None => items.extend(b.model_rows(&sub_kind.models)),
                }
            }

            items
        }))
    }

    fn model_sub_kind(
        &mut self,
        nation: &ModelNationGroup,
        kind: &ModelKindGroup,
        sub_kind: &ModelSubKindGroup,
        name: &ModelSubKind,
    ) -> Option<TreeItem> {
        if !sub_kind.models.iter().any(|model| self.model_matches(model)) {
            return None;
        }

        let id: SharedString = format!("model-sub-{}-{:?}-{name:?}", nation.nation, kind.kind).into();
        let searching = self.searching();
        Some(
            self.folder(id, model_sub_kind_label(name), RowKind::ModelGroup, searching, |b| {
                b.model_rows(&sub_kind.models)
            }),
        )
    }

    fn model_rows(&mut self, models: &[ModelRow]) -> Vec<TreeItem> {
        let mut items = Vec::new();
        for model in models {
            if self.model_matches(model) {
                items.push(self.model_row(model));
            }
        }

        items
    }

    fn model_row(&mut self, model: &ModelRow) -> TreeItem {
        let id = model_row_id(&model.path);
        let label = model.path.stem().to_string();
        let kind = RowKind::Model(model.path.clone());
        if model.variants.is_empty() {
            return self.leaf(id, label, kind);
        }

        // A search that names only a variant opens the model to show it; one
        // that names the model leaves its variants folded.
        let base_matches = self.query.is_empty() || model.matches(self.query);
        self.folder(id, label, kind, !base_matches, |b| b.variant_rows(&model.variants, base_matches))
    }

    /// `show_all` lists every variant, for a parent the search names itself.
    fn variant_rows(&mut self, variants: &[VariantRow], show_all: bool) -> Vec<TreeItem> {
        let query = self.query;
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
            items.push(self.folder(id, label, kind, !show_nested, |b| b.variant_rows(&variant.variants, show_nested)));
        }

        items
    }
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
    use super::super::model_tree::ShipPartRows;

    pub const IOWA_INDEX: &str = "PASB018";
    pub const IOWA: &str = "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model";
    pub const IOWA_TURRET: &str = "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model";
    const IOWA_SKIN: &str = "content/gameplay/usa/ship/battleship/ASB128_Iowa_Halloween/ASB128_Iowa_Halloween.model";
    const SECONDARY: &str = "content/gameplay/usa/gun/secondary/AGS010_5in38_Mk32_Mod12/AGS010_5in38_Mk32_Mod12.model";
    const DIRECTOR: &str = "content/gameplay/usa/director/AD001_Director_Mk37/AD001_Director_Mk37.model";
    const BOLLARD: &str = "content/gameplay/usa/misc/AM2061/AM2061.model";

    pub fn path(raw: &str) -> ModelPath {
        ModelPath::parse(raw).expect("a model path")
    }

    fn ship(param_index: &str, display_name: &str, tier: u32) -> ShipEntry {
        ShipEntry {
            param_index: param_index.to_string(),
            display_name: display_name.to_string(),
            search_name: display_name.to_lowercase(),
            tier,
        }
    }

    pub fn ships() -> ShipCatalog {
        let nation = |nation: &str, display_name: &str, ship: ShipEntry| NationGroup {
            nation: nation.to_string(),
            display_name: display_name.to_string(),
            classes: vec![ClassGroup { species: Species::Battleship, ships: vec![ship] }],
        };

        ShipCatalog {
            nations: vec![
                nation("Japan", "Japan", ship("PJSB018", "Yamato", 10)),
                nation("USA", "U.S.A.", ship(IOWA_INDEX, "Iowa", 9)),
            ],
        }
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
            "content/gameplay/japan/ship/battleship/JSB018_Yamato_1944/JSB018_Yamato_1944.model",
            "content/gameplay/japan/gun/main/JGM178_460mm_45_Type94/JGM178_460mm_45_Type94.model",
            "content/gameplay/events/director/XD006_Lazarus_Director/XD006_Lazarus_Director.model",
        ];
        let skins = [PeculiarityModel { stock: IOWA.to_string(), replacement: IOWA_SKIN.to_string() }];

        ModelCatalog::from_paths(paths.map(path), &skins)
    }

    /// Iowa's parts; Yamato lists none.
    pub fn ship_parts(catalog: &ModelCatalog) -> HashMap<VehicleParamIndex, ShipPartRows> {
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

        HashMap::from([(VehicleParamIndex::new(IOWA_INDEX), ShipPartRows::from_listing(&listing, catalog))])
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
        let index = ModelTreeIndex::build(&catalog, &ships);
        let parts = fixture::ship_parts(&catalog);

        Data { ships, index, parts }
    }

    fn build(data: &Data, query: &str, set: &mut HashSet<SharedString>, auto: bool) -> BuiltTree {
        let sources =
            TreeSources { ships: &data.ships, ship_parts: &data.parts, models: ModelSection::Ready(&data.index) };

        build_tree(&sources, query, &mut Expansion { set, auto })
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
                "U.S.A.",
                "  Battleship",
                "    IX Iowa +",
                "Models",
                "U.S.A.",
                "  Ships",
                "    Battleships",
                "      ASB028_Iowa_1945 +",
            ]
        );
        assert_eq!(
            search(&data, "yamato"),
            [
                "Japan",
                "  Battleship",
                "    X Yamato",
                "Models",
                "Japan",
                "  Ships",
                "    Battleships",
                "      JSB018_Yamato_1944"
            ]
        );
    }

    #[test]
    fn a_search_matches_model_paths_and_opens_a_model_to_a_matching_variant() {
        let data = data();

        assert_eq!(search(&data, "events/director"), ["Models", "events", "  Directors", "    XD006_Lazarus_Director"]);
        assert_eq!(
            search(&data, "mk7_dead"),
            ["Models", "U.S.A.", "  Guns", "    Main battery", "      AGM034_16in50_Mk7", "        Wreck"]
        );
        assert_eq!(
            search(&data, "stern"),
            ["Models", "U.S.A.", "  Ships", "    Battleships", "      ASB028_Iowa_1945", "        Stern"]
        );
        assert_eq!(
            search(&data, "halloween_dead"),
            [
                "Models",
                "U.S.A.",
                "  Ships",
                "    Battleships",
                "      ASB028_Iowa_1945",
                "        ASB128_Iowa_Halloween",
                "          Wreck",
            ]
        );
    }

    #[test]
    fn a_folder_closed_during_a_search_stays_closed_until_the_search_changes() {
        let data = data();
        let mut set = HashSet::new();
        build(&data, "agm034", &mut set, true);
        set.remove("model-nation-usa");

        let built = build(&data, "agm034", &mut set, false);

        assert_eq!(outline(&built.items), ["Models", "U.S.A. +"]);
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
        let library = ModelLibrary::new(vfs.clone(), Arc::new(bytes), &metadata).expect("model library");
        let ships = ShipCatalog::build(&metadata);

        let started = Instant::now();
        let index = ModelTreeIndex::build(library.catalog(), &ships);
        eprintln!("[measured] index of {} models in {:?}", index.model_count(), started.elapsed());
        assert_eq!(index.model_count(), library.catalog().listed_entries().count());

        let started = Instant::now();
        let parts = list_all_ship_parts(&ships, &metadata, &library);
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
        for query in ["", "a", "iowa", "agm"] {
            let started = Instant::now();
            let built = build(&data, query, &mut HashSet::new(), true);
            eprintln!("[measured] {query:?}: {} rows built in {:?}", built.rows.len(), started.elapsed());
        }
    }
}
