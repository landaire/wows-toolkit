use std::collections::HashMap;

use serde::Serialize;
use wowsunpack::rpc::typedefs::ArgType;
use wowsunpack::rpc::typedefs::ArgValue;
use wowsunpack::rpc::typedefs::PrimitiveType;

/// Build a zero/empty default value for a property type. Used when a nested
/// property update targets a property that was left uninitialized at entity
/// create (e.g. the avatar's `privateVehicleState`, which is materialized only
/// once the first ribbon/buff arrives); the default gives the update a value to
/// walk into instead of the packet being dropped.
pub(crate) fn default_arg_value<'argtype>(arg_type: &'argtype ArgType) -> ArgValue<'argtype> {
    match arg_type {
        ArgType::Primitive(p) => match p {
            PrimitiveType::Uint8 => ArgValue::Uint8(0),
            PrimitiveType::Uint16 => ArgValue::Uint16(0),
            PrimitiveType::Uint32 => ArgValue::Uint32(0),
            PrimitiveType::Uint64 => ArgValue::Uint64(0),
            PrimitiveType::Int8 => ArgValue::Int8(0),
            PrimitiveType::Int16 => ArgValue::Int16(0),
            PrimitiveType::Int32 => ArgValue::Int32(0),
            PrimitiveType::Int64 => ArgValue::Int64(0),
            PrimitiveType::Float32 => ArgValue::Float32(0.0),
            PrimitiveType::Float64 => ArgValue::Float64(0.0),
            PrimitiveType::Vector2 => ArgValue::Vector2((0.0, 0.0)),
            PrimitiveType::Vector3 => ArgValue::Vector3((0.0, 0.0, 0.0)),
            PrimitiveType::String => ArgValue::String(Vec::new()),
            PrimitiveType::UnicodeString => ArgValue::UnicodeString(Vec::new()),
            PrimitiveType::Blob => ArgValue::Blob(Vec::new()),
        },
        // Fixed-size arrays materialize their slots so element-set updates land;
        // variable arrays start empty and grow via add/extend updates.
        ArgType::Array((size, element_type)) => {
            let n = size.unwrap_or(0);
            ArgValue::Array((0..n).map(|_| default_arg_value(element_type)).collect())
        }
        ArgType::FixedDict((allow_none, properties)) => {
            let map: HashMap<&'argtype str, ArgValue<'argtype>> =
                properties.iter().map(|p| (p.name.as_str(), default_arg_value(&p.prop_type))).collect();
            // A nullable dict defaults to a materialized `Some(defaults)`, not
            // `None`: this value only exists because an update is about to write
            // into it, and the navigation helper has no path that materializes a
            // `None` mid-walk. The arriving update overwrites the real fields.
            if *allow_none { ArgValue::NullableFixedDict(Some(map)) } else { ArgValue::FixedDict(map) }
        }
        ArgType::Tuple((element_type, size)) => {
            ArgValue::Tuple((0..*size).map(|_| default_arg_value(element_type)).collect())
        }
        ArgType::Named { inner, .. } => default_arg_value(inner),
    }
}

/// MSB-first bit reader over a byte slice.
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    bit_offset: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, bit_offset: 0 }
    }

    /// Read `count` bits (0–8) and return them as a u8, MSB-first.
    pub fn read_u8(&mut self, count: u8) -> u8 {
        debug_assert!(count <= 8);
        if count == 0 {
            return 0;
        }
        let mut result: u8 = 0;
        for _ in 0..count {
            let byte_idx = self.bit_offset / 8;
            let bit_idx = 7 - (self.bit_offset % 8); // MSB-first
            result = (result << 1) | ((self.data[byte_idx] >> bit_idx) & 1);
            self.bit_offset += 1;
        }
        result
    }

    /// Read full bytes into `buf`, advancing the bit offset.
    /// The current bit position must be byte-aligned.
    pub fn read_u8_slice(&mut self, buf: &mut [u8]) {
        debug_assert!(self.bit_offset.is_multiple_of(8));
        let byte_offset = self.bit_offset / 8;
        buf.copy_from_slice(&self.data[byte_offset..byte_offset + buf.len()]);
        self.bit_offset += buf.len() * 8;
    }

    /// Number of bits remaining.
    pub fn remaining(&self) -> usize {
        self.data.len() * 8 - self.bit_offset
    }
}

#[derive(Debug, Serialize)]
pub enum PropertyNestLevel<'argtype> {
    ArrayIndex(usize),
    DictKey(&'argtype str),
}

#[derive(Debug, Serialize)]
pub enum UpdateAction<'argtype> {
    SetKey { key: &'argtype str, value: ArgValue<'argtype> },
    SetRange { start: usize, stop: usize, values: Vec<ArgValue<'argtype>> },
    SetElement { index: usize, value: ArgValue<'argtype> },
    RemoveRange { start: usize, stop: usize },
}

#[derive(Debug, Serialize)]
pub struct PropertyNesting<'argtype> {
    pub levels: Vec<PropertyNestLevel<'argtype>>,
    pub action: UpdateAction<'argtype>,
}

