//! Error type shared across the Hotview crates.

use thiserror::Error;

/// Errors produced while probing, decoding or converting media.
#[derive(Debug, Error)]
pub enum HotviewError {
    /// The file is not an image or a video we know how to open.
    #[error("unsupported media format")]
    Unsupported,

    /// Underlying I/O failure.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// Image decoding failed.
    #[error("image decode failed: {0}")]
    Image(String),

    /// Video decoding failed.
    #[error("video decode failed: {0}")]
    Video(String),

    /// The caller asked for something impossible.
    #[error("invalid argument: {0}")]
    Invalid(String),

    /// The requested feature was not compiled in.
    #[error("feature not available: {0}")]
    Unavailable(String),
}

impl From<image::ImageError> for HotviewError {
    fn from(value: image::ImageError) -> Self {
        match value {
            image::ImageError::IoError(err) => HotviewError::Io(err),
            other => HotviewError::Image(other.to_string()),
        }
    }
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, HotviewError>;
