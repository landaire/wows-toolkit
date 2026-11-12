use wowsunpack::export::model_catalog::CatalogLinks;
use wowsunpack::export::model_catalog::ModelCatalog;

use super::*;
use crate::model_tree::ModelBuildings;
use crate::sidebar_tree::fixture;

struct Data {
    ships: ShipCatalog,
    index: ModelTreeIndex,
    parts: HashMap<VehicleParamIndex, ShipPartRows>,
    fortifications: Vec<FortificationRow>,
    hidden: HashMap<String, HiddenReason>,
}

fn data() -> Data {
    rust_i18n::set_locale("en");
    let ships = fixture::ships();
    let catalog = fixture::catalog();
    let references = fixture::references(&ships);
    let index = ModelTreeIndex::build(&catalog, &ships, &references, fixture::model_buildings(), &fixture::metadata());
    let parts = fixture::ship_parts(&catalog, &references);

    Data { ships, index, parts, fortifications: Vec::new(), hidden: HashMap::new() }
}

/// A catalogue far larger than a search opens: 3000 main battery guns in
/// each of two nations.
fn large_data() -> Data {
    rust_i18n::set_locale("en");
    let ships = fixture::ships();
    let guns = [("usa", "AGM"), ("japan", "JGM")].into_iter().flat_map(|(nation, prefix)| {
        (0..3000).map(move |idx| {
            fixture::path(&format!(
                "content/gameplay/{nation}/gun/main/{prefix}{idx:04}_Gun/{prefix}{idx:04}_Gun.model"
            ))
        })
    });
    let catalog = ModelCatalog::from_paths(guns, &CatalogLinks::default());
    let references = crate::model_tree::ModelReferences::new();
    let index = ModelTreeIndex::build(&catalog, &ships, &references, ModelBuildings::default(), &fixture::metadata());

    Data { ships, index, parts: HashMap::new(), fortifications: Vec::new(), hidden: HashMap::new() }
}

fn build(data: &Data, query: &str, set: &mut HashSet<RowId>, auto: bool) -> BuiltTree {
    build_in_tiers(data, query, TierRange::ALL, set, auto)
}

fn build_in_tiers(data: &Data, query: &str, tiers: TierRange, set: &mut HashSet<RowId>, auto: bool) -> BuiltTree {
    let sources = TreeSources {
        ships: &data.ships,
        ship_parts: &data.parts,
        models: ModelSection::Ready(&data.index),
        fortifications: &data.fortifications,
        hidden: &data.hidden,
    };
    let search = SearchMatches::from_query(&sources, &FilterQuery::new(query), tiers);

    build_tree(&sources, search.as_ref(), &mut Expansion { set, closed: &HashSet::new(), auto })
}

#[test]
fn armored_buildings_are_listed_by_kind_after_the_ships_and_found_by_name() {
    let data = Data { fortifications: fixture::fortifications(), ..data() };
    let mut set = HashSet::from(["armor-fortifications".into(), "armor-fortifications/Complex".into()]);

    let built = build(&data, "", &mut set, false);
    assert_eq!(
        outline(&built.items)[..5],
        ["Japan +", "U.S.A. +", "Fortifications", "  Fort", "    Fort of the Isles"],
    );
    assert_eq!(
        built.rows.get(&building_row_id(GameParamId::from(4229999600u64))),
        Some(&RowKind::Building {
            id: GameParamId::from(4229999600u64),
            display_name: "Fort of the Isles".to_string(),
            param_name: "PCBX001".to_string(),
        })
    );
    assert_eq!(
        built.unavailable.get(&building_row_id(GameParamId::from(fixture::FORT_OF_THE_ISLES))),
        Some(&GeometryAvailability::Absent),
        "a building whose hull the install lacks is marked"
    );

    assert_eq!(
        search(&data, "pcbx001"),
        [
            "Fortifications (1)",
            "  Fort (1)",
            "    Fort of the Isles",
            "Models",
            "Buildings (1)",
            "  Complex (1)",
            "    LGB001_Fort +"
        ],
        "a building is found by its GameParams name"
    );
    assert_eq!(
        search(&data, "pcbc002")[..2],
        ["Models", "Buildings (1)"],
        "a building armored only through its guns is not a fortification"
    );

    let built = build_in_tiers(&data, "", TierRange::new(1, 5), &mut set, false);
    assert_eq!(
        outline(&built.items)[1..4],
        ["Fortifications", "  Fort", "    Fort of the Isles"],
        "the tiers neither filter nor count buildings"
    );
}

