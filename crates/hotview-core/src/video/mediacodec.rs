//! Android hardware video decoding through the NDK `MediaCodec` C API.
//!
//! This is the only "a little C" in the stack: the Rust core hands the codec a
//! file descriptor, reads its raw I420/NV12 output buffers (honouring the
//! `stride`/`slice-height`/`crop-*` geometry the codec reports) and feeds the
//! normalised planes to the wgpu renderer.

#![allow(non_upper_case_globals)]

use std::ffi::{c_char, CStr, CString};
use std::fs::File;
use std::os::fd::{AsRawFd, OwnedFd};
use std::ptr;

use ndk_sys::{
    media_status_t, AMediaCodec, AMediaCodecBufferInfo, AMediaCodec_configure,
    AMediaCodec_createCodecByName, AMediaCodec_createDecoderByType, AMediaCodec_dequeueInputBuffer,
    AMediaCodec_dequeueOutputBuffer, AMediaCodec_delete, AMediaCodec_flush,
    AMediaCodec_getInputBuffer, AMediaCodec_getOutputBuffer, AMediaCodec_getOutputFormat,
    AMediaCodec_queueInputBuffer, AMediaCodec_releaseOutputBuffer, AMediaCodec_start,
    AMediaCodec_stop, AMediaExtractor, AMediaExtractor_advance, AMediaExtractor_delete,
    AMediaExtractor_getSampleFlags, AMediaExtractor_getSampleTime,
    AMediaExtractor_getTrackCount, AMediaExtractor_getTrackFormat, AMediaExtractor_new,
    AMediaExtractor_readSampleData, AMediaExtractor_seekTo, AMediaExtractor_selectTrack,
    AMediaExtractor_setDataSource, AMediaExtractor_setDataSourceFd, AMediaFormat,
    AMediaFormat_delete, AMediaFormat_getInt32, AMediaFormat_getInt64, AMediaFormat_getString,
    AMediaFormat_setInt32, AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
    AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED, AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED,
    AMEDIACODEC_INFO_TRY_AGAIN_LATER, SeekMode,
};

use crate::error::{HotviewError, Result};
use crate::frame::{ChromaLayout, ColorMatrix, ColorRange, MediaFrame, PlanarFrame, Plane, YuvInfo};
use crate::video::{AudioDecoder, AudioOutcome, DecodeOutcome, VideoDecoder, VideoInfo};

const COLOR_FORMAT_YUV420_PLANAR: i32 = 19;
const COLOR_FORMAT_YUV420_SEMIPLANAR: i32 = 21;
const COLOR_FORMAT_YUV420_FLEXIBLE: i32 = 0x7F42_0888;

unsafe extern "C" {
    /// bionic's `libdl`; `RTLD_DEFAULT` is a null handle.
    fn dlsym(handle: *mut std::ffi::c_void, symbol: *const c_char) -> *mut std::ffi::c_void;
}

/// The component name of a codec (`c2.qti.av1.decoder` vs `c2.android.av1.decoder`).
///
/// `AMediaCodec_getName` is API 28+, so resolve it at runtime: API 26/27 keep
/// running and the API 26 link stays valid.
fn codec_name(codec: *mut AMediaCodec) -> Option<String> {
    type GetNameFn = unsafe extern "C" fn(*mut AMediaCodec, *mut *mut c_char) -> media_status_t;
    type ReleaseNameFn = unsafe extern "C" fn(*mut AMediaCodec, *mut c_char);

    unsafe {
        let get_name = dlsym(ptr::null_mut(), cstr("AMediaCodec_getName").as_ptr());
        let release_name = dlsym(ptr::null_mut(), cstr("AMediaCodec_releaseName").as_ptr());
        if get_name.is_null() || release_name.is_null() {
            return None;
        }
        let get_name: GetNameFn = std::mem::transmute(get_name);
        let release_name: ReleaseNameFn = std::mem::transmute(release_name);
        let mut name: *mut c_char = ptr::null_mut();
        if get_name(codec, &mut name) != media_status_t::AMEDIA_OK || name.is_null() {
            return None;
        }
        let text = CStr::from_ptr(name).to_string_lossy().into_owned();
        release_name(codec, name);
        Some(text)
    }
}

struct ExtractorPtr(*mut AMediaExtractor);

// SAFETY: an extractor is only ever touched from the thread that owns the
// decoder; the pointer is just passed along when the decoder moves threads.
unsafe impl Send for ExtractorPtr {}

impl Drop for ExtractorPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                AMediaExtractor_delete(self.0);
            }
        }
    }
}

struct CodecPtr(*mut AMediaCodec);

// SAFETY: see `ExtractorPtr`.
unsafe impl Send for CodecPtr {}

impl Drop for CodecPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                AMediaCodec_stop(self.0);
                AMediaCodec_delete(self.0);
            }
        }
    }
}

