//! The command palette's native half: its labels, the rows its index-backed
//! layers offer, the ship layer's ranking and the replay picker.

use nsh_core::file_dialog::FileFilter;
use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use wows_toolkit_config::index::query_text;
use wows_toolkit_viewmodel::palette::PaletteError;
use wows_toolkit_viewmodel::palette::PaletteMode;
use wows_toolkit_viewmodel::palette::ShipRow;
use wows_toolkit_viewmodel::palette::index_rows;
use wows_toolkit_viewmodel::palette::rank_ships;
use wows_toolkit_viewmodel::query_bar::seed;
use wows_toolkit_viewmodel::translate;

use crate::runtime::RuntimeError;

/// Why an index-backed layer could not be read.
#[derive(Debug, thiserror::Error)]
enum LayerError {
    #[error("{source}")]
    Runtime { source: RuntimeError },
    #[error("the replay index is not open: {source}")]
    Open { source: sqlx::Error },
    #[error("{source}")]
    Rows { source: PaletteError },
}

/// One of the palette's second layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum PaletteLayer {
    Players,
    MyShips,
    ArmorShips,
}

/// What a layer is called where it is entered, in its header and in its
/// search field.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct LayerText {
    pub layer: PaletteLayer,
    pub label: String,
    pub title: String,
    pub placeholder: String,
}

/// A search the palette seeds the Search tab with.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SeededSearch {
    pub label: String,
    pub query: String,
}

/// The palette's words, in the shared catalogue's current language.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PaletteText {
    pub title: String,
    pub placeholder: String,
    /// "Go to: <tab>" for each tab label asked for, in order.
    pub go_to: Vec<String>,
    pub advanced_search: String,
    pub layers: Vec<LayerText>,
    pub seeded: Vec<SeededSearch>,
    pub open_replay_file: String,
    pub open_replay_directory: String,
    pub index_all_replays: String,
    pub refresh_persisted_data: String,
    pub import_constants: String,
    pub send_all_replays: String,
    pub send_all_replays_again: String,
    pub copy_latest_log: String,
    pub copy_bug_report_info: String,
    pub theme_system: String,
    pub theme_dark: String,
    pub theme_light: String,
    pub back_hint: String,
    pub ships_loading: String,
    pub no_matches: String,
}

/// One row an index-backed layer offers: its text and the search it runs.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PaletteRow {
    pub label: String,
    pub query: String,
}

/// One ship of the loaded build, as the catalogue's owner hands it over.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ShipInput {
    pub param_index: String,
    pub display_name: String,
    /// The tier as read (`X`).
    pub tier: String,
    /// The class as read (`Battleship`).
    pub class: String,
    pub nation: String,
}

/// One ship row the ship layer shows.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ShipRowView {
    pub param_index: String,
    pub display_name: String,
    pub label: String,
}

/// The palette's services.
#[module]
pub struct Palette;

#[module]
impl Palette {
    /// The palette's words; `tabs` are the tab labels the "Go to" entries name.
    pub fn text(tabs: Vec<String>) -> PaletteText {
        let go_to = translate("ui.palette.go_to");
        let set_theme = translate("ui.palette.set_theme");
        let theme = |key: &str| set_theme.replace("%{theme}", &translate(key));
        let layers = PaletteMode::ALL
            .into_iter()
            .map(|mode| LayerText {
                layer: layer_of(mode),
                label: translate(mode.label_key()),
                title: translate(mode.title_key()),
                placeholder: translate(mode.placeholder_key()),
            })
            .collect();

        PaletteText {
            title: translate("ui.palette.title"),
            placeholder: translate("ui.palette.placeholder"),
            go_to: tabs.iter().map(|tab| go_to.replace("%{tab}", tab)).collect(),
            advanced_search: translate("ui.palette.advanced_search"),
            layers,
            seeded: vec![
                SeededSearch {
                    label: translate("ui.palette.games_i_died_in"),
                    query: query_text::print_query(&seed::games_i_died_in()),
                },
                SeededSearch {
                    label: translate("ui.palette.games_i_won"),
                    query: query_text::print_query(&seed::games_i_won()),
                },
            ],
            open_replay_file: translate("ui.replay.open_manually"),
            open_replay_directory: translate("ui.replay.open_directory"),
            index_all_replays: translate("ui.settings.replay.index_all_replays"),
            refresh_persisted_data: translate("ui.replay.refresh_persisted_data"),
            import_constants: translate("ui.replay.import_constants"),
            send_all_replays: translate("ui.palette.send_all_replays"),
            send_all_replays_again: translate("ui.palette.send_all_replays_again"),
            copy_latest_log: translate("ui.replay.copy_latest_log"),
            copy_bug_report_info: translate("ui.replay.copy_bug_report_info"),
            theme_system: theme("ui.settings.app.theme_system"),
            theme_dark: theme("ui.settings.app.theme_dark"),
            theme_light: theme("ui.settings.app.theme_light"),
            back_hint: translate("ui.palette.back_hint"),
            ships_loading: translate("ui.palette.ships_loading"),
            no_matches: translate("ui.search.no_matches"),
        }
    }

