package com.hotsteel.hotview.native

import android.content.Context
import android.view.Surface
import java.nio.ByteBuffer

/** Playback events pushed from the Rust render thread. */
interface NativeMediaEvents {
    fun onPrepared(durationMs: Long)
    fun onFirstFrame()
    fun onEnded()
    fun onError(code: Int, message: String)
}

/**
 * Entry points into `libhotview_android.so`, the Rust renderer/decoder.
 * Matches `android/rust/src/bridge.rs`.
 *
 * The library is loaded lazily (with a second, absolute-path attempt) and any
 * failure is kept as a readable message so the UI can explain what happened
 * instead of dying with `ExceptionInInitializerError`/`NoClassDefFoundError`.
 */
object NativeBridge {
    const val STATUS_OK = 0
    const val STATUS_FALLBACK = 1
    const val STATUS_ERROR = -1

    @Volatile
    private var loadError: Throwable? = null

    @Volatile
    private var loaded = false

    /** Idempotent: loads the native library once, remembering any failure. */
    @Synchronized
    fun ensureLoaded(context: Context): Boolean {
        if (loaded) return true
        if (loadError != null) return false

        try {
            System.loadLibrary("hotview_android")
            loaded = true
        } catch (libraryError: Throwable) {
            // Some devices refuse to map libraries straight out of the APK;
            // retry from the extracted native library directory.
            val fallback = runCatching {
                val dir = context.applicationInfo.nativeLibraryDir
                System.load("$dir/libhotview_android.so")
            }
            if (fallback.isSuccess) {
                loaded = true
            } else {
                loadError = libraryError
                return false
            }
        }

        try {
            val logFile = runCatching {
                val dir = context.getExternalFilesDir(null) ?: context.filesDir
                val file = java.io.File(dir, "hotview.log")
                if (file.length() > 512 * 1024) file.delete()
                file.absolutePath
            }.getOrNull()
            initLogger(logFile)
        } catch (error: Throwable) {
            loaded = false
            loadError = error
            return false
        }
        return true
    }

    /** Non-null when the native library or the logger failed to initialise. */
    fun loadFailure(): Throwable? = loadError

    fun isAvailable(): Boolean = loaded

    /** `path` is the file that mirrors logcat, or null to log to logcat only. */
    external fun initLogger(path: String?)

    external fun createRenderer(
        surface: Surface,
        width: Int,
        height: Int,
        events: NativeMediaEvents,
    ): Long

    external fun destroyRenderer(handle: Long)
    external fun attachSurface(handle: Long, surface: Surface, width: Int, height: Int)
    external fun releaseSurface(handle: Long)
    external fun surfaceChanged(handle: Long, width: Int, height: Int)

    /** Decode an image from a descriptor; returns one of the `STATUS_*` values. */
    external fun setImageFile(handle: Long, fd: Int, offset: Long, length: Long): Int

    /** Decode a video through MediaCodec; returns one of the `STATUS_*` values. */
    external fun setVideoFile(handle: Long, fd: Int, offset: Long, length: Long): Int

    /** Upload platform-decoded RGBA pixels (e.g. HEIC fallback). */
    external fun setBitmap(handle: Long, buffer: ByteBuffer, width: Int, height: Int): Int

    external fun setViewport(handle: Long, scale: Float, panX: Float, panY: Float)
    external fun setPlaying(handle: Long, playing: Boolean)
    external fun isPlaying(handle: Long): Boolean
    external fun isPrepared(handle: Long): Boolean
    external fun hasAudio(handle: Long): Boolean
    external fun seekTo(handle: Long, positionMs: Long)
    external fun positionMs(handle: Long): Long
    external fun durationMs(handle: Long): Long
    external fun setLooping(handle: Long, looping: Boolean)
}
