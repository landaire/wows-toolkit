use std::collections::HashMap;

use crate::error::PResult;
use crate::error::ParseError;
use crate::error::failure;
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
) -> PResult<PropertyNesting<'argtype>> {
    let t = t.peeled();
    match (t, &mut prop_value) {
        (ArgType::FixedDict((_, entries)), _) => {
            let entry = entries
                .get(reader.read_u8(entries.len().next_power_of_two().trailing_zeros() as u8) as usize)
                .ok_or_else(|| failure(ParseError::InvalidPacketData))?;
            while !reader.remaining().is_multiple_of(8) {
                reader.read_u8(1);
            }
            let mut remaining = vec![0; reader.remaining() / 8];
            reader.read_u8_slice(&mut remaining[..]);
            let value =
                entry.prop_type.parse_value(&mut &remaining[..]).map_err(|_| failure(ParseError::InvalidPacketData))?;
            match prop_value {
                ArgValue::FixedDict(d) => {
                    d.insert(&entry.name, value.clone());
                }
                ArgValue::NullableFixedDict(Some(d)) => {
                    d.insert(&entry.name, value.clone());
                }
                _ => return Err(failure(ParseError::InvalidPacketData)),
            }
            Ok(PropertyNesting { levels: vec![], action: UpdateAction::SetKey { key: &entry.name, value } })
        }
        (ArgType::Array((_size, element_type)), ArgValue::Array(elements)) => {
            let idx_bits =
                if is_slice { elements.len() + 1 } else { elements.len() }.next_power_of_two().trailing_zeros();
            let idx1 = reader.read_u8(idx_bits as u8) as usize;
            let idx2 = if is_slice { Some(reader.read_u8(idx_bits as u8) as usize) } else { None };

            while !reader.remaining().is_multiple_of(8) {
                reader.read_u8(1);
            }
            let mut remaining = vec![0; reader.remaining() / 8];
            reader.read_u8_slice(&mut remaining[..]);

            if remaining.is_empty() {
                // An empty payload removes the [idx1, idx2) range (slice form only).
                let stop = idx2.ok_or_else(|| failure(ParseError::InvalidPacketData))?;
                slice_insert(idx1, stop, elements, vec![]);
                return Ok(PropertyNesting { levels: vec![], action: UpdateAction::RemoveRange { start: idx1, stop } });
            }

            // A clean payload is exactly N whole elements. A leftover or non-
            // advancing parse means the bytes are misaligned, not benign padding,
            // so fail rather than fabricate a half-decoded element.
            let mut new_elements = vec![];
            let mut i = &remaining[..];
            while !i.is_empty() {
                let before = i.len();
                match element_type.parse_value(&mut i) {
                    Ok(element) if i.len() < before => new_elements.push(element),
                    _ => return Err(failure(ParseError::InvalidPacketData)),
                }
            }

            if let Some(stop) = idx2 {
                slice_insert(idx1, stop, elements, new_elements.clone());
                Ok(PropertyNesting {
                    levels: vec![],
                    action: UpdateAction::SetRange { start: idx1, stop, values: new_elements },
                })
            } else {
                let value = if new_elements.is_empty() {
                    return Err(failure(ParseError::InvalidPacketData));
                } else {
                    new_elements.remove(0)
                };
                // A non-slice element set can target one past the end (append) or
                // an array left empty at entity create; grow with defaults so the
                // assignment lands instead of indexing out of bounds.
                if idx1 >= elements.len() {
                    elements.resize_with(idx1 + 1, || default_arg_value(element_type));
                }
                elements[idx1] = value;
                Ok(PropertyNesting {
                    levels: vec![],
                    action: UpdateAction::SetElement { index: idx1, value: elements[idx1].clone() },
                })
            }
        }
        (_, _) => Err(failure(ParseError::InvalidPacketData)),
    }
}

/// Read a byte-aligned scalar value at the reader's current position. A scalar
/// leaf in a nested-property path carries no `cont` bit (only containers do), so
/// the parent navigation arm reads the value directly via this helper instead of
/// recursing into [`get_nested_prop_path_helper`], which would consume the
/// value's leading bits as a spurious continuation flag.
fn read_aligned_scalar<'argtype>(t: &'argtype ArgType, reader: &mut BitReader) -> PResult<ArgValue<'argtype>> {
    while !reader.remaining().is_multiple_of(8) {
        reader.read_u8(1);
    }
    let mut buf = vec![0; reader.remaining() / 8];
    reader.read_u8_slice(&mut buf[..]);
    t.parse_value(&mut &buf[..]).map_err(|_| failure(ParseError::InvalidPacketData))
}

