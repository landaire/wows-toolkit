//! Icon cache: resolves the game's GUI icon assets (ship-class, captain-skill,
//! achievement, ribbon/subribbon, consumable, modernization, signal) from the
//! preloaded VFS, decodes them into `gpui::RenderImage`s, and caches the
//! decoded result so repeated renders of the same icon reuse one image.
//! Mirrors the egui app's per-kind icon maps on `WorldOfWarshipsData`
//! (`data/wows_data.rs`: `ship_icons`, `ribbon_icons`, `subribbon_icons`,
//! `achievement_icons`, `consumable_icons`, `crew_skill_icons`,
//! `modernization_icons`, `signal_flag_icons`) and its `icon_texture` decode
//! step (`ui/replay_parser/mod.rs`), but stores the already-decoded
//! `RenderImage` rather than raw asset bytes.
//!
//! Every icon kind except ship-class resolves to a PNG, decoded via
//! [`decode_icon`] (the `image` crate). Ship-class icons are the one
//! exception: the game ships them as SVG (`gui/fla/minimap/ship_icons`), which
//! the `image` crate cannot decode, so [`decode_ship_class_svg`] rasterizes
//! them through gpui's own `SvgRenderer` instead -- the same renderer
//! gpui-component uses for its bundled icon SVGs -- and rotates the result 90
//! degrees clockwise to match the egui app's `Image::rotate` on the same
//! asset.
//!
//! `table.rs`/`expanded.rs`/`browser_view.rs` treat a cache miss (an asset
//! absent from this build, or one `populate_from_rows` has not been asked to
//! resolve) the same way the egui app treats a missing icon texture: fall
//! back to a plain text label.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use gpui::RenderImage;
use gpui::SvgRenderer;
use image::Frame;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use super::columns::player_color_kind;
use super::columns::player_color_kind_rgb;
use super::model::PlayerRow;

/// Decodes raw image bytes (whatever format `image::load_from_memory` can
/// sniff; every icon kind except ship-class ships as PNG) into a
/// `RenderImage`. gpui's `RenderImage` buffer is BGRA even though the `image`
/// crate decodes to RGBA, so this swaps the red/blue channels per pixel,
/// matching gpui's own `ClipboardItem::to_image_data` conversion
/// (`gpui::platform`).
pub fn decode_icon(bytes: &[u8]) -> anyhow::Result<RenderImage> {
    let mut buffer = image::load_from_memory(bytes)?.into_rgba8();
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(RenderImage::new(vec![Frame::new(buffer)]))
}

/// Rasterizes a ship-class SVG (`gui/fla/minimap/ship_icons`) into a
/// `RenderImage` via gpui's `SvgRenderer`, rotates the raster 90 degrees
/// clockwise to match the egui app's `Image::rotate(90.0_f32.to_radians(),
/// Vec2::splat(0.5))` on the same asset (`ui/replay_parser/mod.rs`'s
/// `ReplayColumn::Name` arm), then tints every pixel's RGB channels by
/// `tint` (packed `0xRRGGBB`) to match that same call site's
/// `.tint(report.color)` -- the Name cell's ally/enemy/self/division-mate
/// icon coloring. `SvgRenderer::render_single_frame` already returns BGRA
/// (premultiplied) bytes -- the same format `decode_icon` produces -- so the
/// rotation only permutes pixel positions, never channel order, and the tint
/// step (like `decode_icon`'s channel swap) indexes `0`/`1`/`2` as blue/
/// green/red.
pub fn decode_ship_class_svg(svg_renderer: &SvgRenderer, bytes: &[u8], tint: u32) -> anyhow::Result<RenderImage> {
    let rendered =
        svg_renderer.render_single_frame(bytes, 1.0).map_err(|e| anyhow::anyhow!("failed to render SVG: {e:?}"))?;
    let size = rendered.size(0);
    let raw = rendered.as_bytes(0).ok_or_else(|| anyhow::anyhow!("rendered SVG has no frame data"))?;
    let buffer = image::RgbaImage::from_raw(size.width.0 as u32, size.height.0 as u32, raw.to_vec())
        .ok_or_else(|| anyhow::anyhow!("rendered SVG buffer size did not match its own dimensions"))?;
    let mut rotated = image::imageops::rotate90(&buffer);
    tint_bgra_pixels(&mut rotated, tint);
    Ok(RenderImage::new(vec![Frame::new(rotated)]))
}

