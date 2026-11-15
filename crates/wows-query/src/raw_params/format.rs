//! Layout of `params.bin`, read in place from a memory map.
//!
//! A header names ten sections, each an array of little-endian words.
//! Strings are interned. Arrays and objects are nodes: runs of slot words in
//! one arena, each distinct run stored once however often it occurs. A node's
//! children are nodes written before it, so every walk ends. A value is one
//! word: a tag in the low three bits and a payload above them.
//!
//! Every read is bounds checked, so a damaged file yields [`StoreDamage`],
//! never undefined behavior.

use std::ops::Range;

use crate::error::StoreDamage;

pub(crate) const MAGIC: [u8; 4] = *b"WQRP";

/// Bump on any change to the layout or to what is stored; files of any
/// other version are rebuilt.
pub const FORMAT_VERSION: u32 = 2;

/// Marks an absent string or level in an entry record.
pub(crate) const ABSENT: u32 = u32::MAX;

/// Words of one entry record: name, root, index, type, species, nation,
/// level, and the id's low and high halves.
pub(crate) const ENTRY_WORDS: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    /// UTF-8 text of every string, back to back.
    StringBytes,
    /// End offset of each string in `StringBytes`.
    StringEnds,
    /// Integers outside the inline range, as 8-byte words.
    Ints,
    /// Floats, as 8-byte words.
    Floats,
    /// End of each node's slots, in words.
    NodeEnds,
    /// Array items, or object keys and values in turn.
    Slots,
    Entries,
    /// Entry positions ordered by name.
    ByName,
    /// Entry positions ordered by upper-case index.
    ByIndex,
    /// Entry positions ordered by id.
    ById,
}

impl Section {
    pub(crate) const ALL: [Self; 10] = [
        Self::StringBytes,
        Self::StringEnds,
        Self::Ints,
        Self::Floats,
        Self::NodeEnds,
        Self::Slots,
        Self::Entries,
        Self::ByName,
        Self::ByIndex,
        Self::ById,
    ];

    /// Bytes per element; a section's length is a multiple of it.
    pub(crate) fn element_len(self) -> usize {
        match self {
            Self::StringBytes => 1,
            Self::Ints | Self::Floats => 8,
            Self::Entries => 4 * ENTRY_WORDS,
            Self::StringEnds | Self::NodeEnds | Self::Slots | Self::ByName | Self::ByIndex | Self::ById => 4,
        }
    }
}

/// Magic, format version, build, a reserved word, then each section's
/// offset and length as 8-byte words.
pub(crate) const HEADER_LEN: usize = 16 + 16 * Section::ALL.len();

