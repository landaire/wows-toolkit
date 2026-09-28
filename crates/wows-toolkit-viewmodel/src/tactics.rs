//! A tactics board as it is saved and read back.
//!
//! The board itself is drawn by each front end; what is on it -- the map, the
//! capture points and the annotations -- is one format on disk, so a board set
//! up in either app opens in the other.

use std::collections::BTreeSet;
use std::path::PathBuf;

use wt_collab_protocol::types::Annotation;
use wt_collab_protocol::types::AnnotationRangeFilter;
use wt_collab_protocol::types::AnnotationShipConfig;

/// A ship assigned to a placed marker, as the preset stores it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PresetShipConfig {
    pub param_id: u64,
    pub ship_name: String,
    #[serde(default)]
    pub hull_name: String,
    #[serde(default = "one")]
    pub vis_coeff: f32,
    #[serde(default = "one")]
    pub gm_coeff: f32,
    #[serde(default = "one")]
    pub gs_coeff: f32,
    #[serde(default)]
    pub range_filter: PresetRangeFilter,
}

/// Which range circles a placed ship shows.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PresetRangeFilter {
    #[serde(default)]
    pub detection: bool,
    #[serde(default)]
    pub main_battery: bool,
    #[serde(default)]
    pub secondary_battery: bool,
    #[serde(default)]
    pub torpedo: bool,
    #[serde(default)]
    pub radar: bool,
    #[serde(default)]
    pub hydro: bool,
}

/// A coefficient a preset written before the field existed is read with.
fn one() -> f32 {
    1.0
}

/// One drawn thing on the board.
///
/// Mirrors the wire annotation rather than reusing it: the wire type is rkyv
/// and this is what is written to disk, so the two are free to move apart.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum PresetAnnotation {
    Ship {
        pos: [f32; 2],
        yaw: f32,
        species: String,
        friendly: bool,
        #[serde(default)]
        config: Option<PresetShipConfig>,
    },
    FreehandStroke {
        points: Vec<[f32; 2]>,
        color: [u8; 4],
        width: f32,
    },
    Line {
        start: [f32; 2],
        end: [f32; 2],
        color: [u8; 4],
        width: f32,
    },
    Circle {
        center: [f32; 2],
        radius: f32,
        color: [u8; 4],
        width: f32,
        filled: bool,
    },
    Rectangle {
        center: [f32; 2],
        half_size: [f32; 2],
        rotation: f32,
        color: [u8; 4],
        width: f32,
        filled: bool,
    },
    Triangle {
        center: [f32; 2],
        radius: f32,
        rotation: f32,
        color: [u8; 4],
        width: f32,
        filled: bool,
    },
    Arrow {
        points: Vec<[f32; 2]>,
        color: [u8; 4],
        width: f32,
    },
    Measurement {
        start: [f32; 2],
        end: [f32; 2],
        color: [u8; 4],
        width: f32,
    },
}

impl PresetAnnotation {
    pub fn from_annotation(annotation: &Annotation) -> Self {
        match annotation {
            Annotation::Ship { pos, yaw, species, friendly, config } => Self::Ship {
                pos: *pos,
                yaw: *yaw,
                species: species.clone(),
                friendly: *friendly,
                config: config.as_ref().map(PresetShipConfig::from_config),
            },
            Annotation::FreehandStroke { points, color, width } => {
                Self::FreehandStroke { points: points.clone(), color: *color, width: *width }
            }
            Annotation::Line { start, end, color, width } => {
                Self::Line { start: *start, end: *end, color: *color, width: *width }
            }
            Annotation::Circle { center, radius, color, width, filled } => {
                Self::Circle { center: *center, radius: *radius, color: *color, width: *width, filled: *filled }
            }
            Annotation::Rectangle { center, half_size, rotation, color, width, filled } => Self::Rectangle {
                center: *center,
                half_size: *half_size,
                rotation: *rotation,
                color: *color,
                width: *width,
                filled: *filled,
            },
            Annotation::Triangle { center, radius, rotation, color, width, filled } => Self::Triangle {
                center: *center,
                radius: *radius,
                rotation: *rotation,
                color: *color,
                width: *width,
                filled: *filled,
            },
            Annotation::Arrow { points, color, width } => {
                Self::Arrow { points: points.clone(), color: *color, width: *width }
            }
            Annotation::Measurement { start, end, color, width } => {
                Self::Measurement { start: *start, end: *end, color: *color, width: *width }
            }
        }
    }

