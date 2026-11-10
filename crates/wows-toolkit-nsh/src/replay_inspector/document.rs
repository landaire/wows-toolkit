//! The replays the inspector has open, by id. The service the TSX calls
//! parses and registers them; the player table component reads them by the
//! id the TSX hands it. The registry lives on the engine thread, where both
//! run; parsing happens off it.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;

use base64::Engine as _;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::load::ParsedReplay;
use wows_toolkit_replay_data::model::ReplayReportModel;
use wows_toolkit_viewmodel::index_rows::ConstantsFit;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::vfs::VfsPath;

use crate::class_icon::ClassIcon;
use crate::game_data::GameDataError;
use crate::js_newtype::js_newtype;
use crate::replay_inspector::constants::BuildNumber;
use crate::replay_inspector::error::InspectorError;

/// Identifies one open replay for as long as it is open. A re-read replay
/// gets a new id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReplayDocId(u32);

js_newtype!(ReplayDocId, u32);

#[cfg(test)]
impl ReplayDocId {
    pub(crate) fn new(raw: u32) -> Self {
        Self(raw)
    }
}

impl std::fmt::Display for ReplayDocId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The build a replay's results were read with a mapping that does not fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantsMismatch {
    pub build: BuildNumber,
    pub version: String,
}

/// The recording player's nation flag, as a `data:` URL, and ship class
/// icon, which the tab title shows. Each is `None` when the build lacks it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleIcons {
    pub nation_flag: Option<String>,
    pub class_icon: Option<ClassIcon>,
}

impl TitleIcons {
    /// Reads the icons of `model`'s self row from `game_data`. Blocking.
    pub fn read(model: &ReplayReportModel, game_data: &LoadedGameData) -> Self {
        let Some(player) = model.rows.iter().find(|row| row.is_self) else {
            return Self::default();
        };
        let vfs = game_data.vfs();

        let nation_flag = player
            .ship_id
            .and_then(|id| GameParamProvider::game_param_by_id(game_data.provider().as_ref(), id))
            .and_then(|param| GuiAsset::NationFlag(param.nation()).read(vfs, None))
            .map(|bytes| data_uri("image/png", &bytes));
        let class_icon = ClassIcon::read(player.ship_class, vfs);

        Self { nation_flag, class_icon }
    }
}

/// One parsed replay, as the inspector shows it.
#[derive(Clone)]
pub struct ReplayDocument {
    pub path: PathBuf,
    /// Other recordings of the same battle read alongside it.
    pub alts: Vec<PathBuf>,
    /// The rows, with the fire chance and personal ratings applied.
    pub model: ReplayReportModel,
    /// The match as the Export menu writes it, in its debug form.
    pub export: ExportedMatch,
    /// The export's file name without its extension.
    pub export_stem: String,
    pub raw_metadata_json: String,
    /// `None` when the replay has no battle-results packet yet.
    pub raw_results_json: Option<String>,
    pub mapped_results_json: Option<String>,
    /// The game files of the build the replay was read against, where the
    /// table's icons come from. `None` for a document built without game
    /// data, which then shows text in place of icons.
    pub vfs: Option<VfsPath>,
    /// This battle as the session statistics record it.
    pub session_stat: Option<PerGameStat>,
    pub constants_mismatch: Option<ConstantsMismatch>,
    pub title_icons: TitleIcons,
}

