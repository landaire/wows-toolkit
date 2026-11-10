//! `ReplayInspectorService`: what `ui/replay_inspector.tsx` asks of the
//! native side for open replays, the listing's stored state, the landed-
//! replay side effects and the result mappings.
//!
//! Failures reach the TSX as the error's text; the refusals a reader is
//! meant to see are worded in the error itself.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;

use nsh_core::file_dialog::FileFilter;
use nsh_core::file_dialog::OpenDialog;
use nsh_core::file_dialog::SaveDialog;
use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use nsh_core::prelude::blocking;
use nsh_core::prelude::dark;
use nsh_core::prelude::module;
use nsh_core::prelude::pick_paths;
use nsh_core::prelude::pick_save_path;
use wows_toolkit_config::ReplayExportFormat;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_replay_data::export::write_export;

use crate::replay_inspector::auto_export;
use crate::replay_inspector::columns::visible_column_keys;
use crate::replay_inspector::constants;
use crate::replay_inspector::constants::BuildNumber;
use crate::replay_inspector::constants::ConstantsBuild;
use crate::replay_inspector::document;
use crate::replay_inspector::document::ReplayDocId;
use crate::replay_inspector::document::ReplayDocument;
use crate::replay_inspector::error::InspectorError;
use crate::replay_inspector::landed;
use crate::replay_inspector::landed::LandedIn;
use crate::replay_inspector::landed::LandedReport;
use crate::replay_inspector::settings;
use crate::replay_inspector::summary;
use crate::replay_inspector::summary::ReplaySummary;
use crate::shell::ReplayPreferences;

/// How the replay listing groups its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum Grouping {
    Date,
    Ship,
    None,
}

impl Grouping {
    fn from_stored(grouping: ReplayGrouping) -> Self {
        match grouping {
            ReplayGrouping::Date => Self::Date,
            ReplayGrouping::Ship => Self::Ship,
            ReplayGrouping::None => Self::None,
        }
    }

    fn stored(self) -> ReplayGrouping {
        match self {
            Self::Date => ReplayGrouping::Date,
            Self::Ship => ReplayGrouping::Ship,
            Self::None => ReplayGrouping::None,
        }
    }
}

/// A format the Export menu writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum ExportFormat {
    Json,
    Cbor,
    Csv,
}

impl ExportFormat {
    fn stored(self) -> ReplayExportFormat {
        match self {
            Self::Json => ReplayExportFormat::Json,
            Self::Cbor => ReplayExportFormat::Cbor,
            Self::Csv => ReplayExportFormat::Csv,
        }
    }
}

/// Which of a replay's debug payloads the raw JSON viewer shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum RawJson {
    /// The replay header's metadata.
    Metadata,
    /// The battle-results packet as the server sent it.
    Results,
    /// The same results with their positional arrays resolved to named fields.
    Mapped,
}

/// The listing's stored state.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ListingState {
    pub grouping: Grouping,
    pub listing_collapsed: bool,
    /// The visible player-table columns, as the TSX names them.
    pub columns: Vec<String>,
}

/// How many other recordings one replay is read alongside. Each is a packet
/// stream held for as long as the replay is open and walked twice by the
/// merge; a full enemy team is eleven.
const MAX_ALTS: usize = 11;

/// The stored replay row as last read or written here, which the column flags
/// the Settings tab does not edit come from.
static REPLAY_ROW: Mutex<Option<ReplaySettings>> = Mutex::new(None);

/// The Replay Inspector's native services.
#[module]
pub struct ReplayInspectorService;

#[module]
impl ReplayInspectorService {
    /// Reads the replay at `path` with the other recordings `alts`, registers
    /// it, writes the auto-export when it is set, and summarizes it. `debug`
    /// is the session's debug mode, which keeps the debug fields in the export.
    pub async fn open(path: String, alts: Vec<String>, debug: bool) -> Result<ReplaySummary, InspectorError> {
        let path = PathBuf::from(path);
        let alts: Vec<PathBuf> = alts.into_iter().map(PathBuf::from).collect();
        let (document, export_status) =
            blocking(move || read_for_tab(&path, &alts, debug)).await.map_err(task_failed)??;

        let id = document::insert(document);
        let Some(document) = document::get(id) else {
            return Err(InspectorError::UnknownDocument { id });
        };

        Ok(summary::summarize(id, &document, dark(), export_status))
    }

    /// Forgets a closed replay.
    pub fn close(document: ReplayDocId) {
        document::remove(document);
    }

