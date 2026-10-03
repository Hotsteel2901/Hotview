//! Android hardware video decoding through the NDK `MediaCodec` C API.
//!
//! This is the only "a little C" in the stack: the Rust core hands the codec a
//! file descriptor, pulls `AImage` planes, normalises them to I420/NV12 and
//! feeds them to the wgpu renderer.

#![allow(non_upper_case_globals)]

use std::ffi::{c_char, CStr, CString};
use std::fs::File;
use std::os::fd::{AsRawFd, OwnedFd};
use std::ptr;

use ndk_sys::{
    media_status_t, AImage, AImageCropRect, AImage_delete, AImage_getCropRect, AImage_getHeight,
    AImage_getNumberOfPlanes, AImage_getPlaneData, AImage_getPlanePixelStride,
    AImage_getPlaneRowStride, AImage_getWidth, AMediaCodec, AMediaCodecBufferInfo,
    AMediaCodec_configure, AMediaCodec_createDecoderByType, AMediaCodec_dequeueInputBuffer,
    AMediaCodec_dequeueOutputBuffer, AMediaCodec_delete, AMediaCodec_flush,
    AMediaCodec_getInputBuffer, AMediaCodec_getOutputBuffer, AMediaCodec_getOutputFormat,
    AMediaCodec_queueInputBuffer, AMediaCodec_releaseOutputBuffer, AMediaCodec_start,
    AMediaCodec_stop, AMediaExtractor, AMediaExtractor_advance, AMediaExtractor_delete,
    AMediaExtractor_getSampleFlags, AMediaExtractor_getSampleSize, AMediaExtractor_getSampleTime,
    AMediaExtractor_getTrackCount, AMediaExtractor_getTrackFormat, AMediaExtractor_new,
    AMediaExtractor_readSampleData, AMediaExtractor_seekTo, AMediaExtractor_selectTrack,
    AMediaExtractor_setDataSourceFd, AMediaFormat, AMediaFormat_delete, AMediaFormat_getInt32,
    AMediaFormat_getInt64, AMediaFormat_getString, AMediaFormat_setInt32,
    AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM, AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED,
    AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED, AMEDIACODEC_INFO_TRY_AGAIN_LATER, SeekMode,
};

use crate::error::{HotviewError, Result};
use crate::frame::{ChromaLayout, ColorMatrix, ColorRange, MediaFrame, PlanarFrame, Plane, YuvInfo};
use crate::video::{AudioDecoder, AudioOutcome, DecodeOutcome, VideoDecoder, VideoInfo};

unsafe extern "C" {
    /// Not exported by `ndk-sys` yet, but part of the stable NDK since API 21.
    fn AMediaCodec_getOutputImage(codec: *mut AMediaCodec, index: usize) -> *mut AImage;
}

const COLOR_FORMAT_YUV420_PLANAR: i32 = 19;
const COLOR_FORMAT_YUV420_SEMIPLANAR: i32 = 21;
const COLOR_FORMAT_YUV420_FLEXIBLE: i32 = 0x7F42_0888;

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

/// A raw image plane as reported by `AImage`.
struct RawPlane {
    ptr: *const u8,
    len: usize,
    row_stride: usize,
    pixel_stride: usize,
}

unsafe fn read_plane(image: *mut AImage, index: i32) -> Result<RawPlane> {
    unsafe {
    let mut data: *mut u8 = ptr::null_mut();
    let mut len = 0i32;
    check(
        AImage_getPlaneData(image, index, &mut data, &mut len),
        "AImage_getPlaneData",
    )?;
    let mut row_stride = 0i32;
    check(
        AImage_getPlaneRowStride(image, index, &mut row_stride),
        "AImage_getPlaneRowStride",
    )?;
    let mut pixel_stride = 0i32;
    check(
        AImage_getPlanePixelStride(image, index, &mut pixel_stride),
        "AImage_getPlanePixelStride",
    )?;
    Ok(RawPlane {
        ptr: data,
        len: len.max(0) as usize,
        row_stride: row_stride.max(0) as usize,
        pixel_stride: pixel_stride.max(1) as usize,
    })
    }
}

fn copy_rows(plane: &RawPlane, x: usize, y: usize, width_bytes: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0u8; width_bytes * height];
    if plane.row_stride < x + width_bytes || plane.ptr.is_null() {
        return out;
    }
    for row in 0..height {
        let offset = (y + row) * plane.row_stride + x;
        if offset + width_bytes > plane.len {
            break;
        }
        unsafe {
            let src = std::slice::from_raw_parts(plane.ptr.add(offset), width_bytes);
            out[row * width_bytes..(row + 1) * width_bytes].copy_from_slice(src);
        }
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
    input_done: bool,
    output_done: bool,
    last_pts_us: i64,
}

