package com.hotsteel.hotview.native

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.net.Uri
import android.os.Build
import kotlin.math.max

/**
 * Platform (Android) image decoding helpers.
 *
 * Used as the fallback whenever the Rust decoder does not know a format
 * (HEIC/AVIF) or when the native renderer is unavailable. `ImageDecoder`
 * (API 28+) applies EXIF orientation and can downsample efficiently;
 * `BitmapFactory` covers Android 8.0–8.1.
 */
internal object PlatformDecode {

    /** Longest edge of a platform-decoded bitmap. */
    private const val MAX_DIM = 3072

    fun decode(context: Context, uri: Uri): Bitmap? = try {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            decodeWithImageDecoder(context, uri)
        } else {
            decodeWithBitmapFactory(context, uri)
        }
    } catch (_: Throwable) {
        null
    }

    private fun decodeWithImageDecoder(context: Context, uri: Uri): Bitmap {
        val source = ImageDecoder.createSource(context.contentResolver, uri)
        return ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
            var sample = 1
            while (max(info.size.width, info.size.height) / (sample * 2) >= MAX_DIM) {
                sample *= 2
            }
            decoder.setTargetSampleSize(sample)
            decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        }
    }

    private fun decodeWithBitmapFactory(context: Context, uri: Uri): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        context.contentResolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, bounds)
        }
        var sample = 1
        while (max(bounds.outWidth, bounds.outHeight) / (sample * 2) >= MAX_DIM) {
            sample *= 2
        }
        val options = BitmapFactory.Options().apply { inSampleSize = sample }
        return context.contentResolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, options)
        }
    }
}
