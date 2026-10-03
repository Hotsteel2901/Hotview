//! Sending playback events back to Kotlin through a global reference.

use std::sync::Arc;

use jni::objects::{GlobalRef, JValue};
use jni::JavaVM;

/// A Kotlin object implementing `com.hotsteel.hotview.native.NativeMediaEvents`.
pub struct EventSink {
    vm: Arc<JavaVM>,
    callback: GlobalRef,
}

impl EventSink {
    pub fn new(vm: JavaVM, callback: GlobalRef) -> Self {
        Self {
            vm: Arc::new(vm),
            callback,
        }
    }

    /// Cheap second handle to the same Kotlin callback. Used to report a
    /// render-thread panic even after the sink has been moved into the thread.
    pub fn clone_ref(&self) -> Self {
        Self {
            vm: Arc::clone(&self.vm),
            callback: self.callback.clone(),
        }
    }

    pub fn prepared(&self, duration_ms: i64) {
        self.call_void("onPrepared", "(J)V", &[JValue::Long(duration_ms)]);
    }

    pub fn first_frame(&self) {
        self.call_void("onFirstFrame", "()V", &[]);
    }

    pub fn ended(&self) {
        self.call_void("onEnded", "()V", &[]);
    }

    pub fn error(&self, code: i32, message: &str) {
        let Ok(mut env) = self.vm.attach_current_thread_permanently() else {
            return;
        };
        let Ok(text) = env.new_string(message) else {
            return;
        };
        let value: &jni::objects::JObject = text.as_ref();
        let _ = env.call_method(
            self.callback.as_obj(),
            "onError",
            "(ILjava/lang/String;)V",
            &[JValue::Int(code), JValue::Object(value)],
        );
        if let Ok(true) = env.exception_check() {
            let _ = env.exception_clear();
        }
    }

    fn call_void(&self, name: &str, signature: &str, args: &[JValue]) {
        let Ok(mut env) = self.vm.attach_current_thread_permanently() else {
            return;
        };
        let _ = env.call_method(self.callback.as_obj(), name, signature, args);
        if let Ok(true) = env.exception_check() {
            let _ = env.exception_clear();
        }
    }
}
