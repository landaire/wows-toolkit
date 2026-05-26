//! GPU video encoder via gpu-video (Vulkan Video).
//!
//! Supports H.264 and H.265 encode. AV1 GPU encode is not yet shipped upstream
//! (see https://docs.rs/gpu-video — compatibility matrix shows AV1 as planned).

use std::num::NonZeroU32;
use std::sync::Arc;

use gpu_video::BytesEncoderH264;
use gpu_video::BytesEncoderH265;
use gpu_video::InputFrame;
use gpu_video::RawFrameData;
use gpu_video::VulkanDevice;
use gpu_video::VulkanInstance;
use gpu_video::parameters::EncoderParametersH264;
use gpu_video::parameters::EncoderParametersH265;
use gpu_video::parameters::RateControl;
use gpu_video::parameters::Rational;
use gpu_video::parameters::VideoParameters;
use gpu_video::parameters::VulkanAdapterDescriptor;
use gpu_video::parameters::VulkanDeviceDescriptor;
use rootcause::prelude::*;
use yuvutils_rs::BufferStoreMut;
use yuvutils_rs::YuvBiPlanarImageMut;
use yuvutils_rs::YuvConversionMode;
use yuvutils_rs::YuvRange;
use yuvutils_rs::YuvStandardMatrix;

use crate::codec::VideoCodec;
use crate::encoder::CodecSupport;
use crate::encoder::EncoderStatus;
use crate::error::VideoError;
use crate::video::FPS;

enum CodecEncoder {
    H264(BytesEncoderH264),
    H265(BytesEncoderH265),
}

pub struct GpuEncoder {
    inner: CodecEncoder,
    nv12_buf: Vec<u8>,
    frame_count: u64,
}

fn open_device() -> Result<(Arc<VulkanInstance>, Arc<VulkanDevice>, String), VulkanInitError> {
    let instance = VulkanInstance::new().map_err(|e| VulkanInitError::Instance(format!("{e:?}")))?;
    let adapter = instance
        .create_adapter(&VulkanAdapterDescriptor { supports_encoding: true, ..Default::default() })
        .map_err(|e| VulkanInitError::Adapter(format!("{e:?}")))?;
    if !adapter.supports_encoding() {
        return Err(VulkanInitError::NoEncodeSupport(adapter.info().name.clone()));
    }
    let name = adapter.info().name.clone();
    let device = adapter
        .create_device(&VulkanDeviceDescriptor {
            wgpu_features: wgpu::Features::empty(),
            wgpu_experimental_features: wgpu::ExperimentalFeatures::disabled(),
            wgpu_limits: wgpu::Limits { max_immediate_size: 128, ..Default::default() },
        })
        .map_err(|e| VulkanInitError::Device(format!("{e:?}")))?;
    Ok((instance, device, name))
}

enum VulkanInitError {
    Instance(String),
    Adapter(String),
    Device(String),
    NoEncodeSupport(String),
}

impl std::fmt::Display for VulkanInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Instance(e) => write!(f, "Vulkan init failed: {e}"),
            Self::Adapter(e) => write!(f, "no Vulkan adapter: {e}"),
            Self::Device(e) => write!(f, "Vulkan device creation failed: {e}"),
            Self::NoEncodeSupport(name) => write!(f, "adapter '{name}' lacks encode support"),
        }
    }
}

/// Populate `status.gpu_*` fields by trying each codec encoder against this host's Vulkan device.
pub fn probe_status(status: &mut EncoderStatus) {
    let (_instance, device, name) = match open_device() {
        Ok(v) => v,
        Err(e) => {
            status.gpu_error = Some(e.to_string());
            for codec in VideoCodec::ALL {
                status
                    .gpu_codecs
                    .insert(codec, CodecSupport::Unsupported("Vulkan device unavailable".into()));
            }
            return;
        }
    };
    status.gpu_adapter_name = Some(name);

    status.gpu_codecs.insert(
        VideoCodec::H264,
        match device.encoder_output_parameters_h264_high_quality(default_rate_control()) {
            Ok(_) => CodecSupport::Supported,
            Err(e) => CodecSupport::Unsupported(format!("{e:?}")),
        },
    );
    status.gpu_codecs.insert(
        VideoCodec::H265,
        match device.encoder_output_parameters_h265_high_quality(default_rate_control()) {
            Ok(_) => CodecSupport::Supported,
            Err(e) => CodecSupport::Unsupported(format!("{e:?}")),
        },
    );
    status
        .gpu_codecs
        .insert(VideoCodec::Av1, CodecSupport::Unsupported("gpu-video does not yet support AV1 encode".into()));
}

