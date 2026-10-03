package com.hotsteel.hotview.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.hotsteel.hotview.media.MediaItem
import com.hotsteel.hotview.native.MediaSurfaceView
import kotlin.math.abs
import kotlin.math.max
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import androidx.compose.ui.res.stringResource
import com.hotsteel.hotview.R
import androidx.compose.animation.core.animate
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.material.icons.filled.VolumeOff
import androidx.compose.foundation.Image
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.produceState
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextAlign
import com.hotsteel.hotview.native.PlatformDecode
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.compose.ui.platform.LocalContext

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ViewerScreen(
    items: List<MediaItem>,
    initialIndex: Int,
    onClose: () -> Unit,
) {
    if (items.isEmpty()) {
        LaunchedEffect(Unit) { onClose() }
        return
    }

    val pagerState = rememberPagerState(
        initialPage = initialIndex.coerceIn(0, items.lastIndex),
    ) { items.size }
    val currentItem = items.getOrNull(pagerState.currentPage)

    var controlsVisible by remember { mutableStateOf(true) }
    var infoItem by remember { mutableStateOf<MediaItem?>(null) }
    var currentView by remember { mutableStateOf<MediaSurfaceView?>(null) }
    var isPlaying by remember { mutableStateOf(false) }
    var positionMs by remember { mutableLongStateOf(0L) }
    var durationMs by remember { mutableLongStateOf(0L) }
    var looping by remember { mutableStateOf(false) }
    var hasAudio by remember { mutableStateOf(true) }
    var errorText by remember { mutableStateOf<String?>(null) }
    var scrubValue by remember { mutableStateOf<Float?>(null) }

    // Predictive back: scale the viewer down as the user drags back.
    var backScale by remember { mutableFloatStateOf(1f) }
    var backAlpha by remember { mutableFloatStateOf(1f) }

    ImmersiveSystemBars()

    // Android 17 hardens background audio: pause as soon as we are not visible.
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) currentView?.pause()
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    PredictiveBackHandler(enabled = true) { progress ->
        try {
            progress.collect { event ->
                val value = event.progress
                backScale = 1f - 0.16f * value
                backAlpha = 1f - 0.65f * value
            }
            onClose()
        } catch (cancellation: CancellationException) {
            backScale = 1f
            backAlpha = 1f
            throw cancellation
        }
    }

    LaunchedEffect(pagerState.currentPage) {
        errorText = null
        scrubValue = null
        isPlaying = false
        hasAudio = true
        positionMs = 0L
        durationMs = currentItem?.durationMs ?: 0L
        currentView?.setViewport(1f, 0f, 0f)
    }

    LaunchedEffect(pagerState.currentPage, currentView) {
        val item = items.getOrNull(pagerState.currentPage) ?: return@LaunchedEffect
        if (!item.isVideo) return@LaunchedEffect
        while (true) {
            val view = currentView ?: break
            isPlaying = view.isPlaying()
            hasAudio = view.hasAudio()
            positionMs = view.positionMs()
            val reported = view.durationMs()
            if (reported > 0L) durationMs = reported
            delay(200)
        }
    }

    LaunchedEffect(controlsVisible, currentItem?.id, isPlaying, pagerState.isScrollInProgress) {
        if (controlsVisible && !pagerState.isScrollInProgress) {
            delay(3500)
            controlsVisible = false
        }
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            .graphicsLayer {
                scaleX = backScale
                scaleY = backScale
                alpha = backAlpha
                transformOrigin = androidx.compose.ui.graphics.TransformOrigin(0.5f, 0.5f)
            },
    ) {
        HorizontalPager(
            state = pagerState,
            modifier = Modifier.fillMaxSize(),
            key = { items[it].id },
            beyondViewportPageCount = 1,
        ) { page ->
            val item = items[page]
            val offset = (pagerState.currentPage - page) + pagerState.currentPageOffsetFraction
            val distance = abs(offset).coerceIn(0f, 1f)
            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        val pageScale = 1f - 0.10f * distance
                        scaleX = pageScale
                        scaleY = pageScale
                        alpha = 1f - 0.35f * distance
                    },
            ) {
                ViewerPage(
                    item = item,
                    active = page == pagerState.currentPage && !pagerState.isScrollInProgress,
                    onViewReady = { view ->
                        if (page == pagerState.currentPage) currentView = view
                    },
                    onTap = { controlsVisible = !controlsVisible },
                    onError = { message ->
                        if (page == pagerState.currentPage) errorText = message
                    },
                )
            }
        }

        // ---- top chrome ---------------------------------------------------
        AnimatedVisibility(
            visible = controlsVisible,
            enter = fadeIn(tween(200)) + slideInVertically(tween(280)) { -it / 2 },
            exit = fadeOut(tween(160)) + slideOutVertically(tween(240)) { -it / 2 },
            modifier = Modifier.align(Alignment.TopCenter),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .fillMaxWidth()
                    .background(
                        Brush.verticalGradient(
                            listOf(Color.Black.copy(alpha = 0.75f), Color.Transparent),
                        ),
                    )
                    .windowInsetsPadding(WindowInsets.statusBars)
                    .padding(horizontal = 4.dp, vertical = 4.dp),
            ) {
                IconButton(onClick = onClose) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                        contentDescription = stringResource(R.string.action_back),
                        tint = Color.White,
                    )
                }
                Column(Modifier.weight(1f)) {
                    Text(
                        text = currentItem?.let { formatDate(it.dateMillis) }.orEmpty(),
                        color = Color.White,
                        style = MaterialTheme.typography.titleSmall,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = "${pagerState.currentPage + 1} / ${items.size}",
                        color = Color.White.copy(alpha = 0.78f),
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
                IconButton(onClick = { infoItem = currentItem }) {
                    Icon(
                        imageVector = Icons.Filled.Info,
                        contentDescription = stringResource(R.string.action_info),
                        tint = Color.White,
                    )
                }
            }
        }

        // ---- error --------------------------------------------------------
        errorText?.let { message ->
            Text(
                text = message,
                color = Color(0xFFFFD9D2),
                style = MaterialTheme.typography.bodySmall,
                textAlign = TextAlign.Center,
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(24.dp)
                    .background(Color.Black.copy(alpha = 0.55f), RoundedCornerShape(14.dp))
                    .padding(horizontal = 16.dp, vertical = 10.dp),
            )
        }

        // ---- bottom chrome ------------------------------------------------
        AnimatedVisibility(
            visible = controlsVisible,
            enter = fadeIn(tween(200)) + slideInVertically(tween(280)) { it / 2 },
            exit = fadeOut(tween(160)) + slideOutVertically(tween(240)) { it / 2 },
            modifier = Modifier.align(Alignment.BottomCenter),
        ) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .background(
                        Brush.verticalGradient(
                            listOf(Color.Transparent, Color.Black.copy(alpha = 0.78f)),
                        ),
                    )
                    .windowInsetsPadding(WindowInsets.navigationBars)
                    .padding(horizontal = 14.dp, vertical = 10.dp),
            ) {
                if (currentItem?.isVideo == true) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        IconButton(
                            onClick = {
                                val view = currentView
                                if (isPlaying) view?.pause() else view?.play()
                            },
                        ) {
                            AnimatedContent(
                                targetState = isPlaying,
                                transitionSpec = {
                                    (
                                        scaleIn(initialScale = 0.7f) + fadeIn(tween(140))
                                        ) togetherWith (
                                        scaleOut(targetScale = 0.7f) + fadeOut(tween(120))
                                        )
                                },
                                label = "playPause",
                            ) { playing ->
                                if (playing) {
                                    PauseGlyph(glyphSize = 22.dp)
                                } else {
                                    PlayGlyph(glyphSize = 22.dp)
                                }
                            }
                        }
                        Text(
                            text = formatDuration(positionMs),
                            color = Color.White,
                            style = MaterialTheme.typography.labelMedium,
                        )
                        Slider(
                            value = (scrubValue ?: positionMs.toFloat())
                                .coerceIn(0f, max(durationMs, 1L).toFloat()),
                            onValueChange = { scrubValue = it },
                            onValueChangeFinished = {
                                scrubValue?.let { currentView?.seekTo(it.toLong()) }
                                scrubValue = null
                            },
                            valueRange = 0f..max(durationMs, 1L).toFloat(),
                            colors = SliderDefaults.colors(
                                thumbColor = Color.White,
                                activeTrackColor = Color.White,
                                inactiveTrackColor = Color.White.copy(alpha = 0.32f),
                            ),
                            modifier = Modifier.weight(1f),
                        )
                        Text(
                            text = formatDuration(durationMs),
                            color = Color.White,
                            style = MaterialTheme.typography.labelMedium,
                        )
                        IconButton(
                            onClick = {
                                looping = !looping
                                currentView?.setLooping(looping)
                            },
                        ) {
                            Icon(
                                imageVector = Icons.Filled.Refresh,
                                contentDescription = stringResource(R.string.action_loop),
                                tint = if (looping) {
                                    Color.White
                                } else {
                                    Color.White.copy(alpha = 0.45f)
                                },
                            )
                        }
                        if (!hasAudio) {
                            Icon(
                                imageVector = Icons.Filled.VolumeOff,
                                contentDescription = stringResource(R.string.viewer_no_audio),
                                tint = Color.White.copy(alpha = 0.45f),
                            )
                        }
                    }
                } else {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text(
                            text = currentItem?.let { resolutionLabel(it.width, it.height) }.orEmpty(),
                            color = Color.White.copy(alpha = 0.85f),
                            style = MaterialTheme.typography.labelMedium,
                        )
                    }
                }
            }
        }
    }

    infoItem?.let { item ->
        ModalBottomSheet(onDismissRequest = { infoItem = null }) {
            InfoSheet(item)
        }
    }
}

