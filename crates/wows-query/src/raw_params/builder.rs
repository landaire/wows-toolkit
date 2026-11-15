//! Builds `params.bin` from GameParams entries, interning strings and
//! storing each distinct array and object once.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;
use std::io::Write;
use std::ops::Range;

use serde_json::Value;

use super::format::ABSENT;
use super::format::ENTRY_WORDS;
use super::format::HEADER_LEN;
use super::format::Layout;
use super::format::NodeId;
use super::format::SECTION_ALIGN;
use super::format::Section;
use super::format::StringId;
use super::format::Tag;
use super::format::ValueRef;
use crate::error::RawParamsError;

/// Store contents held in memory until written.
#[derive(Default)]
pub(crate) struct Builder {
    string_ids: HashMap<Box<str>, StringId>,
    string_bytes: Vec<u8>,
    string_ends: Vec<u32>,

    int_ids: HashMap<i64, u32>,
    ints: Vec<i64>,
    float_ids: HashMap<u64, u32>,
    floats: Vec<f64>,

    node_ends: Vec<u32>,
    slots: Vec<u32>,
    /// Hash of a node's slots to the newest node with that hash; `node_chain`
    /// links each node to the previous one with the same hash.
    node_heads: HashMap<u64, NodeId>,
    node_chain: Vec<Option<NodeId>>,

    entries: Vec<[u32; ENTRY_WORDS]>,

    /// Slots of the nodes being interned, innermost last.
    scratch: Vec<u32>,
}