/// Multiplies every pixel's blue/green/red channels (alpha untouched) by
/// `tint`'s (`0xRRGGBB`) own channel intensities, matching how a tint color
/// darkens/colors a white-ish source icon (egui's `Image::tint` does the same
/// component-wise multiply). `buffer` is BGRA, per `decode_ship_class_svg`'s
/// doc comment.
fn tint_bgra_pixels(buffer: &mut image::RgbaImage, tint: u32) {
    let tint_r = ((tint >> 16) & 0xff) as f32 / 255.0;
    let tint_g = ((tint >> 8) & 0xff) as f32 / 255.0;
    let tint_b = (tint & 0xff) as f32 / 255.0;
    for pixel in buffer.chunks_exact_mut(4) {
        pixel[0] = (pixel[0] as f32 * tint_b).round() as u8;
        pixel[1] = (pixel[1] as f32 * tint_g).round() as u8;
        pixel[2] = (pixel[2] as f32 * tint_r).round() as u8;
    }
}

/// Id-keyed cache of decoded icons: ship-class icons keyed by `(Species,
/// tint)` -- `tint` is the row's packed `0xRRGGBB` team/division color (see
/// `columns::player_color_kind_rgb`), since the same species renders in a
/// different color per row (egui's `Image::tint(report.color)`) -- and every
/// other icon kind (achievement, ribbon, subribbon, consumable,
/// modernization, signal, captain-skill, nation flag) keyed by a
/// caller-chosen string. Key convention: `"<kind>:<id>"`, e.g.
/// `"achievement:{icon_key}"`, `"ribbon:{icon_key}"`, `"subribbon:{icon_key}"`,
/// `"consumable:{icon_key}"`, `"modernization:{game_params_name}"`,
/// `"signal:{game_params_name}"`, `"skill:{internal_name}"`,
/// `"nation:{nation}"`. Cheap to clone: every stored image is `Arc`-shared.
#[derive(Default, Clone)]
pub struct IconCache {
    ship_class: HashMap<(Species, u32), Arc<RenderImage>>,
    keyed: HashMap<String, Arc<RenderImage>>,
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes `bytes` and caches the result under `(species, tint)`,
    /// replacing any previously cached icon for that pair. Returns `false`
    /// (leaving the cache untouched) if `bytes` cannot be decoded as an
    /// image, so a corrupt or missing asset degrades to `get` returning
    /// `None` instead of panicking.
    pub fn set(&mut self, species: Species, tint: u32, bytes: &[u8]) -> bool {
        match decode_icon(bytes) {
            Ok(image) => {
                self.ship_class.insert((species, tint), Arc::new(image));
                true
            }
            Err(_) => false,
        }
    }

    /// Caches an already-decoded icon under `(species, tint)`, bypassing
    /// `decode_icon` (used for ship-class icons, which `decode_ship_class_svg`
    /// decodes -- and tints -- via gpui's `SvgRenderer` rather than the
    /// `image` crate).
    pub fn set_image(&mut self, species: Species, tint: u32, image: RenderImage) {
        self.ship_class.insert((species, tint), Arc::new(image));
    }

    /// The cached icon for `(species, tint)`, if one has been decoded. `None`
    /// means "no icon available"; callers fall back to a text label.
    pub fn get(&self, species: Species, tint: u32) -> Option<Arc<RenderImage>> {
        self.ship_class.get(&(species, tint)).cloned()
    }

    /// Decodes `bytes` and caches the result under `key` (see the struct doc
    /// for the key convention), replacing any previously cached icon for that
    /// key. Returns `false` (leaving the cache untouched) on a decode
    /// failure, matching `set`.
    pub fn set_keyed(&mut self, key: impl Into<String>, bytes: &[u8]) -> bool {
        match decode_icon(bytes) {
            Ok(image) => {
                self.keyed.insert(key.into(), Arc::new(image));
                true
            }
            Err(_) => false,
        }
    }

