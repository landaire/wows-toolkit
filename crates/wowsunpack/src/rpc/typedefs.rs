use crate::data::parser_utils::WResult;
#[cfg(feature = "serde")]
use serde::ser::{SerializeMap, SerializeSeq, SerializeTuple};
use std::collections::HashMap;
use std::convert::TryInto;
use winnow::Parser;
use winnow::binary::{le_f32, le_f64, le_i8, le_i16, le_i32, le_i64, le_u8, le_u16, le_u32, le_u64};
use winnow::token::take;

/// Type alias matching winnow's default error for binary parsers.
type WinnowErr = winnow::error::ErrMode<winnow::error::ContextError>;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("{0}")]
    Parse(WinnowErr),
    #[error("Unknown FixedDict flag: {flag:#x}")]
    UnknownFixedDictFlag { flag: u8 },
}

impl From<WinnowErr> for RpcError {
    fn from(e: WinnowErr) -> Self {
        RpcError::Parse(e)
    }
}

type IResult<T> = Result<T, RpcError>;

pub type TypeAliases = HashMap<String, ArgType>;

fn child_by_name<'a, 'b>(node: &roxmltree::Node<'a, 'b>, name: &str) -> Option<roxmltree::Node<'a, 'b>> {
    node.children().find(|&child| child.tag_name().name() == name)
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveType {
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Int8,
    Int16,
    Int32,
    Int64,
    Float32,
    Float64,
    Vector2,
    Vector3,
    String,
    UnicodeString,
    Blob,
}

impl PrimitiveType {
    fn parse_value<'argtype>(&'argtype self, input: &mut &[u8]) -> WResult<ArgValue<'argtype>> {
        Ok(match self {
            PrimitiveType::Uint8 => ArgValue::Uint8(le_u8.parse_next(input)?),
            PrimitiveType::Uint16 => ArgValue::Uint16(le_u16.parse_next(input)?),
            PrimitiveType::Uint32 => ArgValue::Uint32(le_u32.parse_next(input)?),
            PrimitiveType::Uint64 => ArgValue::Uint64(le_u64.parse_next(input)?),
            PrimitiveType::Int8 => ArgValue::Int8(le_i8.parse_next(input)?),
            PrimitiveType::Int16 => ArgValue::Int16(le_i16.parse_next(input)?),
            PrimitiveType::Int32 => ArgValue::Int32(le_i32.parse_next(input)?),
            PrimitiveType::Int64 => ArgValue::Int64(le_i64.parse_next(input)?),
            PrimitiveType::Float32 => ArgValue::Float32(le_f32.parse_next(input)?),
            PrimitiveType::Float64 => ArgValue::Float64(le_f64.parse_next(input)?),
            PrimitiveType::Vector2 => {
                let x = le_f32.parse_next(input)?;
                let y = le_f32.parse_next(input)?;
                ArgValue::Vector2((x, y))
            }
            PrimitiveType::Vector3 => {
                let x = le_f32.parse_next(input)?;
                let y = le_f32.parse_next(input)?;
                let z = le_f32.parse_next(input)?;
                ArgValue::Vector3((x, y, z))
            }
            PrimitiveType::Blob => {
                let data = parse_length_prefixed_bytes(input)?;
                ArgValue::Blob(data)
            }
            PrimitiveType::String => {
                let data = parse_length_prefixed_bytes(input)?;
                ArgValue::String(data)
            }
            PrimitiveType::UnicodeString => {
                let data = parse_length_prefixed_bytes(input)?;
                ArgValue::UnicodeString(data)
            }
        })
    }
}

/// Helper to read a single u8 via winnow.
fn read_u8(input: &mut &[u8]) -> IResult<u8> {
    Ok(le_u8::<_, WinnowErr>.parse_next(input)?)
}

