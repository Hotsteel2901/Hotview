//! Hotview core: media probing, image decoding, video decoding and frame
//! conversion. Shared by the desktop viewer and the Android viewer.

pub mod error;
pub mod frame;
pub mod image;
pub mod media;
pub mod video;

pub use error::{HotviewError, Result};
pub use frame::{ChromaLayout, ColorMatrix, ColorRange, MediaFrame, PlanarFrame, Plane, RgbaFrame, YuvInfo};
pub use image::{
    decode_bytes, decode_file, decode_file_scaled, image_dimensions, scale_to_fit, thumbnail,
    thumbnail_file,
};
pub use media::{
    is_image_path, is_media_path, is_video_path, probe_path, MediaInfo, MediaKind, IMAGE_EXTENSIONS,
    VIDEO_EXTENSIONS,
};
pub use video::{DecodeOutcome, VideoDecoder, VideoInfo};