fn check(status: media_status_t, what: &str) -> Result<()> {
    if status == media_status_t::AMEDIA_OK {
        Ok(())
    } else {
        Err(HotviewError::Video(format!("{what} failed ({})", status.0)))
    }
}

fn cstr(value: &str) -> CString {
    CString::new(value).expect("no interior nul")
}

fn format_string(format: *mut AMediaFormat, key: &str) -> Option<String> {
    unsafe {
        let mut out: *const c_char = ptr::null();
        if AMediaFormat_getString(format, cstr(key).as_ptr(), &mut out) && !out.is_null() {
            Some(CStr::from_ptr(out).to_string_lossy().into_owned())
        } else {
            None
        }
    }
}

fn format_int32(format: *mut AMediaFormat, key: &str) -> Option<i32> {
    unsafe {
        let mut out = 0i32;
        AMediaFormat_getInt32(format, cstr(key).as_ptr(), &mut out).then_some(out)
    }
}

fn format_int64(format: *mut AMediaFormat, key: &str) -> Option<i64> {
    unsafe {
        let mut out = 0i64;
        AMediaFormat_getInt64(format, cstr(key).as_ptr(), &mut out).then_some(out)
    }
}

/// `openAssetFileDescriptor` reports `UNKNOWN_LENGTH` (-1) for most content
/// providers, but `MediaExtractor` expects the real byte range.
fn data_source_range(file: &File, offset: i64, length: i64) -> (i64, i64) {
    let offset = offset.max(0);
    if length > 0 {
        return (offset, length);
    }
    let size = file.metadata().map(|meta| meta.len()).unwrap_or(0) as i64;
    (offset, (size - offset).max(0))
}

/// Point an extractor at our descriptor, retrying through `/proc/self/fd`
/// when the descriptor route is rejected (some providers hand out pipes).
unsafe fn open_extractor_data_source(
    extractor: *mut AMediaExtractor,
    file: &File,
    fd: i32,
    offset: i64,
    length: i64,
) -> Result<()> {
    unsafe {
        let (offset, length) = data_source_range(file, offset, length);
        let status = AMediaExtractor_setDataSourceFd(extractor, fd, offset, length);
        if status == media_status_t::AMEDIA_OK {
            return Ok(());
        }
        log::warn!(
            "extractor rejected fd data source ({}) for offset {offset} length {length}; retrying /proc/self/fd",
            status.0
        );
        let path = cstr(&format!("/proc/self/fd/{fd}"));
        let retry = AMediaExtractor_setDataSource(extractor, path.as_ptr());
        if retry == media_status_t::AMEDIA_OK {
            return Ok(());
        }
        Err(HotviewError::Video(format!(
            "extractor could not open descriptor ({}, retry {})",
            status.0, retry.0
        )))
    }
}

/// How a codec lays out one raw output buffer.
#[derive(Clone, Copy, Debug)]
struct Geometry {
    /// Bytes between two rows of the luma plane.
    stride: usize,
    /// Rows in the (possibly padded) luma plane.
    slice_height: usize,
    /// Visible area inside the padded buffer.
    left: usize,
    top: usize,
    width: usize,
    height: usize,
}

impl Geometry {
    /// Tightly packed fallback when the codec reports nothing better.
    fn exact(width: u32, height: u32) -> Self {
        let width = (width as usize).max(2);
        let height = (height as usize).max(2);
        Self {
            stride: width,
            slice_height: height,
            left: 0,
            top: 0,
            width,
            height,
        }
    }
}

/// Read `stride` / `slice-height` / `crop-*` from a codec output format.
///
/// Android reports the crop rectangle with inclusive right/bottom edges, and
/// pads rows to `stride` bytes with `slice_height` rows per plane.
fn read_geometry(format: *mut AMediaFormat, width: u32, height: u32) -> Geometry {
    let mut geometry = Geometry::exact(width, height);
    if format.is_null() {
        return geometry;
    }
    if let Some(stride) = format_int32(format, "stride") {
        if stride > 0 {
            geometry.stride = stride as usize;
        }
    }
    if let Some(slice) = format_int32(format, "slice-height") {
        if slice > 0 {
            geometry.slice_height = slice as usize;
        }
    }
    geometry.stride = geometry.stride.max(2);
    geometry.slice_height = geometry.slice_height.max(2);

    let crop_left = format_int32(format, "crop-left").unwrap_or(0).max(0);
    let crop_top = format_int32(format, "crop-top").unwrap_or(0).max(0);
    let right = format_int32(format, "crop-right")
        .map(|value| value.saturating_add(1))
        .unwrap_or_else(|| crop_left.saturating_add(width as i32));
    let bottom = format_int32(format, "crop-bottom")
        .map(|value| value.saturating_add(1))
        .unwrap_or_else(|| crop_top.saturating_add(height as i32));
    let visible_width = right.saturating_sub(crop_left).max(0) as usize;
    let visible_height = bottom.saturating_sub(crop_top).max(0) as usize;

    if visible_width >= 2 && visible_height >= 2 {
        let left = (crop_left as usize).min(geometry.stride - 2);
        let top = (crop_top as usize).min(geometry.slice_height - 2);
        geometry.left = left;
        geometry.top = top;
        geometry.width = visible_width.min(geometry.stride - left) & !1;
        geometry.height = visible_height.min(geometry.slice_height - top) & !1;
    }
    if geometry.width < 2 || geometry.height < 2 {
        geometry = Geometry::exact(width, height);
    }
    geometry
}

