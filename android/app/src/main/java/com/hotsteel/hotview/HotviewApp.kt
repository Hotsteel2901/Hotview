package com.hotsteel.hotview

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import dev.chrisbanes.haze.hazeSource
import dev.chrisbanes.haze.rememberHazeState
import com.hotsteel.hotview.ui.AboutScreen
import com.hotsteel.hotview.ui.AlbumScreen
import com.hotsteel.hotview.ui.AlbumsTab
import com.hotsteel.hotview.ui.LiquidGlassBottomBar
import com.hotsteel.hotview.ui.PhotosTab
import com.hotsteel.hotview.ui.PickedTab
import com.hotsteel.hotview.ui.ViewerScreen
import com.hotsteel.hotview.ui.theme.HotviewTheme
import com.hotsteel.hotview.ui.theme.rememberThemeController
import com.hotsteel.hotview.ui.theme.next
import androidx.annotation.StringRes

/** Root destinations shown in the floating glass bar. */
enum class RootTab(@StringRes val labelRes: Int, val icon: ImageVector) {
    Albums(R.string.tabs_albums, Icons.Filled.PhotoLibrary),
    Photos(R.string.tabs_photos, Icons.Filled.Image),
    Picked(R.string.tabs_picked, Icons.Filled.Bookmark),
}

@Composable
fun HotviewApp(viewModel: GalleryViewModel = viewModel()) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val themeController = rememberThemeController()
    var themeMode by remember { mutableStateOf(themeController.load()) }
    var tab by rememberSaveable { mutableStateOf(RootTab.Albums) }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) {
        viewModel.updateAccess(currentAccess(context))
    }

    // The official, privacy-preserving photo picker.
    val picker = rememberLauncherForActivityResult(
        ActivityResultContracts.PickMultipleVisualMedia(100),
    ) { uris ->
        viewModel.addPicked(uris)
    }
    val launchPicker: () -> Unit = {
        picker.launch(
            PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo),
        )
    }

    LaunchedEffect(Unit) {
        val access = currentAccess(context)
        viewModel.updateAccess(access)
        if (access == MediaAccess.Denied) {
            permissionLauncher.launch(requiredMediaPermissions())
        }
    }

    HotviewTheme(mode = themeMode) {
        BackHandler(enabled = state.screen !is Screen.Albums) {
            when (state.screen) {
                is Screen.Viewer -> viewModel.closeViewer()
                else -> viewModel.showAlbums()
            }
        }

        val hazeState = rememberHazeState()

        Box(
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background),
        ) {
            when (val screen = state.screen) {
                is Screen.Viewer -> ViewerScreen(
                    items = state.viewerItems,
                    initialIndex = state.viewerIndex,
                    onClose = { viewModel.closeViewer() },
                )

                is Screen.AlbumDetail -> {
                    val album = state.albums.firstOrNull { it.id == screen.albumId }
                    AlbumScreen(
                        title = screen.title,
                        items = album?.items.orEmpty(),
                        onBack = { viewModel.showAlbums() },
                        onOpen = { index ->
                            viewModel.openViewer(album?.items.orEmpty(), index)
                        },
                    )
                }

                is Screen.About -> AboutScreen(onBack = { viewModel.showAlbums() })

                is Screen.Albums -> {
                    Box(
                        Modifier
                            .fillMaxSize()
                            .hazeSource(hazeState),
                    ) {
                        AnimatedContent(
                            targetState = tab,
                            transitionSpec = {
                                (
                                    fadeIn(tween(durationMillis = 240)) +
                                        scaleIn(initialScale = 0.97f)
                                    ) togetherWith fadeOut(tween(durationMillis = 160))
                            },
                            label = "rootTab",
                        ) { target ->
                            when (target) {
                                RootTab.Albums -> AlbumsTab(
                                    state = state,
                                    themeMode = themeMode,
                                    onCycleTheme = {
                                        themeMode = themeMode.next()
                                        themeController.save(themeMode)
                                    },
                                    onOpenAlbum = { album -> viewModel.openAlbum(album.id) },
                                    onOpenAll = { viewModel.openAll() },
                                    onPickPhotos = launchPicker,
                                    onOpenAbout = { viewModel.showAbout() },
                                    onGrantAccess = {
                                        permissionLauncher.launch(requiredMediaPermissions())
                                    },
                                )

                                RootTab.Photos -> PhotosTab(
                                    items = state.items,
                                    onOpen = { index -> viewModel.openViewer(state.items, index) },
                                )

                                RootTab.Picked -> PickedTab(
                                    items = state.pickedItems,
                                    onPick = launchPicker,
                                    onOpen = { index ->
                                        viewModel.openViewer(state.pickedItems, index)
                                    },
                                )
                            }
                        }
                    }

                    LiquidGlassBottomBar(
                        selected = tab,
                        onSelected = { tab = it },
                        hazeState = hazeState,
                        modifier = Modifier.align(Alignment.BottomCenter),
                    )
                }
            }
        }
    }
}
