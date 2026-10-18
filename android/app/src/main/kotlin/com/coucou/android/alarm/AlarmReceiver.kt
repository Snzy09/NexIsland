package com.coucou.android.alarm

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class AlarmReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val alarmId = intent.getStringExtra(AlarmScheduler.EXTRA_ALARM_ID) ?: return
        val label = intent.getStringExtra(AlarmScheduler.EXTRA_ALARM_LABEL) ?: "Coucou Alarm"
        val hour = intent.getIntExtra(AlarmScheduler.EXTRA_ALARM_HOUR, 7)
        val minute = intent.getIntExtra(AlarmScheduler.EXTRA_ALARM_MINUTE, 0)

        AlarmAlertService.start(context, alarmId, label, hour, minute)
        // Reschedule for the next day automatically
        val store = AlarmStore(context)
        val alarm = store.list().firstOrNull { it.id == alarmId }
        if (alarm != null && alarm.enabled) {
            AlarmScheduler.schedule(context, alarm)
        }
    }
}
