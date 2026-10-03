package com.hotsteel.hotview.ui.theme

import android.content.Context
import android.content.SharedPreferences
import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.res.stringResource
import com.hotsteel.hotview.R

/** Follow the wallpaper (Monet), or force light/dark. */
enum class ThemeMode { System, Light, Dark }

val ThemeMode.label: String
    @Composable get() = stringResource(
        when (this) {
            ThemeMode.System -> R.string.theme_system
            ThemeMode.Light -> R.string.theme_light
            ThemeMode.Dark -> R.string.theme_dark
        },
    )

/** Cycle order for the toolbar button: system → dark → light. */
fun ThemeMode.next(): ThemeMode = when (this) {
    ThemeMode.System -> ThemeMode.Dark
    ThemeMode.Dark -> ThemeMode.Light
    ThemeMode.Light -> ThemeMode.System
}

/** Persists the user's theme choice. */
class ThemeController(private val preferences: SharedPreferences) {
    fun load(): ThemeMode =
        ThemeMode.entries.getOrNull(preferences.getInt(KEY, ThemeMode.System.ordinal))
            ?: ThemeMode.System

    fun save(mode: ThemeMode) {
        preferences.edit().putInt(KEY, mode.ordinal).apply()
    }

    private companion object {
        const val KEY = "theme_mode"
    }
}

@Composable
fun rememberThemeController(): ThemeController {
    val context = LocalContext.current
    return androidx.compose.runtime.remember {
        ThemeController(context.getSharedPreferences("hotview", Context.MODE_PRIVATE))
    }
}

/** M3 Expressive shapes: noticeably rounder than the baseline scale. */
val HotviewShapes = Shapes(
    extraSmall = RoundedCornerShape(8.dp),
    small = RoundedCornerShape(12.dp),
    medium = RoundedCornerShape(18.dp),
    large = RoundedCornerShape(24.dp),
    extraLarge = RoundedCornerShape(32.dp),
)

private val LightColors = lightColorScheme(
    primary = Color(0xFF1D63D2),
    secondary = Color(0xFF4A6178),
    tertiary = Color(0xFF7A5AA8),
    surfaceContainerLow = Color(0xFFF6F6FA),
    surfaceContainerHigh = Color(0xFFEDEDF4),
)

private val DarkColors = darkColorScheme(
    primary = Color(0xFF9CCAFF),
    secondary = Color(0xFFB6C8DC),
    tertiary = Color(0xFFD5BBFF),
)

@Immutable
data class HotviewColors(
    val viewerBackground: Color = Color(0xFF0A0A0C),
)

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun HotviewTheme(
    mode: ThemeMode = ThemeMode.System,
    content: @Composable () -> Unit,
) {
    val dark = when (mode) {
        ThemeMode.System -> isSystemInDarkTheme()
        ThemeMode.Light -> false
        ThemeMode.Dark -> true
    }
    val context = LocalContext.current
    val colorScheme: ColorScheme = when {
        // Full Monet dynamic colour (Material You) on Android 12+.
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
            if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> DarkColors
        else -> LightColors
    }

    MaterialExpressiveTheme(
        colorScheme = colorScheme,
        motionScheme = MotionScheme.expressive(),
        shapes = HotviewShapes,
        typography = Typography(),
        content = content,
    )
}