/// Sections start on 8-byte boundaries.
pub(crate) const SECTION_ALIGN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct StringId(pub(crate) u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct NodeId(pub(crate) u32);

/// Position of an entry in GameParams dictionary order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct EntryPosition(pub(crate) u32);

impl EntryPosition {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// The kind of a value word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tag {
    /// Payload 0 is null, 1 false, 2 true.
    Special = 0,
    /// Payload is a 29-bit two's complement integer.
    SmallInt = 1,
    Int = 2,
    Float = 3,
    Str = 4,
    Array = 5,
    Object = 6,
}

impl Tag {
    fn from_bits(bits: u32) -> Option<Self> {
        Some(match bits {
            0 => Self::Special,
            1 => Self::SmallInt,
            2 => Self::Int,
            3 => Self::Float,
            4 => Self::Str,
            5 => Self::Array,
            6 => Self::Object,
            _ => return None,
        })
    }
}

/// One value: a tag and a payload, or an index into the table it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ValueRef(pub(crate) u32);

impl ValueRef {
    const TAG_BITS: u32 = 3;
    /// Payloads are below this.
    pub(crate) const PAYLOAD_LIMIT: u32 = 1 << (32 - Self::TAG_BITS);
    /// Integers in this range are stored in the word itself.
    pub(crate) const SMALL_INT: Range<i64> = -(1 << 28)..(1 << 28);

    pub(crate) const NULL: Self = Self(Tag::Special as u32);
    pub(crate) const FALSE: Self = Self((1 << Self::TAG_BITS) | Tag::Special as u32);
    pub(crate) const TRUE: Self = Self((2 << Self::TAG_BITS) | Tag::Special as u32);

    /// `None` when `payload` does not fit.
    pub(crate) fn new(tag: Tag, payload: u32) -> Option<Self> {
        (payload < Self::PAYLOAD_LIMIT).then_some(Self((payload << Self::TAG_BITS) | tag as u32))
    }

    pub(crate) fn small_int(value: i64) -> Option<Self> {
        if !Self::SMALL_INT.contains(&value) {
            return None;
        }

        // Two's complement in the payload bits; decoding sign-extends.
        Self::new(Tag::SmallInt, (value as u32) & (Self::PAYLOAD_LIMIT - 1))
    }

    pub(crate) fn tag(self) -> Option<Tag> {
        Tag::from_bits(self.0 & ((1 << Self::TAG_BITS) - 1))
    }

    pub(crate) fn payload(self) -> u32 {
        self.0 >> Self::TAG_BITS
    }

    fn small_int_value(self) -> i64 {
        i64::from((self.0 as i32) >> Self::TAG_BITS)
    }
}

/// A decoded value. Strings and nodes borrow the store.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Value<'a> {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(&'a str),
    Array(Node<'a>),
    Object(Node<'a>),
}

/// The slots of one array or object.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Node<'a> {
    id: NodeId,
    slots: &'a [u8],
}

impl<'a> Node<'a> {
    pub(crate) fn id(self) -> NodeId {
        self.id
    }

    pub(crate) fn slot_count(self) -> usize {
        self.slots.len() / 4
    }

    /// Slot `index`, which must be below [`Self::slot_count`].
    pub(crate) fn slot(self, index: usize) -> u32 {
        word(self.slots, index).expect("slot index below slot_count")
    }
}

/// Section boundaries of a store, checked against its length.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub(crate) build: u32,
    sections: [Range<usize>; Section::ALL.len()],
}

/// Why a header was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderError {
    Version { found: u32 },
    Damaged(StoreDamage),
}

impl Layout {
    pub(crate) fn new(build: u32, sections: [Range<usize>; Section::ALL.len()]) -> Self {
        Self { build, sections }
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, HeaderError> {
        let damaged = |damage| HeaderError::Damaged(damage);
        if bytes.len() < HEADER_LEN || bytes[..4] != MAGIC {
            return Err(damaged(StoreDamage::Header));
        }
        let version = word(bytes, 1).ok_or(damaged(StoreDamage::Header))?;
        if version != FORMAT_VERSION {
            return Err(HeaderError::Version { found: version });
        }
        let build = word(bytes, 2).ok_or(damaged(StoreDamage::Header))?;

        let mut sections: [Range<usize>; Section::ALL.len()] = Default::default();
        for (position, section) in Section::ALL.into_iter().enumerate() {
            let at = 16 + 16 * position;
            let offset = long(bytes, at).ok_or(damaged(StoreDamage::Header))?;
            let len = long(bytes, at + 8).ok_or(damaged(StoreDamage::Header))?;
            let range = usize::try_from(offset)
                .ok()
                .zip(usize::try_from(len).ok())
                .and_then(|(offset, len)| Some(offset..offset.checked_add(len)?))
                .filter(|range| range.start >= HEADER_LEN && range.end <= bytes.len())
                .filter(|range| range.start % SECTION_ALIGN == 0 && range.len() % section.element_len() == 0)
                .ok_or(damaged(StoreDamage::Section))?;
            sections[position] = range;
        }

        Ok(Self { build, sections })
    }

    /// The header bytes of this layout.
    pub(crate) fn header(&self) -> Vec<u8> {
        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(&MAGIC);
        header.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        header.extend_from_slice(&self.build.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        for range in &self.sections {
            header.extend_from_slice(&(range.start as u64).to_le_bytes());
            header.extend_from_slice(&(range.len() as u64).to_le_bytes());
        }

        header
    }

    pub(crate) fn range(&self, section: Section) -> Range<usize> {
        self.sections[section as usize].clone()
    }
}

/// Typed reads over a store's bytes.
#[derive(Clone, Copy)]
pub(crate) struct Tables<'a> {
    bytes: &'a [u8],
    layout: &'a Layout,
}

impl<'a> Tables<'a> {
    /// `bytes` must be the bytes `layout` was parsed from.
    pub(crate) fn new(bytes: &'a [u8], layout: &'a Layout) -> Self {
        Self { bytes, layout }
    }