@Composable
private fun ViewerPage(
    item: MediaItem,
    active: Boolean,
    onViewReady: (MediaSurfaceView) -> Unit,
    onTap: () -> Unit,
    onError: (String) -> Unit,
) {
    val context = LocalContext.current
    val viewRef = remember { mutableStateOf<MediaSurfaceView?>(null) }
    var failure by remember(item.id) { mutableStateOf<String?>(null) }
    var zoom by remember(item.id) { mutableFloatStateOf(1f) }
    var zoomTarget by remember(item.id) { mutableFloatStateOf(1f) }
    var pan by remember(item.id) { mutableStateOf(Offset.Zero) }
    val zoomedIn by remember { derivedStateOf { zoomTarget > 1.01f } }

    // Smoothly chase the gesture target: pinch follows the fingers, double tap
    // animates with an expressive spring.
    LaunchedEffect(zoomTarget) {
        androidx.compose.animation.core.animate(
            initialValue = zoom,
            targetValue = zoomTarget,
            animationSpec = spring(dampingRatio = 0.78f, stiffness = 420f),
        ) { value, _ ->
            zoom = value
        }
    }

    // Push the viewport to the native renderer every animation frame.
    LaunchedEffect(Unit) {
        snapshotFlow { zoom to pan }.collect { (scale, offset) ->
            viewRef.value?.setViewport(scale, offset.x, offset.y)
        }
    }

    // When the native renderer cannot start, still show the picture by
    // decoding it with the platform decoder.
    val fallback by produceState<ImageBitmap?>(initialValue = null, item.uri, failure) {
        if (failure != null && !item.isVideo) {
            value = withContext(Dispatchers.IO) {
                PlatformDecode.decode(context, item.uri)?.asImageBitmap()
            }
        }
    }

    Box(Modifier.fillMaxSize()) {
        AndroidView(
            factory = { context ->
                MediaSurfaceView(context).also { view ->
                    view.onErrorEvent = { message ->
                        failure = message
                        onError(message)
                    }
                    viewRef.value = view
                    view.load(item)
                    onViewReady(view)
                    if (item.isVideo) {
                        if (active) view.play() else view.pause()
                    }
                }
            },
            onRelease = { view ->
                view.dispose()
                if (viewRef.value === view) viewRef.value = null
            },
            modifier = Modifier.fillMaxSize(),
        )

        fallback?.let { image ->
            Image(
                bitmap = image,
                contentDescription = null,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Fit,
            )
        }

        LaunchedEffect(active) {
            val view = viewRef.value ?: return@LaunchedEffect
            if (item.isVideo) {
                if (active) view.play() else view.pause()
            }
        }

        Box(
            Modifier
                .fillMaxSize()
                .pointerInput(item.id) {
                    detectTapGestures(
                        onTap = { onTap() },
                        onDoubleTap = {
                            pan = Offset.Zero
                            zoomTarget = if (zoomedIn) 1f else 2.5f
                        },
                    )
                }
                .pointerInput(item.id, zoomedIn) {
                    awaitEachGesture {
                        var multiTouch = false
                        awaitFirstDown(requireUnconsumed = false)
                        while (true) {
                            val event = awaitPointerEvent()
                            if (event.changes.none { it.pressed }) break
                            val pressed = event.changes.count { it.pressed }
                            val zoomChange = event.calculateZoom()
                            val panChange = event.calculatePan()
                            if (pressed >= 2) {
                                multiTouch = true
                                zoomTarget = (zoomTarget * zoomChange).coerceIn(1f, 8f)
                                pan += panChange
                                event.changes.forEach { it.consume() }
                            } else if (multiTouch || zoomedIn) {
                                pan += panChange
                                event.changes.forEach {
                                    if (it.positionChanged()) it.consume()
                                }
                            }
                        }
                    }
                },
        )
    }
}

