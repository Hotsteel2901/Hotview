//! Media classification helpers.

use std::path::Path;

use crate::{HotviewError, Result};

/// Supported still-image extensions.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "png", "gif", "webp", "bmp", "tif", "tiff", "ico", "qoi", "dds", "exr",
    "ff", "hdr", "pnm", "pbm", "pgm", "ppm", "pam", "tga", "avif", "heic", "heif", "jxl", "jp2",
];

/// Supported video extensions (containers FFmpeg/MediaCodec can demux).
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "ts", "m2ts", "mpg", "mpeg", "wmv", "flv", "3gp",
    "3g2", "ogv", "mp2", "vob", "divx",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

/// Everything we know about a media file before decoding it.
#[derive(Clone, Debug)]
pub struct MediaInfo {
    pub kind: MediaKind,
    pub width: u32,
    pub height: u32,
    pub duration_us: Option<i64>,
    pub fps: Option<f64>,
    pub mime: Option<String>,
}

impl MediaInfo {
    pub fn is_video(&self) -> bool {
        self.kind == MediaKind::Video
    }
}

/// Lowercased extension of a path, if any.
pub fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

pub fn is_image_path(path: &Path) -> bool {
    extension(path)
        .map(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or(false)
}

pub fn is_video_path(path: &Path) -> bool {
    extension(path)
        .map(|e| VIDEO_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or(false)
}

pub fn is_media_path(path: &Path) -> bool {
    is_image_path(path) || is_video_path(path)
}

/// Guess a MIME type from the file extension.
pub fn mime_from_path(path: &Path) -> Option<String> {
    let ext = extension(path)?;
    let mime = match ext.as_str() {
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "ico" => "image/x-icon",
        "qoi" => "image/qoi",
        "dds" => "image/vnd-ms.dds",
        "exr" => "image/x-exr",
        "ff" => "image/x-farbfeld",
        "hdr" => "image/vnd.radiance",
        "pnm" | "pbm" | "pgm" | "ppm" | "pam" => "image/x-portable-anymap",
        "tga" => "image/x-tga",
        "avif" => "image/avif",
        "heic" | "heif" => "image/heif",
        "jxl" => "image/jxl",
        "jp2" => "image/jp2",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "ts" | "m2ts" => "video/mp2t",
        "mpg" | "mpeg" => "video/mpeg",
        "wmv" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "3gp" => "video/3gpp",
        "3g2" => "video/3gpp2",
        "ogv" => "video/ogg",
        _ => return None,
    };
    Some(mime.to_string())
}

/// Probe a file: images are detected from their header, videos through the
/// FFmpeg backend when it is compiled in.
pub fn probe_path(path: &Path) -> Result<MediaInfo> {
    if let Ok((width, height)) = crate::image::image_dimensions(path) {
        return Ok(MediaInfo {
            kind: MediaKind::Image,
            width,
            height,
            duration_us: None,
            fps: None,
            mime: mime_from_path(path),
        });
    }

    #[cfg(feature = "ffmpeg")]
    {
        return crate::video::ffmpeg::probe_video(path);
    }

    #[allow(unreachable_code)]
    Err(HotviewError::Unsupported)
}
