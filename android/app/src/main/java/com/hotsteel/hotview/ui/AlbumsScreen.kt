package com.hotsteel.hotview.ui

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.DarkMode
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.hotsteel.hotview.GalleryState
import com.hotsteel.hotview.MediaAccess
import com.hotsteel.hotview.media.Album
import com.hotsteel.hotview.ui.theme.ThemeMode
import com.hotsteel.hotview.ui.theme.label
import androidx.compose.ui.res.stringResource
import com.hotsteel.hotview.R

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AlbumsTab(
    state: GalleryState,
    themeMode: ThemeMode,
    onCycleTheme: () -> Unit,
    onOpenAlbum: (Album) -> Unit,
    onOpenAll: () -> Unit,
    onPickPhotos: () -> Unit,
    onOpenAbout: () -> Unit,
    onOpenSettings: () -> Unit,
    onGrantAccess: () -> Unit,
) {
    LazyVerticalGrid(
        columns = GridCells.Fixed(2),
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = BottomBarSpace),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(span = { GridItemSpan(maxLineSpan) }, key = "header") {
            ScreenHeader(
                title = stringResource(R.string.tabs_albums),
                subtitle = stringResource(R.string.albums_subtitle, state.albums.size, state.items.size),
                trailing = {
                    Row {
                        IconButton(onClick = onOpenSettings) {
                            Icon(
                                imageVector = Icons.Filled.Settings,
                                contentDescription = stringResource(R.string.action_settings),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        IconButton(onClick = onOpenAbout) {
                            Icon(
                                imageVector = Icons.Filled.Info,
                                contentDescription = stringResource(R.string.about_title),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        IconButton(onClick = onCycleTheme) {
                            Icon(
                                imageVector = Icons.Filled.DarkMode,
                                contentDescription = themeMode.label,
                                tint = MaterialTheme.colorScheme.primary,
                            )
                        }
                    }
                },
            )
        }

        item(span = { GridItemSpan(maxLineSpan) }, key = "hero") {
            RevealOnAppear(index = 0) {
                HeroCard(state = state, onClick = onOpenAll)
            }
        }

        if (state.access == MediaAccess.Denied || state.access == MediaAccess.Partial) {
            item(span = { GridItemSpan(maxLineSpan) }, key = "access") {
                RevealOnAppear(index = 1) {
                    AccessCard(
                        partial = state.access == MediaAccess.Partial,
                        onGrantAccess = onGrantAccess,
                        onPickPhotos = onPickPhotos,
                    )
                }
            }
        }

        items(
            items = state.albums,
            key = { it.id },
            contentType = { "album" },
        ) { album ->
            val index = state.albums.indexOf(album).coerceAtLeast(0)
            RevealOnAppear(index = index + 2) {
                AlbumCard(album = album, onClick = { onOpenAlbum(album) })
            }
        }
    }
}

@Composable
fun PhotosTab(
    items: List<com.hotsteel.hotview.media.MediaItem>,
    columns: Int,
    onOpen: (Int) -> Unit,
) {
    MediaGrid(
        items = items,
        columns = columns,
        onOpen = onOpen,
        header = {
            ScreenHeader(
                title = stringResource(R.string.tabs_photos),
                subtitle = stringResource(R.string.hero_items_count, items.size),
            )
        },
    )
}

@Composable
fun PickedTab(
    items: List<com.hotsteel.hotview.media.MediaItem>,
    columns: Int,
    onPick: () -> Unit,
    onOpen: (Int) -> Unit,
) {
    if (items.isEmpty()) {
        EmptyPicked(onPick = onPick)
    } else {
        MediaGrid(
            items = items,
            columns = columns,
            onOpen = onOpen,
            header = {
                ScreenHeader(
                    title = stringResource(R.string.tabs_picked),
                    subtitle = stringResource(R.string.picked_subtitle, items.size),
                    trailing = {
                        FilledTonalButton(onClick = onPick) {
                            Text(stringResource(R.string.action_add))
                        }
                    },
                )
            },
        )
    }
}

@Composable
private fun EmptyPicked(onPick: () -> Unit) {
    val transition = rememberInfiniteTransition(label = "emptyPulse")
    val pulse by transition.animateFloat(
        initialValue = 0.9f,
        targetValue = 1.06f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 1800),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "emptyPulseValue",
    )
    Column(
        modifier = Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.statusBars)
            .padding(horizontal = 32.dp, vertical = BottomBarSpace),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Box(
            Modifier
                .size(108.dp)
                .graphicsLayer {
                    scaleX = pulse
                    scaleY = pulse
                }
                .clip(androidx.compose.foundation.shape.CircleShape)
                .background(
                    Brush.linearGradient(
                        listOf(
                            MaterialTheme.colorScheme.primaryContainer,
                            MaterialTheme.colorScheme.tertiaryContainer,
                        ),
                    ),
                ),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.Filled.Bookmark,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onPrimaryContainer,
                modifier = Modifier.size(46.dp),
            )
        }
        Spacer(Modifier.height(20.dp))
        Text(
            text = stringResource(R.string.picked_empty_title),
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            text = stringResource(R.string.picked_empty_message),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp),
        )
        Spacer(Modifier.height(22.dp))
        Button(onClick = onPick) {
            Icon(
                imageVector = Icons.Filled.PhotoLibrary,
                contentDescription = null,
                modifier = Modifier.size(18.dp),
            )
            Spacer(Modifier.size(8.dp))
            Text(stringResource(R.string.action_open_picker))
        }
    }
}