/// Parse a length-prefixed byte sequence: u8 length, or 0xFF then u16 length + u8 unknown.
fn parse_length_prefixed_bytes(input: &mut &[u8]) -> WResult<Vec<u8>> {
    let size = le_u8.parse_next(input)?;
    if size == 0xff {
        let size = le_u16.parse_next(input)?;
        let _unknown = le_u8.parse_next(input)?;
        let data = take(size as usize).parse_next(input)?;
        Ok(data.to_vec())
    } else {
        let data = take(size as usize).parse_next(input)?;
        Ok(data.to_vec())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixedDictProperty {
    pub name: String,
    pub prop_type: ArgType,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArgType {
    Primitive(PrimitiveType),
    Array((Option<usize>, Box<ArgType>)),

    /// (allow_none, properties)
    FixedDict((bool, Vec<FixedDictProperty>)),
    Tuple((Box<ArgType>, usize)),
}

#[derive(Clone, Debug, PartialEq, variantly::Variantly)]
pub enum ArgValue<'argtype> {
    Uint8(u8),
    Uint16(u16),
    Uint32(u32),
    Uint64(u64),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    Float32(f32),
    Float64(f64),
    Vector2((f32, f32)),
    Vector3((f32, f32, f32)),
    String(Vec<u8>),
    UnicodeString(Vec<u8>),
    Blob(Vec<u8>),
    Array(Vec<ArgValue<'argtype>>),
    FixedDict(HashMap<&'argtype str, ArgValue<'argtype>>),
    NullableFixedDict(Option<HashMap<&'argtype str, ArgValue<'argtype>>>),
    Tuple(Vec<ArgValue<'argtype>>),
}

impl<'argtype> ArgValue<'argtype> {
    /// Convert any integer variant to i32 (widening or narrowing as needed).
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Int8(v) => Some(*v as i32),
            Self::Int16(v) => Some(*v as i32),
            Self::Int32(v) => Some(*v),
            Self::Int64(v) => Some(*v as i32),
            Self::Uint8(v) => Some(*v as i32),
            Self::Uint16(v) => Some(*v as i32),
            Self::Uint32(v) => Some(*v as i32),
            Self::Uint64(v) => Some(*v as i32),
            _ => None,
        }
    }

    /// Convert any integer variant to u32 (widening or narrowing as needed).
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::Int8(v) => Some(*v as u32),
            Self::Int16(v) => Some(*v as u32),
            Self::Int32(v) => Some(*v as u32),
            Self::Int64(v) => Some(*v as u32),
            Self::Uint8(v) => Some(*v as u32),
            Self::Uint16(v) => Some(*v as u32),
            Self::Uint32(v) => Some(*v),
            Self::Uint64(v) => Some(*v as u32),
            _ => None,
        }
    }

    /// Convert any integer variant to i64 (always lossless for signed).
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int8(v) => Some(*v as i64),
            Self::Int16(v) => Some(*v as i64),
            Self::Int32(v) => Some(*v as i64),
            Self::Int64(v) => Some(*v),
            Self::Uint8(v) => Some(*v as i64),
            Self::Uint16(v) => Some(*v as i64),
            Self::Uint32(v) => Some(*v as i64),
            Self::Uint64(v) => Some(*v as i64),
            _ => None,
        }
    }

    /// Convert any integer variant to u64 (widening or narrowing as needed).
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Int8(v) => Some(*v as u64),
            Self::Int16(v) => Some(*v as u64),
            Self::Int32(v) => Some(*v as u64),
            Self::Int64(v) => Some(*v as u64),
            Self::Uint8(v) => Some(*v as u64),
            Self::Uint16(v) => Some(*v as u64),
            Self::Uint32(v) => Some(*v as u64),
            Self::Uint64(v) => Some(*v),
            _ => None,
        }
    }

    /// Convert any numeric variant to f32.
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::Float32(v) => Some(*v),
            Self::Float64(v) => Some(*v as f32),
            Self::Int8(v) => Some(*v as f32),
            Self::Int16(v) => Some(*v as f32),
            Self::Int32(v) => Some(*v as f32),
            Self::Int64(v) => Some(*v as f32),
            Self::Uint8(v) => Some(*v as f32),
            Self::Uint16(v) => Some(*v as f32),
            Self::Uint32(v) => Some(*v as f32),
            Self::Uint64(v) => Some(*v as f32),
            _ => None,
        }
    }

    /// Convert any numeric variant to f64.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float64(v) => Some(*v),
            Self::Float32(v) => Some(*v as f64),
            Self::Int8(v) => Some(*v as f64),
            Self::Int16(v) => Some(*v as f64),
            Self::Int32(v) => Some(*v as f64),
            Self::Int64(v) => Some(*v as f64),
            Self::Uint8(v) => Some(*v as f64),
            Self::Uint16(v) => Some(*v as f64),
            Self::Uint32(v) => Some(*v as f64),
            Self::Uint64(v) => Some(*v as f64),
            _ => None,
        }
    }
}

