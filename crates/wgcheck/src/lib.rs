//! Parser for WGCheck `.gch` report archives.
//!
//! On-disk layout (from the WGCheck client, `FileManager.GetDataFromGzipLog` /
//! `PackReportsToGzip`):
//!
//! ```text
//! file = DES-CBC( GZip( BinaryFormatter-serialized LogFile ) )
//! ```
//!
//! DES key and IV are both the byte string `{101,72,31,42,54,66,70,81}`.
//! The inner stream is a .NET `BinaryFormatter` graph (MS-NRBF). The root
//! object is a `LogFile` whose members include the captured client text files
//! as strings: `PythonLog`, `PythonLog32`, `PythonLog64`, `ClientName`, etc.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use cbc::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use des::Des;
use flate2::read::GzDecoder;
use std::io::Read;

type DesCbcDec = cbc::Decryptor<Des>;

/// DES key and IV used by the WGCheck client. Key and IV are identical.
const KEY_IV: [u8; 8] = [101, 72, 31, 42, 54, 66, 70, 81];

/// Decrypt and decompress a `.gch` file into the raw NRBF byte stream.
pub fn decode_gch(file_bytes: &[u8]) -> Result<Vec<u8>> {
    // DES block size is 8 bytes. Trailing bytes that do not complete a block
    // cannot be part of the ciphertext; drop them. The GZip member ends well
    // before any PKCS7 padding, so decrypting with NoPadding and letting the
    // GZip reader stop at the member end is robust against padding variations.
    let usable = file_bytes.len() - (file_bytes.len() % 8);
    if usable == 0 {
        bail!("file too small to contain a DES block");
    }
    let mut buf = file_bytes[..usable].to_vec();
    DesCbcDec::new(&KEY_IV.into(), &KEY_IV.into())
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|e| anyhow!("DES decrypt failed: {e}"))?;

    if buf.len() < 2 || buf[0] != 0x1f || buf[1] != 0x8b {
        bail!("decrypted data is not a GZip stream (bad key or not a .gch file)");
    }

    let mut out = Vec::new();
    GzDecoder::new(&buf[..])
        .read_to_end(&mut out)
        .context("GZip decompress failed")?;
    Ok(out)
}

/// A parsed report: the named members of the root `LogFile` object.
#[derive(Debug, Default)]
pub struct Report {
    pub class_name: String,
    pub members: Vec<(String, Member)>,
}

/// A resolved member value. Only the value kinds that appear on `LogFile` are
/// modeled concretely; anything else is summarized in `Other`.
#[derive(Debug, Clone)]
pub enum Member {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// A reference to a non-string object (nested class, array, ...).
    Other(String),
}

/// Normalize a C# member name. Auto-property backing fields are emitted as
/// `<PropName>k__BackingField`; reduce them to `PropName`.
fn member_ident(raw: &str) -> &str {
    if let Some(rest) = raw.strip_prefix('<') {
        if let Some(end) = rest.find('>') {
            return &rest[..end];
        }
    }
    raw
}

impl Report {
    /// Return a string member by (normalized) name (only present strings).
    pub fn str(&self, name: &str) -> Option<&str> {
        self.members.iter().find_map(|(n, v)| match v {
            Member::Str(s) if member_ident(n) == name => Some(s.as_str()),
            _ => None,
        })
    }

    /// The three python.log variants captured by WGCheck, in (label, text) form.
    pub fn python_logs(&self) -> Vec<(&'static str, &str)> {
        let mut out = Vec::new();
        for (label, field) in [
            ("python.log", "PythonLog"),
            ("win32/python.log", "PythonLog32"),
            ("win64/python.log", "PythonLog64"),
        ] {
            if let Some(s) = self.str(field) {
                if !s.is_empty() {
                    out.push((label, s));
                }
            }
        }
        out
    }
}

/// Parse a `.gch` file all the way to a [`Report`].
pub fn parse_gch(file_bytes: &[u8]) -> Result<Report> {
    let nrbf = decode_gch(file_bytes)?;
    parse_nrbf(&nrbf)
}

// ------------------------------------------------------------------ NRBF ----

#[derive(Clone)]
enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// An inline string value (primitive-typed string, no object id).
    Str(String),
    /// Inline or referenced object, resolved against `objects` after parsing.
    Ref(i32),
}

enum Obj {
    Str(String),
    Class { name: String, members: Vec<(String, Value)> },
    /// Arrays and byte blobs are kept only as a short description.
    Summary(String),
}

