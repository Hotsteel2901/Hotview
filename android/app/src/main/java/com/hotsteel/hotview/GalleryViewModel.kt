package com.hotsteel.hotview

import android.app.Application
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.content.ContextCompat
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.hotsteel.hotview.media.Album
import com.hotsteel.hotview.media.MediaItem
import com.hotsteel.hotview.media.MediaStoreRepository
import com.hotsteel.hotview.media.buildAlbums
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class MediaAccess { Unknown, Full, Partial, Denied }

sealed interface Screen {
    data object Albums : Screen
    data class AlbumDetail(val albumId: Long, val title: String) : Screen
    data object Viewer : Screen
    data object About : Screen
}

data class GalleryState(
    val access: MediaAccess = MediaAccess.Unknown,
    val loading: Boolean = false,
    val items: List<MediaItem> = emptyList(),
    val albums: List<Album> = emptyList(),
    val pickedItems: List<MediaItem> = emptyList(),
    val viewerItems: List<MediaItem> = emptyList(),
    val viewerIndex: Int = 0,
    val screen: Screen = Screen.Albums,
    val viewerOrigin: Screen = Screen.Albums,
) {
}

class GalleryViewModel(application: Application) : AndroidViewModel(application) {

    private val _state = MutableStateFlow(GalleryState())
    val state: StateFlow<GalleryState> = _state.asStateFlow()

    fun updateAccess(access: MediaAccess) {
        if (_state.value.access == access && _state.value.items.isNotEmpty()) return
        _state.update { it.copy(access = access) }
        refresh()
    }

    fun refresh() {
        val access = _state.value.access
        if (access == MediaAccess.Unknown || access == MediaAccess.Denied) return
        viewModelScope.launch {
            _state.update { it.copy(loading = true) }
            val items = runCatching { MediaStoreRepository.query(getApplication()) }
                .getOrDefault(emptyList())
            _state.update {
                it.copy(loading = false, items = items, albums = buildAlbums(items))
            }
        }
    }

    fun openAll() = openViewer(_state.value.items, 0)

    fun openPicked() = openViewer(_state.value.pickedItems, 0)

    fun openAlbum(albumId: Long) {
        val album = _state.value.albums.firstOrNull { it.id == albumId } ?: return
        _state.update { it.copy(screen = Screen.AlbumDetail(album.id, album.name)) }
    }

    fun openViewer(items: List<MediaItem>, index: Int) {
        if (items.isEmpty()) return
        _state.update {
            it.copy(
                screen = Screen.Viewer,
                viewerOrigin = it.screen,
                viewerItems = items,
                viewerIndex = index.coerceIn(0, items.lastIndex),
            )
        }
    }

    fun closeViewer() {
        _state.update {
            it.copy(screen = it.viewerOrigin, viewerItems = emptyList())
        }
    }

    fun showAlbums() {
        _state.update { it.copy(screen = Screen.Albums) }
    }

    fun showAbout() {
        _state.update { it.copy(screen = Screen.About) }
    }

    fun addPicked(uris: List<Uri>) {
        if (uris.isEmpty()) return
        viewModelScope.launch {
            val application = getApplication<Application>()
            val added = uris.mapNotNull { uri ->
                MediaStoreRepository.itemFromUri(application, uri)
            }
            _state.update { it.copy(pickedItems = it.pickedItems + added) }
        }
    }
}

/** The permissions that match the running Android version. */
fun requiredMediaPermissions(): Array<String> = when {
    Build.VERSION.SDK_INT >= 34 -> arrayOf(
        android.Manifest.permission.READ_MEDIA_IMAGES,
        android.Manifest.permission.READ_MEDIA_VIDEO,
        android.Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
    )
    Build.VERSION.SDK_INT >= 33 -> arrayOf(
        android.Manifest.permission.READ_MEDIA_IMAGES,
        android.Manifest.permission.READ_MEDIA_VIDEO,
    )
    else -> arrayOf(android.Manifest.permission.READ_EXTERNAL_STORAGE)
}

fun currentAccess(context: Context): MediaAccess = when {
    Build.VERSION.SDK_INT >= 33 -> {
        val images = ContextCompat.checkSelfPermission(
            context,
            android.Manifest.permission.READ_MEDIA_IMAGES,
        ) == PackageManager.PERMISSION_GRANTED
        val videos = ContextCompat.checkSelfPermission(
            context,
            android.Manifest.permission.READ_MEDIA_VIDEO,
        ) == PackageManager.PERMISSION_GRANTED
        when {
            images && videos -> MediaAccess.Full
            Build.VERSION.SDK_INT >= 34 && ContextCompat.checkSelfPermission(
                context,
                android.Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
            ) == PackageManager.PERMISSION_GRANTED -> MediaAccess.Partial
            else -> MediaAccess.Denied
        }
    }
    ContextCompat.checkSelfPermission(
        context,
        android.Manifest.permission.READ_EXTERNAL_STORAGE,
    ) == PackageManager.PERMISSION_GRANTED -> MediaAccess.Full
    else -> MediaAccess.Denied
}
