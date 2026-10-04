//! One render thread per media surface: owns the wgpu surface, the renderer
//! and the playback session (MediaCodec video + audio, AAudio output).

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hotview_core::video::{AudioDecoder, AudioOutcome, DecodeOutcome, VideoDecoder};
use hotview_core::{ImageAnimation, MediaFrame, RgbaFrame, YuvInfo};
use hotview_render::wgpu;
use hotview_render::{fit_transform, surface_from_android_window, GpuContext, MediaRenderer};

use crate::audio::AaudioOutput;
use crate::events::EventSink;

/// A raw `ANativeWindow*` that we promise to only touch from the render thread.
pub struct SendWindow(pub *mut c_void);

// SAFETY: the pointer is handed to the render thread and never used from
// anywhere else while the surface exists.
unsafe impl Send for SendWindow {}

pub enum Command {
    AttachSurface {
        window: SendWindow,
        width: u32,
        height: u32,
        ack: Sender<()>,
    },
    DetachSurface {
        ack: Sender<()>,
    },
    Resize {
        width: u32,
        height: u32,
    },
    SetFrame(MediaFrame),
    SetAnimation(Arc<[u8]>),
    SetVideo {
        video: Box<dyn VideoDecoder>,
        audio: Option<Box<dyn AudioDecoder>>,
    },
    Viewport {
        scale: f32,
        pan: [f32; 2],
        fill: bool,
    },
    SetPlaying(bool),
    SetImageAnimationPlaying(bool),
    Seek(i64),
    SetLooping(bool),
    Quit {
        ack: Sender<()>,
    },
}

/// State shared with the JNI getters (lock free).
pub struct Shared {
    pub position_us: AtomicI64,
    pub duration_us: AtomicI64,
    pub playing: AtomicBool,
    pub prepared: AtomicBool,
    pub has_audio: AtomicBool,
    pub width: AtomicU32,
    pub height: AtomicU32,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            position_us: AtomicI64::new(0),
            duration_us: AtomicI64::new(0),
            playing: AtomicBool::new(false),
            prepared: AtomicBool::new(false),
            has_audio: AtomicBool::new(false),
            width: AtomicU32::new(0),
            height: AtomicU32::new(0),
        })
    }
}

/// Video + audio playback with the audio device as the master clock.
struct PlaybackSession {
    video: Box<dyn VideoDecoder>,
    audio: Option<Box<dyn AudioDecoder>>,
    output: Option<AaudioOutput>,
    frames: VecDeque<MediaFrame>,
    position_us: i64,
    playing: bool,
    looping: bool,
    video_eos: bool,
    audio_eos: bool,
    ended_sent: bool,
    needs_display: bool,
    first_frame_sent: bool,
    wall_clock: Option<(Instant, i64)>,
    audio_base: Option<(u64, i64)>,
    /// Wall-clock anchor used to interpolate between coarse audio reports.
    clock_anchor: Option<(Instant, i64)>,
    /// PCM the ring was too full to accept last time; pushed first next round
    /// so no sample is ever dropped (dropped samples are audible clicks).
    audio_pending: Vec<f32>,
    /// `(decoded, displayed, dropped)` frame counters.
    stats: (u64, u64, u64),
    /// Render cost since the last stats log: `(total µs, renders)`.
    render_stats: (u64, u64),
    /// `(when, decoded, displayed, dropped, position_us)` of the last stats log.
    stats_last: Option<(Instant, u64, u64, u64, i64)>,
    last_pts_us: i64,
}

/// Open the audio device on a worker thread: a broken audio HAL can make
/// `openStream` hang forever, and the render thread must not be blocked by it.
fn open_audio_output(sample_rate: u32, channels: u16) -> Option<AaudioOutput> {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new()
        .name("hotview-audio-open".into())
        .spawn(move || {
            let _ = tx.send(AaudioOutput::new(sample_rate, channels));
        });
    if spawned.is_err() {
        return None;
    }
    match rx.recv_timeout(Duration::from_millis(700)) {
        Ok(Ok(output)) => Some(output),
        Ok(Err(err)) => {
            log::warn!("audio output unavailable: {err}");
            None
        }
        Err(_) => {
            log::warn!("audio output did not open within 700 ms; playing without sound");
            None
        }
    }
}

