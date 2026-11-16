//! Naming a search result's ship.
//!
//! Which name a result row carries is the same decision in both front ends;
//! where the loaded game data comes from is not.

use wows_replays::types::GameParamId;
use wows_toolkit_config::index::query_ast::Expr;
use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_config::index::query_ast::MatchField;
use wows_toolkit_config::index::query_ast::MatchTerm;
use wows_toolkit_config::index::rows::MatchHit;
use wowsunpack::data::ResourceLoader as _;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;

/// The name stored with the match, if it is one.
///
/// A row indexed before ship names were stored carries an empty string, and
/// one indexed against a build that could not name the ship carries the id
/// back as text; neither is a name.
fn stored_ship_name(hit: &MatchHit, ship_id: GameParamId) -> Option<&str> {
    let stored = hit.self_ship_name.as_deref()?;
    if stored.is_empty() || stored == ship_id.to_string() {
        return None;
    }
    Some(stored)
}

/// The ship name a loaded provider resolves, if it resolves one.
pub fn try_resolve_ship_name(ship_id: GameParamId, provider: Option<&GameMetadataProvider>) -> Option<String> {
    let provider = provider?;
    let param = GameParamProvider::game_param_by_id(provider, ship_id)?;
    provider.localized_name_from_param(&param)
}

/// The display name for a hit's self ship, given whatever name the match's
/// own build resolves for it (`live`) when that build's game data is loaded.
///
/// `live` wins. It is the same source every other surface names ships from,
/// so preferring it keeps the tab in the app's current locale, while a stored
/// name is frozen in whatever locale was active when the match was indexed.
/// The stored name is the fallback for the case the whole rule exists for: a
/// match whose build's game data is no longer installed. A bracketed id is
/// the last resort when neither can name the ship.
pub fn ship_display_name(hit: &MatchHit, live: Option<String>) -> Option<String> {
    let ship_id = hit.self_ship_id?;
    if let Some(live) = live {
        return Some(live);
    }
    if let Some(stored) = stored_ship_name(hit, ship_id) {
        return Some(stored.to_owned());
    }
    Some(format!("[{ship_id}]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `MatchHit` has no `Default`, so the fixture names every field; only
    /// the two the rule reads carry anything.
    fn hit(ship_id: Option<GameParamId>, stored: Option<&str>) -> MatchHit {
        use wows_replays::types::ArenaId;
        use wows_toolkit_config::index::rows::MatchOutcome;
        use wows_toolkit_config::index::rows::SourceId;

        MatchHit {
            arena_id: ArenaId::from(1i64),
            timestamp: jiff::Timestamp::UNIX_EPOCH,
            map: String::new(),
            game_mode: String::new(),
            game_mode_id: None,
            game_type: String::new(),
            match_group: String::new(),
            version_build: None,
            source_id: SourceId(0),
            outcome: MatchOutcome::Unknown,
            self_account_id: None,
            self_ship_id: ship_id,
            self_ship_name: stored.map(str::to_string),
            self_survived: None,
            self_damage: None,
            self_kills: None,
            self_pr: None,
            results_available: false,
            replay_path: std::path::PathBuf::new(),
            file_mtime: None,
        }
    }

    #[test]
    fn a_live_name_wins_over_a_stored_one() {
        let hit = hit(Some(GameParamId::from(1u64)), Some("Frozen Name"));
        assert_eq!(ship_display_name(&hit, Some("Live Name".into())), Some("Live Name".to_string()));
    }

    #[test]
    fn a_stored_name_carries_a_build_that_is_no_longer_installed() {
        let hit = hit(Some(GameParamId::from(1u64)), Some("Frozen Name"));
        assert_eq!(ship_display_name(&hit, None), Some("Frozen Name".to_string()));
    }

    /// The id written back as text is not a name, and neither is an empty
    /// string; both fall through to the bracketed id.
    #[test]
    fn a_stored_id_or_blank_is_not_a_name() {
        let ship_id = GameParamId::from(4_179_539_664u64);
        let expected = Some(format!("[{ship_id}]"));

        assert_eq!(ship_display_name(&hit(Some(ship_id), Some(&ship_id.to_string())), None), expected);
        assert_eq!(ship_display_name(&hit(Some(ship_id), Some("")), None), expected);
        assert_eq!(ship_display_name(&hit(Some(ship_id), None), None), expected);
    }

    #[test]
    fn a_hit_with_no_ship_names_none() {
        assert_eq!(ship_display_name(&hit(None, Some("Whatever")), Some("Live".into())), None);
    }
}

/// Whether a query filters on game mode at all.
pub fn references_game_mode(expr: &MatchExpr) -> bool {
    match expr {
        Expr::Leaf(MatchTerm::Field(MatchField::GameMode, ..)) => true,
        Expr::Leaf(_) => false,
        other => other.children().iter().any(references_game_mode),
    }
}

/// Whether a "some matches have no recorded game mode" hint belongs on
/// screen.
///
/// Both conditions matter: some indexed match must be missing its game mode,
/// *and* the query on screen must filter on it. A gap the query never asked
/// about is not this user's problem right now, so it stays quiet. What the
/// hint then says is each front end's own wording.
pub fn game_mode_gap_applies(missing_count: i64, expr: &MatchExpr) -> bool {
    missing_count > 0 && references_game_mode(expr)
}

#[cfg(test)]
mod gap_hint_tests {
    use super::*;
    use wows_toolkit_config::index::query_text;

    fn parse(query: &str) -> MatchExpr {
        query_text::parse_query(query).expect("the fixture query parses")
    }

    #[test]
    fn a_query_that_never_mentions_the_mode_stays_quiet() {
        assert!(!game_mode_gap_applies(12, &parse("map:ocean")));
    }

    #[test]
    fn a_full_index_stays_quiet_even_for_a_mode_query() {
        assert!(!game_mode_gap_applies(0, &parse("mode:domination")));
    }

    #[test]
    fn a_mode_query_over_a_gapped_index_gets_the_hint() {
        assert!(game_mode_gap_applies(12, &parse("mode:domination")));
        assert!(game_mode_gap_applies(1, &parse("mode:domination")));
    }

    #[test]
    fn a_mode_term_nested_under_a_boolean_still_counts() {
        assert!(game_mode_gap_applies(3, &parse("map:ocean AND mode:domination")));
    }
}
