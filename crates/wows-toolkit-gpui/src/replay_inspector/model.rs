//! Presentation row model built from `NormalizedBattleReport`. Pure: no
//! `gpui`/`egui` types anywhere. Mirrors the field mapping in the egui app's
//! `ui/replay_parser/mod.rs` (`PlayerReport` construction, lines 535-851) and
//! `ui/replay_parser/models.rs` (`PlayerReport` field list), with
//! `RichText`/`Color32`/texture fields dropped or replaced by plain
//! strings/`ColorRole` (colors live in `columns.rs`, resolved by cell, not
//! stored per-row). The Dazzle/Incoming Fire Alert skill markers
//! (`util::colorize_captain_points`'s star/siren glyphs) are kept as plain
//! `has_dazzle`/`has_ifa` booleans rather than dropped, so the render layer
//! can rebuild the glyphs without string-matching hover text. Personal
//! Rating is left `None` here and filled in by a separate
//! `populate_personal_ratings` call once PR reference data is loaded,
//! mirroring `UiReport::populate_personal_ratings`.

use std::collections::HashMap;

use serde_json::Value;
use wows_replay_insights::battle_report::AchievementResult;
use wows_replay_insights::battle_report::ConsumableResult;
use wows_replay_insights::battle_report::DAMAGE_DESCRIPTIONS;
use wows_replay_insights::battle_report::Damage;
use wows_replay_insights::battle_report::DamageInteraction;
use wows_replay_insights::battle_report::HITS_DESCRIPTIONS;
use wows_replay_insights::battle_report::Hits;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replay_insights::battle_report::NormalizedPlayer;
use wows_replay_insights::battle_report::POTENTIAL_DAMAGE_DESCRIPTIONS;
use wows_replay_insights::battle_report::PotentialDamage;
use wows_replay_insights::battle_report::RECEIVED_DAMAGE_DESCRIPTIONS;
use wows_replay_insights::battle_report::RibbonResult;
use wows_replay_insights::battle_report::TranslatedBuild;
use wows_replay_insights::personal_rating::PersonalRatingData;
use wows_replay_insights::personal_rating::PersonalRatingResult;
use wows_replay_insights::personal_rating::ShipBattleStats;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::types::AccountId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_replays::types::Relation;
use wows_replays::types::TeamId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::KnownCrewSkill;
use wowsunpack::game_params::types::Species;

use super::columns::ReplayColumn;

/// One player's presentation-ready row: scalars and formatted strings pulled
/// from `NormalizedPlayer`/`ServerResults`. Colors are not stored here; they
/// are derived per-cell in `columns::cell_value` from the plain fields below
/// (relation, division/abuser flags, skill tier, PR category), so this type
/// stays a pure data record.
#[derive(Clone, Debug)]
pub struct PlayerRow {
    pub db_id: AccountId,
    pub team_id: TeamId,
    pub relation: Relation,
    pub is_self: bool,
    pub is_bot: bool,
    pub is_abuser: bool,
    pub is_test_ship: bool,
    /// User-toggleable per-row NDA override. Always `false` fresh out of
    /// `from_normalized`; a later "hide stats" UI action (Milestone 2) flips
    /// it, mirroring the egui app's `PlayerReport::manual_stat_hide_toggle`.
    pub manual_stat_hide_toggle: bool,

    pub display_name: String,
    pub clan_tag: Option<String>,
    /// Packed `0xRRGGBB` clan-league color, `0` when clanless.
    pub clan_color_rgb: u32,
    pub division_label: Option<String>,
    /// True when this row is in the self player's division, but is not the
    /// self player itself (the "gold name" case).
    pub is_self_division_mate: bool,

    pub ship_name: String,
    pub ship_species_text: String,
    pub ship_class: Species,
    /// The ship's `GameParams` id, resolved from `ship_index` via the
    /// metadata provider (`GameParamProvider::game_param_by_index`). `None`
    /// only if the index cannot be resolved against the loaded provider;
    /// used to key `ShipBattleStats` in `populate_personal_ratings`.
    pub ship_id: Option<GameParamId>,
    /// Proxy for "the replay's client observed this ship's vehicle entity"
    /// (`translated_build.is_some()`, which `TranslatedBuild::new` returns
    /// `None` for exactly when `player.vehicle_entity()` is `None`).
    /// `NormalizedPlayer` does not carry the raw flag directly.
    /// `TranslatedBuild::new` also requires a known species, which makes this
    /// proxy narrower than the egui original's plain `vehicle.is_some()` in
    /// theory; in practice they are equivalent, because
    /// `NormalizedBattleReport::from_battle_report` already resolves and
    /// `.expect()`s a known species while building `NormalizedPlayer` (see
    /// `battle_report/mod.rs`), so a row with an unrecognized species never
    /// reaches this type at all.
    pub has_vehicle_entity: bool,

