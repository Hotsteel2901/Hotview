//! AAudio output plus the lock-free ring buffer feeding it.
//!
//! MediaCodec delivers PCM on the render thread; the AAudio callback runs on
//! its own real-time thread, so the two communicate through a single
//! producer / single consumer ring of atomic f32 slots.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use ndk_sys::{
    aaudio_data_callback_result_t, AAudioStream, AAudioStreamBuilder, AAUDIO_CALLBACK_RESULT_CONTINUE,
    AAUDIO_DIRECTION_OUTPUT, AAUDIO_FORMAT_PCM_FLOAT, AAUDIO_OK, AAUDIO_PERFORMANCE_MODE_NONE,
    AAUDIO_SHARING_MODE_SHARED, AAudioStreamBuilder_delete, AAudioStreamBuilder_openStream,
    AAudioStreamBuilder_setChannelCount, AAudioStreamBuilder_setDataCallback,
    AAudioStreamBuilder_setDirection, AAudioStreamBuilder_setFormat,
    AAudioStreamBuilder_setPerformanceMode, AAudioStreamBuilder_setSampleRate,
    AAudioStreamBuilder_setSharingMode, AAudioStream_close, AAudioStream_getChannelCount,
    AAudioStream_getSampleRate, AAudioStream_requestFlush, AAudioStream_requestPause,
    AAudioStream_requestStart, AAudioStream_requestStop, AAudio_createStreamBuilder,
};

/// Single producer / single consumer ring of f32 samples.
pub struct SpscRing {
    slots: Box<[AtomicU32]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl SpscRing {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(64).next_power_of_two();
        let slots = (0..capacity)
            .map(|_| AtomicU32::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    /// Producer side. Returns the number of samples written.
    pub fn push_slice(&self, src: &[f32]) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        let capacity = self.slots.len();
        let mask = capacity - 1;
        let mut free = capacity - tail.wrapping_sub(head);
        let mut written = 0;
        while written < src.len() && free > 0 {
            self.slots[(tail + written) & mask].store(src[written].to_bits(), Ordering::Relaxed);
            written += 1;
            free -= 1;
        }
        self.tail.store(tail.wrapping_add(written), Ordering::Release);
        written
    }

    /// Consumer side (audio callback). Returns the number of samples read.
    pub fn pop_into(&self, dst: &mut [f32]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let capacity = self.slots.len();
        let mask = capacity - 1;
        let filled = tail.wrapping_sub(head);
        let count = filled.min(dst.len());
        for (offset, slot) in dst.iter_mut().enumerate().take(count) {
            *slot = f32::from_bits(self.slots[(head + offset) & mask].load(Ordering::Relaxed));
        }
        self.head.store(head.wrapping_add(count), Ordering::Release);
        count
    }

    /// Drop everything currently buffered (only used while the stream is
    /// flushed/paused).
    pub fn clear(&self) {
        let tail = self.tail.load(Ordering::Acquire);
        self.head.store(tail, Ordering::Release);
    }
}

struct CallbackCtx {
    ring: Arc<SpscRing>,
    played_frames: Arc<AtomicU64>,
    channels: usize,
}

/// An AAudio output stream playing interleaved f32 samples pulled from a ring.
pub struct AaudioOutput {
    stream: *mut AAudioStream,
    _ctx: Box<CallbackCtx>,
    ring: Arc<SpscRing>,
    played: Arc<AtomicU64>,
    sample_rate: u32,
    channels: u16,
}

// SAFETY: the stream is created and used from the render thread only.
unsafe impl Send for AaudioOutput {}

impl AaudioOutput {
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, String> {
        let channels = channels.clamp(1, 2);
        let ring = Arc::new(SpscRing::new(sample_rate as usize * channels as usize));
        let played = Arc::new(AtomicU64::new(0));
        let mut ctx = Box::new(CallbackCtx {
            ring: Arc::clone(&ring),
            played_frames: Arc::clone(&played),
            channels: channels as usize,
        });

        unsafe {
            let mut builder: *mut AAudioStreamBuilder = std::ptr::null_mut();
            if AAudio_createStreamBuilder(&mut builder) != AAUDIO_OK || builder.is_null() {
                return Err("AAudio_createStreamBuilder failed".into());
            }
            AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_OUTPUT as i32);
            AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_FLOAT as i32);
            AAudioStreamBuilder_setChannelCount(builder, channels as i32);
            AAudioStreamBuilder_setSampleRate(builder, sample_rate as i32);
            AAudioStreamBuilder_setSharingMode(builder, AAUDIO_SHARING_MODE_SHARED as i32);
            AAudioStreamBuilder_setPerformanceMode(builder, AAUDIO_PERFORMANCE_MODE_NONE as i32);
            AAudioStreamBuilder_setDataCallback(
                builder,
                Some(data_callback),
                ctx.as_mut() as *mut CallbackCtx as *mut c_void,
            );

