package com.coucou.android.alarm

data class AlarmItem(
    val id: String,
    val hour: Int,
    val minute: Int,
    val label: String,
    val enabled: Boolean,
    val createdAtMs: Long,
) {
    val formattedTime: String get() {
        val h = hour.toString().padStart(2, '0')
        val m = minute.toString().padStart(2, '0')
        return "$h:$m"
    }

    val periodString: String get() {
        return if (hour < 12) "AM" else "PM"
    }

    val displayHour12: String get() {
        val h = when {
            hour == 0 -> 12
            hour > 12 -> hour - 12
            else -> hour
        }
        return h.toString().padStart(2, '0')
    }
}
