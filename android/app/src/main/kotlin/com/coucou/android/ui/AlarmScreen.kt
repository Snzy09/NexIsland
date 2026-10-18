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
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
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
import com.coucou.android.alarm.AlarmAlertService
import com.coucou.android.alarm.AlarmItem
import com.coucou.android.alarm.AlarmScheduler
import com.coucou.android.alarm.AlarmStore
import com.coucou.android.core.IconKind
import com.coucou.android.core.TypeScale
import java.util.Calendar
import java.util.UUID

@Composable
fun AlarmScreen(context: Context) {
    val store = remember { AlarmStore(context) }
    var alarms by remember { mutableStateOf(store.list()) }
    var isAdding by remember { mutableStateOf(false) }

    val t = tokens()

    Column(
        Modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp)
    ) {
        ScreenTitle(
            title = "Alarm",
            onBack = null,
            trailing = {
                Box(
                    modifier = Modifier
                        .clip(CircleShape)
                        .background(t.primaryButton.c())
                        .clickable { isAdding = true }
                        .padding(horizontal = 14.dp, vertical = 7.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "+ Tambah Alarm",
                        style = TypeScale.LABEL.style(t.onPrimaryButton.c()).copy(fontWeight = FontWeight.Bold),
                    )
                }
            }
        )

        Spacer(Modifier.height(8.dp))

        // Quick Test Bar: allows testing alarm sound & notification in 5 seconds
        Box(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(14.dp))
                .background(t.panel.c())
                .border(1.dp, t.line.c(), RoundedCornerShape(14.dp))
                .padding(horizontal = 14.dp, vertical = 10.dp)
        ) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        "Uji Coba Alarm Langsung",
                        style = TypeScale.LABEL.style(t.text.c()).copy(fontWeight = FontWeight.SemiBold),
                    )
                    Text(
                        "Tes nada dering & notifikasi layar penuh",
                        style = TypeScale.SECONDARY.style(t.textDim.c()),
                    )
                }
                Box(
                    Modifier
                        .clip(RoundedCornerShape(10.dp))
                        .background(Color(0xFF38BDF8))
                        .clickable {
                            val testCal = Calendar.getInstance().apply { add(Calendar.SECOND, 5) }
                            val testAlarm = AlarmItem(
                                id = "test_alarm",
                                hour = testCal.get(Calendar.HOUR_OF_DAY),
                                minute = testCal.get(Calendar.MINUTE),
                                label = "Tes Alarm Coucou Mochi 🔔",
                                enabled = true,
                                createdAtMs = System.currentTimeMillis(),
                            )
                            AlarmScheduler.schedule(context, testAlarm)
                        }
                        .padding(horizontal = 10.dp, vertical = 6.dp)
                ) {
                    Text(
                        "Tes (5 Detik)",
                        style = TypeScale.SECONDARY.style(Color.Black).copy(fontWeight = FontWeight.Bold),
                    )
                }
            }
        }

        Spacer(Modifier.height(12.dp))

        if (alarms.isEmpty()) {
            Box(
                Modifier
                    .fillMaxSize()
                    .padding(bottom = BarClearance),
                contentAlignment = Alignment.Center,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    CoucouIcon(IconKind.ALARM, tint = t.textDim.c(), size = 48.dp)
                    Spacer(Modifier.height(12.dp))
                    Text(
                        "Belum ada alarm",
                        style = TypeScale.TITLE.style(t.text.c()),
                    )
                    Spacer(Modifier.height(4.dp))
                    Text(
                        "Ketuk '+ Tambah Alarm' untuk mengatur waktu pengingat harian.",
                        style = TypeScale.SECONDARY.style(t.textDim.c()),
                    )
                }
            }
        } else {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(bottom = BarClearance + 16.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                items(alarms, key = { it.id }) { alarm ->
                    AlarmCard(
                        alarm = alarm,
                        onToggle = { enabled ->
                            val updated = store.toggle(alarm.id, enabled)
                            if (updated != null) {
                                if (enabled) {
                                    AlarmScheduler.schedule(context, updated)
                                } else {
                                    AlarmScheduler.cancel(context, alarm.id)
                                }
                            }
                            alarms = store.list()
                        },
                        onDelete = {
                            AlarmScheduler.cancel(context, alarm.id)
                            alarms = store.delete(alarm.id)
                        }
                    )
                }
            }
        }
    }

    if (isAdding) {
        AddAlarmDialog(
            onDismiss = { isAdding = false },
            onSave = { hour, minute, label ->
                val newAlarm = AlarmItem(
                    id = UUID.randomUUID().toString(),
                    hour = hour,
                    minute = minute,
                    label = label,
                    enabled = true,
                    createdAtMs = System.currentTimeMillis(),
                )
                alarms = store.save(newAlarm)
                AlarmScheduler.schedule(context, newAlarm)
                isAdding = false
            }
        )
    }
}

