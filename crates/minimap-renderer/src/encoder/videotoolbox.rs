//! VideoToolbox H.264 encoder backend for macOS.
//!
//! Uses Apple's VideoToolbox framework for hardware-accelerated encoding.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::ptr::{
    self,
};
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2_core_foundation::kCFBooleanFalse;
use objc2_core_foundation::kCFBooleanTrue;
use objc2_core_foundation::kCFTypeDictionaryKeyCallBacks;
use objc2_core_foundation::kCFTypeDictionaryValueCallBacks;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFMutableDictionary;
use objc2_core_foundation::CFNumber;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFString;
use objc2_core_foundation::CFType;
use objc2_core_media::CMBlockBuffer;
use objc2_core_media::CMFormatDescription;
use objc2_core_media::CMSampleBuffer;
use objc2_core_media::CMTime;
use objc2_core_media::CMTimeFlags;
use objc2_core_media::CMVideoCodecType;
use objc2_core_media::CMVideoFormatDescriptionGetH264ParameterSetAtIndex;
use objc2_core_video::CVPixelBuffer;
use objc2_core_video::CVPixelBufferCreate;
use objc2_core_video::CVPixelBufferGetBaseAddressOfPlane;
use objc2_core_video::CVPixelBufferGetBytesPerRowOfPlane;
use objc2_core_video::CVPixelBufferLockBaseAddress;
use objc2_core_video::CVPixelBufferLockFlags;
use objc2_core_video::CVPixelBufferUnlockBaseAddress;
use objc2_video_toolbox::kVTCompressionPropertyKey_AllowFrameReordering;
use objc2_video_toolbox::kVTCompressionPropertyKey_AverageBitRate;
use objc2_video_toolbox::kVTCompressionPropertyKey_ExpectedFrameRate;
use objc2_video_toolbox::kVTCompressionPropertyKey_MaxKeyFrameInterval;
use objc2_video_toolbox::kVTCompressionPropertyKey_ProfileLevel;
use objc2_video_toolbox::kVTCompressionPropertyKey_RealTime;
use objc2_video_toolbox::kVTEncodeFrameOptionKey_ForceKeyFrame;
use objc2_video_toolbox::kVTProfileLevel_H264_High_AutoLevel;

use objc2_video_toolbox::VTCompressionSession;
use objc2_video_toolbox::VTEncodeInfoFlags;
use objc2_video_toolbox::VTSessionSetProperty;
use rootcause::prelude::*;
use yuvutils_rs::BufferStoreMut;
use yuvutils_rs::YuvBiPlanarImageMut;
use yuvutils_rs::YuvConversionMode;
use yuvutils_rs::YuvRange;
use yuvutils_rs::YuvStandardMatrix;

use crate::error::VideoError;
use crate::video::FPS;

/// NV12 pixel format type (420YpCbCr8BiPlanarFullRange)
const K_CV_PIXEL_FORMAT_TYPE_420_YP_CB_CR8_BI_PLANAR_FULL_RANGE: u32 = 0x34323066; // '420f'

/// H.264 codec type
const K_CM_VIDEO_CODEC_TYPE_H264: CMVideoCodecType = 0x61766331; // 'avc1'

pub struct VideoToolboxEncoder {
    session: Retained<VTCompressionSession>,
    nv12_buf: Vec<u8>,
    frame_count: u64,
    width: u32,
    height: u32,
    /// Shared buffer for encoded output (filled by callback).
    /// Stored as raw pointer to pass to C callback. Cleaned up in Drop.
    output_buffer: *mut Mutex<Vec<u8>>,
}

// Safety: VTCompressionSession is thread-safe for encoding operations
unsafe impl Send for VideoToolboxEncoder {}

