//! What the shell offers on the palette itself: the commands whose work it or the shared services
//! can do without a tab.

use rust_i18n::t;
use wows_toolkit_replay_data::replay_index::IndexMode;
use wows_toolkit_viewmodel::palette::PaletteMode;
use wows_toolkit_viewmodel::settings::keys;
use zgui::reactive::RwSignal;
use zgui::task::spawn_detached;
use zgui::view;
use zgui::view::AnyView;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogDescriptionProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;

use crate::shell::ReplayRequest;
use crate::shell::Shell;
use crate::shell::crash;
use crate::shell::palette::PaletteRun;
use crate::shell::palette::PaletteTheme;
use crate::shell::palette::RootCommand;
use crate::shell::palette::StatsSource;
use crate::shell::palette::TabRequest;
use crate::shell::palette::chosen;
use crate::shell::palette::modes;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::TabKey;

/// "Games I died in" as the Search tab parses it, so confirming the entry fills the bar with
/// something the reader can edit. A loss in which the reader's own ship sank, as the egui palette
/// seeds it: `self.survived:false` alone also finds the wins they sank in.
const GAMES_I_DIED_IN: &str = "outcome:loss self.survived:false";
/// "Games I won" as the Search tab parses it.
const GAMES_I_WON: &str = "outcome:win";

/// Offers everything the shell can do, for as long as the calling scope lasts.
pub fn offer(shell: Shell) {
    let palette = shell.palette;
    let services = shell.services();

    for tab in AppTab::ALL {
        // Listed as the advanced search instead.
        if tab == AppTab::Search {
            continue;
        }
        palette.offer(RootCommand::GoTo(tab), PaletteRun::action(move || shell.select(TabKey::App(tab))));
    }
    palette.offer(RootCommand::OpenSettings, PaletteRun::action(move || shell.settings_drawer.open()));
    palette.offer(RootCommand::AdvancedSearch, PaletteRun::action(move || shell.select(TabKey::App(AppTab::Search))));

    palette.offer_mode(PaletteMode::Players, modes::players(services));
    palette.offer_mode(PaletteMode::MyShips, modes::my_ships(services));
    // A test offers its own ship list in place of the catalogue.
    #[cfg(test)]
    let offers_ships = TEST_MODES.with_borrow(|modes| modes.ships.is_none());
    #[cfg(not(test))]
    let offers_ships = true;
    if offers_ships {
        let armor = shell.armor;
        palette
            .offer_mode(PaletteMode::ArmorShips, modes::armor_ships(armor.ship_list(services), move || armor.want()));
    }

    palette.offer(RootCommand::GamesIDiedIn, PaletteRun::Request(TabRequest::Search(GAMES_I_DIED_IN.to_owned())));
    palette.offer(RootCommand::GamesIWon, PaletteRun::Request(TabRequest::Search(GAMES_I_WON.to_owned())));

    palette.offer(RootCommand::OpenReplayFile, PaletteRun::action(move || open_replay_file(shell)));
    palette
        .offer(RootCommand::IndexAllReplays, PaletteRun::action(move || shell.build_replay_index(IndexMode::FillGaps)));
    palette.offer(RootCommand::RefreshPersistedData, PaletteRun::action(move || confirm_refresh_persisted_data(shell)));
    palette.offer(RootCommand::CopyLatestLog, PaletteRun::action(move || crash::copy_latest_log(shell)));
    palette.offer(RootCommand::CopyBugReportInfo, PaletteRun::action(move || crash::copy_bug_report_info(shell)));

    for theme in PaletteTheme::ALL {
        palette.offer(RootCommand::SetTheme(theme), PaletteRun::action(move || shell.set_theme(theme.choice())));
    }

    palette.offer(RootCommand::SampleStats, PaletteRun::Request(TabRequest::ShowStats(StatsSource::Sample)));
    palette.offer(RootCommand::RestoreStats, PaletteRun::Request(TabRequest::ShowStats(StatsSource::Recorded)));

    #[cfg(test)]
    offer_test_modes(shell);
}

