//! Still image decoding on top of the pure-Rust `image` crate.

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Seek};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageReader};

use crate::{HotviewError, Result, RgbaFrame};

/// Read only the header of an image file and return its dimensions.
pub fn image_dimensions(path: &Path) -> Result<(u32, u32)> {
    image::image_dimensions(path).map_err(Into::into)
}

/// Probe dimensions of an image file, falling back to FFmpeg for formats the
/// `image` crate does not recognise (AVIF, HEIC, JPEG XL, JP2, …).
pub fn probe_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    if let Ok(dims) = image_dimensions(path) {
        return Some(dims);
    }
    #[cfg(feature = "ffmpeg")]
    {
        if let Ok(info) = crate::video::ffmpeg::probe_video(path) {
            if info.width > 0 && info.height > 0 {
                return Some((info.width, info.height));
            }
        }
    }
    None
}

/// Decode an image file into RGBA, applying its EXIF orientation.
pub fn decode_file(path: &Path) -> Result<RgbaFrame> {
    let file = File::open(path)?;
    decode_reader(BufReader::new(file))
}

/// Decode an in-memory image (used for content:// descriptors on Android).
pub fn decode_bytes(bytes: &[u8]) -> Result<RgbaFrame> {
    decode_reader(Cursor::new(bytes))
}

/// Read image dimensions from an in-memory image without decoding its pixels.
pub fn probe_image_dimensions_bytes(bytes: &[u8]) -> Result<(u32, u32)> {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| HotviewError::Image(e.to_string()))?
        .into_dimensions()
        .map_err(Into::into)
}

/// Whether an image may contain animation. GIF is routed through the animation
/// path (which also handles single-frame GIFs); WebP exposes an animation flag.
pub fn may_contain_animation_bytes(bytes: &[u8]) -> Result<bool> {
    match image::guess_format(bytes).map_err(HotviewError::from)? {
        image::ImageFormat::Gif => Ok(true),
        image::ImageFormat::WebP => {
            let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))?;
            Ok(decoder.has_animation())
        }
        _ => Ok(false),
    }
}

/// A decoded animation frame and the time it should remain visible.
pub struct TimedImageFrame {
    pub frame: RgbaFrame,
    pub delay: Duration,
}

#[derive(Clone, Copy)]
enum AnimationFormat {
    Gif,
    WebP,
}

enum AnimationFrames {
    Gif(image::Frames<'static>),
    WebP(image::Frames<'static>),
}

impl AnimationFrames {
    fn next(&mut self) -> Option<image::ImageResult<image::Frame>> {
        match self {
            Self::Gif(frames) | Self::WebP(frames) => frames.next(),
        }
    }
}

/// Pull-based GIF/WebP animation decoder. Frames are decoded on demand so a
/// long animation does not have to be held in memory all at once.
pub struct ImageAnimation {
    bytes: Arc<[u8]>,
    format: AnimationFormat,
    frames: AnimationFrames,
    width: u32,
    height: u32,
    max_dim: u32,
}

impl ImageAnimation {
    /// Create an animation decoder, returning `None` for still images.
    /// `max_pixels` bounds the decoded frame size before downscaling.
    pub fn from_bytes(bytes: Arc<[u8]>, max_dim: u32, max_pixels: u64) -> Result<Option<Self>> {
        let format = match image::guess_format(&bytes).map_err(HotviewError::from)? {
            image::ImageFormat::Gif => AnimationFormat::Gif,
            image::ImageFormat::WebP => AnimationFormat::WebP,
            _ => return Ok(None),
        };
        let (width, height) = probe_image_dimensions_bytes(&bytes)?;
        if width == 0 || height == 0 || width as u64 * height as u64 > max_pixels {
            return Err(HotviewError::Image(
                "animated image exceeds the decode size limit".into(),
            ));
        }

        let mut frames = open_animation_frames(Arc::clone(&bytes), format)?;
        match format {
            AnimationFormat::Gif => {
                // GIF has no animation flag. Read two frames to distinguish a
                // static GIF, then reopen the stream for normal playback.
                if next_animation_frame(&mut frames)?.is_none()
                    || next_animation_frame(&mut frames)?.is_none()
                {
                    return Ok(None);
                }
                frames = open_animation_frames(Arc::clone(&bytes), format)?;
            }
            AnimationFormat::WebP => {
                let decoder =
                    image::codecs::webp::WebPDecoder::new(Cursor::new(Arc::clone(&bytes)))?;
                if !decoder.has_animation() {
                    return Ok(None);
                }
            }
        }

        Ok(Some(Self {
            bytes,
            format,
            frames,
            width,
            height,
            max_dim,
        }))
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Return the next composited frame, or `None` at the end of the loop.
    pub fn next_frame(&mut self) -> Result<Option<TimedImageFrame>> {
        let Some(frame) = next_animation_frame(&mut self.frames)? else {
            return Ok(None);
        };
        let delay = Duration::from(frame.delay())
            .max(Duration::from_millis(10))
            .min(Duration::from_secs(10));
        let buffer = frame.into_buffer();
        let rgba = RgbaFrame {
            width: buffer.width(),
            height: buffer.height(),
            data: buffer.into_raw(),
            pts_us: None,
        };
        Ok(Some(TimedImageFrame {
            frame: scale_to_fit(rgba, self.max_dim),
            delay,
        }))
    }

    /// Reopen the encoded source after the last frame, ready for another loop.
    pub fn rewind(&mut self) -> Result<()> {
        self.frames = open_animation_frames(Arc::clone(&self.bytes), self.format)?;
        Ok(())
    }
}

/// Open a GIF or animated WebP file for playback. Other formats return `None`.
pub fn open_animated_file(
    path: &Path,
    max_dim: u32,
    max_pixels: u64,
) -> Result<Option<ImageAnimation>> {
    let is_candidate = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "gif" | "webp"))
        .unwrap_or(false);
    if !is_candidate {
        return Ok(None);
    }
    let bytes: Arc<[u8]> = std::fs::read(path)?.into();
    ImageAnimation::from_bytes(bytes, max_dim, max_pixels)
}

fn open_animation_frames(bytes: Arc<[u8]>, format: AnimationFormat) -> Result<AnimationFrames> {
    match format {
        AnimationFormat::Gif => {
            let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))?;
            decoder.set_limits(decode_limits())?;
            Ok(AnimationFrames::Gif(decoder.into_frames()))
        }
        AnimationFormat::WebP => {
            let mut decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))?;
            decoder.set_background_color(image::Rgba([0, 0, 0, 0]))?;
            decoder.set_limits(decode_limits())?;
            Ok(AnimationFrames::WebP(decoder.into_frames()))
        }
    }
}

