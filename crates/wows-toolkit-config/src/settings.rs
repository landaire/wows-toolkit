//! Leaf settings types.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde::Serialize;

/// Replay grouping strategy in the file browser
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayGrouping {
    #[default]
    Date,
    Ship,
    None,
}

impl ReplayGrouping {
    pub fn label(&self) -> &'static str {
        match self {
            ReplayGrouping::Date => "Date",
            ReplayGrouping::Ship => "Ship",
            ReplayGrouping::None => "None",
        }
    }
}

/// Export format for replay data.
#[derive(Copy, Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum ReplayExportFormat {
    #[default]
    Json,
    Cbor,
    Csv,
}

impl ReplayExportFormat {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

    pub fn extension(&self) -> &str {
        match self {
            ReplayExportFormat::Json => "json",
            ReplayExportFormat::Cbor => "cbor",
            ReplayExportFormat::Csv => "csv",
        }
    }
}

impl AsRef<str> for ReplayExportFormat {
    fn as_ref(&self) -> &str {
        match self {
            ReplayExportFormat::Json => "JSON",
            ReplayExportFormat::Cbor => "CBOR",
            ReplayExportFormat::Csv => "CSV",
        }
    }
}

/// Settings specific to replay parsing and display
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplaySettings {
    pub show_entity_id: bool,
    pub show_observed_damage: bool,
    #[serde(default)]
    pub show_raw_xp: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_heals: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_received_damage: bool,
    /// Whether the player table draws the damage-type bar under damage dealt.
    #[serde(default = "default_bool::<true>")]
    pub show_damage_dealt_bar: bool,
    /// Whether the player table draws the damage-type bar under damage received.
    #[serde(default = "default_bool::<true>")]
    pub show_damage_received_bar: bool,
    /// Whether the player table draws a nation's flag beside each ship.
    #[serde(default = "default_bool::<true>")]
    pub show_nation_flags: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_distance_traveled: bool,
    /// Storage keys of hidden inspector columns that have no individual visibility field.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden_columns: BTreeSet<String>,
    #[serde(default = "default_bool::<false>")]
    pub auto_export_data: bool,
    #[serde(default)]
    pub auto_export_path: String,
    #[serde(default)]
    pub auto_export_format: ReplayExportFormat,
    #[serde(default)]
    pub grouping: ReplayGrouping,
    /// Whether the replay listing panel is collapsed to its rail.
    #[serde(default)]
    pub listing_collapsed: bool,
    /// Hovering a replay row parses that replay in the background to build a
    /// looping preview. Off is for machines where that read is expensive.
    #[serde(default = "default_bool::<true>")]
    pub enable_replay_previews: bool,
    /// Display generated pseudonyms in place of player names and clan tags.
    /// Exports keep the real values.
    #[serde(default)]
    pub anonymize_player_names: bool,
    /// Whether a listing group header's win rate counts every game mode rather than only Random
    /// and Ranked battles.
    #[serde(default)]
    pub group_win_rate_all_modes: bool,
    /// Which optional elements a replay listing row draws.
    #[serde(default)]
    pub listing_row_elements: ListingRowElements,
}

impl Default for ReplaySettings {
    fn default() -> Self {
        Self {
            show_entity_id: false,
            show_observed_damage: false,
            show_raw_xp: false,
            show_heals: true,
            show_received_damage: true,
            show_damage_dealt_bar: true,
            show_damage_received_bar: true,
            show_nation_flags: true,
            show_distance_traveled: true,
            hidden_columns: BTreeSet::new(),
            auto_export_data: false,
            auto_export_path: String::new(),
            auto_export_format: ReplayExportFormat::default(),
            grouping: ReplayGrouping::default(),
            listing_collapsed: false,
            enable_replay_previews: true,
            anonymize_player_names: false,
            group_win_rate_all_modes: false,
            listing_row_elements: ListingRowElements::default(),
        }
    }
}

/// An optional element of a replay listing row. The label and the map thumbnail are always drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListingRowElement {
    PersonalRating,
    MatchContribution,
    Division,
    Twitch,
    Damage,
    Kills,
    Time,
}

/// Whether a replay listing row draws each of its optional elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ListingRowElements {
    pub personal_rating: bool,
    pub match_contribution: bool,
    pub division: bool,
    /// The mark on a battle whose players other than the self player were seen in Twitch chat.
    pub twitch: bool,

    pub damage: bool,
    pub kills: bool,
    pub time: bool,
}

impl ListingRowElements {
    pub const fn new() -> Self {
        Self {
            personal_rating: true,
            match_contribution: true,
            division: true,
            twitch: true,
            damage: true,
            kills: true,
            time: true,
        }
    }