impl VideoToolboxEncoder {
    pub fn new(width: u32, height: u32) -> rootcause::Result<Self, VideoError> {
        // Create output buffer and leak it (we'll clean it up in Drop)
        let output_buffer = Box::into_raw(Box::new(Mutex::new(Vec::new())));

        // Create compression session
        let mut session_out: *mut VTCompressionSession = ptr::null_mut();
        let status = unsafe {
            VTCompressionSession::create(
                None,                                    // allocator
                width as i32,                            // width
                height as i32,                           // height
                K_CM_VIDEO_CODEC_TYPE_H264,              // codec type
                None,                                    // encoder specification
                None,                                    // source image buffer attributes
                None,                                    // compressed data allocator
                Some(compression_output_callback),       // output callback
                output_buffer as *mut c_void,            // callback refcon
                NonNull::new(&mut session_out).unwrap(), // session out
            )
        };

        if status != 0 {
            // Clean up the buffer we created
            unsafe {
                drop(Box::from_raw(output_buffer));
            }
            bail!(VideoError::EncoderInit(format!("VTCompressionSessionCreate failed with status {status}")));
        }

        let session = unsafe {
            Retained::retain(session_out).ok_or_else(|| {
                drop(Box::from_raw(output_buffer));
                report!(VideoError::EncoderInit("VTCompressionSession is null".into()))
            })?
        };

        // Configure encoder properties
        unsafe {
            // Average bitrate: 20 Mbps
            let bitrate = CFNumber::new_i32(20_000_000);
            let _ =
                VTSessionSetProperty(&session, kVTCompressionPropertyKey_AverageBitRate, Some(&*bitrate as &CFType));

            // Expected frame rate: 30 fps
            let framerate = CFNumber::new_f64(FPS);
            let _ = VTSessionSetProperty(
                &session,
                kVTCompressionPropertyKey_ExpectedFrameRate,
                Some(&*framerate as &CFType),
            );

            // Max keyframe interval: 30 frames (1 keyframe per second at 30fps)
            let keyframe_interval = CFNumber::new_i32(30);
            let _ = VTSessionSetProperty(
                &session,
                kVTCompressionPropertyKey_MaxKeyFrameInterval,
                Some(&*keyframe_interval as &CFType),
            );

            // Profile: H.264 High Auto Level
            let _ = VTSessionSetProperty(
                &session,
                kVTCompressionPropertyKey_ProfileLevel,
                Some(&**kVTProfileLevel_H264_High_AutoLevel),
            );

            // Disable frame reordering (no B-frames) for simpler muxing
            if let Some(false_val) = kCFBooleanFalse {
                let _ = VTSessionSetProperty(
                    &session,
                    kVTCompressionPropertyKey_AllowFrameReordering,
                    Some(false_val as &CFType),
                );

                // Not real-time encoding (quality over speed)
                let _ = VTSessionSetProperty(&session, kVTCompressionPropertyKey_RealTime, Some(false_val as &CFType));
            }

            // Prepare to encode
            let status = session.prepare_to_encode_frames();
            if status != 0 {
                drop(Box::from_raw(output_buffer));
                bail!(VideoError::EncoderInit(format!("prepareToEncodeFrames failed: {status}")));
            }
        }

        let nv12_size = (width as usize) * (height as usize) * 3 / 2;

        Ok(Self { session, nv12_buf: vec![0u8; nv12_size], frame_count: 0, width, height, output_buffer })
    }

    pub fn encode_frame(&mut self, rgb: &[u8], width: u32, height: u32) -> rootcause::Result<Vec<u8>, VideoError> {
        debug_assert_eq!(width, self.width);
        debug_assert_eq!(height, self.height);

        let y_len = (width * height) as usize;
        let uv_len = (width * height / 2) as usize;

        // Convert RGB to NV12
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
            .map_err(|e| report!(VideoError::EncodeFailed(format!("RGB→NV12 conversion failed: {e:?}"))))?;
        }

        // Create CVPixelBuffer from NV12 data
        let pixel_buffer = self.create_pixel_buffer()?;

        // Copy NV12 data into pixel buffer
        unsafe {
            let lock_flags = CVPixelBufferLockFlags::empty();
            let status = CVPixelBufferLockBaseAddress(&pixel_buffer, lock_flags);
            if status != 0 {
                bail!(VideoError::EncodeFailed(format!("CVPixelBufferLockBaseAddress failed: {status}")));
            }

            // Copy Y plane
            let y_base = CVPixelBufferGetBaseAddressOfPlane(&pixel_buffer, 0);
            let y_stride = CVPixelBufferGetBytesPerRowOfPlane(&pixel_buffer, 0);
            for row in 0..height as usize {
                let src_offset = row * width as usize;
                let dst_offset = row * y_stride;
                ptr::copy_nonoverlapping(
                    self.nv12_buf.as_ptr().add(src_offset),
                    (y_base as *mut u8).add(dst_offset),
                    width as usize,
                );
            }

            // Copy UV plane
            let uv_base = CVPixelBufferGetBaseAddressOfPlane(&pixel_buffer, 1);
            let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(&pixel_buffer, 1);
            let uv_height = height as usize / 2;
            for row in 0..uv_height {
                let src_offset = y_len + row * width as usize;
                let dst_offset = row * uv_stride;
                ptr::copy_nonoverlapping(
                    self.nv12_buf.as_ptr().add(src_offset),
                    (uv_base as *mut u8).add(dst_offset),
                    width as usize,
                );
            }

            CVPixelBufferUnlockBaseAddress(&pixel_buffer, lock_flags);
        }

