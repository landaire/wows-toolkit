//! What an exported video of a replay is called.

use wows_replays::ReplayMeta;

/// The file name (without extension) an exported video gets:
/// `YYYY-MM-DD_HH-MM-SS - player - map - ship`.
///
/// It leads with the battle's start time in a lexically sortable form so that a
/// folder of exports orders chronologically by name alone. `source_stem` is the
/// replay file's own name, which the game builds from the same start time plus
/// the recording player's ship; its timestamp is dropped as redundant, keeping
/// the ship.
pub fn video_file_stem(meta: &ReplayMeta, translated_map: &str, source_stem: Option<&str>) -> String {
    let recorded = source_stem.and_then(split_recorded_stem);
    let tail = match &recorded {
        Some((_, tail)) => Some(*tail),
        None => source_stem,
    }
    .filter(|tail| !tail.is_empty());
    // A replay whose dateTime is unreadable can still be placed by the stamp the
    // game put in its file name.
    let stamp = sortable_stamp(&meta.dateTime).or_else(|| recorded.map(|(stamp, _)| stamp));

    let mut name = String::new();
    if let Some(stamp) = stamp {
        name.push_str(&stamp);
        name.push_str(" - ");
    }
    name.push_str(&meta.playerName);
    name.push_str(" - ");
    name.push_str(translated_map);
    if let Some(tail) = tail {
        name.push_str(" - ");
        name.push_str(tail);
    }

    name
}

/// Reformats the replay `dateTime` field (`DD.MM.YYYY HH:MM:SS`) so that it
/// sorts lexically. `None` when the field is not in that form.
fn sortable_stamp(date_time: &str) -> Option<String> {
    const REPLAY_DATE_FORMAT: &str = "%d.%m.%Y %H:%M:%S";

    let parsed = jiff::civil::DateTime::strptime(REPLAY_DATE_FORMAT, date_time).ok()?;
    Some(parsed.strftime("%Y-%m-%d_%H-%M-%S").to_string())
}

/// Splits the game's own `YYYYMMDD_HHMMSS_<rest>` replay file stem into a
/// sortable timestamp and the remainder (the ship the replay was recorded on).
/// `None` for a stem the user has renamed away from that shape.
fn split_recorded_stem(stem: &str) -> Option<(String, &str)> {
    let (date, rest) = stem.split_at_checked(8)?;
    let (time, rest) = rest.strip_prefix('_')?.split_at_checked(6)?;
    if !date.bytes().all(|b| b.is_ascii_digit()) || !time.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let stamp = format!("{}-{}-{}_{}-{}-{}", &date[..4], &date[4..6], &date[6..], &time[..2], &time[2..4], &time[4..]);

    Some((stamp, rest.strip_prefix('_').unwrap_or(rest)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(date_time: &str) -> ReplayMeta {
        serde_json::from_value(serde_json::json!({
            "matchGroup": "pvp",
            "gameMode": 1,
            "gameType": "RandomBattle",
            "clientVersionFromExe": "0, 0, 0, 0",
            "mapDisplayName": "Test Map",
            "mapId": 1,
            "clientVersionFromXml": "0.0.0",
            "duration": 1200,
            "gameLogic": "RandomBattle",
            "name": "test",
            "scenario": "test",
            "playerID": 1,
            "vehicles": [],
            "playersPerTeam": 1,
            "dateTime": date_time,
            "mapName": "spaces/test",
            "playerName": "Self",
            "scenarioConfigId": 1,
            "teamsCount": 2,
            "logic": "RandomBattle",
            "playerVehicle": "TestShip",
        }))
        .expect("the meta deserializes")
    }

    #[test]
    fn a_video_is_named_by_its_start_player_map_and_ship() {
        let named = video_file_stem(&meta("07.08.2026 14:23:05"), "Ocean", Some("20260807_142305_PJSB018-Yamato"));
        assert_eq!(named, "2026-08-07_14-23-05 - Self - Ocean - PJSB018-Yamato");

        let unreadable = video_file_stem(&meta("soon"), "Ocean", Some("20260807_142305_PJSB018-Yamato"));
        assert_eq!(unreadable, "2026-08-07_14-23-05 - Self - Ocean - PJSB018-Yamato", "the file's stamp stands in");

        let renamed = video_file_stem(&meta("soon"), "Ocean", Some("good game"));
        assert_eq!(renamed, "Self - Ocean - good game");
    }

    #[test]
    fn date_time_reorders_to_sortable_stamp() {
        assert_eq!(sortable_stamp("07.08.2026 14:23:05").as_deref(), Some("2026-08-07_14-23-05"));
        assert_eq!(sortable_stamp("not a date"), None);
    }

    #[test]
    fn game_stem_splits_into_stamp_and_ship() {
        let (stamp, tail) = split_recorded_stem("20260807_142305_PJSB018-Yamato").expect("game stem");
        assert_eq!(stamp, "2026-08-07_14-23-05");
        assert_eq!(tail, "PJSB018-Yamato");
    }

    #[test]
    fn renamed_stem_has_no_recorded_prefix() {
        assert!(split_recorded_stem("my favourite game").is_none());
        assert!(split_recorded_stem("2026080_142305_x").is_none());
        assert!(split_recorded_stem("short").is_none());
    }
}