#[test]
fn hidden_ships_list_in_their_nation_and_class_tagged_with_why_they_are_hidden() {
    let listed = data();
    let hidden = listed.ships.hidden.iter().map(|ship| (ship.ship.param_index.clone(), ship.reason)).collect();
    let data = Data { ships: listed.ships.with_hidden(), hidden, ..listed };
    let mut set = HashSet::from(["armor-nation-USA".into(), "armor-class-USA-Battleship".into()]);

    let built = build(&data, "", &mut set, false);

    assert_eq!(
        outline(&built.items)[..6],
        ["Japan +", "U.S.A.", "  Battleship", "    III S. Carolina", "    IX Iowa +", "    IX [Iowa]"]
    );
    let rental = ship_row_id("PASB708");
    assert_eq!(built.notes.get(&rental).map(|note| note.to_string()).as_deref(), Some("Clan rental"));
    assert!(data.ships.hidden.is_empty(), "nothing is left hidden");
}

#[test]
fn a_tier_range_lists_only_the_ships_in_it_and_every_model() {
    let data = data();
    let mut set = HashSet::from(["armor-nation-USA".into(), "armor-class-USA-Battleship".into()]);

    let built = build_in_tiers(&data, "", TierRange::new(9, 11), &mut set, false);

    assert_eq!(
        outline(&built.items),
        [
            "Japan (1) +",
            "U.S.A. (1)",
            "  Battleship (1)",
            "    IX Iowa +",
            "Models",
            "events +",
            "Japan +",
            "U.S.A. +",
            "Buildings +",
            "Locations +",
            "Ports +"
        ],
        "South Carolina, tier III, is left out"
    );

    let built = build_in_tiers(&data, "", TierRange::new(1, 5), &mut set, false);
    assert_eq!(outline(&built.items)[..3], ["U.S.A. (1)", "  Battleship (1)", "    III S. Carolina"]);
}

#[test]
fn a_tier_range_and_a_search_apply_together() {
    let data = data();

    let built = build_in_tiers(&data, "carolina", TierRange::new(9, 11), &mut HashSet::new(), true);
    assert_eq!(outline(&built.items), ["Models"], "the ship the search names is outside the tiers");

    let built = build_in_tiers(&data, "carolina", TierRange::new(3, 3), &mut HashSet::new(), true);
    assert_eq!(outline(&built.items), ["U.S.A. (1)", "  Battleship (1)", "    III S. Carolina", "Models"]);

    let built = build_in_tiers(&data, "agm034", TierRange::new(10, 11), &mut HashSet::new(), true);
    assert_eq!(
        outline(&built.items),
        ["Models", "U.S.A. (1)", "  Guns (1)", "    Main battery (1)", "      AGM034_16in50_Mk7 +"],
        "Iowa, which mounts the turret, is outside the tiers; the turret model is still listed"
    );
}

/// One line per row, indented by depth; a collapsed folder ends in `+`.
fn outline(items: &[SidebarItem]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending: Vec<(&SidebarItem, usize)> = items.iter().rev().map(|item| (item, 0)).collect();
    while let Some((item, depth)) = pending.pop() {
        let marker = if item.children == ItemChildren::Collapsed { " +" } else { "" };
        lines.push(format!("{}{}{marker}", "  ".repeat(depth), item.label));
        if let ItemChildren::Expanded(children) = &item.children {
            pending.extend(children.iter().rev().map(|child| (child, depth + 1)));
        }
    }

    lines
}

fn collapsed_folders(items: &[SidebarItem]) -> Vec<RowId> {
    let mut collapsed = Vec::new();
    let mut pending: Vec<&SidebarItem> = items.iter().collect();
    while let Some(item) = pending.pop() {
        match &item.children {
            ItemChildren::Collapsed => collapsed.push(item.id.clone()),
            ItemChildren::Expanded(children) => pending.extend(children),
            ItemChildren::Leaf => {}
        }
    }

    collapsed
}

fn expand_all(data: &Data) -> BuiltTree {
    let mut set = HashSet::new();
    loop {
        let built = build(data, "", &mut set, false);
        let collapsed = collapsed_folders(&built.items);
        if collapsed.is_empty() {
            return built;
        }
        set.extend(collapsed);
    }
}

fn search(data: &Data, query: &str) -> Vec<String> {
    outline(&build(data, query, &mut HashSet::new(), true).items)
}