    pub const fn shows(&self, element: ListingRowElement) -> bool {
        match element {
            ListingRowElement::PersonalRating => self.personal_rating,
            ListingRowElement::MatchContribution => self.match_contribution,
            ListingRowElement::Division => self.division,
            ListingRowElement::Twitch => self.twitch,
            ListingRowElement::Damage => self.damage,
            ListingRowElement::Kills => self.kills,
            ListingRowElement::Time => self.time,
        }
    }

    pub fn set(&mut self, element: ListingRowElement, shown: bool) {
        let field = match element {
            ListingRowElement::PersonalRating => &mut self.personal_rating,
            ListingRowElement::MatchContribution => &mut self.match_contribution,
            ListingRowElement::Division => &mut self.division,
            ListingRowElement::Twitch => &mut self.twitch,
            ListingRowElement::Damage => &mut self.damage,
            ListingRowElement::Kills => &mut self.kills,
            ListingRowElement::Time => &mut self.time,
        };
        *field = shown;
    }

    /// Whether any element of the stats line under the label is drawn.
    pub const fn shows_stats_line(&self) -> bool {
        self.damage || self.kills || self.time
    }
}

impl Default for ListingRowElements {
    fn default() -> Self {
        Self::new()
    }
}

pub const fn default_bool<const V: bool>() -> bool {
    V
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_collapsed_defaults_to_false_and_survives_missing_field() {
        // Settings files written before this field existed must still load.
        let legacy = r#"{"show_entity_id":false,"show_observed_damage":false}"#;
        let parsed: ReplaySettings = serde_json::from_str(legacy).expect("legacy settings should parse");
        assert!(!parsed.listing_collapsed);
        assert!(!ReplaySettings::default().listing_collapsed);
    }

    #[test]
    fn enable_replay_previews_defaults_on_and_survives_a_missing_field() {
        let legacy = r#"{"show_entity_id":false,"show_observed_damage":false}"#;
        let parsed: ReplaySettings = serde_json::from_str(legacy).expect("legacy settings should parse");
        assert!(parsed.enable_replay_previews);
        assert!(ReplaySettings::default().enable_replay_previews);
    }

    #[test]
    fn anonymize_player_names_defaults_off_and_survives_a_missing_field() {
        let legacy = r#"{"show_entity_id":false,"show_observed_damage":false}"#;
        let parsed: ReplaySettings = serde_json::from_str(legacy).expect("legacy settings should parse");
        assert!(!parsed.anonymize_player_names);
        assert!(!ReplaySettings::default().anonymize_player_names);
    }

    #[test]
    fn player_table_elements_default_on_and_survive_missing_fields() {
        let legacy = r#"{"show_entity_id":false,"show_observed_damage":false}"#;
        let parsed: ReplaySettings = serde_json::from_str(legacy).expect("legacy settings should parse");

        assert!(parsed.show_damage_dealt_bar);
        assert!(parsed.show_damage_received_bar);
        assert!(parsed.show_nation_flags);
        assert!(ReplaySettings::default().show_damage_dealt_bar);
        assert!(ReplaySettings::default().show_damage_received_bar);
        assert!(ReplaySettings::default().show_nation_flags);
    }

    #[test]
    fn group_win_rate_all_modes_defaults_off_and_survives_a_missing_field() {
        let legacy = r#"{"show_entity_id":false,"show_observed_damage":false}"#;
        let parsed: ReplaySettings = serde_json::from_str(legacy).expect("legacy settings should parse");
        assert!(!parsed.group_win_rate_all_modes);
        assert!(!ReplaySettings::default().group_win_rate_all_modes);
    }

    #[test]
    fn listing_row_elements_default_shown_and_survive_missing_fields() {
        let legacy = r#"{"show_entity_id":false,"show_observed_damage":false}"#;
        let parsed: ReplaySettings = serde_json::from_str(legacy).expect("legacy settings should parse");
        assert_eq!(parsed.listing_row_elements, ListingRowElements::new());

        // A field added to the elements later loads shown beside the choices already saved.
        let partial = r#"{"show_entity_id":false,"show_observed_damage":false,"listing_row_elements":{"kills":false}}"#;
        let parsed: ReplaySettings = serde_json::from_str(partial).expect("partial settings should parse");
        assert_eq!(parsed.listing_row_elements, ListingRowElements { kills: false, ..ListingRowElements::new() });

        // Elements saved before the Twitch mark existed load with it shown.
        let saved = r#"{"show_entity_id":false,"show_observed_damage":false,"listing_row_elements":{"personal_rating":true,"match_contribution":false,"division":false,"damage":true,"kills":true,"time":false}}"#;
        let parsed: ReplaySettings = serde_json::from_str(saved).expect("saved settings should parse");
        assert_eq!(
            parsed.listing_row_elements,
            ListingRowElements { match_contribution: false, division: false, time: false, ..ListingRowElements::new() }
        );
    }
}
