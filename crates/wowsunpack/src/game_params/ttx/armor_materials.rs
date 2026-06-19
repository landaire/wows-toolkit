//! Collision-material name table and armor-type classification.
//!
//! `collision_material_name` transcribes the game client's
//! `py_collisionMaterialName` table (`Lesta.collisionMaterialName`, used by
//! `PreprocessedArmor.py:10`). `armor_type_classifies` transcribes
//! `ArmorConstants.getArmorType` (`ArmorConstants.py:21`): a plate is part of the
//! displayed-armor set when its collision-material name matches any armor-type
//! prefix (or, for `Dual_*` materials, when a split token equals a prefix).
//!
//! These live in the parsing-gated `game_params` tree so `FactoryArmor`
//! (`factories::armor`) can classify plates without the `models` feature.
//! `gltf_export` re-uses the same table.

/// Per-armor-type collision-name prefixes (`ArmorConstants.py:39`'s `armorTypes`).
/// Index = `ARMOR_TYPES` ordinal (CITADEL=0 .. SUB_BOW_ST=10). Empty tuples are
/// the dual/submarine pseudo-types that carry no prefixes of their own.
/// Order here matches `ARMOR_TYPES.TYPE_ORDER` (`ArmorConstants.py:14`); within
/// `getArmorType` the iteration order only affects which single type a non-dual
/// name resolves to, which does not change the displayed min/max.
const ARMOR_TYPE_PREFIXES: &[&[&str]] = &[
    // ARTI (1)
    &["AuTurret", "Turret", "Tur", "SGBarbetteSS", "SGDownSS", "SS_SGBarbette", "SS_SGDown"],
    // CITADEL (0)
    &["Cit", "SideCit", "DeckCit", "TransCit", "InclinCit"],
    // DD_CAS (7) - no prefixes
    &[],
    // SUB_CAS (8) - no prefixes
    &[],
    // CAS (2)
    &["Cas", "SideCas", "DeckCas", "TransCas", "InclinCas"],
    // UPCAS (3)
    &["SSC", "SideSSC", "DeckSSC", "TransSSC", "InclinSSC"],
    // SS (4)
    &["SS", "SideSS", "DeckSS", "TransSS", "InclinSS"],
    // OUTER (5)
    &["Bulge", "Belt", "Bottom", "OCit"],
    // SUB_OUTER (9) - no prefixes
    &[],
    // BOW_ST (6)
    &[
        "Bow",
        "SideBow",
        "DeckBow",
        "TransBow",
        "InclinBow",
        "St",
        "SideStern",
        "DeckStern",
        "TransStern",
        "InclinStern",
    ],
    // SUB_BOW_ST (10) - no prefixes
    &[],
];

/// Transcription of `ArmorConstants.getArmorType` (`ArmorConstants.py:21`),
/// reduced to the boolean the `PreprocessedArmor.py:10` filter needs: returns
/// `true` when `name` classifies into at least one armor type (i.e. the deob's
/// returned `frozenset` is non-empty), `false` otherwise.
///
/// A non-`Dual` name returns true as soon as it `startswith` any prefix. A
/// `Dual_*` name accumulates matches by comparing the 2nd/3rd `_`-split tokens
/// against each prefix; it returns true if any token equals a prefix.
pub fn armor_type_classifies(name: &str) -> bool {
    let is_dual = name.starts_with("Dual");
    let dual_tokens: Vec<&str> = if is_dual { name.split('_').skip(1).take(2).collect() } else { Vec::new() };

    for prefixes in ARMOR_TYPE_PREFIXES {
        for prefix in *prefixes {
            if name.starts_with(prefix) {
                return true;
            }
            if is_dual && dual_tokens.iter().any(|t| t == prefix) {
                return true;
            }
        }
    }
    false
}