fn next_animation_frame(frames: &mut AnimationFrames) -> Result<Option<image::Frame>> {
    match frames.next() {
        Some(Ok(frame)) => Ok(Some(frame)),
        Some(Err(err)) => Err(err.into()),
        None => Ok(None),
    }
}

/// Decode and downscale so the longest edge is at most `max_dim` pixels.
/// Used before uploading to the GPU, which has a hard texture size limit.
///
/// Formats the `image` crate does not know (AVIF, HEIC, JPEG XL, …) fall back
/// to the FFmpeg backend, which can decode a first frame from almost anything.
pub fn decode_file_scaled(path: &Path, max_dim: u32) -> Result<RgbaFrame> {
    match decode_file(path) {
        Ok(frame) => Ok(scale_to_fit(frame, max_dim)),
        Err(err) => {
            #[cfg(feature = "ffmpeg")]
            {
                if let Ok(frame) = crate::video::ffmpeg::first_frame_scaled(path, max_dim) {
                    return Ok(frame);
                }
            }
            Err(err)
        }
    }
}

/// Decode a file into a small thumbnail using the fast box filter.
pub fn thumbnail_file(path: &Path, max_dim: u32) -> Result<RgbaFrame> {
    match decode_file(path) {
        Ok(frame) => thumbnail(frame, max_dim),
        Err(err) => {
            #[cfg(feature = "ffmpeg")]
            {
                if let Ok(frame) = crate::video::ffmpeg::first_frame_scaled(path, max_dim) {
                    return Ok(frame);
                }
            }
            Err(err)
        }
    }
}

/// Downscale a frame into a thumbnail using the fast box filter.
pub fn thumbnail(frame: RgbaFrame, max_dim: u32) -> Result<RgbaFrame> {
    if max_dim == 0 || frame.width.max(frame.height) <= max_dim {
        return Ok(frame);
    }
    let img = rgba_image(&frame)?;
    let (nw, nh) = fit_size(frame.width, frame.height, max_dim, max_dim);
    let thumb = image::imageops::thumbnail(&img, nw, nh);
    Ok(RgbaFrame {
        width: nw,
        height: nh,
        data: thumb.into_raw(),
        pts_us: frame.pts_us,
    })
}