/// This function emulates Python's slice semantics
fn slice_insert<T>(idx1: usize, idx2: usize, target: &mut Vec<T>, mut source: Vec<T>) {
    // First we delete target[idx1..idx2]
    for _ in idx1..idx2 {
        if target.len() <= idx1 {
            break;
        }
        target.remove(idx1);
    }

    // Then we insert source[..] into target[idx1] repeatedly
    for (i, elem) in source.drain(..).enumerate() {
        target.insert(std::cmp::min(idx1 + i, target.len()), elem);
    }
}

fn nested_update_command<'argtype>(
    is_slice: bool,
    t: &'argtype ArgType,
    mut prop_value: &mut ArgValue<'argtype>,
    mut reader: BitReader,
) -> PropertyNesting<'argtype> {
    let t = t.peeled();
    match (t, &mut prop_value) {
        (ArgType::FixedDict((_, entries)), _) => {
            let entry_idx = reader.read_u8(entries.len().next_power_of_two().trailing_zeros() as u8);
            while !reader.remaining().is_multiple_of(8) {
                reader.read_u8(1);
            }
            let mut remaining = vec![0; reader.remaining() / 8];
            reader.read_u8_slice(&mut remaining[..]);
            assert!(reader.remaining() == 0);
            let value = entries[entry_idx as usize].prop_type.parse_value(&mut &remaining[..]).unwrap();
            match prop_value {
                ArgValue::FixedDict(d) => {
                    d.insert(&entries[entry_idx as usize].name, value.clone());
                }
                ArgValue::NullableFixedDict(Some(d)) => {
                    d.insert(&entries[entry_idx as usize].name, value.clone());
                }
                ArgValue::NullableFixedDict(None) => unimplemented!(),
                _ => panic!("FixedDict type caused unexpected value {:?}", prop_value),
            }
            PropertyNesting {
                levels: vec![],
                action: UpdateAction::SetKey { key: &entries[entry_idx as usize].name, value },
            }
        }
        (ArgType::Array((_size, element_type)), ArgValue::Array(elements)) => {
            let idx_bits =
                if is_slice { elements.len() + 1 } else { elements.len() }.next_power_of_two().trailing_zeros();
            let idx1 = reader.read_u8(idx_bits as u8);
            let idx2 = if is_slice { Some(reader.read_u8(idx_bits as u8)) } else { None };

            while !reader.remaining().is_multiple_of(8) {
                reader.read_u8(1);
            }
            let mut remaining = vec![0; reader.remaining() / 8];
            reader.read_u8_slice(&mut remaining[..]);

            if remaining.is_empty() {
                // Remove elements
                if is_slice {
                    slice_insert(idx1 as usize, idx2.unwrap() as usize, elements, vec![]);
                    return PropertyNesting {
                        levels: vec![],
                        action: UpdateAction::RemoveRange { start: idx1 as usize, stop: idx2.unwrap() as usize },
                    };
                } else {
                    unimplemented!();
                }
            }

            let mut new_elements = vec![];
            let mut i = &remaining[..];
            while !i.is_empty() {
                let element = element_type.parse_value(&mut i).unwrap();
                new_elements.push(element);
            }

            if is_slice {
                slice_insert(idx1 as usize, idx2.unwrap() as usize, elements, new_elements.clone());
                PropertyNesting {
                    levels: vec![],
                    action: UpdateAction::SetRange {
                        start: idx1 as usize,
                        stop: idx2.unwrap() as usize,
                        values: new_elements,
                    },
                }
            } else {
                elements[idx1 as usize] = new_elements.remove(0);
                PropertyNesting {
                    levels: vec![],
                    action: UpdateAction::SetElement { index: idx1 as usize, value: elements[idx1 as usize].clone() },
                }
            }
        }
        x => {
            println!("{:#?}", x);
            panic!();
        }
    }
}