    pub fn to_annotation(&self) -> Annotation {
        match self {
            Self::Ship { pos, yaw, species, friendly, config } => Annotation::Ship {
                pos: *pos,
                yaw: *yaw,
                species: species.clone(),
                friendly: *friendly,
                config: config.as_ref().map(PresetShipConfig::to_config),
            },
            Self::FreehandStroke { points, color, width } => {
                Annotation::FreehandStroke { points: points.clone(), color: *color, width: *width }
            }
            Self::Line { start, end, color, width } => {
                Annotation::Line { start: *start, end: *end, color: *color, width: *width }
            }
            Self::Circle { center, radius, color, width, filled } => {
                Annotation::Circle { center: *center, radius: *radius, color: *color, width: *width, filled: *filled }
            }
            Self::Rectangle { center, half_size, rotation, color, width, filled } => Annotation::Rectangle {
                center: *center,
                half_size: *half_size,
                rotation: *rotation,
                color: *color,
                width: *width,
                filled: *filled,
            },
            Self::Triangle { center, radius, rotation, color, width, filled } => Annotation::Triangle {
                center: *center,
                radius: *radius,
                rotation: *rotation,
                color: *color,
                width: *width,
                filled: *filled,
            },
            Self::Arrow { points, color, width } => {
                Annotation::Arrow { points: points.clone(), color: *color, width: *width }
            }
            Self::Measurement { start, end, color, width } => {
                Annotation::Measurement { start: *start, end: *end, color: *color, width: *width }
            }
        }
    }
}

impl PresetShipConfig {
    fn from_config(config: &AnnotationShipConfig) -> Self {
        Self {
            param_id: config.param_id,
            ship_name: config.ship_name.clone(),
            hull_name: config.hull_name.clone(),
            vis_coeff: config.vis_coeff,
            gm_coeff: config.gm_coeff,
            gs_coeff: config.gs_coeff,
            range_filter: PresetRangeFilter {
                detection: config.range_filter.detection,
                main_battery: config.range_filter.main_battery,
                secondary_battery: config.range_filter.secondary_battery,
                torpedo: config.range_filter.torpedo,
                radar: config.range_filter.radar,
                hydro: config.range_filter.hydro,
            },
        }
    }

    fn to_config(&self) -> AnnotationShipConfig {
        AnnotationShipConfig {
            param_id: self.param_id,
            ship_name: self.ship_name.clone(),
            hull_name: self.hull_name.clone(),
            vis_coeff: self.vis_coeff,
            gm_coeff: self.gm_coeff,
            gs_coeff: self.gs_coeff,
            range_filter: AnnotationRangeFilter {
                detection: self.range_filter.detection,
                main_battery: self.range_filter.main_battery,
                secondary_battery: self.range_filter.secondary_battery,
                torpedo: self.range_filter.torpedo,
                radar: self.range_filter.radar,
                hydro: self.range_filter.hydro,
            },
        }
    }
}

/// One capture point, as the preset stores it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PresetCapPoint {
    pub index: usize,
    pub world_x: f32,
    pub world_z: f32,
    pub radius: f32,
    pub team_id: i64,
    #[serde(default)]
    pub frozen: bool,
}

/// A saved board.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct TacticsPreset {
    /// What the reader called it, which is also its file name.
    pub name: String,
    /// The map's space name, such as `spaces/16_OC_bees_to_honey`.
    pub map_name: String,
    pub map_id: u32,
    pub cap_points: Vec<PresetCapPoint>,
    pub annotations: Vec<PresetAnnotation>,
}