    pub base_xp: Option<i64>,
    pub base_xp_text: Option<String>,
    pub raw_xp: Option<i64>,
    pub raw_xp_text: Option<String>,

    pub observed_damage: u64,
    pub observed_damage_text: String,
    pub observed_kills: i64,

    pub actual_damage: Option<u64>,
    pub actual_damage_report: Option<Damage>,
    pub actual_damage_text: Option<String>,
    pub actual_damage_hover_text: Option<String>,

    pub hits: Option<u64>,
    pub hits_report: Option<Hits>,
    pub hits_text: Option<String>,
    pub hits_hover_text: Option<String>,

    pub spotting_damage: Option<u64>,
    pub spotting_damage_text: Option<String>,
    /// Always `None`: the self-player controller-fallback breakdown
    /// (`build_damage_stat_hover_text`) needs the raw `self_damage_stats()`
    /// list, which `NormalizedBattleReport` does not carry. Only the numeric
    /// total (`spotting_damage`) survives; this is a documented, narrow gap
    /// (self player only, hover text only).
    pub spotting_damage_hover_text: Option<String>,

    pub potential_damage: Option<u64>,
    pub potential_damage_text: Option<String>,
    /// `None` in the self-player controller-fallback case for the same
    /// reason as `spotting_damage_hover_text`; `Some` whenever server
    /// results are present (fully reproducible from `ServerResults`).
    pub potential_damage_hover_text: Option<String>,
    pub potential_damage_report: Option<PotentialDamage>,

    pub received_damage: Option<u64>,
    pub received_damage_text: Option<String>,
    pub received_damage_hover_text: Option<String>,
    pub received_damage_report: Option<Damage>,
    pub damage_interactions: Option<HashMap<AccountId, DamageInteraction>>,

    pub fires: Option<u64>,
    pub floods: Option<u64>,
    pub citadels: Option<u64>,
    pub crits: Option<u64>,

    pub time_lived_secs: Option<u64>,
    pub time_lived_text: Option<String>,

    pub distance_traveled: Option<f64>,

    pub kills: Option<i64>,

    pub heal_count: Option<u32>,

    pub skill_points: usize,
    pub num_skills: usize,
    pub highest_tier: usize,
    pub num_tier_1_skills: usize,
    /// "{points}pts ({skills} skills)"; the tower-defense/warning tier icon
    /// is rebuilt from `skill_warning` and `skill_points` by the render layer
    /// (Milestone 2). Dazzle/IFA star/siren markers are not encoded in this
    /// text; see `has_dazzle`/`has_ifa`.
    pub skill_label_text: String,
    pub skill_hover_text: Option<String>,
    /// True for the "tower defense" (all tier-1 skills) and "no skills above
    /// tier 2" cases, which force the label to the "bad" color regardless of
    /// point tier. Mirrors `util::colorize_captain_points`.
    pub skill_warning: bool,
    /// Learned captain skills include Dazzle. Mirrors the `has_dazzle` scan
    /// in `util::colorize_captain_points`; the render layer (Milestone 2)
    /// prepends a star glyph when true.
    pub has_dazzle: bool,
    /// Learned captain skills include Incoming Fire Alert. Mirrors the
    /// `has_ifa` scan in `util::colorize_captain_points`; the render layer
    /// (Milestone 2) prepends a siren glyph when true.
    pub has_ifa: bool,

    pub translated_build: Option<TranslatedBuild>,
    pub achievements: Vec<AchievementResult>,
    pub ribbons: Vec<RibbonResult>,
    pub consumables: Vec<ConsumableResult>,

    /// Always `None` fresh out of `from_normalized`: PR reference data isn't
    /// loaded at report-construction time, so the egui app computes PR in a
    /// separate step (`UiReport::populate_personal_ratings`) once that data
    /// is available. Call `ReplayReportModel::populate_personal_ratings`
    /// after loading PR data (a later milestone wires that load into the
    /// gpui app) to fill this in.
    pub personal_rating: Option<PersonalRatingResult>,
}

impl PlayerRow {
    /// Mirrors `PlayerReport::should_hide_stats`: true when the user manually
    /// hid this row's stats, or the ship is a test/demo ship and this is not
    /// the self player.
    pub fn should_hide_stats(&self) -> bool {
        self.manual_stat_hide_toggle || (!self.relation.is_self() && self.is_test_ship)
    }
}

