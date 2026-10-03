package com.hotsteel.hotview.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas as DrawCanvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hotsteel.hotview.media.MediaItem
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.delay
import androidx.compose.ui.res.stringResource
import com.hotsteel.hotview.R

/** A thumbnail tile with a video badge and a shimmer while it loads. */
@Composable
fun MediaThumb(
    item: MediaItem?,
    modifier: Modifier = Modifier,
    contentScale: ContentScale = ContentScale.Crop,
) {
    if (item == null) {
        Box(modifier.shimmerBackground())
        return
    }
    val context = LocalContext.current
    val bitmap by produceState<ImageBitmap?>(
        initialValue = ThumbnailLoader.peek(item.uri),
        key1 = item.uri,
    ) {
        if (value == null) {
            value = ThumbnailLoader.load(context, item)
        }
    }
    Box(modifier) {
        val image = bitmap
        if (image != null) {
            // Soft crossfade from the shimmer to the picture.
            var visible by remember(image) { mutableStateOf(false) }
            LaunchedEffect(image) { visible = true }
            val alpha by androidx.compose.animation.core.animateFloatAsState(
                targetValue = if (visible) 1f else 0f,
                animationSpec = tween(durationMillis = 260),
                label = "thumbFade",
            )
            Image(
                bitmap = image,
                contentDescription = null,
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer { this.alpha = alpha },
                contentScale = contentScale,
            )
        } else {
            Box(Modifier.fillMaxSize().shimmerBackground())
        }
        if (item.isVideo) {
            VideoBadge(item.durationMs, Modifier.align(Alignment.BottomEnd).padding(6.dp))
        }
    }
}

@Composable
fun VideoBadge(durationMs: Long, modifier: Modifier = Modifier) {
    Box(
        modifier
            .clip(RoundedCornerShape(6.dp))
            .background(Color.Black.copy(alpha = 0.62f))
            .padding(horizontal = 6.dp, vertical = 2.dp),
    ) {
        Text(
            text = formatDuration(durationMs),
            color = Color.White,
            fontSize = 11.sp,
            fontWeight = FontWeight.Medium,
        )
    }
}

/** Play triangle (the core icon set only ships PlayArrow). */
@Composable
fun PlayGlyph(modifier: Modifier = Modifier, color: Color = Color.White, glyphSize: Dp = 20.dp) {
    DrawCanvas(modifier.size(glyphSize)) {
        val width = size.width
        val height = size.height
        val path = Path().apply {
            moveTo(width * 0.24f, height * 0.12f)
            lineTo(width * 0.90f, height * 0.50f)
            lineTo(width * 0.24f, height * 0.88f)
            close()
        }
        drawPath(path, color)
    }
}

/** Pause bars. */
@Composable
fun PauseGlyph(modifier: Modifier = Modifier, color: Color = Color.White, glyphSize: Dp = 20.dp) {
    DrawCanvas(modifier.size(glyphSize)) {
        val width = size.width
        val height = size.height
        val barWidth = width * 0.22f
        val radius = CornerRadius(barWidth / 3f, barWidth / 3f)
        drawRoundRect(
            color = color,
            topLeft = Offset(width * 0.18f, height * 0.12f),
            size = Size(barWidth, height * 0.76f),
            cornerRadius = radius,
        )
        drawRoundRect(
            color = color,
            topLeft = Offset(width * 0.60f, height * 0.12f),
            size = Size(barWidth, height * 0.76f),
            cornerRadius = radius,
        )
    }
}

/** Animated shimmer used for placeholders. */
@Composable
fun Modifier.shimmerBackground(corner: Dp = 0.dp): Modifier = this
    .clip(RoundedCornerShape(corner))
    .background(MaterialTheme.colorScheme.surfaceContainerHigh)

