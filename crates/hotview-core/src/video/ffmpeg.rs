//! FFmpeg-backed video and audio decoding (desktop builds).
//!
//! FFmpeg does the heavy lifting as a C library; everything above it — frame
//! pacing, conversion, playback — is Rust.

use std::path::Path;
use std::sync::OnceLock;

use ffmpeg_next as ffmpeg;
use ffmpeg::format::Pixel;
use ffmpeg::media::Type;

use crate::error::{HotviewError, Result};
use crate::frame::{MediaFrame, RgbaFrame};
use crate::media::{mime_from_path, MediaInfo, MediaKind};
use crate::video::{DecodeOutcome, VideoDecoder, VideoInfo};

fn init() -> Result<()> {
    static INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    match INIT.get_or_init(|| ffmpeg::init().map_err(|err| err.to_string())) {
        Ok(()) => Ok(()),
        Err(err) => Err(HotviewError::Video(format!("ffmpeg init failed: {err}"))),
    }
}

fn verr(context: &str, err: ffmpeg::Error) -> HotviewError {
    HotviewError::Video(format!("{context}: {err}"))
}

fn is_eagain(err: &ffmpeg::Error) -> bool {
    matches!(err, ffmpeg::Error::Other { errno } if *errno == ffmpeg::util::error::EAGAIN)
}

fn pts_to_us(pts: i64, time_base: ffmpeg::Rational) -> i64 {
    let (num, den) = (time_base.numerator() as i128, time_base.denominator() as i128);
    if den == 0 {
        return 0;
    }
    (pts as i128 * num * 1_000_000 / den) as i64
}

fn us_to_pts(us: i64, time_base: ffmpeg::Rational) -> i64 {
    let (num, den) = (time_base.numerator() as i128, time_base.denominator() as i128);
    if num == 0 {
        return 0;
    }
    (us as i128 * den / (num * 1_000_000)) as i64
}

fn stream_fps(time_base: ffmpeg::Rational) -> Option<f64> {
    if time_base.denominator() <= 0 || time_base.numerator() <= 0 {
        return None;
    }
    let fps = time_base.numerator() as f64 / time_base.denominator() as f64;
    (fps > 0.0 && fps < 1000.0).then_some(fps)
}

/// Probe a video container.
pub fn probe_video(path: &Path) -> Result<MediaInfo> {
    init()?;
    let input = ffmpeg::format::input(path).map_err(|e| verr("open input", e))?;
    let stream = input
        .streams()
        .best(Type::Video)
        .ok_or(HotviewError::Unsupported)?;
    let fps = stream_fps(stream.avg_frame_rate());
    let duration_us = (input.duration() > 0).then(|| input.duration());
    let params = stream.parameters();
    let decoder = ffmpeg::codec::context::Context::from_parameters(params)
        .map_err(|e| verr("decoder params", e))?
        .decoder()
        .video()
        .map_err(|e| verr("video decoder", e))?;

    Ok(MediaInfo {
        kind: MediaKind::Video,
        width: decoder.width(),
        height: decoder.height(),
        duration_us,
        fps,
        mime: mime_from_path(path),
    })
}

/// `swscale` contexts are not marked `Send` upstream, but a decoder is owned
/// by exactly one worker thread, so moving it between threads (once) is safe.
struct SendScaler(ffmpeg::software::scaling::Context);

// SAFETY: the wrapped context is only ever used from the thread that owns the
// decoder; we never share it concurrently.
unsafe impl Send for SendScaler {}

/// Decode the first video frame of any container FFmpeg understands.
/// Used as a still-image fallback for formats like AVIF/HEIC/JPEG XL.
#[cfg(feature = "ffmpeg")]
pub fn first_frame_rgba(path: &Path) -> Result<RgbaFrame> {
    use crate::video::VideoDecoder;
    let mut decoder = FfmpegVideoDecoder::open(path)?;
    for _ in 0..240 {
        match decoder.next_frame()? {
            DecodeOutcome::Frame(frame) => return Ok(frame.to_rgba()),
            DecodeOutcome::Pending => continue,
            DecodeOutcome::Eos => break,
        }
    }
    Err(HotviewError::Unsupported)
}

/// Software video decoder that converts every frame to packed RGBA.
pub struct FfmpegVideoDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    stream_index: usize,
    time_base: ffmpeg::Rational,
    scaler: SendScaler,
    info: VideoInfo,
    eos: bool,
    decoded: ffmpeg::frame::Video,
    rgba: ffmpeg::frame::Video,
}

