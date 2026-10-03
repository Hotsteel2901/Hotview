//! Hotview for Android: JNI bridge, MediaCodec decoding and the wgpu render
//! thread. Exposed to Kotlin as `com.hotsteel.hotview.native.NativeBridge`.

mod audio;
mod bridge;
mod events;
mod renderer;

use std::ffi::{c_char, CStr};
use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use log::{Level, Metadata, Record};

/// Writes to logcat and, when Kotlin hands us a path, to a file inside the
/// app's external files directory (`Android/data/<package>/files`). The file
/// makes on-device problems diagnosable without adb.
struct HotviewLogger {
    file: Mutex<Option<File>>,
}

impl log::Log for HotviewLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let priority = match record.level() {
            Level::Error => ndk_sys::android_LogPriority::ANDROID_LOG_ERROR,
            Level::Warn => ndk_sys::android_LogPriority::ANDROID_LOG_WARN,
            Level::Info => ndk_sys::android_LogPriority::ANDROID_LOG_INFO,
            Level::Debug => ndk_sys::android_LogPriority::ANDROID_LOG_DEBUG,
            Level::Trace => ndk_sys::android_LogPriority::ANDROID_LOG_VERBOSE,
        };
        let tag = CStr::from_bytes_with_nul(b"hotview\0").unwrap();
        let message = format!("{}\0", record.args());
        unsafe {
            ndk_sys::__android_log_write(
                priority.0 as i32,
                tag.as_ptr(),
                message.as_ptr() as *const c_char,
            );
        }

        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let seconds = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs_f64())
                    .unwrap_or(0.0);
                // Unbuffered on purpose: the tail of the log survives a crash.
                let _ = writeln!(
                    file,
                    "[{seconds:.3}] [{:?}] {}",
                    record.level(),
                    record.args()
                );
            }
        }
    }

    fn flush(&self) {}
}

static LOGGER: OnceLock<HotviewLogger> = OnceLock::new();

/// Called by ART right after `dlopen`. Logging here proves whether the library
/// actually loaded, even when the Kotlin side later fails to find a symbol.
#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(
    _vm: *mut jni::sys::JavaVM,
    _reserved: *mut std::ffi::c_void,
) -> jni::sys::jint {
    logcat(
        ndk_sys::android_LogPriority::ANDROID_LOG_INFO,
        "Hotview native library loaded (JNI_OnLoad)",
    );
    jni::sys::JNI_VERSION_1_6
}

fn logcat(priority: ndk_sys::android_LogPriority, message: &str) {
    let tag = CStr::from_bytes_with_nul(b"hotview\0").unwrap();
    let text = format!("{message}\0");
    unsafe {
        ndk_sys::__android_log_write(priority.0 as i32, tag.as_ptr(), text.as_ptr() as *const c_char);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hotsteel_hotview_native_NativeBridge_initLogger(
    mut env: jni::JNIEnv,
    _this: jni::objects::JObject,
    path: jni::objects::JString,
) {
    let logger = LOGGER.get_or_init(|| HotviewLogger {
        file: Mutex::new(None),
    });

    if !path.is_null() {
        let opened = env
            .get_string(&path)
            .map(String::from)
            .ok()
            .and_then(|path| File::options().create(true).append(true).open(path).ok());
        match opened {
            Some(file) => {
                if let Ok(mut guard) = logger.file.lock() {
                    *guard = Some(file);
                }
            }
            None => logcat(
                ndk_sys::android_LogPriority::ANDROID_LOG_WARN,
                "hotview: could not open the log file",
            ),
        }
    }

    let _ = log::set_logger(logger);
    log::set_max_level(log::LevelFilter::Debug);

    // Make native panics visible in logcat/file instead of a silent abort.
    static HOOK: OnceLock<()> = OnceLock::new();
    HOOK.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("native panic: {info}");
            previous(info);
        }));
    });

    log::info!("hotview native library loaded");
}