@Composable
private fun InfoSheet(item: MediaItem) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(start = 22.dp, end = 22.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            text = item.displayName,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        InfoRow(
            stringResource(R.string.info_type),
            stringResource(if (item.isVideo) R.string.info_video else R.string.info_image),
        )
        InfoRow(stringResource(R.string.info_resolution), resolutionLabel(item.width, item.height))
        InfoRow(stringResource(R.string.info_size), humanSize(item.sizeBytes))
        item.mimeType?.let { InfoRow(stringResource(R.string.info_format), it) }
        if (item.isVideo && item.durationMs > 0) {
            InfoRow(stringResource(R.string.info_duration), formatDuration(item.durationMs))
        }
        InfoRow(stringResource(R.string.info_date), formatDate(item.dateMillis))
        InfoRow(stringResource(R.string.info_album), item.bucketName)
    }
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth()) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.weight(0.35f),
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.weight(0.65f),
        )
    }
}

@Composable
private fun ImmersiveSystemBars() {
    val view = androidx.compose.ui.platform.LocalView.current
    if (!view.isInEditMode) {
        DisposableEffect(Unit) {
            val activity = view.context.findActivity()
            val window = activity?.window
            val controller = window?.let { WindowCompat.getInsetsController(it, view) }
            controller?.hide(WindowInsetsCompat.Type.systemBars())
            controller?.systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            onDispose {
                controller?.show(WindowInsetsCompat.Type.systemBars())
            }
        }
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
