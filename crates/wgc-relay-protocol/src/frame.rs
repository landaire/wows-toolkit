use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;

use crate::error::FrameError;

/// A frame is a little-endian `u32` payload length followed by the payload.
pub const FRAME_HEADER_LEN: usize = 4;

/// WGC IPC messages are a few hundred bytes. The limit only bounds what a
/// malformed peer can make the other side allocate.
pub const MAX_MESSAGE_LEN: u32 = 1 << 20;

pub fn encode_frame_header(payload_len: usize) -> Result<[u8; FRAME_HEADER_LEN], FrameError> {
    let len = u32::try_from(payload_len)
        .ok()
        .filter(|len| *len <= MAX_MESSAGE_LEN)
        .ok_or(FrameError::TooLong { len: payload_len as u64, max: MAX_MESSAGE_LEN })?;

    Ok(len.to_le_bytes())
}

pub fn parse_frame_header(header: [u8; FRAME_HEADER_LEN]) -> Result<usize, FrameError> {
    let len = u32::from_le_bytes(header);
    if len > MAX_MESSAGE_LEN {
        return Err(FrameError::TooLong { len: u64::from(len), max: MAX_MESSAGE_LEN });
    }

    Ok(len as usize)
}

pub fn write_frame<W>(writer: &mut W, payload: &[u8]) -> Result<(), FrameError>
where
    W: Write,
{
    writer.write_all(&encode_frame_header(payload.len())?)?;
    writer.write_all(payload)?;
    writer.flush()?;

    Ok(())
}

/// Reads one frame. `None` means the peer closed the link between frames.
pub fn read_frame<R>(reader: &mut R) -> Result<Option<Vec<u8>>, FrameError>
where
    R: Read,
{
    let mut header = [0u8; FRAME_HEADER_LEN];
    let mut filled = 0;
    while filled < FRAME_HEADER_LEN {
        match reader.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(FrameError::Io(ErrorKind::UnexpectedEof.into())),
            Ok(count) => filled += count,
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) => return Err(err.into()),
        }
    }

    let mut payload = vec![0u8; parse_frame_header(header)?];
    reader.read_exact(&mut payload)?;

    Ok(Some(payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_read_back_in_order_including_empty() {
        let mut stream = Vec::new();
        write_frame(&mut stream, b"first").unwrap();
        write_frame(&mut stream, b"").unwrap();
        write_frame(&mut stream, b"third").unwrap();

        let mut reader = std::io::Cursor::new(stream);
        let mut frames = Vec::new();
        while let Some(frame) = read_frame(&mut reader).unwrap() {
            frames.push(frame);
        }

        assert_eq!(frames, [b"first".to_vec(), Vec::new(), b"third".to_vec()]);
    }

    #[test]
    fn eof_inside_a_header_is_an_error() {
        let mut reader = std::io::Cursor::new(vec![5u8, 0]);

        let err = read_frame(&mut reader).unwrap_err();

        assert!(matches!(err, FrameError::Io(ref io) if io.kind() == ErrorKind::UnexpectedEof));
    }

    #[test]
    fn oversized_length_is_rejected_before_allocating() {
        let header = (MAX_MESSAGE_LEN + 1).to_le_bytes();

        let err = parse_frame_header(header).unwrap_err();

        assert!(matches!(err, FrameError::TooLong { len, .. } if len == u64::from(MAX_MESSAGE_LEN) + 1));
    }
}