/// Where presets are kept, made if it is not there yet.
pub fn presets_dir() -> Option<PathBuf> {
    let dir = wows_toolkit_config::storage_dir()?.join("tactics_presets");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Every saved preset's name, in order.
///
/// A directory that cannot be read is an empty list rather than an error: the
/// board still works, it simply has nothing saved to offer.
pub fn list_preset_names() -> Vec<String> {
    let Some(dir) = presets_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let names: BTreeSet<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".json").map(str::to_owned)
        })
        .collect();
    names.into_iter().collect()
}

/// Why a preset could not be saved or read.
#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("there is no storage directory to keep presets in")]
    NoStorage,
    #[error("the preset file could not be read or written: {source}")]
    File {
        #[source]
        source: std::io::Error,
    },
    #[error("the preset did not parse: {source}")]
    Parse {
        #[source]
        source: serde_json::Error,
    },
}

pub fn save_preset(preset: &TacticsPreset) -> Result<(), PresetError> {
    let dir = presets_dir().ok_or(PresetError::NoStorage)?;
    let json = serde_json::to_string_pretty(preset).map_err(|source| PresetError::Parse { source })?;
    std::fs::write(dir.join(format!("{}.json", preset.name)), json).map_err(|source| PresetError::File { source })
}

pub fn load_preset(name: &str) -> Result<TacticsPreset, PresetError> {
    let dir = presets_dir().ok_or(PresetError::NoStorage)?;
    let json =
        std::fs::read_to_string(dir.join(format!("{name}.json"))).map_err(|source| PresetError::File { source })?;
    serde_json::from_str(&json).map_err(|source| PresetError::Parse { source })
}

pub fn delete_preset(name: &str) -> Result<(), PresetError> {
    let dir = presets_dir().ok_or(PresetError::NoStorage)?;
    std::fs::remove_file(dir.join(format!("{name}.json"))).map_err(|source| PresetError::File { source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_annotation_round_trips_through_the_preset_form() {
        let placed = Annotation::Ship {
            pos: [1.5, -2.5],
            yaw: 0.75,
            species: "Cruiser".to_owned(),
            friendly: true,
            config: Some(AnnotationShipConfig {
                param_id: 4242,
                ship_name: "Moskva".to_owned(),
                hull_name: "PRUH510_Moskva_1".to_owned(),
                vis_coeff: 0.9,
                gm_coeff: 1.1,
                gs_coeff: 1.0,
                range_filter: AnnotationRangeFilter { detection: true, radar: true, ..Default::default() },
            }),
        };

        let back = PresetAnnotation::from_annotation(&placed).to_annotation();

        let Annotation::Ship { pos, yaw, species, friendly, config } = back else {
            panic!("a ship reads back as a ship");
        };
        assert_eq!(pos, [1.5, -2.5]);
        assert_eq!(yaw, 0.75);
        assert_eq!(species, "Cruiser");
        assert!(friendly);
        let config = config.expect("the ship kept its configuration");
        assert_eq!(config.param_id, 4242);
        assert_eq!(config.vis_coeff, 0.9);
        assert!(config.range_filter.detection && config.range_filter.radar);
        assert!(!config.range_filter.torpedo);
    }

    /// A preset written before the coefficients existed reads as stock rather
    /// than as zero, which would put every range circle at nothing.
    #[test]
    fn a_preset_without_coefficients_reads_as_stock() {
        let json = r#"{"param_id":1,"ship_name":"Moskva"}"#;
        let config: PresetShipConfig = serde_json::from_str(json).expect("it parses");
        assert_eq!(config.vis_coeff, 1.0);
        assert_eq!(config.gm_coeff, 1.0);
        assert_eq!(config.gs_coeff, 1.0);
        assert_eq!(config.hull_name, "");
    }
}
