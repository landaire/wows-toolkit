//! Catalogue of the `.model` prototypes under `content/gameplay/`.
//!
//! Gameplay models follow `content/gameplay/<nation>/<kind>/[<sub-kind>/]<dir>/<leaf>.model`.
//! The model named after its directory is the base model. Its siblings are
//! variants of it: `_dead` wreck models and the hull sections of a ship
//! (`_Bow`, `_MidFront`, `_MidBack`, `_Stern`). Skin models live in their own
//! directories and are linked to their stock model through the GameParams
//! `peculiarityModels` of the exteriors that apply them.

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;

use thiserror::Error;

use crate::data::assets_bin_vfs::PrototypeType;
use crate::game_params::provider::GameMetadataProvider;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::PeculiarityModel;
use crate::models::assets_bin::PrototypeDatabase;

use super::part_group::PartGroup;

const GAMEPLAY_PREFIX: &str = "content/gameplay/";
const MODEL_EXTENSION: &str = ".model";
const DEAD_SUFFIX: &str = "_dead";

/// Why a string is not a [`ModelPath`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelPathError {
    #[error("model path {path:?} does not end in .model")]
    NotAModel { path: String },
    #[error("model path {path:?} has an empty file name")]
    EmptyName { path: String },
}

/// VFS path of a `.model` prototype, without a leading `/`, e.g.
/// `content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelPath(String);

