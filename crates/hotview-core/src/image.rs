//! Still image decoding on top of the pure-Rust `image` crate.

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Seek};
use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::{HotviewError, Result, RgbaFrame};

/// Read only the header of an image file and return its dimensions.
pub fn image_dimensions(path: &Path) -> Result<(u32, u32)> {
    image::image_dimensions(path).map_err(Into::into)
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
                if let Ok(frame) = crate::video::ffmpeg::first_frame_rgba(path) {
                    return Ok(scale_to_fit(frame, max_dim));
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
                if let Ok(frame) = crate::video::ffmpeg::first_frame_rgba(path) {
                    return thumbnail(frame, max_dim);
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

fn decode_reader<R: BufRead + Seek>(reader: R) -> Result<RgbaFrame> {
    let reader = ImageReader::new(reader)
        .with_guessed_format()
        .map_err(|e| HotviewError::Image(e.to_string()))?;

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
