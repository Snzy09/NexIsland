package com.coucou.android.alarm

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

class AlarmStore(context: Context) {
    private val prefs = context.getSharedPreferences("coucou_alarms", Context.MODE_PRIVATE)

    fun list(): List<AlarmItem> {
        val raw = prefs.getString("alarms_json", null) ?: return defaultAlarms()
        return try {
            val array = JSONArray(raw)
            val out = ArrayList<AlarmItem>(array.length())
            for (i in 0 until array.length()) {
                val obj = array.getJSONObject(i)
                out.add(
                    AlarmItem(
                        id = obj.getString("id"),
                        hour = obj.getInt("hour"),
                        minute = obj.getInt("minute"),
                        label = obj.optString("label", "Alarm"),
                        enabled = obj.optBoolean("enabled", true),
                        createdAtMs = obj.optLong("createdAtMs", System.currentTimeMillis()),
                    )
                )
            }
            out.sortedWith(compareBy({ it.hour }, { it.minute }))
        } catch (_: Exception) {
            defaultAlarms()
        }
    }

    fun save(alarm: AlarmItem): List<AlarmItem> {
        val current = list().filter { it.id != alarm.id }.toMutableList()
        current.add(alarm)
        persist(current)
        return list()
    }

    fun toggle(id: String, enabled: Boolean): AlarmItem? {
        val current = list().toMutableList()
        val index = current.indexOfFirst { it.id == id }
        if (index >= 0) {
            val updated = current[index].copy(enabled = enabled)
            current[index] = updated
            persist(current)
            return updated
        }
        return null
    }

    fun delete(id: String): List<AlarmItem> {
        val current = list().filter { it.id != id }
        persist(current)
        return current
    }

    private fun persist(alarms: List<AlarmItem>) {
        val array = JSONArray()
        for (a in alarms) {
            val obj = JSONObject()
            obj.put("id", a.id)
            obj.put("hour", a.hour)
            obj.put("minute", a.minute)
            obj.put("label", a.label)
            obj.put("enabled", a.enabled)
            obj.put("createdAtMs", a.createdAtMs)
            array.put(obj)
        }
        prefs.edit().putString("alarms_json", array.toString()).apply()
    }

    private fun defaultAlarms(): List<AlarmItem> = listOf(
        AlarmItem(
            id = "default_morning",
            hour = 7,
            minute = 0,
            label = "Bangun Pagi bersama Mochi",
            enabled = false,
            createdAtMs = System.currentTimeMillis(),
        ),
        AlarmItem(
            id = "default_work",
            hour = 9,
            minute = 30,
            label = "Review Task / Standup",
            enabled = false,
            createdAtMs = System.currentTimeMillis(),
        )
    )
}
