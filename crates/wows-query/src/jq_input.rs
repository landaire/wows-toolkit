//! jaq values of stored GameParams entries. A node becomes one jaq value,
//! built once per evaluation and shared wherever the node occurs, so the
//! repeated subtrees of GameParams cost one conversion and one allocation.

use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::hash::Hasher;

use jaq_json::Map;
use jaq_json::Num;
use jaq_json::Rc;
use jaq_json::Val;

use crate::error::StoreDamage;
use crate::raw_params::Node;
use crate::raw_params::StringId;
use crate::raw_params::Tables;
use crate::raw_params::Value;
use crate::raw_params::ValueRef;

/// Builds jaq values from one store, remembering every node and string it
/// built.
pub(crate) struct Inputs<'a> {
    tables: Tables<'a>,
    /// Keyed by value word: an array and an object with equal slots share a
    /// node but not a value.
    nodes: HashMap<ValueRef, Val, BuildHasherDefault<IdHasher>>,
    strings: HashMap<StringId, Val, BuildHasherDefault<IdHasher>>,
}

impl<'a> Inputs<'a> {
    pub(crate) fn new(tables: Tables<'a>) -> Self {
        Self { tables, nodes: HashMap::default(), strings: HashMap::default() }
    }

    /// The jaq value of `root`. Walks with an explicit stack, since values
    /// nest deeper than is safe to recurse.
    pub(crate) fn value(&mut self, root: ValueRef) -> Result<Val, StoreDamage> {
        struct Frame<'a> {
            node: Node<'a>,
            word: ValueRef,
            object: bool,
            next: usize,
            items: Vec<Val>,
        }

        let mut frames: Vec<Frame<'a>> = Vec::new();
        let mut pending = Some((self.tables.value(root)?, root));
        loop {
            if let Some((value, word)) = pending.take() {
                let built = match value {
                    Value::Array(node) | Value::Object(node) => match self.nodes.get(&word) {
                        Some(built) => Some(built.clone()),
                        None => {
                            let object = matches!(value, Value::Object(_));
                            let items = Vec::with_capacity(node.slot_count());
                            frames.push(Frame { node, word, object, next: 0, items });
                            None
                        }
                    },
                    scalar => Some(self.scalar(scalar, word)),
                };
                if let Some(built) = built {
                    match frames.last_mut() {
                        Some(frame) => frame.items.push(built),
                        None => return Ok(built),
                    }
                }
            }

            let Some(frame) = frames.last_mut() else {
                unreachable!("a node frame is open until the root is returned");
            };
            if frame.next < frame.node.slot_count() {
                if frame.object {
                    let key = StringId(frame.node.slot(frame.next));
                    let key = self.string(key)?;
                    frame.items.push(key);
                    frame.next += 1;
                }
                let child = ValueRef(frame.node.slot(frame.next));
                frame.next += 1;
                pending = Some((self.tables.child(frame.node.id(), child)?, child));
                continue;
            }

            let Some(frame) = frames.pop() else {
                unreachable!("the frame just inspected");
            };
            let built = if frame.object {
                let mut map = Map::with_capacity_and_hasher(frame.items.len() / 2, Default::default());
                let mut items = frame.items.into_iter();
                while let (Some(key), Some(value)) = (items.next(), items.next()) {
                    map.insert(key, value);
                }
                Val::Obj(Rc::new(map))
            } else {
                Val::Arr(Rc::new(frame.items))
            };
            self.nodes.insert(frame.word, built.clone());
            match frames.last_mut() {
                Some(parent) => parent.items.push(built),
                None => return Ok(built),
            }
        }
    }

    fn scalar(&mut self, value: Value<'_>, word: ValueRef) -> Val {
        match value {
            Value::Null => Val::Null,
            Value::Bool(value) => Val::Bool(value),
            Value::Int(value) => Val::Num(Num::from_integral(value)),
            Value::Float(value) => float(value),
            Value::Str(text) => {
                let id = StringId(word.payload());
                self.strings.entry(id).or_insert_with(|| Val::utf8_str(text.to_owned())).clone()
            }
            Value::Array(_) | Value::Object(_) => unreachable!("nodes are built by value"),
        }
    }

    fn string(&mut self, id: StringId) -> Result<Val, StoreDamage> {
        if let Some(built) = self.strings.get(&id) {
            return Ok(built.clone());
        }

        let built = Val::utf8_str(self.tables.string(id)?.to_owned());
        self.strings.insert(id, built.clone());

        Ok(built)
    }
}