/// Copy the visible rectangle out of a (possibly padded) plane. Missing rows
/// and columns stay zeroed instead of reading out of bounds.
fn copy_rect(
    src: &[u8],
    row_stride: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> Vec<u8> {
    let mut out = vec![0u8; width * height];
    if row_stride == 0 {
        return out;
    }
    for row in 0..height {
        let src_off = (y + row) * row_stride + x;
        if src_off + width > src.len() {
            break;
        }
        out[row * width..(row + 1) * width].copy_from_slice(&src[src_off..src_off + width]);
    }
    out
}

/// Hardware decoder backed by `AMediaCodec` writing into byte buffers.
pub struct MediaCodecDecoder {
    extractor: ExtractorPtr,
    codec: CodecPtr,
    /// Keeps the file descriptor alive while the extractor uses it.
    _file: File,
    info: VideoInfo,
    width: u32,
    height: u32,
    color: YuvInfo,
    color_format: i32,
    geometry: Geometry,
    /// True when the user asked for the AOSP software decoder: its "flexible"
    /// output is planar I420, while hardware codecs hand out NV12.
    software: bool,
    first_frame_logged: bool,
    input_done: bool,
    output_done: bool,
    last_pts_us: i64,
}

impl MediaCodecDecoder {
    /// `fd` ownership is transferred; the decoder closes it when dropped.
    pub fn open(fd: OwnedFd, offset: i64, length: i64, software: bool) -> Result<Self> {
        let file = File::from(fd);

        let extractor = unsafe {
            let extractor = AMediaExtractor_new();
            if extractor.is_null() {
                return Err(HotviewError::Video("could not create extractor".into()));
            }
            if let Err(err) =
                open_extractor_data_source(extractor, &file, file.as_raw_fd(), offset, length)
            {
                AMediaExtractor_delete(extractor);
                return Err(err);
            }
            ExtractorPtr(extractor)
        };

        let (track_index, mime, track_format) = unsafe {
            let extractor = extractor.0;
            let count = AMediaExtractor_getTrackCount(extractor);
            let mut found = None;
            for index in 0..count.max(0) as usize {
                let format = AMediaExtractor_getTrackFormat(extractor, index);
                if format.is_null() {
                    continue;
                }
                let mime = format_string(format, "mime").unwrap_or_default();
                if mime.starts_with("video/") {
                    found = Some((index, mime, format));
                    break;
                }
                AMediaFormat_delete(format);
            }
            found.ok_or(HotviewError::Unsupported)?
        };

        unsafe {
            check(
                AMediaExtractor_selectTrack(extractor.0, track_index),
                "AMediaExtractor_selectTrack",
            )?;
        }

        let track_width = format_int32(track_format, "width").unwrap_or(0).max(0) as u32;
        let track_height = format_int32(track_format, "height").unwrap_or(0).max(0) as u32;
        let duration_us = format_int64(track_format, "durationUs");

        // Ask for a colour layout we can parse. Software decoders natively
        // produce planar I420, hardware decoders usually NV12.
        let first_format = if software {
            COLOR_FORMAT_YUV420_PLANAR
        } else {
            COLOR_FORMAT_YUV420_SEMIPLANAR
        };
        let second_format = if software {
            COLOR_FORMAT_YUV420_SEMIPLANAR
        } else {
            COLOR_FORMAT_YUV420_PLANAR
        };
        let codec = unsafe {
            configure_codec(&mime, track_format, Some(first_format), software)
                .or_else(|_| {
                    let format = AMediaExtractor_getTrackFormat(extractor.0, track_index);
                    configure_codec(&mime, format, Some(second_format), software)
                })
                .or_else(|_| {
                    let format = AMediaExtractor_getTrackFormat(extractor.0, track_index);
                    configure_codec(&mime, format, None, software)
                })
        }
        .map(CodecPtr)?;

        if let Some(name) = codec_name(codec.0) {
            log::info!("video decoder component: {name}");
        }

        unsafe {
            check(AMediaCodec_start(codec.0), "AMediaCodec_start")?;
        }

        let (width, height, color, color_format, geometry) = unsafe {
            let format = AMediaCodec_getOutputFormat(codec.0);
            let width = if format.is_null() {
                track_width
            } else {
                format_int32(format, "width").unwrap_or(track_width as i32).max(0) as u32
            };
            let height = if format.is_null() {
                track_height
            } else {
                format_int32(format, "height").unwrap_or(track_height as i32).max(0) as u32
            };
            let color_format = if format.is_null() {
                COLOR_FORMAT_YUV420_FLEXIBLE
            } else {
                format_int32(format, "color-format").unwrap_or(COLOR_FORMAT_YUV420_FLEXIBLE)
            };
            let geometry = read_geometry(format, width, height);
            let color = read_color_info(format, width, height);
            if !format.is_null() {
                AMediaFormat_delete(format);
            }
            (width, height, color, color_format, geometry)
        };

        if width == 0 || height == 0 {
            return Err(HotviewError::Video("video track reports no size".into()));
        }

        log::info!(
            "video decoder {mime}: {width}x{height}, color {color_format:#x}, stride {}, slice {}, crop ({}, {}) {}x{}",
            geometry.stride,
            geometry.slice_height,
            geometry.left,
            geometry.top,
            geometry.width,
            geometry.height,
        );

        Ok(Self {
            extractor,
            codec,
            _file: file,
            info: VideoInfo {
                width,
                height,
                duration_us,
                fps: None,
                codec: mime,
                has_audio: false,
            },
            width,
            height,
            color,
            color_format,
            geometry,
            software,
            first_frame_logged: false,
            input_done: false,
            output_done: false,
            last_pts_us: 0,
        })
    }

    fn feed_input(&mut self) -> Result<()> {
        if self.input_done {
            return Ok(());
        }
        unsafe {
            let index = AMediaCodec_dequeueInputBuffer(self.codec.0, 0);
            if index < 0 {
                return Ok(());
            }
            let index = index as usize;
            let mut capacity = 0usize;
            let buffer = AMediaCodec_getInputBuffer(self.codec.0, index, &mut capacity);
            if buffer.is_null() {
                return Err(HotviewError::Video("codec returned a null input buffer".into()));
            }

            let written = AMediaExtractor_readSampleData(self.extractor.0, buffer, capacity);
            if written < 0 {
                // No more samples: flush the codec with an end-of-stream flag.
                AMediaCodec_queueInputBuffer(
                    self.codec.0,
                    index,
                    0,
                    0,
                    self.last_pts_us.max(0) as u64,
                    AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM as u32,
                );
                self.input_done = true;
                return Ok(());
            }
            let pts = AMediaExtractor_getSampleTime(self.extractor.0);
            let flags = AMediaExtractor_getSampleFlags(self.extractor.0);
            self.last_pts_us = pts.max(0);
            check(
                AMediaCodec_queueInputBuffer(
                    self.codec.0,
                    index,
                    0,
                    written as usize,
                    self.last_pts_us as u64,
                    flags,
                ),
                "AMediaCodec_queueInputBuffer",
            )?;
            AMediaExtractor_advance(self.extractor.0);
        }
        Ok(())
    }

    /// Normalise one raw codec buffer (I420 / NV12) into a tightly packed frame.
    ///
    /// The NDK `MediaCodec` C API does not hand out images, so the buffer is
    /// plain memory whose layout comes from the output format keys `stride`,
    /// `slice-height` and `crop-*` captured in `Geometry`.
    unsafe fn read_output_buffer(
        &self,
        index: usize,
        info: &AMediaCodecBufferInfo,
        pts_us: i64,
    ) -> Result<MediaFrame> {
        unsafe {
            let mut size = 0usize;
            let data = AMediaCodec_getOutputBuffer(self.codec.0, index, &mut size);
            if data.is_null() || size == 0 {
                return Err(HotviewError::Video("codec produced an empty buffer".into()));
            }
            let offset = info.offset.max(0) as usize;
            let byte_count = info.size.max(0) as usize;
            if byte_count == 0 || offset + byte_count > size {
                return Err(HotviewError::Video(
                    "codec output buffer is out of range".into(),
                ));
            }
            let payload = std::slice::from_raw_parts(data.add(offset), byte_count);

            let g = self.geometry;
            if payload.len() < g.stride {
                return Err(HotviewError::Video(
                    "codec output buffer is shorter than one row".into(),
                ));
            }
            let y_size = g.stride.saturating_mul(g.slice_height);
            let chroma_width = (g.width / 2).max(1);
            let chroma_height = (g.height / 2).max(1);
            let y = copy_rect(payload, g.stride, g.left, g.top, g.width, g.height);

            let frame = if self.color_format == COLOR_FORMAT_YUV420_PLANAR
                || (self.software && self.color_format == COLOR_FORMAT_YUV420_FLEXIBLE)
            {
                if self.software && self.color_format == COLOR_FORMAT_YUV420_FLEXIBLE {
                    log::debug!("flexible YUV from the software decoder read as I420");
                }
                let chroma_stride = (g.stride / 2).max(1);
                let chroma_rows = (g.slice_height / 2).max(1);
                let u_at = y_size.min(payload.len());
                let v_at = (y_size + chroma_stride * chroma_rows).min(payload.len());
                let u = copy_rect(
                    &payload[u_at..],
                    chroma_stride,
                    g.left / 2,
                    g.top / 2,
                    chroma_width,
                    chroma_height,
                );
                let v = copy_rect(
                    &payload[v_at..],
                    chroma_stride,
                    g.left / 2,
                    g.top / 2,
                    chroma_width,
                    chroma_height,
                );
                PlanarFrame {
                    width: g.width as u32,
                    height: g.height as u32,
                    y: Plane::new(y, g.width),
                    chroma: ChromaLayout::Planar {
                        u: Plane::new(u, chroma_width),
                        v: Plane::new(v, chroma_width),
                    },
                    info: self.color,
                    pts_us: Some(pts_us),
                }
            } else {
                if self.color_format != COLOR_FORMAT_YUV420_SEMIPLANAR
                    && self.color_format != COLOR_FORMAT_YUV420_FLEXIBLE
                {
                    log::debug!("codec colour format {:#x} read as NV12", self.color_format);
                }
                let uv = copy_rect(
                    &payload[y_size.min(payload.len())..],
                    g.stride,
                    g.left & !1,
                    g.top / 2,
                    chroma_width * 2,
                    chroma_height,
                );
                PlanarFrame {
                    width: g.width as u32,
                    height: g.height as u32,
                    y: Plane::new(y, g.width),
                    chroma: ChromaLayout::SemiPlanar {
                        uv: Plane::new(uv, chroma_width * 2),
                        vu: false,
                    },
                    info: self.color,
                    pts_us: Some(pts_us),
                }
            };
            Ok(MediaFrame::Planar(frame))
        }
    }

    /// Pull one output buffer. `Ok(None)` means the codec wants more input.
    unsafe fn dequeue_output(&mut self) -> Result<Option<DecodeOutcome>> {
        unsafe {
        let mut buffer_info = AMediaCodecBufferInfo {
            offset: 0,
            size: 0,
            presentationTimeUs: 0,
            flags: 0,
        };
        let index = AMediaCodec_dequeueOutputBuffer(self.codec.0, &mut buffer_info, 0);
        if index >= 0 {
            let index = index as usize;
            let pts_us = buffer_info.presentationTimeUs;
            let eos = buffer_info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM as u32 != 0;
            let frame = if eos {
                None
            } else {
                Some(self.read_output_buffer(index, &buffer_info, pts_us)?)
            };
            if let Some(frame) = &frame {
                if !self.first_frame_logged {
                    self.first_frame_logged = true;
                    let (frame_width, frame_height) = frame.dimensions();
                    log::info!(
                        "first video frame decoded: {frame_width}x{frame_height} at {pts_us}us"
                    );
                }
            }
            AMediaCodec_releaseOutputBuffer(self.codec.0, index, false);
            return Ok(Some(match frame {
                Some(frame) => DecodeOutcome::Frame(frame),
                None => {
                    self.output_done = true;
                    DecodeOutcome::Eos
                }
            }));
        }

        let code = index as i32;
        if code == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED {
            let format = AMediaCodec_getOutputFormat(self.codec.0);
            if !format.is_null() {
                self.color_format = format_int32(format, "color-format")
                    .unwrap_or(COLOR_FORMAT_YUV420_FLEXIBLE);
                let width = format_int32(format, "width")
                    .unwrap_or(self.width as i32)
                    .max(0) as u32;
                let height = format_int32(format, "height")
                    .unwrap_or(self.height as i32)
                    .max(0) as u32;
                self.geometry = read_geometry(format, width, height);
                self.color = read_color_info(format, self.width, self.height);
                AMediaFormat_delete(format);
            }
        } else if code != AMEDIACODEC_INFO_TRY_AGAIN_LATER
            && code != AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED
        {
            return Err(HotviewError::Video(format!("codec dequeue failed ({code})")));
        }
        Ok(None)
        }
    }
}

unsafe fn configure_codec(
    mime: &str,
    format: *mut AMediaFormat,
    color_format: Option<i32>,
    software: bool,
) -> Result<*mut AMediaCodec> {
    unsafe {
    if format.is_null() {
        return Err(HotviewError::Video("missing track format".into()));
    }
    let codec = create_codec(mime, software);
    if codec.is_null() {
        AMediaFormat_delete(format);
        return Err(HotviewError::Video(format!("no decoder for {mime}")));
    }
    if let Some(color_format) = color_format {
        AMediaFormat_setInt32(format, cstr("color-format").as_ptr(), color_format);
    }
    let status = AMediaCodec_configure(codec, format, ptr::null_mut(), ptr::null_mut(), 0);
    AMediaFormat_delete(format);

    if status != media_status_t::AMEDIA_OK {
        AMediaCodec_delete(codec);
        return Err(HotviewError::Video(format!(
            "could not configure decoder ({})",
            status.0
        )));
    }
    Ok(codec)
    }
}

/// AOSP software decoder name for a mime type (`c2.android.*`, API 29+).
fn software_decoder_for(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "video/avc" => "c2.android.avc.decoder",
        "video/hevc" => "c2.android.hevc.decoder",
        "video/av01" => "c2.android.av1.decoder",
        "video/vp9" => "c2.android.vp9.decoder",
        "video/vp8" => "c2.android.vp8.decoder",
        "video/mp4v-es" => "c2.android.mpeg4.decoder",
        "video/3gpp" => "c2.android.h263.decoder",
        _ => return None,
    })
}

