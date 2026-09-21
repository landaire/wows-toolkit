//! Pure assembly of the two-line replay-listing row: which stats win when both
//! the index and an in-memory parse have an opinion, and how the two lines read
//! for each grouping mode.
//!
//! Shared so the listing reads the same in both front ends; each draws the
//! assembled lines with its own text layout.

use rust_i18n::t;
use wows_replays::ReplayMeta;
use wows_replays::ReplayMetaRef;
use wows_replays::types::GameParamId;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::rows::DivisionMate;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::RowSummary;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::formatting::separate_number;

/// What a listing keeps for one replay file: the raw metadata fields its row
/// draws, and nothing else.
///
/// A hydrated `Replay` holds an `Arc<GameMetadataProvider>` and an
/// `Arc<GameConstants>` for the build it was recorded on, plus the whole packet
/// stream. Keeping one per listed file makes a directory spanning thirty builds
/// hold thirty builds' game data resident. These fields are translated at draw
/// time against the currently loaded metadata provider, which is also what
/// makes a locale change show up without re-reading anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedReplay {
    /// The perspective player's ship. `None` for a spectator recording.
    pub ship_id: Option<GameParamId>,
    pub map_name: String,
    /// `ReplayMeta::gameType`, empty when the replay carries none.
    pub game_type: String,
    pub scenario: String,
    /// Raw `dd.mm.yyyy HH:MM:SS` from the replay meta.
    pub date_time: String,
    /// The build the replay was recorded on, from
    /// `ReplayMeta::clientVersionFromExe`. `None` when the header names none
    /// or names one that will not parse. Read so a listing can warm the
    /// build its replays actually need, which is not always the installed
    /// one.
    pub build: Option<u32>,
}

impl ListedReplay {
    pub fn from_meta(meta: &ReplayMeta) -> Self {
        ListedReplay {
            ship_id: meta.vehicles.iter().find(|vehicle| vehicle.relation == 0).map(|vehicle| vehicle.shipId),
            map_name: meta.mapName.clone(),
            game_type: meta.gameType.clone().unwrap_or_default(),
            scenario: meta.scenario.clone(),
            date_time: meta.dateTime.clone(),
            build: wowsunpack::data::Version::try_from_client_exe(&meta.clientVersionFromExe)
                .and_then(|version| version.build)
                .map(|build| build.get()),
        }
    }

    /// Same extraction from the borrowed metadata parse, so bulk directory
    /// scans only allocate the handful of fields a row keeps.
    pub fn from_meta_ref(meta: &ReplayMetaRef<'_>) -> Self {
        ListedReplay {
            ship_id: meta.vehicles.iter().find(|vehicle| vehicle.relation == 0).map(|vehicle| vehicle.shipId),
            map_name: meta.mapName.clone().into_owned(),
            // Absent gameType renders as an empty mode column, matching
            // `from_meta`.
            game_type: meta.gameType.clone().map(std::borrow::Cow::into_owned).unwrap_or_default(),
            scenario: meta.scenario.clone().into_owned(),
            date_time: meta.dateTime.clone().into_owned(),
            build: wowsunpack::data::Version::try_from_client_exe(&meta.clientVersionFromExe)
                .and_then(|version| version.build)
                .map(|build| build.get()),
        }
    }
}

/// Identity fields lifted off a `Replay` before any layout work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowIdentity {
    pub ship: String,
    pub map: String,
    pub scenario: String,
    pub mode: String,
    /// Raw `dd.mm.yyyy HH:MM:SS` from the replay meta.
    pub date_time: String,
}

impl RowIdentity {
    /// The `HH:MM:SS` half of `date_time`, or the whole string when it does not
    /// split (a meta field we do not control).
    fn time_part(&self) -> &str {
        self.date_time.split(' ').nth(1).unwrap_or(&self.date_time)
    }
}

