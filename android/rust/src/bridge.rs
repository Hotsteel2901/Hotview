//! The JNI functions Kotlin calls. Every entry point catches panics so a bug
//! in Rust can never take down the app.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use jni::objects::{GlobalRef, JByteBuffer, JObject};
use jni::sys::{jboolean, jfloat, jint, jlong, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use hotview_core::video::mediacodec::{MediaCodecAudioDecoder, MediaCodecDecoder};
use hotview_core::video::AudioDecoder;
use hotview_core::{decode_bytes, scale_to_fit, MediaFrame};
use hotview_render::GpuContext;
use ndk_sys::{ANativeWindow_fromSurface, ANativeWindow_release, ANativeWindow};

use crate::events::EventSink;
use crate::renderer::{ack_pair, spawn, Command, SendWindow, Shared};

/// Refuse to decode images larger than this; Kotlin falls back to the platform
/// decoder with an appropriate sample size for those.
const MAX_IMAGE_PIXELS: u64 = 80_000_000;

/// Largest texture edge we upload.
const MAX_TEXTURE_DIM: u32 = 4096;

struct Handle {
    tx: Sender<Command>,
    join: Mutex<Option<JoinHandle<()>>>,
    shared: Arc<Shared>,
}

static REGISTRY: OnceLock<Mutex<HashMap<i64, Arc<Handle>>>> = OnceLock::new();
static NEXT_ID: AtomicI64 = AtomicI64::new(1);
static GPU: OnceLock<Result<Arc<GpuContext>, String>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<i64, Arc<Handle>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn gpu() -> Result<Arc<GpuContext>, String> {
    match GPU.get_or_init(GpuContext::new) {
        Ok(context) => Ok(context.clone()),
        Err(err) => Err(err.clone()),
    }
}

fn lookup(handle: jlong) -> Option<Arc<Handle>> {
    registry().lock().ok().and_then(|map| map.get(&handle).cloned())
}

/// Run `f`, turning panics into the type's default value.
fn caught<T: Default>(f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_default()
}

fn send(handle: jlong, command: Command) -> bool {
    match lookup(handle) {
        Some(handle) => handle.tx.send(command).is_ok(),
        None => false,
    }
}

fn acquire_window(env: &JNIEnv, surface: &JObject) -> *mut ANativeWindow {
    unsafe { ANativeWindow_fromSurface(env.get_raw() as *mut _, surface.as_raw()) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_createRenderer(
    env: JNIEnv,
    _this: JObject,
    surface: JObject,
    width: jint,
    height: jint,
    events: JObject,
) -> jlong {
    caught(|| {
        let context = match gpu() {
            Ok(context) => context,
            Err(err) => {
                log::error!("GPU init failed: {err}");
                return 0;
            }
        };

        let window = acquire_window(&env, &surface);
        if window.is_null() {
            log::error!("ANativeWindow_fromSurface returned null");
            return 0;
        }

        let event_sink = (|| -> Option<EventSink> {
            let vm = env.get_java_vm().ok()?;
            let callback: GlobalRef = env.new_global_ref(&events).ok()?;
            Some(EventSink::new(vm, callback))
        })();

        let shared = Shared::new();
        let (tx, rx) = std::sync::mpsc::channel();
        let Some(join) = spawn(context, rx, shared.clone(), event_sink) else {
            log::error!("could not spawn render thread");
            unsafe { ANativeWindow_release(window) };
            return 0;
        };

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        registry().lock().unwrap().insert(
            id,
            Arc::new(Handle {
                tx,
                join: Mutex::new(Some(join)),
                shared,
            }),
        );

        let (ack_tx, ack_rx) = ack_pair();
        let _ = send(
            id,
            Command::AttachSurface {
                window: SendWindow(window as *mut std::ffi::c_void),
                width: width.max(1) as u32,
                height: height.max(1) as u32,
                ack: ack_tx,
            },
        );
        let _ = ack_rx.recv_timeout(Duration::from_secs(3));
        id
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_destroyRenderer(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
) {
    caught(|| {
        let entry = registry().lock().unwrap().remove(&handle);
        let Some(entry) = entry else {
            return;
        };
        let (ack_tx, ack_rx) = ack_pair();
        let _ = entry.tx.send(Command::Quit { ack: ack_tx });
        let _ = ack_rx.recv_timeout(Duration::from_secs(3));
        let join = entry.join.lock().unwrap().take();
        if let Some(join) = join {
            let _ = join.join();
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_attachSurface(
    env: JNIEnv,
    _this: JObject,
    handle: jlong,
    surface: JObject,
    width: jint,
    height: jint,
) {
    caught(|| {
        let window = acquire_window(&env, &surface);
        if window.is_null() {
            log::error!("attachSurface: null window");
            return;
        }
        let (ack_tx, ack_rx) = ack_pair();
        if !send(
            handle,
            Command::AttachSurface {
                window: SendWindow(window as *mut std::ffi::c_void),
                width: width.max(1) as u32,
                height: height.max(1) as u32,
                ack: ack_tx,
            },
        ) {
            unsafe { ANativeWindow_release(window) };
            return;
        }
        let _ = ack_rx.recv_timeout(Duration::from_secs(3));
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_releaseSurface(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
) {
    caught(|| {
        let (ack_tx, ack_rx) = ack_pair();
        if send(handle, Command::DetachSurface { ack: ack_tx }) {
            let _ = ack_rx.recv_timeout(Duration::from_secs(3));
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_surfaceChanged(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
    width: jint,
    height: jint,
) {
    caught(|| {
        let _ = send(
            handle,
            Command::Resize {
                width: width.max(1) as u32,
                height: height.max(1) as u32,
            },
        );
    })
}

/// Decode an image file descriptor in Rust.
/// Returns 0 on success, 1 when Kotlin should fall back to the platform
/// decoder, -1 on hard errors.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_setImageFile(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
    fd: jint,
    offset: jlong,
    length: jlong,
) -> jint {
    caught(|| {
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut file = File::from(owned);
        let mut bytes = Vec::new();
        if offset > 0 && file.seek(SeekFrom::Start(offset as u64)).is_err() {
            return -1;
        }
        let result = if length > 0 {
            file.take(length as u64).read_to_end(&mut bytes)
        } else {
            file.read_to_end(&mut bytes)
        };
        if result.is_err() {
            return -1;
        }

        match decode_bytes(&bytes) {
            Ok(frame) => {
                if frame.width as u64 * frame.height as u64 > MAX_IMAGE_PIXELS {
                    log::warn!("image too large for the Rust decoder, deferring to platform");
                    return 1;
                }
                let frame = scale_to_fit(frame, MAX_TEXTURE_DIM);
                if send(handle, Command::SetFrame(MediaFrame::Rgba(frame))) {
                    0
                } else {
                    -1
                }
            }
            Err(err) => {
                log::warn!("Rust image decode failed ({err}); falling back to platform decoder");
                1
            }
        }
    })
}

/// Hand a video file descriptor to the hardware decoder (video + audio).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_setVideoFile(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
    fd: jint,
    offset: jlong,
    length: jlong,
) -> jint {
    caught(|| {
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        // The audio decoder needs its own descriptor; both close what they own.
        let audio_fd = match owned.try_clone() {
            Ok(dup) => Some(dup),
            Err(err) => {
                log::warn!("could not duplicate descriptor for audio: {err}");
                None
            }
        };
        match MediaCodecDecoder::open(owned, offset, length) {
            Ok(video) => {
                let audio = audio_fd.and_then(|fd| {
                    match MediaCodecAudioDecoder::open(fd, offset, length) {
                        Ok(audio) => audio,
                        Err(err) => {
                            log::warn!("audio decoder unavailable: {err}");
                            None
                        }
                    }
                });
                let command = Command::SetVideo {
                    video: Box::new(video),
                    audio: audio.map(|decoder| Box::new(decoder) as Box<dyn AudioDecoder>),
                };
                if send(handle, command) {
                    0
                } else {
                    -1
                }
            }
            Err(err) => {
                log::error!("MediaCodec setup failed: {err}");
                -1
            }
        }
    })
}

/// Upload an RGBA bitmap decoded by the platform (HEIC & friends).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_setBitmap(
    env: JNIEnv,
    _this: JObject,
    handle: jlong,
    buffer: JByteBuffer,
    width: jint,
    height: jint,
) -> jint {
    caught(|| {
        let width = width.max(0) as u32;
        let height = height.max(0) as u32;
        let expected = width as usize * height as usize * 4;
        if expected == 0 {
            return -1;
        }
        let Ok(address) = env.get_direct_buffer_address(&buffer) else {
            return -1;
        };
        let Ok(capacity) = env.get_direct_buffer_capacity(&buffer) else {
            return -1;
        };
        if capacity < expected || address.is_null() {
            return -1;
        }
        let data = unsafe { std::slice::from_raw_parts(address, expected) }.to_vec();
        let frame = MediaFrame::Rgba(hotview_core::RgbaFrame {
            width,
            height,
            data,
            pts_us: None,
        });
        if send(handle, Command::SetFrame(frame)) {
            0
        } else {
            -1
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_setViewport(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
    scale: jfloat,
    pan_x: jfloat,
    pan_y: jfloat,
) {
    caught(|| {
        let _ = send(
            handle,
            Command::Viewport {
                scale,
                pan: [pan_x, pan_y],
            },
        );
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_setPlaying(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
    playing: jboolean,
) {
    caught(|| {
        let _ = send(handle, Command::SetPlaying(playing != JNI_FALSE));
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_seekTo(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
    position_ms: jlong,
) {
    caught(|| {
        let _ = send(handle, Command::Seek(position_ms.saturating_mul(1000)));
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_setLooping(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
    looping: jboolean,
) {
    caught(|| {
        let _ = send(handle, Command::SetLooping(looping != JNI_FALSE));
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_positionMs(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
) -> jlong {
    caught(|| {
        lookup(handle)
            .map(|entry| entry.shared.position_us.load(Ordering::Relaxed) / 1000)
            .unwrap_or(0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_durationMs(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
) -> jlong {
    caught(|| {
        lookup(handle)
            .map(|entry| entry.shared.duration_us.load(Ordering::Relaxed) / 1000)
            .unwrap_or(0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_isPlaying(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
) -> jboolean {
    caught(|| {
        match lookup(handle) {
            Some(entry) if entry.shared.playing.load(Ordering::Relaxed) => JNI_TRUE,
            _ => JNI_FALSE,
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_isPrepared(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
) -> jboolean {
    caught(|| {
        match lookup(handle) {
            Some(entry) if entry.shared.prepared.load(Ordering::Relaxed) => JNI_TRUE,
            _ => JNI_FALSE,
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_hasAudio(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
) -> jboolean {
    caught(|| {
        match lookup(handle) {
            Some(entry) if entry.shared.has_audio.load(Ordering::Relaxed) => JNI_TRUE,
            _ => JNI_FALSE,
        }
    })
}