@Composable
private fun AlarmCard(
    alarm: AlarmItem,
    onToggle: (Boolean) -> Unit,
    onDelete: () -> Unit,
) {
    val t = tokens()
    val remainingText = remember(alarm.hour, alarm.minute, alarm.enabled) {
        if (!alarm.enabled) "Nonaktif"
        else formatTimeRemaining(alarm.hour, alarm.minute)
    }

    Box(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .background(t.panel.c())
            .border(1.dp, t.line.c(), RoundedCornerShape(18.dp))
            .padding(16.dp)
    ) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.Bottom) {
                    Text(
                        text = alarm.formattedTime,
                        fontSize = 32.sp,
                        fontWeight = FontWeight.Bold,
                        color = if (alarm.enabled) t.text.c() else t.textDim.c(),
                    )
                    Spacer(Modifier.width(6.dp))
                    Text(
                        text = alarm.periodString,
                        fontSize = 14.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = if (alarm.enabled) Color(0xFF38BDF8) else t.textDim.c(),
                        modifier = Modifier.padding(bottom = 4.dp),
                    )
                }

                Spacer(Modifier.height(2.dp))

                Text(
                    text = alarm.label.ifBlank { "Alarm" },
                    style = TypeScale.BODY.style(if (alarm.enabled) t.text.c() else t.textDim.c()),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )

                Text(
                    text = remainingText,
                    style = TypeScale.SECONDARY.style(if (alarm.enabled) Color(0xFF34D399) else t.textDim.c()),
                )
            }

            Column(horizontalAlignment = Alignment.End) {
                Switch(
                    checked = alarm.enabled,
                    onCheckedChange = onToggle,
                    colors = SwitchDefaults.colors(
                        checkedThumbColor = Color.White,
                        checkedTrackColor = Color(0xFF38BDF8),
                        uncheckedThumbColor = t.textDim.c(),
                        uncheckedTrackColor = t.panel2.c(),
                    )
                )
                Spacer(Modifier.height(6.dp))
                Text(
                    text = "Hapus",
                    style = TypeScale.SECONDARY.style(t.danger.c()),
                    modifier = Modifier
                        .clickable(onClick = onDelete)
                        .padding(horizontal = 4.dp, vertical = 2.dp),
                )
            }
        }
    }
}

@Composable
private fun AddAlarmDialog(
    onDismiss: () -> Unit,
    onSave: (hour: Int, minute: Int, label: String) -> Unit,
) {
    val cal = Calendar.getInstance()
    var hour by remember { mutableIntStateOf(cal.get(Calendar.HOUR_OF_DAY)) }
    var minute by remember { mutableIntStateOf((cal.get(Calendar.MINUTE) + 5) % 60) }
    var label by remember { mutableStateOf("") }
    val t = tokens()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text(
                "Setel Alarm Baru",
                style = TypeScale.TITLE.style(t.text.c()),
            )
        },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                // Time selector
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TimeNumberSpinner(
                        value = hour,
                        range = 0..23,
                        onValueChange = { hour = it },
                        modifier = Modifier.width(60.dp),
                    )
                    Text(
                        ":",
                        fontSize = 28.sp,
                        fontWeight = FontWeight.Bold,
                        color = t.text.c(),
                        modifier = Modifier.padding(horizontal = 8.dp),
                    )
                    TimeNumberSpinner(
                        value = minute,
                        range = 0..59,
                        onValueChange = { minute = it },
                        modifier = Modifier.width(60.dp),
                    )
                }

                OutlinedTextField(
                    value = label,
                    onValueChange = { label = it },
                    label = { Text("Label / Catatan Alarm") },
                    singleLine = true,
                    placeholder = { Text("misal: Bangun Pagi, Review Task") },
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
                    onSave(hour, minute, label.ifBlank { "Alarm" })
                }
            ) {
                Text("Simpan Alarm", fontWeight = FontWeight.Bold, color = Color(0xFF38BDF8))
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

@Composable
private fun TimeNumberSpinner(
    value: Int,
    range: IntRange,
    onValueChange: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val t = tokens()
    Column(modifier = modifier, horizontalAlignment = Alignment.CenterHorizontally) {
        Box(
            Modifier
                .size(36.dp)
                .clip(CircleShape)
                .background(t.panel2.c())
                .clickable {
                    val next = if (value + 1 > range.last) range.first else value + 1
                    onValueChange(next)
                },
            contentAlignment = Alignment.Center,
        ) {
            Text("▲", fontSize = 12.sp, color = t.text.c())
        }

        Spacer(Modifier.height(4.dp))

        Text(
            text = value.toString().padStart(2, '0'),
            fontSize = 24.sp,
            fontWeight = FontWeight.Bold,
            color = t.text.c(),
        )

        Spacer(Modifier.height(4.dp))

        Box(
            Modifier
                .size(36.dp)
                .clip(CircleShape)
                .background(t.panel2.c())
                .clickable {
                    val prev = if (value - 1 < range.first) range.last else value - 1
                    onValueChange(prev)
                },
            contentAlignment = Alignment.Center,
        ) {
            Text("▼", fontSize = 12.sp, color = t.text.c())
        }
    }
}

private fun formatTimeRemaining(hour: Int, minute: Int): String {
    val now = Calendar.getInstance()
    val target = Calendar.getInstance().apply {
        set(Calendar.HOUR_OF_DAY, hour)
        set(Calendar.MINUTE, minute)
        set(Calendar.SECOND, 0)
    }
    var diffMs = target.timeInMillis - now.timeInMillis
    if (diffMs <= 0) {
        diffMs += 24 * 60 * 60 * 1000L
    }
    val diffMinutes = (diffMs / (60 * 1000)).toInt()
    val h = diffMinutes / 60
    val m = diffMinutes % 60
    return when {
        h > 0 && m > 0 -> "Berdering dalam $h jam $m mnt"
        h > 0 -> "Berdering dalam $h jam"
        m > 0 -> "Berdering dalam $m menit"
        else -> "Berdering kurang dari 1 menit"
    }
}