    /// The cached icon for `key`, if `set_keyed` has decoded one. `None`
    /// means "no icon available"; callers fall back to a text label.
    pub fn get_keyed(&self, key: &str) -> Option<Arc<RenderImage>> {
        self.keyed.get(key).cloned()
    }

    /// Number of distinct ship-class icons currently cached.
    pub fn ship_class_count(&self) -> usize {
        self.ship_class.len()
    }

    /// Number of distinct keyed (non-ship-class) icons currently cached.
    pub fn keyed_count(&self) -> usize {
        self.keyed.len()
    }

    /// Resolves and decodes every icon `rows` references -- one ship-class
    /// icon per distinct `(Species, tint)` pair among them (the same species
    /// can render in several colors: self/ally/enemy/division-mate; see the
    /// struct doc), plus one keyed icon per distinct
    /// achievement/ribbon/subribbon/consumable/modernization/signal/
    /// captain-skill the rows carry -- from `vfs`, caching each one. A
    /// per-call dedupe set means a 24-player replay reads (and decodes) each
    /// shared asset -- e.g. every row earning `RIBBON_MAIN_CALIBER`, or every
    /// same-team row flying the same ship class -- only once, not once per
    /// row. An asset absent from this build (an older client missing some
    /// icon, or a key with no matching file) is silently skipped; `get`/
    /// `get_keyed` returning `None` for it is the render layer's existing
    /// signal to fall back to text.
    pub fn populate_from_rows(&mut self, rows: &[PlayerRow], vfs: &VfsPath, svg_renderer: &SvgRenderer) {
        let mut species_seen: HashSet<(Species, u32)> = HashSet::new();
        let mut keys_seen: HashSet<String> = HashSet::new();

        for row in rows {
            let tint = player_color_kind_rgb(player_color_kind(row));
            if species_seen.insert((row.ship_class, tint)) {
                self.load_ship_class(row.ship_class, tint, vfs, svg_renderer);
            }

            for achievement in &row.achievements {
                let key = format!("achievement:{}", achievement.icon_key);
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::Achievement(&achievement.icon_key));
            }
            for ribbon in &row.ribbons {
                if ribbon.is_subribbon {
                    let sub_icon_key = format!("sub{}", ribbon.icon_key);
                    let key = format!("subribbon:{}", ribbon.icon_key);
                    self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::SubRibbon(&sub_icon_key));
                } else {
                    let key = format!("ribbon:{}", ribbon.icon_key);
                    self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::Ribbon(&ribbon.icon_key));
                }
            }
            for consumable in &row.consumables {
                let key = format!("consumable:{}", consumable.icon_key);
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::Consumable(&consumable.icon_key));
            }

            let Some(build) = row.translated_build.as_ref() else { continue };
            for slot in build.modernization_slots.iter().flatten() {
                let key = format!("modernization:{}", slot.game_params_name);
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::Modernization(&slot.game_params_name));
            }
            for signal in &build.signals {
                let key = format!("signal:{}", signal.game_params_name);
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::SignalFlag(&signal.game_params_name));
            }
            for skill in build.captain_skills.iter().flatten().flat_map(|row| &row.skills) {
                let key = format!("skill:{}", skill.internal_name.as_str());
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::CrewSkill { name: &skill.internal_name });
            }
        }
    }

    fn load_ship_class(&mut self, species: Species, tint: u32, vfs: &VfsPath, svg_renderer: &SvgRenderer) {
        let Some(bytes) = GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive }.read(vfs, None) else {
            return;
        };
        if let Ok(image) = decode_ship_class_svg(svg_renderer, &bytes, tint) {
            self.set_image(species, tint, image);
        }
    }

    fn load_keyed(&mut self, keys_seen: &mut HashSet<String>, vfs: &VfsPath, key: String, asset: GuiAsset<'_>) {
        if !keys_seen.insert(key.clone()) {
            return;
        }
        if let Some(bytes) = asset.read(vfs, None) {
            self.set_keyed(key, &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// A 2x2 solid-red PNG, encoded in-memory so the fixture bytes live in
    /// source rather than a binary test asset.
    fn red_png_bytes() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut bytes = Vec::new();
        image.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png).expect("failed to encode test PNG");
        bytes
    }

    #[test]
    fn decode_icon_swaps_red_and_blue_channels() {
        let image = decode_icon(&red_png_bytes()).expect("valid PNG should decode");
        let pixel = image.as_bytes(0).expect("frame 0 should have pixel data");
        // Red (255, 0, 0, 255) in RGBA becomes (0, 0, 255, 255) in BGRA.
        assert_eq!(&pixel[0..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn decode_icon_rejects_non_image_bytes() {
        assert!(decode_icon(b"not an image").is_err());
    }

    const TINT: u32 = 0xffffff;

    /// Pins the Name-cell icon tint math: a white BGRA pixel (like the
    /// SvgRenderer's ship-class raster, which is drawn white so a tint can
    /// recolor it freely) multiplied by a pure-red `0xff0000` tint keeps its
    /// blue channel's own intensity applied to red -- i.e. a fully-saturated
    /// tint channel leaves the source channel unchanged, a zeroed tint channel
    /// zeroes it -- matching `Image::tint`'s component-wise multiply.
    #[test]
    fn tint_bgra_pixels_multiplies_each_channel_by_the_tints_own_intensity() {
        let mut buffer = image::RgbaImage::from_pixel(1, 1, image::Rgba([0xff, 0xff, 0xff, 0xff]));
        tint_bgra_pixels(&mut buffer, 0xff0000);
        // buffer is BGRA: index 0 = blue (tint's blue channel is 0x00), 1 =
        // green (tint's green channel is 0x00), 2 = red (tint's red channel
        // is 0xff), 3 = alpha (untouched).
        assert_eq!(buffer.as_raw().as_slice(), &[0x00, 0x00, 0xff, 0xff]);
    }

    #[test]
    fn icon_cache_get_is_none_until_set() {
        let cache = IconCache::new();
        assert!(cache.get(Species::Destroyer, TINT).is_none());
    }

    #[test]
    fn icon_cache_set_caches_by_species_and_get_returns_it() {
        let mut cache = IconCache::new();
        assert!(cache.set(Species::Destroyer, TINT, &red_png_bytes()));
        assert!(cache.get(Species::Destroyer, TINT).is_some());
        assert!(cache.get(Species::Cruiser, TINT).is_none(), "a different species should stay uncached");
    }

    #[test]
    fn icon_cache_set_returns_false_and_leaves_cache_untouched_for_invalid_bytes() {
        let mut cache = IconCache::new();
        assert!(!cache.set(Species::Destroyer, TINT, b"not an image"));
        assert!(cache.get(Species::Destroyer, TINT).is_none());
    }

    #[test]
    fn icon_cache_set_image_caches_an_already_decoded_image() {
        let mut cache = IconCache::new();
        let image = decode_icon(&red_png_bytes()).expect("valid PNG should decode");
        cache.set_image(Species::AirCarrier, TINT, image);
        assert!(cache.get(Species::AirCarrier, TINT).is_some());
        assert_eq!(cache.ship_class_count(), 1);
    }

    #[test]
    fn icon_cache_set_image_caches_the_same_species_separately_per_tint() {
        let mut cache = IconCache::new();
        let image_a = decode_icon(&red_png_bytes()).expect("valid PNG should decode");
        let image_b = decode_icon(&red_png_bytes()).expect("valid PNG should decode");
        cache.set_image(Species::Cruiser, 0xff0000, image_a);
        cache.set_image(Species::Cruiser, 0x00ff00, image_b);
        assert!(cache.get(Species::Cruiser, 0xff0000).is_some());
        assert!(cache.get(Species::Cruiser, 0x00ff00).is_some());
        assert!(cache.get(Species::Cruiser, 0x0000ff).is_none(), "an untouched tint should stay uncached");
        assert_eq!(cache.ship_class_count(), 2, "the same species in two tints is two cache entries");
    }

    #[test]
    fn icon_cache_get_keyed_is_none_until_set_keyed() {
        let cache = IconCache::new();
        assert!(cache.get_keyed("achievement:dfc").is_none());
    }

    #[test]
    fn icon_cache_set_keyed_caches_by_key_and_get_keyed_returns_it() {
        let mut cache = IconCache::new();
        assert!(cache.set_keyed("achievement:dfc", &red_png_bytes()));
        assert!(cache.get_keyed("achievement:dfc").is_some());
        assert!(cache.get_keyed("achievement:kraken").is_none(), "a different key should stay uncached");
        assert_eq!(cache.keyed_count(), 1);
    }

    #[test]
    fn icon_cache_set_keyed_returns_false_and_leaves_cache_untouched_for_invalid_bytes() {
        let mut cache = IconCache::new();
        assert!(!cache.set_keyed("achievement:dfc", b"not an image"));
        assert!(cache.get_keyed("achievement:dfc").is_none());
    }

    #[test]
    fn icon_cache_populate_from_rows_is_a_no_op_on_a_row_with_no_achievements_ribbons_or_build() {
        // No VFS/SvgRenderer needed: `populate_from_rows` only ever reaches
        // out to them for keys a row actually carries, and `base_row` (see
        // `test_support.rs`) carries none. This exercises the "nothing to
        // resolve" path without needing a real game install.
        use crate::replay_inspector::test_support::base_row;
        use wows_replays::types::Relation;

        let mut cache = IconCache::new();
        let rows = vec![base_row(1, Relation::new(0), true)];
        // `populate_from_rows` still needs a `VfsPath`/`SvgRenderer` value to
        // call, even though this fixture never reaches the code paths that
        // use them (no achievements/ribbons/consumables/build on `base_row`,
        // and the fixture VFS has no ship_icons directory for the row's own
        // species either). A `PhysicalFS` rooted at a fresh empty temp dir
        // gives a valid, asset-less `VfsPath` without touching a real game
        // install.
        let dir = std::env::temp_dir().join("wtk-gpui-icons-test-populate-noop");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let vfs: VfsPath = wowsunpack::vfs::PhysicalFS::new(&dir).into();
        let svg_renderer = SvgRenderer::new(std::sync::Arc::new(()));

        cache.populate_from_rows(&rows, &vfs, &svg_renderer);

        assert_eq!(cache.ship_class_count(), 0);
        assert_eq!(cache.keyed_count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pins `populate_from_rows`'s subribbon asset path: the game stores
    /// subribbon icons under a `"sub"`-prefixed filename (e.g.
    /// `subribbons/submain_caliber.png` for a `main_caliber` subribbon), not
    /// the bare `icon_key` (`subribbons/main_caliber.png`, which is what
    /// `GuiAsset::SubRibbon(&ribbon.icon_key)` alone would resolve to). This
    /// only creates the `"sub"`-prefixed file on disk, so a regression back to
    /// the un-prefixed lookup makes this test fail with a cache miss.
    #[test]
    fn populate_from_rows_resolves_subribbon_icons_under_the_sub_prefixed_filename() {
        use wows_replay_insights::battle_report::RibbonResult;

        use crate::replay_inspector::test_support::base_row;
        use wows_replays::types::Relation;

        let dir = std::env::temp_dir().join("wtk-gpui-icons-test-subribbon-prefix");
        let _ = std::fs::remove_dir_all(&dir);
        let subribbons_dir = dir.join("gui").join("ribbons").join("subribbons");
        std::fs::create_dir_all(&subribbons_dir).unwrap();
        std::fs::write(subribbons_dir.join("submain_caliber.png"), red_png_bytes()).unwrap();
        let vfs: VfsPath = wowsunpack::vfs::PhysicalFS::new(&dir).into();
        let svg_renderer = SvgRenderer::new(std::sync::Arc::new(()));

        let mut row = base_row(1, Relation::new(0), true);
        row.ribbons.push(RibbonResult {
            name: "main_caliber".to_string(),
            display_name: "Main Caliber Hit".to_string(),
            description: String::new(),
            icon_key: "main_caliber".to_string(),
            is_subribbon: true,
            count: 1,
        });
        let rows = vec![row];

        let mut cache = IconCache::new();
        cache.populate_from_rows(&rows, &vfs, &svg_renderer);

        assert!(
            cache.get_keyed("subribbon:main_caliber").is_some(),
            "expected the subribbon icon to resolve via its \"sub\"-prefixed filename"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
