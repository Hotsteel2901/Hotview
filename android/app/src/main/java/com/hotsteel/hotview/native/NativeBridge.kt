package com.hotsteel.hotview.native

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
 */
object NativeBridge {
    const val STATUS_OK = 0
    const val STATUS_FALLBACK = 1
    const val STATUS_ERROR = -1

    init {
        System.loadLibrary("hotview_android")
        initLogger()
    }

    external fun initLogger()

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
    external fun hasAudio(handle: Long): Boolean
    external fun isPrepared(handle: Long): Boolean
    external fun seekTo(handle: Long, positionMs: Long)
    external fun positionMs(handle: Long): Long
    external fun durationMs(handle: Long): Long
    external fun setLooping(handle: Long, looping: Boolean)
}
