//! Parser for `camouflages.xml` — camouflage definitions including color schemes.
//!
//! The game's camouflage system defines texture overrides in a large XML file
//! (`camouflages.xml` in the VFS root). Each `<camouflage>` entry maps a name
//! (e.g. `mat_Steel`) to per-part albedo texture paths. Tiled camouflages also
//! reference a `colorScheme` that provides 4 RGBA colors used to colorize a
//! repeating tile pattern texture.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;

use vfs::VfsPath;

/// A color scheme with 4 RGBA colors (linear space).
///
/// The tile texture acts as a color-indexed mask: Black/R/G/B zones map to
/// color0/color1/color2/color3 respectively.
pub struct ColorScheme {
    pub name: String,
    pub colors: [[f32; 4]; 4],
}

/// UV scale/offset transform for a part category in a tiled camo.
#[derive(Clone, Debug, PartialEq)]
pub struct UvTransform {
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

impl Default for UvTransform {
    fn default() -> Self {
        Self { scale: [1.0, 1.0], offset: [0.0, 0.0] }
    }
}

/// A parsed camouflage entry from `camouflages.xml`.
pub struct CamouflageEntry {
    /// Name, e.g. "mat_Steel" or "camo_CN_NY_2018_02_tile".
    pub name: String,
    /// Whether this camo uses UV tiling (tile texture + colorScheme).
    pub tiled: bool,
    /// Whether the camo recolors the ship over its base albedo (preserving the ship's baked
    /// detail like the hull number) rather than pasting an opaque texture. Drives whether a
    /// tiled camo is composited as a detail-preserving recolor or a flat opaque replacement.
    pub use_color_scheme: bool,
    /// Per-part albedo texture paths. Key = part category (lowercase, e.g. "hull"),
    /// Value = VFS path to the albedo DDS. For tiled camos, typically just "tile".
    pub textures: HashMap<String, String>,
    /// Name of the color scheme (for tiled camos).
    pub color_scheme: Option<String>,
    /// Per-part UV transforms (for tiled camos). Key = part category in lowercase.
    pub uv_transforms: HashMap<String, UvTransform>,
    /// Ship group names this entry applies to (empty = default/fallback).
    pub ship_groups: Vec<String>,
    /// Ships this entry targets directly by full name (empty = none). Takes
    /// priority over ship_groups when resolving a per-ship variant.
    pub target_ships: Vec<String>,
}

/// Classify an MFM stem into a camouflage part category.
///
/// The camouflages.xml UV section uses categories like Tile (=hull), DeckHouse,
/// Gun, Director, Plane, Float, Misc, Bulge. MFM stems use prefixes like
/// `JSB039_Yamato_1945_Hull` or `JGA010_25mm_Type96`.
pub fn classify_part_category(mfm_stem: &str) -> &'static str {
    // Check suffix-based patterns first (hull parts end with _Hull, _DeckHouse, etc.)
    let lower = mfm_stem.to_lowercase();
    if lower.ends_with("_hull") {
        return "tile"; // "Tile" in XML = hull
    }
    if lower.ends_with("_deckhouse") {
        return "deckhouse";
    }
    if lower.contains("_bulge") {
        return "bulge";
    }

    // Non-hull fittings that camos rarely repaint. Give each its own category so that,
    // absent an explicit camo texture for it, the part keeps its stock albedo rather than
    // inheriting the whole-ship hull atlas (which uses an unrelated UV layout).
    if lower.contains("glass") {
        return "glass";
    }
    if lower.ends_with("_wire") {
        return "wire";
    }
    // Alpha-cutout geometry (nets, grids, railings) is never the painted hull.
    if lower.contains("_alpha") || lower.contains("razlom") {
        return "net";
    }