pub(crate) fn get_nested_prop_path_helper<'argtype>(
    is_slice: bool,
    t: &'argtype ArgType,
    prop_value: &mut ArgValue<'argtype>,
    mut reader: BitReader,
) -> PropertyNesting<'argtype> {
    let t = t.peeled();
    let cont = reader.read_u8(1);
    if cont == 0 {
        return nested_update_command(is_slice, t, prop_value, reader);
    }
    match (t, prop_value) {
        (ArgType::FixedDict((_, propspec)), ArgValue::FixedDict(propvalue)) => {
            let prop_idx = reader.read_u8(propspec.len().next_power_of_two().trailing_zeros() as u8);
            let prop_id = &propspec[prop_idx as usize].name;
            let mut nesting = get_nested_prop_path_helper(
                is_slice,
                &propspec[prop_idx as usize].prop_type,
                propvalue.get_mut(prop_id.as_str()).unwrap(),
                reader,
            );
            nesting.levels.insert(0, PropertyNestLevel::DictKey(&propspec[prop_idx as usize].name));
            nesting
        }
        (ArgType::FixedDict((_, propspec)), ArgValue::NullableFixedDict(Some(propvalue))) => {
            let prop_idx = reader.read_u8(propspec.len().next_power_of_two().trailing_zeros() as u8);
            let prop_id = &propspec[prop_idx as usize].name;
            let mut nesting = get_nested_prop_path_helper(
                is_slice,
                &propspec[prop_idx as usize].prop_type,
                propvalue.get_mut(prop_id.as_str()).unwrap(),
                reader,
            );
            nesting.levels.insert(0, PropertyNestLevel::DictKey(&propspec[prop_idx as usize].name));
            nesting
        }
        (ArgType::Array((_size, element_type)), ArgValue::Array(arr)) => {
            let idx = reader.read_u8(arr.len().next_power_of_two().trailing_zeros() as u8) as usize;
            // A property that was default-constructed (uninitialized at entity
            // create, materialized lazily) can have an empty array here; grow it
            // with defaults so navigating to element `idx` applies the update
            // instead of panicking on an out-of-bounds index.
            if idx >= arr.len() {
                arr.resize_with(idx + 1, || default_arg_value(element_type));
            }
            let mut nesting = get_nested_prop_path_helper(is_slice, element_type, &mut arr[idx], reader);
            nesting.levels.insert(0, PropertyNestLevel::ArrayIndex(idx));
            nesting
        }
        x => {
            println!("{:#?}", x);
            panic!()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn slice_insert_single_into_empty() {
        let mut v: Vec<u32> = vec![];
        slice_insert(0, 0, &mut v, vec![5]);
        assert_eq!(v, vec![5]);
    }

    #[test]
    fn multi_into_empty() {
        let mut v: Vec<u32> = vec![];
        slice_insert(2, 5, &mut v, vec![5, 6, 7, 8]);
        assert_eq!(v, vec![5, 6, 7, 8]);
    }

    #[test]
    fn replace_mid_single() {
        let mut v: Vec<u32> = vec![1, 2, 3, 4, 5];
        slice_insert(2, 3, &mut v, vec![6]);
        assert_eq!(v, vec![1, 2, 6, 4, 5]);
    }

    #[test]
    fn insert_mid() {
        let mut v: Vec<u32> = vec![1, 2, 3, 4, 5];
        slice_insert(2, 2, &mut v, vec![6]);
        assert_eq!(v, vec![1, 2, 6, 3, 4, 5]);
    }

    #[test]
    fn insert_mid_partial_replace() {
        let mut v: Vec<u32> = vec![1, 2, 3, 4, 5];
        slice_insert(2, 4, &mut v, vec![6, 7, 8]);
        assert_eq!(v, vec![1, 2, 6, 7, 8, 5]);
    }

    #[test]
    fn shrink_mid_with_replace() {
        let mut v: Vec<u32> = vec![1, 2, 3, 4, 5];
        slice_insert(2, 4, &mut v, vec![6]);
        assert_eq!(v, vec![1, 2, 6, 5]);
    }

    #[test]
    fn append() {
        let mut v: Vec<u32> = vec![1, 2, 3, 4, 5];
        slice_insert(5, 12, &mut v, vec![6, 7, 8]);
        assert_eq!(v, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn nested_update_peels_named_wrapper() {
        use std::collections::HashMap;
        use wowsunpack::rpc::typedefs::FixedDictProperty;
        use wowsunpack::rpc::typedefs::PrimitiveType;

        // A FIXED_DICT referenced through an alias arrives wrapped in Named.
        let t = ArgType::Named {
            name: "FOO".to_string(),
            inner: Box::new(ArgType::FixedDict((
                false,
                vec![FixedDictProperty { name: "x".to_string(), prop_type: ArgType::Primitive(PrimitiveType::Uint8) }],
            ))),
        };

        let key: &str = match &t {
            ArgType::Named { inner, .. } => match inner.as_ref() {
                ArgType::FixedDict((_, props)) => &props[0].name,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        let mut map = HashMap::new();
        map.insert(key, ArgValue::Uint8(0));
        let mut value = ArgValue::FixedDict(map);

        // 1-entry dict => 0 index bits; the single payload byte is the new u8.
        let data = [5u8];
        let nesting = nested_update_command(false, &t, &mut value, BitReader::new(&data));

        match nesting.action {
            UpdateAction::SetKey { key, value } => {
                assert_eq!(key, "x");
                assert_eq!(value, ArgValue::Uint8(5));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }
}