/// Create the codec the user asked for, falling back to the platform default
/// when the software decoder is missing.
unsafe fn create_codec(mime: &str, software: bool) -> *mut AMediaCodec {
    unsafe {
        if software {
            if let Some(name) = software_decoder_for(mime) {
                let codec = AMediaCodec_createCodecByName(cstr(name).as_ptr());
                if !codec.is_null() {
                    log::info!("video decoder: software ({name})");
                    return codec;
                }
                log::warn!(
                    "software decoder {name} unavailable for {mime}; using the default decoder"
                );
            } else {
                log::warn!("no known software decoder for {mime}; using the default decoder");
            }
        }
        AMediaCodec_createDecoderByType(cstr(mime).as_ptr())
    }
}

fn read_color_info(format: *mut AMediaFormat, width: u32, height: u32) -> YuvInfo {
    if format.is_null() {
        return YuvInfo::guess_for_size(width, height);
    }
    let mut info = YuvInfo::guess_for_size(width, height);
    if let Some(standard) = format_int32(format, "color-standard") {
        info.matrix = match standard {
            1 => ColorMatrix::Bt709,
            2 | 4 => ColorMatrix::Bt601,
            5 | 6 => ColorMatrix::Bt2020,
            _ => info.matrix,
        };
    }
    if let Some(range) = format_int32(format, "color-range") {
        info.range = match range {
            2 => ColorRange::Full,
            _ => ColorRange::Limited,
        };
    }
    info
}