/// Drops a trailing `:SS` from a `HH:MM:SS`-shaped string, leaving anything
/// else untouched. `Replay::game_time()` is the raw `dateTime` field from
/// replay metadata, whose exact shape is not guaranteed across game versions,
/// so this only acts once two-digit groups are confirmed on both sides of the
/// last two colons (a plain `HH:MM` has only one colon, so its `MM` is never
/// mistaken for seconds). No fixed-width slicing, so a shorter or
/// differently-shaped string is returned unchanged rather than panicking.
fn strip_seconds(time: &str) -> &str {
    let two_digits = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());

    let Some(last_colon) = time.rfind(':') else { return time };
    let seconds = &time[last_colon + 1..];
    if !two_digits(seconds) {
        return time;
    }

    let rest = &time[..last_colon];
    let Some(prev_colon) = rest.rfind(':') else { return time };
    let minutes = &rest[prev_colon + 1..];
    if !two_digits(minutes) {
        return time;
    }

    rest
}

/// Stats read off a fully parsed replay held in memory.
pub struct ParsedStats {
    pub outcome: MatchOutcome,
    pub damage: Option<u64>,
    pub kills: Option<i64>,
    pub in_division: bool,
}

/// What the row draws once precedence has been applied.
#[derive(Debug, Clone, PartialEq)]
pub struct RowStats {
    pub outcome: MatchOutcome,
    pub damage: Option<u64>,
    pub kills: Option<i64>,
    pub survived: Option<bool>,
    pub in_division: bool,
    /// The other players who shared the division, for the hover tooltip. Always
    /// empty on the parsed branch: a parsed report carries no mate roster.
    pub division_mates: Vec<DivisionMate>,
    /// False only when neither an index summary nor a parsed report exists.
    pub known: bool,
}

/// A parsed report in memory is fresher than the index for everything it
/// carries, so it wins. Survival is the exception: `PlayerReport` records
/// `time_lived_secs` and no survival flag, so that field always comes from the
/// index regardless of what is parsed.
pub fn resolve_row_stats(parsed: Option<ParsedStats>, summary: Option<&RowSummary>) -> RowStats {
    let survived = summary.and_then(|s| s.self_survived);
    match parsed {
        Some(parsed) => RowStats {
            outcome: parsed.outcome,
            damage: parsed.damage,
            kills: parsed.kills,
            survived,
            in_division: parsed.in_division,
            division_mates: Vec::new(),
            known: true,
        },
        None => match summary {
            // A division id with nobody else recorded in it is not a division:
            // deriving the flag from the mate list keeps the glyph and the
            // tooltip consistent, rather than the glyph firing on a division_id
            // whose mate line would then render empty.
            Some(s) => RowStats {
                outcome: s.outcome,
                damage: s.self_damage,
                kills: s.self_kills,
                survived,
                in_division: !s.division_mates.is_empty(),
                division_mates: s.division_mates.clone(),
                known: true,
            },
            None => RowStats {
                outcome: MatchOutcome::Unknown,
                damage: None,
                kills: None,
                survived: None,
                in_division: false,
                division_mates: Vec::new(),
                known: false,
            },
        },
    }
}

/// Line 1: what match this is. Each grouping omits the field its group header
/// already states.
pub fn identity_line(identity: &RowIdentity, grouping: ReplayGrouping) -> String {
    match grouping {
        ReplayGrouping::Ship => identity.map.clone(),
        ReplayGrouping::Date | ReplayGrouping::None => format!("{} - {}", identity.ship, identity.map),
    }
}

/// The timestamp half of line 2: just the time when the group header already
/// states the date, the full `dd.mm.yyyy HH:MM` otherwise. Seconds are
/// dropped in both cases so the value cannot be misread as a match duration.
fn timestamp_for(identity: &RowIdentity, grouping: ReplayGrouping) -> String {
    match grouping {
        ReplayGrouping::Date => strip_seconds(identity.time_part()).to_string(),
        ReplayGrouping::Ship | ReplayGrouping::None => strip_seconds(&identity.date_time).to_string(),
    }
}

/// Line 2 as drawn: icons instead of words, so adjacent stats never read as
/// one phrase (a word-based "0 kills sunk" was misread as "2 sunk"). Survival
/// is not shown here at all; it costs no row width in the hover tooltip
/// instead. The timestamp carries a clock glyph and drops its seconds so it
/// cannot be misread as a match duration.
pub fn stats_line(identity: &RowIdentity, stats: &RowStats, grouping: ReplayGrouping, locale: Option<&str>) -> String {
    stats_line_parts(identity, stats, grouping, locale)
        .iter()
        .map(|part| match part {
            LinePart::Text(text) => text.as_str(),
            LinePart::Glyph(glyph) => glyph,
        })
        .collect()
}

