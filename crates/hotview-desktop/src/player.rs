//! Video playback worker: decodes on its own thread, paces frames against the
//! audio clock (when the file has audio) and streams frames to the UI.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hotview_core::video::ffmpeg::{FfmpegAudioDecoder, FfmpegVideoDecoder};
use hotview_core::video::DecodeOutcome;
use hotview_core::{RgbaFrame, VideoDecoder};

const TICK: Duration = Duration::from_millis(4);
const AUDIO_BUFFER_SAMPLES: usize = 96_000; // ~1 s of stereo 48 kHz samples

pub enum PlayerCommand {
    Play,
    Pause,
    Seek(i64),
    SetLooping(bool),
    SetVolume(f32),
    SetMuted(bool),
    SetSpeed(f32),
    Quit,
}

pub enum PlayerEvent {
    Frame(Arc<RgbaFrame>),
    State {
        playing: bool,
        position_us: i64,
        duration_us: i64,
        has_audio: bool,
    },
    Ended,
    Error(String),
}

pub struct Player {
    commands: Sender<PlayerCommand>,
    pub events: Receiver<PlayerEvent>,
    join: Option<std::thread::JoinHandle<()>>,
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
    pub codec: String,
    pub duration_us: i64,
    pub has_audio: bool,
}

impl Player {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();

        // Open once on the calling thread so errors surface immediately.
        let probe = FfmpegVideoDecoder::open(&path).map_err(|err| err.to_string())?;
        let info = probe.info().clone();
        drop(probe);

        log::info!(
            "video opened: {}x{} @ {:.2} fps ({}), {:.2}s{}",
            info.width,
            info.height,
            info.fps.unwrap_or(0.0),
            info.codec,
            info.duration_us.unwrap_or(0) as f64 / 1_000_000.0,
            if info.has_audio { ", with audio" } else { "" }
        );

        let join = std::thread::Builder::new()
            .name("hotview-player".into())
            .spawn(move || {
                // Report worker panics instead of freezing the viewer.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(path, rx, event_tx.clone())
                }));
                if result.is_err() {
                    let _ = event_tx.send(PlayerEvent::Error(
                        "player crashed while decoding this file".to_string(),
                    ));
                }
            })
            .map_err(|err| err.to_string())?;

        Ok(Self {
            commands: tx,
            events: event_rx,
            join: Some(join),
            width: info.width,
            height: info.height,
            fps: info.fps,
            codec: info.codec,
            duration_us: info.duration_us.unwrap_or(0),
            has_audio: info.has_audio,
        })
    }

    pub fn play(&self) {
        let _ = self.commands.send(PlayerCommand::Play);
    }

    pub fn pause(&self) {
        let _ = self.commands.send(PlayerCommand::Pause);
    }

    pub fn seek(&self, position_us: i64) {
        let _ = self.commands.send(PlayerCommand::Seek(position_us.max(0)));
    }

    pub fn set_looping(&self, looping: bool) {
        let _ = self.commands.send(PlayerCommand::SetLooping(looping));
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self
            .commands
            .send(PlayerCommand::SetVolume(volume.clamp(0.0, 1.0)));
    }

    pub fn set_muted(&self, muted: bool) {
        let _ = self.commands.send(PlayerCommand::SetMuted(muted));
    }

    pub fn set_speed(&self, speed: f32) {
        let _ = self
            .commands
            .send(PlayerCommand::SetSpeed(speed.clamp(0.25, 4.0)));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(PlayerCommand::Quit);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct AudioShared {
    buffer: Mutex<VecDeque<f32>>,
    played_frames: AtomicU64,
    active: AtomicBool,
    gain_bits: AtomicU32,
    speed_bits: AtomicU32,
}

impl AudioShared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            buffer: Mutex::new(VecDeque::with_capacity(AUDIO_BUFFER_SAMPLES * 2)),
            played_frames: AtomicU64::new(0),
            active: AtomicBool::new(false),
            gain_bits: AtomicU32::new(1.0_f32.to_bits()),
            speed_bits: AtomicU32::new(1.0_f32.to_bits()),
        })
    }

    fn set_gain(&self, volume: f32, muted: bool) {
        let effective = if muted { 0.0 } else { volume.clamp(0.0, 1.0) };
        self.gain_bits.store(effective.to_bits(), Ordering::Relaxed);
    }

    fn set_speed(&self, speed: f32) {
        self.speed_bits
            .store(speed.clamp(0.25, 4.0).to_bits(), Ordering::Relaxed);
    }

    fn clear_buffer(&self) {
        if let Ok(mut guard) = self.buffer.lock() {
            guard.clear();
        }
    }
}