impl PlaybackSession {
    fn new(video: Box<dyn VideoDecoder>, audio: Option<Box<dyn AudioDecoder>>) -> Self {
        // Opening the output can fail (no audio device / busy); the video then
        // falls back to the wall clock.
        let output = audio
            .as_ref()
            .and_then(|decoder| open_audio_output(decoder.sample_rate(), decoder.channels()));
        if let Some(output) = &output {
            log::info!(
                "audio output: {} Hz, {} ch (track: {} Hz, {} ch)",
                output.sample_rate(),
                output.channels(),
                audio.as_ref().map(|decoder| decoder.sample_rate()).unwrap_or(0),
                audio.as_ref().map(|decoder| decoder.channels()).unwrap_or(0),
            );
        }
        Self {
            video,
            audio,
            output,
            frames: VecDeque::new(),
            position_us: 0,
            playing: false,
            looping: false,
            video_eos: false,
            audio_eos: false,
            ended_sent: false,
            needs_display: true,
            first_frame_sent: false,
            wall_clock: None,
            audio_base: None,
            clock_anchor: None,
            audio_pending: Vec::new(),
            stats: (0, 0, 0),
            render_stats: (0, 0),
            stats_last: None,
            last_pts_us: 0,
        }
    }

    fn has_audio(&self) -> bool {
        self.output.is_some()
    }

    /// Raw clock: the audio device position, or the wall clock without audio.
    fn raw_clock(&self) -> i64 {
        if let (Some(output), Some((base_frames, base_us))) = (&self.output, self.audio_base) {
            let frames = output.played_frames().saturating_sub(base_frames);
            return base_us + (frames as i64) * 1_000_000 / output.sample_rate().max(1) as i64;
        }
        match self.wall_clock {
            Some((started, media)) => media + started.elapsed().as_micros() as i64,
            None => self.position_us,
        }
    }

    /// Smoothed playback position.
    ///
    /// The audio device reports its position in callback-sized steps (tens of
    /// ms), which makes several video frames "due" at once and forces drops.
    /// Interpolate with the wall clock between reports, re-anchor when the raw
    /// clock catches up, and never run more than 100 ms ahead of it.
    fn clock_position(&mut self) -> i64 {
        let target = self.raw_clock();
        let now = Instant::now();
        let mut smoothed = match self.clock_anchor {
            Some((anchor, position)) => position + anchor.elapsed().as_micros() as i64,
            None => target,
        };
        if target >= smoothed || smoothed > target + 100_000 {
            smoothed = target;
            self.clock_anchor = Some((now, target));
        }
        smoothed.max(0)
    }

    fn fill_video(&mut self, events: &Option<EventSink>) {
        // Buffer up to ~32 megapixels (roughly a second of 720p): enough to
        // absorb a bursty decoder, while 4K frames stay capped in memory.
        const MAX_QUEUED_PIXELS: u64 = 32_000_000;
        let mut queued: u64 = self
            .frames
            .iter()
            .map(|frame| {
                let (width, height) = frame.dimensions();
                width as u64 * height as u64
            })
            .sum();
        let mut attempts = 0;
        let started = Instant::now();
        while queued < MAX_QUEUED_PIXELS && attempts < 8 {
            // Software decoders (dav1d AV1, ...) can take tens of ms per
            // frame; never block the pump longer than a few ms so audio stays
            // fed and displayed frames stay on time.
            if attempts > 0 && started.elapsed() >= Duration::from_millis(6) {
                break;
            }
            attempts += 1;
            match self.video.next_frame() {
                Ok(DecodeOutcome::Frame(frame)) => {
                    let (width, height) = frame.dimensions();
                    queued += width as u64 * height as u64;
                    let pts = frame.pts_us().unwrap_or(self.last_pts_us);
                    self.last_pts_us = pts;
                    self.stats.0 += 1;
                    self.frames.push_back(frame);
                }
                Ok(DecodeOutcome::Pending) => break,
                Ok(DecodeOutcome::Eos) => {
                    self.video_eos = true;
                    break;
                }
                Err(err) => {
                    if let Some(events) = events {
                        events.error(2, &err.to_string());
                    }
                    self.video_eos = true;
                    break;
                }
            }
        }
    }

    fn fill_audio(&mut self) {
        let Some(output) = &self.output else {
            self.audio_eos = true;
            return;
        };
        if self.audio_eos {
            return;
        }
        let Some(audio) = &mut self.audio else {
            self.audio_eos = true;
            return;
        };

        // Push whatever the ring refused last time before decoding more.
        if !self.audio_pending.is_empty() {
            let written = output.push(&self.audio_pending);
            if written >= self.audio_pending.len() {
                self.audio_pending.clear();
            } else {
                self.audio_pending.drain(..written);
                return;
            }
        }

        // Keep about a second of PCM queued up. `push` reports how much the
        // ring accepted; the remainder is kept for the next pump instead of
        // being dropped (dropping mid-chunk is what made the audio crackle).
        let target = output.free_samples();
        let mut decoded = 0usize;
        let started = Instant::now();
        while decoded < target {
            // Same time budget as video: keep the pump responsive.
            if decoded > 0 && started.elapsed() >= Duration::from_millis(4) {
                break;
            }
            match audio.next_chunk() {
                Ok(AudioOutcome::Samples(samples)) => {
                    let written = output.push(&samples);
                    decoded += written;
                    if written < samples.len() {
                        self.audio_pending.extend_from_slice(&samples[written..]);
                        break;
                    }
                    if written == 0 {
                        break;
                    }
                }
                Ok(AudioOutcome::Pending) => break,
                Ok(AudioOutcome::Eos) => {
                    self.audio_eos = true;
                    break;
                }
                Err(err) => {
                    log::warn!("audio decode failed: {err}");
                    self.audio_eos = true;
                    break;
                }
            }
        }
    }

