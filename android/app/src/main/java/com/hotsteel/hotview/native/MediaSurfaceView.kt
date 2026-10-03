package com.hotsteel.hotview.native

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.net.Uri
import android.os.Build
import android.view.SurfaceHolder
import android.view.SurfaceView
import com.hotsteel.hotview.media.MediaItem
import java.nio.ByteBuffer
import kotlin.math.max
import com.hotsteel.hotview.R

/**
 * A `SurfaceView` whose contents are rendered by the Rust wgpu renderer.
 * Images are decoded in Rust; videos go through Rust + MediaCodec. When the
 * Rust decoder does not know a format (HEIC), Kotlin decodes the image and
 * hands the RGBA buffer back to the renderer.
 */
class MediaSurfaceView(context: Context) :
    SurfaceView(context),
    SurfaceHolder.Callback,
    NativeMediaEvents {

    private var handle = 0L
    private var pending: (() -> Unit)? = null

    /** Duration in ms, fired when a video has been prepared. */
    var onPrepared: ((Long) -> Unit)? = null
    var onEnded: (() -> Unit)? = null
    var onErrorEvent: ((String) -> Unit)? = null

    init {
        holder.addCallback(this)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        if (handle == 0L) {
            handle = NativeBridge.createRenderer(
                holder.surface,
                width.coerceAtLeast(1),
                height.coerceAtLeast(1),
                this,
            )
        } else {
            NativeBridge.attachSurface(
                handle,
                holder.surface,
                width.coerceAtLeast(1),
                height.coerceAtLeast(1),
            )
        }
        pending?.invoke()
        pending = null
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, w: Int, h: Int) {
        if (handle != 0L) {
            NativeBridge.surfaceChanged(handle, w.coerceAtLeast(1), h.coerceAtLeast(1))
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        if (handle != 0L) {
            NativeBridge.releaseSurface(handle)
        }
    }

    fun load(item: MediaItem) {
        val action = { loadInternal(item) }
        if (handle == 0L) {
            pending = action
        } else {
            action()
        }
    }

    private fun loadInternal(item: MediaItem) {
        val status = open(item.uri) { fd, offset, length ->
            if (item.isVideo) {
                NativeBridge.setVideoFile(handle, fd, offset, length)
            } else {
                NativeBridge.setImageFile(handle, fd, offset, length)
            }
        }
        if (!item.isVideo && status != NativeBridge.STATUS_OK) {
            decodeWithPlatform(item.uri)
        } else if (item.isVideo && status != NativeBridge.STATUS_OK) {
            onErrorEvent?.invoke(context.getString(R.string.viewer_video_unsupported))
        }
    }

    /**
     * Opens a content URI and hands the raw descriptor to Rust, which takes
     * ownership and closes it.
     */
    private inline fun <T> open(uri: Uri, block: (Int, Long, Long) -> T): T? {
        val descriptor = try {
            context.contentResolver.openAssetFileDescriptor(uri, "r")
        } catch (error: Exception) {
            null
        } ?: return null
        return descriptor.use {
            block(it.parcelFileDescriptor.detachFd(), it.startOffset, it.length)
        }
    }

    private fun decodeWithPlatform(uri: Uri) {
        Thread {
            val bitmap = try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    decodeWithImageDecoder(uri)
                } else {
                    decodeWithBitmapFactory(uri)
                }
            } catch (error: Throwable) {
                null
            }
            if (bitmap != null && handle != 0L) {
                val argb = if (bitmap.config == Bitmap.Config.ARGB_8888) {
                    bitmap
                } else {
                    bitmap.copy(Bitmap.Config.ARGB_8888, false)
                }
                val buffer = ByteBuffer.allocateDirect(argb.byteCount)
                argb.copyPixelsToBuffer(buffer)
                buffer.rewind()
                NativeBridge.setBitmap(handle, buffer, argb.width, argb.height)
            } else if (bitmap == null) {
                post { onErrorEvent?.invoke(context.getString(R.string.viewer_image_unsupported)) }
            }
        }.start()
    }

    private fun decodeWithImageDecoder(uri: Uri): Bitmap {
        val source = ImageDecoder.createSource(context.contentResolver, uri)
        return ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
            var sample = 1
            while (max(info.size.width, info.size.height) / (sample * 2) >= 4096) {
                sample *= 2
            }
            decoder.setTargetSampleSize(sample)
            decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        }
    }

    private fun decodeWithBitmapFactory(uri: Uri): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        context.contentResolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, bounds)
        }
        var sample = 1
        while (max(bounds.outWidth, bounds.outHeight) / (sample * 2) >= 4096) {
            sample *= 2
        }
        val options = BitmapFactory.Options().apply { inSampleSize = sample }
        return context.contentResolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, options)
        }
    }

    // ------------------------------------------------------------- controls

    fun play() {
        if (handle != 0L) NativeBridge.setPlaying(handle, true)
    }

    fun pause() {
        if (handle != 0L) NativeBridge.setPlaying(handle, false)
    }

    fun isPlaying(): Boolean = handle != 0L && NativeBridge.isPlaying(handle)

    fun isPrepared(): Boolean = handle != 0L && NativeBridge.isPrepared(handle)

    fun hasAudio(): Boolean = handle != 0L && NativeBridge.hasAudio(handle)

    fun positionMs(): Long = if (handle != 0L) NativeBridge.positionMs(handle) else 0L

    fun durationMs(): Long = if (handle != 0L) NativeBridge.durationMs(handle) else 0L

    fun seekTo(positionMs: Long) {
        if (handle != 0L) NativeBridge.seekTo(handle, positionMs)
    }

    fun setLooping(looping: Boolean) {
        if (handle != 0L) NativeBridge.setLooping(handle, looping)
    }

    fun setViewport(scale: Float, panX: Float, panY: Float) {
        if (handle != 0L) NativeBridge.setViewport(handle, scale, panX, panY)
    }

    fun dispose() {
        if (handle != 0L) {
            NativeBridge.destroyRenderer(handle)
            handle = 0L
        }
    }

    // ------------------------------------------------------ native callbacks

    override fun onPrepared(durationMs: Long) {
        post { onPrepared?.invoke(durationMs) }
    }

    override fun onFirstFrame() = Unit

    override fun onEnded() {
        post { onEnded?.invoke() }
    }

    override fun onError(code: Int, message: String) {
        post { onErrorEvent?.invoke(message) }
    }
}
