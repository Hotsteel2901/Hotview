package com.hotsteel.hotview.ui

import android.content.Context
import android.content.SharedPreferences
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext

/**
 * Preferences behind the settings screen. Playback reads them when a video is
 * opened, so a change applies to the next opened video (no live re-init).
 */
class SettingsStore(private val preferences: SharedPreferences) {
    /** Decode video with the AOSP software decoder instead of the default one. */
    var softwareDecode: Boolean
        get() = preferences.getBoolean(KEY_SOFTWARE_DECODE, false)
        set(value) {
            preferences.edit().putBoolean(KEY_SOFTWARE_DECODE, value).apply()
        }

    /** Start video playback as soon as the viewer page becomes active. */
    var autoPlayVideo: Boolean
        get() = preferences.getBoolean(KEY_AUTOPLAY, true)
        set(value) {
            preferences.edit().putBoolean(KEY_AUTOPLAY, value).apply()
        }

    /** Loop videos by default. */
    var loopVideos: Boolean
        get() = preferences.getBoolean(KEY_LOOP, false)
        set(value) {
            preferences.edit().putBoolean(KEY_LOOP, value).apply()
        }

    /** Thumbnail grid columns: 2, 3 or 4. */
    var gridColumns: Int
        get() = preferences.getInt(KEY_GRID_COLUMNS, 2).coerceIn(2, 4)
        set(value) {
            preferences.edit().putInt(KEY_GRID_COLUMNS, value.coerceIn(2, 4)).apply()
        }

    /** Keep the screen awake while the viewer is open. */
    var keepScreenOn: Boolean
        get() = preferences.getBoolean(KEY_KEEP_SCREEN_ON, true)
        set(value) {
            preferences.edit().putBoolean(KEY_KEEP_SCREEN_ON, value).apply()
        }

    /** Cover the screen (fill) instead of fitting the media inside it. */
    var fillScreen: Boolean
        get() = preferences.getBoolean(KEY_FILL_SCREEN, false)
        set(value) {
            preferences.edit().putBoolean(KEY_FILL_SCREEN, value).apply()
        }

    /** Root tab shown on launch: 0 albums, 1 photos, 2 picked. */
    var startTab: Int
        get() = preferences.getInt(KEY_START_TAB, 0).coerceIn(0, 2)
        set(value) {
            preferences.edit().putInt(KEY_START_TAB, value.coerceIn(0, 2)).apply()
        }

    /** Keep playback alive (audio + notification) when the app is backgrounded. */
    var backgroundPlayback: Boolean
        get() = preferences.getBoolean(KEY_BACKGROUND_PLAYBACK, false)
        set(value) {
            preferences.edit().putBoolean(KEY_BACKGROUND_PLAYBACK, value).apply()
        }

    companion object {
        private const val KEY_SOFTWARE_DECODE = "video.software_decode"
        private const val KEY_AUTOPLAY = "video.autoplay"
        private const val KEY_LOOP = "video.loop"
        private const val KEY_GRID_COLUMNS = "ui.grid_columns"
        private const val KEY_KEEP_SCREEN_ON = "ui.keep_screen_on"
        private const val KEY_FILL_SCREEN = "ui.fill_screen"
        private const val KEY_START_TAB = "ui.start_tab"
        private const val KEY_BACKGROUND_PLAYBACK = "playback.background"

        fun from(context: Context): SettingsStore =
            SettingsStore(context.getSharedPreferences("hotview", Context.MODE_PRIVATE))
    }
}

@Composable
fun rememberSettingsStore(): SettingsStore {
    val context = LocalContext.current
    return remember { SettingsStore.from(context) }
}