    /// Rates the rows of an open replay that were read before the personal-
    /// rating table loaded. Registers the rated replay in place of `document`
    /// and summarizes it; `None` when the table is not loaded or rates no
    /// further row.
    pub async fn rerate(document: ReplayDocId) -> Result<Option<ReplaySummary>, InspectorError> {
        let Some(table) = crate::game_data::personal_rating() else {
            return Ok(None);
        };
        let Some(rated) = open_document(document)?.rated(&table) else {
            return Ok(None);
        };

        let id = document::insert(rated);
        document::remove(document);
        let Some(rated) = document::get(id) else {
            return Err(InspectorError::UnknownDocument { id });
        };

        Ok(Some(summary::summarize(id, &rated, dark(), None)))
    }

    /// One of the replay's debug payloads; `None` when the replay has none.
    pub fn raw_json(document: ReplayDocId, which: RawJson) -> Result<Option<String>, InspectorError> {
        let document = open_document(document)?;

        Ok(match which {
            RawJson::Metadata => Some(document.raw_metadata_json.clone()),
            RawJson::Results => document.raw_results_json.clone(),
            RawJson::Mapped => document.mapped_results_json.clone(),
        })
    }

    /// Asks where to write the match and writes it there: the debug form in
    /// debug mode, the stripped one otherwise. The written path, or `None`
    /// when the dialog was cancelled.
    pub async fn export_results(
        document: ReplayDocId,
        format: ExportFormat,
        debug: bool,
    ) -> Result<Option<String>, InspectorError> {
        let document = open_document(document)?;
        let format = format.stored();
        let dialog = SaveDialog::new()
            .title(rust_i18n::t!("ui.replay.export_results_title"))
            .file_name(wows_toolkit_replay_data::export::file_name(&document.export_stem, format));
        let Some(path) = save_path(dialog).await? else {
            return Ok(None);
        };

        let export = if debug { document.export.clone() } else { document.export.clone().stripped() };
        let target = path.clone();
        blocking(move || write_export(&export, &target, format)).await.map_err(task_failed)??;

        Ok(Some(path.display().to_string()))
    }

    /// The whole chat log, one `[clan] sender (Channel): message` per line.
    pub fn chat_transcript(document: ReplayDocId) -> Result<String, InspectorError> {
        let document = open_document(document)?;

        Ok(wows_toolkit_replay_data::chat::transcript(&document.model.chat))
    }

    /// Asks where to save the chat log, offering `<title> - Game Chat.txt`,
    /// and writes it there. The written path, or `None` when cancelled.
    pub async fn save_chat(document: ReplayDocId, title: String) -> Result<Option<String>, InspectorError> {
        let transcript = wows_toolkit_replay_data::chat::transcript(&open_document(document)?.model.chat);
        let dialog = SaveDialog::new()
            .title(rust_i18n::t!("ui.replay.chat_save_title"))
            .file_name(format!("{title} - Game Chat.txt"));
        let Some(path) = save_path(dialog).await? else {
            return Ok(None);
        };

        let target = path.clone();
        blocking(move || std::fs::write(&target, transcript))
            .await
            .map_err(task_failed)?
            .map_err(|source| InspectorError::Write { path: path.clone(), source })?;

        Ok(Some(path.display().to_string()))
    }

    /// Asks for a replay file and remembers it as the one the reader opened.
    pub async fn pick_replay_file() -> Result<Option<String>, InspectorError> {
        let Some(path) = pick_one(OpenDialog::files().filter(replays_filter())).await? else {
            return Ok(None);
        };

        let remembered = path.clone();
        blocking(move || settings::save_current_replay_path(&remembered)).await.map_err(task_failed)??;

        Ok(Some(path.display().to_string()))
    }

    /// Asks for a directory of replays to list.
    pub async fn pick_replay_directory() -> Result<Option<String>, InspectorError> {
        let dialog = OpenDialog::folders().title(rust_i18n::t!("ui.replay.open_directory"));
        let picked = pick_one(dialog).await?;

        Ok(picked.map(|path| path.display().to_string()))
    }

    /// Asks for another recording of the open replay's battle and checks it is
    /// one. The recording to read the battle through, or `None` when the
    /// dialog was cancelled or the recording is already read. Refused past
    /// eleven recordings, and for a recording of another battle.
    pub async fn pick_alt_perspective(document: ReplayDocId) -> Result<Option<String>, InspectorError> {
        let document = open_document(document)?;
        if document.alts.len() >= MAX_ALTS {
            return Err(InspectorError::AltLimit { limit: MAX_ALTS });
        }

        let dialog =
            OpenDialog::files().title(rust_i18n::t!("ui.replay.load_alt_perspective")).filter(replays_filter());
        let Some(alt) = pick_one(dialog).await? else {
            return Ok(None);
        };
        if document.alts.iter().any(|kept| same_file(kept, &alt)) {
            return Ok(None);
        }

        let primary = document.path.clone();
        let checking = alt.clone();
        blocking(move || check_alt(&primary, &checking)).await.map_err(task_failed)??;

        Ok(Some(alt.display().to_string()))
    }

