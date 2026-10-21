package com.coucou.android.ui

import android.os.SystemClock
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.coucou.android.R
import com.coucou.android.app.AppModel
import com.coucou.android.core.HomePanel
import com.coucou.android.core.HomeText
import com.coucou.android.core.IconKind
import com.coucou.android.core.Pills
import com.coucou.android.core.ToolLabels
import com.coucou.android.core.TypeScale
import com.coucou.android.mochi.BotState
import com.coucou.android.mochi.MochiConst
import com.coucou.android.mochi.MochiEngine
import com.coucou.android.mochi.MochiView

/**
 * Detailed live session screen for AI coding agents (Antigravity, VS Code, Cursor, Claude Code):
 * shows live task steps, thoughts, conversation, modified files, and an interactive prompt
 * composer to dispatch instructions directly to the application running on the PC.
 */
@Composable
fun SessionScreen(model: AppModel, pillId: String, onBack: () -> Unit) {
    val s = model.sessions.firstOrNull { it.pillId == pillId }
    val clock = remember { { SystemClock.elapsedRealtimeNanos() / 1e6 } }
    val engine = remember(pillId) { MochiEngine(clock) }

    var promptInput by remember { mutableStateOf("") }
    var feedbackMessage by remember { mutableStateOf<String?>(null) }

    if (s == null) {
        Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
            ScreenTitle(stringResource(R.string.session_details), onBack)
            Text(stringResource(R.string.session_ended), Modifier.padding(Gutter), style = TypeScale.BODY.style(tokens().textDim.c()))
        }
        return
    }

    if (engine.state != s.state) engine.setState(s.state)
    engine.bodyColor = HomePanel.colorHex(s)?.let { HomePanel.rgb(it) }
    val name = s.agent.ifBlank { Pills.byId(s.pillId)?.name.orEmpty() }
    val accent = HomePanel.colorHex(s)?.let { HomePanel.rgb(it) }?.let(::rgbColor) ?: tokens().text.c()
    val t = tokens()
    val busy = s.state == BotState.WORKING || s.state == BotState.THINKING || s.state == BotState.SEARCHING

    fun sendPromptToApp(raw: String) {
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) return
        val dispatchCmd = if (s.pillId == "agent_antigravity" || s.pillId.contains("antigravity", ignoreCase = true)) {
            "/antigravity $trimmed"
        } else {
            "/prompt ${name.lowercase()} $trimmed"
        }
        model.chatSend(dispatchCmd)
        promptInput = ""
        feedbackMessage = "Terkirim ke $name"
    }

    Column(Modifier.fillMaxSize()) {
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = Gutter),
            verticalArrangement = Arrangement.spacedBy(Gap),
        ) {
            item { ScreenTitle(name, onBack) }

            // Agent Status Header
            item {
                Panel(wash = MochiConst.STATES[s.state]?.color?.let(::rgbColor)) {
                    Row(
                        Modifier.padding(Gutter),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Gutter),
                    ) {
                        MochiView(engine, Modifier.size(96.dp))
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Text(
                                HomeText.line(s) ?: stringResource(stateLabel(s.state)),
                                style = TypeScale.HEADLINE.style(t.text.c()),
                                maxLines = 3,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                s.project?.let { Chip(stringResource(R.string.session_project, it)) }
                                if (busy) {
                                    Chip("Live", tint = Color(0xFF38BDF8))
                                }
                            }
                        }
                    }
                }
            }

            // Final Result / Message
            if (!s.finalLine.isNullOrBlank()) {
                item { SectionHeading(stringResource(R.string.session_last)) }
                item {
                    Panel(wash = MochiConst.STATES[BotState.FINISHED]?.color?.let(::rgbColor)) {
                        SelectionContainer {
                            Text(s.finalLine, Modifier.padding(Gutter), style = TypeScale.BODY.style(t.text.c()))
                        }
                    }
                }
            }

            // Modified Files & Diffs
            if (s.files.isNotEmpty()) {
                item { SectionHeading(stringResource(R.string.files_heading)) }
                item {
                    Panel {
                        s.files.asReversed().forEachIndexed { i, f ->
                            if (i > 0) RowDivider()
                            ListRow(
                                f.name,
                                hint = if (f.isNew) stringResource(R.string.files_new) else null,
                                onClick = { model.openDiff(s.pillId, f) },
                                trailing = {
                                    Text(
                                        stringResource(R.string.files_counts, f.added, f.removed),
                                        maxLines = 1,
                                        style = TypeScale.SECONDARY.style(t.textDim.c()).copy(fontFamily = FontFamily.Monospace),
                                    )
                                },
                            )
                        }
                    }
                }
            }

            // Steps & Execution Details
            item { SectionHeading(stringResource(R.string.session_steps)) }
            val steps = HomePanel.stepsNewestFirst(s)
            if (steps.isEmpty()) {
                item {
                    Panel {
                        Text(
                            stringResource(if (model.detailsOffered) R.string.session_no_steps else R.string.session_hint),
                            Modifier.padding(Gutter),
                            style = TypeScale.SECONDARY.style(t.textDim.c()),
                        )
                    }
                }
            } else {
                item {
                    Panel {
                        SelectionContainer {
                            Column {
                                steps.forEachIndexed { i, step ->
                                    if (i > 0) RowDivider()
                                    val current = i == 0 && busy

                                    val isPrompt = step.startsWith("Prompt:", ignoreCase = true) || step.startsWith("User:", ignoreCase = true)
                                    val isThought = step.startsWith("Thought:", ignoreCase = true) || step.startsWith("Thinking:", ignoreCase = true)
                                    val cleanStep = when {
                                        isPrompt -> step.substringAfter(":").trim()
                                        isThought -> step.substringAfter(":").trim()
                                        else -> step
                                    }

                                    Row(
                                        Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(horizontal = Gutter, vertical = 10.dp),
                                        verticalAlignment = Alignment.CenterVertically,
                                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                                    ) {
                                        if (isPrompt) {
                                            Box(
                                                Modifier.size(24.dp).clip(CircleShape).background(Color(0x33A855F7)),
                                                contentAlignment = Alignment.Center,
                                            ) {
                                                Text("💬", fontSize = 12.sp)
                                            }
                                        } else if (isThought) {
                                            Box(
                                                Modifier.size(24.dp).clip(CircleShape).background(Color(0x22FBBF24)),
                                                contentAlignment = Alignment.Center,
                                            ) {
                                                Text("💭", fontSize = 12.sp)
                                            }
                                        } else if (current) {
                                            StateDot(accent, Modifier.padding(horizontal = 7.dp))
                                        } else {
                                            CoucouIcon(IconKind.CHECK, tint = if (i == 0) StatusColors.online else t.textFaint.c(), size = 22.dp)
                                        }

                                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                            val label = ToolLabels.label(cleanStep).ifEmpty {
                                                stringResource(if (current) R.string.session_step_working else R.string.session_step_done)
                                            }

                                            Text(
                                                label,
                                                style = TypeScale.BODY.style(if (i == 0) t.text.c() else t.textDim.c()).copy(
                                                    fontWeight = if (current || isPrompt) FontWeight.SemiBold else FontWeight.Normal,
                                                    fontStyle = if (isThought) FontStyle.Italic else FontStyle.Normal,
                                                ),
                                                maxLines = 4,
                                                overflow = TextOverflow.Ellipsis,
                                            )

                                            // If the step has raw tool details distinct from the generic label, show them
                                            if (!isPrompt && !isThought && cleanStep != label && cleanStep.isNotBlank() && cleanStep.length > label.length) {
                                                Text(
                                                    cleanStep,
                                                    style = TypeScale.SECONDARY.style(t.textFaint.c()).copy(fontFamily = FontFamily.Monospace, fontSize = 11.sp),
                                                    maxLines = 2,
                                                    overflow = TextOverflow.Ellipsis,
                                                )
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            item { Spacer(Modifier.height(Gutter)) }
        }

        // Interactive Prompt Composer Bottom Section
        Column(
            Modifier.fillMaxWidth().background(t.panel.c()).padding(horizontal = Gutter, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            // Feedback notification toast
            if (feedbackMessage != null) {
                Row(
                    Modifier.fillMaxWidth()
                        .clip(RoundedCornerShape(8.dp))
                        .background(Color(0x2210B981))
                        .padding(horizontal = 12.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Text("✓", color = Color(0xFF34D399), fontWeight = FontWeight.Bold)
                    Text(
                        feedbackMessage ?: "",
                        style = TypeScale.SECONDARY.style(Color(0xFF34D399)),
                        fontWeight = FontWeight.Medium,
                    )
                }
            }

            // Quick suggestion chips
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                listOf(
                    "▶ Lanjutkan" to "Lanjutkan task berikutnya",
                    "🔍 Jelaskan progress" to "Jelaskan progress dan apa yang sudah dikerjakan",
                    "🧪 Test & Build" to "Jalankan test dan build sekarang",
                    "⏹ Selesai" to "Tugas selesai, berikan ringkasan",
                ).forEach { (label, prompt) ->
                    Box(
                        Modifier.clip(CircleShape)
                            .background(t.panel2.c())
                            .clickable { sendPromptToApp(prompt) }
                            .padding(horizontal = 12.dp, vertical = 6.dp),
                    ) {
                        Text(label, style = TypeScale.LABEL.style(t.text.c()), maxLines = 1)
                    }
                }
            }

            // Prompt text input & Send button
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedTextField(
                    value = promptInput,
                    onValueChange = {
                        promptInput = it
                        feedbackMessage = null
                    },
                    modifier = Modifier.weight(1f),
                    placeholder = {
                        Text(
                            "Kirim prompt ke $name…",
                            color = t.textFaint.c(),
                            fontSize = 13.5.sp,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    },
                    maxLines = 3,
                    shape = RoundedCornerShape(24.dp),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                    keyboardActions = KeyboardActions(onSend = { sendPromptToApp(promptInput) }),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedContainerColor = t.panel2.c(),
                        unfocusedContainerColor = t.panel2.c(),
                        disabledContainerColor = t.panel2.c(),
                        focusedBorderColor = accent,
                        unfocusedBorderColor = t.line.c(),
                        disabledBorderColor = t.line.c(),
                        focusedTextColor = t.text.c(),
                        unfocusedTextColor = t.text.c(),
                        cursorColor = accent,
                    ),
                )

                val canSend = promptInput.trim().isNotEmpty()
                val sendDesc = stringResource(R.string.chat_send)
                Box(
                    Modifier.alpha(if (canSend) 1f else 0.4f)
                        .size(48.dp)
                        .clip(CircleShape)
                        .background(accent)
                        .clickable(enabled = canSend, role = Role.Button) { sendPromptToApp(promptInput) }
                        .semantics { contentDescription = sendDesc },
                    contentAlignment = Alignment.Center,
                ) {
                    CoucouIcon(IconKind.SEND, tint = Color.Black, size = 20.dp)
                }
            }
        }
    }
}
