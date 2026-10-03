package com.hotsteel.hotview.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Build
import android.util.LruCache
import android.util.Size
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import com.hotsteel.hotview.media.MediaItem
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.max

/**
 * Platform thumbnail loading (the official `ContentResolver.loadThumbnail`
 * fast path on Android 10+, with a decoder fallback on older releases).
 */
object ThumbnailLoader {
    private val cache = object : LruCache<String, ImageBitmap>(48 * 1024 * 1024) {
        override fun sizeOf(key: String, value: ImageBitmap): Int =
            value.width * value.height * 4
    }

    fun peek(uri: Uri): ImageBitmap? = cache.get(uri.toString())

    suspend fun load(context: Context, item: MediaItem, sizePx: Int = 512): ImageBitmap? =
        withContext(Dispatchers.IO) {
            cache.get(item.uri.toString())?.let { return@withContext it }
            val bitmap = runCatching { loadBitmap(context, item, sizePx) }.getOrNull()
                ?: return@withContext null
            val image = bitmap.asImageBitmap()
            cache.put(item.uri.toString(), image)
            image
        }

    private fun loadBitmap(context: Context, item: MediaItem, sizePx: Int): Bitmap? {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            return context.contentResolver.loadThumbnail(item.uri, Size(sizePx, sizePx), null)
        }
        return if (item.isVideo) {
            videoFrame(context, item.uri)
        } else {
            sampledBitmap(context, item.uri, sizePx)
        }
    }

    @Suppress("DEPRECATION")
    private fun videoFrame(context: Context, uri: Uri): Bitmap? {
        val retriever = MediaMetadataRetriever()
        return try {
            context.contentResolver.openFileDescriptor(uri, "r")?.use { descriptor ->
                retriever.setDataSource(descriptor.fileDescriptor)
                retriever.getFrameAtTime(0, MediaMetadataRetriever.OPTION_CLOSEST_SYNC)
            }
        } finally {
            runCatching { retriever.release() }
        }
    }

    private fun sampledBitmap(context: Context, uri: Uri, sizePx: Int): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        context.contentResolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, bounds)
        }
        var sample = 1
        while (max(bounds.outWidth, bounds.outHeight) / (sample * 2) >= sizePx) {
            sample *= 2
        }
        val options = BitmapFactory.Options().apply { inSampleSize = sample }
        return context.contentResolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, options)
        }
    }
}