impl FfmpegVideoDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        init()?;
        let input = ffmpeg::format::input(path).map_err(|e| verr("open input", e))?;
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or(HotviewError::Unsupported)?;

        let stream_index = stream.index();
        let time_base = stream.time_base();
        let fps = stream_fps(stream.avg_frame_rate());
        let duration_us = (input.duration() > 0).then(|| input.duration());
        let has_audio = input.streams().best(Type::Audio).is_some();
        let codec_id = stream.parameters().id();
        let params = stream.parameters();

        let decoder = ffmpeg::codec::context::Context::from_parameters(params)
            .map_err(|e| verr("decoder params", e))?
            .decoder()
            .video()
            .map_err(|e| verr("video decoder", e))?;

        let (width, height) = (decoder.width(), decoder.height());
        if width == 0 || height == 0 {
            return Err(HotviewError::Video("empty video stream".into()));
        }

        let scaler = ffmpeg::software::scaling::Context::get(
            decoder.format(),
            width,
            height,
            Pixel::RGBA,
            width,
            height,
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .map_err(|e| verr("scaler", e))?;

        Ok(Self {
            input,
            decoder,
            stream_index,
            time_base,
            scaler: SendScaler(scaler),
            info: VideoInfo {                width,
                height,
                duration_us,
                fps,
                codec: format!("{codec_id:?}"),
                has_audio,
            },
            eos: false,
            decoded: ffmpeg::frame::Video::empty(),
            rgba: ffmpeg::frame::Video::empty(),
        })
    }

    fn convert_current(&mut self) -> Result<MediaFrame> {
        self.scaler
            .0
            .run(&self.decoded, &mut self.rgba)
            .map_err(|e| verr("swscale", e))?;

        let width = self.rgba.width();
        let height = self.rgba.height();
        let stride = self.rgba.stride(0);
        let row_bytes = width as usize * 4;
        let src = self.rgba.data(0);
        let mut data = Vec::with_capacity(row_bytes * height as usize);
        for row in 0..height as usize {
            let start = row * stride;
            data.extend_from_slice(&src[start..start + row_bytes]);
        }

        Ok(MediaFrame::Rgba(RgbaFrame {
            width,
            height,
            data,
            pts_us: self.decoded.pts().map(|pts| pts_to_us(pts, self.time_base)),
        }))
    }

    fn feed_packets(&mut self) -> Result<()> {
        let mut sent = false;
        {
            let mut packets = self.input.packets();
            while let Some((stream, packet)) = packets.next() {
                if stream.index() == self.stream_index {
                    self.decoder
                        .send_packet(&packet)
                        .map_err(|e| verr("send_packet", e))?;
                    sent = true;
                    break;
                }
            }
        }
        if !sent {
            match self.decoder.send_eof() {
                Ok(()) | Err(ffmpeg::Error::Eof) => {}
                Err(e) => return Err(verr("send_eof", e)),
            }
        }
        Ok(())
    }
}

impl VideoDecoder for FfmpegVideoDecoder {
    fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn next_frame(&mut self) -> Result<DecodeOutcome> {
        if self.eos {
            return Ok(DecodeOutcome::Eos);
        }
        loop {
            match self.decoder.receive_frame(&mut self.decoded) {
                Ok(()) => return Ok(DecodeOutcome::Frame(self.convert_current()?)),
                Err(ffmpeg::Error::Eof) => {
                    self.eos = true;
                    return Ok(DecodeOutcome::Eos);
                }
                Err(ref e) if is_eagain(e) => {}
                Err(e) => return Err(verr("receive_frame", e)),
            }
            self.feed_packets()?;
        }
    }

    fn seek(&mut self, position_us: i64) -> Result<()> {
        let target = us_to_pts(position_us, self.time_base);
        self.input
            .seek(target, ..)
            .map_err(|e| verr("seek", e))?;
        self.decoder.flush();
        self.eos = false;
        Ok(())
    }
}

/// Audio decoder that resamples everything to packed f32 stereo at 48 kHz.
#[cfg(feature = "audio")]
pub struct FfmpegAudioDecoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Audio,
    stream_index: usize,
    time_base: ffmpeg::Rational,
    resampler: ffmpeg::software::resampling::Context,
    decoded: ffmpeg::frame::Audio,
    resampled: ffmpeg::frame::Audio,
    eos: bool,
    flushing: bool,
    pub sample_rate: u32,
    pub channels: u16,
}

#[cfg(feature = "audio")]
impl FfmpegAudioDecoder {
    pub const OUTPUT_RATE: u32 = 48_000;
    pub const OUTPUT_CHANNELS: u16 = 2;