impl ModelPath {
    pub fn parse(path: &str) -> Result<Self, ModelPathError> {
        let path = path.strip_prefix('/').unwrap_or(path);
        let Some(without_extension) = path.strip_suffix(MODEL_EXTENSION) else {
            return Err(ModelPathError::NotAModel { path: path.to_string() });
        };

        let stem = without_extension.rsplit('/').next().unwrap_or(without_extension);
        if stem.is_empty() {
            return Err(ModelPathError::EmptyName { path: path.to_string() });
        }

        Ok(Self(path.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The file name without `.model`, e.g. `AGM034_16in50_Mk7`.
    pub fn stem(&self) -> &str {
        let without_extension = &self.0[..self.0.len() - MODEL_EXTENSION.len()];

        without_extension.rsplit('/').next().unwrap_or(without_extension)
    }

    /// The directory holding the model, or `""` for a bare file name.
    pub fn directory(&self) -> &str {
        self.0.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
    }

    /// The path of the sibling prototype with the same stem and `extension`
    /// (which includes the dot, e.g. `.visual`).
    pub fn with_extension(&self, extension: &str) -> String {
        format!("{}{extension}", &self.0[..self.0.len() - MODEL_EXTENSION.len()])
    }
}

impl fmt::Display for ModelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for ModelPath {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// The nation directory of a gameplay model, e.g. `usa`, `japan`, `events`,
/// `common`. These are directory names, not GameParams nation names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelNation(String);

impl ModelNation {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelNation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The kind directory of a gameplay model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModelKind {
    Ship,
    Gun,
    Director,
    Finder,
    Radar,
    Catapult,
    Aircraft,
    Projectile,
    Misc,
    Cap,
    /// A kind directory with no dedicated variant (e.g. `character`).
    Other,
}

impl ModelKind {
    /// Every kind, in display order.
    pub const ALL: [ModelKind; 11] = [
        Self::Ship,
        Self::Gun,
        Self::Director,
        Self::Finder,
        Self::Radar,
        Self::Catapult,
        Self::Aircraft,
        Self::Projectile,
        Self::Misc,
        Self::Cap,
        Self::Other,
    ];

    fn from_directory(directory: &str) -> Self {
        match directory {
            "ship" => Self::Ship,
            "gun" => Self::Gun,
            "director" => Self::Director,
            "finder" => Self::Finder,
            "radar" => Self::Radar,
            "catapult" => Self::Catapult,
            "aircraft" => Self::Aircraft,
            "projectile" => Self::Projectile,
            "misc" => Self::Misc,
            "cap" => Self::Cap,
            _ => Self::Other,
        }
    }
}

/// The sub-kind directory of a gameplay model, read in the context of its
/// [`ModelKind`] (`torpedo` is a launcher under `gun` and a weapon under
/// `projectile`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModelSubKind {
    Battleship,
    Cruiser,
    Destroyer,
    AircraftCarrier,
    Submarine,
    Auxiliary,
    MainBattery,
    SecondaryBattery,
    AntiAircraft,
    TorpedoTubes,
    DepthChargeLauncher,
    MissileLauncher,
    SearchRadar,
    FireControlRadar,
    Fighter,
    Bomber,
    DiveBomber,
    Scout,
    Torpedo,
    Bomb,
    Rocket,
    DepthCharge,
    Mine,
    Missile,
    Jato,
    Shell,
    /// A sub-kind directory this catalogue has no variant for.
    Unrecognized(String),
}

impl ModelSubKind {
    fn from_directory(kind: ModelKind, directory: &str) -> Self {
        let known = match (kind, directory) {
            (ModelKind::Ship, "battleship") => Some(Self::Battleship),
            (ModelKind::Ship, "cruiser") => Some(Self::Cruiser),
            (ModelKind::Ship, "destroyer") => Some(Self::Destroyer),
            (ModelKind::Ship, "aircarrier") => Some(Self::AircraftCarrier),
            (ModelKind::Ship, "submarine") => Some(Self::Submarine),
            (ModelKind::Ship, "auxiliary") => Some(Self::Auxiliary),
            (ModelKind::Gun, "main") => Some(Self::MainBattery),
            (ModelKind::Gun, "secondary") => Some(Self::SecondaryBattery),
            (ModelKind::Gun, "aaircraft") => Some(Self::AntiAircraft),
            (ModelKind::Gun, "torpedo") => Some(Self::TorpedoTubes),
            (ModelKind::Gun, "dcharge") => Some(Self::DepthChargeLauncher),
            (ModelKind::Gun, "missilegun") => Some(Self::MissileLauncher),
            (ModelKind::Radar, "search") => Some(Self::SearchRadar),
            (ModelKind::Radar, "fcontrol") => Some(Self::FireControlRadar),
            (ModelKind::Aircraft, "fighter") => Some(Self::Fighter),
            (ModelKind::Aircraft, "bomber") => Some(Self::Bomber),
            (ModelKind::Aircraft, "dive") => Some(Self::DiveBomber),
            (ModelKind::Aircraft, "scout") => Some(Self::Scout),
            (ModelKind::Projectile, "torpedo") => Some(Self::Torpedo),
            (ModelKind::Projectile, "bomb") => Some(Self::Bomb),
            (ModelKind::Projectile, "rocket") => Some(Self::Rocket),
            (ModelKind::Projectile, "depthcharge") => Some(Self::DepthCharge),
            (ModelKind::Projectile, "mine") => Some(Self::Mine),
            (ModelKind::Projectile, "missile") => Some(Self::Missile),
            (ModelKind::Projectile, "jato") => Some(Self::Jato),
            (ModelKind::Projectile, "artillery") => Some(Self::Shell),
            _ => None,
        };

        known.unwrap_or_else(|| Self::Unrecognized(directory.to_string()))
    }
}

/// How a variant relates to its base model.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModelVariantKind {
    /// The wreck model (`<base>_dead.model`).
    Dead,
    /// A sibling `<base>_<suffix>.model`, e.g. the `Bow` hull section.
    Section(String),
    /// A model an exterior shows in place of the base (`peculiarityModels`).
    Skin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelVariant {
    pub kind: ModelVariantKind,
    pub path: ModelPath,
}

/// One base model of the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelEntry {
    path: ModelPath,
    nation: ModelNation,
    kind: ModelKind,
    sub_kind: Option<ModelSubKind>,
    variants: Vec<ModelVariant>,
    skin_of: Vec<ModelPath>,
}

impl ModelEntry {
    pub fn path(&self) -> &ModelPath {
        &self.path
    }

    /// The model's id, its file stem, e.g. `AGM034_16in50_Mk7`.
    pub fn name(&self) -> &str {
        self.path.stem()
    }

    pub fn nation(&self) -> &ModelNation {
        &self.nation
    }

    pub fn kind(&self) -> ModelKind {
        self.kind
    }

    pub fn sub_kind(&self) -> Option<&ModelSubKind> {
        self.sub_kind.as_ref()
    }

    /// Dead, section and skin variants, sorted by kind then path.
    pub fn variants(&self) -> &[ModelVariant] {
        &self.variants
    }

    /// The hull sections a ship model is drawn from, in path order.
    pub fn sections(&self) -> impl Iterator<Item = &ModelPath> {
        self.variants.iter().filter(|v| matches!(v.kind, ModelVariantKind::Section(_))).map(|v| &v.path)
    }

    /// Stock models this model is a skin of. A non-empty list means a model
    /// browser lists this entry under those models rather than on its own.
    pub fn skin_of(&self) -> &[ModelPath] {
        &self.skin_of
    }

    /// The part group a ship lists a mount of this model under.
    pub fn part_group(&self) -> PartGroup {
        match (self.kind, &self.sub_kind) {
            (ModelKind::Ship, _) => PartGroup::Hull,
            (ModelKind::Gun, Some(ModelSubKind::MainBattery)) => PartGroup::MainBattery,
            (ModelKind::Gun, Some(ModelSubKind::SecondaryBattery)) => PartGroup::SecondaryBattery,
            (ModelKind::Gun, Some(ModelSubKind::AntiAircraft)) => PartGroup::AaGuns,
            (ModelKind::Gun, Some(ModelSubKind::TorpedoTubes)) => PartGroup::Torpedoes,
            (ModelKind::Gun, Some(ModelSubKind::DepthChargeLauncher)) => PartGroup::DepthCharges,
            (ModelKind::Gun, Some(ModelSubKind::MissileLauncher)) => PartGroup::Missiles,
            (ModelKind::Director | ModelKind::Finder, _) => PartGroup::FireControl,
            (ModelKind::Radar, _) => PartGroup::Radar,
            (ModelKind::Misc, _) => PartGroup::Misc,
            _ => PartGroup::Other,
        }
    }
}

/// Every gameplay model in an `assets.bin`, grouped for browsing.
#[derive(Debug, Clone, Default)]
pub struct ModelCatalog {
    /// Base models, sorted by path.
    entries: Vec<ModelEntry>,
    entry_index: HashMap<ModelPath, usize>,
    /// Dead and section variant path to the index of its base entry.
    variant_owner: HashMap<ModelPath, usize>,
    kind_counts: BTreeMap<ModelKind, usize>,
}

impl ModelCatalog {
    /// Group `paths` and link the skins `peculiarity_models` names. Paths that
    /// do not follow the gameplay layout are left out. A skin pair whose stock
    /// or replacement is not a base model of the catalogue is skipped.
    pub fn from_paths(paths: impl IntoIterator<Item = ModelPath>, peculiarity_models: &[PeculiarityModel]) -> Self {
        let mut by_directory: BTreeMap<String, Vec<ModelPath>> = BTreeMap::new();
        for path in paths {
            by_directory.entry(path.directory().to_string()).or_default().push(path);
        }

        let mut entries: Vec<ModelPath> = Vec::new();
        let mut pending_variants: Vec<(ModelPath, ModelVariant)> = Vec::new();
        for (directory, mut paths) in by_directory {
            paths.sort();
            paths.dedup();
            let directory_name = directory.rsplit('/').next().unwrap_or(&directory);

            let Some(base) = paths.iter().find(|p| p.stem() == directory_name).cloned() else {
                entries.extend(paths);
                continue;
            };

            for path in paths {
                if path == base {
                    continue;
                }
                match variant_kind(directory_name, path.stem()) {
                    Some(kind) => pending_variants.push((base.clone(), ModelVariant { kind, path })),
                    None => entries.push(path),
                }
            }
            entries.push(base);
        }

        let mut entries: Vec<ModelEntry> = entries.into_iter().filter_map(new_entry).collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let entry_index: HashMap<ModelPath, usize> =
            entries.iter().enumerate().map(|(idx, entry)| (entry.path.clone(), idx)).collect();

        let mut variant_owner = HashMap::new();
        for (base, variant) in pending_variants {
            let Some(&owner) = entry_index.get(&base) else {
                continue;
            };
            variant_owner.insert(variant.path.clone(), owner);
            entries[owner].variants.push(variant);
        }

        for pair in peculiarity_models {
            let (Ok(stock), Ok(skin)) = (ModelPath::parse(&pair.stock), ModelPath::parse(&pair.replacement)) else {
                continue;
            };
            let (Some(&stock_idx), Some(&skin_idx)) = (entry_index.get(&stock), entry_index.get(&skin)) else {
                continue;
            };
            if stock_idx == skin_idx {
                continue;
            }

            let skin_variant = ModelVariant { kind: ModelVariantKind::Skin, path: skin };
            if !entries[stock_idx].variants.contains(&skin_variant) {
                entries[stock_idx].variants.push(skin_variant);
            }
            if !entries[skin_idx].skin_of.contains(&stock) {
                entries[skin_idx].skin_of.push(stock);
            }
        }

        let mut kind_counts = BTreeMap::new();
        for entry in &mut entries {
            entry.variants.sort_by(|a, b| (&a.kind, &a.path).cmp(&(&b.kind, &b.path)));
            entry.skin_of.sort();
            *kind_counts.entry(entry.kind).or_insert(0) += 1;
        }

        Self { entries, entry_index, variant_owner, kind_counts }
    }

    /// Every base model, skins included, sorted by path.
    pub fn entries(&self) -> &[ModelEntry] {
        &self.entries
    }

    /// The base models a browser lists at top level: every entry that is not
    /// a skin of another model.
    pub fn listed_entries(&self) -> impl Iterator<Item = &ModelEntry> {
        self.entries.iter().filter(|entry| entry.skin_of.is_empty())
    }

    /// The base entry for `path`, which must name a base model.
    pub fn entry(&self, path: &ModelPath) -> Option<&ModelEntry> {
        self.entry_index.get(path).map(|&idx| &self.entries[idx])
    }

    /// The base entry `path` belongs to: itself for a base model, its owner for
    /// a dead or section variant.
    pub fn owning_entry(&self, path: &ModelPath) -> Option<&ModelEntry> {
        self.entry(path).or_else(|| self.variant_owner.get(path).map(|&idx| &self.entries[idx]))
    }

    /// Number of base models per kind, skins included.
    pub fn count_by_kind(&self) -> &BTreeMap<ModelKind, usize> {
        &self.kind_counts
    }
}

/// Paths-storage position of every `.model` prototype in an `assets.bin`, so a
/// model is found by path without scanning the store.
#[derive(Debug, Clone, Default)]
pub struct ModelPathIndex {
    positions: BTreeMap<ModelPath, usize>,
}

impl ModelPathIndex {
    /// One pass over the paths store. `self_id_index` is
    /// [`PrototypeDatabase::build_self_id_index`] of the same database.
    pub fn build(db: &PrototypeDatabase<'_>, self_id_index: &HashMap<u64, usize>) -> Self {
        let mut positions = BTreeMap::new();
        for (position, entry) in db.paths_storage.iter().enumerate() {
            if !entry.name.ends_with(MODEL_EXTENSION) {
                continue;
            }
            let is_model_record =
                db.lookup_r2p(entry.self_id).and_then(|value| db.decode_r2p_value(value).ok()).is_some_and(
                    |location| PrototypeType::from_blob_index(location.blob_index) == Some(PrototypeType::Model),
                );
            if !is_model_record {
                continue;
            }
            if let Ok(path) = ModelPath::parse(&db.reconstruct_path(position, self_id_index)) {
                positions.insert(path, position);
            }
        }

        Self { positions }
    }

    /// Index of the model's entry in `PrototypeDatabase::paths_storage`.
    pub fn position(&self, path: &ModelPath) -> Option<usize> {
        self.positions.get(path).copied()
    }

    pub fn paths(&self) -> impl Iterator<Item = &ModelPath> {
        self.positions.keys()
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// Every `peculiarityModels` pair across the GameParams exteriors, sorted and
/// without duplicates.
pub fn peculiarity_models(metadata: &GameMetadataProvider) -> Vec<PeculiarityModel> {
    let mut pairs: Vec<PeculiarityModel> = metadata
        .params()
        .iter()
        .filter_map(|param| param.exterior())
        .flat_map(|exterior| exterior.peculiarity_models().iter().cloned())
        .collect();
    pairs.sort();
    pairs.dedup();

    pairs
}

/// Builds the entry for `path` when it follows the gameplay layout:
/// `<nation>/<kind>/[<sub-kind>/]<dir>/<leaf>.model` below `content/gameplay/`,
/// every directory named.
fn new_entry(path: ModelPath) -> Option<ModelEntry> {
    let relative = path.as_str().strip_prefix(GAMEPLAY_PREFIX)?;
    let segments: Vec<&str> = relative.split('/').collect();
    if !(4..=5).contains(&segments.len()) || segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }

    let nation = ModelNation(segments[0].to_string());
    let kind = ModelKind::from_directory(segments[1]);
    let sub_kind = (segments.len() == 5).then(|| ModelSubKind::from_directory(kind, segments[2]));

    Some(ModelEntry { path, nation, kind, sub_kind, variants: Vec::new(), skin_of: Vec::new() })
}

/// The variant a sibling of the base model in `directory_name` is, or `None`
/// for an unrelated model sharing the directory.
fn variant_kind(directory_name: &str, stem: &str) -> Option<ModelVariantKind> {
    if stem.ends_with(DEAD_SUFFIX) {
        return Some(ModelVariantKind::Dead);
    }

    stem.strip_prefix(directory_name)
        .and_then(|rest| rest.strip_prefix('_'))
        .filter(|suffix| !suffix.is_empty())
        .map(|suffix| ModelVariantKind::Section(suffix.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(raw: &str) -> ModelPath {
        ModelPath::parse(raw).expect("valid model path")
    }

    fn catalog(raw: &[&str], skins: &[(&str, &str)]) -> ModelCatalog {
        let skins: Vec<PeculiarityModel> = skins
            .iter()
            .map(|(stock, replacement)| PeculiarityModel {
                stock: stock.to_string(),
                replacement: replacement.to_string(),
            })
            .collect();

        ModelCatalog::from_paths(raw.iter().map(|p| path(p)), &skins)
    }

    #[test]
    fn model_paths_need_a_model_file_name() {
        assert_eq!(
            ModelPath::parse("content/a/b.visual"),
            Err(ModelPathError::NotAModel { path: "content/a/b.visual".to_string() })
        );
        assert_eq!(
            ModelPath::parse("content/a/.model"),
            Err(ModelPathError::EmptyName { path: "content/a/.model".to_string() })
        );

        let model = path("/content/gameplay/usa/misc/AM2061/AM2061.model");
        assert_eq!(model.as_str(), "content/gameplay/usa/misc/AM2061/AM2061.model");
        assert_eq!(model.stem(), "AM2061");
        assert_eq!(model.directory(), "content/gameplay/usa/misc/AM2061");
        assert_eq!(model.with_extension(".visual"), "content/gameplay/usa/misc/AM2061/AM2061.visual");
    }

    #[test]
    fn paths_off_the_gameplay_layout_are_left_out() {
        let catalog = catalog(
            &[
                "content/gameplay/usa/AM1.model",
                "content/gameplay/usa/misc/AM2.model",
                "content/gameplay/usa/gun/main/extra/AGX/AGX.model",
                "content/gameplay//misc/AM3/AM3.model",
                "content/location/speedtree/Tree01/Tree01.model",
                "content/gameplay/usa/misc/AM4/AM4.model",
            ],
            &[],
        );

        let names: Vec<&str> = catalog.entries().iter().map(ModelEntry::name).collect();
        assert_eq!(names, ["AM4"]);
    }

    #[test]
    fn groups_real_paths_by_nation_kind_and_sub_kind() {
        let catalog = catalog(
            &[
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model",
                "content/gameplay/japan/gun/main/JGM178_460mm_45_Type94/JGM178_460mm_45_Type94.model",
                "content/gameplay/usa/gun/secondary/AGS010_5in38_Mk32_Mod12/AGS010_5in38_Mk32_Mod12.model",
                "content/gameplay/usa/gun/aaircraft/AGA002_40mm_Bofors_Mk1/AGA002_40mm_Bofors_Mk1.model",
                "content/gameplay/japan/gun/torpedo/JGT012_610mm4_Type_93/JGT012_610mm4_Type_93.model",
                "content/gameplay/japan/gun/dcharge/JGB107_Depth_Charge_Thrower_450mm/JGB107_Depth_Charge_Thrower_450mm.model",
                "content/gameplay/usa/director/AD001_Director_Mk37/AD001_Director_Mk37.model",
                "content/gameplay/usa/finder/AF312_Rangefinder_18ft/AF312_Rangefinder_18ft.model",
                "content/gameplay/usa/radar/search/ARS310_Radar/ARS310_Radar.model",
                "content/gameplay/usa/radar/fcontrol/ARF322_Radar_Mk27_Black/ARF322_Radar_Mk27_Black.model",
                "content/gameplay/usa/catapult/AC001_Catapult_1/AC001_Catapult_1.model",
                "content/gameplay/usa/aircraft/fighter/AAF001_Grumman_F3F/AAF001_Grumman_F3F.model",
                "content/gameplay/usa/projectile/torpedo/APT003_Avia_Torpedo_Mk13/APT003_Avia_Torpedo_Mk13.model",
                "content/gameplay/usa/projectile/jato/APJ001_Jato_14AS_1000/APJ001_Jato_14AS_1000.model",
                "content/gameplay/usa/misc/AM2061/AM2061.model",
                "content/gameplay/events/director/XD006_Lazarus_Director/XD006_Lazarus_Director.model",
                "content/gameplay/common/character/CH001_Sailor/CH001_Sailor.model",
                "content/gameplay/usa/gun/railgun/AGR001_Test/AGR001_Test.model",
                "content/location/speedtree/Tree01/Tree01.model",
            ],
            &[],
        );

        let summary: Vec<String> = catalog
            .entries()
            .iter()
            .map(|e| {
                format!("{} | {} | {:?} | {:?} | {:?}", e.name(), e.nation(), e.kind(), e.sub_kind(), e.part_group())
            })
            .collect();
        insta::assert_debug_snapshot!(summary);
    }

    #[test]
    fn dead_and_section_models_attach_to_their_base() {
        let catalog = catalog(
            &[
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model",
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Bow.model",
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_MidFront.model",
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_MidBack.model",
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Stern.model",
                "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model",
                "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7_dead.model",
                // A dead model whose stem does not repeat the directory name.
                "content/gameplay/russia/misc/RM492_Crane_Mount/RM492_Crane_Mount.model",
                "content/gameplay/russia/misc/RM492_Crane_Mount/RM492_dead.model",
                // A directory without a base model keeps every model listed.
                "content/gameplay/usa/misc/AM900_Set/AM901_Part.model",
                "content/gameplay/usa/misc/AM900_Set/AM902_Part.model",
            ],
            &[],
        );

        let listed: Vec<(&str, &[ModelVariant])> = catalog.listed_entries().map(|e| (e.name(), e.variants())).collect();
        insta::assert_debug_snapshot!(listed);

        let bow = path("content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Bow.model");
        assert_eq!(catalog.entry(&bow), None, "a section is not a base model");
        assert_eq!(catalog.owning_entry(&bow).map(ModelEntry::name), Some("ASB028_Iowa_1945"));
        let iowa = catalog.owning_entry(&bow).expect("Iowa entry");
        assert_eq!(iowa.sections().count(), 4);
    }

    #[test]
    fn skins_nest_under_their_stock_model() {
        let stock = "content/gameplay/germany/gun/main/GGM058_280mm54_5_C34/GGM058_280mm54_5_C34.model";
        let stock_dead = "content/gameplay/germany/gun/main/GGM058_280mm54_5_C34/GGM058_280mm54_5_C34_dead.model";
        let skin = "content/gameplay/germany/gun/main/GGM546_280mm54_5_C34_Black/GGM546_280mm54_5_C34_Black.model";
        let skin_dead =
            "content/gameplay/germany/gun/main/GGM546_280mm54_5_C34_Black/GGM546_280mm54_5_C34_Black_dead.model";
        let catalog = catalog(
            &[stock, stock_dead, skin, skin_dead],
            &[
                (stock, skin),
                // A pair between variants links nothing: the bases already do.
                (stock_dead, skin_dead),
                // A pair naming a model absent from the catalogue is skipped.
                (stock, "content/gameplay/germany/gun/main/GGM999_Missing/GGM999_Missing.model"),
            ],
        );

        let listed: Vec<&str> = catalog.listed_entries().map(ModelEntry::name).collect();
        assert_eq!(listed, ["GGM058_280mm54_5_C34"], "the skin is listed under its stock model only");

        let stock_entry = catalog.entry(&path(stock)).expect("stock entry");
        let kinds: Vec<(&ModelVariantKind, &str)> =
            stock_entry.variants().iter().map(|v| (&v.kind, v.path.stem())).collect();
        assert_eq!(
            kinds,
            [
                (&ModelVariantKind::Dead, "GGM058_280mm54_5_C34_dead"),
                (&ModelVariantKind::Skin, "GGM546_280mm54_5_C34_Black")
            ]
        );

        let skin_entry = catalog.entry(&path(skin)).expect("skin entry");
        assert_eq!(skin_entry.skin_of(), [path(stock)]);
        assert_eq!(skin_entry.variants().len(), 1, "the skin keeps its own dead model");
    }
}