@Composable
fun ShimmerBox(modifier: Modifier = Modifier, corner: Dp = 0.dp) {
    val transition = rememberInfiniteTransition(label = "shimmer")
    val progress by transition.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 1200, easing = LinearEasing),
            repeatMode = RepeatMode.Restart,
        ),
        label = "shimmerProgress",
    )
    val base = MaterialTheme.colorScheme.surfaceContainerHigh
    val highlight = MaterialTheme.colorScheme.surfaceContainerLowest
    androidx.compose.foundation.Canvas(modifier) {
        val width = size.width
        val sweep = width * 0.7f
        val startX = -sweep + (width + sweep) * progress
        val brush = Brush.linearGradient(
            colors = listOf(base, highlight, base),
            start = Offset(startX, 0f),
            end = Offset(startX + sweep, size.height),
        )
        drawRoundRect(brush = brush, cornerRadius = CornerRadius(corner.toPx()))
    }
}

/** Scale + fade entrance, staggered by [index]. */
@Composable
fun RevealOnAppear(
    index: Int,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    var visible by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        delay((index.coerceAtMost(12) * 45).toLong())
        visible = true
    }
    val progress by androidx.compose.animation.core.animateFloatAsState(
        targetValue = if (visible) 1f else 0f,
        animationSpec = spring(dampingRatio = 0.72f, stiffness = 260f),
        label = "reveal-$index",
    )
    Box(
        modifier.graphicsLayer {
            alpha = progress
            translationY = (1f - progress) * 64f
            val scale = 0.92f + 0.08f * progress
            scaleX = scale
            scaleY = scale
        },
    ) {
        content()
    }
}

/** Press feedback used on cards and tiles. */
@Composable
fun Modifier.pressScale(
    interactionSource: MutableInteractionSource,
    pressedScale: Float = 0.95f,
): Modifier {
    val pressed by interactionSource.collectIsPressedAsState()
    val scale by androidx.compose.animation.core.animateFloatAsState(
        targetValue = if (pressed) pressedScale else 1f,
        animationSpec = spring(dampingRatio = 0.55f, stiffness = 520f),
        label = "pressScale",
    )
    return this.graphicsLayer {
        scaleX = scale
        scaleY = scale
    }
}

@Composable
fun formatDuration(millis: Long): String {
    if (millis <= 0) return stringResource(R.string.duration_unknown)
    val totalSeconds = millis / 1000
    val hours = totalSeconds / 3600
    val minutes = (totalSeconds % 3600) / 60
    val seconds = totalSeconds % 60
    return if (hours > 0) {
        String.format(Locale.US, "%d:%02d:%02d", hours, minutes, seconds)
    } else {
        String.format(Locale.US, "%d:%02d", minutes, seconds)
    }
}

fun formatDate(millis: Long): String = try {
    val locale = Locale.getDefault()
    val pattern = android.text.format.DateFormat.getBestDateTimePattern(locale, "yMMMdHm")
    SimpleDateFormat(pattern, locale).format(Date(millis))
} catch (_: Throwable) {
    ""
}

fun formatDay(millis: Long): String = try {
    val locale = Locale.getDefault()
    val pattern = android.text.format.DateFormat.getBestDateTimePattern(locale, "yMMMMEEEEd")
    SimpleDateFormat(pattern, locale).format(Date(millis))
} catch (_: Throwable) {
    ""
}

@Composable
fun humanSize(bytes: Long): String {
    if (bytes <= 0) return stringResource(R.string.size_unknown)
    val units = arrayOf("B", "KB", "MB", "GB")
    var value = bytes.toDouble()
    var unit = 0
    while (value >= 1024 && unit < units.lastIndex) {
        value /= 1024
        unit++
    }
    return if (unit == 0) "$bytes B" else String.format(Locale.US, "%.1f %s", value, units[unit])
}

@Composable
fun resolutionLabel(width: Int, height: Int): String =
    if (width > 0 && height > 0) "$width × $height" else stringResource(R.string.resolution_unknown)
