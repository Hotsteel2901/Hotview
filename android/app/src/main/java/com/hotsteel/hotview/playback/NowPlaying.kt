package com.hotsteel.hotview.playback

import com.hotsteel.hotview.native.MediaSurfaceView

/**
 * The playback session the background service controls. The viewer registers
 * its surface view while background playback is active; the service drives it
 * from the notification without holding a hard reference after detach.
 */
object NowPlaying {
    @Volatile
    private var view: MediaSurfaceView? = null

    @Volatile
    var title: String = "Hotview"

    fun attach(target: MediaSurfaceView, name: String) {
        view = target
        title = name.ifBlank { "Hotview" }
    }

    fun detach() {
        view = null
    }

    fun isPlaying(): Boolean = view?.isPlaying() ?: false

    fun play() {
        view?.play()
    }

    fun pause() {
        view?.pause()
    }

    fun toggle() {
        val target = view ?: return
        if (target.isPlaying()) target.pause() else target.play()
    }
}
