//! Video decoding: a small trait plus platform backends.
//!
//! * Desktop: FFmpeg (software decoding, wide format support).
//! * Android: NDK MediaCodec (hardware decoding, zero extra dependencies).

use crate::{MediaFrame, Result};

/// Backend-independent description of a video stream.
#[derive(Clone, Debug, Default)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration_us: Option<i64>,
    pub fps: Option<f64>,
    pub codec: String,
    pub has_audio: bool,
}

/// Result of a single decode step.
#[derive(Debug)]
pub enum DecodeOutcome {
    /// A frame is ready.
    Frame(MediaFrame),
    /// No frame right now (Android MediaCodec needs more input/time).
    Pending,
    /// The stream is exhausted.
    Eos,
}

/// Pull-style video decoder. Implementations are owned by a single worker
/// thread (the desktop player thread or the Android render thread).
pub trait VideoDecoder: Send {
    fn info(&self) -> &VideoInfo;

    /// Decode forward. Never blocks longer than roughly 100 ms.
    fn next_frame(&mut self) -> Result<DecodeOutcome>;

    /// Seek to `position_us`, then the next frames start from there.
    fn seek(&mut self, position_us: i64) -> Result<()>;
}

/// Result of a single audio decode step.
#[derive(Debug)]
pub enum AudioOutcome {
    /// Interleaved f32 samples are ready.
    Samples(Vec<f32>),
    /// No samples right now (Android MediaCodec needs more input).
    Pending,
    /// The stream is exhausted.
    Eos,
}

/// Pull-style audio decoder producing interleaved 32-bit float samples.
pub trait AudioDecoder: Send {
    /// Output sample rate in Hz.
    fn sample_rate(&self) -> u32;

    /// Output channel count (implementations downmix to stereo at most).
    fn channels(&self) -> u16;

    /// Decode the next chunk of interleaved samples.
    fn next_chunk(&mut self) -> Result<AudioOutcome>;

    fn seek(&mut self, position_us: i64) -> Result<()>;
}

#[cfg(feature = "ffmpeg")]
pub mod ffmpeg;

#[cfg(feature = "mediacodec")]
pub mod mediacodec;