    /// Exports, indexes and contributes a replay the watcher saw land in the
    /// opened `directory`, or in the game's own when `None`, the export in the
    /// session's `debug` mode.
    pub async fn process_landed(
        path: String,
        directory: Option<String>,
        debug: bool,
    ) -> Result<LandedReport, InspectorError> {
        let landed_in = match directory {
            Some(root) => LandedIn::OpenedDirectory(PathBuf::from(root)),
            None => LandedIn::GameDirectory,
        };

        landed::process(PathBuf::from(path), landed_in, debug).await
    }

    /// Fetches the mapping of each listed build that has none and was not
    /// asked about this session. Returns how many were written.
    pub async fn fetch_missing_constants(builds: Vec<ConstantsBuild>) -> Result<u32, InspectorError> {
        blocking(move || constants::fetch_missing(builds)).await.map_err(task_failed)
    }

    /// Fetches `build`'s mapping again after a battle was read through one
    /// that does not fit it. Returns how many were written; `0` when this build
    /// was already recovered this session.
    pub async fn recover_constants(build: BuildNumber, version: Option<String>) -> Result<u32, InspectorError> {
        blocking(move || constants::recover(build, version)).await.map_err(task_failed)?
    }

    /// Checks for a newer published mapping for the installed build, once a
    /// session. `true` when one was written.
    pub async fn check_latest_constants() -> Result<bool, InspectorError> {
        blocking(constants::check_latest).await.map_err(task_failed)?
    }

    /// Asks for a `constants.json` and takes it as the loaded build's mapping.
    /// The confirmation to show, or `None` when the dialog was cancelled.
    pub async fn import_constants() -> Result<Option<String>, InspectorError> {
        let build = constants::loaded_build()?;
        let dialog = OpenDialog::files()
            .title(rust_i18n::t!("ui.replay.constants_import_title"))
            .filter(FileFilter::new("JSON", ["json"]));
        let Some(path) = pick_one(dialog).await? else {
            return Ok(None);
        };

        blocking(move || constants::import(&path, build)).await.map_err(task_failed)??;

        Ok(Some(rust_i18n::t!("ui.replay.constants_imported", build = build).into_owned()))
    }

    /// A short label for a directory: every component above the last as its
    /// initial, the last cut to its ends when long.
    pub fn shortened_path(path: String) -> String {
        wows_toolkit_replay_data::directories::shortened_path(Path::new(&path))
    }

    /// Stores how the listing groups its rows.
    pub async fn set_grouping(grouping: Grouping) -> Result<(), InspectorError> {
        edit_replay_row(move |row| row.grouping = grouping.stored()).await
    }

    /// Stores whether the listing is collapsed to its rail.
    pub async fn set_listing_collapsed(collapsed: bool) -> Result<(), InspectorError> {
        edit_replay_row(move |row| row.listing_collapsed = collapsed).await
    }

    /// The listing's stored grouping, collapse and visible columns.
    pub async fn listing_state() -> Result<ListingState, InspectorError> {
        let inspector = blocking(settings::load).await.map_err(task_failed)??;
        let state = listing_state_of(&inspector.replay);
        remember_row(inspector.replay);

        Ok(state)
    }

    /// The visible columns for the Settings tab's `preferences` over the row
    /// `listingState` last read, without reading the database again.
    pub fn columns_for(preferences: ReplayPreferences) -> Result<Vec<String>, InspectorError> {
        let Some(mut row) = lock_row().clone() else {
            return Err(InspectorError::ListingNotRead);
        };
        row.show_raw_xp = preferences.show_raw_xp;
        row.show_observed_damage = preferences.show_observed_damage;
        row.show_entity_id = preferences.show_entity_id;
        row.show_heals = preferences.show_heals;

        Ok(visible_column_keys(&row))
    }
}

/// Reads a replay for a tab: the document with the Twitch sightings around
/// its battle applied, and the auto-export written. The second value is why
/// the auto-export failed, when it did.
fn read_for_tab(
    path: &Path,
    alts: &[PathBuf],
    debug: bool,
) -> Result<(ReplayDocument, Option<String>), InspectorError> {
    let inspector = settings::load()?;
    let mut document = document::read(path, alts)?;
    flag_twitch_candidates(&mut document);

    let export_status = match auto_export::write(&document.export, &document.export_stem, &inspector.replay, debug) {
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "auto-export failed");
            Some(error.to_string())
        }
    };

    Ok((document, export_status))
}