pub(crate) fn get_nested_prop_path_helper<'argtype>(
    is_slice: bool,
    t: &'argtype ArgType,
    prop_value: &mut ArgValue<'argtype>,
    mut reader: BitReader,
) -> PResult<PropertyNesting<'argtype>> {
    let t = t.peeled();
    let cont = reader.read_u8(1);
    if cont == 0 {
        return nested_update_command(is_slice, t, prop_value, reader);
    }
    match (t, prop_value) {
        (ArgType::FixedDict((_, propspec)), ArgValue::FixedDict(propvalue)) => {
            let prop = propspec
                .get(reader.read_u8(propspec.len().next_power_of_two().trailing_zeros() as u8) as usize)
                .ok_or_else(|| failure(ParseError::InvalidPacketData))?;
            // A scalar child is a leaf with no further `cont` bit: set it here and
            // surface it as a SetKey at this dict, the same shape the `cont == 0`
            // path produces, rather than recursing (which would misread the value).
            if matches!(prop.prop_type.peeled(), ArgType::Primitive(_)) {
                let value = read_aligned_scalar(&prop.prop_type, &mut reader)?;
                propvalue.insert(prop.name.as_str(), value.clone());
                return Ok(PropertyNesting { levels: vec![], action: UpdateAction::SetKey { key: &prop.name, value } });
            }
            let child = propvalue.get_mut(prop.name.as_str()).ok_or_else(|| failure(ParseError::InvalidPacketData))?;
            let mut nesting = get_nested_prop_path_helper(is_slice, &prop.prop_type, child, reader)?;
            nesting.levels.insert(0, PropertyNestLevel::DictKey(&prop.name));
            Ok(nesting)
        }
        (ArgType::FixedDict((_, propspec)), ArgValue::NullableFixedDict(Some(propvalue))) => {
            let prop = propspec
                .get(reader.read_u8(propspec.len().next_power_of_two().trailing_zeros() as u8) as usize)
                .ok_or_else(|| failure(ParseError::InvalidPacketData))?;
            if matches!(prop.prop_type.peeled(), ArgType::Primitive(_)) {
                let value = read_aligned_scalar(&prop.prop_type, &mut reader)?;
                propvalue.insert(prop.name.as_str(), value.clone());
                return Ok(PropertyNesting { levels: vec![], action: UpdateAction::SetKey { key: &prop.name, value } });
            }
            let child = propvalue.get_mut(prop.name.as_str()).ok_or_else(|| failure(ParseError::InvalidPacketData))?;
            let mut nesting = get_nested_prop_path_helper(is_slice, &prop.prop_type, child, reader)?;
            nesting.levels.insert(0, PropertyNestLevel::DictKey(&prop.name));
            Ok(nesting)
        }
        (ArgType::Array((_size, element_type)), ArgValue::Array(arr)) => {
            let idx = reader.read_u8(arr.len().next_power_of_two().trailing_zeros() as u8) as usize;
            // A property that was default-constructed (uninitialized at entity
            // create, materialized lazily) can have an empty array here; grow it
            // with defaults so navigating to element `idx` applies the update
            // instead of indexing out of bounds.
            if idx >= arr.len() {
                arr.resize_with(idx + 1, || default_arg_value(element_type));
            }
            // A scalar element is a leaf with no `cont` bit (see the dict arm).
            if matches!(element_type.peeled(), ArgType::Primitive(_)) {
                let value = read_aligned_scalar(element_type, &mut reader)?;
                arr[idx] = value.clone();
                return Ok(PropertyNesting { levels: vec![], action: UpdateAction::SetElement { index: idx, value } });
            }
            let mut nesting = get_nested_prop_path_helper(is_slice, element_type, &mut arr[idx], reader)?;
            nesting.levels.insert(0, PropertyNestLevel::ArrayIndex(idx));
            Ok(nesting)
        }
        (_, _) => Err(failure(ParseError::InvalidPacketData)),
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
        let nesting = nested_update_command(false, &t, &mut value, BitReader::new(&data)).expect("parse");

        match nesting.action {
            UpdateAction::SetKey { key, value } => {
                assert_eq!(key, "x");
                assert_eq!(value, ArgValue::Uint8(5));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    use std::collections::HashMap;
    use wowsunpack::rpc::typedefs::FixedDictProperty;
    use wowsunpack::rpc::typedefs::PrimitiveType;

    fn named(name: &str, inner: ArgType) -> ArgType {
        ArgType::Named { name: name.to_string(), inner: Box::new(inner) }
    }

    fn prop(name: &str, prop_type: ArgType) -> FixedDictProperty {
        FixedDictProperty { name: name.to_string(), prop_type }
    }

    /// A nested update that descends into a scalar dict field carries no `cont`
    /// bit at the scalar; the value bytes follow directly. The leaf must surface
    /// as a SetKey at the owning dict, never panic on a spurious continuation.
    #[test]
    fn descend_into_scalar_dict_field_sets_key() {
        let t = ArgType::FixedDict((false, vec![prop("x", named("X", ArgType::Primitive(PrimitiveType::Int8)))]));
        let mut value = ArgValue::FixedDict(HashMap::from([("x", ArgValue::Int8(0))]));

        // cont=1 (descend), 0 prop-index bits (1 field), 7 align bits, then the i8.
        let data = [0b1000_0000u8, 7];
        let nesting = get_nested_prop_path_helper(false, &t, &mut value, BitReader::new(&data)).expect("parse");

        assert!(nesting.levels.is_empty(), "scalar leaf is a SetKey at its dict, not a nested level");
        match nesting.action {
            UpdateAction::SetKey { key, value } => {
                assert_eq!(key, "x");
                assert_eq!(value, ArgValue::Int8(7));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    /// The real modern-replay shape: `ribbons[0].ribbonId` arrives by descending
    /// dict -> array -> dict -> scalar. Expect the documented convention:
    /// levels stop at the array element, action is a SetKey for `ribbonId`.
    #[test]
    fn descend_ribbons_ribbon_id_matches_convention() {
        let ribbon_state = named(
            "RIBBON_STATE",
            ArgType::FixedDict((
                false,
                vec![
                    prop("ribbonId", named("RIBBON_ID", ArgType::Primitive(PrimitiveType::Int8))),
                    prop("count", ArgType::Primitive(PrimitiveType::Uint16)),
                ],
            )),
        );
        let t = ArgType::FixedDict((
            false,
            vec![prop("ribbons", named("RIBBONS_STATE", ArgType::Array((None, Box::new(ribbon_state)))))],
        ));
        let mut value = ArgValue::FixedDict(HashMap::from([(
            "ribbons",
            ArgValue::Array(vec![ArgValue::FixedDict(HashMap::from([
                ("ribbonId", ArgValue::Int8(0)),
                ("count", ArgValue::Uint16(0)),
            ]))]),
        )]));

        // cont(top)=1, cont(array)=1, cont(dict)=1, dict-prop-idx=0 (ribbonId),
        // 4 align bits, then the i8 value 3.  => 0b1110_0000, 0x03
        let data = [0b1110_0000u8, 3];
        let nesting = get_nested_prop_path_helper(false, &t, &mut value, BitReader::new(&data)).expect("parse");

        assert_eq!(nesting.levels.len(), 2);
        assert!(matches!(nesting.levels[0], PropertyNestLevel::DictKey("ribbons")));
        assert!(matches!(nesting.levels[1], PropertyNestLevel::ArrayIndex(0)));
        match nesting.action {
            UpdateAction::SetKey { key, value } => {
                assert_eq!(key, "ribbonId");
                assert_eq!(value, ArgValue::Int8(3));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    /// Descending into a scalar array element surfaces as a SetElement.
    #[test]
    fn descend_into_scalar_array_element_sets_element() {
        let t = ArgType::Array((None, Box::new(ArgType::Primitive(PrimitiveType::Int8))));
        let mut value = ArgValue::Array(vec![ArgValue::Int8(0), ArgValue::Int8(0)]);

        // cont=1 (descend), idx=1 (1 bit for len 2), 6 align bits, then the i8.
        let data = [0b1100_0000u8, 5];
        let nesting = get_nested_prop_path_helper(false, &t, &mut value, BitReader::new(&data)).expect("parse");

        assert!(nesting.levels.is_empty());
        match nesting.action {
            UpdateAction::SetElement { index, value } => {
                assert_eq!(index, 1);
                assert_eq!(value, ArgValue::Int8(5));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    /// A clean slice payload is exactly N whole elements with no leftover.
    #[test]
    fn slice_insert_parses_whole_elements() {
        let element = ArgType::FixedDict((
            false,
            vec![
                prop("ribbonId", ArgType::Primitive(PrimitiveType::Int8)),
                prop("count", ArgType::Primitive(PrimitiveType::Uint16)),
            ],
        ));
        let t = ArgType::Array((None, Box::new(element)));
        let mut value = ArgValue::Array(vec![]);

        // Empty array slice => 0 index bits; exactly one 3-byte element {id=4, count=1}.
        let data = [4u8, 1, 0];
        let nesting = nested_update_command(true, &t, &mut value, BitReader::new(&data)).expect("parse");

        match nesting.action {
            UpdateAction::SetRange { start, stop, values } => {
                assert_eq!((start, stop), (0, 0));
                assert_eq!(values.len(), 1);
                let ArgValue::FixedDict(map) = &values[0] else { panic!("expected a dict element") };
                assert_eq!(map.get("ribbonId"), Some(&ArgValue::Int8(4)));
                assert_eq!(map.get("count"), Some(&ArgValue::Uint16(1)));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    /// A leftover tail (misaligned bytes, not whole elements) fails fast rather
    /// than fabricating a half-decoded element.
    #[test]
    fn slice_insert_rejects_misaligned_tail() {
        let element = ArgType::FixedDict((
            false,
            vec![
                prop("ribbonId", ArgType::Primitive(PrimitiveType::Int8)),
                prop("count", ArgType::Primitive(PrimitiveType::Uint16)),
            ],
        ));
        let t = ArgType::Array((None, Box::new(element)));
        let mut value = ArgValue::Array(vec![]);

        // One 3-byte element plus a stray byte: 4 bytes for a 3-byte element.
        let data = [4u8, 1, 0, 0];
        assert!(nested_update_command(true, &t, &mut value, BitReader::new(&data)).is_err());
    }
}