impl VideoDecoder for MediaCodecDecoder {
    fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn next_frame(&mut self) -> Result<DecodeOutcome> {
        if self.output_done {
            return Ok(DecodeOutcome::Eos);
        }
        for _ in 0..2 {
            if let Some(outcome) = unsafe { self.dequeue_output()? } {
                return Ok(outcome);
            }
            self.feed_input()?;
        }
        Ok(DecodeOutcome::Pending)
    }

    fn seek(&mut self, position_us: i64) -> Result<()> {
        unsafe {
            check(
                AMediaExtractor_seekTo(
                    self.extractor.0,
                    position_us.max(0),
                    SeekMode::AMEDIAEXTRACTOR_SEEK_PREVIOUS_SYNC,
                ),
                "AMediaExtractor_seekTo",
            )?;
            check(AMediaCodec_flush(self.codec.0), "AMediaCodec_flush")?;
        }
        self.input_done = false;
        self.output_done = false;
        self.last_pts_us = position_us.max(0);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

/// Sample encodings MediaCodec can hand back (mirrors `AudioFormat`).
const PCM_ENCODING_16BIT: i32 = 2;
const PCM_ENCODING_8BIT: i32 = 3;
const PCM_ENCODING_FLOAT: i32 = 4;

/// Description of a decoded audio track.
#[derive(Clone, Copy, Debug, Default)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: u16,
}

fn read_audio_format(format: *mut AMediaFormat) -> AudioInfo {
    if format.is_null() {
        return AudioInfo::default();
    }
    AudioInfo {
        sample_rate: format_int32(format, "sample-rate").unwrap_or(0).max(0) as u32,
        channels: format_int32(format, "channel-count").unwrap_or(0).max(0) as u16,
    }
}

/// Downmix to at most stereo and clamp, so the audio device only ever sees
/// mono/stereo float samples.
fn downmix(samples: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        0 | 1 | 2 => samples.to_vec(),
        6 => {
            // L R C LFE Ls Rs -> stereo
            let mut out = Vec::with_capacity(samples.len() / 3);
            for frame in samples.chunks_exact(6) {
                let (l, r, c, lfe, ls, rs) = (frame[0], frame[1], frame[2], frame[3], frame[4], frame[5]);
                out.push((l + 0.7071 * c + 0.7071 * ls + 0.5 * lfe).clamp(-1.0, 1.0));
                out.push((r + 0.7071 * c + 0.7071 * rs + 0.5 * lfe).clamp(-1.0, 1.0));
            }
            out
        }
        other => {
            let mut out = Vec::with_capacity(samples.len() / other * 2);
            for frame in samples.chunks_exact(other) {
                out.push(frame[0].clamp(-1.0, 1.0));
                out.push(frame[1].clamp(-1.0, 1.0));
            }
            out
        }
    }
}