struct AudioOutput {
    stream: cpal::Stream,
    sample_rate: u32,
}

impl AudioOutput {
    fn probe_preferred_rate() -> u32 {
        use cpal::traits::{DeviceTrait, HostTrait};
        cpal::default_host()
            .default_output_device()
            .and_then(|dev| dev.default_output_config().ok())
            .map(|cfg| cfg.sample_rate().clamp(8_000, 192_000))
            .unwrap_or(FfmpegAudioDecoder::OUTPUT_RATE)
    }

    fn new(shared: Arc<AudioShared>, fallback_rate: u32) -> Result<Self, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output".to_string())?;

        if let Ok(supported) = device.default_output_config() {
            let sample_rate = supported.sample_rate().clamp(8_000, 192_000);
            let channels = supported.channels().max(1) as usize;
            let config = supported.config();
            let stream = match supported.sample_format() {
                cpal::SampleFormat::F32 => {
                    Self::build_f32_stream(&device, config, channels, shared)?
                }
                cpal::SampleFormat::I16 => {
                    Self::build_i16_stream(&device, config, channels, shared)?
                }
                cpal::SampleFormat::U16 => {
                    Self::build_u16_stream(&device, config, channels, shared)?
                }
                _ => {
                    let stereo_cfg = cpal::StreamConfig {
                        channels: 2,
                        sample_rate,
                        buffer_size: cpal::BufferSize::Default,
                    };
                    Self::build_f32_stream(&device, stereo_cfg, 2, shared)?
                }
            };
            let _ = stream.play();
            return Ok(Self {
                stream,
                sample_rate,
            });
        }

        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: fallback_rate,
            buffer_size: cpal::BufferSize::Default,
        };
        let stream = Self::build_f32_stream(&device, config, 2, shared)?;
        let _ = stream.play();
        Ok(Self {
            stream,
            sample_rate: fallback_rate,
        })
    }

    fn build_f32_stream(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        channels: usize,
        shared: Arc<AudioShared>,
    ) -> Result<cpal::Stream, String> {
        use cpal::traits::DeviceTrait;
        let mut phase = 0.0_f32;
        let mut current = (0.0_f32, 0.0_f32);
        device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    fill_audio_frames(
                        data,
                        channels,
                        &shared,
                        &mut phase,
                        &mut current,
                        0.0_f32,
                        |v| v,
                    );
                },
                |err| log::warn!("audio output error: {err}"),
                None,
            )
            .map_err(|err| err.to_string())
    }

    fn build_i16_stream(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        channels: usize,
        shared: Arc<AudioShared>,
    ) -> Result<cpal::Stream, String> {
        use cpal::traits::DeviceTrait;
        let mut phase = 0.0_f32;
        let mut current = (0.0_f32, 0.0_f32);
        device
            .build_output_stream(
                config,
                move |data: &mut [i16], _| {
                    fill_audio_frames(
                        data,
                        channels,
                        &shared,
                        &mut phase,
                        &mut current,
                        0_i16,
                        |v| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16,
                    );
                },
                |err| log::warn!("audio output error: {err}"),
                None,
            )
            .map_err(|err| err.to_string())
    }

    fn build_u16_stream(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        channels: usize,
        shared: Arc<AudioShared>,
    ) -> Result<cpal::Stream, String> {
        use cpal::traits::DeviceTrait;
        let mut phase = 0.0_f32;
        let mut current = (0.0_f32, 0.0_f32);
        device
            .build_output_stream(
                config,
                move |data: &mut [u16], _| {
                    fill_audio_frames(
                        data,
                        channels,
                        &shared,
                        &mut phase,
                        &mut current,
                        u16::MAX / 2,
                        |v| (((v.clamp(-1.0, 1.0) + 1.0) * 0.5) * u16::MAX as f32) as u16,
                    );
                },
                |err| log::warn!("audio output error: {err}"),
                None,
            )
            .map_err(|err| err.to_string())
    }
}