/// The built-in collision material name table.
///
/// Contiguous array indexed by material ID (0..=254). Extracted from the game
/// client's `py_collisionMaterialName` table at 0x142a569a0.
pub const COLLISION_MATERIAL_NAMES: &[&str] = &[
    // 0-1: generic
    "common", // 0
    "zero",   // 1
    // 2-31: Dual-zone materials
    "Dual_SSC_Bow_Side",       // 2
    "Dual_SSC_St_Side",        // 3
    "Dual_Cas_OCit_Belt",      // 4
    "Dual_OCit_St_Trans",      // 5
    "Dual_OCit_Bow_Trans",     // 6
    "Dual_Cit_Bow_Side",       // 7
    "Dual_Cit_Bow_Belt",       // 8
    "Dual_Cit_Bow_ArtSide",    // 9
    "Dual_Cit_St_Side",        // 10
    "Dual_Cit_St_Belt",        // 11
    "Bottom",                  // 12
    "Dual_Cit_St_ArtSide",     // 13
    "Dual_Cas_Bow_Belt",       // 14
    "Dual_Cas_St_Belt",        // 15
    "Dual_Cas_SSC_Belt",       // 16
    "Dual_SSC_Bow_ConstrSide", // 17
    "Dual_SSC_St_ConstrSide",  // 18
    "Cas_Inclin",              // 19
    "SSC_Inclin",              // 20
    "Dual_Cas_SSC_Inclin",     // 21
    "Dual_Cas_Bow_Inclin",     // 22
    "Dual_Cas_St_Inclin",      // 23
    "Dual_SSC_Bow_Inclin",     // 24
    "Dual_SSC_St_Inclin",      // 25
    "Dual_Cit_Bow_Bulge",      // 26
    "Dual_Cit_St_Bulge",       // 27
    "Dual_Cas_SS_Belt",        // 28
    "Dual_Cit_Cas_ArtDeck",    // 29
    "Dual_Cit_Cas_ArtSide",    // 30
    "Dual_OCit_OCit_Side",     // 31
    // 32-45: turret/artillery/auxiliary turret
    "TurretSide",       // 32
    "TurretTop",        // 33
    "TurretFront",      // 34
    "TurretAft",        // 35
    "FunnelSide",       // 36
    "ArtBottom",        // 37
    "ArtSide",          // 38
    "ArtTop",           // 39
    "AuTurretAft",      // 40
    "AuTurretBarbette", // 41
    "AuTurretDown",     // 42
    "AuTurretFwd",      // 43
    "AuTurretSide",     // 44
    "AuTurretTop",      // 45
    // 46-51: Bow
    "Bow_Belt",       // 46
    "Bow_Bottom",     // 47
    "Bow_ConstrSide", // 48
    "Bow_Deck",       // 49
    "Bow_Inclin",     // 50
    "Bow_Trans",      // 51
    // 52-54: Bridge
    "BridgeBottom", // 52
    "BridgeSide",   // 53
    "BridgeTop",    // 54
    // 55-58: Casemate
    "Cas_AftTrans", // 55
    "Cas_Belt",     // 56
    "Cas_Deck",     // 57
    "Cas_FwdTrans", // 58
    // 59-68: Citadel
    "Cit_AftTrans",       // 59
    "Cit_Barbette",       // 60
    "Cit_Belt",           // 61
    "Cit_Bottom",         // 62
    "Cit_Bulge",          // 63
    "Cit_Deck",           // 64
    "Cit_FwdTrans",       // 65
    "Cit_Inclin",         // 66
    "Cit_Side",           // 67
    "Dual_Cit_Cas_Bulge", // 68
    // 69-79: Hull/misc
    "ConstrSide",        // 69
    "Dual_Cit_Cas_Belt", // 70
    "Bow_Fdck",          // 71
    "St_Fdck",           // 72
    "KdpBottom",         // 73
    "KdpSide",           // 74
    "KdpTop",            // 75
    "OCit_AftTrans",     // 76
    "OCit_Belt",         // 77
    "OCit_Deck",         // 78
    "OCit_FwdTrans",     // 79
    // 80-83: Rudder
    "RudderAft",  // 80
    "RudderFwd",  // 81
    "RudderSide", // 82
    "RudderTop",  // 83
    // 84-90: Superstructure casemate / Superstructure
    "SSC_AftTrans",   // 84
    "SSCasemate",     // 85
    "SSC_ConstrSide", // 86
    "SSC_Deck",       // 87
    "SSC_FwdTrans",   // 88
    "SS_Side",        // 89
    "SS_Top",         // 90
    // 91-96: Stern
    "St_Belt",       // 91
    "St_Bottom",     // 92
    "St_ConstrSide", // 93
    "St_Deck",       // 94
    "St_Inclin",     // 95
    "St_Trans",      // 96
    // 97-106: Turret generic / hull generic
    "TurretBarbette",     // 97
    "TurretBarbette2",    // 98
    "TurretDown",         // 99
    "TurretFwd",          // 100
    "Bulge",              // 101
    "Trans",              // 102
    "Deck",               // 103
    "Belt",               // 104
    "Dual_Cit_SSC_Bulge", // 105
    "Inclin",             // 106
    // 107-110: SS/Bridge, Casemate bottom
    "SS_BridgeTop",    // 107
    "SS_BridgeSide",   // 108
    "SS_BridgeBottom", // 109
    "Cas_Bottom",      // 110
    // 111-133: Zone sub-face materials (Side/Deck/Trans/Inclin per zone)
    "SideCit",     // 111
    "DeckCit",     // 112
    "TransCit",    // 113
    "InclinCit",   // 114
    "SideCas",     // 115
    "DeckCas",     // 116
    "TransCas",    // 117
    "InclinCas",   // 118
    "SideSSC",     // 119
    "DeckSSC",     // 120
    "TransSSC",    // 121
    "InclinSSC",   // 122
    "SideBow",     // 123
    "DeckBow",     // 124
    "TransBow",    // 125
    "InclinBow",   // 126
    "SideStern",   // 127
    "DeckStern",   // 128
    "TransStern",  // 129
    "InclinStern", // 130
    "SideSS",      // 131
    "DeckSS",      // 132
    "TransSS",     // 133
    // 134-153: Turret barbettes (GkBar) for turrets 1-20
    "Tur1GkBar",  // 134
    "Tur2GkBar",  // 135
    "Tur3GkBar",  // 136
    "Tur4GkBar",  // 137
    "Tur5GkBar",  // 138
    "Tur6GkBar",  // 139
    "Tur7GkBar",  // 140
    "Tur8GkBar",  // 141
    "Tur9GkBar",  // 142
    "Tur10GkBar", // 143
    "Tur11GkBar", // 144
    "Tur12GkBar", // 145
    "Tur13GkBar", // 146
    "Tur14GkBar", // 147
    "Tur15GkBar", // 148
    "Tur16GkBar", // 149
    "Tur17GkBar", // 150
    "Tur18GkBar", // 151
    "Tur19GkBar", // 152
    "Tur20GkBar", // 153
    // 154-173: Dual-zone transitions (Cas/SSC/Bow/St/SS combinations)
    "Dual_Cas_Bow_Trans",  // 154
    "Dual_Cas_Bow_Deck",   // 155
    "Dual_Cas_St_Trans",   // 156
    "Dual_Cas_St_Deck",    // 157
    "Dual_Cas_SSC_Deck",   // 158
    "Dual_Cas_SSC_Trans",  // 159
    "Dual_Cas_SS_Deck",    // 160
    "Dual_Cas_SS_Trans",   // 161
    "Dual_SSC_Bow_Trans",  // 162
    "Dual_SSC_Bow_Deck",   // 163
    "Dual_SSC_St_Trans",   // 164
    "Dual_SSC_St_Deck",    // 165
    "Dual_SSC_SS_Deck",    // 166
    "Dual_SSC_SS_Trans",   // 167
    "Dual_Bow_SS_Deck",    // 168
    "Dual_Bow_SS_Trans",   // 169
    "Dual_St_SS_Deck",     // 170
    "Dual_St_SS_Trans",    // 171
    "Dual_Cit_Bow_Bottom", // 172
    "Dual_Cit_St_Bottom",  // 173
    // 174-193: Turret undersides (GkDown) for turrets 1-20
    "Tur1GkDown",  // 174
    "Tur2GkDown",  // 175
    "Tur3GkDown",  // 176
    "Tur4GkDown",  // 177
    "Tur5GkDown",  // 178
    "Tur6GkDown",  // 179
    "Tur7GkDown",  // 180
    "Tur8GkDown",  // 181
    "Tur9GkDown",  // 182
    "Tur10GkDown", // 183
    "Tur11GkDown", // 184
    "Tur12GkDown", // 185
    "Tur13GkDown", // 186
    "Tur14GkDown", // 187
    "Tur15GkDown", // 188
    "Tur16GkDown", // 189
    "Tur17GkDown", // 190
    "Tur18GkDown", // 191
    "Tur19GkDown", // 192
    "Tur20GkDown", // 193
    // 194-213: Dual same-zone / cross-zone combinations
    "Dual_Cit_Cit_Deck",       // 194
    "Dual_Cit_Cit_Inclin",     // 195
    "Dual_Cit_Cit_Trans",      // 196
    "Dual_Cit_Cit_Side",       // 197
    "Dual_Cas_Cas_Belt",       // 198
    "Dual_Cas_Cas_Deck",       // 199
    "Dual_SSC_SSC_ConstrSide", // 200
    "Dual_SSC_SSC_Deck",       // 201
    "Dual_Bow_Bow_Deck",       // 202
    "Dual_Bow_Bow_ConstrSide", // 203
    "Dual_St_St_Deck",         // 204
    "Dual_St_St_ConstrSide",   // 205
    "Dual_SS_SS_Top",          // 206
    "Dual_SS_SS_Side",         // 207
    "Dual_Cit_Bow_ArtDeck",    // 208
    "Dual_Cit_St_ArtDeck",     // 209
    "Dual_Cas_Bow_Side",       // 210
    "Dual_Cas_St_Side",        // 211
    "Dual_Cit_Cas_Side",       // 212
    "Dual_Cit_SSC_Side",       // 213
    // 214-233: Turret tops (GkTop) for turrets 1-20
    "Tur1GkTop",  // 214
    "Tur2GkTop",  // 215
    "Tur3GkTop",  // 216
    "Tur4GkTop",  // 217
    "Tur5GkTop",  // 218
    "Tur6GkTop",  // 219
    "Tur7GkTop",  // 220
    "Tur8GkTop",  // 221
    "Tur9GkTop",  // 222
    "Tur10GkTop", // 223
    "Tur11GkTop", // 224
    "Tur12GkTop", // 225
    "Tur13GkTop", // 226
    "Tur14GkTop", // 227
    "Tur15GkTop", // 228
    "Tur16GkTop", // 229
    "Tur17GkTop", // 230
    "Tur18GkTop", // 231
    "Tur19GkTop", // 232
    "Tur20GkTop", // 233
    // 234-241: Hangar/forecastle deck, steering gear barbette
    "Cas_Hang",      // 234
    "Cas_Fdck",      // 235
    "SSC_Fdck",      // 236
    "SSC_Hang",      // 237
    "SS_SGBarbette", // 238
    "SS_SGDown",     // 239
    "SGBarbetteSS",  // 240
    "SGDownSS",      // 241
    // 242-254: Dual Citadel zone transitions
    "Dual_Cit_Cas_Deck",   // 242
    "Dual_Cit_Cas_Inclin", // 243
    "Dual_Cit_Cas_Trans",  // 244
    "Dual_Cit_SSC_Deck",   // 245
    "Dual_Cit_SSC_Inclin", // 246
    "Dual_Cit_SSC_Trans",  // 247
    "Dual_Cit_Bow_Trans",  // 248
    "Dual_Cit_Bow_Inclin", // 249
    "Dual_Cit_Bow_Deck",   // 250
    "Dual_Cit_St_Trans",   // 251
    "Dual_Cit_St_Inclin",  // 252
    "Dual_Cit_St_Deck",    // 253
    "Dual_Cit_SS_Deck",    // 254
];