/// Hardware audio decoder (MediaCodec) producing interleaved f32 stereo.
pub struct MediaCodecAudioDecoder {
    extractor: ExtractorPtr,
    codec: CodecPtr,
    _file: File,
    info: AudioInfo,
    pcm_encoding: i32,
    input_done: bool,
    output_done: bool,
    last_pts_us: i64,
}

impl MediaCodecAudioDecoder {
    /// Returns `Ok(None)` when the file has no audio track.
    pub fn open(fd: OwnedFd, offset: i64, length: i64) -> Result<Option<Self>> {
        let file = File::from(fd);

        let extractor = unsafe {
            let extractor = AMediaExtractor_new();
            if extractor.is_null() {
                return Err(HotviewError::Video("could not create extractor".into()));
            }
            if let Err(err) =
                open_extractor_data_source(extractor, &file, file.as_raw_fd(), offset, length)
            {
                AMediaExtractor_delete(extractor);
                return Err(err);
            }
            ExtractorPtr(extractor)
        };

        let found = unsafe {
            let extractor = extractor.0;
            let count = AMediaExtractor_getTrackCount(extractor);
            let mut found = None;
            for index in 0..count.max(0) as usize {
                let format = AMediaExtractor_getTrackFormat(extractor, index);
                if format.is_null() {
                    continue;
                }
                let mime = format_string(format, "mime").unwrap_or_default();
                if mime.starts_with("audio/") {
                    found = Some((index, mime, format));
                    break;
                }
                AMediaFormat_delete(format);
            }
            found
        };
        let Some((track_index, mime, track_format)) = found else {
            return Ok(None);
        };

        let fallback_info = read_audio_format(track_format);
        unsafe {
            check(
                AMediaExtractor_selectTrack(extractor.0, track_index),
                "AMediaExtractor_selectTrack",
            )?;
        }

        let codec = unsafe { configure_codec(&mime, track_format, None, false) }.map(CodecPtr)?;
        unsafe {
            check(AMediaCodec_start(codec.0), "AMediaCodec_start (audio)")?;
        }

        let (info, pcm_encoding) = unsafe {
            let format = AMediaCodec_getOutputFormat(codec.0);
            let mut info = read_audio_format(format);
            if info.sample_rate == 0 {
                info.sample_rate = fallback_info.sample_rate;
            }
            if info.channels == 0 {
                info.channels = fallback_info.channels;
            }
            let pcm_encoding = if format.is_null() {
                PCM_ENCODING_16BIT
            } else {
                format_int32(format, "pcm-encoding").unwrap_or(PCM_ENCODING_16BIT)
            };
            if !format.is_null() {
                AMediaFormat_delete(format);
            }
            (info, pcm_encoding)
        };

        log::debug!(
            "audio codec {} -> {} Hz, {} ch, pcm {}",
            mime,
            info.sample_rate,
            info.channels,
            pcm_encoding
        );

        Ok(Some(Self {
            extractor,
            codec,
            _file: file,
            info,
            pcm_encoding,
            input_done: false,
            output_done: false,
            last_pts_us: 0,
        }))
    }