impl ReplayDocument {
    /// Builds the document from a parse of `path` (and `alts`), rating the
    /// rows against `personal_rating` when the table is loaded.
    pub fn from_parsed(
        path: PathBuf,
        alts: Vec<PathBuf>,
        parsed: ParsedReplay,
        personal_rating: Option<&PersonalRatingData>,
    ) -> Self {
        let ParsedReplay {
            mut model,
            indexable,
            export,
            game_data,
            raw_metadata_json,
            raw_results_json,
            mapped_results_json,
            fire_chance,
            session_stat,
            export_stem,
            shareable: _,
        } = parsed;

        model.set_fire_chance(fire_chance);
        if let Some(table) = personal_rating {
            model.populate_personal_ratings(table);
        }
        let constants_mismatch = match (indexable.constants_fit, indexable.version_build) {
            (ConstantsFit::Mismatched, Some(build)) => {
                Some(ConstantsMismatch { build: BuildNumber(build), version: model.context.version.clone() })
            }
            _ => None,
        };

        let title_icons = TitleIcons::read(&model, &game_data);

        Self {
            path,
            alts,
            model,
            export,
            export_stem,
            raw_metadata_json,
            raw_results_json,
            mapped_results_json,
            vfs: Some(game_data.vfs().clone()),
            session_stat,
            constants_mismatch,
            title_icons,
        }
    }

    /// A copy with the rows `table` can rate and are not yet rated filled in;
    /// `None` when that changes no row.
    pub fn rated(&self, table: &PersonalRatingData) -> Option<Self> {
        let unrated = |document: &Self| document.model.rows.iter().filter(|row| row.personal_rating.is_none()).count();
        let before = unrated(self);
        if before == 0 {
            return None;
        }

        let mut rated = self.clone();
        rated.model.populate_personal_ratings(table);

        (unrated(&rated) < before).then_some(rated)
    }
}

fn data_uri(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// What reads a replay into a document: the real parse, or a test's.
pub type ReplaySource = fn(&Path, &[PathBuf]) -> Result<ReplayDocument, InspectorError>;

static SOURCE: Mutex<Option<ReplaySource>> = Mutex::new(None);

/// Reads `path` (and `alts`) into a document against the game data the
/// settings name, rating the rows when the personal-rating table is loaded.
/// Blocks; run it in `blocking`.
pub fn read(path: &Path, alts: &[PathBuf]) -> Result<ReplayDocument, InspectorError> {
    let source = *SOURCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(source) = source {
        return source(path, alts);
    }

    let cache = crate::game_data::cache().ok_or(InspectorError::GameData(GameDataError::NotConfigured))?;
    let personal_rating = crate::game_data::personal_rating();
    let parsed =
        wows_toolkit_replay_data::load::parse_replay_with_alts(path, alts, &cache, personal_rating.as_deref())?;

    Ok(ReplayDocument::from_parsed(path.to_path_buf(), alts.to_vec(), parsed, personal_rating.as_deref()))
}

/// Makes [`read`] use `source` instead of the game-data parse, for tests
/// that have no game install.
#[cfg(feature = "test-support")]
pub fn set_source(source: ReplaySource) {
    *SOURCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(source);
}

thread_local! {
    static DOCUMENTS: RefCell<HashMap<ReplayDocId, Rc<ReplayDocument>>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
}

/// Registers `document` and returns its id.
pub fn insert(document: ReplayDocument) -> ReplayDocId {
    let id = NEXT_ID.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1));
        ReplayDocId(id)
    });
    DOCUMENTS.with(|documents| documents.borrow_mut().insert(id, Rc::new(document)));

    id
}

pub fn get(id: ReplayDocId) -> Option<Rc<ReplayDocument>> {
    DOCUMENTS.with(|documents| documents.borrow().get(&id).cloned())
}

/// Forgets a closed replay.
pub fn remove(id: ReplayDocId) {
    DOCUMENTS.with(|documents| documents.borrow_mut().remove(&id));
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use crate::replay_inspector::test_support;

    #[test]
    fn a_later_table_rates_the_unrated_self_row_once() {
        let document = test_support::unrated_sample_document(Path::new("sample.wowsreplay"), &[]);
        let table = test_support::sample_personal_rating();

        let rated = document.rated(&table).expect("the self row becomes rated");

        assert!(rated.model.rows.iter().any(|row| row.is_self && row.personal_rating.is_some()));
        assert!(rated.rated(&table).is_none(), "nothing further is rated");
    }
}