fn fill_audio_frames<T: Copy>(
    data: &mut [T],
    channels: usize,
    shared: &AudioShared,
    phase: &mut f32,
    current: &mut (f32, f32),
    silence: T,
    convert: impl Fn(f32) -> T,
) {
    if !shared.active.load(Ordering::Relaxed) {
        data.fill(silence);
        return;
    }
    let Ok(mut guard) = shared.buffer.try_lock() else {
        data.fill(silence);
        return;
    };
    let gain = f32::from_bits(shared.gain_bits.load(Ordering::Relaxed));
    let speed = f32::from_bits(shared.speed_bits.load(Ordering::Relaxed)).clamp(0.25, 4.0);
    let channels = channels.max(1);
    let mut consumed_frames = 0_u64;

    for frame in data.chunks_mut(channels) {
        *phase += speed;
        let mut had_sample = false;
        while *phase >= 1.0 {
            *phase -= 1.0;
            if guard.len() >= 2 {
                let l = guard.pop_front().unwrap_or(0.0);
                let r = guard.pop_front().unwrap_or(0.0);
                *current = (l, r);
                consumed_frames += 1;
                had_sample = true;
            } else {
                *current = (0.0, 0.0);
                *phase = 0.0;
                break;
            }
        }
        if !had_sample && guard.is_empty() && current.0 == 0.0 && current.1 == 0.0 {
            frame.fill(silence);
            continue;
        }
        let l = current.0 * gain;
        let r = current.1 * gain;
        if channels == 1 {
            frame[0] = convert((l + r) * 0.5);
        } else {
            frame[0] = convert(l);
            frame[1] = convert(r);
            for extra in &mut frame[2..] {
                *extra = silence;
            }
        }
    }
    drop(guard);
    if consumed_frames > 0 {
        shared
            .played_frames
            .fetch_add(consumed_frames, Ordering::Relaxed);
    }
}