#[test]
fn folders_start_collapsed_with_ships_above_the_models_heading() {
    let data = data();

    let built = build(&data, "", &mut HashSet::new(), true);

    assert_eq!(
        outline(&built.items),
        ["Japan +", "U.S.A. +", "Models", "events +", "Japan +", "U.S.A. +", "Buildings +", "Locations +", "Ports +"]
    );
    assert_eq!(built.rows.get(SEPARATOR_ID), Some(&RowKind::Separator));
}

#[test]
fn a_model_the_install_lacks_stays_listed_and_is_marked_wherever_it_is_listed() {
    let data = data();
    let turret = model_row_id(&fixture::path(fixture::YAMATO_TURRET));
    let secondary = model_row_id(&fixture::path(fixture::SECONDARY));
    let mount = part_row_id(
        &VehicleParamIndex::new(fixture::IOWA_INDEX),
        &ShipPartKey::Mount(wowsunpack::export::hardpoint::HardpointName::new("HP_AGS_1")),
    );

    let built = expand_all(&data);

    assert!(built.rows.contains_key(&turret), "the row is listed");
    let expected = HashMap::from([
        (turret, GeometryAvailability::Absent),
        (secondary, GeometryAvailability::NoGeometry),
        (mount, GeometryAvailability::NoGeometry),
    ]);
    assert_eq!(built.unavailable, expected, "the model, and the ship part showing it, are marked");
}

#[test]
fn a_skin_row_is_tagged_as_a_skin() {
    let data = data();
    let skin = model_row_id(&fixture::path(fixture::IOWA_SKIN));

    let built = expand_all(&data);

    assert_eq!(built.notes.get(&skin).map(|note| note.to_string()).as_deref(), Some("Skin"));
    assert_eq!(built.notes.len(), 1, "outside a search only the skin row carries a note");
}

#[test]
fn a_search_folds_accents_case_and_punctuation_away() {
    use crate::catalog::ClassGroup;
    use crate::catalog::NationGroup;
    use wowsunpack::game_params::types::Species;

    let smaland = ShipEntry {
        param_index: "PWSD110".to_string(),
        display_name: "Sm\u{e5}land".to_string(),
        filter_fields: ["Sm\u{e5}land", "PWSD110"].map(str::to_string).into_iter().collect(),
        tier: 10,
    };
    let europe = NationGroup {
        nation: "Europe".to_string(),
        display_name: "Europe".to_string(),
        classes: vec![ClassGroup { species: Species::Destroyer, ships: vec![smaland] }],
    };
    let data = Data { ships: ShipCatalog { nations: vec![europe], hidden: Vec::new() }, ..data() };

    let built = build(&data, "SMALAND", &mut HashSet::new(), true);
    assert!(built.rows.contains_key(&ship_row_id("PWSD110")), "the ring on the a folds away");

    let built = build(&data, "406mm50", &mut HashSet::new(), true);
    let turret = model_row_id(&fixture::path(fixture::IOWA_TURRET));
    assert!(built.rows.contains_key(&turret), "the gun's name is found without its spaces and slash");
    assert_eq!(built.notes.get(&turret).map(|note| note.to_string()).as_deref(), Some("406 mm/50 Mk.7 in a turret"));
}

#[test]
fn a_search_finds_a_building_model_by_the_building_param_using_it() {
    let data = data();
    let fort = model_row_id(&fixture::path(fixture::FORT));

    let built = build(&data, "pcbx022", &mut HashSet::new(), true);

    assert_eq!(outline(&built.items), ["Models", "Buildings (1)", "  Complex (1)", "    LGB001_Fort +"]);
    assert_eq!(built.notes.get(&fort).map(|note| note.to_string()).as_deref(), Some("PCBX022_PCGM002"));
}