    fn display(
        &mut self,
        frame: MediaFrame,
        ctx: &GpuContext,
        renderer: &mut MediaRenderer,
        dirty: &mut bool,
        events: &Option<EventSink>,
    ) {
        renderer.set_frame(&ctx.device, &ctx.queue, &frame);
        self.position_us = frame.pts_us().unwrap_or(self.last_pts_us);
        self.last_pts_us = self.position_us;
        *dirty = true;
        if !self.first_frame_sent {
            self.first_frame_sent = true;
            if let Some(events) = events {
                events.first_frame();
            }
        }
    }

    fn pump(
        &mut self,
        ctx: &GpuContext,
        mut renderer: Option<&mut MediaRenderer>,
        shared: &Shared,
        dirty: &mut bool,
        events: &Option<EventSink>,
    ) {
        let has_renderer = renderer.is_some();
        // Without a surface (background audio) video is not decoded; it is
        // re-synced with the clock when a surface comes back.
        if has_renderer {
            self.fill_video(events);
        }
        self.fill_audio();

        if self.playing {
            let position = self.clock_position();
            if let Some(renderer) = renderer.as_deref_mut() {
                // Show the newest frame that is due and drop stale ones: when
                // the clock jumps ahead (or we were busy), catching up must not
                // play the backlog back as a fast-forward burst.
                let mut due_frame: Option<MediaFrame> = None;
                while let Some(front) = self.frames.front() {
                    if front.pts_us().unwrap_or(self.last_pts_us) <= position + 20_000 {
                        if due_frame.is_some() {
                            self.stats.2 += 1;
                        }
                        due_frame = self.frames.pop_front();
                    } else {
                        break;
                    }
                }
                if let Some(frame) = due_frame {
                    self.stats.1 += 1;
                    self.display(frame, ctx, renderer, dirty, events);
                }
            }

            let audio_drained = self
                .output
                .as_ref()
                .map(|output| self.audio_eos && output.buffered_samples() == 0)
                .unwrap_or(true);
            let video_done = self.video_eos || !has_renderer;

            if self.frames.is_empty() && video_done && audio_drained {
                if self.looping {
                    self.restart(events);
                } else if !self.ended_sent {
                    self.ended_sent = true;
                    self.playing = false;
                    self.wall_clock = None;
                    if let Some(output) = &self.output {
                        output.request_pause();
                    }
                    if let Some(events) = events {
                        events.ended();
                    }
                }
            }
        } else if self.needs_display {
            if let Some(renderer) = renderer.as_deref_mut() {
                if let Some(frame) = self.frames.pop_front() {
                    self.display(frame, ctx, renderer, dirty, events);
                    self.needs_display = false;
                }
            }
        }

        shared
            .position_us
            .store(self.clock_position().max(0), Ordering::Relaxed);
        shared.playing.store(self.playing, Ordering::Relaxed);

        // Per-2s playback stats so on-device stutter can be quantified.
        if self.playing
            && self
                .stats_last
                .map(|(when, _, _, _, _)| when.elapsed() >= Duration::from_secs(2))
                .unwrap_or(true)
        {
            let (decoded, displayed, dropped) = self.stats;
            let (last_decoded, last_displayed, last_dropped, last_position) = self
                .stats_last
                .map(|(_, decoded, displayed, dropped, position)| {
                    (decoded, displayed, dropped, position)
                })
                .unwrap_or((0, 0, 0, self.position_us));
            let underruns = self
                .output
                .as_ref()
                .map(|output| output.underruns())
                .unwrap_or(0);
            let (render_us, renders) = self.render_stats;
            let average_render_ms = if renders > 0 {
                render_us as f64 / renders as f64 / 1000.0
            } else {
                0.0
            };
            let wall_ms = self
                .stats_last
                .map(|(when, _, _, _, _)| when.elapsed().as_millis() as i64)
                .unwrap_or(2000);
            let media_ms = (self.position_us - last_position).max(0) / 1000;
            log::info!(
                "playback 2s: decoded {}, displayed {}, dropped {}, audio underruns {}, renders {renders} ({average_render_ms:.1} ms avg), clock {media_ms}/{wall_ms} ms",
                decoded - last_decoded,
                displayed - last_displayed,
                dropped - last_dropped,
                underruns,
            );
            self.stats_last = Some((Instant::now(), decoded, displayed, dropped, self.position_us));
            self.render_stats = (0, 0);
        }
    }

