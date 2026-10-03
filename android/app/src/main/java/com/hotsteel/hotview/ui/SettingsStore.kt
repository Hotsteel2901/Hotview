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

    companion object {
        private const val KEY_SOFTWARE_DECODE = "video.software_decode"
        private const val KEY_AUTOPLAY = "video.autoplay"
        private const val KEY_LOOP = "video.loop"

        fun from(context: Context): SettingsStore =
            SettingsStore(context.getSharedPreferences("hotview", Context.MODE_PRIVATE))
    }
}

@Composable
fun rememberSettingsStore(): SettingsStore {
    val context = LocalContext.current
    return remember { SettingsStore.from(context) }
}