#[test]
fn a_building_model_lists_its_buildings_which_share_its_geometry_and_are_found_by_name_and_id() {
    let fort = fixture::path(fixture::FORT);
    let catalog = fixture::catalog().with_geometry(HashMap::from([(fort.clone(), GeometryAvailability::Absent)]));
    let ships = fixture::ships();
    let references = fixture::references(&ships);
    let buildings = fixture::model_buildings();
    let index = ModelTreeIndex::build(&catalog, &ships, &references, buildings, &fixture::metadata());
    let data = Data { index, ..data() };
    let battery = GameParamId::from(fixture::COASTAL_BATTERY);
    let battery_row = model_building_row_id(&fort, battery);

    let built = expand_all(&data);
    let lines = outline(&built.items);
    let at = lines.iter().position(|line| line.trim() == "LGB001_Fort").expect("the fort model is listed");
    assert_eq!(
        lines[at + 1..at + 3].iter().map(|line| line.trim()).collect::<Vec<_>>(),
        ["Fort of the Isles", "PCBC002"]
    );
    assert_eq!(
        built.rows.get(&battery_row),
        Some(&RowKind::Building {
            id: battery,
            display_name: "PCBC002".to_string(),
            param_name: "PCBC002".to_string()
        })
    );
    assert_eq!(built.unavailable.get(&battery_row), Some(&GeometryAvailability::Absent), "the hull is missing");
    assert_eq!(data.index.building_hulls.get(&battery), Some(&fort), "an open building finds its row");

    for query in ["fort of the isles", "4229999601"] {
        let built = build(&data, query, &mut HashSet::from([model_row_id(&fort)]), true);
        assert!(built.rows.contains_key(&battery_row), "{query} lists the buildings under the model");
    }
}

#[test]
fn ships_list_their_parts_and_models_nest_by_nation_kind_and_sub_kind() {
    let data = data();

    let built = expand_all(&data);

    let expected = [
        "Japan",
        "  Battleship",
        "    X Yamato",
        "U.S.A.",
        "  Battleship",
        "    III S. Carolina",
        "    IX Iowa",
        "      Hull",
        "        ASB028_Iowa_1945",
        "        ASB028_Iowa_1945_Bow",
        "        ASB028_Iowa_1945_MidBack",
        "        ASB028_Iowa_1945_MidFront",
        "        ASB028_Iowa_1945_Stern",
        "      Main battery",
        "        HP_AGM_1 AGM034_16in50_Mk7",
        "        HP_AGM_2 AGM034_16in50_Mk7",
        "      Secondaries",
        "        HP_AGS_1 AGS010_5in38_Mk32_Mod12",
        "      Directors",
        "        HP_AD_1 AD001_Director_Mk37",
        "      Misc",
        "        Bow MP_AM2061_1 AM2061",
        "Models",
        "events",
        "  Directors",
        "    XD006_Lazarus_Director",
        "Japan",
        "  Ships",
        "    Battleships",
        "      JSB018_Yamato_1944",
        "  Guns",
        "    Main battery",
        "      JGM178_460mm_45_Type94",
        "U.S.A.",
        "  Ships",
        "    Battleships",
        "      ASB028_Iowa_1945",
        "        Bow",
        "        MidBack",
        "        MidFront",
        "        Stern",
        "        ASB128_Iowa_Halloween",
        "          Wreck",
        "  Guns",
        "    Main battery",
        "      AGM034_16in50_Mk7",
        "        Wreck",
        "    Secondary battery",
        "      AGS010_5in38_Mk32_Mod12",
        "  Directors",
        "    AD001_Director_Mk37",
        "  Miscellaneous",
        "    Deck fittings",
        "      AM116_Boat_Winch_2",
        "    Other",
        "      AM2061",
        "Buildings",
        "  Complex",
        "    LGB001_Fort",
        "      Fort of the Isles",
        "      PCBC002",
        "Locations",
        "  building",
        "    city",
        "      LBC001_House",
        "Ports",
        "  misc",
        "    wharf",
        "      PMW001_Crane",
    ];
    assert_eq!(outline(&built.items), expected);

    let turret = fixture::path(fixture::IOWA_TURRET);
    let mount = built.rows.get(&part_row_id(
        &VehicleParamIndex::new(fixture::IOWA_INDEX),
        &ShipPartKey::Mount(wowsunpack::export::hardpoint::HardpointName::new("HP_AGM_1")),
    ));
    assert!(
        matches!(mount, Some(RowKind::ShipPart { model, .. }) if *model == turret),
        "the mount row opens the turret model"
    );
}

#[test]
fn a_search_filters_ships_and_models_and_opens_the_folders_holding_matches() {
    let data = data();

    assert_eq!(
        search(&data, "iowa"),
        [
            "U.S.A. (1)",
            "  Battleship (1)",
            "    IX Iowa +",
            "Models",
            "U.S.A. (1)",
            "  Ships (1)",
            "    Battleships (1)",
            "      ASB028_Iowa_1945 +",
        ]
    );
    assert_eq!(
        search(&data, "yamato"),
        [
            "Japan (1)",
            "  Battleship (1)",
            "    X Yamato",
            "Models",
            "Japan (1)",
            "  Ships (1)",
            "    Battleships (1)",
            "      JSB018_Yamato_1944"
        ]
    );
}