#[cfg(feature = "serde")]
impl<'argtype> serde::Serialize for ArgValue<'argtype> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        //serializer.serialize_i32(5)
        match self {
            Self::Uint8(i) => serializer.serialize_u8(*i),
            Self::Uint16(i) => serializer.serialize_u16(*i),
            Self::Uint32(i) => serializer.serialize_u32(*i),
            Self::Uint64(i) => serializer.serialize_u64(*i),
            Self::Int8(i) => serializer.serialize_i8(*i),
            Self::Int16(i) => serializer.serialize_i16(*i),
            Self::Int32(i) => serializer.serialize_i32(*i),
            Self::Int64(i) => serializer.serialize_i64(*i),
            Self::Float32(f) => serializer.serialize_f32(*f),
            Self::Float64(f) => serializer.serialize_f64(*f),
            Self::Vector2((x, y)) => {
                let mut tup = serializer.serialize_tuple(2)?;
                tup.serialize_element(x)?;
                tup.serialize_element(y)?;
                tup.end()
            }
            Self::Vector3((x, y, z)) => {
                let mut tup = serializer.serialize_tuple(3)?;
                tup.serialize_element(x)?;
                tup.serialize_element(y)?;
                tup.serialize_element(z)?;
                tup.end()
            }
            Self::String(s) => serializer.serialize_bytes(s),
            Self::UnicodeString(s) => serializer.serialize_bytes(s),
            Self::Blob(blob) => {
                // TODO: Determine when we can/can't pickle-decode this
                // Also, make pickled::Value implement Serialize
                #[cfg(feature = "json")]
                {
                    let decoded: Result<serde_json::Value, _> =
                        pickled::from_slice(blob, pickled::de::DeOptions::new());
                    match decoded {
                        Ok(v) => serializer.serialize_some(&v),
                        Err(_) => serializer.serialize_bytes(blob),
                    }
                }
                #[cfg(not(feature = "json"))]
                {
                    serializer.serialize_bytes(blob)
                }
            }
            Self::Array(a) => {
                let mut seq = serializer.serialize_seq(Some(a.len()))?;
                for element in a.iter() {
                    seq.serialize_element(element)?;
                }
                seq.end()
            }
            Self::FixedDict(d) => {
                let mut obj = serializer.serialize_map(Some(d.len()))?;
                for (k, v) in d.iter() {
                    obj.serialize_entry(k, v)?;
                }
                obj.end()
            }
            Self::NullableFixedDict(Some(d)) => {
                let mut obj = serializer.serialize_map(Some(d.len()))?;
                for (k, v) in d.iter() {
                    obj.serialize_entry(k, v)?;
                }
                obj.end()
            }
            Self::NullableFixedDict(None) => serializer.serialize_none(),
            Self::Tuple(_t) => {
                unimplemented!();
            }
        }
    }
}

const INFINITY: usize = 0xffff;

impl ArgType {
    pub fn sort_size(&self) -> usize {
        match self {
            Self::Primitive(PrimitiveType::Uint8) => 1,
            Self::Primitive(PrimitiveType::Uint16) => 2,
            Self::Primitive(PrimitiveType::Uint32) => 4,
            Self::Primitive(PrimitiveType::Uint64) => 8,
            Self::Primitive(PrimitiveType::Int8) => 1,
            Self::Primitive(PrimitiveType::Int16) => 2,
            Self::Primitive(PrimitiveType::Int32) => 4,
            Self::Primitive(PrimitiveType::Int64) => 8,
            Self::Primitive(PrimitiveType::Float32) => 4,
            Self::Primitive(PrimitiveType::Float64) => 8,
            Self::Primitive(PrimitiveType::Vector2) => 8,
            Self::Primitive(PrimitiveType::Vector3) => 12,
            Self::Primitive(PrimitiveType::String) => INFINITY,
            Self::Primitive(PrimitiveType::UnicodeString) => INFINITY,
            Self::Primitive(PrimitiveType::Blob) => INFINITY,
            Self::Array((None, _)) => INFINITY,
            Self::Array((Some(count), t)) => {
                let sort_size = t.sort_size();
                if sort_size == INFINITY { INFINITY } else { sort_size * count }
            }
            Self::FixedDict((allow_none, props)) => {
                if *allow_none {
                    return INFINITY;
                }
                props
                    .iter()
                    .map(|x| x.prop_type.sort_size())
                    .fold(0, |a, b| if a == INFINITY || b == INFINITY { INFINITY } else { a + b })
            }
            Self::Tuple((t, count)) => {
                let sort_size = t.sort_size();
                if sort_size == INFINITY { INFINITY } else { sort_size * count }
            }
        }
    }

