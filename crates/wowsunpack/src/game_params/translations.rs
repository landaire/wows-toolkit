//! Translation key builders and icon path helpers for WoWS game data.
//!
//! These functions build the localization IDS_* keys used to look up
//! translated strings via `ResourceLoader::localized_name_from_id()`.

use crate::data::ResourceLoader;

// =============================================================================
// Map / Game Mode / Scenario translations
// =============================================================================

/// Build the IDS key for a map name (e.g. "spaces/01_solo_ocean" -> "IDS_SPACES/01_SOLO_OCEAN").
pub fn translate_map_name(map_name: &str, resource_loader: &dyn ResourceLoader) -> String {
    let id = format!("IDS_{}", map_name.to_uppercase());
    resource_loader.localized_name_from_id(&id).unwrap_or_else(|| map_name.to_string())
}

/// Build the IDS key for a game mode (e.g. "Domination" -> "IDS_DOMINATION").
pub fn translate_game_mode(game_type: &str, resource_loader: &dyn ResourceLoader) -> String {
    let id = format!("IDS_{}", game_type.to_uppercase());
    resource_loader.localized_name_from_id(&id).unwrap_or_else(|| game_type.to_string())
}

/// Build the IDS key for a scenario.
pub fn translate_scenario(scenario: &str, resource_loader: &dyn ResourceLoader) -> String {
    let id = format!("IDS_SCENARIO_{}", scenario.to_uppercase());
    resource_loader.localized_name_from_id(&id).unwrap_or_else(|| scenario.to_string())
}

// =============================================================================
// Achievement translations
// =============================================================================

/// Translate an achievement's display name from its `ui_name`.
pub fn translate_achievement_name(ui_name: &str, resource_loader: &dyn ResourceLoader) -> Option<String> {
    resource_loader.localized_name_from_id(&format!("IDS_ACHIEVEMENT_{ui_name}"))
}

/// Translate an achievement's description from its `ui_name`.
pub fn translate_achievement_description(ui_name: &str, resource_loader: &dyn ResourceLoader) -> Option<String> {
    resource_loader.localized_name_from_id(&format!("IDS_ACHIEVEMENT_DESCRIPTION_{ui_name}"))
}

// =============================================================================
// Ribbon translations
// =============================================================================

/// Result of looking up a ribbon's display name.
pub struct RibbonTranslation {
    pub display_name: String,
    pub description: String,
    pub is_subribbon: bool,
    /// Lowercase ribbon key for icon lookups.
    pub icon_key: String,
}

/// Translate a ribbon key (e.g. "RIBBON_MAIN_CALIBER") into its display name and description.
///
/// Tries `IDS_RIBBON_{key}` first, then falls back to `IDS_RIBBON_SUB{key}`.
/// Returns `None` if no translation is found.
pub fn translate_ribbon(key: &str, resource_loader: &dyn ResourceLoader) -> Option<RibbonTranslation> {
    let (display_name, is_subribbon) = resource_loader
        .localized_name_from_id(&format!("IDS_RIBBON_{key}"))
        .map(|name| (name, false))
        .or_else(|| {
            resource_loader
                .localized_name_from_id(&format!("IDS_RIBBON_SUB{key}"))
                .map(|name| (name, true))
        })?;

    let description = resource_loader
        .localized_name_from_id(&format!("IDS_RIBBON_DESCRIPTION_{key}"))
        .or_else(|| resource_loader.localized_name_from_id(&format!("IDS_RIBBON_DESCRIPTION_SUB{key}")))
        .unwrap_or_default();

    Some(RibbonTranslation { display_name, description, is_subribbon, icon_key: key.to_lowercase() })
}

// =============================================================================
// Module / Consumable translations
// =============================================================================

/// Translate a module (modernization/upgrade) by its GameParams name.
/// Returns `(name, description)` where either may be `None`.
pub fn translate_module(
    game_params_name: &str,
    resource_loader: &dyn ResourceLoader,
) -> (Option<String>, Option<String>) {
    let name_id = format!("IDS_TITLE_{}", game_params_name.to_uppercase());
    let name = resource_loader.localized_name_from_id(&name_id);

    let desc_id = format!("IDS_DESC_{}", game_params_name.to_uppercase());
    let description = resource_loader
        .localized_name_from_id(&desc_id)
        .and_then(|desc| if desc.is_empty() || desc == " " { None } else { Some(desc) });

    (name, description)
}

/// Translate a consumable (ability) by its GameParams name.
pub fn translate_consumable(game_params_name: &str, resource_loader: &dyn ResourceLoader) -> Option<String> {
    let id = format!("IDS_DOCK_CONSUME_TITLE_{}", game_params_name.to_uppercase());
    resource_loader.localized_name_from_id(&id)
}

// =============================================================================
// Icon path helpers
// =============================================================================

/// Returns the game-file path for a ship class minimap icon SVG.
///
/// e.g. `"gui/fla/minimap/ship_icons/minimap_destroyer.svg"`
pub fn ship_class_icon_path(species: &crate::game_params::types::Species) -> String {
    format!("gui/fla/minimap/ship_icons/minimap_{}.svg", species.name().to_ascii_lowercase())
}

/// Returns the game-file path for an achievement icon PNG.
///
/// e.g. `"gui/achievements/icon_achievement_warrior.png"`
pub fn achievement_icon_path(icon_key: &str) -> String {
    format!("gui/achievements/icon_achievement_{icon_key}.png")
}

/// Directory path for ribbon icons.
pub const RIBBON_ICONS_DIR: &str = "gui/ribbons";

/// Directory path for sub-ribbon icons.
pub const RIBBON_SUBICONS_DIR: &str = "gui/ribbons/subribbons";