#[test]
fn a_search_matches_model_names_and_opens_a_model_to_a_matching_variant() {
    let data = data();

    assert_eq!(search(&data, "lazarus"), ["Models", "events (1)", "  Directors (1)", "    XD006_Lazarus_Director"]);
    assert_eq!(
        search(&data, "mk7_dead"),
        ["Models", "U.S.A. (1)", "  Guns (1)", "    Main battery (1)", "      AGM034_16in50_Mk7", "        Wreck"]
    );
    assert_eq!(
        search(&data, "stern"),
        [
            "U.S.A. (1)",
            "  Battleship (1)",
            "    IX Iowa",
            "      Hull",
            "        ASB028_Iowa_1945_Stern",
            "Models",
            "U.S.A. (1)",
            "  Ships (1)",
            "    Battleships (1)",
            "      ASB028_Iowa_1945",
            "        Stern",
        ]
    );
    assert_eq!(
        search(&data, "halloween_dead"),
        [
            "Models",
            "U.S.A. (1)",
            "  Ships (1)",
            "    Battleships (1)",
            "      ASB028_Iowa_1945",
            "        ASB128_Iowa_Halloween",
            "          Wreck",
        ]
    );
}

#[test]
fn a_search_naming_only_directories_matches_no_model() {
    let data = data();

    for query in ["content", "gameplay", "events/director", "gun/main", "battleship/asb028"] {
        assert_eq!(search(&data, query), ["Models"], "{query}");
    }
}

fn note(built: &BuiltTree, id: &RowId) -> Option<String> {
    built.notes.get(id).map(|note| note.to_string())
}

#[test]
fn a_search_finds_ships_and_their_hull_models_by_game_params_index_name_and_id() {
    let data = data();
    let iowa = ship_row_id(fixture::IOWA_INDEX);
    let iowa_model = model_row_id(&fixture::path(fixture::IOWA));

    for (query, field) in [("pasb018", "PASB018"), ("iowa_1944", "PASB018_Iowa_1944"), ("4276041712", "4276041712")] {
        let built = build(&data, query, &mut HashSet::new(), true);

        assert_eq!(
            outline(&built.items),
            [
                "U.S.A. (1)",
                "  Battleship (1)",
                "    IX Iowa +",
                "Models",
                "U.S.A. (1)",
                "  Ships (1)",
                "    Battleships (1)",
                "      ASB028_Iowa_1945 +",
            ],
            "{query}"
        );
        assert_eq!(note(&built, &iowa).as_deref(), Some(field), "{query} names the field it matched");
        assert_eq!(note(&built, &iowa_model).as_deref(), Some(field), "{query} on the hull model");
    }
}

#[test]
fn a_search_finds_a_ship_by_its_full_and_its_short_name() {
    let data = data();
    let south_carolina = ship_row_id("PASB001");

    for query in ["south carolina", "s. carolina"] {
        let built = build(&data, query, &mut HashSet::new(), true);

        assert_eq!(
            outline(&built.items),
            ["U.S.A. (1)", "  Battleship (1)", "    III S. Carolina", "Models"],
            "{query}"
        );
    }

    let built = build(&data, "south carolina", &mut HashSet::new(), true);
    assert_eq!(note(&built, &south_carolina).as_deref(), Some("South Carolina"), "the full name is noted");
    let built = build(&data, "s. carolina", &mut HashSet::new(), true);
    assert_eq!(note(&built, &south_carolina), None, "the short name is the row's own");
}

#[test]
fn a_search_finds_models_and_ship_parts_by_stem_and_by_the_mount_using_them() {
    let data = data();
    let turret = model_row_id(&fixture::path(fixture::IOWA_TURRET));
    let iowa = VehicleParamIndex::new(fixture::IOWA_INDEX);
    let mount = part_row_id(&iowa, &ShipPartKey::Mount(wowsunpack::export::hardpoint::HardpointName::new("HP_AGM_1")));
    let turret_rows = [
        "U.S.A. (1)",
        "  Battleship (1)",
        "    IX Iowa",
        "      Main battery",
        "        HP_AGM_1 AGM034_16in50_Mk7",
        "        HP_AGM_2 AGM034_16in50_Mk7",
        "Models",
        "U.S.A. (1)",
        "  Guns (1)",
        "    Main battery (1)",
        "      AGM034_16in50_Mk7 +",
    ];

    for (query, field) in [
        ("agm034", None),
        ("406 mm/50", Some("406 mm/50 Mk.7 in a turret")),
        ("pagm034", Some("PAGM034")),
        ("16in50_mk7", None),
        ("4258916336", Some("4258916336")),
    ] {
        let built = build(&data, query, &mut HashSet::new(), true);

        assert_eq!(outline(&built.items), turret_rows, "{query}");
        assert_eq!(note(&built, &turret).as_deref(), field, "{query} on the model");
        assert_eq!(note(&built, &mount).as_deref(), field, "{query} on the part");
    }

    assert_eq!(
        search(&data, "pad001_director"),
        [
            "U.S.A. (1)",
            "  Battleship (1)",
            "    IX Iowa",
            "      Directors",
            "        HP_AD_1 AD001_Director_Mk37",
            "Models",
            "U.S.A. (1)",
            "  Directors (1)",
            "    AD001_Director_Mk37",
        ],
        "a mount the game gives no name is found by its GameParams name"
    );
}

