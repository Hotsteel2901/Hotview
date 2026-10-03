package com.hotsteel.hotview.media

import android.content.ContentUris
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import com.hotsteel.hotview.R

/**
 * MediaStore queries following the scoped-storage rules:
 * `MediaStore.Files` plus the `IS_PENDING = 0` filter on Android 10+.
 */
object MediaStoreRepository {

    suspend fun query(context: Context): List<MediaItem> =
        withContext(Dispatchers.IO) { queryBlocking(context) }

    suspend fun itemFromUri(context: Context, uri: Uri): MediaItem? =
        withContext(Dispatchers.IO) {
            val mime = context.contentResolver.getType(uri)
            val isVideo = mime?.startsWith("video/") == true
            MediaItem(
                id = uri.toString().hashCode().toLong(),
                uri = uri,
                isVideo = isVideo,
                bucketId = PICKED_BUCKET_ID,
                bucketName = context.getString(R.string.bucket_picked),
                displayName = uri.lastPathSegment ?: context.getString(R.string.media_unnamed),
                mimeType = mime,
                sizeBytes = 0L,
                dateMillis = System.currentTimeMillis(),
                width = 0,
                height = 0,
                durationMs = 0L,
            )
        }

    const val PICKED_BUCKET_ID = -1L

    private fun queryBlocking(context: Context): List<MediaItem> {
        val resolver = context.contentResolver
        val collection = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL)
        } else {
            MediaStore.Files.getContentUri("external")
        }

        val columns = mutableListOf(
            MediaStore.MediaColumns._ID,
            MediaStore.MediaColumns.DISPLAY_NAME,
            MediaStore.MediaColumns.MIME_TYPE,
            MediaStore.MediaColumns.SIZE,
            MediaStore.MediaColumns.DATE_ADDED,
            MediaStore.Files.FileColumns.MEDIA_TYPE,
            MediaStore.MediaColumns.WIDTH,
            MediaStore.MediaColumns.HEIGHT,
        )
        val hasBuckets = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
        if (hasBuckets) {
            columns += MediaStore.MediaColumns.DATE_TAKEN
            columns += MediaStore.MediaColumns.DURATION
            columns += MediaStore.MediaColumns.BUCKET_ID
            columns += MediaStore.MediaColumns.BUCKET_DISPLAY_NAME
        } else {
            @Suppress("DEPRECATION")
            columns += MediaStore.MediaColumns.DATA
        }

        val selection = buildString {
            append('(')
            append(MediaStore.Files.FileColumns.MEDIA_TYPE).append("=? OR ")
            append(MediaStore.Files.FileColumns.MEDIA_TYPE).append("=?)")
            if (hasBuckets) {
                append(" AND ").append(MediaStore.MediaColumns.IS_PENDING).append("=0")
            }
        }
        val selectionArgs = arrayOf(
            MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE.toString(),
            MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO.toString(),
        )
        val order = "${MediaStore.MediaColumns.DATE_ADDED} DESC"

        val volume = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            MediaStore.VOLUME_EXTERNAL
        } else {
            "external"
        }
        val fileUri = MediaStore.Files.getContentUri(volume)

        val result = ArrayList<MediaItem>()
        resolver.query(collection, columns.toTypedArray(), selection, selectionArgs, order)?.use { cursor ->
            val idCol = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns._ID)
            val nameCol = cursor.getColumnIndex(MediaStore.MediaColumns.DISPLAY_NAME)
            val mimeCol = cursor.getColumnIndex(MediaStore.MediaColumns.MIME_TYPE)
            val sizeCol = cursor.getColumnIndex(MediaStore.MediaColumns.SIZE)
            val addedCol = cursor.getColumnIndex(MediaStore.MediaColumns.DATE_ADDED)
            val typeCol = cursor.getColumnIndex(MediaStore.Files.FileColumns.MEDIA_TYPE)
            val widthCol = cursor.getColumnIndex(MediaStore.MediaColumns.WIDTH)
            val heightCol = cursor.getColumnIndex(MediaStore.MediaColumns.HEIGHT)
            val takenCol = cursor.getColumnIndex(MediaStore.MediaColumns.DATE_TAKEN)
            val durationCol = cursor.getColumnIndex(MediaStore.MediaColumns.DURATION)
            val bucketIdCol = cursor.getColumnIndex(MediaStore.MediaColumns.BUCKET_ID)
            val bucketNameCol = cursor.getColumnIndex(MediaStore.MediaColumns.BUCKET_DISPLAY_NAME)
            @Suppress("DEPRECATION")
            val dataCol = cursor.getColumnIndex(MediaStore.MediaColumns.DATA)

            while (cursor.moveToNext()) {
                val id = cursor.getLong(idCol)
                val isVideo = cursor.getIntOrZero(typeCol) == MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO
                val uri = ContentUris.withAppendedId(fileUri, id)

                val bucketId: Long
                val bucketName: String
                if (hasBuckets && bucketIdCol >= 0) {
                    bucketId = cursor.getLongOrZero(bucketIdCol)
                    bucketName = cursor.stringOrNull(bucketNameCol) ?: context.getString(R.string.bucket_internal)
                } else {
                    val path = cursor.stringOrNull(dataCol)
                    val parent = path?.substringBeforeLast('/', "")
                        ?.substringAfterLast('/', "")
                        .orEmpty()
                    bucketName = parent.ifEmpty { context.getString(R.string.bucket_internal) }
                    bucketId = bucketName.hashCode().toLong()
                }

                val dateTaken = if (takenCol >= 0) cursor.longOrNull(takenCol) else null
                val dateMillis = dateTaken ?: (cursor.getLongOrZero(addedCol) * 1000L)

                result += MediaItem(
                    id = id,
                    uri = uri,
                    isVideo = isVideo,
                    bucketId = bucketId,
                    bucketName = bucketName,
                    displayName = cursor.stringOrNull(nameCol) ?: context.getString(R.string.media_fallback_name, id),
                    mimeType = cursor.stringOrNull(mimeCol),
                    sizeBytes = if (sizeCol >= 0) cursor.getLongOrZero(sizeCol) else 0L,
                    dateMillis = dateMillis,
                    width = if (widthCol >= 0) cursor.getIntOrZero(widthCol) else 0,
                    height = if (heightCol >= 0) cursor.getIntOrZero(heightCol) else 0,
                    durationMs = if (durationCol >= 0) cursor.getLongOrZero(durationCol) else 0L,
                )
            }
        }
        return result
    }
}

private fun Cursor.stringOrNull(index: Int): String? =
    if (index >= 0 && !isNull(index)) getString(index) else null

private fun Cursor.longOrNull(index: Int): Long? =
    if (index >= 0 && !isNull(index)) getLong(index) else null

private fun Cursor.getLongOrZero(index: Int): Long =
    if (index >= 0 && !isNull(index)) getLong(index) else 0L

private fun Cursor.getIntOrZero(index: Int): Int =
    if (index >= 0 && !isNull(index)) getInt(index) else 0