    fn section(self, section: Section) -> &'a [u8] {
        &self.bytes[self.layout.range(section)]
    }

    pub(crate) fn count(self, section: Section) -> usize {
        self.layout.range(section).len() / section.element_len()
    }

    pub(crate) fn word(self, section: Section, index: usize) -> Option<u32> {
        word(self.section(section), index)
    }

    pub(crate) fn string(self, id: StringId) -> Result<&'a str, StoreDamage> {
        let index = id.0 as usize;
        let end = self.word(Section::StringEnds, index).ok_or(StoreDamage::String)? as usize;
        let start = match index {
            0 => 0,
            _ => self.word(Section::StringEnds, index - 1).ok_or(StoreDamage::String)? as usize,
        };
        let bytes = self.section(Section::StringBytes).get(start..end).ok_or(StoreDamage::String)?;

        std::str::from_utf8(bytes).map_err(|_| StoreDamage::String)
    }

    /// The string a record word names, `None` for [`ABSENT`].
    pub(crate) fn optional_string(self, word: u32) -> Result<Option<&'a str>, StoreDamage> {
        match word {
            ABSENT => Ok(None),
            id => self.string(StringId(id)).map(Some),
        }
    }

    pub(crate) fn value(self, value: ValueRef) -> Result<Value<'a>, StoreDamage> {
        let payload = value.payload();
        let index = payload as usize;

        Ok(match value.tag().ok_or(StoreDamage::Value)? {
            Tag::Special => match payload {
                0 => Value::Null,
                1 => Value::Bool(false),
                2 => Value::Bool(true),
                _ => return Err(StoreDamage::Value),
            },
            Tag::SmallInt => Value::Int(value.small_int_value()),
            Tag::Int => Value::Int(i64::from_le_bytes(self.long_bytes(Section::Ints, index)?)),
            Tag::Float => Value::Float(f64::from_le_bytes(self.long_bytes(Section::Floats, index)?)),
            Tag::Str => Value::Str(self.string(StringId(payload))?),
            Tag::Array => Value::Array(self.node(NodeId(payload))?),
            Tag::Object => {
                let node = self.node(NodeId(payload))?;
                if node.slot_count() % 2 != 0 {
                    return Err(StoreDamage::Node);
                }
                Value::Object(node)
            }
        })
    }

    /// A value in a slot of `parent`. Nodes must precede their parent, which
    /// rules out cycles.
    pub(crate) fn child(self, parent: NodeId, value: ValueRef) -> Result<Value<'a>, StoreDamage> {
        let is_node = matches!(value.tag(), Some(Tag::Array | Tag::Object));
        if is_node && value.payload() >= parent.0 {
            return Err(StoreDamage::Node);
        }

        self.value(value)
    }

    fn node(self, id: NodeId) -> Result<Node<'a>, StoreDamage> {
        let index = id.0 as usize;
        let end = self.word(Section::NodeEnds, index).ok_or(StoreDamage::Node)? as usize;
        let start = match index {
            0 => 0,
            _ => self.word(Section::NodeEnds, index - 1).ok_or(StoreDamage::Node)? as usize,
        };
        let slots = self
            .section(Section::Slots)
            .get(start.checked_mul(4).ok_or(StoreDamage::Node)?..end.checked_mul(4).ok_or(StoreDamage::Node)?)
            .ok_or(StoreDamage::Node)?;

        Ok(Node { id, slots })
    }

    fn long_bytes(self, section: Section, index: usize) -> Result<[u8; 8], StoreDamage> {
        self.section(section)
            .get(index * 8..index * 8 + 8)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(StoreDamage::Value)
    }
}

/// Word `index` of `bytes`, little-endian.
pub(crate) fn word(bytes: &[u8], index: usize) -> Option<u32> {
    let start = index.checked_mul(4)?;
    let bytes = bytes.get(start..start + 4)?;

    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn long(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}
