//! Hotview for Android: JNI bridge, MediaCodec decoding and the wgpu render
//! thread. Exposed to Kotlin as `com.hotsteel.hotview.native.NativeBridge`.

mod audio;
mod bridge;
mod events;
mod renderer;

use std::ffi::{c_char, CStr};
use std::sync::OnceLock;

use log::{Level, Metadata, Record};

struct LogcatLogger;

impl log::Log for LogcatLogger {
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
        let tag = CStr::from_bytes_with_nul(b"lens\0").unwrap();
        let message = format!("{}\0", record.args());
        unsafe {
            ndk_sys::__android_log_write(priority.0 as i32, tag.as_ptr(), message.as_ptr() as *const c_char);
        }
    }

    fn flush(&self) {}
}

static LOGGER: OnceLock<LogcatLogger> = OnceLock::new();

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
    _env: jni::JNIEnv,
    _this: jni::objects::JObject,
) {
    let logger = LOGGER.get_or_init(|| LogcatLogger);
    let _ = log::set_logger(logger);
    log::set_max_level(log::LevelFilter::Debug);

    // Make native panics visible in logcat instead of a silent abort.
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
