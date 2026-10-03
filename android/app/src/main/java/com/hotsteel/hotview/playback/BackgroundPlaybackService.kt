package com.hotsteel.hotview.playback

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.IBinder
import com.hotsteel.hotview.MainActivity
import com.hotsteel.hotview.R

/**
 * Keeps playback alive while the app is in the background: a `mediaPlayback`
 * foreground service with a `MediaSession` and a notification whose actions
 * drive the native player through [NowPlaying]. Video decoding is suspended by
 * the native session while no surface exists; audio keeps playing.
 */
class BackgroundPlaybackService : Service() {
    private lateinit var session: MediaSession
    private lateinit var notifications: NotificationManager

    override fun onCreate() {
        super.onCreate()
        notifications = getSystemService(NotificationManager::class.java)
        session = MediaSession(this, "hotview").apply {
            setCallback(
                object : MediaSession.Callback() {
                    override fun onPlay() = NowPlaying.play()
                    override fun onPause() = NowPlaying.pause()
                    override fun onStop() {
                        NowPlaying.pause()
                        stopSelf()
                    }
                },
            )
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                NowPlaying.pause()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_TOGGLE -> NowPlaying.toggle()
        }

        notifications.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                getString(R.string.playback_channel),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )

        val playing = NowPlaying.isPlaying()
        session.setPlaybackState(
            PlaybackState.Builder()
                .setActions(
                    PlaybackState.ACTION_PLAY or
                        PlaybackState.ACTION_PAUSE or
                        PlaybackState.ACTION_STOP,
                )
                .setState(
                    if (playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
                    PlaybackState.PLAYBACK_POSITION_UNKNOWN,
                    1f,
                )
                .build(),
        )
        session.isActive = true

        val openApp = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val toggle = PendingIntent.getService(
            this,
            1,
            Intent(this, BackgroundPlaybackService::class.java).setAction(ACTION_TOGGLE),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this,
            2,
            Intent(this, BackgroundPlaybackService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_playback)
            .setContentTitle(NowPlaying.title)
            .setContentText(getString(R.string.app_name))
            .setContentIntent(openApp)
            .setOngoing(playing)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .addAction(
                Notification.Action.Builder(
                    Icon.createWithResource(this, R.drawable.ic_stat_playback),
                    getString(if (playing) R.string.action_pause else R.string.action_play),
                    toggle,
                ).build(),
            )
            .addAction(
                Notification.Action.Builder(
                    Icon.createWithResource(this, R.drawable.ic_stat_close),
                    getString(R.string.action_close),
                    stop,
                ).build(),
            )
            .setStyle(Notification.MediaStyle().setMediaSession(session.sessionToken))
            .build()
        startForeground(NOTIFICATION_ID, notification)
        return START_STICKY
    }

    override fun onDestroy() {
        session.isActive = false
        session.release()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        private const val CHANNEL_ID = "hotview-playback"
        private const val NOTIFICATION_ID = 11
        private const val ACTION_TOGGLE = "com.hotsteel.hotview.playback.TOGGLE"
        private const val ACTION_STOP = "com.hotsteel.hotview.playback.STOP"

        fun start(context: Context) {
            val intent = Intent(context, BackgroundPlaybackService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, BackgroundPlaybackService::class.java))
        }
    }
}
