//! Naming a search result's ship.
//!
//! Which name a result row carries is the same decision in both front ends;
//! where the loaded game data comes from is not.

use wows_replays::types::GameParamId;
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