/// One member's storage kind in a class layout.
#[derive(Clone)]
enum MemberKind {
    Primitive(u8),
    /// Anything encoded as a nested record (string, object, array, ...).
    Record,
}

#[derive(Clone)]
struct Layout {
    name: String,
    names: Vec<String>,
    kinds: Vec<MemberKind>,
}

struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
    objects: HashMap<i32, Obj>,
    layouts: HashMap<i32, Layout>,
    root_id: i32,
}

/// Result of reading one record in a member-value position.
enum Step {
    Value(Value),
    /// A run of N consecutive null members (ObjectNullMultiple*).
    Nulls(usize),
    End,
}

pub fn parse_nrbf(data: &[u8]) -> Result<Report> {
    let mut p = Parser {
        b: data,
        pos: 0,
        objects: HashMap::new(),
        layouts: HashMap::new(),
        root_id: 0,
    };
    p.run()?;
    p.into_report()
}

impl<'a> Parser<'a> {
    fn run(&mut self) -> Result<()> {
        let tag = self.u8()?;
        if tag != 0 {
            bail!("expected SerializationHeaderRecord, got tag {tag}");
        }
        self.root_id = self.i32()?;
        let _header_id = self.i32()?;
        let _major = self.i32()?;
        let _minor = self.i32()?;

        loop {
            match self.read_step()? {
                Step::End => break,
                Step::Value(_) => {}
                Step::Nulls(_) => {}
            }
            if self.pos >= self.b.len() {
                break;
            }
        }
        Ok(())
    }

    fn into_report(self) -> Result<Report> {
        let root = self
            .objects
            .get(&self.root_id)
            .ok_or_else(|| anyhow!("root object {} not found", self.root_id))?;
        let (name, members) = match root {
            Obj::Class { name, members } => (name.clone(), members),
            _ => bail!("root object is not a class"),
        };

        let mut out = Report { class_name: name, members: Vec::new() };
        for (mname, val) in members {
            out.members.push((mname.clone(), self.resolve(val)));
        }
        Ok(out)
    }

    fn resolve(&self, v: &Value) -> Member {
        match v {
            Value::Null => Member::Null,
            Value::Bool(b) => Member::Bool(*b),
            Value::Int(i) => Member::Int(*i),
            Value::Float(f) => Member::Float(*f),
            Value::Str(s) => Member::Str(s.clone()),
            Value::Ref(id) => match self.objects.get(id) {
                Some(Obj::Str(s)) => Member::Str(s.clone()),
                Some(Obj::Class { name, .. }) => Member::Other(format!("<{name}>")),
                Some(Obj::Summary(s)) => Member::Other(s.clone()),
                None => Member::Null,
            },
        }
    }

    // -- record dispatch ---------------------------------------------------

    /// Read one record where a value/reference is expected (top level or a
    /// class member of record kind).
    fn read_step(&mut self) -> Result<Step> {
        let tag = self.u8()?;
        Ok(match tag {
            11 => Step::End,                          // MessageEnd
            10 => Step::Nulls(1),                     // ObjectNull
            13 => Step::Nulls(self.u8()? as usize),   // ObjectNullMultiple256
            14 => Step::Nulls(self.i32()? as usize),  // ObjectNullMultiple
            _ => Step::Value(self.read_record(tag)?),
        })
    }

    fn read_record(&mut self, tag: u8) -> Result<Value> {
        match tag {
            12 => {
                // BinaryLibrary: LibraryId, LibraryName. Metadata only.
                let _id = self.i32()?;
                let _name = self.lp_string()?;
                // No value; caller (top level) ignores. Treat as a benign null.
                Ok(Value::Null)
            }
            9 => Ok(Value::Ref(self.i32()?)), // MemberReference
            8 => {
                // MemberPrimitiveTyped
                let prim = self.u8()?;
                self.read_primitive(prim)
            }
            6 => {
                // BinaryObjectString
                let id = self.i32()?;
                let s = self.lp_string()?;
                self.objects.insert(id, Obj::Str(s));
                Ok(Value::Ref(id))
            }
            5 => self.read_class(true),  // ClassWithMembersAndTypes
            4 => self.read_class(false), // SystemClassWithMembersAndTypes
            1 => self.read_class_with_id(),
            17 => self.read_object_array(true),  // ArraySingleString
            16 => self.read_object_array(false), // ArraySingleObject
            15 => self.read_primitive_array(),   // ArraySinglePrimitive
            7 => self.read_binary_array(),       // BinaryArray
            other => bail!("unsupported NRBF record tag {other} at offset {}", self.pos - 1),
        }
    }

