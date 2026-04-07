//! Vulkan Video H.264 encoder backend.
//!
//! Uses vk-video for GPU-accelerated encoding on Linux/Windows.

use std::num::NonZeroU32;

use rootcause::prelude::*;
use vk_video::BytesEncoder;
use vk_video::InputFrame;
use vk_video::RawFrameData;
use vk_video::VulkanInstance;
use vk_video::parameters::EncoderParameters;
use vk_video::parameters::RateControl;
use vk_video::parameters::VideoParameters;
use vk_video::parameters::VulkanAdapterDescriptor;
use vk_video::parameters::VulkanDeviceDescriptor;
use yuvutils_rs::BufferStoreMut;
use yuvutils_rs::YuvBiPlanarImageMut;
use yuvutils_rs::YuvConversionMode;
use yuvutils_rs::YuvRange;
use yuvutils_rs::YuvStandardMatrix;

use crate::error::VideoError;
use crate::video::FPS;

pub struct VulkanEncoder {
    encoder: BytesEncoder,
    nv12_buf: Vec<u8>,
    frame_count: u64,
}

impl VulkanEncoder {
    pub fn new(width: u32, height: u32) -> rootcause::Result<Self, VideoError> {
        let instance = VulkanInstance::new()
            .map_err(|e| report!(VideoError::EncoderInit(format!("Vulkan init failed: {e:?}"))))?;
        let adapter = instance
            .create_adapter(&VulkanAdapterDescriptor { supports_encoding: true, ..Default::default() })
            .map_err(|e| report!(VideoError::EncoderInit(format!("No Vulkan adapter: {e:?}"))))?;

        let device = adapter
            .create_device(&VulkanDeviceDescriptor {
                wgpu_features: wgpu::Features::empty(),
                wgpu_experimental_features: wgpu::ExperimentalFeatures::disabled(),
                wgpu_limits: wgpu::Limits { max_immediate_size: 128, ..Default::default() },
            })
            .map_err(|e| report!(VideoError::EncoderInit(format!("Vulkan device creation failed: {e:?}"))))?;

        let output_params = device
            .encoder_output_parameters_high_quality(RateControl::VariableBitrate {
                average_bitrate: 20_000_000,
                max_bitrate: 40_000_000,
                virtual_buffer_size: std::time::Duration::from_secs(2),
            })
            .map_err(|e| report!(VideoError::EncoderInit(format!("Encoder params failed: {e:?}"))))?;

        let params = EncoderParameters {
            input_parameters: VideoParameters {
                width: NonZeroU32::new(width).expect("non-zero width"),
                height: NonZeroU32::new(height).expect("non-zero height"),
                target_framerate: (FPS as u32).into(),
            },
            output_parameters: output_params,
        };

        let encoder = device
            .create_bytes_encoder(params)
            .map_err(|e| report!(VideoError::EncoderInit(format!("Encoder creation failed: {e:?}"))))?;

        let nv12_size = (width as usize) * (height as usize) * 3 / 2;

        Ok(Self { encoder, nv12_buf: vec![0u8; nv12_size], frame_count: 0 })
    }

    pub fn encode_frame(&mut self, rgb: &[u8], width: u32, height: u32) -> rootcause::Result<Vec<u8>, VideoError> {
        let y_len = (width * height) as usize;
        let uv_len = (width * height / 2) as usize;

        // Split nv12_buf into Y and UV planes
        let (y_plane, uv_plane) = self.nv12_buf[..y_len + uv_len].split_at_mut(y_len);

        let mut nv12_image = YuvBiPlanarImageMut {
            y_plane: BufferStoreMut::Borrowed(y_plane),
            y_stride: width,
            uv_plane: BufferStoreMut::Borrowed(uv_plane),
            uv_stride: width,
            width,
            height,
        };

        yuvutils_rs::rgb_to_yuv_nv12(
            &mut nv12_image,
            rgb,
            width * 3,
            YuvRange::Full,
            YuvStandardMatrix::Bt709,
            YuvConversionMode::Balanced,
        )
        .map_err(|e| report!(VideoError::EncodeFailed(format!("RGB->NV12 conversion failed: {e:?}"))))?;

        let force_keyframe = self.frame_count == 0;
        let frame = InputFrame {
            data: RawFrameData { frame: self.nv12_buf.clone(), width, height },
            pts: Some(self.frame_count),
        };

        let output = self
            .encoder
            .encode(&frame, force_keyframe)
            .map_err(|e| report!(VideoError::EncodeFailed(format!("GPU encode failed: {e:?}"))))?;

        self.frame_count += 1;
        Ok(output.data)
    }
}