/// Downscale a frame (Lanczos is overkill for display, Triangle is a good
/// quality/speed compromise) so its longest edge is at most `max_dim`.
pub fn scale_to_fit(frame: RgbaFrame, max_dim: u32) -> RgbaFrame {
    if max_dim == 0 || frame.width.max(frame.height) <= max_dim {
        return frame;
    }
    let rgba = match rgba_image(&frame) {
        Ok(img) => img,
        Err(_) => return frame,
    };
    let (nw, nh) = fit_size(frame.width, frame.height, max_dim, max_dim);
    let resized = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Triangle);
    RgbaFrame {
        width: nw,
        height: nh,
        data: resized.into_raw(),
        pts_us: frame.pts_us,
    }
}

fn rgba_image(frame: &RgbaFrame) -> Result<image::RgbaImage> {
    image::RgbaImage::from_raw(frame.width, frame.height, frame.data.clone())
        .ok_or_else(|| HotviewError::Image("invalid RGBA buffer size".into()))
}

fn fit_size(width: u32, height: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let scale = (max_w as f32 / width as f32)
        .min(max_h as f32 / height as f32)
        .min(1.0);
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

/// Hard ceiling that stops decompression bombs from taking the process down:
/// at most ~512 MB of decoded pixels, plus sane dimension limits.
fn decode_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(65_535);
    limits.max_image_height = Some(65_535);
    limits.max_alloc = Some(512 * 1024 * 1024);
    limits
}

fn decode_reader<R: BufRead + Seek>(reader: R) -> Result<RgbaFrame> {
    let mut reader = ImageReader::new(reader)
        .with_guessed_format()
        .map_err(|e| HotviewError::Image(e.to_string()))?;
    reader.limits(decode_limits());

    let mut decoder = reader
        .into_decoder()
        .map_err(|e| HotviewError::Image(e.to_string()))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);

    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    Ok(dynamic_to_rgba(img))
}

fn dynamic_to_rgba(img: DynamicImage) -> RgbaFrame {
    let (width, height) = (img.width(), img.height());
    RgbaFrame {
        width,
        height,
        data: img.into_rgba8().into_raw(),
        pts_us: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, Rgba, RgbaImage};

    fn two_frame_gif() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            encoder.set_repeat(Repeat::Infinite).unwrap();
            let frames = [
                Frame::from_parts(
                    RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255])),
                    0,
                    0,
                    Delay::from_numer_denom_ms(40, 1),
                ),
                Frame::from_parts(
                    RgbaImage::from_pixel(2, 2, Rgba([0, 255, 0, 255])),
                    0,
                    0,
                    Delay::from_numer_denom_ms(80, 1),
                ),
            ];
            encoder.encode_frames(frames).unwrap();
        }
        bytes
    }

    #[test]
    fn animation_decoder_reads_delays_and_can_rewind() {
        let bytes: Arc<[u8]> = two_frame_gif().into();
        assert!(may_contain_animation_bytes(&bytes).unwrap());
        let mut animation = ImageAnimation::from_bytes(Arc::clone(&bytes), 16, 64)
            .unwrap()
            .expect("two-frame GIF should be animated");

        let first = animation.next_frame().unwrap().unwrap();
        assert_eq!(&first.frame.data[..4], &[255, 0, 0, 255]);
        assert_eq!(first.delay, Duration::from_millis(40));

        let second = animation.next_frame().unwrap().unwrap();
        assert_eq!(&second.frame.data[..4], &[0, 255, 0, 255]);
        assert_eq!(second.delay, Duration::from_millis(80));
        assert!(animation.next_frame().unwrap().is_none());

        animation.rewind().unwrap();
        let looped = animation.next_frame().unwrap().unwrap();
        assert_eq!(&looped.frame.data[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn rgba_decode_keeps_transparent_pixels_transparent() {
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 255, 0])))
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();

        let decoded = decode_bytes(encoded.get_ref()).unwrap();
        assert_eq!(decoded.data, [0, 0, 255, 0]);
    }

    #[test]
    fn static_gif_is_not_treated_as_animation() {
        let mut bytes = Vec::new();
        GifEncoder::new(&mut bytes)
            .encode(&[1, 2, 3, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        assert!(ImageAnimation::from_bytes(bytes.into(), 16, 64)
            .unwrap()
            .is_none());
    }

    #[test]
    fn open_animated_file_checks_the_extension_case_insensitively() {
        let path = std::env::temp_dir().join(format!(
            "hotview-animation-test-{}.GIF",
            std::process::id()
        ));
        std::fs::write(&path, two_frame_gif()).unwrap();
        let animation = open_animated_file(&path, 16, 64).unwrap();
        assert!(animation.is_some());
        let _ = std::fs::remove_file(&path);
    }
}
