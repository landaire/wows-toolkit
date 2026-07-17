//! Leaf settings types shared between the egui app and the GPUI port.

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
    #[serde(default = "default_bool::<true>")]
    pub show_distance_traveled: bool,
    #[serde(default = "default_bool::<false>")]
    pub auto_export_data: bool,
    #[serde(default)]
    pub auto_export_path: String,
    #[serde(default)]
    pub auto_export_format: ReplayExportFormat,
    #[serde(default)]
    pub grouping: ReplayGrouping,
}

impl Default for ReplaySettings {
    fn default() -> Self {
        Self {
            show_entity_id: false,
            show_observed_damage: false,
            show_raw_xp: false,
            show_heals: true,
            show_received_damage: true,
            show_distance_traveled: true,
            auto_export_data: false,
            auto_export_path: String::new(),
            auto_export_format: ReplayExportFormat::default(),
            grouping: ReplayGrouping::default(),
        }
    }
}

pub const fn default_bool<const V: bool>() -> bool {
    V
}