    fn read_class(&mut self, with_library: bool) -> Result<Value> {
        let (obj_id, layout) = self.read_class_info_and_types()?;
        if with_library {
            let _lib = self.i32()?;
        }
        self.layouts.insert(obj_id, layout.clone());
        let members = self.read_members(&layout)?;
        self.objects
            .insert(obj_id, Obj::Class { name: layout.name, members });
        Ok(Value::Ref(obj_id))
    }

    fn read_class_with_id(&mut self) -> Result<Value> {
        // ClassWithId: ObjectId, MetadataId (an object id whose layout to reuse).
        let obj_id = self.i32()?;
        let meta_id = self.i32()?;
        let layout = self
            .layouts
            .get(&meta_id)
            .cloned()
            .ok_or_else(|| anyhow!("ClassWithId references unknown metadata id {meta_id}"))?;
        let members = self.read_members(&layout)?;
        self.objects
            .insert(obj_id, Obj::Class { name: layout.name, members });
        Ok(Value::Ref(obj_id))
    }

    fn read_class_info_and_types(&mut self) -> Result<(i32, Layout)> {
        // ClassInfo
        let obj_id = self.i32()?;
        let name = self.lp_string()?;
        let count = self.i32()? as usize;
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            names.push(self.lp_string()?);
        }
        // MemberTypeInfo: one BinaryTypeEnum per member, then AdditionalInfos.
        let mut btypes = Vec::with_capacity(count);
        for _ in 0..count {
            btypes.push(self.u8()?);
        }
        let mut kinds = Vec::with_capacity(count);
        for &bt in &btypes {
            let kind = match bt {
                0 => {
                    // Primitive: additional info is the primitive type enum.
                    MemberKind::Primitive(self.u8()?)
                }
                1 | 2 | 5 | 6 => MemberKind::Record, // String, Object, ObjectArray, StringArray
                3 => {
                    let _class_name = self.lp_string()?; // SystemClass
                    MemberKind::Record
                }
                4 => {
                    let _class_name = self.lp_string()?; // Class
                    let _lib = self.i32()?;
                    MemberKind::Record
                }
                7 => {
                    let _prim = self.u8()?; // PrimitiveArray
                    MemberKind::Record
                }
                other => bail!("unknown BinaryTypeEnum {other}"),
            };
            kinds.push(kind);
        }
        Ok((obj_id, Layout { name, names, kinds }))
    }

    fn read_members(&mut self, layout: &Layout) -> Result<Vec<(String, Value)>> {
        let mut out = Vec::with_capacity(layout.names.len());
        let mut null_run: usize = 0;
        for (name, kind) in layout.names.iter().zip(layout.kinds.iter()) {
            let v = match kind {
                MemberKind::Primitive(p) => self.read_primitive(*p)?,
                MemberKind::Record => {
                    if null_run > 0 {
                        null_run -= 1;
                        Value::Null
                    } else {
                        match self.read_step()? {
                            Step::Value(v) => v,
                            Step::Nulls(n) => {
                                null_run = n.saturating_sub(1);
                                Value::Null
                            }
                            Step::End => bail!("unexpected MessageEnd inside members"),
                        }
                    }
                }
            };
            out.push((name.clone(), v));
        }
        Ok(out)
    }

    // -- arrays ------------------------------------------------------------

    fn read_object_array(&mut self, strings: bool) -> Result<Value> {
        let obj_id = self.i32()?;
        let len = self.i32()? as usize;
        let mut null_run = 0usize;
        let mut n = 0usize;
        while n < len {
            if null_run > 0 {
                null_run -= 1;
                n += 1;
                continue;
            }
            match self.read_step()? {
                Step::Value(_) => n += 1,
                Step::Nulls(k) => {
                    null_run = k.saturating_sub(1);
                    n += 1;
                }
                Step::End => bail!("unexpected MessageEnd inside array"),
            }
        }
        let kind = if strings { "string[]" } else { "object[]" };
        self.objects.insert(obj_id, Obj::Summary(format!("{kind} len {len}")));
        Ok(Value::Ref(obj_id))
    }

    fn read_primitive_array(&mut self) -> Result<Value> {
        let obj_id = self.i32()?;
        let len = self.i32()? as usize;
        let prim = self.u8()?;
        for _ in 0..len {
            self.read_primitive(prim)?;
        }
        self.objects
            .insert(obj_id, Obj::Summary(format!("primitive[{prim}] len {len}")));
        Ok(Value::Ref(obj_id))
    }

    fn read_binary_array(&mut self) -> Result<Value> {
        let obj_id = self.i32()?;
        let array_type = self.u8()?; // BinaryArrayTypeEnumeration
        let rank = self.i32()? as usize;
        let mut total: usize = 1;
        for _ in 0..rank {
            let l = self.i32()? as usize;
            total = total.saturating_mul(l);
        }
        // Offset variants (3,4,5) carry lower bounds per rank.
        if matches!(array_type, 3 | 4 | 5) {
            for _ in 0..rank {
                let _lb = self.i32()?;
            }
        }
        // Element type info.
        let bt = self.u8()?;
        let prim = match bt {
            0 => Some(self.u8()?),
            3 => {
                let _n = self.lp_string()?;
                None
            }
            4 => {
                let _n = self.lp_string()?;
                let _lib = self.i32()?;
                None
            }
            7 => Some(self.u8()?),
            _ => None,
        };
        if bt == 0 {
            let prim = prim.unwrap();
            for _ in 0..total {
                self.read_primitive(prim)?;
            }
        } else {
            let mut null_run = 0usize;
            let mut n = 0usize;
            while n < total {
                if null_run > 0 {
                    null_run -= 1;
                    n += 1;
                    continue;
                }
                match self.read_step()? {
                    Step::Value(_) => n += 1,
                    Step::Nulls(k) => {
                        null_run = k.saturating_sub(1);
                        n += 1;
                    }
                    Step::End => bail!("unexpected MessageEnd inside binary array"),
                }
            }
        }
        self.objects
            .insert(obj_id, Obj::Summary(format!("array rank {rank} len {total}")));
        Ok(Value::Ref(obj_id))
    }

    // -- primitives --------------------------------------------------------

    fn read_primitive(&mut self, prim: u8) -> Result<Value> {
        Ok(match prim {
            1 => Value::Bool(self.u8()? != 0),
            2 => Value::Int(self.u8()? as i64),       // Byte
            3 => Value::Int(self.read_char()? as i64), // Char
            5 => Value::Float(self.lp_string()?.parse().unwrap_or(0.0)), // Decimal
            6 => Value::Float(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            7 => Value::Int(i16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            8 => Value::Int(i32::from_le_bytes(self.take(4)?.try_into().unwrap()) as i64),
            9 => Value::Int(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            10 => Value::Int(self.u8()? as i8 as i64), // SByte
            11 => Value::Float(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) as f64),
            12 => Value::Int(i64::from_le_bytes(self.take(8)?.try_into().unwrap())), // TimeSpan
            13 => {
                // DateTime: 64-bit, top 2 bits are kind. Keep the ticks.
                let raw = i64::from_le_bytes(self.take(8)?.try_into().unwrap());
                Value::Int(raw & 0x3FFF_FFFF_FFFF_FFFF)
            }
            14 => Value::Int(u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            15 => Value::Int(u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as i64),
            16 => Value::Int(u64::from_le_bytes(self.take(8)?.try_into().unwrap()) as i64),
            17 => Value::Null,
            18 => {
                let s = self.lp_string()?;
                Value::Str(s)
            }
            other => bail!("unknown PrimitiveTypeEnum {other}"),
        })
    }

    fn read_char(&mut self) -> Result<char> {
        // A single UTF-8 encoded character.
        let first = self.u8()?;
        let extra = if first < 0x80 {
            0
        } else if first >> 5 == 0b110 {
            1
        } else if first >> 4 == 0b1110 {
            2
        } else {
            3
        };
        let mut bytes = vec![first];
        for _ in 0..extra {
            bytes.push(self.u8()?);
        }
        let s = std::str::from_utf8(&bytes).map_err(|_| anyhow!("bad char"))?;
        s.chars().next().ok_or_else(|| anyhow!("empty char"))
    }

    // -- low-level cursor --------------------------------------------------

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.b.len() {
            bail!("unexpected end of stream at {} (+{n})", self.pos);
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// 7-bit length-prefixed UTF-8 string (.NET `BinaryReader` format).
    fn lp_string(&mut self) -> Result<String> {
        let mut len: usize = 0;
        let mut shift = 0;
        loop {
            let byte = self.u8()?;
            len |= ((byte & 0x7f) as usize) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 35 {
                bail!("string length varint too long");
            }
        }
        let bytes = self.take(len)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}