    /// "No ship list: <reason>".
    pub fn ships_unavailable(reason: String) -> String {
        translate("ui.palette.ships_unavailable").replace("%{reason}", &reason)
    }

    /// "The search failed: <reason>".
    pub fn search_failed(reason: String) -> String {
        translate("ui.search.failed").replace("%{reason}", &reason)
    }

    /// The rows an index-backed layer offers for `needle`.
    pub async fn layer_rows(layer: PaletteLayer, needle: String) -> Result<Vec<PaletteRow>, String> {
        blocking(move || read_layer(layer, &needle))
            .await
            .map_err(|error| format!("the palette layer task did not complete: {error}"))?
            .map_err(|error| error.to_string())
    }

    /// Shows the open dialog for one replay; its path, or `null` when
    /// cancelled.
    pub async fn pick_replay() -> Result<Option<String>, String> {
        let picked = pick_paths(
            OpenDialog::files()
                .filter(FileFilter::new("World of Warships replay", ["wowsreplay"]))
                .title("Open Replay")
                .button_label("Open"),
        )
        .await
        .map_err(|error| format!("could not show the replay file dialog: {error}"))?;

        Ok(picked.and_then(|mut paths| paths.pop()).map(|path| path.to_string_lossy().into_owned()))
    }
}

/// The loaded build's ships, ranked for the ship layer.
#[class]
pub struct ShipSearch {
    rows: Vec<ShipRow>,
}

#[class]
impl ShipSearch {
    pub fn new(ships: Vec<ShipInput>) -> Self {
        let rows = ships
            .into_iter()
            .map(|ship| ShipRow::new(ship.param_index, ship.display_name, &ship.tier, &ship.class, &ship.nation))
            .collect();

        Self { rows }
    }

    /// Every ship for an empty query, else the matches, best first.
    pub fn rank(&self, query: String) -> Vec<ShipRowView> {
        rank_ships(&self.rows, &query)
            .into_iter()
            .map(|row| ShipRowView {
                param_index: row.param_index.clone(),
                display_name: row.display_name.clone(),
                label: row.label.clone(),
            })
            .collect()
    }
}

fn read_layer(layer: PaletteLayer, needle: &str) -> Result<Vec<PaletteRow>, LayerError> {
    let mode = match layer {
        PaletteLayer::Players => PaletteMode::Players,
        PaletteLayer::MyShips => PaletteMode::MyShips,
        PaletteLayer::ArmorShips => {
            return Err(LayerError::Rows { source: PaletteError::NotIndexed { mode: PaletteMode::ArmorShips } });
        }
    };
    let runtime = crate::runtime::shared().map_err(|source| LayerError::Runtime { source })?;

    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| LayerError::Open { source })?;
        let rows = index_rows(mode, &pool, needle).await.map_err(|source| LayerError::Rows { source })?;

        Ok(rows.into_iter().map(|row| PaletteRow { label: row.label, query: row.query }).collect())
    })
}

fn layer_of(mode: PaletteMode) -> PaletteLayer {
    match mode {
        PaletteMode::Players => PaletteLayer::Players,
        PaletteMode::MyShips => PaletteLayer::MyShips,
        PaletteMode::ArmorShips => PaletteLayer::ArmorShips,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_label_is_translated() {
        let text = Palette::text(vec!["Search".to_owned()]);

        assert_eq!(text.go_to, ["Go to: Search"]);
        assert_eq!(text.layers.len(), 3);
        for label in [&text.title, &text.open_replay_file, &text.theme_dark, &text.layers[2].title] {
            assert!(!label.is_empty() && !label.starts_with("ui."), "{label:?} is not a translation");
        }
    }

    #[test]
    fn the_seeded_searches_parse_back_to_their_seeds() {
        let text = Palette::text(Vec::new());
        let parsed: Vec<_> =
            text.seeded.iter().map(|seeded| query_text::parse_query(&seeded.query).expect("parses")).collect();

        assert!(parsed.contains(&seed::games_i_died_in()));
        assert!(parsed.contains(&seed::games_i_won()));
    }

    #[test]
    fn the_ship_layer_ranks_the_catalogue() {
        let ship = |index: &str, name: &str| ShipInput {
            param_index: index.to_owned(),
            display_name: name.to_owned(),
            tier: "X".to_owned(),
            class: "Battleship".to_owned(),
            nation: "Japan".to_owned(),
        };
        let search = ShipSearch::new(vec![ship("PJSB018", "Yamato"), ship("PJSB510", "Musashi")]);

        assert_eq!(search.rank(String::new()).len(), 2);
        let found = search.rank("yama".to_owned());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].param_index, "PJSB018");
    }
}
