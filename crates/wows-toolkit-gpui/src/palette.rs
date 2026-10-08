//! The command palette: everything the window can do, by name.
//!
//! Ports the egui app's `ui/command_palette.rs`, which opens on ctrl+k or
//! ctrl+p over a fuzzy-matched list. What is offered here is the flat root set --
//! going to a tab, setting the theme, opening a replay, the seeded searches.
//! The egui palette's cascading sub-modes search the index or loaded ship
//! catalogue as the reader types.
//!
//! The root list is built once and `Command` filters it; cascading modes query
//! the replay index or the loaded ship catalogue as the reader types.

use gpui_kit::component::command::CommandItem;
use rust_i18n::t;

use crate::app::AppTab;
use wows_toolkit_viewmodel::settings::ThemeChoice;

/// What confirming an entry asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    GoTo(AppTab),
    SetTheme(ThemeChoice),
    OpenReplayFile,
    /// Put a query in the Search tab's bar and run it.
    SearchFor(String),
    /// Fill the palette with what one of the cascading modes offers.
    ///
    /// The egui palette's own three (`ui/command_palette.rs`'s `SubKind`): a
    /// player to find matches with, a ship of the reader's own to list their
    /// matches in, and a ship to look at the armor of.
    EnterMode(PaletteMode),
    /// Look at the armor of one ship, by the parameter that names it.
    ViewArmor {
        param_index: String,
    },
    /// Put the newest log file on the clipboard, for a bug report.
    CopyLatestLog,
    /// List a directory of replays other than the install's.
    OpenReplayDirectory,
    /// Read every replay that is not indexed yet.
    IndexAllReplays,
    /// Read every replay again and rewrite what the index holds, once the reader
    /// has confirmed it.
    RefreshPersistedData,
    /// Contribute every listed battle, as the data-sharing setting asks.
    ContributeAllReplays(crate::upload::LedgerUse),
    /// Take a result mapping the reader points at as the loaded build's.
    ImportConstants,
}

/// Which cascading list the palette is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteMode {
    /// Players the index has seen, most met first.
    Players,
    /// Ships the reader has played, most played first.
    MyShips,
    /// Every ship the loaded build has armor for.
    ArmorShips,
}

impl PaletteMode {
    pub const ALL: [PaletteMode; 3] = [Self::Players, Self::MyShips, Self::ArmorShips];

    /// What the root entry that enters this mode is called.
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.search_players",
            Self::MyShips => "ui.palette.my_matches_in_ship",
            Self::ArmorShips => "ui.palette.view_armor_for_ship",
        }
    }
}

/// Maximum rows returned by one cascading index query.
pub const MODE_LIMIT: i64 = 50;

/// One entry: what it is called and what it does.
#[derive(Clone)]
pub struct PaletteEntry {
    pub label: String,
    pub action: PaletteAction,
}

