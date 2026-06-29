//! OpenH264 CPU-based H.264 encoder backend.
//!
//! Software encoder fallback for systems without GPU encoding support.

use openh264::OpenH264API;
use openh264::encoder::BitRate;
use openh264::encoder::Complexity;
use openh264::encoder::Encoder;
use openh264::encoder::EncoderConfig;
use openh264::encoder::FrameRate;
use openh264::formats::RgbSliceU8;
use openh264::formats::YUVBuffer;
use rootcause::prelude::*;

use crate::encoder::EncoderConfig as Cfg;
use crate::error::VideoError;
use crate::video::FPS;

pub struct CpuEncoder {
    encoder: Encoder,
}

impl CpuEncoder {
    pub fn new(config: Cfg) -> rootcause::Result<Self, VideoError> {
        let bitrate = config.h264_bitrate_bps();
        let config = EncoderConfig::new()
            .max_frame_rate(FrameRate::from_hz(FPS as f32))
            .rate_control_mode(openh264::encoder::RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate))
            .complexity(Complexity::High);
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
            .context(VideoError::EncoderInit)
            .attach("creating openh264 H.264 encoder")?;
        Ok(Self { encoder })
    }

    pub fn encode_frame(&mut self, rgb: &[u8], width: usize, height: usize) -> rootcause::Result<Vec<u8>, VideoError> {
        let rgb_slice = RgbSliceU8::new(rgb, (width, height));
        let yuv = YUVBuffer::from_rgb_source(rgb_slice);
        let bitstream = self.encoder.encode(&yuv).context(VideoError::EncodeFailed).attach("openh264 encode")?;
        Ok(bitstream.to_vec())
    }
}
