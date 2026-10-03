//! Video playback worker: decodes on its own thread, paces frames against the
//! audio clock (when the file has audio) and streams frames to the UI.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hotview_core::video::ffmpeg::{FfmpegAudioDecoder, FfmpegVideoDecoder};
use hotview_core::video::DecodeOutcome;
use hotview_core::{RgbaFrame, VideoDecoder};

const TICK: Duration = Duration::from_millis(4);
const AUDIO_BUFFER_SAMPLES: usize = 96_000; // 0.5 s of stereo 48 kHz

pub enum PlayerCommand {
    Play,
    Pause,
    Seek(i64),
    SetLooping(bool),
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
            "video opened: {}x{} @ {:.2} fps, {:.2}s{}",
            info.width,
            info.height,
            info.fps.unwrap_or(0.0),
            info.duration_us.unwrap_or(0) as f64 / 1_000_000.0,
            if info.has_audio { ", with audio" } else { "" }
        );

        let join = std::thread::Builder::new()
            .name("hotview-player".into())
            .spawn(move || run(path, rx, event_tx))
            .map_err(|err| err.to_string())?;

        Ok(Self {
            commands: tx,
            events: event_rx,
            join: Some(join),
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
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(PlayerCommand::Quit);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct AudioOutput {
    _stream: cpal::Stream,
}

impl AudioOutput {
    fn new(
        sample_rate: u32,
        channels: u16,
        buffer: Arc<Mutex<VecDeque<f32>>>,
        played_frames: Arc<AtomicU64>,
    ) -> Result<Self, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output".to_string())?;
        let config = cpal::StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };
        let channels = channels.max(1) as usize;
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    let mut guard = match buffer.try_lock() {
                        Ok(guard) => guard,
                        Err(_) => {
                            data.fill(0.0);
                            return;
                        }
                    };
                    for sample in data.iter_mut() {
                        *sample = guard.pop_front().unwrap_or(0.0);
                    }
                    drop(guard);
                    played_frames.fetch_add((data.len() / channels) as u64, Ordering::Relaxed);
                },
                |err| log::warn!("audio output error: {err}"),
                None,
            )
            .map_err(|err| err.to_string())?;
        stream.play().map_err(|err| err.to_string())?;
        Ok(Self { _stream: stream })
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

    let mut audio_decoder = FfmpegAudioDecoder::open(&path).ok().flatten();
    let audio_buffer = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(
        AUDIO_BUFFER_SAMPLES * 2,
    )));
    let played_frames = Arc::new(AtomicU64::new(0));
    let audio_output = audio_decoder.as_ref().and_then(|decoder| {
        AudioOutput::new(
            decoder.sample_rate,
            decoder.channels,
            audio_buffer.clone(),
            played_frames.clone(),
        )
        .ok()
    });
    let has_audio = audio_output.is_some();

    let mut playing = false;
    let mut looping = false;
    let mut position_us = 0i64;
    let mut wall_clock: Option<(Instant, i64)> = None;
    let mut audio_base: Option<(u64, i64)> = None;
    let mut next_frame: Option<Arc<RgbaFrame>> = None;
    let mut video_eos = false;
    let mut audio_eos = audio_decoder.is_none();
    let mut display_after_seek = true;
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
                        restart(&mut video, &mut audio_decoder, &audio_buffer);
                        video_eos = false;
                        audio_eos = audio_decoder.is_none();
                        position_us = 0;
                    }
                    playing = true;
                    log::info!("playback started (audio output: {has_audio})");
                    wall_clock = Some((Instant::now(), position_us));
                    audio_base = Some((played_frames.load(Ordering::Relaxed), position_us));
                    if let Some(output) = &audio_output {
                        use cpal::traits::StreamTrait;
                        let _ = output._stream.play();
                    }
                }
                Ok(PlayerCommand::Pause) => {
                    position_us = clock(
                        playing,
                        wall_clock,
                        audio_base,
                        &played_frames,
                        has_audio,
                        position_us,
                    );
                    playing = false;
                    if let Some(output) = &audio_output {
                        use cpal::traits::StreamTrait;
                        let _ = output._stream.pause();
                    }
                }
                Ok(PlayerCommand::Seek(target)) => {
                    position_us = target.max(0);
                    let _ = video.seek(position_us);
                    if let Some(decoder) = &mut audio_decoder {
                        let _ = decoder.seek(position_us);
                    }
                    if let Ok(mut buffer) = audio_buffer.lock() {
                        buffer.clear();
                    }
                    played_frames.store(0, Ordering::Relaxed);
                    audio_base = Some((0, position_us));
                    wall_clock = Some((Instant::now(), position_us));
                    next_frame = None;
                    video_eos = false;
                    audio_eos = audio_decoder.is_none();
                    display_after_seek = true;
                }
                Ok(PlayerCommand::SetLooping(value)) => looping = value,
                Ok(PlayerCommand::Quit) | Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => break,
            }
        }

        // ---- audio refill ---------------------------------------------
        if let Some(decoder) = &mut audio_decoder {
            if !audio_eos {
                loop {
                    let buffered = audio_buffer.lock().map(|b| b.len()).unwrap_or(usize::MAX);
                    if buffered >= AUDIO_BUFFER_SAMPLES {
                        break;
                    }
                    match decoder.next_chunk() {
                        Ok(Some(chunk)) => {
                            if let Ok(mut buffer) = audio_buffer.lock() {
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
            &played_frames,
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
                    position_us = pts;
                    let _ = events.send(PlayerEvent::Frame(frame.clone()));
                    next_frame = None;
                }
            }

            let audio_drained = !has_audio
                || (audio_eos
                    && audio_buffer.lock().map(|b| b.is_empty()).unwrap_or(true));
            if video_eos && next_frame.is_none() && audio_drained {
                if looping {
                    restart(&mut video, &mut audio_decoder, &audio_buffer);
                    video_eos = false;
                    audio_eos = audio_decoder.is_none();
                    position_us = 0;
                    wall_clock = Some((Instant::now(), 0));
                    audio_base = Some((played_frames.load(Ordering::Relaxed), 0));
                    display_after_seek = true;
                } else {
                    playing = false;
                    if let Some(output) = &audio_output {
                        use cpal::traits::StreamTrait;
                        let _ = output._stream.pause();
                    }
                    let _ = events.send(PlayerEvent::Ended);
                }
            }
        } else if display_after_seek {
            // Show the frame at the seek target while paused.
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
                if next_frame.is_some() {
                    break;
                }
            }
            if let Some(frame) = next_frame.take() {
                position_us = frame.pts_us.unwrap_or(position_us);
                let _ = events.send(PlayerEvent::Frame(frame));
                display_after_seek = false;
            }
        } else {
            position_us = position;
        }

        // ---- state ----------------------------------------------------
        if last_state.elapsed() >= Duration::from_millis(150) {
            last_state = Instant::now();
            let _ = events.send(PlayerEvent::State {
                playing,
                position_us: if playing { position } else { position_us },
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
    buffer: &Arc<Mutex<VecDeque<f32>>>,
) {
    let _ = video.seek(0);
    if let Some(audio) = audio {
        let _ = audio.seek(0);
    }
    if let Ok(mut buffer) = buffer.lock() {
        buffer.clear();
    }
}

fn clock(
    playing: bool,
    wall: Option<(Instant, i64)>,
    audio: Option<(u64, i64)>,
    played_frames: &AtomicU64,
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
            return base_us + (frames as i64) * 1_000_000 / 48_000;
        }
    }
    if let Some((started, base_us)) = wall {
        return base_us + started.elapsed().as_micros() as i64;
    }
    fallback
}