/// One in-game chat message, presentation-ready: bot/no-relation sender name
/// and message text are already translated, the message body is already
/// HTML-decoded, and the clan tag/color are plain (no brackets, no
/// `Rc<Player>`). Mirrors the field list the egui app reads off
/// `wows_replays::analyzer::battle_controller::GameMessage` in
/// `build_replay_chat_content` (`ui/replay_parser/mod.rs` ~4893-4960), minus
/// `entity_id` (never read there) and the `Rc<Player>` handle itself (not
/// `Send`; every field the render layer needs is copied out of it here,
/// during the background parse, before the model crosses back to the UI
/// thread).
#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub clock: GameClock,
    /// `None` for a message with no resolvable team relation to the self
    /// player (matches `GameMessage::sender_relation`); rendered gray.
    pub sender_relation: Option<Relation>,
    /// Already translated when the sender is a bot or has no relation
    /// (`metadata_provider.localized_name_from_id`); otherwise the raw
    /// in-game name, exactly as `build_replay_chat_content` resolves it.
    pub sender_name: String,
    pub channel: ChatChannel,
    /// Already HTML-decoded, and already translated when the sender is a bot
    /// or has no relation, same gating as `sender_name`.
    pub message: String,
    /// The sender's clan tag with no surrounding brackets (`Player::clan()`);
    /// `None` when the sender is clanless or has no resolved `Player`.
    pub clan_tag: Option<String>,
    /// Packed `0xRRGGBB` clan-league color; `Some` exactly when `clan_tag`
    /// is. Read from the player's raw `clanColor` property, falling back to
    /// the player's team-relation color for older replays that omit it
    /// (mirrors `clan_color_for_player`).
    pub clan_color_rgb: Option<u32>,
}

/// Team-relation color packed as `0xRRGGBB`, matching
/// `util::formatting::player_color_for_team_relation`: self = white, ally =
/// light green, enemy = light red.
fn team_relation_rgb(relation: Relation) -> u32 {
    if relation.is_self() {
        0xffffff
    } else if relation.is_ally() {
        0x90ee90
    } else {
        0xff8080
    }
}

/// Mirrors `clan_color_for_player`: the clan-league color packed as
/// `0xRRGGBB`, read from the player's raw `clanColor` property. Older
/// replays omit that property; this falls back to the player's own
/// team-relation color (via `Player::relation`, not the message's
/// `sender_relation`, matching the egui original) rather than panicking.
fn clan_color_for_player(player: &Player) -> u32 {
    match player.initial_state().raw_with_names().get("clanColor").and_then(|c| c.as_i64()) {
        Some(clan_color) => (clan_color & 0xFF_FFFF) as u32,
        None => team_relation_rgb(player.relation()),
    }
}

/// Builds the presentation-ready chat log from a battle report's raw
/// `&[GameMessage]` (`wows_battle_world::report::BattleReport::game_chat`).
/// Ports `build_replay_chat_content`'s translation/decode logic
/// (`ui/replay_parser/mod.rs` ~4893-4933) field-for-field, dropping only the
/// rendering itself (Milestone 6's `chat.rs`).
pub fn build_chat_messages(messages: &[GameMessage], metadata_provider: &GameMetadataProvider) -> Vec<ChatMessage> {
    messages.iter().map(|message| chat_message_from_game_message(message, metadata_provider)).collect()
}

fn chat_message_from_game_message(message: &GameMessage, metadata_provider: &GameMetadataProvider) -> ChatMessage {
    let GameMessage { clock, sender_relation, sender_name, channel, message: text, entity_id: _, player } = message;

    // Bots and senders with no resolvable relation get their name and
    // message translated as localization keys; everyone else's text is used
    // verbatim (aside from the HTML decode below), exactly matching
    // `build_replay_chat_content`.
    let is_bot_or_unrelated = sender_relation.is_none() || player.as_ref().is_some_and(|p| p.is_bot());
    let translated_name = is_bot_or_unrelated
        .then(|| metadata_provider.localized_name_from_id(&TranslationKey::new(sender_name.as_str())))
        .flatten();
    let translated_text = is_bot_or_unrelated
        .then(|| metadata_provider.localized_name_from_id(&TranslationKey::new(text.as_str())))
        .flatten();

    let decoded_text = escaper::decode_html(text.as_str()).unwrap_or_else(|_| text.clone());

    let (clan_tag, clan_color_rgb) = match player {
        Some(player) if !player.initial_state().clan().is_empty() => {
            (Some(player.initial_state().clan().to_string()), Some(clan_color_for_player(player)))
        }
        _ => (None, None),
    };

    ChatMessage {
        clock: *clock,
        sender_relation: *sender_relation,
        sender_name: translated_name.unwrap_or_else(|| sender_name.clone()),
        channel: channel.clone(),
        message: translated_text.unwrap_or(decoded_text),
        clan_tag,
        clan_color_rgb,
    }
}

/// A replay's player table: rows plus the outcome and the active column set.
/// `columns` starts as every `ReplayColumn` (mirroring `UiReport::new`'s
/// initial full list); callers apply `columns::default_columns(&settings)` to
/// filter the optional ones.
pub struct ReplayReportModel {
    pub self_team: TeamId,
    pub rows: Vec<PlayerRow>,
    pub battle_result: Option<BattleResult>,
    pub columns: Vec<ReplayColumn>,
    /// Translated map name (`NormalizedBattleReport::metadata.map`), used by
    /// the per-replay dock panel's tab title ("{ship} - {map}").
    pub map: String,
    /// The replay's in-game chat log, presentation-ready. Empty for a replay
    /// with no chat activity.
    pub chat: Vec<ChatMessage>,
}