#[test]
fn a_broad_search_leaves_its_folders_closed_with_their_counts() {
    let data = large_data();
    let mut set = HashSet::new();

    let built = build(&data, "gun", &mut set, true);

    assert_eq!(outline(&built.items), ["Models", "Japan (3000) +", "U.S.A. (3000) +"]);
    assert_eq!(built.rows.len(), 3, "the heading and two nations");
    assert!(set.is_empty(), "nothing was opened for the search");

    set.insert("model-folder/nation-usa".into());
    let built = build(&data, "gun", &mut set, false);

    assert_eq!(
        outline(&built.items),
        ["Models", "Japan (3000) +", "U.S.A. (3000)", "  Guns (3000) +"],
        "opening a folder lists only its own children"
    );
}

/// The models no nation owns are listed after the nations, under a name
/// of their own rather than their directory's.
#[test]
fn the_common_models_follow_the_nations_under_their_own_name() {
    rust_i18n::set_locale("en");
    let ships = fixture::ships();
    let guns = ["common", "usa", "japan"]
        .into_iter()
        .map(|nation| fixture::path(&format!("content/gameplay/{nation}/gun/main/AGM0001_Gun/AGM0001_Gun.model")));
    let catalog = ModelCatalog::from_paths(guns, &CatalogLinks::default());
    let references = crate::model_tree::ModelReferences::new();
    let index = ModelTreeIndex::build(&catalog, &ships, &references, ModelBuildings::default(), &fixture::metadata());
    let data = Data { ships, index, parts: HashMap::new(), fortifications: Vec::new(), hidden: HashMap::new() };

    let built = build(&data, "gun", &mut HashSet::new(), false);

    assert_eq!(outline(&built.items), ["Models", "Japan (1) +", "U.S.A. (1) +", "Common models (1) +"]);
}

#[test]
fn a_narrow_search_in_a_large_catalogue_opens_the_folders_holding_its_matches() {
    let data = large_data();

    assert_eq!(
        search(&data, "agm0042_"),
        ["Models", "U.S.A. (1)", "  Guns (1)", "    Main battery (1)", "      AGM0042_Gun"]
    );
}

#[test]
fn a_folder_closed_during_a_search_stays_closed_until_the_search_changes() {
    let data = data();
    let mut set = HashSet::new();
    build(&data, "agm034", &mut set, true);
    set.remove("armor-nation-USA");
    set.remove("model-folder/nation-usa");

    let built = build(&data, "agm034", &mut set, false);

    assert_eq!(outline(&built.items), ["U.S.A. (1) +", "Models", "U.S.A. (1) +"]);
}

