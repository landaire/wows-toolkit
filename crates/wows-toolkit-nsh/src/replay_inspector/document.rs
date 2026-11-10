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

use wows_toolkit_replay_data::load::ParsedReplay;
use wows_toolkit_replay_data::model::ReplayReportModel;
use wows_toolkit_viewmodel::index_rows::ConstantsFit;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wowsunpack::vfs::VfsPath;

use crate::game_data::GameDataError;
use crate::replay_inspector::error::InspectorError;

/// Identifies one open replay for as long as it is open. A re-read replay
/// gets a new id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReplayDocId(u32);

impl ReplayDocId {
    /// The id as the TSX passes it around.
    pub fn from_js(raw: u32) -> Self {
        Self(raw)
    }

    pub fn to_js(self) -> u32 {
        self.0
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
    pub build: u32,
    pub version: String,
}

/// One parsed replay, as the inspector shows it.
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
                Some(ConstantsMismatch { build, version: model.context.version.clone() })
            }
            _ => None,
        };

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
        }
    }
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