impl ReplayReportModel {
    /// Builds the presentation model from a normalized battle report. `meta`
    /// and `constants` are accepted for parity with the background-load
    /// bundle the Milestone 5 loader already has on hand; both are fully
    /// folded into `normalized` by the time this runs, so neither is read
    /// here. `chat_messages` is the battle report's raw chat log
    /// (`BattleReport::game_chat`), translated into `ReplayReportModel::chat`
    /// via `build_chat_messages`.
    pub fn from_normalized(
        normalized: &NormalizedBattleReport,
        _meta: &ReplayMeta,
        metadata_provider: &GameMetadataProvider,
        _constants: &Value,
        chat_messages: &[GameMessage],
    ) -> Self {
        let mut model = Self::build(
            normalized,
            |species| {
                metadata_provider
                    .localized_name_from_id(&TranslationKey::new(species.translation_id()))
                    .unwrap_or_else(|| species.name().to_string())
            },
            |ship_index| metadata_provider.game_param_by_index(ship_index).map(|param| param.id()),
        );
        model.chat = build_chat_messages(chat_messages, metadata_provider);
        model
    }

    /// Provider-free core of `from_normalized`. Split out so the row-mapping
    /// logic (self-row identification, damage/hit/xp scalars, NDA-relevant
    /// fields, breakdown gating) is unit-testable without a real
    /// `GameMetadataProvider`, which needs loaded game data and cannot be
    /// cheaply fabricated in a test.
    fn build(
        normalized: &NormalizedBattleReport,
        species_text: impl Fn(Species) -> String,
        ship_id: impl Fn(&str) -> Option<GameParamId>,
    ) -> Self {
        let self_player = normalized.players.iter().find(|p| p.is_self);
        let self_team =
            self_player.map(|p| TeamId::from(p.team_id)).expect("normalized battle report carries no self player");
        let self_division_id = self_player.and_then(|p| p.division_id);
        let self_db_id = self_player.map(|p| p.db_id);

        let rows = normalized
            .players
            .iter()
            .map(|np| {
                PlayerRow::from_normalized_player(
                    np,
                    self_division_id,
                    self_db_id,
                    species_text(np.ship_class),
                    ship_id(&np.ship_index),
                )
            })
            .collect();

        ReplayReportModel {
            self_team,
            rows,
            battle_result: normalized.metadata.battle_result,
            columns: ReplayColumn::ALL.to_vec(),
            map: normalized.metadata.map.clone(),
            chat: Vec::new(),
        }
    }

    /// Populates Personal Rating for every row using externally loaded PR
    /// reference data. Ports `UiReport::populate_personal_ratings`
    /// (`ui/replay_parser/mod.rs:2217-2249`) exactly: build one
    /// `ShipBattleStats` per row from that row's `ship_id`/`actual_damage`/
    /// win-or-loss/`kills`, then hand it to `pr_data.calculate_pr`. A row
    /// whose PR is already `Some`, or that lacks a resolved `ship_id` or
    /// `actual_damage`, is left untouched.
    pub fn populate_personal_ratings(&mut self, pr_data: &PersonalRatingData) {
        let is_win = matches!(self.battle_result, Some(BattleResult::Win(_)));

        for row in &mut self.rows {
            if row.personal_rating.is_some() {
                continue;
            }
            let Some(ship_id) = row.ship_id else {
                continue;
            };
            let Some(actual_damage) = row.actual_damage else {
                continue;
            };

            let stats = ShipBattleStats {
                ship_id,
                battles: 1,
                damage: actual_damage,
                wins: if is_win { 1 } else { 0 },
                frags: row.kills.unwrap_or(0),
            };

            row.personal_rating = pr_data.calculate_pr(&[stats]);
        }
    }
}

const NUM_SKILLS_IN_TIER: usize = 6;

struct SkillLabel {
    text: String,
    hover: Option<String>,
    warning: bool,
}

