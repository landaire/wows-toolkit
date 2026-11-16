//! Deterministic display pseudonyms for player names and clan tags.
//!
//! A pseudonym depends only on the real value, so every view that shows the
//! same player or clan shows the same pseudonym.

use std::borrow::Cow;

/// How player names and clan tags are displayed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum NameDisplay {
    #[default]
    Real,
    Pseudonym,
}

impl NameDisplay {
    pub fn from_anonymize(anonymize: bool) -> Self {
        if anonymize { Self::Pseudonym } else { Self::Real }
    }

    pub fn is_pseudonym(self) -> bool {
        self == Self::Pseudonym
    }

    pub fn player_name(self, real: &str) -> Cow<'_, str> {
        match self {
            Self::Real => Cow::Borrowed(real),
            Self::Pseudonym => Cow::Owned(player_name(real)),
        }
    }

    /// `real` is the bare tag, without brackets.
    pub fn clan_tag(self, real: &str) -> Cow<'_, str> {
        match self {
            Self::Real => Cow::Borrowed(real),
            Self::Pseudonym => Cow::Owned(clan_tag(real)),
        }
    }
}

const ADJECTIVES: [&str; 128] = [
    "Amber", "Arctic", "Ashen", "Autumn", "Azure", "Bold", "Brave", "Breezy", "Bright", "Brisk", "Bronze", "Calm",
    "Candid", "Clear", "Clever", "Coastal", "Cobalt", "Copper", "Crimson", "Crisp", "Deep", "Distant", "Dusky",
    "Eager", "Early", "Eastern", "Emerald", "Even", "Fair", "Faithful", "Fast", "Fearless", "Fiery", "Firm", "Fleet",
    "Frosty", "Gentle", "Gilded", "Glad", "Golden", "Grand", "Granite", "Green", "Hardy", "Hazel", "Hidden", "High",
    "Hollow", "Honest", "Humble", "Icy", "Indigo", "Iron", "Ivory", "Jade", "Jolly", "Keen", "Kind", "Lively", "Loyal",
    "Lucky", "Lunar", "Mellow", "Merry", "Mighty", "Misty", "Modest", "Mossy", "Noble", "Northern", "Oaken", "Olive",
    "Patient", "Pearl", "Placid", "Plain", "Polar", "Proud", "Quick", "Quiet", "Rapid", "Ready", "Rocky", "Royal",
    "Ruby", "Rugged", "Russet", "Rustic", "Sable", "Salty", "Sandy", "Scarlet", "Serene", "Sharp", "Silent", "Silver",
    "Slate", "Sleek", "Smooth", "Solar", "Solid", "Southern", "Speedy", "Spry", "Stable", "Steady", "Steel", "Stormy",
    "Stout", "Sturdy", "Sunny", "Swift", "Tidal", "Topaz", "Tranquil", "True", "Valiant", "Velvet", "Vivid", "Warm",
    "Western", "Wild", "Windy", "Winter", "Wise", "Young", "Cedar", "Coral",
];

const NOUNS: [&str; 128] = [
    "Anchor",
    "Anvil",
    "Arrow",
    "Atlas",
    "Badger",
    "Bay",
    "Beacon",
    "Bell",
    "Birch",
    "Bison",
    "Boulder",
    "Breaker",
    "Breeze",
    "Bridge",
    "Brook",
    "Buoy",
    "Cabin",
    "Canal",
    "Canyon",
    "Cape",
    "Capstan",
    "Comet",
    "Compass",
    "Cove",
    "Crane",
    "Crest",
    "Current",
    "Cutter",
    "Delta",
    "Dock",
    "Dolphin",
    "Drift",
    "Dune",
    "Eagle",
    "Ember",
    "Falcon",
    "Ferry",
    "Field",
    "Fjord",
    "Flag",
    "Forest",
    "Fox",
    "Galley",
    "Gannet",
    "Glacier",
    "Gull",
    "Gust",
    "Harbor",
    "Hawk",
    "Heron",
    "Hill",
    "Horizon",
    "Inlet",
    "Island",
    "Jetty",
    "Keel",
    "Kestrel",
    "Kite",
    "Lagoon",
    "Lake",
    "Lantern",
    "Ledge",
    "Lighthouse",
    "Lynx",
    "Mariner",
    "Marsh",
    "Mast",
    "Meadow",
    "Meridian",
    "Mesa",
    "Moon",
    "Narwhal",
    "Oak",
    "Oar",
    "Ocean",
    "Orca",
    "Osprey",
    "Otter",
    "Owl",
    "Paddle",
    "Peak",
    "Pelican",
    "Pier",
    "Pilot",
    "Pine",
    "Plover",
    "Port",
    "Quay",
    "Rain",
    "Raven",
    "Reef",
    "Ridge",
    "River",
    "Robin",
    "Rudder",
    "Sail",
    "Salmon",
    "Sandbar",
    "Seal",
    "Shell",
    "Shoal",
    "Signal",
    "Sound",
    "Spar",
    "Sparrow",
    "Squall",
    "Star",
    "Stern",
    "Stone",
    "Storm",
    "Strait",
    "Summit",
    "Swell",
    "Tern",
    "Thunder",
    "Tide",
    "Timber",
    "Trawler",
    "Valley",
    "Voyage",
    "Walrus",
    "Wave",
    "Whale",
    "Wharf",
    "Willow",
    "Wind",
    "Wren",
    "Maple",
];