    pub fn info(&self) -> AudioInfo {
        self.info
    }

    fn feed_input(&mut self) -> Result<()> {
        if self.input_done {
            return Ok(());
        }
        unsafe {
            let index = AMediaCodec_dequeueInputBuffer(self.codec.0, 0);
            if index < 0 {
                return Ok(());
            }
            let index = index as usize;
            let mut capacity = 0usize;
            let buffer = AMediaCodec_getInputBuffer(self.codec.0, index, &mut capacity);
            if buffer.is_null() {
                return Err(HotviewError::Video("codec returned a null audio buffer".into()));
            }

            let written = AMediaExtractor_readSampleData(self.extractor.0, buffer, capacity);
            if written < 0 {
                AMediaCodec_queueInputBuffer(
                    self.codec.0,
                    index,
                    0,
                    0,
                    self.last_pts_us.max(0) as u64,
                    AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM as u32,
                );
                self.input_done = true;
                return Ok(());
            }
            let pts = AMediaExtractor_getSampleTime(self.extractor.0).max(0);
            self.last_pts_us = pts;
            check(
                AMediaCodec_queueInputBuffer(
                    self.codec.0,
                    index,
                    0,
                    written as usize,
                    pts as u64,
                    AMediaExtractor_getSampleFlags(self.extractor.0),
                ),
                "AMediaCodec_queueInputBuffer (audio)",
            )?;
            AMediaExtractor_advance(self.extractor.0);
        }
        Ok(())
    }