/// Ported from `util::colorize_captain_points`: text/hover/warning only, no
/// icon glyphs or color (those are Milestone 2 concerns, derived from
/// `warning`/`skill_points` by `columns::cell_value`).
fn build_skill_label(
    skill_points: usize,
    num_skills: usize,
    highest_tier: usize,
    num_tier_1_skills: usize,
    has_dazzle: bool,
    has_ifa: bool,
) -> SkillLabel {
    let mut extra_hover_text = Vec::new();
    if has_dazzle {
        extra_hover_text.push("Dazzle");
    }
    if has_ifa {
        extra_hover_text.push("IFA");
    }
    let text = format!("{skill_points}pts ({num_skills} skills)");

    if num_tier_1_skills == NUM_SKILLS_IN_TIER {
        let default_text = "Player is playing tower defense with their skills";
        let hover = if extra_hover_text.is_empty() {
            default_text.to_string()
        } else {
            format!("{default_text} and has {}", extra_hover_text.join(", "))
        };
        return SkillLabel { text, hover: Some(hover), warning: true };
    }

    if highest_tier <= 2 && skill_points >= 6 {
        let default_text = "Player has no skills above tier 2";
        let hover = if extra_hover_text.is_empty() {
            default_text.to_string()
        } else {
            format!("{default_text} and has {}", extra_hover_text.join(", "))
        };
        return SkillLabel { text, hover: Some(hover), warning: true };
    }

    let hover =
        if extra_hover_text.is_empty() { None } else { Some(format!("Player has {}", extra_hover_text.join(", "))) };
    SkillLabel { text, hover, warning: false }
}

/// Whether the learned captain skills include Dazzle / Incoming Fire Alert,
/// read from the skill grid (`TranslatedBuild::new` already flags `learned`
/// per skill), matching the raw-`commander_skills` scan in
/// `util::colorize_captain_points`.
fn captain_extras(build: Option<&TranslatedBuild>) -> (bool, bool) {
    let Some(rows) = build.and_then(|b| b.captain_skills.as_ref()) else {
        return (false, false);
    };

    let mut has_dazzle = false;
    let mut has_ifa = false;
    for row in rows {
        for skill in &row.skills {
            if !skill.learned {
                continue;
            }
            match KnownCrewSkill::recognize(&skill.internal_name, skill.skill_type).known() {
                Some(KnownCrewSkill::Dazzle) => has_dazzle = true,
                Some(KnownCrewSkill::IncomingFireAlert) => has_ifa = true,
                _ => {}
            }
        }
    }
    (has_dazzle, has_ifa)
}

/// Comma-groups an integer for display (English/default grouping only; the
/// egui app's French space-grouping locale branch is not carried forward
/// here). `1234567 -> "1,234,567"`.
pub fn separate_number<T: Into<i128>>(n: T) -> String {
    let n: i128 = n.into();
    let neg = n < 0;
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if neg {
        out.insert(0, '-');
    }
    out
}

