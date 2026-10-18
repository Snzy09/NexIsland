package com.coucou.android.notes

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

class NotesStore(context: Context) {
    private val prefs = context.getSharedPreferences("coucou_notes", Context.MODE_PRIVATE)

    fun list(): List<Note> {
        val raw = prefs.getString("notes_json", null) ?: return defaultNotes()
        return try {
            val array = JSONArray(raw)
            val out = ArrayList<Note>(array.length())
            for (i in 0 until array.length()) {
                val obj = array.getJSONObject(i)
                out.add(
                    Note(
                        id = obj.getString("id"),
                        title = obj.optString("title", ""),
                        content = obj.optString("content", ""),
                        updatedAtMs = obj.optLong("updatedAtMs", System.currentTimeMillis()),
                    )
                )
            }
            out.sortedByDescending { it.updatedAtMs }
        } catch (_: Exception) {
            defaultNotes()
        }
    }

    fun save(note: Note): List<Note> {
        val current = list().filter { it.id != note.id }.toMutableList()
        current.add(0, note)
        persist(current)
        return current
    }

    fun delete(id: String): List<Note> {
        val current = list().filter { it.id != id }
        persist(current)
        return current
    }

    private fun persist(notes: List<Note>) {
        val array = JSONArray()
        for (n in notes) {
            val obj = JSONObject()
            obj.put("id", n.id)
            obj.put("title", n.title)
            obj.put("content", n.content)
            obj.put("updatedAtMs", n.updatedAtMs)
            array.put(obj)
        }
        prefs.edit().putString("notes_json", array.toString()).apply()
    }

    private fun defaultNotes(): List<Note> = listOf(
        Note(
            id = "welcome",
            title = "Catatan Selamat Datang 📝",
            content = "Selamat datang di tab Notes Coucou!\n\nDi sini kamu bisa menulis catatan cepat, ide prompt untuk AI, to-do list, atau snippet kode.\n\nKamu juga bisa mengetuk tombol 'Kirim ke Chat' untuk langsung mendiskusikan catatan ini dengan AI!",
            updatedAtMs = System.currentTimeMillis(),
        )
    )
}