/// Every entry the palette offers, in the order it lists them.
///
/// Tabs first because going somewhere is the common reason to open it, then
/// the searches, then the settings-like actions.
pub fn entries() -> Vec<PaletteEntry> {
    let mut entries = Vec::new();

    for tab in AppTab::ALL {
        if tab == AppTab::Search {
            continue;
        }
        entries.push(PaletteEntry {
            label: t!("ui.palette.go_to", tab = t!(tab.label_key())).into_owned(),
            action: PaletteAction::GoTo(tab),
        });
    }

    entries.push(PaletteEntry {
        label: t!("ui.palette.advanced_search").into_owned(),
        action: PaletteAction::GoTo(AppTab::Search),
    });

    // The cascading modes first, as the egui palette lists them: they are the
    // entries that lead somewhere rather than doing something.
    for mode in PaletteMode::ALL {
        entries.push(PaletteEntry { label: t!(mode.label_key()).into_owned(), action: PaletteAction::EnterMode(mode) });
    }

    // The two seeded searches the egui palette offers. The queries are the
    // grammar the Search tab parses, so confirming one fills the bar with
    // something the reader can then edit.
    entries.push(PaletteEntry {
        label: t!("ui.palette.games_i_died_in").into_owned(),
        // A battle lost in which the reader's own ship sank, which is the egui
        // seed (`query_bar::seed::games_i_died_in`). `survived:false` alone also
        // returns the wins they sank in.
        action: PaletteAction::SearchFor("outcome:loss self.survived:false".to_owned()),
    });
    entries.push(PaletteEntry {
        label: t!("ui.palette.games_i_won").into_owned(),
        action: PaletteAction::SearchFor("outcome:win".to_owned()),
    });

    entries.push(PaletteEntry {
        label: t!("ui.replay.open_manually").into_owned(),
        action: PaletteAction::OpenReplayFile,
    });
    entries.push(PaletteEntry {
        label: t!("ui.replay.open_directory").into_owned(),
        action: PaletteAction::OpenReplayDirectory,
    });
    entries.push(PaletteEntry {
        label: t!("ui.settings.replay.index_all_replays").into_owned(),
        action: PaletteAction::IndexAllReplays,
    });
    entries.push(PaletteEntry {
        label: t!("ui.replay.refresh_persisted_data").into_owned(),
        action: PaletteAction::RefreshPersistedData,
    });
    entries.push(PaletteEntry {
        label: t!("ui.replay.import_constants").into_owned(),
        action: PaletteAction::ImportConstants,
    });
    entries.push(PaletteEntry {
        label: t!("ui.palette.send_all_replays").into_owned(),
        action: PaletteAction::ContributeAllReplays(crate::upload::LedgerUse::Consult),
    });
    entries.push(PaletteEntry {
        label: t!("ui.palette.send_all_replays_again").into_owned(),
        action: PaletteAction::ContributeAllReplays(crate::upload::LedgerUse::Ignore),
    });
    entries.push(PaletteEntry {
        label: t!("ui.replay.copy_latest_log").into_owned(),
        action: PaletteAction::CopyLatestLog,
    });

    for (key, choice) in [
        ("ui.settings.app.theme_system", ThemeChoice::System),
        ("ui.settings.app.theme_dark", ThemeChoice::Dark),
        ("ui.settings.app.theme_light", ThemeChoice::Light),
    ] {
        entries.push(PaletteEntry {
            label: t!("ui.palette.set_theme", theme = t!(key)).into_owned(),
            action: PaletteAction::SetTheme(choice),
        });
    }

    entries
}

/// The entries as the component's own items, in the same order, so an index
/// into one is an index into the other.
pub fn items(entries: &[PaletteEntry]) -> Vec<CommandItem> {
    entries.iter().map(|entry| CommandItem::new().label(entry.label.clone())).collect()
}