    /// Record how long one frame render took (draw + present).
    fn add_render_time(&mut self, micros: u64) {
        self.render_stats.0 += micros;
        self.render_stats.1 += 1;
    }

    fn seek(&mut self, position_us: i64, events: &Option<EventSink>) {
        let position_us = position_us.max(0);
        if let Err(err) = self.video.seek(position_us) {
            if let Some(events) = events {
                events.error(3, &err.to_string());
            }
            return;
        }
        if let Some(audio) = &mut self.audio {
            let _ = audio.seek(position_us);
        }
        if let Some(output) = &self.output {
            output.request_pause();
            output.request_flush();
            output.clear();
        }
        self.frames.clear();
        self.audio_pending.clear();
        self.position_us = position_us;
        self.last_pts_us = position_us;
        self.video_eos = false;
        self.audio_eos = self.audio.is_none();
        self.ended_sent = false;
        self.needs_display = true;
        self.wall_clock = self.playing.then(|| (Instant::now(), position_us));
        self.audio_base = Some((
            self.output
                .as_ref()
                .map(|output| output.played_frames())
                .unwrap_or(0),
            position_us,
        ));
        self.clock_anchor = None;
        if self.playing {
            if let Some(output) = &self.output {
                output.request_start();
            }
        }
    }

    fn restart(&mut self, events: &Option<EventSink>) {
        self.seek(0, events);
        self.playing = true;
        self.ended_sent = false;
        self.wall_clock = Some((Instant::now(), 0));
        if let Some(output) = &self.output {
            output.request_start();
        }
    }

    /// Re-align the video decoder with the clock after a surface came back
    /// (the video was not decoded while playing in the background).
    fn resync_video(&mut self, position_us: i64) {
        let _ = self.video.seek(position_us);
        self.frames.clear();
        self.video_eos = false;
        self.ended_sent = false;
        self.needs_display = true;
        self.last_pts_us = position_us;
    }

    fn set_playing(&mut self, playing: bool) {
        if playing == self.playing {
            return;
        }
        if playing {
            if self.video_eos && self.frames.is_empty() {
                self.seek(0, &None);
                self.video_eos = false;
            }
            let position = self.position_us;
            self.wall_clock = Some((Instant::now(), position));
            self.audio_base = Some((
                self.output
                    .as_ref()
                    .map(|output| output.played_frames())
                    .unwrap_or(0),
                position,
            ));
            self.clock_anchor = None;
            if let Some(output) = &self.output {
                output.request_start();
            }
            self.ended_sent = false;
            self.needs_display = false;
        } else {
            self.position_us = self.clock_position();
            self.wall_clock = None;
            if let Some(output) = &self.output {
                output.request_pause();
            }
        }
        self.playing = playing;
    }
}

/// Owns one `ANativeWindow*` reference.
///
/// Declared *after* the wgpu surface inside [`SurfaceState`] on purpose: struct
/// fields drop in declaration order, so the Vulkan surface is destroyed while
/// the window is still valid. Releasing the window first is a use-after-free
/// that crashes the process when a surface is recycled.
struct WindowRef(*mut c_void);

impl Drop for WindowRef {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                ndk_sys::ANativeWindow_release(self.0 as *mut ndk_sys::ANativeWindow);
            }
        }
    }
}

struct SurfaceState {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Must stay last: dropped after `surface`.
    _window: WindowRef,
    /// Only the first acquire failure is logged, otherwise a stuck surface
    /// would flood logcat every frame.
    logged_failure: bool,
    /// Set once the first frame actually reached the screen.
    logged_success: bool,
}

struct ImageAnimationPlayback {
    decoder: ImageAnimation,
    next_frame_at: Instant,
    playing: bool,
}

const MAX_ANIMATION_TEXTURE_DIM: u32 = 3072;
/// Animated frames are decoded at up to this many pixels before scaling.
const MAX_ANIMATION_PIXELS: u64 = 24_000_000;