/// One piece of an assembled line.
///
/// A front end whose text font carries the icon glyphs draws the joined
/// [`stats_line`] and is done; one that keeps its icons in a separate font
/// draws the parts, putting only the glyphs in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinePart {
    Text(String),
    Glyph(&'static str),
}

/// The stats line in pieces: which figures it carries, in what order, and
/// which glyph introduces each.
pub fn stats_line_parts(
    identity: &RowIdentity,
    stats: &RowStats,
    grouping: ReplayGrouping,
    locale: Option<&str>,
) -> Vec<LinePart> {
    let when = timestamp_for(identity, grouping);

    if !stats.known {
        return vec![
            LinePart::Text(format!("{}  ", t!("ui.replay.row_not_indexed"))),
            LinePart::Glyph(crate::glyphs::CLOCK),
            LinePart::Text(format!(" {when}")),
        ];
    }

    // Two spaces separate the figures, and the timestamp always closes the
    // line, so each figure carries its own trailing separator.
    let mut parts = Vec::new();
    if let Some(damage) = stats.damage {
        parts.push(LinePart::Glyph(crate::glyphs::CROSSHAIR_SIMPLE));
        parts.push(LinePart::Text(format!(" {}  ", separate_number(damage, locale))));
    }
    if let Some(kills) = stats.kills {
        parts.push(LinePart::Glyph(crate::glyphs::SWORD));
        parts.push(LinePart::Text(format!(" {kills}  ")));
    }
    parts.push(LinePart::Glyph(crate::glyphs::CLOCK));
    parts.push(LinePart::Text(format!(" {when}")));
    parts
}

/// The word-based equivalent of [`stats_line`], used only for the hover
/// tooltip so the icon convention stays discoverable and the translation keys
/// stay in use.
fn stats_words(stats: &RowStats, when: &str, locale: Option<&str>) -> String {
    if !stats.known {
        return format!("{}  {when}", t!("ui.replay.row_not_indexed"));
    }

    let mut parts: Vec<String> = Vec::new();
    if let Some(damage) = stats.damage {
        parts.push(separate_number(damage, locale));
    }
    if let Some(kills) = stats.kills {
        parts.push(t!("ui.replay.row_kills", count = kills).to_string());
    }
    match stats.survived {
        Some(true) => parts.push(t!("ui.replay.row_survived").to_string()),
        Some(false) => parts.push(t!("ui.replay.row_sunk").to_string()),
        None => {}
    }
    parts.push(when.to_string());
    parts.join("  ")
}

/// The division member list line, formatted `[CLAN] Name` per member when the
/// member has a clan and bare `Name` otherwise. `None` when there are no
/// mates, so the tooltip never shows an empty division line.
fn division_line(stats: &RowStats) -> Option<String> {
    let members = division_members(stats)?;
    Some(t!("ui.replay.row_division", members = members).to_string())
}

/// The division mates, `[CLAN] Name` per member where there is a clan and a
/// bare name otherwise. `None` when the row has no mates.
fn division_members(stats: &RowStats) -> Option<String> {
    if stats.division_mates.is_empty() {
        return None;
    }
    let members = stats
        .division_mates
        .iter()
        .map(|m| if m.clan.is_empty() { m.player_name.clone() } else { format!("[{}] {}", m.clan, m.player_name) })
        .collect::<Vec<_>>()
        .join(", ");
    Some(members)
}

/// One labelled fact about a replay, for a hover that lays them out rather
/// than running them together.
///
/// The label is already translated: a caller draws the pair, it does not
/// decide what either side says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverFact {
    pub label: String,
    pub value: String,
}