/// Rebuilds a monospace-style breakdown block ("Label   : 1,234") from a
/// per-type value lookup, in `descriptions` order, skipping zero entries.
/// Ported from `ui/replay_parser/mod.rs::breakdown_hover_string` (values
/// only; the monospace `RichText` wrapper is a render-layer concern).
fn breakdown_hover_string<F: Fn(&str) -> u64>(descriptions: &[(&str, &str)], get: F) -> String {
    let longest_width =
        descriptions.iter().filter(|(key, _)| get(key) > 0).map(|(_, desc)| desc.len()).max().unwrap_or_default() + 1;
    descriptions
        .iter()
        .filter_map(|(key, description)| {
            let num = get(key);
            if num > 0 { Some(format!("{description:<longest_width$}: {}", separate_number(num))) } else { None }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl PlayerRow {
    fn from_normalized_player(
        np: &NormalizedPlayer,
        self_division_id: Option<u32>,
        self_db_id: Option<AccountId>,
        ship_species_text: String,
        ship_id: Option<GameParamId>,
    ) -> Self {
        let is_self_division_mate = match (self_division_id, self_db_id) {
            (Some(self_div), Some(self_id)) => self_id != np.db_id && np.division_id == Some(self_div),
            _ => false,
        };

        let clan_tag = (!np.clan.is_empty()).then(|| format!("[{}]", np.clan));

        let server = np.server_results.as_ref();

        let (base_xp, base_xp_text) = match server.and_then(|sr| sr.xp) {
            Some(xp) => (Some(xp), Some(separate_number(xp))),
            None => (None, None),
        };
        let (raw_xp, raw_xp_text) = match server.and_then(|sr| sr.raw_xp) {
            Some(raw_xp) => (Some(raw_xp), Some(separate_number(raw_xp))),
            None => (None, None),
        };

        let observed_damage = np.observed_results.damage;
        let observed_damage_text = separate_number(observed_damage);

        let (actual_damage, actual_damage_report, actual_damage_text, actual_damage_hover_text) = match server {
            Some(sr) if sr.damage.is_some() => {
                let damage_number = sr.damage.expect("damage present");
                let text = separate_number(damage_number);
                let hover = breakdown_hover_string(&DAMAGE_DESCRIPTIONS, |key| {
                    sr.damage_by_type.get(key).copied().unwrap_or(0)
                });
                (Some(damage_number), Some(sr.damage_details.clone()), Some(text), Some(hover))
            }
            _ => (None, None, None, None),
        };

        let (hits, hits_report, hits_text, hits_hover_text) = match server {
            Some(sr) => {
                let hits_number = sr.hits.unwrap_or(0);
                let text = separate_number(hits_number);
                let hover =
                    breakdown_hover_string(&HITS_DESCRIPTIONS, |key| sr.hits_by_type.get(key).copied().unwrap_or(0));
                (sr.hits, Some(sr.hits_details.clone()), Some(text), Some(hover))
            }
            None => (None, None, None, None),
        };

        let (received_damage, received_damage_text, received_damage_hover_text, received_damage_report) = match server {
            Some(sr) => {
                let total = sr.received_damage;
                let text = separate_number(total);
                let hover = breakdown_hover_string(&RECEIVED_DAMAGE_DESCRIPTIONS, |key| {
                    sr.received_damage_by_type.get(key).copied().unwrap_or(0)
                });
                (Some(total), Some(text), Some(hover), Some(sr.received_damage_details.clone()))
            }
            None => (None, None, None, None),
        };

        // Spotting: prefer server scouting_damage, else the self-player
        // controller fallback. The hover breakdown is always None; see the
        // field doc comment.
        let (spotting_damage, spotting_damage_text) =
            if let Some(damage_number) = server.and_then(|sr| sr.spotting_damage) {
                (Some(damage_number), Some(separate_number(damage_number)))
            } else if np.is_self {
                match np.controller_spotting_damage {
                    Some(total) => (Some(total), Some(separate_number(total))),
                    None => (None, None),
                }
            } else {
                (None, None)
            };

        let (potential_damage, potential_damage_text, potential_damage_hover_text, potential_damage_report) =
            match server {
                Some(sr) => {
                    let total = sr.potential_damage;
                    let art = sr.potential_damage_details.artillery;
                    let tpd = sr.potential_damage_details.torpedoes;
                    let air = sr.potential_damage_details.planes;
                    // Depth-charge agro is the only potential key the 3-field
                    // report drops; recover it from the total (total == art +
                    // tpd + air + dbomb by construction).
                    let dbomb = total.saturating_sub(art + tpd + air);
                    let hover = breakdown_hover_string(&POTENTIAL_DAMAGE_DESCRIPTIONS, |key| match key {
                        "agro_art" => art,
                        "agro_tpd" => tpd,
                        "agro_air" => air,
                        "agro_dbomb" => dbomb,
                        _ => 0,
                    });
                    (Some(total), Some(separate_number(total)), Some(hover), Some(sr.potential_damage_details.clone()))
                }
                None => {
                    if np.is_self {
                        match np.controller_potential_damage {
                            Some(total) => (Some(total), Some(separate_number(total)), None, None),
                            None => (None, None, None, None),
                        }
                    } else {
                        (None, None, None, None)
                    }
                }
            };

        let time_lived_secs = np.time_lived_secs;
        let time_lived_text = time_lived_secs.map(|secs| format!("{}:{:02}", secs / 60, secs % 60));

        // `fires_dealt` is `Some` exactly when the resolved object carried an
        // interactions map, matching the old `interactions`-key gate.
        let (fires, floods, citadels, crits, damage_interactions) = match server {
            Some(sr) if sr.fires_dealt.is_some() => (
                sr.fires_dealt,
                sr.floods_dealt,
                sr.citadels_dealt,
                sr.crits_dealt,
                Some(sr.damage_interactions.clone()),
            ),
            _ => (None, None, None, None, None),
        };

        let distance_traveled = server.and_then(|sr| sr.distance_traveled);
        let kills = server.and_then(|sr| sr.kills);

        // TranslatedBuild::new returns None exactly when player.vehicle_entity()
        // is None (or the species is unrecognized), so its presence stands in
        // for "this replay observed the vehicle entity".
        let has_vehicle_entity = np.build.is_some();

        let (has_dazzle, has_ifa) = captain_extras(np.build.as_ref());
        let skill_label = build_skill_label(
            np.skill_info.skill_points,
            np.skill_info.num_skills,
            np.skill_info.highest_tier,
            np.skill_info.num_tier_1_skills,
            has_dazzle,
            has_ifa,
        );

        PlayerRow {
            db_id: np.db_id,
            team_id: TeamId::from(np.team_id),
            relation: np.relation,
            is_self: np.is_self,
            is_bot: np.is_bot,
            is_abuser: np.is_abuser,
            is_test_ship: np.is_test_ship,
            manual_stat_hide_toggle: false,
            display_name: np.display_name.clone(),
            clan_tag,
            clan_color_rgb: np.clan_color_rgb,
            division_label: np.division_label.clone(),
            is_self_division_mate,
            ship_name: np.ship_name.clone(),
            ship_species_text,
            ship_class: np.ship_class,
            ship_id,
            has_vehicle_entity,
            base_xp,
            base_xp_text,
            raw_xp,
            raw_xp_text,
            observed_damage,
            observed_damage_text,
            observed_kills: np.observed_results.kills,
            actual_damage,
            actual_damage_report,
            actual_damage_text,
            actual_damage_hover_text,
            hits,
            hits_report,
            hits_text,
            hits_hover_text,
            spotting_damage,
            spotting_damage_text,
            spotting_damage_hover_text: None,
            potential_damage,
            potential_damage_text,
            potential_damage_hover_text,
            potential_damage_report,
            received_damage,
            received_damage_text,
            received_damage_hover_text,
            received_damage_report,
            damage_interactions,
            fires,
            floods,
            citadels,
            crits,
            time_lived_secs,
            time_lived_text,
            distance_traveled,
            kills,
            heal_count: np.heal_count,
            skill_points: np.skill_info.skill_points,
            num_skills: np.skill_info.num_skills,
            highest_tier: np.skill_info.highest_tier,
            num_tier_1_skills: np.skill_info.num_tier_1_skills,
            skill_label_text: skill_label.text,
            skill_hover_text: skill_label.hover,
            skill_warning: skill_label.warning,
            has_dazzle,
            has_ifa,
            translated_build: np.build.clone(),
            achievements: np.achievements.clone(),
            ribbons: np.ribbons.clone(),
            consumables: np.consumables.clone(),
            // PR data isn't loaded at report-construction time; see the
            // field doc and `ReplayReportModel::populate_personal_ratings`.
            personal_rating: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay_inspector::test_support;

    #[test]
    fn separate_number_groups_by_thousands() {
        assert_eq!(separate_number(1_234_567i64), "1,234,567");
        assert_eq!(separate_number(0i64), "0");
        assert_eq!(separate_number(999i64), "999");
        assert_eq!(separate_number(1_000i64), "1,000");
        assert_eq!(separate_number(-1_234_567i64), "-1,234,567");
        assert_eq!(separate_number(50_000u64), "50,000");
    }

    #[test]
    fn from_normalized_builds_one_row_per_player_and_identifies_self() {
        let normalized = test_support::fixture_normalized_battle_report();

        let model = ReplayReportModel::build(&normalized, |species| species.name().to_string(), |_ship_index| None);

        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.self_team, TeamId::from(0i64));
        assert_eq!(model.columns.len(), ReplayColumn::ALL.len());

        let self_row = model.rows.iter().find(|r| r.is_self).expect("self row present");
        assert_eq!(self_row.db_id, AccountId(1));
        assert_eq!(self_row.actual_damage, Some(50_000));
        assert_eq!(self_row.actual_damage_text.as_deref(), Some("50,000"));
    }

    #[test]
    fn from_normalized_populates_breakdowns_only_when_server_results_exist() {
        let normalized = test_support::fixture_normalized_battle_report();
        let model = ReplayReportModel::build(&normalized, |species| species.name().to_string(), |_ship_index| None);

        let self_row = model.rows.iter().find(|r| r.is_self).expect("self row present");
        assert!(self_row.actual_damage_report.is_some());
        assert!(self_row.actual_damage_hover_text.is_some());
        assert!(self_row.received_damage_report.is_some());
        assert_eq!(self_row.fires, Some(1));

        let enemy_row = model.rows.iter().find(|r| !r.is_self).expect("enemy row present");
        assert!(enemy_row.actual_damage.is_none());
        assert!(enemy_row.actual_damage_report.is_none());
        assert!(enemy_row.actual_damage_hover_text.is_none());
        assert!(enemy_row.fires.is_none());
    }

    #[test]
    fn should_hide_stats_matches_manual_toggle_or_non_self_test_ship() {
        let mut row = test_support::base_row(1, Relation::new(1), false);
        assert!(!row.should_hide_stats());

        row.is_test_ship = true;
        assert!(row.should_hide_stats(), "non-self test ship should hide stats");

        row.is_test_ship = false;
        row.manual_stat_hide_toggle = true;
        assert!(row.should_hide_stats(), "manual toggle should hide stats");

        let self_test_ship = PlayerRow { is_test_ship: true, ..test_support::base_row(1, Relation::new(0), true) };
        assert!(!self_test_ship.should_hide_stats(), "self player's own test ship is never hidden");
    }

    #[test]
    fn populate_personal_ratings_computes_pr_from_row_ship_id_damage_and_win() {
        use wows_replay_insights::personal_rating::ExpectedValuesData;
        use wows_replay_insights::personal_rating::PersonalRatingCategory;
        use wows_replay_insights::personal_rating::PersonalRatingData;
        use wows_replay_insights::personal_rating::ShipExpectedValues;
        use wows_replay_insights::personal_rating::ShipExpectedValuesEntry;

        let normalized = test_support::fixture_normalized_battle_report();
        let self_ship_id = GameParamId::from(3_374_266_064u64);
        let mut model = ReplayReportModel::build(
            &normalized,
            |species| species.name().to_string(),
            |_ship_index| Some(self_ship_id),
        );
        model.battle_result = Some(BattleResult::Win(0));

        // Expected values exactly match the self row's fixture stats (50,000
        // damage, 1 kill, a win), landing PR at the 700+300+150 baseline.
        let mut expected = HashMap::new();
        expected.insert(
            self_ship_id.raw().to_string(),
            ShipExpectedValuesEntry::Values(ShipExpectedValues {
                average_damage_dealt: 50_000.0,
                average_frags: 1.0,
                win_rate: 100.0,
            }),
        );
        let mut pr_data = PersonalRatingData::new();
        pr_data.load(ExpectedValuesData { time: 0, data: expected });

        model.populate_personal_ratings(&pr_data);

        let self_row = model.rows.iter().find(|r| r.is_self).expect("self row present");
        let pr = self_row.personal_rating.as_ref().expect("PR should be computed for the self row");
        assert!((pr.pr - 1150.0).abs() < 1e-6, "expected PR 1150, got {}", pr.pr);
        assert_eq!(pr.category, PersonalRatingCategory::Average);

        // The enemy row has no `actual_damage` (no server results in the
        // fixture), so `populate_personal_ratings` leaves it untouched.
        let enemy_row = model.rows.iter().find(|r| !r.is_self).expect("enemy row present");
        assert!(enemy_row.personal_rating.is_none());
    }

    #[test]
    fn populate_personal_ratings_skips_rows_that_already_have_a_pr() {
        use wows_replay_insights::personal_rating::PersonalRatingData;

        let mut row = test_support::base_row(1, Relation::new(0), true);
        row.ship_id = Some(GameParamId::from(1u64));
        row.actual_damage = Some(10_000);
        row.personal_rating = Some(PersonalRatingResult::new(999.0));
        let mut model = ReplayReportModel {
            self_team: TeamId::from(0i64),
            rows: vec![row],
            battle_result: Some(BattleResult::Win(0)),
            columns: ReplayColumn::ALL.to_vec(),
            map: "Test Map".to_string(),
            chat: Vec::new(),
        };

        // Unloaded PR data would make `calculate_pr` return `None` for any
        // fresh computation, so a non-None result here proves the
        // already-computed row was left alone rather than recomputed.
        model.populate_personal_ratings(&PersonalRatingData::new());

        assert_eq!(model.rows[0].personal_rating.as_ref().map(|pr| pr.pr), Some(999.0));
    }

    /// Needs local game data to build a real `GameMetadataProvider` (species
    /// translation lookups require loaded game params + scripts, which
    /// cannot be cheaply fabricated). Run with, e.g.:
    ///
    /// ```text
    /// WOWS_REPLAY_INSPECTOR_TEST_GAME_DIR=G:\wows_builds\13.11.0 \
    /// WOWS_REPLAY_INSPECTOR_TEST_VERSION="13, 11, 0, 12668706" \
    /// cargo test -p wows-toolkit-gpui -- --ignored from_normalized_resolves_species_text_from_a_real_provider
    /// ```
    ///
    /// See `reference_test_replays_and_builds.md` for the build-directory
    /// convention. The rest of the mapping logic (row count, self
    /// identification, damage/hits/xp scalars, NDA-relevant breakdown
    /// gating) is covered by the fabricated-fixture tests above, which need
    /// no game data.
    #[test]
    #[ignore = "needs a local game build directory; see the doc comment for the run command"]
    fn from_normalized_resolves_species_text_from_a_real_provider() {
        use wowsunpack::game_params::provider::GameMetadataProvider;

        let game_dir = std::env::var("WOWS_REPLAY_INSPECTOR_TEST_GAME_DIR")
            .expect("set WOWS_REPLAY_INSPECTOR_TEST_GAME_DIR to a dumped game build directory");
        let version_str = std::env::var("WOWS_REPLAY_INSPECTOR_TEST_VERSION").expect(
            "set WOWS_REPLAY_INSPECTOR_TEST_VERSION to a clientVersionFromExe string, e.g. \"13, 11, 0, 12668706\"",
        );
        let version = wowsunpack::data::Version::from_client_exe(&version_str);

        let resources = wowsunpack::game_data::load_game_resources(std::path::Path::new(&game_dir), &version)
            .expect("failed to load game resources from WOWS_REPLAY_INSPECTOR_TEST_GAME_DIR");
        let provider =
            GameMetadataProvider::from_vfs(&resources.vfs).expect("failed to build GameMetadataProvider from the VFS");

        let normalized = test_support::fixture_normalized_battle_report();
        let meta = test_support::fixture_replay_meta();
        let constants = serde_json::Value::Null;

        let model = ReplayReportModel::from_normalized(&normalized, &meta, &provider, &constants, &[]);

        assert_eq!(model.rows.len(), normalized.players.len());
        let self_row = model.rows.iter().find(|r| r.is_self).expect("self row present");
        assert!(!self_row.ship_species_text.is_empty(), "provider-backed species text should resolve to a real string");
    }
}