    // Equipment MFM stems are `<nation><family><variant>...`, e.g. `BGM123_...` (nation B, family
    // "GM"). The family's first letter identifies the equipment class across all nations and
    // variants (verified against the full MFM set): G* weapons, D*/F* fire control, A* aircraft,
    // R* radar/sensors. S* is the ship hull and falls through to the tile default (the hull/deckhouse/
    // bulge/wire suffixes above already peel off its named sub-parts). M*/C* (masts, boats, cranes,
    // decks, catapults) are a mixed bag that includes camouflaged deck surfaces, so they are left at
    // the tile default rather than risk keeping a camouflaged deck stock.
    let code = mfm_stem.as_bytes();
    if code.len() >= 4 && code[0].is_ascii_uppercase() {
        match code[1] {
            // Weapons: main/secondary/AA guns, depth-charge throwers, missile launchers, torpedoes.
            b'G' => return if &mfm_stem[1..3] == "GT" { "torpedo" } else { "gun" },
            // Directors and rangefinders (fire control).
            b'D' | b'F' => return "director",
            // Aircraft.
            b'A' => return "plane",
            // Radar and sensors.
            b'R' => return "misc",
            _ => {}
        }
    }

    // Fallback: hull/tile for ship body parts (prefix matches ship code pattern)
    if lower.contains("_hull") {
        return "tile";
    }

    // Default to tile (hull) — the most common category
    "tile"
}

/// Parsed camouflage database from `camouflages.xml`.
pub struct CamouflageDb {
    /// Multiple entries per camo name (different ship groups have different UV values).
    entries: HashMap<String, Vec<CamouflageEntry>>,
    color_schemes: HashMap<String, ColorScheme>,
    /// Ship group → set of ship index names (e.g. "IJN_group_5" → {"PJSB018_Yamato_1944", ...}).
    ship_groups: HashMap<String, HashSet<String>>,
}

impl CamouflageDb {
    /// Load and parse `camouflages.xml` from the VFS.
    pub fn load(vfs: &VfsPath) -> Option<Self> {
        let mut xml_bytes = Vec::new();
        vfs.join("camouflages.xml").ok()?.open_file().ok()?.read_to_end(&mut xml_bytes).ok()?;
        let xml_str = String::from_utf8_lossy(&xml_bytes);
        Self::parse(&xml_str)
    }