impl Builder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds one GameParams entry.
    pub(crate) fn add_entry(&mut self, name: &str, value: &Value) -> Result<(), RawParamsError> {
        let root = self.intern(value)?;

        let typeinfo = value.get("typeinfo");
        let field = |key: &str| text(typeinfo.and_then(|typeinfo| typeinfo.get(key)));
        let index = text(value.get("index"));
        let id = value.get("id").and_then(Value::as_u64);
        let level = value.get("level").and_then(Value::as_u64).and_then(|level| u32::try_from(level).ok());

        let name_id = self.string(name)?;
        let record = [
            name_id.0,
            root.0,
            self.optional_string(index)?,
            self.optional_string(field("type"))?,
            self.optional_string(field("species"))?,
            self.optional_string(field("nation"))?,
            // u32::MAX is no tier of anything; the reader treats it as absent.
            level.filter(|level| *level != ABSENT).unwrap_or(ABSENT),
            id.map_or(ABSENT, |id| id as u32),
            id.map_or(ABSENT, |id| (id >> 32) as u32),
        ];
        self.entries.push(record);

        Ok(())
    }

    /// Writes the store for `build` and returns its length in bytes.
    pub(crate) fn write(self, build: u32, output: &mut impl Write) -> std::io::Result<u64> {
        let by_name = self.order(|left, right| self.entry_string(left, 0).cmp(self.entry_string(right, 0)), |_| true);
        let by_index = self.order(
            |left, right| {
                let upper = |position| self.entry_string(position, 2).bytes().map(|byte| byte.to_ascii_uppercase());
                upper(left).cmp(upper(right))
            },
            |record| record[2] != ABSENT,
        );
        let by_id = self.order(
            |left, right| self.entry_id(left).cmp(&self.entry_id(right)),
            |record| record[7] != ABSENT || record[8] != ABSENT,
        );

        let lengths = [
            self.string_bytes.len(),
            4 * self.string_ends.len(),
            8 * self.ints.len(),
            8 * self.floats.len(),
            4 * self.node_ends.len(),
            4 * self.slots.len(),
            4 * ENTRY_WORDS * self.entries.len(),
            4 * by_name.len(),
            4 * by_index.len(),
            4 * by_id.len(),
        ];
        let mut sections: [Range<usize>; Section::ALL.len()] = Default::default();
        let mut offset = HEADER_LEN.next_multiple_of(SECTION_ALIGN);
        for (range, len) in sections.iter_mut().zip(lengths) {
            *range = offset..offset + len;
            offset = range.end.next_multiple_of(SECTION_ALIGN);
        }
        let layout = Layout::new(build, sections.clone());

        output.write_all(&layout.header())?;
        let mut written = HEADER_LEN;

        for (section, range) in Section::ALL.into_iter().zip(&sections) {
            const ZEROS: [u8; SECTION_ALIGN] = [0; SECTION_ALIGN];
            output.write_all(&ZEROS[..range.start - written])?;
            match section {
                Section::StringBytes => output.write_all(&self.string_bytes)?,
                Section::StringEnds => write_words(output, &self.string_ends)?,
                Section::Ints => write_longs(output, self.ints.iter().map(|value| value.to_le_bytes()))?,
                Section::Floats => write_longs(output, self.floats.iter().map(|value| value.to_le_bytes()))?,
                Section::NodeEnds => write_words(output, &self.node_ends)?,
                Section::Slots => write_words(output, &self.slots)?,
                Section::Entries => write_words(output, self.entries.as_flattened())?,
                Section::ByName => write_words(output, &by_name)?,
                Section::ByIndex => write_words(output, &by_index)?,
                Section::ById => write_words(output, &by_id)?,
            }
            written = range.end;
        }

        Ok(written as u64)
    }

    /// The value word of `root`, interning every node below it. Walks with
    /// an explicit stack; GameParams nests deeper than is safe to recurse.
    fn intern(&mut self, root: &Value) -> Result<ValueRef, RawParamsError> {
        enum Frame<'v> {
            Array(std::slice::Iter<'v, Value>, usize),
            Object(serde_json::map::Iter<'v>, usize),
        }

        let mut frames: Vec<Frame<'_>> = Vec::new();
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        let mut next = Some(root);

        let result = loop {
            if let Some(value) = next.take() {
                match value {
                    Value::Array(items) => frames.push(Frame::Array(items.iter(), scratch.len())),
                    Value::Object(fields) => frames.push(Frame::Object(fields.iter(), scratch.len())),
                    scalar => {
                        let word = self.scalar(scalar)?;
                        if frames.is_empty() {
                            break word;
                        }
                        scratch.push(word.0);
                    }
                }
            }

            let Some(frame) = frames.last_mut() else {
                unreachable!("a container frame is open until the root is returned");
            };
            match frame {
                Frame::Array(items, _) => {
                    if let Some(item) = items.next() {
                        next = Some(item);
                        continue;
                    }
                }
                Frame::Object(fields, _) => {
                    if let Some((key, value)) = fields.next() {
                        let key = self.string(key)?;
                        scratch.push(key.0);
                        next = Some(value);
                        continue;
                    }
                }
            }

            let (tag, start) = match frames.pop() {
                Some(Frame::Array(_, start)) => (Tag::Array, start),
                Some(Frame::Object(_, start)) => (Tag::Object, start),
                None => unreachable!("the frame just inspected"),
            };
            let node = self.node(start, &scratch)?;
            scratch.truncate(start);
            let word = ValueRef::new(tag, node.0).ok_or(RawParamsError::TooLarge)?;
            if frames.is_empty() {
                break word;
            }
            scratch.push(word.0);
        };
        self.scratch = scratch;

        Ok(result)
    }

    fn scalar(&mut self, value: &Value) -> Result<ValueRef, RawParamsError> {
        Ok(match value {
            Value::Null => ValueRef::NULL,
            Value::Bool(false) => ValueRef::FALSE,
            Value::Bool(true) => ValueRef::TRUE,
            Value::String(text) => {
                let id = self.string(text)?;
                ValueRef::new(Tag::Str, id.0).ok_or(RawParamsError::TooLarge)?
            }
            Value::Number(number) => match (number.as_i64(), number.as_f64()) {
                (Some(int), _) => match ValueRef::small_int(int) {
                    Some(word) => word,
                    None => {
                        let next = self.ints.len();
                        let index = *self.int_ids.entry(int).or_insert_with(|| next as u32);
                        if index as usize == next {
                            self.ints.push(int);
                        }
                        ValueRef::new(Tag::Int, index).ok_or(RawParamsError::TooLarge)?
                    }
                },
                (None, Some(float)) if number.is_f64() => {
                    let next = self.floats.len();
                    let index = *self.float_ids.entry(float.to_bits()).or_insert_with(|| next as u32);
                    if index as usize == next {
                        self.floats.push(float);
                    }
                    ValueRef::new(Tag::Float, index).ok_or(RawParamsError::TooLarge)?
                }
                // GameParams numbers are pickle ints and floats, which JSON
                // holds as i64 and f64; a u64 above i64::MAX has no jq form
                // this store keeps.
                _ => return Err(RawParamsError::UnsupportedNumber { number: number.to_string() }),
            },
            Value::Array(_) | Value::Object(_) => unreachable!("containers are interned as nodes"),
        })
    }

    fn string(&mut self, text: &str) -> Result<StringId, RawParamsError> {
        if let Some(id) = self.string_ids.get(text) {
            return Ok(*id);
        }

        let id = StringId(u32::try_from(self.string_ends.len()).map_err(|_| RawParamsError::TooLarge)?);
        if id.0 >= ValueRef::PAYLOAD_LIMIT {
            return Err(RawParamsError::TooLarge);
        }
        self.string_bytes.extend_from_slice(text.as_bytes());
        self.string_ends.push(u32::try_from(self.string_bytes.len()).map_err(|_| RawParamsError::TooLarge)?);
        self.string_ids.insert(text.into(), id);

        Ok(id)
    }

    fn optional_string(&mut self, text: Option<&str>) -> Result<u32, RawParamsError> {
        match text {
            Some(text) => Ok(self.string(text)?.0),
            None => Ok(ABSENT),
        }
    }

    /// The node holding `scratch[start..]`, added unless an equal node exists.
    fn node(&mut self, start: usize, scratch: &[u32]) -> Result<NodeId, RawParamsError> {
        let slots = &scratch[start..];
        let mut hasher = std::hash::DefaultHasher::new();
        slots.hash(&mut hasher);
        let hash = hasher.finish();

        let mut candidate = self.node_heads.get(&hash).copied();
        while let Some(id) = candidate {
            if self.node_slots(id) == slots {
                return Ok(id);
            }
            candidate = self.node_chain[id.0 as usize];
        }

        let id = NodeId(u32::try_from(self.node_ends.len()).map_err(|_| RawParamsError::TooLarge)?);
        if id.0 >= ValueRef::PAYLOAD_LIMIT {
            return Err(RawParamsError::TooLarge);
        }
        self.slots.extend_from_slice(slots);
        self.node_ends.push(u32::try_from(self.slots.len()).map_err(|_| RawParamsError::TooLarge)?);
        self.node_chain.push(self.node_heads.insert(hash, id));

        Ok(id)
    }

    fn node_slots(&self, id: NodeId) -> &[u32] {
        let index = id.0 as usize;
        let start = match index {
            0 => 0,
            _ => self.node_ends[index - 1] as usize,
        };

        &self.slots[start..self.node_ends[index] as usize]
    }

    fn entry_string(&self, position: u32, word: usize) -> &str {
        let id = self.entries[position as usize][word] as usize;
        let start = match id {
            0 => 0,
            _ => self.string_ends[id - 1] as usize,
        };
        let bytes = &self.string_bytes[start..self.string_ends[id] as usize];

        std::str::from_utf8(bytes).expect("interned from a str")
    }

    fn entry_id(&self, position: u32) -> u64 {
        let record = &self.entries[position as usize];

        u64::from(record[7]) | (u64::from(record[8]) << 32)
    }

    /// Positions of the entries `keep` accepts, ordered by `compare`.
    fn order(&self, compare: impl Fn(u32, u32) -> Ordering, keep: impl Fn(&[u32; ENTRY_WORDS]) -> bool) -> Vec<u32> {
        let mut positions: Vec<u32> =
            (0..self.entries.len() as u32).filter(|position| keep(&self.entries[*position as usize])).collect();
        positions.sort_by(|left, right| compare(*left, *right));

        positions
    }
}

fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

fn write_words(output: &mut (impl Write + ?Sized), words: &[u32]) -> std::io::Result<()> {
    const CHUNK_WORDS: usize = 16 * 1024;

    let mut buffer = Vec::with_capacity(4 * CHUNK_WORDS);
    for chunk in words.chunks(CHUNK_WORDS) {
        buffer.clear();
        buffer.extend(chunk.iter().flat_map(|word| word.to_le_bytes()));
        output.write_all(&buffer)?;
    }

    Ok(())
}

fn write_longs(output: &mut (impl Write + ?Sized), longs: impl Iterator<Item = [u8; 8]>) -> std::io::Result<()> {
    for bytes in longs {
        output.write_all(&bytes)?;
    }

    Ok(())
}