/// Entry point of the render thread.
pub fn run(
    ctx: Arc<GpuContext>,
    rx: Receiver<Command>,
    shared: Arc<Shared>,
    events: Option<EventSink>,
) {
    let mut surface: Option<SurfaceState> = None;
    let mut renderer: Option<MediaRenderer> = None;
    let mut pending_frame: Option<MediaFrame> = None;
    let mut session: Option<PlaybackSession> = None;
    let mut image_animation: Option<ImageAnimationPlayback> = None;
    let mut image_animation_playing = true;
    let mut scale = 1.0f32;
    let mut pan = [0.0f32; 2];
    let mut fill = false;
    let mut dirty = false;

    loop {
        let mut timeout = if session.as_ref().map(|s| s.playing).unwrap_or(false) {
            Duration::from_millis(4)
        } else {
            Duration::from_millis(50)
        };
        if renderer.is_some()
            && let Some(animation) = image_animation.as_ref().filter(|animation| animation.playing)
        {
            timeout = timeout.min(
                animation
                    .next_frame_at
                    .saturating_duration_since(Instant::now()),
            );
        }

        match rx.recv_timeout(timeout) {
            Ok(command) => {
                let mut quit = false;
                handle_command_guarded(
                    command,
                    &ctx,
                    &mut surface,
                    &mut renderer,
                    &mut pending_frame,
                    &mut session,
                    &mut image_animation,
                    &mut image_animation_playing,
                    &mut scale,
                    &mut pan,
                    &mut fill,
                    &mut dirty,
                    &shared,
                    &events,
                    &mut quit,
                );
                if quit {
                    break;
                }
                while let Ok(command) = rx.try_recv() {
                    handle_command_guarded(
                        command,
                        &ctx,
                        &mut surface,
                        &mut renderer,
                        &mut pending_frame,
                        &mut session,
                        &mut image_animation,
                        &mut image_animation_playing,
                        &mut scale,
                        &mut pan,
                        &mut fill,
                        &mut dirty,
                        &shared,
                        &events,
                        &mut quit,
                    );
                    if quit {
                        break;
                    }
                }
                if quit {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if let Some(session) = session.as_mut() {
            session.pump(&ctx, renderer.as_mut(), &shared, &mut dirty, &events);
        }
        if renderer.is_some() {
            pump_image_animation(
                &ctx,
                &mut renderer,
                &mut pending_frame,
                &mut image_animation,
                &shared,
                &mut dirty,
                &events,
            );
        }

        if dirty {
            if let (Some(state), Some(renderer)) = (&mut surface, &mut renderer) {
                let transform = fit_transform(
                    (state.config.width, state.config.height),
                    renderer.frame_size(),
                    scale,
                    pan,
                    fill,
                );
                let started = Instant::now();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    render_frame(&ctx, state, renderer, transform, &events);
                }));
                let elapsed_us = started.elapsed().as_micros() as u64;
                if let Some(session) = session.as_mut() {
                    session.add_render_time(elapsed_us);
                }
                if let Err(payload) = result {
                    report_panic(&events, "render frame", &*payload);
                }
            }
            dirty = false;
        }
    }

    // Make sure the GPU is done before the surface goes away.
    let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
}

fn set_still_frame(
    frame: MediaFrame,
    ctx: &GpuContext,
    renderer: &mut Option<MediaRenderer>,
    pending_frame: &mut Option<MediaFrame>,
    shared: &Shared,
    dirty: &mut bool,
) {
    let dimensions = frame.dimensions();
    if let Some(renderer) = renderer.as_mut() {
        renderer.set_frame(&ctx.device, &ctx.queue, &frame);
        *dirty = true;
    } else {
        *pending_frame = Some(frame);
    }
    shared.width.store(dimensions.0, Ordering::Relaxed);
    shared.height.store(dimensions.1, Ordering::Relaxed);
}

fn reset_still_playback(shared: &Shared) {
    shared.duration_us.store(0, Ordering::Relaxed);
    shared.position_us.store(0, Ordering::Relaxed);
    shared.playing.store(false, Ordering::Relaxed);
    shared.prepared.store(false, Ordering::Relaxed);
    shared.has_audio.store(false, Ordering::Relaxed);
}

fn decode_static_animation_frame(bytes: &[u8]) -> Result<RgbaFrame, String> {
    hotview_core::decode_bytes(bytes)
        .map(|frame| hotview_core::scale_to_fit(frame, MAX_ANIMATION_TEXTURE_DIM))
        .map_err(|err| err.to_string())
}