/// What a row's hover says, one labelled fact per line.
///
/// The two drawn lines omit the scenario and the game mode to keep the panel
/// narrow, and the second draws icons rather than words, so the hover is
/// where both kinds of detail live. A fact with nothing to say is left out
/// rather than shown empty: an unindexed replay has no damage to report, and
/// a row with no division mates has no division.
pub fn hover_facts(identity: &RowIdentity, stats: &RowStats, locale: Option<&str>) -> Vec<HoverFact> {
    let fact = |key: &str, value: String| HoverFact { label: t!(key).into_owned(), value };

    let mut facts =
        vec![fact("ui.replay.hover.ship", identity.ship.clone()), fact("ui.replay.hover.map", identity.map.clone())];

    // The scenario and the mode name the same match from two angles
    // ("Domination", "Random Battle"); one line reads as one fact.
    let mode = match (identity.scenario.is_empty(), identity.mode.is_empty()) {
        (false, false) => format!("{} - {}", identity.scenario, identity.mode),
        (false, true) => identity.scenario.clone(),
        (true, false) => identity.mode.clone(),
        (true, true) => String::new(),
    };
    if !mode.is_empty() {
        facts.push(fact("ui.replay.hover.mode", mode));
    }
    facts.push(fact("ui.replay.hover.played", identity.date_time.clone()));

    if !stats.known {
        facts.push(fact("ui.replay.hover.result", t!("ui.replay.hover_not_indexed").into_owned()));
        return facts;
    }

    if let Some(damage) = stats.damage {
        facts.push(fact("ui.replay.hover.damage", separate_number(damage, locale)));
    }
    if let Some(kills) = stats.kills {
        facts.push(fact("ui.replay.hover.kills", kills.to_string()));
    }
    match stats.survived {
        Some(true) => facts.push(fact("ui.replay.hover.result", t!("ui.replay.hover_survived").into_owned())),
        Some(false) => facts.push(fact("ui.replay.hover.result", t!("ui.replay.hover_sunk").into_owned())),
        None => {}
    }
    if let Some(division) = division_members(stats) {
        facts.push(fact("ui.replay.hover.division", division));
    }
    facts
}

/// Hover text for a row. The two drawn lines omit scenario and game mode to
/// keep the panel narrow, and line 2 draws icons rather than words, so the
/// tooltip is where both kinds of detail live. The division member line is
/// only present when the row has at least one division mate.
pub fn hover_text(identity: &RowIdentity, stats: &RowStats, locale: Option<&str>) -> String {
    let stats_text = stats_words(stats, &identity.date_time, locale);
    let mut lines = vec![
        identity.ship.clone(),
        identity.map.clone(),
        identity.scenario.clone(),
        identity.mode.clone(),
        identity.date_time.clone(),
    ];
    if let Some(division) = division_line(stats) {
        lines.push(division);
    }
    lines.push(stats_text);
    lines.join("\n")
}

/// The perspective player's ship name, translated against the currently loaded
/// metadata. A spectator recording names no ship, so it reads as such.
pub fn listed_ship_name(listed: &ListedReplay, metadata_provider: &GameMetadataProvider) -> String {
    listed
        .ship_id
        .and_then(|ship_id| metadata_provider.param_localization_id(ship_id.raw().into()))
        .and_then(|id| metadata_provider.localized_name_from_id(&wowsunpack::data::TranslationKey::new(id)))
        .unwrap_or_else(|| t!("ui.replay.spectator").into())
}

/// Translate a listed replay's raw metadata into what the row draws. Requires
/// the metadata provider for every field except the timestamp.
pub fn listed_row_identity(listed: &ListedReplay, metadata_provider: &GameMetadataProvider) -> RowIdentity {
    use wowsunpack::game_params::translations;

    RowIdentity {
        ship: listed_ship_name(listed, metadata_provider),
        map: translations::translate_map_name(&listed.map_name, metadata_provider),
        scenario: translations::translate_scenario(&listed.scenario, metadata_provider),
        mode: translations::translate_game_mode(&listed.game_type, metadata_provider),
        date_time: listed.date_time.clone(),
    }
}

/// Whether a listed replay's index row still describes the file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowFreshness {
    Fresh,
    /// No index row for this path at all.
    Missing,
    /// The file changed since it was indexed, or one of the two mtimes is
    /// unavailable. Treated as stale rather than fresh so an unknown never
    /// masquerades as up to date.
    Stale,
}

pub fn row_freshness(summary: Option<&RowSummary>, on_disk_mtime: Option<i64>) -> RowFreshness {
    let Some(summary) = summary else {
        return RowFreshness::Missing;
    };
    match (summary.file_mtime, on_disk_mtime) {
        (Some(indexed), Some(on_disk)) if indexed == on_disk => RowFreshness::Fresh,
        _ => RowFreshness::Stale,
    }
}

