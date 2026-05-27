//! CPU AV1 encoder backend powered by rav1e.

use rav1e::Config;
use rav1e::Context;
use rav1e::EncoderConfig;
use rav1e::EncoderStatus;
use rav1e::Packet;
use rav1e::config::SpeedSettings;
use rav1e::prelude::ChromaSampling;
use rav1e::prelude::FrameType;
use rav1e::prelude::Rational;
use rootcause::prelude::*;
use yuvutils_rs::BufferStoreMut;
use yuvutils_rs::YuvConversionMode;
use yuvutils_rs::YuvPlanarImageMut;
use yuvutils_rs::YuvRange;
use yuvutils_rs::YuvStandardMatrix;

use crate::error::VideoError;
use crate::video::FPS;

/// One emitted AV1 frame as OBUs, ready for muxing as a single MP4 sample.
pub struct Av1Packet {
    pub data: Vec<u8>,
    pub input_frameno: u64,
    pub is_keyframe: bool,
}

pub struct CpuAv1Encoder {
    ctx: Context<u8>,
    width: u32,
    height: u32,
    y_buf: Vec<u8>,
    u_buf: Vec<u8>,
    v_buf: Vec<u8>,
    frames_sent: u64,
}

impl CpuAv1Encoder {
    pub fn new(width: u32, height: u32) -> rootcause::Result<Self, VideoError> {
        if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(report!(VideoError::EncoderInit(format!(
                "AV1 requires even dimensions; got {width}x{height}"
            ))));
        }

        // Speed preset 6 trades ~2x encode time vs preset 8 for noticeably
        // better fidelity on small UI text in the HUD/HP-bars.
        let mut cfg = EncoderConfig::with_speed_preset(6);
        cfg.width = width as usize;
        cfg.height = height as usize;
        cfg.bit_depth = 8;
        cfg.chroma_sampling = ChromaSampling::Cs420;
        cfg.time_base = Rational { num: 1, den: FPS as u64 };
        cfg.min_key_frame_interval = 0;
        cfg.max_key_frame_interval = FPS as u64;
        cfg.low_latency = true;
        cfg.speed_settings = SpeedSettings::from_preset(6);
        // Lower quantizer = higher quality. rav1e's default is 100; 60 lands
        // around "visually lossless" for our HUD-heavy frames without
        // ballooning the output too much.
        cfg.quantizer = 60;
        // Workaround for muxide 0.2.5: its AV1 sequence-header parser reads
        // `decoder_model_info_present_flag` unconditionally instead of guarding
        // it on `timing_info_present_flag` (AV1 spec 5.5.1). Forcing
        // timing-info on makes our bitstream traverse the parser's tested
        // path. Track upstream fix and drop once a fixed muxide ships.
        cfg.enable_timing_info = true;

        let config = Config::new().with_encoder_config(cfg);
        let ctx: Context<u8> = config
            .new_context()
            .map_err(|e| report!(VideoError::EncoderInit(format!("rav1e context: {e:?}"))))?;

        let y_size = (width * height) as usize;
        let chroma_size = ((width / 2) * (height / 2)) as usize;

        Ok(Self {
            ctx,
            width,
            height,
            y_buf: vec![0u8; y_size],
            u_buf: vec![0u8; chroma_size],
            v_buf: vec![0u8; chroma_size],
            frames_sent: 0,
        })
    }

    pub fn container_sequence_header(&self) -> Vec<u8> {
        self.ctx.container_sequence_header()
    }

    pub fn encode_frame(&mut self, rgb: &[u8]) -> rootcause::Result<Vec<Av1Packet>, VideoError> {
        self.rgb_to_i420(rgb)?;

        let chroma_w = (self.width / 2) as usize;
        let mut frame = self.ctx.new_frame();
        frame.planes[0].copy_from_raw_u8(&self.y_buf, self.width as usize, 1);
        frame.planes[1].copy_from_raw_u8(&self.u_buf, chroma_w, 1);
        frame.planes[2].copy_from_raw_u8(&self.v_buf, chroma_w, 1);

        self.ctx
            .send_frame(frame)
            .map_err(|e| report!(VideoError::EncodeFailed(format!("rav1e send_frame: {e:?}"))))?;
        self.frames_sent += 1;

        self.drain(false)
    }

    pub fn flush(&mut self) -> rootcause::Result<Vec<Av1Packet>, VideoError> {
        self.ctx.flush();
        self.drain(true)
    }

    fn drain(&mut self, until_eof: bool) -> rootcause::Result<Vec<Av1Packet>, VideoError> {
        let mut out = Vec::new();
        loop {
            match self.ctx.receive_packet() {
                Ok(packet) => out.push(into_av1_packet(packet)),
                Err(EncoderStatus::Encoded) => continue,
                Err(EncoderStatus::NeedMoreData) => {
                    if until_eof {
                        continue;
                    }
                    return Ok(out);
                }
                Err(EncoderStatus::LimitReached) => return Ok(out),
                Err(other) => {
                    return Err(report!(VideoError::EncodeFailed(format!("rav1e receive_packet: {other:?}"))));
                }
            }
        }
    }

    fn rgb_to_i420(&mut self, rgb: &[u8]) -> rootcause::Result<(), VideoError> {
        let width = self.width;
        let height = self.height;
        let chroma_w = width / 2;
        let chroma_h = height / 2;

        let mut image = YuvPlanarImageMut {
            y_plane: BufferStoreMut::Borrowed(&mut self.y_buf),
            y_stride: width,
            u_plane: BufferStoreMut::Borrowed(&mut self.u_buf),
            u_stride: chroma_w,
            v_plane: BufferStoreMut::Borrowed(&mut self.v_buf),
            v_stride: chroma_w,
            width,
            height,
        };

        yuvutils_rs::rgb_to_yuv420(
            &mut image,
            rgb,
            width * 3,
            YuvRange::Limited,
            YuvStandardMatrix::Bt709,
            YuvConversionMode::Balanced,
        )
        .map_err(|e| report!(VideoError::EncodeFailed(format!("RGB->I420: {e:?}"))))?;
        let _ = chroma_h;
        Ok(())
    }
}

fn into_av1_packet(p: Packet<u8>) -> Av1Packet {
    Av1Packet {
        is_keyframe: matches!(p.frame_type, FrameType::KEY),
        input_frameno: p.input_frameno,
        data: p.data,
    }
}
