//! Manages textures loaded from the host's AssetBundle.

use std::collections::HashMap;

use egui::TextureHandle;
use wt_collab_protocol::protocol::GameFontsWire;
use wt_collab_protocol::protocol::RgbaAssetWire;

/// Holds all textures loaded from the host's AssetBundle.
#[derive(Default)]
pub struct AssetStore {
    pub ship_icons: HashMap<String, TextureHandle>,
    pub plane_icons: HashMap<String, TextureHandle>,
    pub consumable_icons: HashMap<String, TextureHandle>,
    pub death_cause_icons: HashMap<String, TextureHandle>,
    pub powerup_icons: HashMap<String, TextureHandle>,
    pub game_fonts: Option<GameFontsData>,
}

/// Parsed game font data (raw TTF bytes).
pub struct GameFontsData {
    pub primary_bytes: Vec<u8>,
    pub fallback_bytes: Vec<Vec<u8>>,
}

impl AssetStore {
    /// Returns true if no assets have been loaded (e.g. AssetBundle not yet received).
    pub fn is_empty(&self) -> bool {
        self.ship_icons.is_empty()
    }

    /// Load all assets from a received AssetBundle into egui textures.
    pub fn load_from_bundle(
        ctx: &egui::Context,
        ship_icons: Vec<(String, RgbaAssetWire)>,
        plane_icons: Vec<(String, RgbaAssetWire)>,
        consumable_icons: Vec<(String, RgbaAssetWire)>,
        death_cause_icons: Vec<(String, RgbaAssetWire)>,
        powerup_icons: Vec<(String, RgbaAssetWire)>,
        game_fonts: Option<GameFontsWire>,
    ) -> Self {
        let upload = |items: Vec<(String, RgbaAssetWire)>, prefix: &str| -> HashMap<String, TextureHandle> {
            items
                .into_iter()
                .map(|(key, asset)| {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [asset.width as usize, asset.height as usize],
                        &asset.data,
                    );
                    let handle = ctx.load_texture(format!("{prefix}_{key}"), image, egui::TextureOptions::LINEAR);
                    (key, handle)
                })
                .collect()
        };

        let fonts_data = game_fonts.map(|gf| {
            let mut fallback_bytes = Vec::new();
            if let Some(ko) = gf.fallback_ko {
                fallback_bytes.push(ko);
            }
            if let Some(ja) = gf.fallback_ja {
                fallback_bytes.push(ja);
            }
            if let Some(zh) = gf.fallback_zh {
                fallback_bytes.push(zh);
            }
            GameFontsData { primary_bytes: gf.primary, fallback_bytes }
        });

        Self {
            ship_icons: upload(ship_icons, "ship"),
            plane_icons: upload(plane_icons, "plane"),
            consumable_icons: upload(consumable_icons, "consumable"),
            death_cause_icons: upload(death_cause_icons, "death"),
            powerup_icons: upload(powerup_icons, "powerup"),
            game_fonts: fonts_data,
        }
    }

    /// Register game fonts into egui's font system.
    pub fn register_fonts(&self, ctx: &egui::Context) {
        let mut font_defs = ctx.fonts(|r| r.definitions().clone());

        if let Some(ref fonts) = self.game_fonts {
            font_defs
                .font_data
                .insert("game_font_primary".to_owned(), egui::FontData::from_owned(fonts.primary_bytes.clone()).into());
            let mut family_fonts = vec!["game_font_primary".to_owned()];
            let fallback_names = ["game_font_ko", "game_font_jp", "game_font_cn"];
            for (i, bytes) in fonts.fallback_bytes.iter().enumerate() {
                let name = fallback_names.get(i).unwrap_or(&"game_font_fallback").to_string();
                font_defs.font_data.insert(name.clone(), egui::FontData::from_owned(bytes.clone()).into());
                family_fonts.push(name);
            }
            font_defs.families.insert(egui::FontFamily::Name("GameFont".into()), family_fonts);
        } else if !font_defs.families.contains_key(&egui::FontFamily::Name("GameFont".into())) {
            let proportional = font_defs.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
            font_defs.families.insert(egui::FontFamily::Name("GameFont".into()), proportional);
        }

        egui_phosphor::add_to_fonts(&mut font_defs, egui_phosphor::Variant::Regular);
        ctx.set_fonts(font_defs);
    }
}