    pub fn parse_value<'a, 'b>(&'b self, input: &mut &'a [u8]) -> IResult<ArgValue<'b>> {
        match self {
            Self::Primitive(p) => Ok(p.parse_value(input)?),
            Self::Array((count, atype)) => {
                let length = match count {
                    Some(count) => *count,
                    None => read_u8(input)? as usize,
                };
                let mut values = Vec::with_capacity(length);
                for _ in 0..length {
                    values.push(atype.parse_value(input)?);
                }
                Ok(ArgValue::Array(values))
            }
            Self::FixedDict((allow_none, props)) => {
                if *allow_none {
                    let flag = read_u8(input)?;
                    if flag == 0 {
                        return Ok(ArgValue::NullableFixedDict(None));
                    } else if flag != 1 {
                        return Err(RpcError::UnknownFixedDictFlag { flag });
                    }
                }
                let mut dict: HashMap<&'b str, ArgValue<'b>> = HashMap::new();
                for property in props.iter() {
                    let element = property.prop_type.parse_value(input)?;
                    dict.insert(&property.name, element);
                }
                if *allow_none { Ok(ArgValue::NullableFixedDict(Some(dict))) } else { Ok(ArgValue::FixedDict(dict)) }
            }
            Self::Tuple((_t, _count)) => {
                panic!("Tuple parsing is unsupported");
            }
        }
    }
}

pub fn parse_type(arg: &roxmltree::Node, aliases: &HashMap<String, ArgType>) -> ArgType {
    let t = arg.first_child().unwrap().text().unwrap().trim();
    if t == "UINT8" {
        ArgType::Primitive(PrimitiveType::Uint8)
    } else if t == "UINT16" {
        ArgType::Primitive(PrimitiveType::Uint16)
    } else if t == "UINT32" {
        ArgType::Primitive(PrimitiveType::Uint32)
    } else if t == "UINT64" {
        ArgType::Primitive(PrimitiveType::Uint64)
    } else if t == "INT8" {
        ArgType::Primitive(PrimitiveType::Int8)
    } else if t == "INT16" {
        ArgType::Primitive(PrimitiveType::Int16)
    } else if t == "INT32" {
        ArgType::Primitive(PrimitiveType::Int32)
    } else if t == "INT64" {
        ArgType::Primitive(PrimitiveType::Int64)
    } else if t == "FLOAT32" {
        ArgType::Primitive(PrimitiveType::Float32)
    } else if t == "FLOAT" {
        // Note that "FLOAT64" is Float64
        ArgType::Primitive(PrimitiveType::Float32)
    } else if t == "STRING" {
        ArgType::Primitive(PrimitiveType::String)
    } else if t == "UNICODE_STRING" {
        ArgType::Primitive(PrimitiveType::UnicodeString)
    } else if t == "VECTOR2" {
        ArgType::Primitive(PrimitiveType::Vector2)
    } else if t == "VECTOR3" {
        ArgType::Primitive(PrimitiveType::Vector3)
    } else if t == "BLOB" {
        ArgType::Primitive(PrimitiveType::Blob)
    } else if t == "USER_TYPE" || t == "MAILBOX" || t == "PYTHON" {
        // TODO: This is a HACKY HACKY workaround for things we don't recognize
        ArgType::Primitive(PrimitiveType::Blob)
    } else if t == "ARRAY" {
        let subtype = parse_type(&child_by_name(arg, "of").unwrap(), aliases);
        /*let subtype = match subtype {
            ArgType::Primitive(p) => p,
            _ => {
                panic!("Unsupported array subtype {:?}", subtype);
            }
        };*/
        let count = child_by_name(arg, "size").map(|count| count.text().unwrap().trim().parse::<usize>().unwrap());
        ArgType::Array((count, Box::new(subtype)))
    } else if t == "FIXED_DICT" {
        let mut props = vec![];
        //println!("{:#?}", arg);
        let allow_none = child_by_name(arg, "AllowNone").is_some();
        let properties = match child_by_name(arg, "Properties") {
            Some(p) => p,
            None => {
                return ArgType::FixedDict((allow_none, vec![]));
            }
        };
        for prop in properties.children() {
            if !prop.is_element() {
                continue;
            }
            let name = prop.tag_name().name();
            let prop_type = child_by_name(&prop, "Type").unwrap();
            let prop_type = parse_type(&prop_type, aliases);
            props.push(FixedDictProperty { name: name.to_string(), prop_type });
        }
        ArgType::FixedDict((allow_none, props))
    } else if t == "TUPLE" {
        let subtype = parse_type(&child_by_name(arg, "of").unwrap(), aliases);
        let count = child_by_name(arg, "size").unwrap().text().unwrap().trim().parse::<usize>().unwrap();
        ArgType::Tuple((Box::new(subtype), count))
    } else if aliases.contains_key(t) {
        aliases.get(t).unwrap().clone()
    } else {
        panic!("Unrecognized type {t}");
    }
}