        // Clear output buffer before encoding
        {
            let buf = unsafe { &*self.output_buffer };
            buf.lock().unwrap().clear();
        }

        // Create presentation timestamp
        let pts = CMTime {
            value: self.frame_count as i64,
            timescale: FPS as i32,
            flags: CMTimeFlags(1), // kCMTimeFlags_Valid
            epoch: 0,
        };

        let duration = CMTime { value: 1, timescale: FPS as i32, flags: CMTimeFlags(1), epoch: 0 };

        // Force keyframe on first frame
        let frame_properties: Option<CFRetained<CFDictionary<CFString, CFType>>> =
            if self.frame_count == 0 { Some(create_force_keyframe_dict()) } else { None };

        let mut info_flags = VTEncodeInfoFlags::empty();

        // Encode the frame
        unsafe {
            let status = self.session.encode_frame(
                &pixel_buffer,
                pts,
                duration,
                frame_properties.as_ref().map(|d| d.as_opaque()),
                ptr::null_mut(),
                &mut info_flags,
            );
            if status != 0 {
                bail!(VideoError::EncodeFailed(format!("encodeFrame failed: {status}")));
            }

            // Force completion to ensure callback has been called
            let complete_time = CMTime { value: i64::MAX, timescale: 1, flags: CMTimeFlags(1), epoch: 0 };
            let status = self.session.complete_frames(complete_time);
            if status != 0 {
                bail!(VideoError::EncodeFailed(format!("completeFrames failed: {status}")));
            }
        }

        self.frame_count += 1;

        // Get the encoded data (already in Annex B format from callback)
        let output = {
            let buf = unsafe { &*self.output_buffer };
            buf.lock().unwrap().clone()
        };

        if output.is_empty() {
            bail!(VideoError::EncodeFailed("No encoded data received from VideoToolbox".into()));
        }

        Ok(output)
    }

    fn create_pixel_buffer(&self) -> rootcause::Result<Retained<CVPixelBuffer>, VideoError> {
        let mut pixel_buffer_out: *mut CVPixelBuffer = ptr::null_mut();

        let status = unsafe {
            CVPixelBufferCreate(
                None,
                self.width as usize,
                self.height as usize,
                K_CV_PIXEL_FORMAT_TYPE_420_YP_CB_CR8_BI_PLANAR_FULL_RANGE,
                None,
                NonNull::new(&mut pixel_buffer_out).unwrap(),
            )
        };

        if status != 0 || pixel_buffer_out.is_null() {
            bail!(VideoError::EncodeFailed(format!("CVPixelBufferCreate failed: {status}")));
        }

        // Safety: CVPixelBufferCreate returns a retained object, we take ownership
        unsafe { Ok(Retained::retain(pixel_buffer_out).unwrap()) }
    }
}

impl Drop for VideoToolboxEncoder {
    fn drop(&mut self) {
        unsafe {
            self.session.invalidate();
            // Clean up the output buffer
            drop(Box::from_raw(self.output_buffer));
        }
    }
}

/// Creates a CFDictionary with kVTEncodeFrameOptionKey_ForceKeyFrame = true
fn create_force_keyframe_dict() -> CFRetained<CFDictionary<CFString, CFType>> {
    unsafe {
        let dict = CFMutableDictionary::new(None, 0, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks)
            .expect("Failed to create mutable dictionary");

        if let Some(true_val) = kCFBooleanTrue {
            CFMutableDictionary::set_value(
                Some(&dict),
                kVTEncodeFrameOptionKey_ForceKeyFrame as *const _ as *const _,
                true_val as *const _ as *const _,
            );
        }

        // Cast to immutable dictionary type
        CFRetained::cast_unchecked(dict)
    }
}

