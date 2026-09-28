//! What a ship-model export contains, and what carries over to the next one.
//!
//! The choices themselves, without the dialog that collects them, so both front
//! ends offer the same contents, hull, level of detail, texture cap and camo
//! set and remember the same answers.

use std::collections::BTreeSet;

use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::ship::CamoSelection;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::texture::MaxEdge;
use wowsunpack::export::texture::TextureLod;

/// The texture detail levels the dialog offers. A cap, not a tier count, because
/// the user thinks in pixels and the authored ladders differ per texture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TextureResolution {
    #[default]
    Full,
    Px2048,
    Px1024,
    Px512,
}

impl TextureResolution {
    pub const ALL: [Self; 4] = [Self::Full, Self::Px2048, Self::Px1024, Self::Px512];

    pub fn to_texture_lod(self) -> TextureLod {
        match self {
            // Full detail is the absence of a cap, not a very large one.
            Self::Full => TextureLod::Full,
            Self::Px2048 => TextureLod::Capped(MaxEdge::new(2048).expect("2048 is a valid edge")),
            Self::Px1024 => TextureLod::Capped(MaxEdge::new(1024).expect("1024 is a valid edge")),
            Self::Px512 => TextureLod::Capped(MaxEdge::new(512).expect("512 is a valid edge")),
        }
    }

    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Full => "ui.armor.export.res_full",
            Self::Px2048 => "ui.armor.export.res_2048",
            Self::Px1024 => "ui.armor.export.res_1024",
            Self::Px512 => "ui.armor.export.res_512",
        }
    }
}

pub const fn contents_label_key(contents: ExportContents) -> &'static str {
    match contents {
        ExportContents::Mesh => "ui.armor.export.contents_mesh",
        ExportContents::Armor => "ui.armor.export.contents_armor",
        ExportContents::MeshAndArmor => "ui.armor.export.contents_both",
    }
}

/// The user's in-progress choices.
pub struct ExportDraft {
    pub contents: ExportContents,
    pub hull: Option<String>,
    pub lod: usize,
    pub texture_res: TextureResolution,
    pub camos: BTreeSet<CamoSchemeId>,
}

/// The options a draft describes. One ticked camo bakes: it is the smallest
/// output and needs no variants extension, so it imports anywhere.
pub fn draft_to_options(draft: &ExportDraft) -> ShipExportOptions {
    let camos = if draft.contents.includes_mesh() {
        let mut ids: Vec<CamoSchemeId> = draft.camos.iter().copied().collect();
        match ids.len() {
            0 => CamoSelection::BaseOnly,
            1 => CamoSelection::Baked(ids.remove(0)),
            _ => CamoSelection::Variants(ids),
        }
    } else {
        CamoSelection::BaseOnly
    };

    ShipExportOptions {
        lod: draft.lod,
        hull: draft.hull.clone(),
        // Armor is untextured, so an armor-only export never reads a DDS.
        textures: draft.contents.includes_mesh(),
        damaged: false,
        contents: draft.contents,
        texture_lod: draft.texture_res.to_texture_lod(),
        camos,
        module_overrides: Default::default(),
    }
}

/// The scheme the dialog pre-ticks. Matched on the raw camo name as well as the
/// label, so a locale that translates "Default" does not hide it. A ship with no
/// such scheme starts with nothing ticked, which exports its stock appearance.
pub fn default_camo(schemes: &[CamoSchemeInfo]) -> Option<CamoSchemeId> {
    schemes
        .iter()
        .find(|s| s.raw_name.eq_ignore_ascii_case("default") || s.display_name.eq_ignore_ascii_case("default"))
        .map(|s| s.id)
}

/// The export choices that carry across ships. A `CamoSchemeId` indexes one
/// ship's ordered scheme list and a hull names one ship's upgrade, so neither is
/// meaningful for the next ship and neither belongs here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExportDefaults {
    pub contents: ExportContents,
    pub lod: usize,
    pub texture_res: TextureResolution,
}

impl Default for ExportDefaults {
    fn default() -> Self {
        // What the armor viewer's export button produced before the dialog existed.
        Self { contents: ExportContents::MeshAndArmor, lod: 0, texture_res: TextureResolution::Full }
    }
}

impl ExportDefaults {
    const SETTING_KEY: &'static str = "model_export_defaults";

    /// The subset of a confirmed draft worth remembering for the next ship:
    /// `hull` and `camos` are deliberately excluded, since both name data
    /// specific to the ship just exported.
    pub fn from_draft(draft: &ExportDraft) -> Self {
        Self { contents: draft.contents, lod: draft.lod, texture_res: draft.texture_res }
    }

    /// Missing state is expected before the first export.
    pub async fn load(pool: &sqlx::SqlitePool) -> Self {
        wows_toolkit_config::queries::get_setting(pool, Self::SETTING_KEY).await.unwrap_or_default()
    }

    pub async fn save(&self, pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
        wows_toolkit_config::queries::set_setting(pool, Self::SETTING_KEY, self).await
    }
}
