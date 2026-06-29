//! Encode-side helpers: turn encoder backend output packets into muxable
//! samples with the right PTS and keyframe metadata.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread::JoinHandle;

use rootcause::prelude::*;

use crate::codec::EncoderKind;
use crate::codec::VideoCodec;
use crate::encoder::EncodedFrame;
use crate::encoder::EncoderBackend;
use crate::encoder::EncoderConfig;
use crate::encoder::Mode;
use crate::error::VideoError;
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

/// Turn one backend output packet into a muxable sample. `input_index` is the
/// index of the input frame this packet was produced from; AnnexB PTS is keyed
/// to it while AV1 carries its own frame number.
///
/// Returns `None` for a zero-byte packet. The Vulkan encoder can emit a
/// zero-byte skip frame (for instance the first P-frame over the static
/// pre-battle minimap, where the content is identical to the forced keyframe),
/// and muxide rejects an empty sample with `EmptyVideoFrame`. Dropping it keeps
/// the timeline intact: PTS is keyed to the input frame index rather than the
/// emitted-sample count, so the gap left by a skipped frame simply extends the
/// previous frame's on-screen duration instead of compressing the video.
pub fn build_sample(chunk: EncodedFrame, codec: VideoCodec, input_index: usize) -> Option<EncodedSample> {
    let (data, is_keyframe, pts_seconds) = match chunk {
        EncodedFrame::AnnexB(data) => {
            let is_keyframe = is_annexb_keyframe(&data, codec);
            (data, is_keyframe, input_index as f64 / FPS)
        }
        EncodedFrame::Av1Packet(packet) => (packet.data, packet.is_keyframe, packet.input_frameno as f64 / FPS),
    };
    if data.is_empty() {
        return None;
    }
    Some(EncodedSample { data, is_keyframe, pts_seconds })
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

/// What the encoder thread returns once the channel closes: every sample in
/// submission order plus the codec it resolved to.
pub struct EncoderOutput {
    pub samples: Vec<EncodedSample>,
    pub codec: VideoCodec,
}

/// Handle to the background encoder thread. Constructed by [`EncoderWorker::spawn`],
/// fed via [`EncoderWorker::submit`], and consumed by [`EncoderWorker::finish`].
pub struct EncoderWorker {
    sender: mpsc::SyncSender<Vec<u8>>,
    handle: JoinHandle<rootcause::Result<EncoderOutput, VideoError>>,
    encoded_count: Arc<AtomicU64>,
}

impl EncoderWorker {
    /// Spawn the encoder thread. The backend is created on the thread (so it
    /// never crosses a thread boundary) and the resolved codec/kind are handed
    /// back synchronously, preserving the caller's early init-failure behavior.
    pub fn spawn(
        width: u32,
        height: u32,
        codec: VideoCodec,
        mode: Mode,
        config: EncoderConfig,
    ) -> rootcause::Result<(EncoderWorker, VideoCodec, EncoderKind), VideoError> {
        let (init_tx, init_rx) = mpsc::channel::<Result<(VideoCodec, EncoderKind), ()>>();
        let (frame_tx, frame_rx) = mpsc::sync_channel::<Vec<u8>>(FRAME_CHANNEL_CAPACITY);
        let encoded_count = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&encoded_count);

        let handle = std::thread::Builder::new()
            .name("minimap-encoder".to_string())
            .spawn(move || -> rootcause::Result<EncoderOutput, VideoError> {
                let created = match EncoderBackend::create(width, height, codec, mode, config) {
                    Ok(c) => {
                        let _ = init_tx.send(Ok((c.codec, c.kind)));
                        c
                    }
                    Err(e) => {
                        let _ = init_tx.send(Err(()));
                        return Err(e);
                    }
                };
                run_encoder(width, height, created.backend, created.codec, frame_rx, counter)
            })
            .context(VideoError::EncodeFailed)
            .attach("spawning encoder thread")?;

        match init_rx.recv() {
            Ok(Ok((resolved_codec, kind))) => {
                Ok((EncoderWorker { sender: frame_tx, handle, encoded_count }, resolved_codec, kind))
            }
            Ok(Err(())) => match handle.join() {
                Ok(Err(e)) => Err(e),
                _ => Err(report!(VideoError::EncodeFailed).attach("encoder thread failed to initialize")),
            },
            Err(_) => Err(report!(VideoError::EncodeFailed).attach("encoder thread disconnected during init")),
        }
    }

    /// Submit one rasterized RGB frame. Blocks when the channel is full
    /// (backpressure). Returns Err if the encoder thread has stopped; the real
    /// cause surfaces from [`EncoderWorker::finish`].
    pub fn submit(&self, frame: Vec<u8>) -> rootcause::Result<(), VideoError> {
        self.sender.send(frame).map_err(|_| report!(VideoError::EncodeFailed).attach("encoder thread stopped"))
    }

    /// Number of frames the encoder thread has consumed so far.
    pub fn encoded_count(&self) -> u64 {
        self.encoded_count.load(Ordering::Relaxed)
    }

    /// Close the channel, join the thread, and return the accumulated samples
    /// (or the first encode error the thread hit).
    pub fn finish(self) -> rootcause::Result<EncoderOutput, VideoError> {
        drop(self.sender);
        match self.handle.join() {
            Ok(result) => result,
            Err(_) => Err(report!(VideoError::EncodeFailed).attach("encoder thread panicked")),
        }
    }
}

