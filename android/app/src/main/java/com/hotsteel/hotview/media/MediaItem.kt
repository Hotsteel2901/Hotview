package com.hotsteel.hotview.media

import android.net.Uri

/** One image or video from the system media store (or the photo picker). */
data class MediaItem(
    val id: Long,
    val uri: Uri,
    val isVideo: Boolean,
    val bucketId: Long,
    val bucketName: String,
    val displayName: String,
    val mimeType: String?,
    val sizeBytes: Long,
    val dateMillis: Long,
    val width: Int,
    val height: Int,
    val durationMs: Long,
)

/** A device album (a media bucket). */
data class Album(
    val id: Long,
    val name: String,
    val cover: MediaItem?,
    val count: Int,
    val latestMillis: Long,
    val items: List<MediaItem>,
)

/** Group media into albums, newest first. */
fun buildAlbums(items: List<MediaItem>): List<Album> =
    items
        .groupBy { it.bucketId }
        .map { (bucketId, bucketItems) ->
            val sorted = bucketItems.sortedByDescending { it.dateMillis }
            Album(
                id = bucketId,
                name = sorted.firstOrNull()?.bucketName.orEmpty(),
                cover = sorted.firstOrNull(),
                count = sorted.size,
                latestMillis = sorted.firstOrNull()?.dateMillis ?: 0L,
                items = sorted,
            )
        }
        .sortedByDescending { it.latestMillis }