fn default_rate_control() -> RateControl {
    // Bumped for HUD/HP-bar text legibility. Minimap content is mostly
    // flat-color UI overlay where small text loses detail at lower bitrates.
    RateControl::VariableBitrate {
        average_bitrate: 40_000_000,
        max_bitrate: 80_000_000,
        virtual_buffer_size: std::time::Duration::from_secs(2),
    }
}

impl GpuEncoder {
    pub fn new(width: u32, height: u32, codec: VideoCodec) -> rootcause::Result<Self, VideoError> {
        let (_instance, device, _name) =
            open_device().map_err(|e| report!(VideoError::EncoderInit(e.to_string())))?;

        let input_parameters = VideoParameters {
            width: NonZeroU32::new(width).expect("non-zero width"),
            height: NonZeroU32::new(height).expect("non-zero height"),
            target_framerate: Rational::from(FPS as u32),
        };

        let inner = match codec {
            VideoCodec::H264 => {
                let output_parameters = device
                    .encoder_output_parameters_h264_high_quality(default_rate_control())
                    .map_err(|e| report!(VideoError::EncoderInit(format!("H.264 encoder params: {e:?}"))))?;
                let encoder = device
                    .create_bytes_encoder_h264(EncoderParametersH264 { input_parameters, output_parameters })
                    .map_err(|e| report!(VideoError::EncoderInit(format!("H.264 encoder create: {e:?}"))))?;
                CodecEncoder::H264(encoder)
            }
            VideoCodec::H265 => {
                let output_parameters = device
                    .encoder_output_parameters_h265_high_quality(default_rate_control())
                    .map_err(|e| report!(VideoError::EncoderInit(format!("H.265 encoder params: {e:?}"))))?;
                let encoder = device
                    .create_bytes_encoder_h265(EncoderParametersH265 { input_parameters, output_parameters })
                    .map_err(|e| report!(VideoError::EncoderInit(format!("H.265 encoder create: {e:?}"))))?;
                CodecEncoder::H265(encoder)
            }
            VideoCodec::Av1 => {
                return Err(report!(VideoError::UnsupportedCodec {
                    codec: "av1",
                    backend: "gpu",
                    reason: "gpu-video does not yet support AV1 encode".into(),
                }));
            }
        };

        let nv12_size = (width as usize) * (height as usize) * 3 / 2;
        Ok(Self { inner, nv12_buf: vec![0u8; nv12_size], frame_count: 0 })
    }

    pub fn encode_frame(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> rootcause::Result<Vec<u8>, VideoError> {
        let y_len = (width * height) as usize;
        let uv_len = (width * height / 2) as usize;

        {
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
            .map_err(|e| report!(VideoError::EncodeFailed(format!("RGB->NV12: {e:?}"))))?;
        }

        let force_keyframe = self.frame_count == 0;
        let frame = InputFrame {
            data: RawFrameData { frame: self.nv12_buf.clone(), width, height },
            pts: Some(self.frame_count),
        };

        let chunk = match &mut self.inner {
            CodecEncoder::H264(enc) => enc
                .encode(&frame, force_keyframe)
                .map_err(|e| report!(VideoError::EncodeFailed(format!("H.264 encode: {e:?}"))))?,
            CodecEncoder::H265(enc) => enc
                .encode(&frame, force_keyframe)
                .map_err(|e| report!(VideoError::EncodeFailed(format!("H.265 encode: {e:?}"))))?,
        };

        self.frame_count += 1;
        Ok(chunk.data)
    }
}
