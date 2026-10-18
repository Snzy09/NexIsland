package com.coucou.android.alarm

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.Ringtone
import android.media.RingtoneManager
import android.os.Build
import android.os.IBinder
import android.os.VibrationEffect
import android.os.Vibrator
import androidx.core.app.NotificationCompat
import com.coucou.android.MainActivity
import com.coucou.android.R

class AlarmAlertService : Service() {
    companion object {
        const val CHANNEL_ID = "coucou_alarm_channel"
        const val NOTIFICATION_ID = 9021
        const val ACTION_DISMISS = "com.coucou.android.ALARM_DISMISS"
        const val ACTION_START = "com.coucou.android.ALARM_START"

        fun start(context: Context, alarmId: String, label: String, hour: Int, minute: Int) {
            val intent = Intent(context, AlarmAlertService::class.java).apply {
                action = ACTION_START
                putExtra(AlarmScheduler.EXTRA_ALARM_ID, alarmId)
                putExtra(AlarmScheduler.EXTRA_ALARM_LABEL, label)
                putExtra(AlarmScheduler.EXTRA_ALARM_HOUR, hour)
                putExtra(AlarmScheduler.EXTRA_ALARM_MINUTE, minute)
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        fun stop(context: Context) {
            val intent = Intent(context, AlarmAlertService::class.java).apply {
                action = ACTION_DISMISS
            }
            context.startService(intent)
        }
    }

    private var ringtone: Ringtone? = null
    private var vibrator: Vibrator? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_DISMISS) {
            stopAlarm()
            stopSelf()
            return START_NOT_STICKY
        }

        val label = intent?.getStringExtra(AlarmScheduler.EXTRA_ALARM_LABEL) ?: "Coucou Alarm"
        val hour = intent?.getIntExtra(AlarmScheduler.EXTRA_ALARM_HOUR, 7) ?: 7
        val minute = intent?.getIntExtra(AlarmScheduler.EXTRA_ALARM_MINUTE, 0) ?: 0
        val timeStr = "${hour.toString().padStart(2, '0')}:${minute.toString().padStart(2, '0')}"

        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification(label, timeStr))
        playAlarmSoundAndVibration()

        return START_STICKY
    }

    private fun playAlarmSoundAndVibration() {
        try {
            val alarmUri = RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
                ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION)
            ringtone = RingtoneManager.getRingtone(this, alarmUri).apply {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
                    audioAttributes = AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_ALARM)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build()
                }
                play()
            }
        } catch (_: Exception) {}

        try {
            vibrator = getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
            val pattern = longArrayOf(0, 800, 400, 800, 400, 800)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                vibrator?.vibrate(VibrationEffect.createWaveform(pattern, 0))
            } else {
                @Suppress("DEPRECATION")
                vibrator?.vibrate(pattern, 0)
            }
        } catch (_: Exception) {}
    }

    private fun stopAlarm() {
        try {
            ringtone?.stop()
            ringtone = null
        } catch (_: Exception) {}
        try {
            vibrator?.cancel()
            vibrator = null
        } catch (_: Exception) {}
    }

    override fun onDestroy() {
        stopAlarm()
        super.onDestroy()
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val name = "Coucou Alarm"
            val descriptionText = "Notifications for active Coucou alarms"
            val importance = NotificationManager.IMPORTANCE_HIGH
            val channel = NotificationChannel(CHANNEL_ID, name, importance).apply {
                description = descriptionText
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
                enableVibration(true)
            }
            val notificationManager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            notificationManager.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(label: String, time: String): Notification {
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val openPending = PendingIntent.getActivity(
            this,
            0,
            openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val dismissIntent = Intent(this, AlarmAlertService::class.java).apply {
            action = ACTION_DISMISS
        }
        val dismissPending = PendingIntent.getService(
            this,
            1,
            dismissIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle("⏰ Alarm: $time")
            .setContentText(label)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setCategory(NotificationCompat.CATEGORY_ALARM)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(openPending)
            .setOngoing(true)
            .addAction(R.mipmap.ic_launcher, "Matikan Alarm", dismissPending)
            .build()
    }
}
