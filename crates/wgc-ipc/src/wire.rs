//! cereal `BinaryArchive` primitives as `wgc_api.dll` writes them: little
//! endian integers, `u64` length prefixes, `std::wstring` as UTF-16LE code
//! units and `std::string` as raw bytes.

use zeroize::Zeroizing;

use crate::error::CodecError;

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, offset: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn take(&mut self, count: u64) -> Result<&'a [u8], CodecError> {
        let remaining = self.remaining() as u64;
        if count > remaining {
            return Err(CodecError::Truncated { offset: self.bytes.len(), needed: count - remaining });
        }

        let start = self.offset;
        self.offset += count as usize;

        Ok(&self.bytes[start..self.offset])
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N as u64)?);

        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.array::<1>()?[0])
    }

    /// cereal stores `bool` as one byte; any nonzero byte loads as true.
    pub(crate) fn bool(&mut self) -> Result<bool, CodecError> {
        Ok(self.u8()? != 0)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub(crate) fn wstring(&mut self) -> Result<String, CodecError> {
        let units = self.u64()?;
        let offset = self.offset;
        let bytes = self.take(units.saturating_mul(2))?;
        let units = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair));

        char::decode_utf16(units).collect::<Result<String, _>>().map_err(|_| CodecError::InvalidUtf16 { offset })
    }

    pub(crate) fn string(&mut self) -> Result<String, CodecError> {
        let len = self.u64()?;
        let offset = self.offset;
        let bytes = self.take(len)?;

        String::from_utf8(bytes.to_vec()).map_err(|_| CodecError::InvalidUtf8 { offset })
    }

    pub(crate) fn wstring_vec(&mut self) -> Result<Vec<String>, CodecError> {
        let count = self.u64()?;
        // Every element takes at least its 8-byte length, which bounds the
        // allocation by the message size.
        if count > self.remaining() as u64 / 8 {
            return Err(CodecError::Truncated {
                offset: self.bytes.len(),
                needed: count.saturating_mul(8) - self.remaining() as u64,
            });
        }

        (0..count).map(|_| self.wstring()).collect()
    }
}

/// The buffer holds tokens, so it is wiped on drop, including when an encode fails midway.
pub(crate) struct Writer {
    bytes: Zeroizing<Vec<u8>>,
}

impl Writer {
    pub(crate) fn new() -> Writer {
        Writer { bytes: Zeroizing::new(Vec::new()) }
    }

    pub(crate) fn into_bytes(self) -> Zeroizing<Vec<u8>> {
        self.bytes
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    pub(crate) fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn wstring(&mut self, value: &str) {
        let units = value.encode_utf16().count();
        self.u64(units as u64);
        for unit in value.encode_utf16() {
            self.u16(unit);
        }
    }

    pub(crate) fn string(&mut self, value: &str) {
        self.u64(value.len() as u64);
        self.bytes.extend_from_slice(value.as_bytes());
    }

    pub(crate) fn wstring_vec(&mut self, values: &[String]) {
        self.u64(values.len() as u64);
        for value in values {
            self.wstring(value);
        }
    }
}
