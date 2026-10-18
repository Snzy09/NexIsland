package com.coucou.android.notes

data class Note(
    val id: String,
    val title: String,
    val content: String,
    val updatedAtMs: Long,
)