            let mut stream: *mut AAudioStream = std::ptr::null_mut();
            let result = AAudioStreamBuilder_openStream(builder, &mut stream);
            AAudioStreamBuilder_delete(builder);
            if result != AAUDIO_OK || stream.is_null() {
                return Err(format!("AAudio openStream failed ({result})"));
            }

            let actual_rate = AAudioStream_getSampleRate(stream).max(1) as u32;
            let actual_channels = AAudioStream_getChannelCount(stream).clamp(1, 2) as u16;
            // Keep the callback in sync with what the device actually opened.
            ctx.channels = actual_channels as usize;
            log::debug!("AAudio stream: {actual_rate} Hz, {actual_channels} ch");

            Ok(Self {
                stream,
                _ctx: ctx,
                ring,
                played,
                sample_rate: actual_rate,
                channels: actual_channels,
            })
        }
    }

    pub fn request_start(&self) {
        unsafe {
            let _ = AAudioStream_requestStart(self.stream);
        }
    }

    pub fn request_pause(&self) {
        unsafe {
            let _ = AAudioStream_requestPause(self.stream);
        }
    }

    pub fn request_flush(&self) {
        unsafe {
            let _ = AAudioStream_requestFlush(self.stream);
        }
    }

    /// Frames consumed by the audio device since the stream was created.
    pub fn played_frames(&self) -> u64 {
        self.played.load(Ordering::Relaxed)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn buffered_samples(&self) -> usize {
        self.ring.len()
    }

    pub fn free_samples(&self) -> usize {
        self.ring.capacity().saturating_sub(self.ring.len())
    }

    pub fn push(&self, samples: &[f32]) -> usize {
        self.ring.push_slice(samples)
    }

    pub fn clear(&self) {
        self.ring.clear();
    }
}

impl Drop for AaudioOutput {
    fn drop(&mut self) {
        unsafe {
            if !self.stream.is_null() {
                let _ = AAudioStream_requestStop(self.stream);
                let _ = AAudioStream_close(self.stream);
            }
        }
    }
}

unsafe extern "C" fn data_callback(
    _stream: *mut AAudioStream,
    user_data: *mut c_void,
    audio_data: *mut c_void,
    num_frames: i32,
) -> aaudio_data_callback_result_t {
    if !user_data.is_null() && !audio_data.is_null() && num_frames > 0 {
        unsafe {
            let ctx = &*(user_data as *const CallbackCtx);
            let frames = num_frames as usize;
            let count = frames * ctx.channels.max(1);
            let dst = std::slice::from_raw_parts_mut(audio_data as *mut f32, count);
            let written = ctx.ring.pop_into(dst);
            if written < count {
                dst[written..].fill(0.0);
            }
            ctx.played_frames
                .fetch_add(frames as u64, Ordering::Relaxed);
        }
    }
    AAUDIO_CALLBACK_RESULT_CONTINUE as i32
}
