//! Pricing an export without performing one.
//!
//! Everything here is derived once per loaded ship, so the caller can price any
//! option combination with arithmetic instead of a background re-estimate.

use std::collections::HashMap;

use crate::export::camo_textures::CamoSchemeId;
use crate::export::ship::CamoSelection;
use crate::export::ship::ShipExportOptions;
use crate::export::texture::TextureLod;

/// POSITION (3xf32) + NORMAL (3xf32) + TEXCOORD_0 (2xf32).
///
/// Every ship sub-model's vertex format carries a UV attribute: instrumenting
/// `unpack_vertices`'s call site and running `export-ship` on Smaland (LOD 0 and
/// LOD 2) and Yamato (LOD 0) never found a primitive with positions but no UVs,
/// across all hull parts, mount instances, and misc instances. `add_primitive_to_root`
/// writes the TEXCOORD_0 accessor conditionally, but the condition never fires for
/// real ship geometry, so the accessor is unconditional in practice.
const MESH_VERTEX_BYTES: u64 = 32;
/// POSITION (3xf32) + NORMAL (3xf32) + COLOR_0 (4xf32). Armor carries no UVs.
///
/// `ArmorSubModel::from_armor_model` and `armor_sub_models_by_zone` both push one
/// color per vertex unconditionally (same loop as positions/normals), so COLOR_0
/// is never empty when positions are non-empty.
const ARMOR_VERTEX_BYTES: u64 = 40;
const INDEX_BYTES: u64 = 4;

/// Encoded PNG size as a fraction of raw RGBA8.
///
/// Measured directly (decode each reachable base albedo DDS to PNG and sum bytes
/// against raw RGBA at the same dims) over WSD011_Smaland_1955's stock texture
/// set at full resolution: 55,279,616 raw against 11,079,681 encoded, a ratio of
/// 0.2004. WoWs ship prop/hull albedos are mostly flat painted metal with modest
/// panel-line detail, which PNG's lossless DEFLATE compresses far tighter than a
/// photographic-texture assumption would predict.
///
/// Calibrated against `size_estimate_accuracy.rs`'s six cases on
/// WSD011_Smaland_1955 (stock at full resolution, a 1024-capped export, an
/// armor-only export, a LOD-2 export, a baked camo, and two selected camo
/// variants), comparing `estimate().total()` to a real `export_glb` byte
/// count: stock 0.94%, capped 3.40%, armor-only 1.79%, deep LOD 1.48%, baked
/// 13.35%, two variants 10.17% off. One ratio holds every case inside the 25%
/// tolerance with room to spare; the baked case runs highest because
/// `estimate` prices it identically to stock while the real bake compresses
/// worse and loses cross-stem dedup (see `CamoSelection::Baked` in
/// `texture_bytes`), but that gap does not need a separate term to stay in
/// tolerance.
const PNG_RATIO: f64 = 0.2004;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshCounts {
    pub vertices: u64,
    pub indices: u64,
}

impl MeshCounts {
    fn bytes(self, per_vertex: u64) -> u64 {
        self.vertices * per_vertex + self.indices * INDEX_BYTES
    }
}

/// One texture's authored resolution ladder, largest tier first.
#[derive(Debug, Clone)]
pub struct TextureLadder {
    tiers: Vec<(u32, u32)>,
}

impl TextureLadder {
    pub fn new(tiers: Vec<(u32, u32)>) -> Self {
        Self { tiers }
    }

    /// The dimensions `lod` yields, mirroring `load_dds_from_vfs` and the mip the
    /// decode then picks. `None` when the texture authors no tier at all.
    pub fn dims_for(&self, lod: TextureLod) -> Option<(u32, u32)> {
        let largest = *self.tiers.first()?;
        match lod {
            TextureLod::Full => Some(largest),
            TextureLod::Tiers(drop) => {
                let idx = drop.tiers().min(self.tiers.len() - 1);
                Some(self.tiers[idx])
            }
            TextureLod::Capped(edge) => {
                if let Some(fits) = self.tiers.iter().find(|(w, h)| (*w).max(*h) <= edge.pixels()) {
                    return Some(*fits);
                }
                // Every authored tier is over budget, so the decode halves down the
                // smallest tier's own mip chain until it fits.
                let (mut w, mut h) = *self.tiers.last()?;
                while w.max(h) > edge.pixels() && w > 1 && h > 1 {
                    w /= 2;
                    h /= 2;
                }
                Some((w, h))
            }
        }
    }