/// The real catalogue and every ship's parts. Run the test binary with
/// `WOWS_DIR=<install> <binary> --ignored real_catalogue`.
#[test]
#[ignore = "needs a game install named by WOWS_DIR"]
fn the_real_catalogue_lists_every_model_and_searches_quickly() {
    use std::sync::Arc;
    use std::time::Instant;

    use wowsunpack::export::model_library::ModelLibrary;
    use wowsunpack::game_data::AssetsBin;
    use wowsunpack::game_params::provider::GameMetadataProvider;

    use crate::model_tree::ModelReferences;
    use crate::model_tree::list_all_ship_parts;

    let Some(dir) = std::env::var_os("WOWS_DIR").map(std::path::PathBuf::from) else {
        eprintln!("skipped: WOWS_DIR is not set");
        return;
    };
    let vfs = wowsunpack::game_data::build_game_vfs(&dir, AssetsBin::Omit).expect("game vfs");
    let mut bytes = Vec::new();
    let mut file = vfs.join("content/assets.bin").expect("a path").open_file().expect("assets.bin opens");
    std::io::Read::read_to_end(&mut file, &mut bytes).expect("assets.bin reads");
    let metadata = GameMetadataProvider::from_vfs(&vfs).expect("GameParams");
    let latest_build = std::fs::read_dir(dir.join("bin"))
        .expect("bin lists builds")
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .max()
        .expect("a build");
    let texts = dir.join(format!("bin/{latest_build}/res/texts/en/LC_MESSAGES/global.mo"));
    let catalog = gettext::Catalog::parse(std::fs::File::open(texts).expect("global.mo opens")).expect("global.mo");
    metadata.set_translations(catalog);
    let library = ModelLibrary::new(vfs.clone(), Arc::new(bytes), &metadata).expect("model library");
    let ships = ShipCatalog::build(&metadata);
    eprintln!("[measured] {} listed ships, {} hidden", ships.ships().count(), ships.hidden.len());
    for hidden in ships.hidden.iter().take(3) {
        eprintln!(
            "  {:?} {} {} ({:?})",
            hidden.reason, hidden.ship.param_index, hidden.ship.display_name, hidden.species
        );
    }

    let started = Instant::now();
    let references = ModelReferences::build(&ships, &metadata);
    eprintln!("[measured] model references in {:?}", started.elapsed());

    let started = Instant::now();
    let buildings = wowsunpack::export::building_armor::BuildingArmorIndex::build(&metadata, &library);
    let fortifications = crate::model_tree::list_fortifications(&buildings, library.catalog(), &metadata);
    let model_buildings = crate::model_tree::list_model_buildings(&buildings, &metadata);
    eprintln!(
        "[measured] {} buildings, {} fortifications in {:?}",
        buildings.buildings().len(),
        fortifications.len(),
        started.elapsed()
    );
    for row in fortifications.iter().take(4) {
        eprintln!("  {:?} {:?} {} ({})", row.kind, row.kind_name, row.display_name, row.param_name);
    }

    let started = Instant::now();
    let index = ModelTreeIndex::build(library.catalog(), &ships, &references, model_buildings, &metadata);
    eprintln!("[measured] index of {} models in {:?}", index.model_count(), started.elapsed());
    assert_eq!(index.model_count(), library.catalog().listed_entries().count());

    let started = Instant::now();
    let parts = list_all_ship_parts(&ships, &metadata, &library, &references);
    let ship_count = ships.ships().count();
    eprintln!("[measured] parts of {}/{ship_count} ships in {:?}", parts.len(), started.elapsed());
    assert!(parts.len() * 10 >= ship_count * 9, "most ships list their parts");

    for folder in &index.folders {
        eprintln!("  {} -> {:?}", folder.key, folder.name);
    }
    let mut skins: Vec<_> = index.skins.iter().collect();
    skins.sort_by(|a, b| a.0.cmp(b.0));
    eprintln!("[measured] {} skin models", skins.len());
    for (skin, uses) in skins.iter().take(3) {
        eprintln!("  {} <- {uses:?}", skin.stem());
    }
    assert!(!skins.is_empty(), "exteriors swap in skin models");
    for directory in ["usa", "japan", "uk", "panasia", "panamerica", "commonwealth"] {
        let key = format!("nation-{directory}");
        let nation = index.folders.iter().find(|folder| folder.key == key);
        let flagged = nation.is_some_and(|nation| matches!(&nation.name, FolderName::Nation { flag: Some(_), .. }));
        assert!(flagged, "{directory} has a flag");
    }

    let mut folders: Vec<&ModelFolder> = index.folders.iter().collect();
    let mut under_models = Vec::new();
    while let Some(folder) = folders.pop() {
        folders.extend(folder.folders.iter());
        under_models.extend(folder.models.iter().flat_map(|model| model.buildings.iter().map(move |b| (model, b))));
    }
    eprintln!("[measured] {} buildings under their models", under_models.len());
    for (model, building) in under_models.iter().take(4) {
        eprintln!("  {} > {} ({}, {})", model.path, building.display_name, building.param_name, building.id);
    }
    assert!(!under_models.is_empty(), "building models list their GameParams buildings");
    let hulls = buildings.buildings().iter().filter(|building| building.model().is_some()).count();
    assert_eq!(index.building_hulls.len(), hulls, "every building with a hull model is listed under it");

    let data = Data { ships, index, parts, fortifications, hidden: HashMap::new() };
    let queries = [
        "",
        "a",
        "gun",
        "iowa",
        "agm",
        "pasb018",
        "south carolina",
        "406 mm/50",
        "pagm034",
        "4276041712",
        "smaland",
        "bow patch",
    ];
    for query in queries {
        let sources = TreeSources {
            ships: &data.ships,
            ship_parts: &data.parts,
            models: ModelSection::Ready(&data.index),
            fortifications: &data.fortifications,
            hidden: &data.hidden,
        };
        let started = Instant::now();
        let matches = SearchMatches::from_query(&sources, &FilterQuery::new(query), TierRange::ALL);
        eprintln!("[measured] {query:?}: matched in {:?}", started.elapsed());
        drop(matches);

        let started = Instant::now();
        let built = build(&data, query, &mut HashSet::new(), true);
        eprintln!("[measured] {query:?}: {} rows built in {:?}", built.rows.len(), started.elapsed());
        let mut notes: Vec<String> = built.notes.iter().map(|(id, note)| format!("{id} [{note}]")).collect();
        notes.sort();
        for note in notes.iter().take(6) {
            eprintln!("    {note}");
        }
    }
    for query in ["a", "gun"] {
        let built = build(&data, query, &mut HashSet::new(), true);
        assert!(built.rows.len() < 100, "{query:?} builds {} rows", built.rows.len());
    }

    // What each keystroke of a typed search costs, matching and building.
    let mut typed = String::new();
    let mut slowest = std::time::Duration::ZERO;
    for letter in "montana".chars() {
        typed.push(letter);
        let started = Instant::now();
        let built = build(&data, &typed, &mut HashSet::new(), true);
        let spent = started.elapsed();
        slowest = slowest.max(spent);
        eprintln!("[measured] keystroke {typed:?}: {} rows in {spent:?}", built.rows.len());
    }
    eprintln!("[measured] slowest keystroke {slowest:?}");
}