/// Encoder thread body: encode each received frame in order, accumulate
/// samples, then drain the backend (AV1 flush) when the channel closes.
fn run_encoder(
    width: u32,
    height: u32,
    mut backend: EncoderBackend,
    codec: VideoCodec,
    frame_rx: mpsc::Receiver<Vec<u8>>,
    encoded_count: Arc<AtomicU64>,
) -> rootcause::Result<EncoderOutput, VideoError> {
    let mut samples: Vec<EncodedSample> = Vec::new();
    let mut input_index: usize = 0;
    while let Ok(frame) = frame_rx.recv() {
        for chunk in backend.encode_frame(&frame, width, height)? {
            if let Some(sample) = build_sample(chunk, codec, input_index) {
                samples.push(sample);
            }
        }
        input_index += 1;
        encoded_count.fetch_add(1, Ordering::Relaxed);
    }
    for chunk in backend.finish()? {
        if let Some(sample) = build_sample(chunk, codec, input_index) {
            samples.push(sample);
        }
    }
    Ok(EncoderOutput { samples, codec })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annexb_idr_nal_is_keyframe() {
        // Start code 00 00 00 01 then an H.264 NAL with type 5 (IDR): 0x65.
        let data = [0x00, 0x00, 0x00, 0x01, 0x65, 0x11, 0x22];
        let sample = build_sample(EncodedFrame::AnnexB(data.to_vec()), VideoCodec::H264, 30).unwrap();
        assert!(sample.is_keyframe);
        assert_eq!(sample.pts_seconds, 30.0 / FPS);
    }

    #[test]
    fn annexb_non_idr_nal_is_not_keyframe() {
        // NAL type 1 (non-IDR slice): 0x41.
        let data = [0x00, 0x00, 0x00, 0x01, 0x41, 0x00];
        let sample = build_sample(EncodedFrame::AnnexB(data.to_vec()), VideoCodec::H264, 0).unwrap();
        assert!(!sample.is_keyframe);
        assert_eq!(sample.pts_seconds, 0.0);
    }

    #[test]
    fn empty_annexb_packet_is_dropped() {
        // A zero-byte skip frame from the GPU encoder must not become a sample;
        // muxide rejects empty samples with EmptyVideoFrame.
        assert!(build_sample(EncodedFrame::AnnexB(Vec::new()), VideoCodec::H264, 1).is_none());
        assert!(build_sample(EncodedFrame::AnnexB(Vec::new()), VideoCodec::H265, 7).is_none());
    }

    #[test]
    fn annexb_pts_is_keyed_to_input_index() {
        // Dropping frame 1 leaves frames 0 and 2; their PTS must stay at the
        // input cadence (0 and 2/FPS) so the skipped frame extends frame 0's
        // duration instead of compressing the timeline.
        let f0 = build_sample(EncodedFrame::AnnexB(vec![0x00, 0x00, 0x01, 0x65]), VideoCodec::H264, 0).unwrap();
        let f2 = build_sample(EncodedFrame::AnnexB(vec![0x00, 0x00, 0x01, 0x41]), VideoCodec::H264, 2).unwrap();
        assert_eq!(f0.pts_seconds, 0.0);
        assert_eq!(f2.pts_seconds, 2.0 / FPS);
    }

    use crate::encoder::EncoderBackend;
    use crate::encoder::EncoderConfig;
    use crate::encoder::Mode;

    /// 64x64 solid-color RGB frames, deterministic per index.
    fn synthetic_frames(n: usize) -> Vec<Vec<u8>> {
        (0..n)
            .map(|i| {
                let v = (i * 16) as u8;
                vec![v; 64 * 64 * 3]
            })
            .collect()
    }

    #[cfg(feature = "cpu")]
    #[test]
    fn worker_matches_synchronous_encode() {
        let frames = synthetic_frames(10);

        // Reference: encode synchronously with a directly created backend.
        let mut created =
            EncoderBackend::create(64, 64, VideoCodec::H264, Mode::ForceCpu, EncoderConfig::default()).unwrap();
        let mut reference: Vec<EncodedSample> = Vec::new();
        let mut input_index = 0;
        for f in &frames {
            for chunk in created.backend.encode_frame(f, 64, 64).unwrap() {
                if let Some(sample) = build_sample(chunk, VideoCodec::H264, input_index) {
                    reference.push(sample);
                }
            }
            input_index += 1;
        }
        for chunk in created.backend.finish().unwrap() {
            if let Some(sample) = build_sample(chunk, VideoCodec::H264, input_index) {
                reference.push(sample);
            }
        }

        // Async: same frames through the worker.
        let (worker, codec, _kind) =
            EncoderWorker::spawn(64, 64, VideoCodec::H264, Mode::ForceCpu, EncoderConfig::default()).unwrap();
        assert_eq!(codec, VideoCodec::H264);
        for f in &frames {
            worker.submit(f.clone()).unwrap();
        }
        let output = worker.finish().unwrap();

        assert_eq!(output.samples.len(), reference.len());
        for (a, b) in output.samples.iter().zip(reference.iter()) {
            assert_eq!(a.data, b.data, "encoded bytes differ");
            assert_eq!(a.is_keyframe, b.is_keyframe);
            assert_eq!(a.pts_seconds, b.pts_seconds);
        }
    }

    #[test]
    fn worker_init_failure_propagates() {
        // CPU H.265 is unsupported per check_encoder(); spawning must return Err,
        // not hang or panic.
        let result = EncoderWorker::spawn(64, 64, VideoCodec::H265, Mode::ForceCpu, EncoderConfig::default());
        assert!(result.is_err(), "expected init failure for CPU H.265");
    }
}