pub fn parse_aliases(def: &[u8]) -> HashMap<String, ArgType> {
    let def = std::str::from_utf8(def).unwrap();
    let mut aliases = HashMap::new();

    //let def = std::fs::read_to_string(&file).unwrap();
    let doc = roxmltree::Document::parse(def).unwrap();
    let root = doc.root();

    for t in root.first_child().unwrap().children() {
        if !t.is_element() {
            continue;
        }
        //println!("{}", t.tag_name().name());
        aliases.insert(t.tag_name().name().to_string(), parse_type(&t, &aliases));
    }
    //println!("Found {} type aliases", aliases.len());
    aliases
}

macro_rules! into_unwrappable_type {
    ($t: ty, $tag: path) => {
        impl<'a> std::convert::TryInto<$t> for &ArgValue<'a> {
            type Error = ();

            fn try_into(self) -> Result<$t, Self::Error> {
                match self {
                    $tag(i) => Ok(*i),
                    _ => Err(()),
                }
            }
        }
    };
}

into_unwrappable_type!(u8, ArgValue::Uint8);
into_unwrappable_type!(u16, ArgValue::Uint16);
into_unwrappable_type!(u32, ArgValue::Uint32);
into_unwrappable_type!(u64, ArgValue::Uint64);
into_unwrappable_type!(i8, ArgValue::Int8);
into_unwrappable_type!(i16, ArgValue::Int16);
into_unwrappable_type!(i32, ArgValue::Int32);
into_unwrappable_type!(i64, ArgValue::Int64);
into_unwrappable_type!(f32, ArgValue::Float32);
into_unwrappable_type!(f64, ArgValue::Float64);

impl<'a, 'b, T> std::convert::TryFrom<&'b ArgValue<'a>> for Vec<T>
where
    &'b ArgValue<'a>: std::convert::TryInto<T, Error = ()>,
{
    type Error = ();

    fn try_from(value: &'b ArgValue<'a>) -> Result<Self, Self::Error> {
        match value {
            ArgValue::Array(v) => {
                let result: Result<Vec<T>, Self::Error> = v.iter().map(|x| x.try_into()).collect();
                result
            }
            _ => Err(()),
        }
    }
}

