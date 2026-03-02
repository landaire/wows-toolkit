//! H.264 encoder backends for video rendering.
//!
//! This module provides a unified interface for H.264 encoding with multiple
//! backend implementations:
//! - `vulkan`: GPU-accelerated encoding via vk-video (Linux/Windows)
//! - `videotoolbox`: GPU-accelerated encoding via VideoToolbox (macOS)
//! - `cpu`: Software encoding via openh264 (all platforms)

use tracing::error;
use tracing::info;

use crate::error::VideoError;

#[cfg(all(feature = "vulkan", not(target_os = "macos")))]
pub mod vulkan;

#[cfg(all(feature = "videotoolbox", target_os = "macos"))]
pub mod videotoolbox;

#[cfg(feature = "cpu")]
pub mod cpu;

/// Result of checking encoder availability.
#[derive(Debug)]
pub struct EncoderStatus {
    pub gpu_available: bool,
    pub gpu_error: Option<String>,
    pub gpu_adapter_name: Option<String>,
    pub cpu_available: bool,
}

impl std::fmt::Display for EncoderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Encoder status:")?;
        if self.gpu_available {
            writeln!(f, "  GPU: available ({})", self.gpu_adapter_name.as_deref().unwrap_or("unknown"))?;
        } else if let Some(ref err) = self.gpu_error {
            writeln!(f, "  GPU: unavailable - {err}")?;
        } else {
            writeln!(f, "  GPU: not compiled in")?;
        }
        if self.cpu_available {
            writeln!(f, "  CPU: available (openh264)")?;
        } else {
            writeln!(f, "  CPU: not compiled in (enable 'cpu' feature)")?;
        }
        Ok(())
    }
}

/// Check which encoder backends are available on this system.
pub fn check_encoder() -> EncoderStatus {
    let mut status = EncoderStatus {
        gpu_available: false,
        gpu_error: None,
        gpu_adapter_name: None,
        cpu_available: cfg!(feature = "cpu"),
    };

    #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
    {
        // On macOS, VideoToolbox is always available if the feature is enabled
        status.gpu_available = true;
        status.gpu_adapter_name = Some("VideoToolbox".to_string());
    }

    #[cfg(all(feature = "vulkan", not(target_os = "macos")))]
    {
        use vk_video::VulkanInstance;
        match VulkanInstance::new() {
            Err(e) => {
                status.gpu_error = Some(format!("Vulkan init failed: {e:?}"));
            }
            Ok(instance) => match instance.create_adapter(None) {
                Err(e) => {
                    status.gpu_error = Some(format!("No Vulkan adapter: {e:?}"));
                }
                Ok(adapter) => {
                    let name = adapter.info().name.clone();
                    status.gpu_adapter_name = Some(name.clone());
                    if adapter.supports_encoding() {
                        status.gpu_available = true;
                    } else {
                        status.gpu_error = Some(format!("Vulkan adapter '{name}' does not support video encoding"));
                    }
                }
            },
        }
    }

    #[cfg(not(any(
        all(feature = "vulkan", not(target_os = "macos")),
        all(feature = "videotoolbox", target_os = "macos")
    )))]
    {
        status.gpu_error = Some("GPU feature not compiled in".to_string());
    }

    status
}

/// Encoder backend dispatch enum.
pub enum EncoderBackend {
    #[cfg(all(feature = "vulkan", not(target_os = "macos")))]
    Vulkan(Box<vulkan::VulkanEncoder>),
    #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
    VideoToolbox(Box<videotoolbox::VideoToolboxEncoder>),
    #[cfg(feature = "cpu")]
    Cpu(Box<cpu::CpuEncoder>),
}

impl EncoderBackend {
    /// Create an encoder backend, preferring GPU acceleration unless `prefer_cpu` is set.
    pub fn create(width: u32, height: u32, prefer_cpu: bool) -> rootcause::Result<Self, VideoError> {
        #[allow(unused_variables)]
        let _ = (width, height, prefer_cpu);

        // CPU explicitly requested
        #[cfg(feature = "cpu")]
        if prefer_cpu {
            info!("Using CPU (openh264) encoder (user preference)");
            return Ok(Self::Cpu(Box::new(cpu::CpuEncoder::new()?)));
        }

        // macOS: try VideoToolbox
        #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
        {
            match videotoolbox::VideoToolboxEncoder::new(width, height) {
                Ok(enc) => {
                    info!("Using VideoToolbox encoder");
                    return Ok(Self::VideoToolbox(Box::new(enc)));
                }
                Err(e) => {
                    error!("VideoToolbox init failed: {e}");
                    #[cfg(feature = "cpu")]
                    {
                        info!("Falling back to CPU encoder");
                        return Ok(Self::Cpu(Box::new(cpu::CpuEncoder::new()?)));
                    }
                    #[cfg(not(feature = "cpu"))]
                    return Err(e);
                }
            }
        }

        // Non-macOS: try Vulkan
        #[cfg(all(feature = "vulkan", not(target_os = "macos")))]
        {
            match vulkan::VulkanEncoder::new(width, height) {
                Ok(enc) => {
                    info!("Using Vulkan Video encoder");
                    return Ok(Self::Vulkan(Box::new(enc)));
                }
                Err(e) => {
                    return Err(e.attach("GPU encoder failed. Enable prefer_cpu to use the CPU encoder instead."));
                }
            }
        }

        // Fallback: CPU (only reachable when no GPU backend is available)
        #[cfg(all(
            feature = "cpu",
            not(all(feature = "videotoolbox", target_os = "macos")),
            not(all(feature = "vulkan", not(target_os = "macos")))
        ))]
        {
            info!("Using CPU (openh264) encoder");
            return Ok(Self::Cpu(Box::new(cpu::CpuEncoder::new()?)));
        }

        #[cfg(not(any(
            feature = "cpu",
            all(feature = "videotoolbox", target_os = "macos"),
            all(feature = "vulkan", not(target_os = "macos"))
        )))]
        {
            return Err(rootcause::Error::new(VideoError::EncoderInit("No encoder backend available".into())));
        }
    }

    /// Encode an RGB frame to H.264 Annex B format.
    pub fn encode_frame(&mut self, rgb: &[u8], width: u32, height: u32) -> rootcause::Result<Vec<u8>, VideoError> {
        match self {
            #[cfg(all(feature = "vulkan", not(target_os = "macos")))]
            Self::Vulkan(enc) => enc.encode_frame(rgb, width, height),
            #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
            Self::VideoToolbox(enc) => enc.encode_frame(rgb, width, height),
            #[cfg(feature = "cpu")]
            Self::Cpu(enc) => enc.encode_frame(rgb, width as usize, height as usize),
        }
    }
}