/// Flags the rows that the logins seen in the monitored Twitch channel's chat
/// around this battle plausibly name. A lookup that fails leaves the rows
/// unflagged, which is what a battle nobody watched looks like.
fn flag_twitch_candidates(document: &mut ReplayDocument) {
    let battle_at = document.model.timestamp.as_second();
    let start = battle_at + (wows_toolkit_viewmodel::twitch::WINDOW_BEFORE_MINUTES * 60.0) as i64;
    let end = battle_at + (wows_toolkit_viewmodel::twitch::WINDOW_AFTER_MINUTES * 60.0) as i64;

    let found = settings::with_pool(|pool| async move {
        Ok(wows_toolkit_config::index::query::observations_in_window(&pool, start, end).await)
    });
    let rows = match found {
        Ok(Ok(rows)) => rows,
        Ok(Err(error)) => {
            tracing::warn!(%error, "replay inspector: the chat lookup failed");
            return;
        }
        Err(error) => {
            tracing::warn!(%error, "replay inspector: the chat lookup did not complete");
            return;
        }
    };

    let observations: Vec<(String, jiff::Timestamp)> = rows
        .into_iter()
        .filter_map(|(login, seen_at)| jiff::Timestamp::from_second(seen_at).ok().map(|seen_at| (login, seen_at)))
        .collect();
    if observations.is_empty() {
        return;
    }

    document.model.populate_twitch_candidates(&observations);
}

/// Checks that `alt` recorded the battle `primary` did. Blocking.
fn check_alt(primary: &Path, alt: &Path) -> Result<(), InspectorError> {
    let cache =
        crate::game_data::cache().ok_or(InspectorError::GameData(crate::game_data::GameDataError::NotConfigured))?;

    wows_toolkit_replay_data::load::check_alt(primary, alt, &cache)
        .map_err(|source| InspectorError::AltRefused { source })
}

/// Whether two paths name the same recording, through the filesystem's
/// answer where it has one, so one file picked under two spellings is one. A
/// path that cannot be resolved is compared as written.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn open_document(id: ReplayDocId) -> Result<std::rc::Rc<ReplayDocument>, InspectorError> {
    document::get(id).ok_or(InspectorError::UnknownDocument { id })
}

fn replays_filter() -> FileFilter {
    FileFilter::new("WoWs Replays", ["wowsreplay"])
}

async fn pick_one(dialog: OpenDialog) -> Result<Option<PathBuf>, InspectorError> {
    let picked = pick_paths(dialog).await.map_err(|reason| InspectorError::Dialog { reason })?;

    Ok(picked.and_then(|mut paths| paths.pop()))
}

async fn save_path(dialog: SaveDialog) -> Result<Option<PathBuf>, InspectorError> {
    pick_save_path(dialog).await.map_err(|reason| InspectorError::Dialog { reason })
}

/// Reads the replay row, applies `edit` and writes it back, ordered with the
/// Settings tab's writes of the same row.
async fn edit_replay_row(edit: impl FnOnce(&mut ReplaySettings) + Send + 'static) -> Result<(), InspectorError> {
    let written =
        nsh_core::prelude::background(crate::shell::update_replay_settings(edit)).await.map_err(task_failed)??;
    remember_row(written);

    Ok(())
}

fn listing_state_of(row: &ReplaySettings) -> ListingState {
    ListingState {
        grouping: Grouping::from_stored(row.grouping),
        listing_collapsed: row.listing_collapsed,
        columns: visible_column_keys(row),
    }
}

fn remember_row(row: ReplaySettings) {
    *lock_row() = Some(row);
}

fn lock_row() -> std::sync::MutexGuard<'static, Option<ReplaySettings>> {
    REPLAY_ROW.lock().unwrap_or_else(PoisonError::into_inner)
}

fn task_failed(source: tokio::task::JoinError) -> InspectorError {
    InspectorError::Task { source }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_round_trips_through_the_stored_row() {
        for grouping in [Grouping::Date, Grouping::Ship, Grouping::None] {
            assert_eq!(Grouping::from_stored(grouping.stored()), grouping);
        }
    }

    #[test]
    fn the_listing_state_reads_the_row() {
        let row = ReplaySettings {
            grouping: ReplayGrouping::Ship,
            listing_collapsed: true,
            show_raw_xp: true,
            ..ReplaySettings::default()
        };

        let state = listing_state_of(&row);

        assert_eq!(state.grouping, Grouping::Ship);
        assert!(state.listing_collapsed);
        assert!(state.columns.iter().any(|column| column == "rawXp"));
    }

    #[test]
    fn the_same_file_under_two_spellings_is_one_recording() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let file = directory.path().join("a.wowsreplay");
        std::fs::write(&file, b"").expect("the file is writable");
        let respelled = directory.path().join(".").join("a.wowsreplay");

        assert!(same_file(&file, &respelled));
        assert!(!same_file(&file, &directory.path().join("b.wowsreplay")));
    }

    #[test]
    fn the_alt_refusals_are_worded_for_the_reader() {
        assert_eq!(
            InspectorError::AltLimit { limit: MAX_ALTS }.to_string(),
            "This battle is already being read through 11 other recordings"
        );
        assert_eq!(InspectorError::NoBuildLoaded.to_string(), "Load a game build first");
    }
}