/// Compression output callback - called by VideoToolbox when a frame is encoded.
///
/// Converts AVCC format to Annex B format and stores in the shared output buffer.
unsafe extern "C-unwind" fn compression_output_callback(
    output_callback_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: i32,
    _info_flags: VTEncodeInfoFlags,
    sample_buffer: *mut CMSampleBuffer,
) {
    // SAFETY: This entire function body requires unsafe operations.
    // We wrap everything in an unsafe block for Rust 2024 compatibility.
    unsafe {
        if status != 0 || sample_buffer.is_null() {
            return;
        }

        let output_buffer = &*(output_callback_ref_con as *const Mutex<Vec<u8>>);
        let sample_buffer = &*sample_buffer;

        // Get the data buffer from the sample buffer
        let data_buffer: Option<CFRetained<CMBlockBuffer>> = sample_buffer.data_buffer();

        let data_buffer: CFRetained<CMBlockBuffer> = match data_buffer {
            Some(db) => db,
            None => return,
        };

        // Get total length and data pointer
        let total_length = data_buffer.data_length();
        if total_length == 0 {
            return;
        }

        let mut data_ptr: *mut i8 = ptr::null_mut();
        let mut length_at_offset: usize = 0;

        let get_status = data_buffer.data_pointer(0, &mut length_at_offset, ptr::null_mut(), &mut data_ptr);

        if get_status != 0 || data_ptr.is_null() {
            return;
        }

        let avcc_data = std::slice::from_raw_parts(data_ptr as *const u8, total_length);

        // Convert AVCC to Annex B format
        let annexb_data = avcc_to_annexb(avcc_data);

        // Get format description for SPS/PPS extraction
        let format_desc: Option<CFRetained<CMFormatDescription>> = sample_buffer.format_description();

        if let Some(ref fd) = format_desc {
            // Check if this is a keyframe by looking for IDR NAL
            let is_keyframe = annexb_data.windows(5).any(|w| {
                (w[0] == 0 && w[1] == 0 && w[2] == 0 && w[3] == 1 && (w[4] & 0x1f) == 5)
                    || (w[0] == 0 && w[1] == 0 && w[2] == 1 && (w[3] & 0x1f) == 5)
            });

            if is_keyframe {
                // Extract and prepend SPS/PPS
                if let Some(sps_pps) = extract_sps_pps(fd) {
                    let mut buf = output_buffer.lock().unwrap();
                    buf.extend_from_slice(&sps_pps);
                    buf.extend_from_slice(&annexb_data);
                    return;
                }
            }
        }

        let mut buf = output_buffer.lock().unwrap();
        buf.extend_from_slice(&annexb_data);
    }
}

/// Convert AVCC format (4-byte length prefix) to Annex B format (start codes).
fn avcc_to_annexb(avcc: &[u8]) -> Vec<u8> {
    let mut annexb = Vec::with_capacity(avcc.len());
    let mut i = 0;

    while i + 4 <= avcc.len() {
        let len = u32::from_be_bytes([avcc[i], avcc[i + 1], avcc[i + 2], avcc[i + 3]]) as usize;
        if i + 4 + len > avcc.len() {
            break;
        }

        // Replace 4-byte length with 4-byte start code
        annexb.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        annexb.extend_from_slice(&avcc[i + 4..i + 4 + len]);
        i += 4 + len;
    }

    annexb
}

/// Extract SPS and PPS from format description and return as Annex B NAL units.
fn extract_sps_pps(format_desc: &CMFormatDescription) -> Option<Vec<u8>> {
    let mut sps_size: usize = 0;
    let mut sps_count: usize = 0;
    let mut sps_ptr: *const u8 = ptr::null();
    let mut nal_unit_header_length: i32 = 0;

    let status = unsafe {
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format_desc,
            0, // SPS index
            &mut sps_ptr,
            &mut sps_size,
            &mut sps_count,
            &mut nal_unit_header_length,
        )
    };

    if status != 0 || sps_ptr.is_null() || sps_size == 0 {
        return None;
    }

    let sps = unsafe { std::slice::from_raw_parts(sps_ptr, sps_size) };

    let mut pps_size: usize = 0;
    let mut pps_ptr: *const u8 = ptr::null();

    let status = unsafe {
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format_desc,
            1, // PPS index
            &mut pps_ptr,
            &mut pps_size,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };

    if status != 0 || pps_ptr.is_null() || pps_size == 0 {
        return None;
    }

    let pps = unsafe { std::slice::from_raw_parts(pps_ptr, pps_size) };

    let mut result = Vec::with_capacity(4 + sps_size + 4 + pps_size);
    result.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    result.extend_from_slice(sps);
    result.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    result.extend_from_slice(pps);

    Some(result)
}