/// Rows one index-backed mode offers for the current query.
///
/// An empty result is distinct from a query failure so the palette can report
/// each one correctly.
pub async fn mode_entries(
    mode: PaletteMode,
    pool: &sqlx::sqlite::SqlitePool,
    needle: &str,
) -> Result<Vec<PaletteEntry>, String> {
    use wows_toolkit_config::index::query;
    use wows_toolkit_viewmodel::query_bar::seed;

    match mode {
        PaletteMode::Players => query::search_players(pool, needle, MODE_LIMIT)
            .await
            .map(|found| {
                found
                    .into_iter()
                    .map(|player| {
                        let named = match player.clan.as_str() {
                            "" => player.latest_name.clone(),
                            clan => format!("[{clan}] {}", player.latest_name),
                        };
                        PaletteEntry {
                            label: format!("{named} -- {}", t!("ui.palette.matches_found", count = player.match_count)),
                            action: PaletteAction::SearchFor(wows_toolkit_config::index::query_text::print_query(
                                &seed::matches_with_player(player.account_id),
                            )),
                        }
                    })
                    .collect()
            })
            .map_err(|err| format!("The indexed players could not be read: {err}")),
        PaletteMode::MyShips => query::search_self_ships(pool, needle, MODE_LIMIT)
            .await
            .map(|found| {
                found
                    .into_iter()
                    .map(|ship| PaletteEntry {
                        label: format!(
                            "{} -- {}",
                            ship.ship_name,
                            t!("ui.palette.matches_found", count = ship.match_count)
                        ),
                        action: PaletteAction::SearchFor(wows_toolkit_config::index::query_text::print_query(
                            &seed::my_matches_in_ship(ship.ship_id),
                        )),
                    })
                    .collect()
            })
            .map_err(|err| format!("The indexed ships could not be read: {err}")),
        // Read from the loaded build rather than the index: armor is a property of
        // the ship, not of anything the reader has played.
        PaletteMode::ArmorShips => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::PaletteAction;
    use super::PaletteMode;
    use super::entries;
    use super::mode_entries;
    use crate::app::AppTab;

    /// Every tab is reachable by name: the palette is the keyboard route to
    /// the tab strip, so a tab missing from it cannot be reached that way.
    #[test]
    fn every_tab_is_offered() {
        let offered: Vec<AppTab> = entries()
            .iter()
            .filter_map(|entry| match entry.action {
                PaletteAction::GoTo(tab) => Some(tab),
                _ => None,
            })
            .collect();
        for tab in AppTab::ALL {
            assert!(offered.contains(&tab), "{tab:?} is not in the palette");
        }
    }

    /// Every seeded search parses, and reads as what its label promises: the
    /// egui palette seeds the same searches as an expression, so the text here
    /// has to come out the same way.
    #[test]
    fn the_seeded_searches_mean_what_they_say() {
        use wows_toolkit_config::index::query_ast::MatchExpr;
        use wows_toolkit_config::index::query_text;
        use wows_toolkit_viewmodel::query_bar::seed;

        let seeded: Vec<String> = entries()
            .iter()
            .filter_map(|entry| match &entry.action {
                PaletteAction::SearchFor(query) => Some(query.clone()),
                _ => None,
            })
            .collect();
        assert!(!seeded.is_empty(), "the palette offers at least one search");

        let parsed: Vec<MatchExpr> = seeded
            .iter()
            .map(|query| {
                query_text::parse_query(query).unwrap_or_else(|err| panic!("{query:?} does not parse: {err:?}"))
            })
            .collect();

        assert!(
            parsed.contains(&seed::games_i_died_in()),
            "a battle the reader sank in is a loss and a sinking, got {parsed:?}"
        );
        assert!(parsed.contains(&seed::games_i_won()), "got {parsed:?}");
    }

    /// All three cascading modes are offered from the root, as the egui palette
    /// offers them: each is the way into a list the reader then filters.
    #[test]
    fn every_cascading_mode_is_offered() {
        let offered: Vec<PaletteMode> = entries()
            .iter()
            .filter_map(|entry| match entry.action {
                PaletteAction::EnterMode(mode) => Some(mode),
                _ => None,
            })
            .collect();

        for mode in PaletteMode::ALL {
            assert!(offered.contains(&mode), "{mode:?} is not in the palette");
        }
    }

    /// A player row leads to the search that finds their battles, and a ship row
    /// to the reader's own battles in it, both as the shared seeds express them.
    #[tokio::test]
    async fn a_mode_row_leads_to_the_search_the_egui_palette_seeds() {
        use wows_toolkit_config::index::query_text;
        use wows_toolkit_viewmodel::query_bar::seed;

        let pool = wows_toolkit_config::test_pool().await;
        // Nothing indexed returns an empty result rather than a query error.
        assert!(mode_entries(PaletteMode::Players, &pool, "").await.expect("query players").is_empty());
        assert!(mode_entries(PaletteMode::MyShips, &pool, "").await.expect("query ships").is_empty());

        // What a row would do, checked against the seed rather than the SQL: a
        // query the Search tab cannot parse back is a row that does nothing.
        let account = wows_replays::types::AccountId(7);
        let printed = query_text::print_query(&seed::matches_with_player(account));
        assert_eq!(
            query_text::parse_query(&printed).expect("the seeded query parses"),
            seed::matches_with_player(account)
        );
    }

    /// Both bulk contributions are offered: the ordinary pass over what has not
    /// been sent, and the one that sends it all again for a reader told the
    /// service lost it. The egui palette offers the same two.
    #[test]
    fn both_bulk_contributions_are_offered() {
        let offered: Vec<crate::upload::LedgerUse> = entries()
            .iter()
            .filter_map(|entry| match entry.action {
                PaletteAction::ContributeAllReplays(ledger) => Some(ledger),
                _ => None,
            })
            .collect();

        assert!(offered.contains(&crate::upload::LedgerUse::Consult));
        assert!(offered.contains(&crate::upload::LedgerUse::Ignore));
    }

    /// Labels are what the reader searches, so an empty one is unreachable.
    #[test]
    fn every_entry_is_named() {
        for entry in entries() {
            assert!(!entry.label.trim().is_empty(), "{:?} has no label", entry.action);
        }
    }
}
