//! What a tactics board calls the maps and modes it offers.

use wows_replay_insights::cap_layout::CapLayout;
use wowsunpack::game_params::provider::GameMetadataProvider;

/// A map's name as the game states it, or a readable form of its space name.
///
/// A build that is not loaded yet has no translations, and the space name is
/// still better than nothing: `spaces/16_OC_bees_to_honey` reads as
/// "OC bees to honey" rather than as a path.
pub fn map_label(map_name: &str, metadata: Option<&GameMetadataProvider>) -> String {
    match metadata {
        Some(metadata) => wowsunpack::game_params::translations::translate_map_name(map_name, metadata),
        None => pretty_map_name(map_name),
    }
}

/// A space name with its directory and its leading number taken off.
pub fn pretty_map_name(map_name: &str) -> String {
    let bare = map_name.strip_prefix("spaces/").unwrap_or(map_name);
    let stripped = bare.find('_').map(|at| &bare[at + 1..]).unwrap_or(bare);
    stripped.replace('_', " ")
}

/// One mode as the picker lists it: what it is called, and how many capture
/// points it puts on the map.
pub fn mode_label(layout: &CapLayout, metadata: Option<&GameMetadataProvider>) -> String {
    let scenario = match metadata {
        Some(metadata) => wowsunpack::game_params::translations::translate_scenario(&layout.scenario, metadata),
        None => layout.scenario.clone(),
    };
    format!("{scenario} - {} caps", layout.points.len())
}

/// The labels for a map's modes, with the ties broken.
///
/// Two layouts of one map can be the same mode with the caps in different
/// places, which reads as one entry twice. Numbering them says there are two
/// without claiming to know what tells them apart.
pub fn mode_labels(layouts: &[CapLayout], metadata: Option<&GameMetadataProvider>) -> Vec<String> {
    let base: Vec<String> = layouts.iter().map(|layout| mode_label(layout, metadata)).collect();

    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for label in &base {
        *seen.entry(label.as_str()).or_default() += 1;
    }

    let mut numbered: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    base.iter()
        .map(|label| {
            if seen[label.as_str()] < 2 {
                return label.clone();
            }
            let at = numbered.entry(label.as_str()).or_default();
            *at += 1;
            format!("{label} (variant {at})")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_name_reads_without_its_directory_or_number() {
        assert_eq!(pretty_map_name("spaces/16_OC_bees_to_honey"), "OC bees to honey");
        assert_eq!(pretty_map_name("13_OC_new_dawn"), "OC new dawn");
        assert_eq!(pretty_map_name("solomon"), "solomon");
    }

    #[test]
    fn two_layouts_of_one_mode_are_numbered_and_one_is_not() {
        let layout = |scenario: &str, caps: usize| CapLayout {
            key: wows_replay_insights::cap_layout::CapLayoutKey { map_id: 1, scenario_config_id: caps as u32 },
            map_name: "spaces/16_OC_bees_to_honey".to_owned(),
            scenario: scenario.to_owned(),
            game_mode: 7,
            points: Vec::new(),
        };

        let labels = mode_labels(&[layout("Domination", 0), layout("Domination", 1), layout("Epicenter", 2)], None);

        assert_eq!(
            labels,
            ["Domination - 0 caps (variant 1)", "Domination - 0 caps (variant 2)", "Epicenter - 0 caps"]
        );
    }
}