    unsafe fn convert_output(&self, index: usize, info: &AMediaCodecBufferInfo) -> Result<Vec<f32>> {
        unsafe {
        let mut size = 0usize;
        let data = AMediaCodec_getOutputBuffer(self.codec.0, index, &mut size);
        if data.is_null() {
            return Err(HotviewError::Video("codec returned a null audio buffer".into()));
        }
        let offset = info.offset.max(0) as usize;
        let byte_count = info.size.max(0) as usize;
        if offset + byte_count > size {
            return Err(HotviewError::Video("audio output buffer is out of range".into()));
        }
        let payload = std::slice::from_raw_parts(data.add(offset), byte_count);
        let channels = self.info.channels.max(1) as usize;

        let samples: Vec<f32> = match self.pcm_encoding {
            PCM_ENCODING_FLOAT => payload
                .chunks_exact(4)
                .map(|bytes| f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                .collect(),
            PCM_ENCODING_8BIT => payload
                .iter()
                .map(|byte| (*byte as f32 - 128.0) / 128.0)
                .collect(),
            _ => payload
                .chunks_exact(2)
                .map(|bytes| i16::from_ne_bytes([bytes[0], bytes[1]]) as f32 / 32768.0)
                .collect(),
        };

        Ok(downmix(&samples, channels))
        }
    }

    /// Decode the next piece of audio. `Pending` means "feed me more input".
    pub fn next_chunk(&mut self) -> Result<AudioOutcome> {
        if self.output_done {
            return Ok(AudioOutcome::Eos);
        }
        for _ in 0..3 {
            unsafe {
                let mut buffer_info = AMediaCodecBufferInfo {
                    offset: 0,
                    size: 0,
                    presentationTimeUs: 0,
                    flags: 0,
                };
                let index = AMediaCodec_dequeueOutputBuffer(self.codec.0, &mut buffer_info, 0);
                if index >= 0 {
                    let index = index as usize;
                    let eos = buffer_info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM as u32 != 0;
                    let samples = if eos {
                        None
                    } else {
                        Some(self.convert_output(index, &buffer_info)?)
                    };
                    AMediaCodec_releaseOutputBuffer(self.codec.0, index, false);
                    match samples {
                        Some(samples) if !samples.is_empty() => {
                            return Ok(AudioOutcome::Samples(samples))
                        }
                        Some(_) => {}
                        None => {
                            self.output_done = true;
                            return Ok(AudioOutcome::Eos);
                        }
                    }
                } else {
                    let code = index as i32;
                    if code == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED {
                        let format = AMediaCodec_getOutputFormat(self.codec.0);
                        if !format.is_null() {
                            let info = read_audio_format(format);
                            if info.sample_rate > 0 {
                                self.info.sample_rate = info.sample_rate;
                            }
                            if info.channels > 0 {
                                self.info.channels = info.channels;
                            }
                            self.pcm_encoding = format_int32(format, "pcm-encoding")
                                .unwrap_or(self.pcm_encoding);
                            AMediaFormat_delete(format);
                        }
                    } else if code != AMEDIACODEC_INFO_TRY_AGAIN_LATER
                        && code != AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED
                    {
                        return Err(HotviewError::Video(format!(
                            "audio codec dequeue failed ({code})"
                        )));
                    }
                }
            }
            self.feed_input()?;
        }
        Ok(AudioOutcome::Pending)
    }

    pub fn seek(&mut self, position_us: i64) -> Result<()> {
        unsafe {
            check(
                AMediaExtractor_seekTo(
                    self.extractor.0,
                    position_us.max(0),
                    SeekMode::AMEDIAEXTRACTOR_SEEK_PREVIOUS_SYNC,
                ),
                "AMediaExtractor_seekTo (audio)",
            )?;
            check(AMediaCodec_flush(self.codec.0), "AMediaCodec_flush (audio)")?;
        }
        self.input_done = false;
        self.output_done = false;
        self.last_pts_us = position_us.max(0);
        Ok(())
    }
}

impl AudioDecoder for MediaCodecAudioDecoder {
    fn sample_rate(&self) -> u32 {
        self.info.sample_rate
    }

    fn channels(&self) -> u16 {
        self.info.channels
    }

    fn next_chunk(&mut self) -> Result<AudioOutcome> {
        MediaCodecAudioDecoder::next_chunk(self)
    }

    fn seek(&mut self, position_us: i64) -> Result<()> {
        MediaCodecAudioDecoder::seek(self, position_us)
    }
}