impl MediaCodecDecoder {
    /// `fd` ownership is transferred; the decoder closes it when dropped.
    pub fn open(fd: OwnedFd, offset: i64, length: i64) -> Result<Self> {
        let file = File::from(fd);

        let extractor = unsafe {
            let extractor = AMediaExtractor_new();
            if extractor.is_null() {
                return Err(HotviewError::Video("could not create extractor".into()));
            }
            let status = AMediaExtractor_setDataSourceFd(extractor, file.as_raw_fd(), offset, length);
            if status != media_status_t::AMEDIA_OK {
                AMediaExtractor_delete(extractor);
                return Err(HotviewError::Video(format!(
                    "extractor could not open descriptor ({})",
                    status.0
                )));
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

        // Configure with flexible YUV first; some devices reject that and need
        // the codec's default byte-buffer format.
        let codec = unsafe {
            configure_codec(&mime, track_format, true).or_else(|_| {
                let format = AMediaExtractor_getTrackFormat(extractor.0, track_index);
                configure_codec(&mime, format, false)
            })
        }
        .map(CodecPtr)?;

        unsafe {
            check(AMediaCodec_start(codec.0), "AMediaCodec_start")?;
        }

        let (width, height, color, color_format) = unsafe {
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
            let color = read_color_info(format, width, height);
            if !format.is_null() {
                AMediaFormat_delete(format);
            }
            (width, height, color, color_format)
        };

        if width == 0 || height == 0 {
            return Err(HotviewError::Video("video track reports no size".into()));
        }

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

            let sample_size = AMediaExtractor_getSampleSize(self.extractor.0);
            if sample_size < 0 {
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

            let to_read = (sample_size as usize).min(capacity);
            let written = AMediaExtractor_readSampleData(self.extractor.0, buffer, to_read);
            if written < 0 {
                return Err(HotviewError::Video("could not read sample data".into()));
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

    fn read_output_image(&mut self, index: usize, pts_us: i64) -> Result<MediaFrame> {
        unsafe {
            let image = AMediaCodec_getOutputImage(self.codec.0, index);
            if image.is_null() {
                return self.read_output_buffer(index, pts_us);
            }
            let result = self.image_to_frame(image, pts_us);
            AImage_delete(image);
            result
        }
    }

    unsafe fn image_to_frame(&self, image: *mut AImage, pts_us: i64) -> Result<MediaFrame> {
        unsafe {
        let mut width = 0i32;
        let mut height = 0i32;
        check(AImage_getWidth(image, &mut width), "AImage_getWidth")?;
        check(AImage_getHeight(image, &mut height), "AImage_getHeight")?;
        if width <= 0 || height <= 0 {
            return Err(HotviewError::Video("codec produced an empty frame".into()));
        }

        let mut crop = AImageCropRect {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        AImage_getCropRect(image, &mut crop);
        let left = crop.left.max(0) as usize;
        let top = crop.top.max(0) as usize;
        let vis_width = (crop.right - crop.left).max(0) as u32;
        let vis_height = (crop.bottom - crop.top).max(0) as u32;
        if vis_width == 0 || vis_height == 0 {
            return Err(HotviewError::Video("codec produced an empty crop".into()));
        }

        let mut plane_count = 0i32;
        check(
            AImage_getNumberOfPlanes(image, &mut plane_count),
            "AImage_getNumberOfPlanes",
        )?;
        let planes = (0..plane_count)
            .map(|index| read_plane(image, index))
            .collect::<Result<Vec<_>>>()?;
        if planes.is_empty() {
            return Err(HotviewError::Video("frame has no planes".into()));
        }

        let y = &planes[0];
        let y_data = copy_rows(y, left, top, vis_width as usize, vis_height as usize);
        let chroma_width = vis_width.div_ceil(2) as usize;
        let chroma_height = vis_height.div_ceil(2) as usize;
        let chroma_x = left / 2;
        let chroma_y = top / 2;

        let chroma = if planes.len() >= 3 {
            let u = &planes[1];
            let v = &planes[2];
            if u.pixel_stride == 1 && v.pixel_stride == 1 {
                let u_data = copy_rows(u, chroma_x, chroma_y, chroma_width, chroma_height);
                let v_data = copy_rows(v, chroma_x, chroma_y, chroma_width, chroma_height);
                ChromaLayout::Planar {
                    u: Plane::new(u_data, chroma_width),
                    v: Plane::new(v_data, chroma_width),
                }
            } else if u.pixel_stride == 2 {
                let uv_data =
                    copy_rows(u, chroma_x * 2, chroma_y, chroma_width * 2, chroma_height);
                let vu = (v.ptr as usize) < (u.ptr as usize);
                ChromaLayout::SemiPlanar {
                    uv: Plane::new(uv_data, chroma_width * 2),
                    vu,
                }
            } else {
                return Err(HotviewError::Video("unsupported chroma layout".into()));
            }
        } else if planes.len() == 2 {
            let uv = &planes[1];
            let uv_data = copy_rows(uv, chroma_x * 2, chroma_y, chroma_width * 2, chroma_height);
            ChromaLayout::SemiPlanar {
                uv: Plane::new(uv_data, chroma_width * 2),
                vu: false,
            }
        } else {
            return Err(HotviewError::Video("unsupported packed frame layout".into()));
        };

        Ok(MediaFrame::Planar(PlanarFrame {
            width: vis_width,
            height: vis_height,
            y: Plane::new(y_data, vis_width as usize),
            chroma,
            info: self.color,
            pts_us: Some(pts_us),
        }))
        }
    }

    /// Fallback for codecs that cannot hand out an `AImage`.
    unsafe fn read_output_buffer(&self, index: usize, pts_us: i64) -> Result<MediaFrame> {
        unsafe {
        let mut size = 0usize;
        let data = AMediaCodec_getOutputBuffer(self.codec.0, index, &mut size);
        if data.is_null() || size == 0 {
            return Err(HotviewError::Video("codec produced an empty buffer".into()));
        }
        let buffer = std::slice::from_raw_parts(data, size);
        let (width, height) = (self.width as usize, self.height as usize);
        let y_len = width * height;
        let cw = width.div_ceil(2);
        let ch = height.div_ceil(2);

        let frame = match self.color_format {
            COLOR_FORMAT_YUV420_PLANAR => {
                if buffer.len() < y_len + 2 * cw * ch {
                    return Err(HotviewError::Video("short planar buffer".into()));
                }
                let y = buffer[..y_len].to_vec();
                let u = buffer[y_len..y_len + cw * ch].to_vec();
                let v = buffer[y_len + cw * ch..y_len + 2 * cw * ch].to_vec();
                PlanarFrame {
                    width: width as u32,
                    height: height as u32,
                    y: Plane::new(y, width),
                    chroma: ChromaLayout::Planar {
                        u: Plane::new(u, cw),
                        v: Plane::new(v, cw),
                    },
                    info: self.color,
                    pts_us: Some(pts_us),
                }
            }
            COLOR_FORMAT_YUV420_SEMIPLANAR | COLOR_FORMAT_YUV420_FLEXIBLE => {
                let uv_len = 2 * cw * ch;
                if buffer.len() < y_len + uv_len {
                    return Err(HotviewError::Video("short semi-planar buffer".into()));
                }
                let y = buffer[..y_len].to_vec();
                let uv = buffer[y_len..y_len + uv_len].to_vec();
                PlanarFrame {
                    width: width as u32,
                    height: height as u32,
                    y: Plane::new(y, width),
                    chroma: ChromaLayout::SemiPlanar {
                        uv: Plane::new(uv, cw * 2),
                        vu: false,
                    },
                    info: self.color,
                    pts_us: Some(pts_us),
                }
            }
            other => {
                return Err(HotviewError::Video(format!(
                    "unsupported codec colour format {other:#x}"
                )))
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
                Some(self.read_output_image(index, pts_us)?)
            };
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
    force_flexible: bool,
) -> Result<*mut AMediaCodec> {
    unsafe {
    if format.is_null() {
        return Err(HotviewError::Video("missing track format".into()));
    }
    let codec = AMediaCodec_createDecoderByType(cstr(mime).as_ptr());
    if codec.is_null() {
        AMediaFormat_delete(format);
        return Err(HotviewError::Video(format!("no hardware decoder for {mime}")));
    }
    if force_flexible {
        AMediaFormat_setInt32(
            format,
            cstr("color-format").as_ptr(),
            COLOR_FORMAT_YUV420_FLEXIBLE,
        );
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
            let status =
                AMediaExtractor_setDataSourceFd(extractor, file.as_raw_fd(), offset, length);
            if status != media_status_t::AMEDIA_OK {
                AMediaExtractor_delete(extractor);
                return Err(HotviewError::Video(format!(
                    "audio extractor could not open descriptor ({})",
                    status.0
                )));
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

        let codec = unsafe { configure_codec(&mime, track_format, false) }.map(CodecPtr)?;
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

            let sample_size = AMediaExtractor_getSampleSize(self.extractor.0);
            if sample_size < 0 {
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

            let to_read = (sample_size as usize).min(capacity);
            let written = AMediaExtractor_readSampleData(self.extractor.0, buffer, to_read);
            if written < 0 {
                return Err(HotviewError::Video("could not read audio sample data".into()));
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
