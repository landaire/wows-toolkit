use serde_json::Value;
use serde_json::json;

use super::*;

/// A cache root private to one test.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("wows-query-raw-params-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the test cache root");

        Self(dir)
    }

    fn kind_dir(&self) -> PathBuf {
        self.0.join(raw_params_kind())
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sample() -> Vec<(String, Value)> {
    let turret = json!({"numBarrels": 3, "rotationSpeed": [4.5, 25.0], "ammoList": ["PAPA001", "PAPA002"]});

    vec![
        (
            "PASC020_Des_Moines_1948".to_owned(),
            json!({
                "id": 4273911792u64,
                "index": "PASC020",
                "level": 10,
                "typeinfo": {"type": "Ship", "species": "Cruiser", "nation": "USA"},
                "HP_AGM_1": turret.clone(),
                "HP_AGM_2": turret.clone(),
                "HP_AGM_3": turret,
                "floats": [0.1, -0.0, 1e21, 1e-7, 5.0, 268435456.5],
                "ints": [0, -1, 268435455, 268435456, -268435456, -268435457, 9007199254740993i64, i64::MIN],
                "empty": {"array": [], "object": {}},
                "text": "Grosser Kurf\u{fc}rst \"quoted\" \\ \n\u{1}",
                "flags": [true, false, null],
            }),
        ),
        ("PAPA001_AP".to_owned(), json!({"id": 1, "index": "PAPA001", "typeinfo": {"type": "Projectile"}})),
        ("Plain".to_owned(), json!([1, [2, [3, []]]])),
    ]
}

fn build(root: &TempRoot, build: u32) -> Arc<RawParams> {
    RawParams::build(&root.kind_dir(), build, sample()).expect("the store builds")
}

#[test]
fn entries_read_back_as_the_json_they_were_built_from() {
    let root = TempRoot::new("round-trip");
    let store = build(&root, 7);

    let texts: Vec<(String, Vec<u8>)> = store
        .entries()
        .map(|entry| (entry.name().to_owned(), store.entry_json(&entry).expect("the entry reads")))
        .collect();
    let expected: Vec<(String, Vec<u8>)> =
        sample().into_iter().map(|(name, value)| (name, serde_json::to_vec(&value).unwrap())).collect();

    assert_eq!(texts, expected);
}

#[test]
fn entries_are_found_by_name_index_and_id() {
    let root = TempRoot::new("find");
    let store = build(&root, 7);

    let name = |key: &str| store.find(key).map(|entry| entry.name());
    assert_eq!(name("PASC020_Des_Moines_1948"), Some("PASC020_Des_Moines_1948"));
    assert_eq!(name("pasc020_des_moines_1948"), Some("PASC020_Des_Moines_1948"));
    assert_eq!(name(" pasc020 "), Some("PASC020_Des_Moines_1948"));
    assert_eq!(name("4273911792"), Some("PASC020_Des_Moines_1948"));
    assert_eq!(name("1"), Some("PAPA001_AP"));
    assert_eq!(name("PASC021"), None);

    let ship = store.find("PASC020").unwrap();
    assert_eq!(
        (ship.kind(), ship.species(), ship.nation(), ship.level(), ship.id().map(GameParamId::raw)),
        (Some("Ship"), Some("Cruiser"), Some("USA"), Some(10), Some(4273911792))
    );
    let plain = store.find("Plain").unwrap();
    assert_eq!((plain.index(), plain.kind(), plain.level(), plain.id()), (None, None, None, None));
}

#[test]
fn repeated_subtrees_are_stored_once() {
    let root = TempRoot::new("dedup");
    let store = build(&root, 7);

    // Three equal turrets share one object, its two arrays and their strings.
    let tables = store.tables();
    let ship = store.find("PASC020").unwrap();
    let format::Value::Object(node) = tables.value(ship.root()).unwrap() else {
        panic!("the ship is an object");
    };
    let turrets: Vec<u32> = (0..node.slot_count() / 2)
        .filter(|field| tables.string(StringId(node.slot(2 * field))).unwrap().starts_with("HP_AGM_"))
        .map(|field| node.slot(2 * field + 1))
        .collect();

    assert_eq!(turrets.len(), 3);
    assert!(turrets.iter().all(|turret| *turret == turrets[0]), "{turrets:?}");
}

#[test]
fn a_store_of_another_format_version_is_rebuilt() {
    let root = TempRoot::new("version");
    let store = build(&root, 7);
    let path = store.path().to_path_buf();
    drop(store);

    let mut bytes = std::fs::read(&path).unwrap();
    bytes[4..8].copy_from_slice(&(FORMAT_VERSION - 1).to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();

    let dir = path.parent().unwrap();
    assert!(matches!(
        RawParams::open(dir, 7),
        Err(RawParamsError::FormatVersion { found, .. }) if found == FORMAT_VERSION - 1
    ));

    let mut rebuilt = false;
    let store = RawParams::open_or_build_from(&root.0, 7, || {
        rebuilt = true;
        Ok(sample())
    })
    .expect("the store is rebuilt");
    assert!(rebuilt);
    assert_eq!(store.len(), 3);
    assert!(raw_params_kind().ends_with(&format!("-v{FORMAT_VERSION}")));
}

#[test]
fn a_store_of_another_build_is_refused() {
    let root = TempRoot::new("build");
    let store = build(&root, 7);
    let dir = store.path().parent().unwrap().to_path_buf();
    drop(store);

    assert!(matches!(RawParams::open(&dir, 8), Err(RawParamsError::WrongBuild { expected: 8, found: 7, .. })));
}

#[test]
fn damaged_stores_report_damage_instead_of_misreading() {
    let root = TempRoot::new("damage");
    let store = build(&root, 7);
    let path = store.path().to_path_buf();
    let len = store.file_len();
    let slots = store.layout.range(Section::Slots);
    drop(store);
    let original = std::fs::read(&path).unwrap();

    // Cut short: a section ends past the end of the file.
    std::fs::write(&path, &original[..len - 8]).unwrap();
    assert!(matches!(RawParams::open_file(&path), Err(RawParamsError::Damaged { damage: StoreDamage::Section, .. })));

    // Every slot of every node points at the newest node, which no node
    // precedes; reads must refuse rather than loop.
    let mut bytes = original.clone();
    let newest = ValueRef::new(format::Tag::Array, u32::MAX >> 4).unwrap();
    for slot in bytes[slots].chunks_exact_mut(4) {
        slot.copy_from_slice(&newest.0.to_le_bytes());
    }
    std::fs::write(&path, &bytes).unwrap();
    let store = RawParams::open_file(&path).expect("headers and entries are intact");
    for entry in store.entries() {
        assert!(
            matches!(store.entry_json(&entry), Err(RawParamsError::DamagedEntry { .. })),
            "{} read despite its damage",
            entry.name()
        );
    }
}