fn advance_image_animation(
    animation: &mut ImageAnimationPlayback,
) -> Result<Option<RgbaFrame>, String> {
    let mut timed = animation
        .decoder
        .next_frame()
        .map_err(|err| err.to_string())?;
    if timed.is_none() {
        animation
            .decoder
            .rewind()
            .map_err(|err| err.to_string())?;
        timed = animation
            .decoder
            .next_frame()
            .map_err(|err| err.to_string())?;
    }
    Ok(timed.map(|frame| {
        animation.next_frame_at = Instant::now() + frame.delay;
        frame.frame
    }))
}

fn pump_image_animation(
    ctx: &GpuContext,
    renderer: &mut Option<MediaRenderer>,
    pending_frame: &mut Option<MediaFrame>,
    image_animation: &mut Option<ImageAnimationPlayback>,
    shared: &Shared,
    dirty: &mut bool,
    events: &Option<EventSink>,
) {
    let now = Instant::now();
    let due = image_animation
        .as_ref()
        .map(|animation| animation.playing && now >= animation.next_frame_at)
        .unwrap_or(false);
    if !due {
        return;
    }

    let result = advance_image_animation(image_animation.as_mut().expect("due animation"));
    match result {
        Ok(Some(frame)) => set_still_frame(
            MediaFrame::Rgba(frame),
            ctx,
            renderer,
            pending_frame,
            shared,
            dirty,
        ),
        Ok(None) => *image_animation = None,
        Err(err) => {
            log::warn!("animated image playback stopped: {err}");
            if let Some(events) = events {
                events.error(5, &err);
            }
            *image_animation = None;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    command: Command,
    ctx: &GpuContext,
    surface: &mut Option<SurfaceState>,
    renderer: &mut Option<MediaRenderer>,
    pending_frame: &mut Option<MediaFrame>,
    session: &mut Option<PlaybackSession>,
    image_animation: &mut Option<ImageAnimationPlayback>,
    image_animation_playing: &mut bool,
    scale: &mut f32,
    pan: &mut [f32; 2],
    fill: &mut bool,
    dirty: &mut bool,
    shared: &Shared,
    events: &Option<EventSink>,
    quit: &mut bool,
) {
    match command {
        Command::AttachSurface {
            window,
            width,
            height,
            ack,
        } => {
            let result = unsafe { surface_from_android_window(&ctx.instance, window.0) };
            match result {
                Ok(new_surface) => {
                    let caps = new_surface.get_capabilities(&ctx.adapter);
                    let format = caps
                        .formats
                        .iter()
                        .copied()
                        .find(|format| !format.is_srgb())
                        .or_else(|| caps.formats.first().copied())
                        .unwrap_or(wgpu::TextureFormat::Rgba8Unorm);
                    // The viewer composites onto its own (black) background, so
                    // the swap chain stays opaque: transparent image pixels are
                    // blended by the media shader, never by the window compositor.
                    let alpha_mode = if caps
                        .alpha_modes
                        .contains(&wgpu::CompositeAlphaMode::Opaque)
                    {
                        wgpu::CompositeAlphaMode::Opaque
                    } else {
                        caps.alpha_modes
                            .first()
                            .copied()
                            .unwrap_or(wgpu::CompositeAlphaMode::Auto)
                    };
                    let config = wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format,
                        color_space: wgpu::SurfaceColorSpace::Auto,
                        width: width.max(1),
                        height: height.max(1),
                        present_mode: wgpu::PresentMode::Fifo,
                        alpha_mode,
                        view_formats: vec![],
                        desired_maximum_frame_latency: 2,
                    };
                    new_surface.configure(&ctx.device, &config);
                    log::info!(
                        "surface attached: {width}x{height}, format {format:?}, alpha {alpha_mode:?}"
                    );
                    let mut new_renderer = MediaRenderer::new(&ctx.device, format);
                    if let Some(frame) = pending_frame.take() {
                        new_renderer.set_frame(&ctx.device, &ctx.queue, &frame);
                        shared
                            .width
                            .store(new_renderer.frame_size().0, Ordering::Relaxed);
                        shared
                            .height
                            .store(new_renderer.frame_size().1, Ordering::Relaxed);
                    }
                    *renderer = Some(new_renderer);
                    *surface = Some(SurfaceState {
                        surface: new_surface,
                        config,
                        _window: WindowRef(window.0),
                        logged_failure: false,
                        logged_success: false,
                    });
                    // Video was not decoded while there was no surface; point
                    // the decoder at the current clock position again.
                    if let Some(session) = session.as_mut() {
                        if session.playing {
                            let position = session.clock_position();
                            session.resync_video(position);
                        }
                    }
                    *dirty = true;
                }
                Err(err) => {
                    if let Some(events) = events {
                        events.error(1, &err);
                    }
                }
            }
            let _ = ack.send(());
        }
        Command::DetachSurface { ack } => {
            let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
            *surface = None;
            *renderer = None;
            let _ = ack.send(());
        }
        Command::Resize { width, height } => {
            if let Some(state) = surface {
                state.config.width = width.max(1);
                state.config.height = height.max(1);
                state.surface.configure(&ctx.device, &state.config);
                *dirty = true;
            }
        }
        Command::SetFrame(frame) => {
            let (frame_width, frame_height) = frame.dimensions();
            log::info!("still frame set: {frame_width}x{frame_height}");
            set_still_frame(frame, ctx, renderer, pending_frame, shared, dirty);
            *session = None;
            *image_animation = None;
            reset_still_playback(shared);
        }
        Command::SetAnimation(bytes) => {
            *session = None;
            match ImageAnimation::from_bytes(
                Arc::clone(&bytes),
                MAX_ANIMATION_TEXTURE_DIM,
                MAX_ANIMATION_PIXELS,
            ) {
                Ok(Some(decoder)) => {
                    let (width, height) = decoder.dimensions();
                    log::info!("animated image set: {width}x{height}");
                    *image_animation = Some(ImageAnimationPlayback {
                        decoder,
                        next_frame_at: Instant::now(),
                        playing: *image_animation_playing,
                    });
                    // Show the first frame right away (even while paused) so a
                    // preloaded, inactive page still has a preview during swipes.
                    if let Ok(Some(frame)) =
                        advance_image_animation(image_animation.as_mut().expect("just set"))
                    {
                        set_still_frame(
                            MediaFrame::Rgba(frame),
                            ctx,
                            renderer,
                            pending_frame,
                            shared,
                            dirty,
                        );
                    }
                }
                Ok(None) => {
                    // Single-frame GIF or still WebP: display the first frame.
                    *image_animation = None;
                    match decode_static_animation_frame(&bytes) {
                        Ok(frame) => {
                            set_still_frame(
                                MediaFrame::Rgba(frame),
                                ctx,
                                renderer,
                                pending_frame,
                                shared,
                                dirty,
                            );
                        }
                        Err(err) => {
                            log::warn!("static frame decode failed: {err}");
                            if let Some(events) = events {
                                events.error(7, &err);
                            }
                        }
                    }
                }
                Err(err) => {
                    // Decoder cannot stream the animation; show its first frame.
                    log::warn!("animation decode failed ({err}); showing first frame");
                    *image_animation = None;
                    if let Ok(frame) = decode_static_animation_frame(&bytes) {
                        set_still_frame(
                            MediaFrame::Rgba(frame),
                            ctx,
                            renderer,
                            pending_frame,
                            shared,
                            dirty,
                        );
                    } else if let Some(events) = events {
                        events.error(7, &err.to_string());
                    }
                }
            }
            reset_still_playback(shared);
        }
        Command::SetImageAnimationPlaying(playing) => {
            *image_animation_playing = playing;
            if let Some(animation) = image_animation.as_mut() {
                if playing && !animation.playing {
                    // Do not replay every frame that elapsed while paused.
                    animation.next_frame_at = Instant::now();
                }
                animation.playing = playing;
                if playing {
                    *dirty = true;
                }
            }
        }
        Command::SetVideo { video, audio } => {
            let info = video.info().clone();
            let mut new_session = PlaybackSession::new(video, audio);
            let has_audio = new_session.has_audio();
            if let Some(audio) = new_session.audio.as_mut() {
                log::info!(
                    "playing audio at {} Hz, {} ch",
                    audio.sample_rate(),
                    audio.channels()
                );
            }
            new_session.audio_eos = new_session.audio.is_none();
            log::info!(
                "video ready: {}x{}, {:.1}s, audio={has_audio}",
                info.width,
                info.height,
                info.duration_us.unwrap_or(0) as f64 / 1e6
            );
            *session = Some(new_session);
            *image_animation = None;
            shared
                .duration_us
                .store(info.duration_us.unwrap_or(0).max(0), Ordering::Relaxed);
            shared.position_us.store(0, Ordering::Relaxed);
            shared.width.store(info.width, Ordering::Relaxed);
            shared.height.store(info.height, Ordering::Relaxed);
            shared.playing.store(false, Ordering::Relaxed);
            shared.prepared.store(true, Ordering::Relaxed);
            shared.has_audio.store(has_audio, Ordering::Relaxed);
            if let Some(events) = events {
                events.prepared(info.duration_us.unwrap_or(0) / 1000);
            }
        }
        Command::Viewport {
            scale: new_scale,
            pan: new_pan,
            fill: new_fill,
        } => {
            *scale = new_scale.max(0.05);
            *pan = new_pan;
            *fill = new_fill;
            *dirty = true;
        }
        Command::SetPlaying(playing) => {
            if let Some(session) = session {
                session.set_playing(playing);
                shared.playing.store(playing, Ordering::Relaxed);
                if playing {
                    *dirty = true;
                }
            }
        }
        Command::Seek(position_us) => {
            if let Some(session) = session {
                session.seek(position_us, events);
                shared.position_us.store(position_us.max(0), Ordering::Relaxed);
                *dirty = true;
            }
        }
        Command::SetLooping(looping) => {
            if let Some(session) = session {
                session.looping = looping;
            }
        }
        Command::Quit { ack } => {
            let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
            let _ = ack.send(());
            *quit = true;
        }
    }
}

/// Run one command, isolating panics so a single bad frame cannot kill the
/// render thread silently.
#[allow(clippy::too_many_arguments)]
fn handle_command_guarded(
    command: Command,
    ctx: &GpuContext,
    surface: &mut Option<SurfaceState>,
    renderer: &mut Option<MediaRenderer>,
    pending_frame: &mut Option<MediaFrame>,
    session: &mut Option<PlaybackSession>,
    image_animation: &mut Option<ImageAnimationPlayback>,
    image_animation_playing: &mut bool,
    scale: &mut f32,
    pan: &mut [f32; 2],
    fill: &mut bool,
    dirty: &mut bool,
    shared: &Shared,
    events: &Option<EventSink>,
    quit: &mut bool,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_command(
            command,
            ctx,
            surface,
            renderer,
            pending_frame,
            session,
            image_animation,
            image_animation_playing,
            scale,
            pan,
            fill,
            dirty,
            shared,
            events,
            quit,
        );
    }));
    if let Err(payload) = result {
        report_panic(events, "render command", &*payload);
    }
}