fn run(path: PathBuf, commands: Receiver<PlayerCommand>, events: Sender<PlayerEvent>) {
    let mut video = match FfmpegVideoDecoder::open(&path) {
        Ok(video) => video,
        Err(err) => {
            let _ = events.send(PlayerEvent::Error(err.to_string()));
            return;
        }
    };
    let info = video.info().clone();
    let duration_us = info.duration_us.unwrap_or(0).max(0);

    let preferred_rate = AudioOutput::probe_preferred_rate();
    let audio_shared = AudioShared::new();
    let audio_output = if info.has_audio {
        AudioOutput::new(audio_shared.clone(), preferred_rate).ok()
    } else {
        None
    };
    let decode_rate = audio_output
        .as_ref()
        .map(|out| out.sample_rate)
        .unwrap_or(preferred_rate);
    let mut audio_decoder = if audio_output.is_some() {
        FfmpegAudioDecoder::open_with_rate(&path, decode_rate)
            .ok()
            .flatten()
    } else {
        None
    };
    let has_audio = audio_output.is_some() && audio_decoder.is_some();

    let mut playing = false;
    let mut looping = false;
    let mut volume = 1.0_f32;
    let mut muted = false;
    let mut speed = 1.0_f32;
    let mut position_us = 0i64;
    let mut wall_clock: Option<(Instant, i64)> = None;
    let mut audio_base: Option<(u64, i64)> = None;
    let mut next_frame: Option<Arc<RgbaFrame>> = None;
    let mut video_eos = false;
    let mut audio_eos = audio_decoder.is_none();
    let mut seek_target_us: Option<i64> = Some(0);
    let mut last_state = Instant::now() - Duration::from_secs(1);

    let _ = events.send(PlayerEvent::State {
        playing,
        position_us,
        duration_us,
        has_audio,
    });

    loop {
        // ---- commands -------------------------------------------------
        loop {
            match commands.try_recv() {
                Ok(PlayerCommand::Play) => {
                    if video_eos && next_frame.is_none() {
                        restart(&mut video, &mut audio_decoder, &audio_shared);
                        video_eos = false;
                        audio_eos = audio_decoder.is_none();
                        position_us = 0;
                    }
                    playing = true;
                    audio_shared.active.store(true, Ordering::Relaxed);
                    wall_clock = Some((Instant::now(), position_us));
                    audio_base = Some((
                        audio_shared.played_frames.load(Ordering::Relaxed),
                        position_us,
                    ));
                    if let Some(output) = &audio_output {
                        use cpal::traits::StreamTrait;
                        let _ = output.stream.play();
                    }
                    last_state = Instant::now() - Duration::from_secs(1);
                }
                Ok(PlayerCommand::Pause) => {
                    position_us = clock(
                        playing,
                        wall_clock,
                        audio_base,
                        &audio_shared.played_frames,
                        decode_rate,
                        speed,
                        has_audio,
                        position_us,
                    );
                    playing = false;
                    audio_shared.active.store(false, Ordering::Relaxed);
                    last_state = Instant::now() - Duration::from_secs(1);
                }
                Ok(PlayerCommand::Seek(target)) => {
                    let clamped = if duration_us > 0 {
                        target.clamp(0, duration_us)
                    } else {
                        target.max(0)
                    };
                    position_us = clamped;
                    audio_shared.active.store(false, Ordering::Relaxed);
                    let _ = video.seek(clamped);
                    if let Some(decoder) = &mut audio_decoder {
                        let _ = decoder.seek(clamped);
                    }
                    audio_shared.clear_buffer();
                    audio_shared.played_frames.store(0, Ordering::Relaxed);
                    audio_base = Some((0, position_us));
                    wall_clock = Some((Instant::now(), position_us));
                    next_frame = None;
                    video_eos = false;
                    audio_eos = audio_decoder.is_none();
                    seek_target_us = Some(clamped);
                    if playing {
                        audio_shared.active.store(true, Ordering::Relaxed);
                    }
                    last_state = Instant::now() - Duration::from_secs(1);
                }
                Ok(PlayerCommand::SetLooping(value)) => looping = value,
                Ok(PlayerCommand::SetVolume(value)) => {
                    volume = value.clamp(0.0, 1.0);
                    audio_shared.set_gain(volume, muted);
                }
                Ok(PlayerCommand::SetMuted(value)) => {
                    muted = value;
                    audio_shared.set_gain(volume, muted);
                }
                Ok(PlayerCommand::SetSpeed(value)) => {
                    position_us = clock(
                        playing,
                        wall_clock,
                        audio_base,
                        &audio_shared.played_frames,
                        decode_rate,
                        speed,
                        has_audio,
                        position_us,
                    );
                    speed = value.clamp(0.25, 4.0);
                    audio_shared.set_speed(speed);
                    wall_clock = Some((Instant::now(), position_us));
                    audio_base = Some((
                        audio_shared.played_frames.load(Ordering::Relaxed),
                        position_us,
                    ));
                }
                Ok(PlayerCommand::Quit) | Err(TryRecvError::Disconnected) => {
                    audio_shared.active.store(false, Ordering::Relaxed);
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }

        // ---- catch up to seek target ----------------------------------
        if let Some(target_us) = seek_target_us.take() {
            let mut chosen: Option<Arc<RgbaFrame>> = None;
            for _ in 0..180 {
                match video.next_frame() {
                    Ok(DecodeOutcome::Frame(frame)) => {
                        let rgba = Arc::new(frame.to_rgba());
                        let pts = rgba.pts_us.unwrap_or(target_us);
                        chosen = Some(rgba);
                        if pts + 35_000 >= target_us {
                            break;
                        }
                    }
                    Ok(DecodeOutcome::Pending) => break,
                    Ok(DecodeOutcome::Eos) => {
                        video_eos = true;
                        break;
                    }
                    Err(err) => {
                        let _ = events.send(PlayerEvent::Error(err.to_string()));
                        video_eos = true;
                        break;
                    }
                }
            }
            if let Some(frame) = chosen {
                let _ = events.send(PlayerEvent::Frame(frame));
            }
            wall_clock = Some((Instant::now(), position_us));
            audio_base = Some((
                audio_shared.played_frames.load(Ordering::Relaxed),
                position_us,
            ));
        }

        // ---- audio refill ---------------------------------------------
        if let Some(decoder) = &mut audio_decoder {
            if !audio_eos {
                loop {
                    let buffered = audio_shared
                        .buffer
                        .lock()
                        .map(|b| b.len())
                        .unwrap_or(usize::MAX);
                    if buffered >= AUDIO_BUFFER_SAMPLES {
                        break;
                    }
                    match decoder.next_chunk() {
                        Ok(Some(chunk)) => {
                            if let Ok(mut buffer) = audio_shared.buffer.lock() {
                                buffer.extend(chunk);
                            }
                        }
                        Ok(None) => {
                            audio_eos = true;
                            break;
                        }
                        Err(err) => {
                            log::warn!("audio decode error: {err}");
                            audio_eos = true;
                            break;
                        }
                    }
                }
            }
        }

        // ---- video ----------------------------------------------------
        let position = clock(
            playing,
            wall_clock,
            audio_base,
            &audio_shared.played_frames,
            decode_rate,
            speed,
            has_audio,
            position_us,
        );

        if playing {
            while next_frame.is_none() && !video_eos {
                match video.next_frame() {
                    Ok(DecodeOutcome::Frame(frame)) => next_frame = Some(Arc::new(frame.to_rgba())),
                    Ok(DecodeOutcome::Pending) => break,
                    Ok(DecodeOutcome::Eos) => {
                        video_eos = true;
                        break;
                    }
                    Err(err) => {
                        let _ = events.send(PlayerEvent::Error(err.to_string()));
                        video_eos = true;
                        break;
                    }
                }
            }

            if let Some(frame) = &next_frame {
                let pts = frame.pts_us.unwrap_or(position);
                if pts <= position + 20_000 {
                    position_us = pts.max(position);
                    let _ = events.send(PlayerEvent::Frame(frame.clone()));
                    next_frame = None;
                }
            }

            let audio_drained = !has_audio
                || (audio_eos
                    && audio_shared
                        .buffer
                        .lock()
                        .map(|b| b.is_empty())
                        .unwrap_or(true));
            if video_eos && next_frame.is_none() && audio_drained {
                if looping {
                    restart(&mut video, &mut audio_decoder, &audio_shared);
                    video_eos = false;
                    audio_eos = audio_decoder.is_none();
                    position_us = 0;
                    wall_clock = Some((Instant::now(), 0));
                    audio_base = Some((audio_shared.played_frames.load(Ordering::Relaxed), 0));
                    seek_target_us = Some(0);
                } else {
                    playing = false;
                    audio_shared.active.store(false, Ordering::Relaxed);
                    position_us = duration_us.max(position_us);
                    let _ = events.send(PlayerEvent::State {
                        playing: false,
                        position_us,
                        duration_us,
                        has_audio,
                    });
                    let _ = events.send(PlayerEvent::Ended);
                }
            }
        } else {
            position_us = position;
        }

        // ---- state ----------------------------------------------------
        if last_state.elapsed() >= Duration::from_millis(100) {
            last_state = Instant::now();
            let reported_pos = if playing { position } else { position_us };
            let clamped_pos = if duration_us > 0 {
                reported_pos.clamp(0, duration_us)
            } else {
                reported_pos.max(0)
            };
            let _ = events.send(PlayerEvent::State {
                playing,
                position_us: clamped_pos,
                duration_us,
                has_audio,
            });
        }

        std::thread::sleep(TICK);
    }
}

fn restart(
    video: &mut FfmpegVideoDecoder,
    audio: &mut Option<FfmpegAudioDecoder>,
    shared: &Arc<AudioShared>,
) {
    let _ = video.seek(0);
    if let Some(audio) = audio {
        let _ = audio.seek(0);
    }
    shared.clear_buffer();
}

#[allow(clippy::too_many_arguments)]
fn clock(
    playing: bool,
    wall: Option<(Instant, i64)>,
    audio: Option<(u64, i64)>,
    played_frames: &AtomicU64,
    sample_rate: u32,
    speed: f32,
    has_audio: bool,
    fallback: i64,
) -> i64 {
    if !playing {
        return fallback;
    }
    if has_audio {
        if let Some((base_frames, base_us)) = audio {
            let frames = played_frames
                .load(Ordering::Relaxed)
                .saturating_sub(base_frames);
            let rate = sample_rate.max(1) as i64;
            return base_us + (frames as i64) * 1_000_000 / rate;
        }
    }
    if let Some((started, base_us)) = wall {
        let elapsed_us = (started.elapsed().as_micros() as f64 * speed as f64) as i64;
        return base_us + elapsed_us;
    }
    fallback
}
