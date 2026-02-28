pub mod assets_bin;
#[cfg(feature = "models")]
pub mod forest;
#[cfg(feature = "models")]
pub mod geometry;
#[cfg(feature = "models")]
pub mod material;
#[cfg(feature = "models")]
pub mod merged_models;
#[cfg(feature = "models")]
pub mod model;
#[cfg(feature = "models")]
pub mod speedtree;
#[cfg(feature = "models")]
pub mod terrain;
#[cfg(feature = "models")]
pub mod vertex_format;
#[cfg(feature = "models")]
pub mod visual;

#[cfg(all(feature = "models", feature = "json"))]
use crate::data::assets_bin_vfs::PrototypeType;

/// Returns true if we have a JSON decoder for the given prototype type.
#[cfg(all(feature = "models", feature = "json"))]
pub fn can_decode_prototype(proto_type: PrototypeType) -> bool {
    matches!(proto_type, PrototypeType::Material | PrototypeType::Visual | PrototypeType::Model)
}

/// Decode a binary prototype record to pretty-printed JSON.
///
/// `record_data` should be the raw bytes from `AssetsBinVfs::open_file()` (record
/// start through end of blob, preserving relative pointer resolution).
#[cfg(all(feature = "models", feature = "json"))]
pub fn decode_prototype_to_json(record_data: &[u8], proto_type: PrototypeType) -> Result<String, String> {
    match proto_type {
        PrototypeType::Material => {
            let proto = material::parse_material(record_data).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&proto).map_err(|e| e.to_string())
        }
        PrototypeType::Visual => {
            let proto = visual::parse_visual(record_data).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&proto).map_err(|e| e.to_string())
        }
        PrototypeType::Model => {
            let proto = model::parse_model(record_data).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&proto).map_err(|e| e.to_string())
        }
        other => Err(format!("no decoder for {other:?}")),
    }
}