/// Surface the payload of a caught panic to logcat/Kotlin instead of dying.
fn report_panic(events: &Option<EventSink>, what: &str, payload: &(dyn std::any::Any + Send)) {
    let detail = payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string());
    let message = format!("{what} panicked: {detail}");
    log::error!("{message}");
    if let Some(events) = events {
        events.error(9, &message);
    }
}

/// Log the first acquire failure only: a stuck surface would otherwise flood
/// the log every frame.
fn log_surface_issue(state: &mut SurfaceState, label: &str) {
    if !state.logged_failure {
        state.logged_failure = true;
        log::warn!("surface acquire reported '{label}'");
    }
}

fn render_frame(
    ctx: &GpuContext,
    state: &mut SurfaceState,
    renderer: &mut MediaRenderer,
    transform: [f32; 4],
    events: &Option<EventSink>,
) {
    match state.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(texture) => {
            state.logged_failure = false;
            if !state.logged_success {
                state.logged_success = true;
                log::info!("first frame presented");
            }
            let view = texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            renderer.render(
                &ctx.device,
                &ctx.queue,
                &view,
                transform,
                YuvInfo::default(),
            );
            ctx.queue.present(texture);
        }
        wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
            state.logged_failure = false;
            let view = texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            renderer.render(
                &ctx.device,
                &ctx.queue,
                &view,
                transform,
                YuvInfo::default(),
            );
            ctx.queue.present(texture);
            state.surface.configure(&ctx.device, &state.config);
        }
        wgpu::CurrentSurfaceTexture::Outdated => {
            log_surface_issue(state, "outdated");
            state.surface.configure(&ctx.device, &state.config);
        }
        wgpu::CurrentSurfaceTexture::Lost => {
            log_surface_issue(state, "lost");
            state.surface.configure(&ctx.device, &state.config);
        }
        wgpu::CurrentSurfaceTexture::Validation => {
            log_surface_issue(state, "validation");
            if let Some(events) = events {
                events.error(4, "surface validation error");
            }
        }
        wgpu::CurrentSurfaceTexture::Timeout => log_surface_issue(state, "timeout"),
        wgpu::CurrentSurfaceTexture::Occluded => log_surface_issue(state, "occluded"),
    }
}

/// Spawn the render thread.
pub fn spawn(
    ctx: Arc<GpuContext>,
    rx: Receiver<Command>,
    shared: Arc<Shared>,
    events: Option<EventSink>,
) -> Option<JoinHandle<()>> {
    let panic_events = events.as_ref().map(EventSink::clone_ref);
    std::thread::Builder::new()
        .name("hotview-render".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(ctx, rx, shared, events);
            }));
            if let Err(payload) = result {
                report_panic(&panic_events, "render thread", &*payload);
            }
        })
        .ok()
}

/// Convenience for the JNI layer: create a one-shot channel pair.
pub fn ack_pair() -> (Sender<()>, Receiver<()>) {
    channel()
}