    fn raw_bytes(&self, lod: TextureLod) -> u64 {
        self.dims_for(lod).map(|(w, h)| w as u64 * h as u64 * 4).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeEstimate {
    /// Summed accessor bytes. Exact.
    pub geometry_bytes: u64,
    /// Raw RGBA at the selected tier, scaled by [`PNG_RATIO`]. Estimated.
    pub texture_bytes: u64,
}

impl SizeEstimate {
    pub fn total(&self) -> u64 {
        self.geometry_bytes + self.texture_bytes
    }
}

/// Everything needed to price any option combination for one loaded ship.
pub struct ExportSizeModel {
    /// Per sub-model, its counts at every LoD it authors.
    mesh_lods: Vec<Vec<MeshCounts>>,
    /// Armor totals. Armor has no LoD ladder.
    armor: MeshCounts,
    /// Base albedo ladder per resolved DDS path (not per MFM stem: several
    /// stems can resolve to the same file, e.g. a `_wire` material variant
    /// falling back to its unsuffixed sibling's albedo, and `ImageCache`
    /// embeds that shared file once).
    base_ladders: HashMap<String, TextureLadder>,
    /// Each scheme's `(path, ladder)` pairs, so pricing a selection can union
    /// paths with the base set and with other selected schemes rather than
    /// summing per-scheme subtotals that double-count a shared path.
    scheme_ladders: HashMap<CamoSchemeId, Vec<(String, TextureLadder)>>,
}

impl ExportSizeModel {
    pub(crate) fn new(
        mesh_lods: Vec<Vec<MeshCounts>>,
        armor: MeshCounts,
        base_ladders: HashMap<String, TextureLadder>,
        scheme_ladders: HashMap<CamoSchemeId, Vec<(String, TextureLadder)>>,
    ) -> Self {
        Self { mesh_lods, armor, base_ladders, scheme_ladders }
    }

    /// Prices `options` against this model's gathered data. Only `lod`,
    /// `contents`, `textures`, `texture_lod` and `camos` are repriceable this
    /// way: `hull`, `damaged` and `module_overrides` are baked into the model
    /// at [`ShipModelContext::size_model`] build time (they select which
    /// geometry and render sets were gathered in the first place), so passing
    /// an `options` with a different `hull`, `damaged` or `module_overrides`
    /// than the model was built with silently prices the wrong ship.
    pub fn estimate(&self, options: &ShipExportOptions) -> SizeEstimate {
        let mut geometry_bytes = 0;

        if options.contents.includes_mesh() {
            for counts in &self.mesh_lods {
                // A part authors a shallower ladder than the hull and clamps into
                // its own, matching what the export does.
                let Some(deepest) = counts.len().checked_sub(1) else {
                    continue;
                };
                geometry_bytes += counts[options.lod.min(deepest)].bytes(MESH_VERTEX_BYTES);
            }
        }
        if options.contents.includes_armor() {
            geometry_bytes += self.armor.bytes(ARMOR_VERTEX_BYTES);
        }

        let texture_bytes =
            if options.textures && options.contents.includes_mesh() { self.texture_bytes(options) } else { 0 };

        SizeEstimate { geometry_bytes, texture_bytes }
    }

    fn texture_bytes(&self, options: &ShipExportOptions) -> u64 {
        let raw: u64 = match &options.camos {
            // A baked camo replaces each stem's base albedo at the base's own
            // resolution, so it costs what stock costs.
            CamoSelection::BaseOnly | CamoSelection::Baked(_) => {
                self.base_ladders.values().map(|l| l.raw_bytes(options.texture_lod)).sum()
            }
            CamoSelection::Variants(ids) => {
                // `ImageCache` dedups by content across the whole file, so a path a
                // selected scheme shares with the base set (or with another selected
                // scheme) must be priced once, not once per set it appears in.
                let mut by_path: HashMap<&str, u64> = HashMap::new();
                for (path, ladder) in &self.base_ladders {
                    by_path.insert(path.as_str(), ladder.raw_bytes(options.texture_lod));
                }
                for id in ids {
                    for (path, ladder) in self.scheme_ladders.get(id).into_iter().flatten() {
                        by_path.insert(path.as_str(), ladder.raw_bytes(options.texture_lod));
                    }
                }
                by_path.values().sum()
            }
        };
        (raw as f64 * PNG_RATIO) as u64
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::export::camo_textures::CamoSchemeId;
    use crate::export::ship::CamoSelection;
    use crate::export::ship::ExportContents;
    use crate::export::ship::ShipExportOptions;
    use crate::export::texture::MaxEdge;
    use crate::export::texture::TextureLod;

    fn ladder() -> TextureLadder {
        TextureLadder::new(vec![(4096, 4096), (2048, 2048), (1024, 1024), (512, 512)])
    }

    fn model() -> ExportSizeModel {
        let mesh_lods = vec![
            vec![MeshCounts { vertices: 100, indices: 300 }, MeshCounts { vertices: 50, indices: 150 }],
            // A part with a shallower ladder than the hull.
            vec![MeshCounts { vertices: 10, indices: 30 }],
        ];
        let armor = MeshCounts { vertices: 8, indices: 24 };
        let mut base_ladders = HashMap::new();
        base_ladders.insert("HULL".to_string(), ladder());
        let mut scheme_ladders = HashMap::new();
        scheme_ladders.insert(CamoSchemeId(0), vec![("scheme0/tex".to_string(), ladder())]);
        scheme_ladders.insert(
            CamoSchemeId(1),
            vec![("scheme1/tex_a".to_string(), ladder()), ("scheme1/tex_b".to_string(), ladder())],
        );
        ExportSizeModel::new(mesh_lods, armor, base_ladders, scheme_ladders)
    }

    fn options() -> ShipExportOptions {
        ShipExportOptions { textures: false, contents: ExportContents::Mesh, ..Default::default() }
    }

    #[test]
    fn the_ladder_picks_the_largest_tier_within_a_cap() {
        assert_eq!(ladder().dims_for(TextureLod::Full), Some((4096, 4096)));
        assert_eq!(ladder().dims_for(TextureLod::Capped(MaxEdge::new(2048).unwrap())), Some((2048, 2048)));
        assert_eq!(ladder().dims_for(TextureLod::Capped(MaxEdge::new(1500).unwrap())), Some((1024, 1024)));
    }

    #[test]
    fn a_cap_below_the_smallest_tier_falls_into_that_tiers_mip_chain() {
        assert_eq!(
            ladder().dims_for(TextureLod::Capped(MaxEdge::new(256).unwrap())),
            Some((256, 256)),
            "the decode picks a mip inside the tail rather than giving up"
        );
    }

    #[test]
    fn geometry_is_summed_at_the_clamped_lod() {
        let m = model();
        // LoD 0: (100 * 32 + 300 * 4) + (10 * 32 + 30 * 4) = 4400 + 440
        assert_eq!(m.estimate(&ShipExportOptions { lod: 0, ..options() }).geometry_bytes, 4840);
        // LoD 1: the hull steps down, the 1-lod part clamps to its own deepest.
        // (50 * 32 + 150 * 4) + (10 * 32 + 30 * 4) = 2200 + 440
        assert_eq!(m.estimate(&ShipExportOptions { lod: 1, ..options() }).geometry_bytes, 2640);
    }

    #[test]
    fn armor_only_prices_armor_and_no_textures() {
        let m = model();
        let opts = ShipExportOptions { textures: true, contents: ExportContents::Armor, ..options() };
        let e = m.estimate(&opts);
        // 8 * 40 + 24 * 4
        assert_eq!(e.geometry_bytes, 416);
        assert_eq!(e.texture_bytes, 0, "armor is untextured");
    }

    #[test]
    fn baking_prices_the_same_as_stock() {
        let m = model();
        let base = ShipExportOptions { textures: true, ..options() };
        let stock = m.estimate(&ShipExportOptions { camos: CamoSelection::BaseOnly, ..base.clone() });
        let baked = m.estimate(&ShipExportOptions { camos: CamoSelection::Baked(CamoSchemeId(0)), ..base });
        assert_eq!(baked.texture_bytes, stock.texture_bytes, "a baked camo is one image per stem, like stock");
    }

    #[test]
    fn each_variant_adds_its_own_textures() {
        let m = model();
        let base = ShipExportOptions { textures: true, ..options() };
        let stock = m.estimate(&ShipExportOptions { camos: CamoSelection::BaseOnly, ..base.clone() });
        let one =
            m.estimate(&ShipExportOptions { camos: CamoSelection::Variants(vec![CamoSchemeId(0)]), ..base.clone() });
        let two = m.estimate(&ShipExportOptions {
            camos: CamoSelection::Variants(vec![CamoSchemeId(0), CamoSchemeId(1)]),
            ..base
        });
        assert!(one.texture_bytes > stock.texture_bytes);
        assert!(two.texture_bytes > one.texture_bytes);
    }

    #[test]
    fn a_smaller_texture_cap_costs_less() {
        let m = model();
        let base = ShipExportOptions { textures: true, ..options() };
        let full = m.estimate(&ShipExportOptions { texture_lod: TextureLod::Full, ..base.clone() });
        let capped =
            m.estimate(&ShipExportOptions { texture_lod: TextureLod::Capped(MaxEdge::new(1024).unwrap()), ..base });
        assert!(capped.texture_bytes * 4 < full.texture_bytes, "4096 to 1024 is a 16x area cut");
    }

    #[test]
    fn a_scheme_texture_sharing_a_path_with_base_is_not_double_priced() {
        let mesh_lods = vec![vec![MeshCounts { vertices: 0, indices: 0 }]];
        let armor = MeshCounts { vertices: 0, indices: 0 };
        let mut base_ladders = HashMap::new();
        base_ladders.insert("shared/path.dds".to_string(), ladder());
        let mut scheme_ladders = HashMap::new();
        scheme_ladders.insert(CamoSchemeId(0), vec![("shared/path.dds".to_string(), ladder())]);
        let m = ExportSizeModel::new(mesh_lods, armor, base_ladders, scheme_ladders);

        let base = ShipExportOptions { textures: true, ..options() };
        let stock = m.estimate(&ShipExportOptions { camos: CamoSelection::BaseOnly, ..base.clone() });
        let variant = m.estimate(&ShipExportOptions { camos: CamoSelection::Variants(vec![CamoSchemeId(0)]), ..base });
        assert_eq!(
            variant.texture_bytes, stock.texture_bytes,
            "ImageCache dedups by content, so a scheme reusing the base's own path costs nothing extra"
        );
    }
}
