package com.coucou.android.ui

import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.coucou.android.core.IconKind
import com.coucou.android.core.TypeScale
import com.coucou.android.notes.Note
import com.coucou.android.notes.NotesStore
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.UUID

@Composable
fun NotesScreen(
    context: Context,
    onSendToChat: (String) -> Unit,
) {
    val store = remember { NotesStore(context) }
    var notes by remember { mutableStateOf(store.list()) }
    var editingNote by remember { mutableStateOf<Note?>(null) }
    var isCreating by remember { mutableStateOf(false) }

    val t = tokens()

    Column(
        Modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp)
    ) {
        ScreenTitle(
            title = "Notes",
            onBack = null,
            trailing = {
                Box(
                    modifier = Modifier
                        .clip(CircleShape)
                        .background(t.primaryButton.c())
                        .clickable {
                            isCreating = true
                            editingNote = Note(
                                id = UUID.randomUUID().toString(),
                                title = "",
                                content = "",
                                updatedAtMs = System.currentTimeMillis(),
                            )
                        }
                        .padding(horizontal = 14.dp, vertical = 7.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "+ Buat Catatan",
                        style = TypeScale.LABEL.style(t.onPrimaryButton.c()).copy(fontWeight = FontWeight.Bold),
                    )
                }
            }
        )

        Spacer(Modifier.height(8.dp))

        if (notes.isEmpty()) {
            Box(
                Modifier
                    .fillMaxSize()
                    .padding(bottom = BarClearance),
                contentAlignment = Alignment.Center,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    CoucouIcon(IconKind.NOTE, tint = t.textDim.c(), size = 48.dp)
                    Spacer(Modifier.height(12.dp))
                    Text(
                        "Belum ada catatan",
                        style = TypeScale.TITLE.style(t.text.c()),
                    )
                    Spacer(Modifier.height(4.dp))
                    Text(
                        "Ketuk '+ Buat Catatan' untuk menambahkan ide, prompt, atau daftar tugas.",
                        style = TypeScale.SECONDARY.style(t.textDim.c()),
                        modifier = Modifier.padding(horizontal = 32.dp),
                    )
                }
            }
        } else {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(bottom = BarClearance + 16.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                items(notes, key = { it.id }) { note ->
                    NoteCard(
                        note = note,
                        onEdit = { editingNote = note },
                        onDelete = { notes = store.delete(note.id) },
                        onSendToChat = { onSendToChat(note.content) },
                    )
                }
            }
        }
    }

    editingNote?.let { noteToEdit ->
        NoteEditorDialog(
            initialNote = noteToEdit,
            isNew = isCreating,
            onDismiss = {
                editingNote = null
                isCreating = false
            },
            onSave = { savedNote ->
                notes = store.save(savedNote)
                editingNote = null
                isCreating = false
            }
        )
    }
}

@Composable
private fun NoteCard(
    note: Note,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
    onSendToChat: () -> Unit,
) {
    val t = tokens()
    val dateStr = remember(note.updatedAtMs) {
        val sdf = SimpleDateFormat("dd MMM, HH:mm", Locale.getDefault())
        sdf.format(Date(note.updatedAtMs))
    }

    Box(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(16.dp))
            .background(t.panel.c())
            .border(1.dp, t.line.c(), RoundedCornerShape(16.dp))
            .clickable(onClick = onEdit)
            .padding(14.dp)
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = note.title.ifBlank { "Tanpa Judul" },
                    style = TypeScale.TITLE.style(t.text.c()).copy(fontSize = 16.sp, fontWeight = FontWeight.Bold),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    text = dateStr,
                    style = TypeScale.SECONDARY.style(t.textDim.c()).copy(fontSize = 11.sp),
                )
            }

            if (note.content.isNotBlank()) {
                Text(
                    text = note.content,
                    style = TypeScale.BODY.style(t.text.c().copy(alpha = 0.85f)),
                    maxLines = 4,
                    overflow = TextOverflow.Ellipsis,
                    lineHeight = 19.sp,
                )
            }

            Spacer(Modifier.height(4.dp))

            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // Quick Action: Send note text directly to AI Chat
                Box(
                    Modifier
                        .clip(RoundedCornerShape(12.dp))
                        .background(t.panel2.c())
                        .clickable(onClick = onSendToChat)
                        .padding(horizontal = 10.dp, vertical = 5.dp)
                ) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(5.dp)
                    ) {
                        CoucouIcon(IconKind.CHAT, tint = Color(0xFF60A5FA), size = 14.dp)
                        Text(
                            "Kirim ke Chat",
                            style = TypeScale.SECONDARY.style(Color(0xFF60A5FA)).copy(fontWeight = FontWeight.SemiBold),
                        )
                    }
                }

                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        "Edit",
                        style = TypeScale.SECONDARY.style(t.textDim.c()),
                        modifier = Modifier
                            .clickable(onClick = onEdit)
                            .padding(horizontal = 6.dp, vertical = 4.dp),
                    )
                    Text(
                        "Hapus",
                        style = TypeScale.SECONDARY.style(t.danger.c()),
                        modifier = Modifier
                            .clickable(onClick = onDelete)
                            .padding(horizontal = 6.dp, vertical = 4.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun NoteEditorDialog(
    initialNote: Note,
    isNew: Boolean,
    onDismiss: () -> Unit,
    onSave: (Note) -> Unit,
) {
    var title by remember { mutableStateOf(initialNote.title) }
    var content by remember { mutableStateOf(initialNote.content) }
    val t = tokens()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text(
                if (isNew) "Catatan Baru" else "Edit Catatan",
                style = TypeScale.TITLE.style(t.text.c()),
            )
        },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                OutlinedTextField(
                    value = title,
                    onValueChange = { title = it },
                    label = { Text("Judul Catatan") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedContainerColor = t.panel.c(),
                        unfocusedContainerColor = t.panel.c(),
                        focusedTextColor = t.text.c(),
                        unfocusedTextColor = t.text.c(),
                    )
                )

                OutlinedTextField(
                    value = content,
                    onValueChange = { content = it },
                    label = { Text("Isi Catatan / Prompt") },
                    minLines = 4,
                    maxLines = 8,
                    modifier = Modifier.fillMaxWidth(),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedContainerColor = t.panel.c(),
                        unfocusedContainerColor = t.panel.c(),
                        focusedTextColor = t.text.c(),
                        unfocusedTextColor = t.text.c(),
                    )
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    onSave(
                        initialNote.copy(
                            title = title.trim(),
                            content = content.trim(),
                            updatedAtMs = System.currentTimeMillis(),
                        )
                    )
                }
            ) {
                Text("Simpan", fontWeight = FontWeight.Bold, color = Color(0xFF60A5FA))
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Batal", color = t.textDim.c())
            }
        },
        containerColor = t.panel.c(),
    )
}
