//! Translation key builders and icon path helpers for WoWS game data.
//!
//! These functions build the localization IDS_* keys used to look up
//! translated strings via `ResourceLoader::localized_name_from_id()`.

use crate::data::ResourceLoader;
use crate::data::TranslationKey;

// =============================================================================
// Map / Game Mode / Scenario translations
// =============================================================================

/// Build the IDS key for a map name (e.g. "spaces/01_solo_ocean" -> "IDS_SPACES/01_SOLO_OCEAN").
pub fn translate_map_name(map_name: &str, resource_loader: &dyn ResourceLoader) -> String {
    let id = format!("IDS_{}", map_name.to_uppercase());
    resource_loader.localized_name_from_id(&TranslationKey::new(id)).unwrap_or_else(|| map_name.to_string())
}

/// Build the IDS key for a game mode (e.g. "Domination" -> "IDS_DOMINATION").
pub fn translate_game_mode(game_type: &str, resource_loader: &dyn ResourceLoader) -> String {
    let id = format!("IDS_{}", game_type.to_uppercase());
    resource_loader.localized_name_from_id(&TranslationKey::new(id)).unwrap_or_else(|| game_type.to_string())
}

/// Build the IDS key for a scenario.
pub fn translate_scenario(scenario: &str, resource_loader: &dyn ResourceLoader) -> String {
    let id = format!("IDS_SCENARIO_{}", scenario.to_uppercase());
    resource_loader.localized_name_from_id(&TranslationKey::new(id)).unwrap_or_else(|| scenario.to_string())
}

// =============================================================================
// Achievement translations
// =============================================================================

/// Translate an achievement's display name from its `ui_name`.
pub fn translate_achievement_name(ui_name: &str, resource_loader: &dyn ResourceLoader) -> Option<String> {
    resource_loader.localized_name_from_id(&TranslationKey::new(format!("IDS_ACHIEVEMENT_{ui_name}")))
}

/// Translate an achievement's description from its `ui_name`.
pub fn translate_achievement_description(ui_name: &str, resource_loader: &dyn ResourceLoader) -> Option<String> {
    resource_loader.localized_name_from_id(&TranslationKey::new(format!("IDS_ACHIEVEMENT_DESCRIPTION_{ui_name}")))
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
        .localized_name_from_id(&TranslationKey::new(format!("IDS_RIBBON_{key}")))
        .map(|name| (name, false))
        .or_else(|| {
            resource_loader
                .localized_name_from_id(&TranslationKey::new(format!("IDS_RIBBON_SUB{key}")))
                .map(|name| (name, true))
        })?;

    let description = resource_loader
        .localized_name_from_id(&TranslationKey::new(format!("IDS_RIBBON_DESCRIPTION_{key}")))
        .or_else(|| {
            resource_loader.localized_name_from_id(&TranslationKey::new(format!("IDS_RIBBON_DESCRIPTION_SUB{key}")))
        })
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
    let name = resource_loader.localized_name_from_id(&TranslationKey::new(name_id));

    let desc_id = format!("IDS_DESC_{}", game_params_name.to_uppercase());
    let description = resource_loader
        .localized_name_from_id(&TranslationKey::new(desc_id))
        .and_then(|desc| if desc.is_empty() || desc == " " { None } else { Some(desc) });

    (name, description)
}

/// Resolve a mounted exterior's (name, description). Tries upgrade keys
/// (`IDS_TITLE_<NAME>`/`IDS_DESC_<NAME>`), then bare signal keys
/// (`IDS_<NAME>`/`IDS_<NAME>_DESCRIPTION`), then the exterior's own title key.
pub fn translate_exterior(
    param: &crate::game_params::types::Param,
    resource_loader: &dyn ResourceLoader,
) -> (Option<String>, Option<String>) {
    translate_exterior_by_name(param.name(), param.exterior().and_then(|e| e.title()), resource_loader)
}

/// Resolve (name, description) for an exterior from its GameParams name and its
/// own title key. Tries upgrade keys (`IDS_TITLE_<NAME>`/`IDS_DESC_<NAME>`), then
/// bare signal keys (`IDS_<NAME>`/`IDS_<NAME>_DESCRIPTION`), then the exterior's
/// own title key.
pub fn translate_exterior_by_name(
    name: &str,
    own_title: Option<&str>,
    resource_loader: &dyn ResourceLoader,
) -> (Option<String>, Option<String>) {
    let (mod_name, mod_desc) = translate_module(name, resource_loader);
    let upper = name.to_ascii_uppercase();
    let direct_name = resource_loader.localized_name_from_id(&TranslationKey::new(format!("IDS_{upper}")));
    let direct_desc = resource_loader.localized_name_from_id(&TranslationKey::new(format!("IDS_{upper}_DESCRIPTION")));
    let own_name = own_title.and_then(|id| resource_loader.localized_name_from_id(&TranslationKey::new(id)));
    (mod_name.or(direct_name).or(own_name), mod_desc.or(direct_desc))
}

/// Generated description for a modifier-based param (modernization or exterior),
/// from its modifiers for the given ship species. `None` when the param has no
/// modifiers or none format for this version.
pub fn generated_param_description(
    param: &crate::game_params::types::Param,
    species: crate::game_params::types::Species,
    resource_loader: &dyn ResourceLoader,
    version: crate::data::Version,
) -> Option<String> {
    let mods = param.modernization().map(|m| m.modifiers()).or_else(|| param.exterior().map(|e| e.modifiers()))?;
    let lines = crate::game_params::modifier_settings_data::describe_modifiers(
        version,
        mods.iter().map(|m| (m.name(), m.get_for_species(&species))),
        species,
        resource_loader,
    );
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Translate a ship unit/module (hull, main battery, torpedoes, fire control, engine,
/// ...) by its GameParams name. Units localize as `IDS_<NAME>` -- unlike upgrades, which
/// use `IDS_TITLE_<NAME>`, and unlike ships, whose `param.index()` is only the name prefix.
pub fn translate_unit(game_params_name: &str, resource_loader: &dyn ResourceLoader) -> Option<String> {
    let id = format!("IDS_{}", game_params_name.to_uppercase());
    resource_loader.localized_name_from_id(&TranslationKey::new(id))
}

/// Translate a consumable (ability) by its GameParams name.
pub fn translate_consumable(game_params_name: &str, resource_loader: &dyn ResourceLoader) -> Option<String> {
    let id = format!("IDS_DOCK_CONSUME_TITLE_{}", game_params_name.to_uppercase());
    resource_loader.localized_name_from_id(&TranslationKey::new(id))
}

/// Translate a consumable's tooltip description by its GameParams name.
pub fn translate_consumable_description(
    game_params_name: &str,
    resource_loader: &dyn ResourceLoader,
) -> Option<String> {
    let id = format!("IDS_DOCK_CONSUME_DESCRIPTION_{}", game_params_name.to_uppercase());
    resource_loader.localized_name_from_id(&TranslationKey::new(id))
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
