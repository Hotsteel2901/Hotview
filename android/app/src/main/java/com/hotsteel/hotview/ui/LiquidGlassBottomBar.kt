package com.hotsteel.hotview.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.chrisbanes.haze.HazeInput
import dev.chrisbanes.haze.HazeState
import dev.chrisbanes.haze.blur.HazeBlurStyle
import dev.chrisbanes.haze.blur.hazeBlur
import com.hotsteel.hotview.RootTab
import kotlin.math.roundToInt
import kotlinx.coroutines.launch
import androidx.compose.ui.res.stringResource

/**
 * Liquid-glass floating bottom bar.
 *
 * Structure inspired by ReSukiSU's manager bottom bar and miuix's liquid glass
 * navigation bar: a floating pill, a sliding selection lens and press
 * feedback. The actual blur comes from Haze (chrisbanes/haze, Apache-2.0),
 * which is the de-facto glassmorphism library for Compose.
 */
@Composable
fun LiquidGlassBottomBar(
    selected: RootTab,
    onSelected: (RootTab) -> Unit,
    hazeState: HazeState,
    modifier: Modifier = Modifier,
) {
    val tabs = RootTab.entries
    val shape = RoundedCornerShape(percent = 50)
    val scope = rememberCoroutineScope()

    val indicator = remember { Animatable(selected.ordinal.toFloat()) }
    var dragOffset by remember { mutableFloatStateOf(0f) }
    var barWidthPx by remember { mutableFloatStateOf(1f) }
    val tabWidthPx = barWidthPx / tabs.size

    LaunchedEffect(selected) {
        indicator.animateTo(
            targetValue = selected.ordinal.toFloat(),
            animationSpec = spring(dampingRatio = 0.74f, stiffness = 420f),
        )
    }

    Box(
        modifier = modifier
            .padding(horizontal = 18.dp)
            .navigationBarsPadding()
            .padding(bottom = 12.dp)
            .fillMaxWidth(),
        contentAlignment = Alignment.Center,
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(68.dp)
                .shadow(18.dp, shape, clip = false)
                .clip(shape)
                .hazeBlur(
                    input = HazeInput.Sources(hazeState),
                    style = HazeBlurStyle { blurRadius(28.dp) },
                )
                .background(
                    Brush.verticalGradient(
                        listOf(
                            MaterialTheme.colorScheme.surface.copy(alpha = 0.62f),
                            MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.55f),
                        ),
                    ),
                    shape,
                )
                .border(
                    width = 1.dp,
                    brush = Brush.linearGradient(
                        listOf(
                            Color.White.copy(alpha = 0.34f),
                            Color.White.copy(alpha = 0.06f),
                        ),
                    ),
                    shape = shape,
                )
                .onSizeChanged { barWidthPx = it.width.toFloat().coerceAtLeast(1f) }
                .pointerInput(tabs.size) {
                    detectHorizontalDragGestures(
                        onDragEnd = {
                            val target = (indicator.value + dragOffset)
                                .roundToInt()
                                .coerceIn(0, tabs.lastIndex)
                            dragOffset = 0f
                            scope.launch { indicator.snapTo(target.toFloat()) }
                            if (tabs[target] != selected) onSelected(tabs[target])
                        },
                        onDragCancel = { dragOffset = 0f },
                    ) { change, amount ->
                        if (tabWidthPx > 0f) {
                            dragOffset = (dragOffset + amount / tabWidthPx)
                                .coerceIn(-0.6f, tabs.size - 0.4f - indicator.value)
                            change.consume()
                        }
                    }
                },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.fillMaxSize()) {
                // Sliding "liquid" lens behind the selected tab.
                Box(
                    modifier = Modifier
                        .fillMaxHeight()
                        .fillMaxWidth(1f / tabs.size)
                        .graphicsLayer {
                            translationX = (indicator.value + dragOffset) * size.width
                        }
                        .padding(7.dp)
                        .clip(CircleShape)
                        .background(
                            Brush.verticalGradient(
                                listOf(
                                    MaterialTheme.colorScheme.primary.copy(alpha = 0.30f),
                                    MaterialTheme.colorScheme.primary.copy(alpha = 0.14f),
                                ),
                            ),
                        ),
                )

                Row(Modifier.fillMaxSize()) {
                    tabs.forEach { tab ->
                        BarItem(
                            tab = tab,
                            selected = tab == selected,
                            onClick = { onSelected(tab) },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun RowScope.BarItem(
    tab: RootTab,
    selected: Boolean,
    onClick: () -> Unit,
) {
    val tint by animateColorAsState(
        targetValue = if (selected) {
            MaterialTheme.colorScheme.primary
        } else {
            MaterialTheme.colorScheme.onSurfaceVariant
        },
        animationSpec = tween(durationMillis = 240),
        label = "barTint",
    )
    val scale by animateFloatAsState(
        targetValue = if (selected) 1.08f else 1f,
        animationSpec = spring(dampingRatio = 0.5f, stiffness = 500f),
        label = "barScale",
    )
    val interaction = remember { MutableInteractionSource() }

    Column(
        modifier = Modifier
            .weight(1f)
            .fillMaxHeight()
            .clip(CircleShape)
            .clickable(
                interactionSource = interaction,
                indication = null,
                onClick = onClick,
            )
            .graphicsLayer {
                scaleX = scale
                scaleY = scale
            },
        verticalArrangement = Arrangement.spacedBy(2.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            imageVector = tab.icon,
            contentDescription = stringResource(tab.labelRes),
            tint = tint,
            modifier = Modifier.size(23.dp),
        )
        Text(
            text = stringResource(tab.labelRes),
            color = tint,
            style = MaterialTheme.typography.labelSmall,
            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
        )
    }
}