    fn parse(xml: &str) -> Option<Self> {
        let doc = roxmltree::Document::parse(xml).ok()?;

        // Parse <shipgroups.xml> section: group name → set of ship index names.
        let mut ship_groups: HashMap<String, HashSet<String>> = HashMap::new();
        if let Some(sg_node) = doc
            .root()
            .children()
            .find(|n| n.is_element())
            .and_then(|data| data.children().find(|n| n.has_tag_name("shipgroups.xml")))
        {
            for group_node in sg_node.children().filter(|n| n.is_element()) {
                let group_name = group_node.tag_name().name().to_string();
                if let Some(ships_node) = group_node.children().find(|n| n.has_tag_name("ships"))
                    && let Some(text) = ships_node.text()
                {
                    let indices: HashSet<String> = text.split_whitespace().map(|s| s.to_string()).collect();
                    ship_groups.insert(group_name, indices);
                }
            }
        }

        // Parse color schemes.
        let mut color_schemes = HashMap::new();
        for cs_node in doc.descendants().filter(|n| n.has_tag_name("colorScheme")) {
            let Some(name) = child_text(&cs_node, "name").map(|s| s.trim()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }

            let mut colors = [[0.0f32; 4]; 4];
            for (i, color) in colors.iter_mut().enumerate() {
                let tag = format!("color{i}");
                if let Some(text) = child_text(&cs_node, &tag) {
                    let parts: Vec<f32> = text.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                    if parts.len() >= 4 {
                        *color = [parts[0], parts[1], parts[2], parts[3]];
                    }
                }
            }

            color_schemes.insert(name.to_string(), ColorScheme { name: name.to_string(), colors });
        }

        // Parse camouflage entries. Same name may appear multiple times with
        // different <shipGroups>, so we collect them into Vec per name.
        let mut entries: HashMap<String, Vec<CamouflageEntry>> = HashMap::new();
        for camo_node in doc.descendants().filter(|n| n.has_tag_name("camouflage")) {
            let Some(name) = child_text(&camo_node, "name").map(|s| s.trim()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            // The XML writes booleans as both "true" and "True", so compare case-insensitively.
            let tiled = child_text(&camo_node, "tiled").map(|s| s.trim().eq_ignore_ascii_case("true")).unwrap_or(false);
            let use_color_scheme = child_text(&camo_node, "useColorScheme")
                .map(|s| s.trim().eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            let mut textures = HashMap::new();
            if let Some(tex_node) = camo_node.children().find(|n| n.has_tag_name("Textures")) {
                for child in tex_node.children().filter(|n| n.is_element()) {
                    let tag = child.tag_name().name();
                    // Skip MGN (metallic/gloss/normal) and animmap entries.
                    if tag.ends_with("_mgn") || tag.ends_with("_animmap") {
                        continue;
                    }
                    if let Some(path) = child.text().map(|t| t.trim().to_string())
                        && !path.is_empty()
                    {
                        textures.insert(tag.to_lowercase(), path);
                    }
                }
            }

            // Parse colorSchemes reference (take first word if multiple).
            let color_scheme = child_text(&camo_node, "colorSchemes")
                .map(|s| s.trim())
                .and_then(|s| s.split_whitespace().next())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());

            // Parse <shipGroups> text: space-separated group names.
            let camo_ship_groups: Vec<String> = child_text(&camo_node, "shipGroups")
                .map(|s| s.split_whitespace().map(|g| g.to_string()).collect())
                .unwrap_or_default();

            // Per-ship targeting: some entries target ships directly instead of via groups.
            let target_ships: Vec<String> = camo_node
                .children()
                .filter(|n| n.has_tag_name("targetShip"))
                .filter_map(|n| n.text().map(|t| t.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect();

            // Parse UV transforms per part category.
            let mut uv_transforms = HashMap::new();
            if let Some(uv_node) = camo_node.children().find(|n| n.has_tag_name("UV")) {
                for child in uv_node.children().filter(|n| n.is_element()) {
                    let tag = child.tag_name().name().to_lowercase();
                    let scale = child_text(&child, "scale")
                        .map(|s| {
                            let parts: Vec<f32> = s.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                            if parts.len() >= 2 { [parts[0], parts[1]] } else { [1.0, 1.0] }
                        })
                        .unwrap_or([1.0, 1.0]);
                    let offset = child_text(&child, "offset")
                        .map(|s| {
                            let parts: Vec<f32> = s.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                            if parts.len() >= 2 { [parts[0], parts[1]] } else { [0.0, 0.0] }
                        })
                        .unwrap_or([0.0, 0.0]);
                    uv_transforms.insert(tag, UvTransform { scale, offset });
                }
            }

            entries.entry(name.to_string()).or_default().push(CamouflageEntry {
                name: name.to_string(),
                tiled,
                use_color_scheme,
                textures,
                color_scheme,
                uv_transforms,
                ship_groups: camo_ship_groups,
                target_ships,
            });
        }

        Some(Self { entries, color_schemes, ship_groups })
    }

    /// Look up a camouflage by name, resolving the correct ship-group-specific
    /// entry for the given ship index (e.g. "PJSB018_Yamato_1944").
    ///
    /// If `ship_index` is provided, returns the entry whose ship groups contain
    /// a group that includes the ship. Falls back to the entry with no ship
    /// groups (default), or the first entry if no match.
    pub fn get(&self, name: &str, ship_index: Option<&str>) -> Option<&CamouflageEntry> {
        let variants = self.entries.get(name)?;

        // If we have a ship index, find the variant whose ship groups match. This must run even for
        // a single variant: a lone entry can still <targetShip> a specific ship, and returning it
        // for a different ship would paint that ship with the wrong per-ship texture.
        if let Some(idx) = ship_index {
            // Exact per-ship targeting wins over group membership.
            for entry in variants {
                if entry.target_ships.iter().any(|t| t == idx) {
                    return Some(entry);
                }
            }
            for entry in variants {
                if entry.ship_groups.is_empty() {
                    continue;
                }
                for group_name in &entry.ship_groups {
                    if let Some(members) = self.ship_groups.get(group_name)
                        && members.contains(idx)
                    {
                        return Some(entry);
                    }
                }
            }
            // No per-ship match for this ship: only a truly universal entry (no groups, no targets)
            // applies. Never fall back to another ship's targeted variant.
            return variants.iter().find(|e| e.ship_groups.is_empty() && e.target_ships.is_empty());
        }

        // No ship context: prefer a truly universal entry, else the first.
        variants.iter().find(|e| e.ship_groups.is_empty() && e.target_ships.is_empty()).or(variants.first())
    }

    /// Look up a color scheme by name.
    pub fn color_scheme(&self, name: &str) -> Option<&ColorScheme> {
        self.color_schemes.get(name)
    }

    /// Number of unique camouflage names in the database.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the database is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total number of camouflage entries (including ship-group variants).
    pub fn total_entries(&self) -> usize {
        self.entries.values().map(|v| v.len()).sum()
    }
}

fn child_text<'a>(node: &'a roxmltree::Node, tag: &str) -> Option<&'a str> {
    node.children().find(|n| n.has_tag_name(tag))?.text()
}

#[cfg(test)]
mod tests {
    use super::classify_part_category;

