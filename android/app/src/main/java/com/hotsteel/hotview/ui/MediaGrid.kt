package com.hotsteel.hotview.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.hotsteel.hotview.media.MediaItem

/** Space reserved at the bottom of scrolling grids for the floating bar. */
val BottomBarSpace = 132.dp

/** Big expressive screen title that scrolls together with the content. */
@Composable
fun ScreenHeader(
    title: String,
    subtitle: String,
    modifier: Modifier = Modifier,
    trailing: (@Composable () -> Unit)? = null,
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .windowInsetsPadding(WindowInsets.statusBars)
            .padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            trailing?.invoke()
        }
    }
}

/**
 * Date-grouped media grid shared by the "photos" and "picked" tabs and the
 * album detail screen.
 */
@Composable
fun MediaGrid(
    items: List<MediaItem>,
    onOpen: (Int) -> Unit,
    modifier: Modifier = Modifier,
    header: (@Composable () -> Unit)? = null,
    contentPadding: PaddingValues = PaddingValues(start = 4.dp, end = 4.dp, bottom = BottomBarSpace),
) {
    val groups = remember(items) {
        items.groupBy { it.dateMillis / 86_400_000L }.toSortedMap(compareByDescending { it })
    }
    val absoluteIndex = remember(items) {
        items.withIndex().associate { (index, item) -> item.id to index }
    }

    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = 108.dp),
        modifier = modifier.fillMaxSize(),
        contentPadding = contentPadding,
        horizontalArrangement = Arrangement.spacedBy(3.dp),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        header?.let { headerContent ->
            item(span = { GridItemSpan(maxLineSpan) }, key = "custom-header") {
                headerContent()
            }
        }
        groups.forEach { (day, dayItems) ->
            item(span = { GridItemSpan(maxLineSpan) }, key = "header-$day") {
                Text(
                    text = formatDay(dayItems.first().dateMillis),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.padding(start = 10.dp, top = 16.dp, bottom = 6.dp),
                )
            }
            itemsIndexed(
                items = dayItems,
                key = { _, item -> item.id },
                contentType = { _, _ -> "cell" },
            ) { _, item ->
                RevealOnAppear(index = (absoluteIndex[item.id] ?: 0) % 24) {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .aspectRatio(1f)
                            .clip(RoundedCornerShape(6.dp))
                            .clickable { onOpen(absoluteIndex[item.id] ?: 0) },
                    ) {
                        MediaThumb(item = item, modifier = Modifier.fillMaxSize())
                    }
                }
            }
        }
    }
}