    /// Returns `Ok(None)` when the file has no audio track.
    pub fn open(path: &Path) -> Result<Option<Self>> {
        init()?;
        let input = ffmpeg::format::input(path).map_err(|e| verr("open input", e))?;
        let Some(stream) = input.streams().best(Type::Audio) else {
            return Ok(None);
        };

        let stream_index = stream.index();
        let time_base = stream.time_base();
        let params = stream.parameters();
        let decoder = ffmpeg::codec::context::Context::from_parameters(params)
            .map_err(|e| verr("decoder params", e))?
            .decoder()
            .audio()
            .map_err(|e| verr("audio decoder", e))?;

        let resampler = ffmpeg::software::resampling::Context::get(
            decoder.format(),
            decoder.channel_layout(),
            decoder.rate(),
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
            ffmpeg::ChannelLayout::STEREO,
            Self::OUTPUT_RATE,
        )
        .map_err(|e| verr("resampler", e))?;

        Ok(Some(Self {
            input,
            decoder,
            stream_index,
            time_base,
            resampler,
            decoded: ffmpeg::frame::Audio::empty(),
            resampled: ffmpeg::frame::Audio::empty(),
            eos: false,
            flushing: false,
            sample_rate: Self::OUTPUT_RATE,
            channels: Self::OUTPUT_CHANNELS,
        }))
    }

    pub fn duration_us(&self) -> Option<i64> {
        (self.input.duration() > 0).then(|| self.input.duration())
    }

    fn take_resampled(&mut self) -> Vec<f32> {
        let samples = self.resampled.samples();
        let channels = self.resampled.channels().max(1) as usize;
        let count = samples * channels;
        if count == 0 {
            return Vec::new();
        }
        let plane = self.resampled.plane::<f32>(0);
        plane[..count.min(plane.len())].to_vec()
    }

    fn feed_packets(&mut self) -> Result<()> {
        let mut sent = false;
        {
            let mut packets = self.input.packets();
            while let Some((stream, packet)) = packets.next() {
                if stream.index() == self.stream_index {
                    self.decoder
                        .send_packet(&packet)
                        .map_err(|e| verr("audio send_packet", e))?;
                    sent = true;
                    break;
                }
            }
        }
        if !sent {
            match self.decoder.send_eof() {
                Ok(()) | Err(ffmpeg::Error::Eof) => {}
                Err(e) => return Err(verr("audio send_eof", e)),
            }
        }
        Ok(())
    }

    /// Decode the next chunk of interleaved stereo f32 samples.
    /// `Ok(None)` means end of stream.
    pub fn next_chunk(&mut self) -> Result<Option<Vec<f32>>> {
        loop {
            if self.flushing {
                match self.resampler.flush(&mut self.resampled) {
                    Ok(Some(_)) => {
                        let chunk = self.take_resampled();
                        if chunk.is_empty() {
                            self.flushing = false;
                            return Ok(None);
                        }
                        return Ok(Some(chunk));
                    }
                    Ok(None) => {
                        self.flushing = false;
                        return Ok(None);
                    }
                    Err(e) => {
                        self.flushing = false;
                        return Err(verr("audio flush", e));
                    }
                }
            }

            match self.decoder.receive_frame(&mut self.decoded) {
                Ok(()) => {
                    let input_samples = self.decoded.samples().max(1);
                    let input_rate = self.decoded.rate().max(1);
                    // Generous output allocation: resampling can produce more
                    // samples than the input frame holds.
                    let capacity = ((input_samples as u64 * Self::OUTPUT_RATE as u64)
                        / input_rate as u64) as usize
                        + 4096;
                    if self.resampled.samples() < capacity {
                        self.resampled = ffmpeg::frame::Audio::new(
                            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
                            capacity,
                            ffmpeg::ChannelLayout::STEREO,
                        );
                    }
                    match self.resampler.run(&self.decoded, &mut self.resampled) {
                        Ok(_) => {
                            let chunk = self.take_resampled();
                            if chunk.is_empty() {
                                continue;
                            }
                            return Ok(Some(chunk));
                        }
                        Err(e) => return Err(verr("audio resample", e)),
                    }
                }
                Err(ffmpeg::Error::Eof) => {
                    self.eos = true;
                    self.flushing = true;
                }
                Err(ref e) if is_eagain(e) => {}
                Err(e) => return Err(verr("audio receive_frame", e)),
            }

            if !self.eos {
                self.feed_packets()?;
            }
        }
    }

    pub fn seek(&mut self, position_us: i64) -> Result<()> {
        let target = us_to_pts(position_us, self.time_base);
        self.input.seek(target, ..).map_err(|e| verr("audio seek", e))?;
        self.decoder.flush();
        self.eos = false;
        self.flushing = false;
        Ok(())
    }
}

#[cfg(feature = "audio")]
impl crate::video::AudioDecoder for FfmpegAudioDecoder {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn next_chunk(&mut self) -> Result<crate::video::AudioOutcome> {
        match FfmpegAudioDecoder::next_chunk(self)? {
            Some(samples) => Ok(crate::video::AudioOutcome::Samples(samples)),
            None => Ok(crate::video::AudioOutcome::Eos),
        }
    }

    fn seek(&mut self, position_us: i64) -> Result<()> {
        FfmpegAudioDecoder::seek(self, position_us)
    }
}