#[macro_export]
macro_rules! unpack_rpc_args {
    ($args: ident, $($t: ty),+) => {
        {
            let mut i = 0;
            ($({
                let x: $t = <&$crate::rpc::typedefs::ArgValue as std::convert::TryInto<$t>>::try_into(&$args[i]).unwrap();
                i += 1;
                let _ = i; // Ignore "assigned variable never read" error
                x
            }),+,)
        }
    };
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_argtype() {
        let doc = "<Arg> UINT8 </Arg>";
        let doc = roxmltree::Document::parse(doc).unwrap();
        let root = doc.root();
        assert_eq!(parse_type(&root, &HashMap::new()), ArgType::Primitive(PrimitiveType::Uint8));
    }

    #[test]
    fn test_int16() {
        let doc = "<Arg> INT16 </Arg>";
        let doc = roxmltree::Document::parse(doc).unwrap();
        let root = doc.root();
        assert_eq!(parse_type(&root, &HashMap::new()), ArgType::Primitive(PrimitiveType::Int16));
    }

    #[test]
    fn test_fixed_dict() {
        let doc = "<Arg>
            FIXED_DICT
            <Properties>
                <byShip><Type>FLOAT</Type></byShip>
                <byPlane><Type>FLOAT</Type></byPlane>
                <bySmoke><Type>FLOAT</Type></bySmoke>
            </Properties>
        </Arg>";
        let doc = roxmltree::Document::parse(doc).unwrap();
        let root = doc.root_element();
        let t = parse_type(&root, &HashMap::new());
        assert_eq!(
            t,
            ArgType::FixedDict((
                false,
                vec![
                    FixedDictProperty {
                        name: "byShip".to_string(),
                        prop_type: ArgType::Primitive(PrimitiveType::Float32),
                    },
                    FixedDictProperty {
                        name: "byPlane".to_string(),
                        prop_type: ArgType::Primitive(PrimitiveType::Float32),
                    },
                    FixedDictProperty {
                        name: "bySmoke".to_string(),
                        prop_type: ArgType::Primitive(PrimitiveType::Float32),
                    }
                ]
            ))
        );
        assert_eq!(t.sort_size(), 12);
    }

    #[test]
    fn test_crew_modifiers() {
        let alias = "<CREW_MODIFIERS_COMPACT_PARAMS>
            FIXED_DICT
            <Properties>
                <paramsId><Type>UINT32</Type></paramsId>
                <isInAdaptation><Type>BOOL</Type></isInAdaptation>
                <learnedSkills><Type>ARRAY<of>ARRAY<of>UINT8</of></of></Type></learnedSkills>
            </Properties>
            <implementedBy>CrewModifiers.crewModifiersCompactParamsConverter</implementedBy>
        </CREW_MODIFIERS_COMPACT_PARAMS>";
        let doc = roxmltree::Document::parse(alias).unwrap();
        let root = doc.root_element();
        let mut aliases = HashMap::new();
        aliases.insert("BOOL".to_string(), ArgType::Primitive(PrimitiveType::Uint8));
        aliases.insert("CREW_MODIFIERS_COMPACT_PARAMS".to_string(), parse_type(&root, &aliases));

        let proptype = "<Type>CREW_MODIFIERS_COMPACT_PARAMS</Type>";
        let doc = roxmltree::Document::parse(proptype).unwrap();
        let root = doc.root();
        let t = parse_type(&root, &aliases);
        assert_eq!(t.sort_size(), 65535);
    }

    #[test]
    fn test_fixeddict_allownone() {
        let spec = "<TRIGGERS_STATE>
            FIXED_DICT
            <Properties>
                <modifier><Type> MODIFIER_STATE </Type></modifier>
            </Properties>
            <AllowNone>true</AllowNone>
        </TRIGGERS_STATE>";
        let mut aliases = HashMap::new();
        aliases.insert("MODIFIER_STATE".to_string(), ArgType::Primitive(PrimitiveType::Uint32));

        let doc = roxmltree::Document::parse(spec).unwrap();
        let root = doc.root_element();
        let t = parse_type(&root, &aliases);
        //println!("{:#?}", t);

        let data = [0];
        let mut input = &data[..];
        let result = t.parse_value(&mut input).unwrap();
        assert!(input.is_empty());
        assert_eq!(result, ArgValue::NullableFixedDict(None));

        let data = [1, 5, 0, 0, 0];
        let mut input = &data[..];
        let result = t.parse_value(&mut input).unwrap();
        assert!(input.is_empty());
        let m = match result {
            ArgValue::NullableFixedDict(Some(h)) => h,
            _ => panic!(),
        };
        assert_eq!(*m.get("modifier").unwrap(), ArgValue::Uint32(5));
    }

    #[test]
    fn test_fixedsize_array() {
        let spec = "<Type>ARRAY<of>UINT16</of><size>2</size></Type>";
        let doc = roxmltree::Document::parse(spec).unwrap();
        let root = doc.root_element();
        let aliases = HashMap::new();
        let t = parse_type(&root, &aliases);
        //println!("{:#?}", t);

        let data = [1, 0, 3, 0];
        let mut input = &data[..];
        let result = t.parse_value(&mut input).unwrap();
        assert!(input.is_empty());
        assert_eq!(result, ArgValue::Array(vec![ArgValue::Uint16(1), ArgValue::Uint16(3)]));
    }

    #[test]
    fn test_unpacker_macro_single() {
        let args = [ArgValue::Uint8(5)];
        let (u8_arg,) = unpack_rpc_args!(args, u8);
        assert_eq!(u8_arg, 5);
    }

    #[test]
    fn test_unpacker_macro() {
        let args = vec![
            ArgValue::Uint8(5),
            ArgValue::Int32(-54),
            ArgValue::Array(vec![ArgValue::Uint16(1), ArgValue::Uint16(3)]),
            //ArgValue::NullableFixedDict(None),
            //ArgValue::NullableFixedDict(Some(HashMap::new())),
            //ArgValue::String("Hello, world!".to_string()),
        ];
        let args = unpack_rpc_args!(args, u8, i32, Vec<u16>);
        assert_eq!(args.0, 5);
        assert_eq!(args.1, -54);
        assert_eq!(args.2, vec![1, 3]);
    }
}