/// The modes a tab offers, listed for a test in place of the tab's own data.
#[cfg(test)]
#[derive(Clone, Default)]
pub struct TestModes {
    pub ships: Option<Vec<modes::ArmorShipRow>>,
    pub folders: Option<Vec<std::path::PathBuf>>,
    /// Read in place of the replay index for the players list.
    pub players: Option<std::rc::Rc<dyn Fn(String) -> super::ModeQuery>>,
}

#[cfg(test)]
thread_local! {
    /// What the next shell built on this thread offers beside its own modes.
    pub static TEST_MODES: std::cell::RefCell<TestModes> = std::cell::RefCell::default();
}

#[cfg(test)]
fn offer_test_modes(shell: Shell) {
    use zgui::reactive::Signal;

    let offered = TEST_MODES.with_borrow(Clone::clone);
    if let Some(ships) = offered.ships {
        let ships = Signal::stored_local(modes::ShipList::Ready(ships.into()));
        shell.palette.offer_mode(PaletteMode::ArmorShips, modes::armor_ships(ships, || {}));
    }
    if let Some(folders) = offered.folders {
        shell
            .palette
            .offer_mode(PaletteMode::ReplayFolders, modes::replay_folders(shell, Signal::stored_local(folders)));
    }
    if let Some(players) = offered.players {
        let spec = super::ModeSpec { source: super::ModeSource::Query(players), on_enter: None };
        shell.palette.offer_mode(PaletteMode::Players, spec);
    }
}

/// Asks for a replay and opens it in the replays tab.
pub fn open_replay_file(shell: Shell) {
    let asked = wows_toolkit_services::dialog::pick_file(None, Some(wows_toolkit_services::dialog::REPLAYS));
    // Detached: the palette that asked has closed before the reader answers.
    spawn_detached(async move {
        let Some(file) = chosen(shell, asked).await else { return };
        if shell.is_unmounted() {
            return;
        }
        // What the settings drawer reports as the current replay: the one the reader named, which
        // is what the egui app writes here too.
        shell.services().settings.edit(keys::CURRENT_REPLAY_PATH, |settings| {
            settings.current_replay_path = file.clone();
            file.clone()
        });
        shell.request_replay(ReplayRequest::OpenReplay(file));
    });
}

/// Asks before reading every replay again, which takes a while and rewrites what is stored.
fn confirm_refresh_persisted_data(shell: Shell) {
    shell.dialogs.open(move |handle| {
        let open = RwSignal::new_local(true);
        AnyView::new(view! {
            AlertDialog(open = open, on_open_change = handle.on_open_change()) {
                AlertDialogContent(attr:data-testid = "refresh-persisted-data-confirm") {
                    AlertDialogHeader {
                        AlertDialogTitle {{t!("ui.windows.refresh_persisted_data").into_owned()}}
                        AlertDialogDescription {{t!("ui.dialogs.refresh_persisted_data_message").into_owned()}}
                    }
                    AlertDialogFooter {
                        AlertDialogCancel {{t!("ui.buttons.cancel").into_owned()}}
                        AlertDialogAction(
                            attr:data-testid = "refresh-persisted-data-ok",
                            on:click = move |_| shell.build_replay_index(IndexMode::RefreshAll)
                        ) {
                            {t!("ui.buttons.refresh_persisted_data_confirm").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

#[cfg(test)]
mod tests {
    use wows_toolkit_config::index::query_ast::MatchExpr;
    use wows_toolkit_config::index::query_text::parse_query;
    use wows_toolkit_viewmodel::query_bar::seed;

    use super::GAMES_I_DIED_IN;
    use super::GAMES_I_WON;

    /// Each seeded search parses to the expression the egui palette seeds: a query the Search tab
    /// reads differently is a row that finds the wrong battles.
    #[test]
    fn the_seeded_searches_mean_what_they_say() {
        let parsed = |query: &str| -> MatchExpr {
            parse_query(query).unwrap_or_else(|err| panic!("{query:?} does not parse: {err:?}"))
        };

        assert_eq!(parsed(GAMES_I_DIED_IN), seed::games_i_died_in());
        assert_eq!(parsed(GAMES_I_WON), seed::games_i_won());
    }
}