/// Whether a summary reload should start. False while one is in flight, and
/// false when the cached map already reflects the current index generation.
pub fn should_reload_summaries(loading: bool, cached: Option<u64>, current: u64) -> bool {
    !loading && cached != Some(current)
}

/// Unix-seconds modification time, matching how the index mapper records it.
pub fn file_mtime_secs(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> RowIdentity {
        RowIdentity {
            ship: "Yamato".into(),
            map: "Ocean".into(),
            scenario: "Domination".into(),
            mode: "Randoms".into(),
            date_time: "28.07.2026 14:23:05".into(),
        }
    }

    fn summary() -> RowSummary {
        RowSummary {
            outcome: MatchOutcome::Win,
            self_damage: Some(114_230),
            self_kills: Some(3),
            self_survived: Some(true),
            self_pr: Some(1500.0),
            division_id: Some(4),
            division_mates: vec![DivisionMate { player_name: "Mate".into(), clan: "MATE".into() }],
            results_available: true,
            file_mtime: Some(42),
        }
    }

    fn meta(vehicles: Vec<wows_replays::VehicleInfoMeta>) -> ReplayMeta {
        ReplayMeta {
            matchGroup: None,
            gameMode: 0,
            gameType: Some("RandomBattle".to_string()),
            clientVersionFromExe: "0,0,0,0".to_string(),
            scenarioUiCategoryId: None,
            mapDisplayName: String::new(),
            mapId: 0,
            clientVersionFromXml: String::new(),
            weatherParams: None,
            duration: 0,
            gameLogic: None,
            name: String::new(),
            scenario: "Domination".to_string(),
            playerID: wows_replays::types::AccountId(0),
            vehicles,
            playersPerTeam: 0,
            dateTime: "28.07.2026 14:23:05".to_string(),
            mapName: "spaces/ocean".to_string(),
            playerName: String::new(),
            scenarioConfigId: 0,
            teamsCount: 0,
            logic: None,
            playerVehicle: String::new(),
            battleDuration: None,
        }
    }

    fn vehicle(relation: u32, ship_id: u32) -> wows_replays::VehicleInfoMeta {
        wows_replays::VehicleInfoMeta {
            shipId: GameParamId::from(ship_id),
            relation,
            id: wows_replays::types::PlayerId::from(1i64),
            name: "Someone".to_string(),
        }
    }

    /// The listed entry keeps the raw metadata a row draws and nothing else, so
    /// the fields it carries have to be the ones the identity line needs.
    #[test]
    fn from_meta_keeps_the_perspective_players_ship_and_the_raw_identity_fields() {
        // relation 0 is the recording player; the others are on the roster too,
        // so an implementation taking the first vehicle picks the wrong ship.
        let listed = ListedReplay::from_meta(&meta(vec![vehicle(1, 777), vehicle(0, 4_281_269_200), vehicle(2, 999)]));
        assert_eq!(listed.ship_id, Some(GameParamId::from(4_281_269_200u32)));
        assert_eq!(listed.map_name, "spaces/ocean");
        assert_eq!(listed.game_type, "RandomBattle");
        assert_eq!(listed.scenario, "Domination");
        assert_eq!(listed.date_time, "28.07.2026 14:23:05");
    }

    /// The borrowed and owned constructors must extract identically, since the
    /// bulk scan uses one and single-file paths use the other. One fixture
    /// carries an escaped vehicle name (forcing serde's owned-Cow fallback)
    /// and the other an absent gameType, so both divergence-prone branches
    /// are pinned.
    #[test]
    fn from_meta_ref_matches_from_meta() {
        let mut with_game_type = meta(vec![vehicle(1, 777), vehicle(0, 4_281_269_200), vehicle(2, 999)]);
        with_game_type.vehicles[0].name = "Some\"one".to_string();
        let mut without_game_type = meta(vec![vehicle(0, 777)]);
        without_game_type.gameType = None;

        for owned in [with_game_type, without_game_type] {
            let json = serde_json::to_vec(&owned).expect("meta serializes");
            let borrowed = ReplayMetaRef::from_slice(&json).expect("meta parses");
            assert_eq!(ListedReplay::from_meta_ref(&borrowed), ListedReplay::from_meta(&owned));
        }
    }

    /// A spectator recording has no vehicle of its own, and an absent game type
    /// is an empty string rather than a missing field, since that is what the
    /// translation call takes.
    #[test]
    fn from_meta_leaves_a_spectator_recording_without_a_ship() {
        let mut spectator = meta(vec![vehicle(1, 777)]);
        spectator.gameType = None;
        let listed = ListedReplay::from_meta(&spectator);
        assert_eq!(listed.ship_id, None);
        assert_eq!(listed.game_type, "");
    }

    #[test]
    fn summary_alone_supplies_every_stat() {
        let stats = resolve_row_stats(None, Some(&summary()));
        assert!(stats.known);
        assert_eq!(stats.outcome, MatchOutcome::Win);
        assert_eq!(stats.damage, Some(114_230));
        assert_eq!(stats.kills, Some(3));
        assert_eq!(stats.survived, Some(true));
        assert!(stats.in_division);
    }

    #[test]
    fn parsed_report_overrides_the_summary_except_for_survival() {
        // A parsed `PlayerReport` has no survival boolean at all, only
        // `time_lived_secs`, so survival must keep coming from the index.
        let parsed =
            ParsedStats { outcome: MatchOutcome::Loss, damage: Some(200_000), kills: Some(5), in_division: false };
        let stats = resolve_row_stats(Some(parsed), Some(&summary()));
        assert_eq!(stats.outcome, MatchOutcome::Loss);
        assert_eq!(stats.damage, Some(200_000));
        assert_eq!(stats.kills, Some(5));
        assert!(!stats.in_division);
        assert_eq!(stats.survived, Some(true), "survival still comes from the index");
    }

    #[test]
    fn parsed_report_alone_leaves_survival_unknown() {
        let parsed = ParsedStats { outcome: MatchOutcome::Win, damage: Some(1), kills: Some(0), in_division: true };
        let stats = resolve_row_stats(Some(parsed), None);
        assert!(stats.known);
        assert_eq!(stats.survived, None);
        assert!(stats.in_division);
    }

    #[test]
    fn neither_source_means_unknown() {
        let stats = resolve_row_stats(None, None);
        assert!(!stats.known);
        assert_eq!(stats.outcome, MatchOutcome::Unknown);
        assert_eq!(stats.damage, None);
        assert_eq!(stats.kills, None);
        assert_eq!(stats.survived, None);
        assert!(!stats.in_division);
    }

    #[test]
    fn strip_seconds_drops_only_a_genuine_hh_mm_ss_tail() {
        assert_eq!(strip_seconds("25.07.2026 16:35:06"), "25.07.2026 16:35");
        // Already seconds-less: the lone colon pair must not be mistaken for
        // minutes:seconds and truncated further.
        assert_eq!(strip_seconds("16:35"), "16:35");
        assert_eq!(strip_seconds(""), "");
        // No space and no colons at all: nothing for the helper to find.
        assert_eq!(strip_seconds("not_a_timestamp"), "not_a_timestamp");
    }

    #[test]
    fn identity_line_drops_the_field_its_group_already_states() {
        let id = identity();
        assert_eq!(identity_line(&id, ReplayGrouping::None), "Yamato - Ocean");
        // Date groups are headed by the date, so the row leads with the ship.
        assert_eq!(identity_line(&id, ReplayGrouping::Date), "Yamato - Ocean");
        // Ship groups are headed by the ship, so it would be redundant here.
        assert_eq!(identity_line(&id, ReplayGrouping::Ship), "Ocean");
    }

    #[test]
    fn stats_line_shows_numbers_with_thousands_separators() {
        let stats = resolve_row_stats(None, Some(&summary()));
        let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
        assert!(line.contains("114,230"), "expected separated damage in {line:?}");
        assert!(line.contains(crate::glyphs::CROSSHAIR_SIMPLE), "expected the damage glyph in {line:?}");
        assert!(line.contains(&format!("{} 3", crate::glyphs::SWORD)), "expected the kill glyph and count in {line:?}");
        // Date grouping heads the group with the date, so the row shows the time
        // only, clock-prefixed and with the seconds dropped.
        assert!(
            line.contains(&format!("{} 14:23", crate::glyphs::CLOCK)),
            "expected the clock-prefixed time in {line:?}"
        );
        assert!(!line.contains("14:23:05"), "seconds must be dropped from the drawn row: {line:?}");
        assert!(!line.contains("28.07.2026"), "date is already in the group header: {line:?}");
        assert!(!line.contains(crate::glyphs::SKULL), "the skull glyph was removed from the drawn row: {line:?}");
    }

    #[test]
    fn hover_text_keeps_the_detail_the_drawn_rows_drop() {
        let stats = resolve_row_stats(None, Some(&summary()));
        let hover = hover_text(&identity(), &stats, Some("en-US"));
        assert!(hover.contains("Domination"), "scenario must survive in the tooltip: {hover:?}");
        assert!(hover.contains("Randoms"), "game mode must survive in the tooltip: {hover:?}");
        assert!(hover.contains("28.07.2026 14:23:05"));
        assert!(
            hover.contains(t!("ui.replay.row_kills", count = 3).as_ref()),
            "kills must read as words in the tooltip: {hover:?}"
        );
        assert!(
            hover.contains(t!("ui.replay.row_survived").as_ref()),
            "survival must read as a word in the tooltip: {hover:?}"
        );
    }

    #[test]
    fn hover_text_shows_sunk_in_words_when_the_player_died() {
        let died = RowSummary { self_survived: Some(false), ..summary() };
        let stats = resolve_row_stats(None, Some(&died));
        let hover = hover_text(&identity(), &stats, Some("en-US"));
        assert!(hover.contains("Domination"), "scenario must survive in the tooltip: {hover:?}");
        assert!(hover.contains("Randoms"), "game mode must survive in the tooltip: {hover:?}");
        assert!(
            hover.contains(t!("ui.replay.row_sunk").as_ref()),
            "death must read as a word in the tooltip: {hover:?}"
        );
        assert!(
            !hover.contains(t!("ui.replay.row_survived").as_ref()),
            "the survived and sunk words must not both appear: {hover:?}"
        );
    }

    #[test]
    fn hover_text_names_division_mates_with_clan_tags_when_present() {
        let with_mates = RowSummary {
            division_mates: vec![
                DivisionMate { player_name: "Clanned".into(), clan: "ABC".into() },
                DivisionMate { player_name: "Clanless".into(), clan: "".into() },
            ],
            ..summary()
        };
        let stats = resolve_row_stats(None, Some(&with_mates));
        let hover = hover_text(&identity(), &stats, Some("en-US"));
        assert!(hover.contains("[ABC] Clanned"), "a clanned mate must render as [CLAN] Name: {hover:?}");
        assert!(hover.contains("Clanless"), "a clanless mate must still be named: {hover:?}");
        assert!(!hover.contains("[] Clanless"), "an empty clan must not render bracketed: {hover:?}");
    }

    #[test]
    fn hover_text_has_no_division_line_when_there_are_no_mates() {
        let with_mates = summary();
        let solo = RowSummary { division_mates: Vec::new(), ..summary() };

        let with_mates_hover = hover_text(&identity(), &resolve_row_stats(None, Some(&with_mates)), Some("en-US"));
        let solo_hover = hover_text(&identity(), &resolve_row_stats(None, Some(&solo)), Some("en-US"));

        assert_eq!(
            with_mates_hover.lines().count(),
            solo_hover.lines().count() + 1,
            "a division line must be present exactly when there are mates: with_mates={with_mates_hover:?} solo={solo_hover:?}"
        );
        assert!(with_mates_hover.contains("Mate"), "the mates case must actually name the mate: {with_mates_hover:?}");
        assert!(!solo_hover.contains("Mate"), "the solo case must not carry over the mate name: {solo_hover:?}");
    }

    #[test]
    fn resolve_row_stats_treats_a_division_id_with_no_mates_as_not_in_a_division() {
        // division_id is Some, but the mate list is empty: this is the case that
        // proves in_division is derived from the mates, not from division_id.
        let orphaned = RowSummary { division_mates: Vec::new(), ..summary() };
        assert!(orphaned.division_id.is_some(), "the fixture must still carry a division id for this to be meaningful");
        let stats = resolve_row_stats(None, Some(&orphaned));
        assert!(!stats.in_division, "a division id with no recorded mates must not show the division glyph");
    }

    #[test]
    fn stats_line_shows_the_full_timestamp_without_seconds_when_ungrouped() {
        let stats = resolve_row_stats(None, Some(&summary()));
        let line = stats_line(&identity(), &stats, ReplayGrouping::None, Some("en-US"));
        assert!(
            line.contains(&format!("{} 28.07.2026 14:23", crate::glyphs::CLOCK)),
            "expected the clock-prefixed full timestamp with no seconds in {line:?}"
        );
        assert!(!line.contains("14:23:05"), "seconds must be dropped from the drawn row: {line:?}");
    }

    #[test]
    fn stats_line_falls_back_to_the_not_indexed_string() {
        let stats = resolve_row_stats(None, None);
        let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
        assert!(line.contains(t!("ui.replay.row_not_indexed").as_ref()));
        assert!(!line.contains(','), "no stat numbers should be rendered: {line:?}");
    }

    #[test]
    fn stats_line_never_shows_the_skull_regardless_of_survival() {
        // The drawn row carries no skull; survival shows up only in the hover
        // tooltip. Exact equality, not a substring check,
        // since the row is nothing but the clock-prefixed time once damage and
        // kills are absent.
        for survived in [Some(false), Some(true), None] {
            let partial = RowSummary { self_damage: None, self_kills: None, self_survived: survived, ..summary() };
            let stats = resolve_row_stats(None, Some(&partial));
            let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
            assert_eq!(line, format!("{} 14:23", crate::glyphs::CLOCK), "survived={survived:?}");
            assert!(!line.contains(crate::glyphs::SKULL), "survived={survived:?}: {line:?}");
        }
    }

    #[test]
    fn no_summary_means_the_file_was_never_indexed() {
        assert!(matches!(row_freshness(None, Some(42)), RowFreshness::Missing));
        assert!(matches!(row_freshness(None, None), RowFreshness::Missing));
    }

    #[test]
    fn an_equal_mtime_is_fresh() {
        assert!(matches!(row_freshness(Some(&summary()), Some(42)), RowFreshness::Fresh));
    }

    #[test]
    fn a_changed_mtime_is_stale() {
        // The game appends battle results after a match, which is exactly this.
        assert!(matches!(row_freshness(Some(&summary()), Some(99)), RowFreshness::Stale));
    }

    #[test]
    fn an_unreadable_or_unrecorded_mtime_is_stale_not_fresh() {
        assert!(matches!(row_freshness(Some(&summary()), None), RowFreshness::Stale));
        let no_mtime = RowSummary { file_mtime: None, ..summary() };
        assert!(matches!(row_freshness(Some(&no_mtime), Some(42)), RowFreshness::Stale));
        let neither_mtime = RowSummary { file_mtime: None, ..summary() };
        assert!(
            matches!(row_freshness(Some(&neither_mtime), None), RowFreshness::Stale),
            "None == None is true, so equality-based implementations would wrongly report Fresh here"
        );
    }

    #[test]
    fn should_reload_summaries_starts_on_first_load() {
        assert!(should_reload_summaries(false, None, 5));
    }

    #[test]
    fn should_reload_summaries_skips_when_cached_matches_current() {
        assert!(!should_reload_summaries(false, Some(5), 5));
    }

    #[test]
    fn should_reload_summaries_reloads_when_cached_is_behind() {
        assert!(should_reload_summaries(false, Some(4), 5));
    }

    #[test]
    fn should_reload_summaries_blocks_while_loading() {
        assert!(!should_reload_summaries(true, None, 5), "in-flight load must block the never-loaded case too");
        assert!(!should_reload_summaries(true, Some(4), 5));
        assert!(!should_reload_summaries(true, Some(5), 5));
    }

    #[test]
    fn file_mtime_secs_returns_whole_unix_seconds() {
        // The exact value matters, not just that one is produced. `replay_index`
        // records the mtime as whole seconds since the Unix epoch, and
        // `row_freshness` compares the two for equality: a milliseconds or
        // otherwise-shifted implementation here would mark every indexed replay
        // Stale forever and re-parse the whole library on every launch.
        let dir = std::env::temp_dir().join(format!("wt_listing_row_mtime_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wowsreplay");

        let file = std::fs::File::create(&path).unwrap();
        // Half a second past the mark, so a rounding implementation is caught too.
        let known = std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_000_500);
        file.set_modified(known).unwrap();
        drop(file);

        assert_eq!(file_mtime_secs(&path), Some(1_700_000_000));
        assert_eq!(file_mtime_secs(&dir.join("absent.wowsreplay")), None);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
