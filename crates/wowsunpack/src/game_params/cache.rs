//! Versioned on-disk cache for parsed GameParams.
//!
//! Wraps an rkyv-serialized `Vec<Param>` payload with a small header so the
//! reader can detect and reject caches written by an older parser. Bump
//! [`FORMAT_VERSION`] whenever the cached payload changes shape or content:
//! both parser-logic changes that leave the rkyv schema alone (e.g. fixing a
//! bug that silently dropped fields) and changes to the cached types
//! themselves. Schema changes usually also make rkyv validation fail, but
//! that rejection is incidental, so the header check is what the reader is
//! meant to rely on.

use std::fs;
use std::io;
use std::path::Path;

use crate::game_params::types::Param;

const MAGIC: [u8; 4] = *b"WUGP";

/// Bump on any change that invalidates previously-written caches, whether it
/// comes from the parser or from the cached types. New writes always carry
/// the latest version; reads that see an older or unknown version return
/// `None`, prompting the caller to re-parse from the source VFS.
pub const FORMAT_VERSION: u32 = 17;

const HEADER_LEN: usize = MAGIC.len() + std::mem::size_of::<u32>();

/// Encode `params` as a versioned cache byte sequence. Use when the caller
/// stores the bytes somewhere other than a plain file (e.g. content-addressed
/// storage). For direct file writes, prefer [`save`].
pub fn encode(params: &[Param]) -> io::Result<Vec<u8>> {
    let payload = rkyv::to_bytes::<rkyv::rancor::Error>(&params.to_vec())
        .map_err(|e| io::Error::other(format!("rkyv serialize: {e}")))?;
    let mut buf = Vec::with_capacity(HEADER_LEN + payload.len());
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    buf.extend_from_slice(&payload);
    Ok(buf)
}

/// Decode a versioned cache byte sequence into `Vec<Param>`.
///
/// Returns `None` if the magic doesn't match, the version isn't
/// [`FORMAT_VERSION`], or rkyv deserialization fails. The caller should fall
/// back to re-parsing from the source VFS.
pub fn decode(bytes: &[u8]) -> Option<Vec<Param>> {
    if bytes.len() < HEADER_LEN {
        tracing::debug!("game-params cache rejected: byte sequence shorter than header");
        return None;
    }
    if bytes[..MAGIC.len()] != MAGIC {
        tracing::debug!("game-params cache rejected: missing WUGP magic (pre-versioned format)");
        return None;
    }
    let version_bytes: [u8; 4] = bytes[MAGIC.len()..HEADER_LEN].try_into().ok()?;
    let version = u32::from_le_bytes(version_bytes);
    if version != FORMAT_VERSION {
        tracing::debug!(
            file_version = version,
            current = FORMAT_VERSION,
            "game-params cache rejected: format version mismatch"
        );
        return None;
    }
    let payload = &bytes[HEADER_LEN..];
    rkyv::from_bytes::<Vec<Param>, rkyv::rancor::Error>(payload).ok()
}

/// Read a cached `Vec<Param>` from `path`. Thin wrapper over [`decode`].
pub fn load(path: &Path) -> Option<Vec<Param>> {
    let bytes = fs::read(path).ok()?;
    decode(&bytes)
}

/// Write `params` to `path` with the current header.
pub fn save(path: &Path, params: &[Param]) -> io::Result<()> {
    let buf = encode(params)?;
    fs::write(path, &buf)
}

#[cfg(test)]
mod tests {
    use super::FORMAT_VERSION;
    use super::HEADER_LEN;
    use super::MAGIC;
    use super::decode;
    use super::encode;

    /// A header this build did not write, for the rejection cases.
    fn header(magic: &[u8], version: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(magic);
        buf.extend_from_slice(&version.to_le_bytes());
        buf
    }

    #[test]
    fn an_empty_parameter_set_round_trips() {
        let encoded = encode(&[]).expect("encoding an empty set succeeds");
        assert_eq!(&encoded[..MAGIC.len()], &MAGIC, "the magic leads");
        assert_eq!(
            u32::from_le_bytes(encoded[MAGIC.len()..HEADER_LEN].try_into().expect("four version bytes")),
            FORMAT_VERSION,
            "a write always carries the version this build reads"
        );
        assert!(decode(&encoded).is_some(), "what this build wrote, this build reads");
    }

    #[test]
    fn a_cache_from_another_format_version_is_refused() {
        let stale = header(&MAGIC, FORMAT_VERSION - 1);
        assert!(decode(&stale).is_none(), "an older cache is re-parsed rather than misread");
        let ahead = header(&MAGIC, FORMAT_VERSION + 1);
        assert!(decode(&ahead).is_none(), "one written by a newer build is refused too");
    }

    #[test]
    fn something_that_is_not_a_cache_is_refused() {
        assert!(decode(&[]).is_none(), "a byte sequence shorter than the header");
        assert!(decode(&header(b"NOPE", FORMAT_VERSION)).is_none(), "the wrong magic");
        let truncated = {
            let mut encoded = encode(&[]).expect("encoding an empty set succeeds");
            encoded.truncate(HEADER_LEN + 1);
            encoded
        };
        assert!(decode(&truncated).is_none(), "a payload that does not deserialize");
    }
}
