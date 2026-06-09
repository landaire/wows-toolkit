//! Encode-side helpers: turn encoder backend output packets into muxable
//! samples with the right PTS and keyframe metadata.

use crate::codec::VideoCodec;
use crate::encoder::EncodedFrame;
use crate::video::FPS;

/// Bounded channel depth between the producer and the encoder thread. Small on
/// purpose: it provides backpressure (the producer blocks instead of buffering
/// the whole match) while keeping both stages busy. Memory is about
/// `CAP * width * height * 3` bytes.
pub const FRAME_CHANNEL_CAPACITY: usize = 4;

/// One encoded frame plus the metadata muxide needs.
pub struct EncodedSample {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
    pub pts_seconds: f64,
}

/// Turn one backend output packet into a muxable sample. `next_index` is the
/// number of samples already produced; AnnexB PTS is index-based while AV1
/// carries its own frame number.
pub fn build_sample(chunk: EncodedFrame, codec: VideoCodec, next_index: usize) -> EncodedSample {
    match chunk {
        EncodedFrame::AnnexB(data) => {
            let pts = next_index as f64 / FPS;
            let is_keyframe = is_annexb_keyframe(&data, codec);
            EncodedSample { data, is_keyframe, pts_seconds: pts }
        }
        EncodedFrame::Av1Packet(packet) => {
            let pts = packet.input_frameno as f64 / FPS;
            EncodedSample { data: packet.data, is_keyframe: packet.is_keyframe, pts_seconds: pts }
        }
    }
}

/// Walk an Annex B byte stream looking for a NAL that signals a random-access
/// point for the given codec. AV1 OBUs are handled separately and never reach
/// this function.
fn is_annexb_keyframe(data: &[u8], codec: VideoCodec) -> bool {
    for nal in parse_annexb_nals(data) {
        if nal.is_empty() {
            continue;
        }
        match codec {
            VideoCodec::H264 => {
                let nal_type = nal[0] & 0x1f;
                if nal_type == 5 {
                    return true;
                }
            }
            VideoCodec::H265 => {
                let nal_type = (nal[0] >> 1) & 0x3f;
                if (16..=21).contains(&nal_type) {
                    return true;
                }
            }
            VideoCodec::Av1 => return false,
        }
    }
    false
}

/// Parse an Annex B byte stream into individual NAL units (without start codes).
fn parse_annexb_nals(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if i + 2 < data.len() && data[i] == 0 && data[i + 1] == 0 {
            let start = if i + 3 < data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                i + 4
            } else if data[i + 2] == 1 {
                i + 3
            } else {
                i += 1;
                continue;
            };
            let mut end = start;
            while end < data.len() {
                if end + 2 < data.len()
                    && data[end] == 0
                    && data[end + 1] == 0
                    && (data[end + 2] == 1 || (end + 3 < data.len() && data[end + 2] == 0 && data[end + 3] == 1))
                {
                    break;
                }
                end += 1;
            }
            if end > start {
                nals.push(&data[start..end]);
            }
            i = end;
        } else {
            i += 1;
        }
    }
    nals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annexb_idr_nal_is_keyframe() {
        // Start code 00 00 00 01 then an H.264 NAL with type 5 (IDR): 0x65.
        let data = [0x00, 0x00, 0x00, 0x01, 0x65, 0x11, 0x22];
        let sample = build_sample(EncodedFrame::AnnexB(data.to_vec()), VideoCodec::H264, 30);
        assert!(sample.is_keyframe);
        assert_eq!(sample.pts_seconds, 30.0 / FPS);
    }

    #[test]
    fn annexb_non_idr_nal_is_not_keyframe() {
        // NAL type 1 (non-IDR slice): 0x41.
        let data = [0x00, 0x00, 0x00, 0x01, 0x41, 0x00];
        let sample = build_sample(EncodedFrame::AnnexB(data.to_vec()), VideoCodec::H264, 0);
        assert!(!sample.is_keyframe);
        assert_eq!(sample.pts_seconds, 0.0);
    }
}