    #[test]
    fn classify_hull_and_named_subparts() {
        // Ship hull families (S*) and suffix-named sub-parts.
        assert_eq!(classify_part_category("WSD011_Smaland_1955"), "tile");
        assert_eq!(classify_part_category("BSB011_Queen_Elizabeth_1942_Hull"), "tile");
        assert_eq!(classify_part_category("BSC026_London_1943_DeckHouse"), "deckhouse");
        assert_eq!(classify_part_category("BS401_Thunderer_HWA_Bulge"), "bulge");
        assert_eq!(classify_part_category("WSD011_Smaland_1955_wire"), "wire");
        // A hull rigging wire keeps stock (wire), not the hull atlas on wire UVs.
        assert_eq!(classify_part_category("BSA004_Malta_1950_Hull_wire"), "wire");
    }

    #[test]
    fn classify_equipment_families() {
        // Weapons: main/secondary/AA guns, depth charges, missiles -> gun; torpedoes -> torpedo.
        assert_eq!(classify_part_category("BGM123_7_5in45_BL_MkVI"), "gun");
        assert_eq!(classify_part_category("BGS535_QF_5_2in_MkI"), "gun");
        assert_eq!(classify_part_category("BGA597_40mm_Bofors_MK_VI"), "gun");
        assert_eq!(classify_part_category("BGB110_Depth_Charge_Thrower"), "gun");
        assert_eq!(classify_part_category("RGR2018_Uran_Launcher"), "gun");
        assert_eq!(classify_part_category("BGT095_21in_5tube_Torpedo_Tube"), "torpedo");
        // Fire control: directors D0-D9 and rangefinders F0-F9 (all variants) -> director.
        assert_eq!(classify_part_category("BD059_HACS_MK_IV_RF15ft"), "director");
        assert_eq!(classify_part_category("ZD2003_MRS3_Director"), "director");
        assert_eq!(classify_part_category("BD3000_DCT_MK_XXl_with_finder"), "director");
        assert_eq!(classify_part_category("BF056_Rangefinder_2_7m"), "director");
        assert_eq!(classify_part_category("ZF3002_SPN_500_1_4m"), "director");
        // Aircraft and radar.
        assert_eq!(classify_part_category("BAB004_Fairey_Swordfish_MkII"), "plane");
        assert_eq!(classify_part_category("BRS079_Radar_SM_1"), "misc");
    }

    #[test]
    fn classify_never_camouflaged_fittings() {
        // Glass, wire, and alpha-cutout nets/grids get their own categories (kept stock).
        assert_eq!(classify_part_category("transparent_glass_alpha"), "glass");
        assert_eq!(classify_part_category("C012_Glass_alpha_holographic"), "glass");
        assert_eq!(classify_part_category("C004_Grid_1_alpha"), "net");
        assert_eq!(classify_part_category("C002_Razlom"), "net");
        assert_eq!(classify_part_category("MidBack_wireShape_wire"), "wire");
    }
}