#[test]
fn a_row_tooltip_names_the_whole_row_and_what_it_opens() {
    use wowsunpack::export::hardpoint::HardpointName;

    let turret = fixture::path(fixture::IOWA_TURRET);
    let ship = RowKind::Ship { param_index: fixture::IOWA_INDEX.to_string(), display_name: "Iowa".to_string() };
    let part = RowKind::ShipPart {
        ship: VehicleParamIndex::new(fixture::IOWA_INDEX),
        part: ShipPartKey::Mount(HardpointName::new("HP_AGM_1")),
        model: turret.clone(),
        node: Some("HP_AGM_1".to_string()),
    };

    assert_eq!(row_tooltip(&ship, &[], None).as_deref(), Some("Iowa\nPASB018"));
    assert_eq!(
        row_tooltip(&RowKind::Model(turret.clone()), &[], None).as_deref(),
        Some(format!("AGM034_16in50_Mk7\n{}", fixture::IOWA_TURRET).as_str())
    );
    assert_eq!(
        row_tooltip(&part, &[], None).as_deref(),
        Some(format!("HP_AGM_1 AGM034_16in50_Mk7\n{}", fixture::IOWA_TURRET).as_str())
    );
    assert_eq!(row_tooltip(&RowKind::PartFolder, &[], None), None, "a folder shows its whole name already");

    rust_i18n::set_locale("en");
    assert_eq!(
        row_tooltip(&RowKind::Model(turret), &[], Some(GeometryAvailability::Absent)).as_deref(),
        Some(format!("AGM034_16in50_Mk7\n{}\nNot in your game files", fixture::IOWA_TURRET).as_str()),
        "a model the install lacks says so"
    );
}

#[test]
fn a_skin_row_tooltip_names_the_model_it_replaces_and_the_exteriors() {
    rust_i18n::set_locale("en");
    let catalog = fixture::catalog();
    let ships = fixture::ships();
    let references = fixture::references(&ships);
    let index = ModelTreeIndex::build(&catalog, &ships, &references, ModelBuildings::default(), &fixture::metadata());
    let skin = fixture::path(fixture::IOWA_SKIN);
    let uses = index.skins.get(&skin).expect("the skin's exteriors are known");

    let tooltip = row_tooltip(&RowKind::Model(skin.clone()), uses, None);

    let expected = format!(
        "ASB128_Iowa_Halloween\n{}\nReplaces ASB028_Iowa_1945 when Halloween is equipped\nReplaces ASB028_Iowa_1945 when Spooky is equipped",
        fixture::IOWA_SKIN
    );
    assert_eq!(tooltip.as_deref(), Some(expected.as_str()));
}