const CLAN_TAGS: [&str; 128] = [
    "BRASS", "KELP", "FERN", "OAK", "ASH", "ELM", "PINE", "REEF", "TIDE", "GULL", "TERN", "WREN", "OWL", "ELK", "FOX",
    "LYNX", "HAWK", "CRANE", "HERON", "ORCA", "SEAL", "BAY", "COVE", "CAPE", "DUNE", "MESA", "PEAK", "CREST", "RIDGE",
    "VALE", "GLEN", "MOOR", "FORD", "KEEL", "MAST", "SAIL", "OAR", "HELM", "BUOY", "PIER", "DOCK", "QUAY", "PORT",
    "WAKE", "GALE", "GUST", "MIST", "RAIN", "SNOW", "HAIL", "FROST", "STORM", "BOLT", "IRON", "TIN", "ZINC", "GOLD",
    "JADE", "OPAL", "RUBY", "ONYX", "AMBER", "PEARL", "CORAL", "SLATE", "FLINT", "STONE", "CLAY", "SAND", "SALT",
    "MOSS", "REED", "RUSH", "SAGE", "MINT", "ROSE", "LILY", "IRIS", "LARK", "ROOK", "KITE", "SWAN", "DOVE", "CROW",
    "LOON", "PIKE", "CARP", "BASS", "COD", "EEL", "RAY", "SKATE", "SQUID", "CRAB", "KRILL", "NORTH", "SOUTH", "EAST",
    "WEST", "ARC", "AXIS", "APEX", "NOVA", "STAR", "MOON", "SUN", "COMET", "ORBIT", "ANVIL", "FORGE", "TORCH", "LAMP",
    "FLARE", "EMBER", "BLAZE", "BIRCH", "CEDAR", "MAPLE", "ALDER", "ROWAN", "YEW", "HAZEL", "LARCH", "PLUM", "FIG",
    "LIME", "OLIVE", "BRINE",
];

/// Two capitalized words, such as `QuietHarbor`, derived from the real name.
pub fn player_name(real: &str) -> String {
    let hash = stable_hash(b"player", real);
    let adjective = ADJECTIVES[word_index(hash, 0)];
    let noun = NOUNS[word_index(hash, 1)];

    format!("{adjective}{noun}")
}

/// A 3 to 5 letter uppercase tag derived from the real bare tag. An empty tag
/// means no clan and stays empty.
pub fn clan_tag(real: &str) -> String {
    if real.is_empty() {
        return String::new();
    }

    CLAN_TAGS[word_index(stable_hash(b"clan", real), 0)].to_string()
}

/// Seven bits of `hash` per word, taken from the top.
fn word_index(hash: u64, word: u32) -> usize {
    ((hash >> (57 - 7 * word)) & 0x7F) as usize
}

/// FNV-1a over `domain`, a separator and `value`, then the SplitMix64
/// finalizer so the high bits depend on every input byte.
fn stable_hash(domain: &[u8], value: &str) -> u64 {
    const OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01B3;

    let mut hash = OFFSET;
    for byte in domain.iter().chain(&[0u8]).chain(value.as_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }

    hash = (hash ^ (hash >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    hash = (hash ^ (hash >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);

    hash ^ (hash >> 31)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn a_fixed_name_maps_to_a_fixed_pseudonym() {
        assert_eq!(player_name("lander"), "OakenThunder");
        assert_eq!(player_name("Player1"), "KindField");
        assert_eq!(clan_tag("WTK"), "ZINC");
    }

    #[test]
    fn pseudonyms_are_deterministic() {
        assert_eq!(player_name("SomeCaptain"), player_name("SomeCaptain"));
        assert_eq!(clan_tag("RED"), clan_tag("RED"));
    }

    #[test]
    fn clanmates_share_one_tag() {
        let first = NameDisplay::Pseudonym.clan_tag("WTK").into_owned();
        let second = NameDisplay::Pseudonym.clan_tag("WTK").into_owned();

        assert_eq!(first, second);
        assert_ne!(first, "WTK");
    }

    #[test]
    fn no_clan_stays_no_clan() {
        assert_eq!(clan_tag(""), "");
        assert_eq!(NameDisplay::Pseudonym.clan_tag(""), "");
    }

    #[test]
    fn real_display_returns_the_input() {
        assert_eq!(NameDisplay::Real.player_name("lander"), "lander");
        assert_eq!(NameDisplay::Real.clan_tag("WTK"), "WTK");
    }

    #[test]
    fn output_is_ascii_and_well_formed() {
        for idx in 0..512 {
            let name = player_name(&format!("player_{idx}"));
            assert!(name.is_ascii() && name.chars().all(|c| c.is_ascii_alphabetic()), "{name}");
            assert!(name.starts_with(|c: char| c.is_ascii_uppercase()), "{name}");

            let tag = clan_tag(&format!("C{idx}"));
            assert!((3..=5).contains(&tag.len()), "{tag}");
            assert!(tag.chars().all(|c| c.is_ascii_uppercase()), "{tag}");
        }
    }

    #[test]
    fn word_lists_have_no_duplicates() {
        for list in [&ADJECTIVES, &NOUNS, &CLAN_TAGS] {
            let unique: HashSet<&str> = list.iter().copied().collect();
            assert_eq!(unique.len(), list.len());
        }
    }
}