/// The number jaq reads from the JSON text of `value`. jaq keeps a decimal
/// literal as its text, which prints as written; without an exponent that
/// text is what printing the f64 gives, but JSON writes `1e+21` where the
/// f64 prints `1e21`, so exponent forms keep their text.
fn float(value: f64) -> Val {
    // Shortest f64 text fits in 32 bytes; only exponent forms allocate.
    let mut buffer = std::io::Cursor::new([0u8; 32]);
    serde_json::to_writer(&mut buffer, &value).expect("stored floats are finite and short");
    let len = buffer.position() as usize;
    let text = &buffer.get_ref()[..len];
    if text.contains(&b'e') {
        let text = String::from_utf8(text.to_vec()).expect("JSON numbers are ASCII");
        return Val::Num(Num::Dec(Rc::new(text)));
    }

    Val::Num(Num::Float(value))
}

/// Hashes the dense ids of nodes and strings by one multiplication.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(*byte)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }

    fn write_u32(&mut self, value: u32) {
        self.0 = u64::from(value).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use std::time::Instant;

    use serde_json::json;

    use super::*;
    use crate::jq::InputOutputs;
    use crate::jq::Program;
    use crate::raw_params::RawParams;

    /// Filters whose results depend on every kind of value: traversal, key
    /// order, number and string printing, arithmetic, comparison, paths and
    /// updates.
    const FILTERS: &[&str] = &[
        ".",
        "keys",
        "keys_unsorted",
        "length",
        "type",
        "[.[]?]",
        "[..]",
        "[paths]",
        "[paths(scalars)]",
        "[paths as $path | [$path, getpath($path)]]",
        "tojson",
        "tostring",
        "@text",
        "@json",
        "to_entries?",
        "[.. | numbers]",
        "[.. | numbers] | add",
        "[.. | numbers | . * 3 - 1]",
        "[.. | numbers | floor, tostring]",
        "[.. | numbers] | sort | unique",
        "[.. | numbers] | (max, min)",
        "[.. | strings] | unique",
        "[.. | strings | ascii_downcase | explode | length]",
        "[.. | select(type == \"object\") | keys]",
        "[.. | arrays | length]",
        "[..] | group_by(type) | map(length)",
        "[..] | map(tojson) | sort",
        ". == .",
        "[., .] | unique | length",
        "walk(if type == \"number\" then . + 1 else . end)",
        "(.. | numbers) |= . * 2",
        "(.. | select(type == \"object\")) |= with_entries(.key |= ascii_upcase)",
        "del(.. | nulls)",
        "getpath([\"typeinfo\", \"type\"])?",
        ".typeinfo.type?, .index?, .id?",
        "select(.typeinfo.type? == \"Ship\") | .name?",
        "[.. | objects | select(has(\"ammoList\")) | .ammoList[]]",
        "reduce (.. | numbers) as $n (0; . + $n)",
        "[paths(type == \"number\")] | length",
        "contains({})? // false",
        ".[0]?, .[-1:]?",
        "error(\"stop\")",
        ".a.b.c",
    ];

    fn sample() -> Vec<(String, serde_json::Value)> {
        let turret = json!({"numBarrels": 3, "rotationSpeed": [4.5, 25.0], "ammoList": ["PAPA001", "PAPA002"]});

        vec![
            (
                "PASC020_Des_Moines_1948".to_owned(),
                json!({
                    "id": 4273911792u64,
                    "index": "PASC020",
                    "name": "PASC020_Des_Moines_1948",
                    "typeinfo": {"type": "Ship", "species": "Cruiser", "nation": "USA"},
                    "HP_AGM_1": turret.clone(),
                    "HP_AGM_2": turret.clone(),
                    "nested": {"turret": turret},
                    "floats": [0.1, -0.0, 1e21, 1e-7, 5.0, 268435456.5, 1.7976931348623157e308, 3.3333333333333335],
                    "ints": [0, -1, 268435455, 268435456, -268435456, -268435457, 9007199254740993i64, i64::MIN, i64::MAX],
                    "empty": {"array": [], "object": {}},
                    "text": "Grosser Kurf\u{fc}rst \"quoted\" \\ \n\u{1} \u{1F6A2}",
                    "flags": [true, false, null],
                    "zz": 1,
                    "aa": 2,
                }),
            ),
            ("Scalar".to_owned(), json!(-12.5)),
            ("Text".to_owned(), json!("abc")),
            ("Nested".to_owned(), json!([1, [2, [3, [], {}]], {"a": {"b": {"c": null}}}])),
        ]
    }

    /// Outputs of `program` on `input`, with limits no test reaches.
    fn run(program: &Program, input: Val) -> InputOutputs {
        let mut output_left = 1 << 30;
        let deadline = Instant::now() + Duration::from_secs(60);

        program.run(input, 1 << 30, deadline, &mut output_left).0
    }

    /// Compares every filter on each entry read from `store` with the same
    /// filter on the entry's JSON text, as jaq reads it.
    fn assert_same_results(store: &RawParams, entries: &[(String, serde_json::Value)]) {
        let programs: Vec<Program> =
            FILTERS.iter().map(|filter| Program::compile(filter).expect("the filter compiles")).collect();
        let mut inputs = Inputs::new(store.tables());
        let mut compared = 0;

        for ((name, value), entry) in entries.iter().zip(store.entries()) {
            assert_eq!(entry.name(), name);
            let text = serde_json::to_vec(value).unwrap();
            let parsed = jaq_json::read::parse_single(&text).expect("the entry is JSON");
            let stored = inputs.value(entry.root()).expect("the entry reads");

            for (filter, program) in FILTERS.iter().zip(&programs) {
                let expected = run(program, parsed.clone());
                let found = run(program, stored.clone());
                assert_eq!(
                    (&found.values, &found.error, found.output_bytes),
                    (&expected.values, &expected.error, expected.output_bytes),
                    "{filter} on {name}"
                );
                compared += 1;
            }
        }

        assert_eq!(compared, entries.len() * FILTERS.len());
    }

    fn build(name: &str, entries: Vec<(String, serde_json::Value)>) -> (PathBuf, Arc<RawParams>) {
        let dir = std::env::temp_dir().join(format!("wows-query-jq-input-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = RawParams::build(&dir, 1, entries).expect("the store builds");

        (dir, store)
    }

    #[test]
    fn stored_values_give_the_results_of_their_json() {
        let (dir, store) = build("sample", sample());

        assert_same_results(&store, &sample());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn repeated_nodes_are_built_once() {
        let (dir, store) = build("shared", sample());
        let mut inputs = Inputs::new(store.tables());
        let ship = store.find("PASC020").unwrap();
        let Val::Obj(fields) = inputs.value(ship.root()).unwrap() else {
            panic!("the ship is an object");
        };
        let field = |key: &str| fields.get(&Val::utf8_str(key.to_owned())).cloned().unwrap();
        let Val::Obj(nested) = field("nested") else {
            panic!("nested is an object");
        };

        let (Val::Obj(first), Val::Obj(second), Some(Val::Obj(third))) =
            (field("HP_AGM_1"), field("HP_AGM_2"), nested.get(&Val::utf8_str("turret".to_owned())).cloned())
        else {
            panic!("turrets are objects");
        };
        assert!(Rc::ptr_eq(&first, &second) && Rc::ptr_eq(&first, &third));

        let _ = std::fs::remove_dir_all(dir);
    }

    /// Every tenth entry of a real build, which covers every entry type.
    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn real_entries_give_the_results_of_their_json() {
        let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
            .expect("dump root has a readable builds.toml");
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let bytes = crate::gameparams::read_game_params(&root.join(&entry.dir)).expect("GameParams.data reads");
        let pickle = wowsunpack::game_params::convert::game_params_to_pickle(bytes).expect("GameParams decodes");
        let entries: Vec<(String, serde_json::Value)> = wowsunpack::game_params::convert::base_params_json(&pickle)
            .expect("GameParams has base params")
            .step_by(10)
            .collect();
        drop(pickle);
        let (dir, store) = build("real", entries.clone());

        assert_same_results(&store, &entries);

        let _ = std::fs::remove_dir_all(dir);
    }
}