/// Look up the collision material name for a given material ID.
///
/// Logs a warning for unknown IDs, indicating the game's material table has been
/// extended and our hardcoded copy needs updating.
pub fn collision_material_name(id: u8) -> &'static str {
    use std::sync::Mutex;
    static WARNED: Mutex<[bool; 256]> = Mutex::new([false; 256]);

    let idx = id as usize;
    if idx < COLLISION_MATERIAL_NAMES.len() {
        COLLISION_MATERIAL_NAMES[idx]
    } else {
        let mut warned = WARNED.lock().unwrap();
        if !warned[idx] {
            warned[idx] = true;
            eprintln!(
                "BUG: collision material ID {id} is beyond the known table (max {}). \
                 The game's material table has likely been updated.",
                COLLISION_MATERIAL_NAMES.len() - 1
            );
        }
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_armor_plates() {
        // Citadel belt, turret barbette, turret face all classify (Cit/Tur prefixes).
        assert!(armor_type_classifies("Cit_Belt"));
        assert!(armor_type_classifies("Tur1GkBar"));
        assert!(armor_type_classifies("TurretFwd"));
        assert!(armor_type_classifies("Belt"));
        assert!(armor_type_classifies("SS_Side"));
    }

    #[test]
    fn excludes_unclassified_plates() {
        // Rudder/bridge/kingpost/funnel have no armor-type prefix -> excluded.
        assert!(!armor_type_classifies("RudderSide"));
        assert!(!armor_type_classifies("BridgeSide"));
        assert!(!armor_type_classifies("KdpSide"));
        assert!(!armor_type_classifies("FunnelSide"));
        assert!(!armor_type_classifies("common"));
        assert!(!armor_type_classifies("zero"));
    }

    #[test]
    fn classifies_dual_via_split_tokens() {
        // Dual_Cit_Belt: tokens [Cit, Belt]; Cit matches CITADEL, Belt matches OUTER.
        assert!(armor_type_classifies("Dual_Cit_Cas_Belt"));
        // Dual_OCit_OCit_Side: tokens [OCit, OCit]; OCit matches OUTER prefix.
        assert!(armor_type_classifies("Dual_OCit_OCit_Side"));
    }

    #[test]
    fn table_covers_full_id_range() {
        assert_eq!(COLLISION_MATERIAL_NAMES.len(), 255);
        assert_eq!(collision_material_name(61), "Cit_Belt");
        assert_eq!(collision_material_name(134), "Tur1GkBar");
        assert_eq!(collision_material_name(100), "TurretFwd");
    }
}