@Composable
private fun HeroCard(state: GalleryState, onClick: () -> Unit) {
    val cover = state.items.firstOrNull()
    val interaction = remember { MutableInteractionSource() }
    Card(
        shape = MaterialTheme.shapes.extraLarge,
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
        ),
        modifier = Modifier
            .fillMaxWidth()
            .pressScale(interaction)
            .clickable(interactionSource = interaction, indication = null, onClick = onClick),
    ) {
        Box(
            Modifier
                .fillMaxWidth()
                .aspectRatio(16f / 8.5f),
        ) {
            MediaThumb(
                item = cover,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
            Box(
                Modifier
                    .fillMaxSize()
                    .background(
                        Brush.verticalGradient(
                            listOf(
                                Color.Transparent,
                                Color.Black.copy(alpha = 0.68f),
                            ),
                        ),
                    ),
            )
            Column(
                Modifier
                    .align(Alignment.BottomStart)
                    .padding(18.dp),
            ) {
                Text(
                    text = stringResource(R.string.hero_all_photos),
                    color = Color.White,
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = if (cover != null) stringResource(R.string.hero_items_count, state.items.size)
                    else stringResource(R.string.hero_grant_hint),
                    color = Color.White.copy(alpha = 0.85f),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }
}

@Composable
private fun AlbumCard(album: Album, onClick: () -> Unit) {
    val interaction = remember { MutableInteractionSource() }
    Card(
        shape = MaterialTheme.shapes.large,
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
        ),
        modifier = Modifier
            .pressScale(interaction)
            .clickable(interactionSource = interaction, indication = null, onClick = onClick),
    ) {
        Box(
            Modifier
                .fillMaxWidth()
                .aspectRatio(1f),
        ) {
            MediaThumb(
                item = album.cover,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
            Box(
                Modifier
                    .fillMaxSize()
                    .background(
                        Brush.verticalGradient(
                            listOf(Color.Transparent, Color.Black.copy(alpha = 0.62f)),
                        ),
                    ),
            )
            Column(
                Modifier
                    .align(Alignment.BottomStart)
                    .padding(14.dp),
            ) {
                Text(
                    text = album.name,
                    color = Color.White,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = stringResource(R.string.album_items_count, album.count),
                    color = Color.White.copy(alpha = 0.82f),
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
    }
}

@Composable
private fun AccessCard(
    partial: Boolean,
    onGrantAccess: () -> Unit,
    onPickPhotos: () -> Unit,
) {
    Card(
        shape = MaterialTheme.shapes.large,
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.tertiaryContainer,
        ),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(18.dp)) {
            Text(
                text = stringResource(if (partial) R.string.access_partial_title else R.string.access_denied_title),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onTertiaryContainer,
            )
            Text(
                text = stringResource(
                    if (partial) R.string.access_partial_message else R.string.access_denied_message,
                ),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onTertiaryContainer.copy(alpha = 0.85f),
                modifier = Modifier.padding(top = 6.dp),
            )
            Row(Modifier.padding(top = 14.dp)) {
                Button(onClick = onGrantAccess) {
                    Text(stringResource(if (partial) R.string.action_reselect else R.string.action_grant))
                }
                Spacer(Modifier.size(10.dp))
                FilledTonalButton(onClick = onPickPhotos) {
                    Text(stringResource(R.string.action_photo_picker))
                }
            }
        }
    }
}
