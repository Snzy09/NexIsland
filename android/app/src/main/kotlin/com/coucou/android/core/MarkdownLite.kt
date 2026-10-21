package com.coucou.android.core

/** A run of text with rich styling for chat answers. */
data class MdSpan(
    val text: String,
    val bold: Boolean = false,
    val italic: Boolean = false,
    val underline: Boolean = false,
    val strike: Boolean = false,
    val mark: Boolean = false,
    val code: Boolean = false,
    val math: Boolean = false,
    val kbd: Boolean = false,
)

sealed interface MdBlock {
    data class Para(val spans: List<MdSpan>) : MdBlock
    data class Bullet(val spans: List<MdSpan>) : MdBlock
    data class Task(val checked: Boolean, val spans: List<MdSpan>) : MdBlock
    data class Code(val text: String, val lang: String = "") : MdBlock
    data class Math(val text: String) : MdBlock
    data class Card(val type: String, val title: String, val text: String) : MdBlock
    data class Table(val headers: List<String>, val rows: List<List<String>>) : MdBlock
}

/**
 * Rich Markdown parser for chat answers: bold, italic, underline, strikethrough,
 * mark highlight, kbd, inline & block scientific math, tables, cards, and task lists.
 */
object MarkdownLite {
    fun parse(text: String): List<MdBlock> {
        val blocks = mutableListOf<MdBlock>()
        val para = mutableListOf<String>()
        fun flush() {
            if (para.isNotEmpty()) blocks.add(MdBlock.Para(inline(para.joinToString("\n")))); para.clear()
        }
        val lines = text.replace("\r\n", "\n").split("\n")
        var i = 0
        while (i < lines.size) {
            val line = lines[i]
            val trimmed = line.trimStart()
            when {
                trimmed.startsWith("```") -> {
                    flush()
                    val lang = trimmed.removePrefix("```").trim()
                    val code = mutableListOf<String>()
                    i++
                    while (i < lines.size && !lines[i].trimStart().startsWith("```")) code.add(lines[i++])
                    if (lang.equals("math", ignoreCase = true) || lang.equals("latex", ignoreCase = true)) {
                        blocks.add(MdBlock.Math(code.joinToString("\n")))
                    } else {
                        blocks.add(MdBlock.Code(code.joinToString("\n"), lang))
                    }
                }
                trimmed.startsWith("$$") -> {
                    flush()
                    if (trimmed.length > 2 && trimmed.endsWith("$$") && trimmed != "$$") {
                        blocks.add(MdBlock.Math(trimmed.removePrefix("$$").removeSuffix("$$").trim()))
                    } else {
                        val mathLines = mutableListOf<String>()
                        val firstContent = trimmed.removePrefix("$$").trim()
                        if (firstContent.isNotEmpty()) mathLines.add(firstContent)
                        i++
                        while (i < lines.size && !lines[i].trim().endsWith("$$")) mathLines.add(lines[i++])
                        if (i < lines.size) {
                            val lastContent = lines[i].trim().removeSuffix("$$").trim()
                            if (lastContent.isNotEmpty()) mathLines.add(lastContent)
                        }
                        blocks.add(MdBlock.Math(mathLines.joinToString("\n")))
                    }
                }
                trimmed.startsWith("\\[") -> {
                    flush()
                    if (trimmed.endsWith("\\]")) {
                        blocks.add(MdBlock.Math(trimmed.removePrefix("\\[").removeSuffix("\\]").trim()))
                    } else {
                        val mathLines = mutableListOf<String>()
                        val firstContent = trimmed.removePrefix("\\[").trim()
                        if (firstContent.isNotEmpty()) mathLines.add(firstContent)
                        i++
                        while (i < lines.size && !lines[i].trim().endsWith("\\]")) mathLines.add(lines[i++])
                        if (i < lines.size) {
                            val lastContent = lines[i].trim().removeSuffix("\\]").trim()
                            if (lastContent.isNotEmpty()) mathLines.add(lastContent)
                        }
                        blocks.add(MdBlock.Math(mathLines.joinToString("\n")))
                    }
                }
                isCardStart(trimmed) -> {
                    flush()
                    val rawType = trimmed.substringAfter(":::").substringBefore(" ").substringBefore("\n").trim().lowercase()
                    val cardType = if (rawType == "card") "info" else rawType
                    val title = trimmed.substringAfter(":::").substringAfter(" ").substringBefore("\n").trim().ifEmpty {
                        cardType.replaceFirstChar { it.uppercase() }
                    }
                    val cardLines = mutableListOf<String>()
                    i++
                    while (i < lines.size && !lines[i].trim().startsWith(":::")) cardLines.add(lines[i++])
                    blocks.add(MdBlock.Card(cardType, title, cardLines.joinToString("\n")))
                }
                isAlertHeader(trimmed) -> {
                    flush()
                    val alertTag = trimmed.substringAfter("[!").substringBefore("]").lowercase()
                    val cardType = if (alertTag == "danger") "caution" else alertTag
                    val title = trimmed.substringAfter("]").trim().ifEmpty {
                        cardType.replaceFirstChar { it.uppercase() }
                    }
                    val cardLines = mutableListOf<String>()
                    i++
                    while (i < lines.size && (lines[i].startsWith("> ") || lines[i] == ">")) {
                        cardLines.add(lines[i].removePrefix("> "))
                        i++
                    }
                    i-- // compensate loop step
                    blocks.add(MdBlock.Card(cardType, title, cardLines.joinToString("\n")))
                }
                isTableStart(trimmed, lines.getOrNull(i + 1)) -> {
                    flush()
                    val headers = parseCells(trimmed)
                    val rows = mutableListOf<List<String>>()
                    i += 2 // skip header and divider
                    while (i < lines.size && lines[i].contains("|") && lines[i].isNotBlank()) {
                        rows.add(parseCells(lines[i]))
                        i++
                    }
                    i-- // compensate loop step
                    blocks.add(MdBlock.Table(headers, rows))
                }
                isTask(trimmed) -> {
                    flush()
                    val checked = trimmed.contains("[x]", ignoreCase = true)
                    val taskText = trimmed.substringAfter("]").trim()
                    blocks.add(MdBlock.Task(checked, inline(taskText)))
                }
                line.isBlank() -> flush()
                isBullet(trimmed) -> { flush(); blocks.add(MdBlock.Bullet(inline(trimmed.substring(2).trimStart()))) }
                trimmed.startsWith("#") && trimmed.dropWhile { it == '#' }.startsWith(" ") -> {
                    flush(); blocks.add(MdBlock.Para(listOf(MdSpan(trimmed.dropWhile { it == '#' }.trim(), bold = true))))
                }
                else -> para.add(line)
            }
            i++
        }
        flush()
        return blocks
    }

    private fun isBullet(t: String) = t.length > 2 && (t[0] == '-' || t[0] == '*' || t[0] == '•') && t[1] == ' '
    private fun isTask(t: String) = (t.startsWith("- [ ]") || t.startsWith("- [x]") || t.startsWith("- [X]") ||
            t.startsWith("* [ ]") || t.startsWith("* [x]") || t.startsWith("* [X]"))
    private fun isAlertHeader(t: String) = t.startsWith("> [!") && t.contains("]")
    private fun isCardStart(t: String) = t.startsWith(":::") && !t.trim().equals(":::")
    private fun isTableStart(line: String, next: String?): Boolean {
        if (!line.contains("|") || next == null || !next.contains("|")) return false
        val d = next.trim()
        return d.contains("---") || d.contains("-")
    }

    private fun parseCells(line: String): List<String> {
        val s = line.trim().removePrefix("|").removeSuffix("|")
        return s.split("|").map { it.trim() }
    }

    /** Parses rich inlines: bold, italic, underline, strikethrough, mark, kbd, math, and code. */
    fun inline(text: String): List<MdSpan> {
        val out = mutableListOf<MdSpan>()
        val plain = StringBuilder()
        fun emitPlain() { if (plain.isNotEmpty()) out.add(MdSpan(plain.toString())); plain.clear() }

        var i = 0
        while (i < text.length) {
            // Display Math $$...$$
            if (text.startsWith("$$", i)) {
                val end = text.indexOf("$$", i + 2)
                if (end > i + 1) {
                    emitPlain()
                    val mathText = text.substring(i + 2, end).trim()
                    out.add(MdSpan(mathText, math = true))
                    i = end + 2
                    continue
                }
            }
            // LaTeX \[...\]
            if (text.startsWith("\\[", i)) {
                val end = text.indexOf("\\]", i + 2)
                if (end > i + 1) {
                    emitPlain()
                    val mathText = text.substring(i + 2, end).trim()
                    out.add(MdSpan(mathText, math = true))
                    i = end + 2
                    continue
                }
            }
            // Inline Math $...$
            if (text[i] == '$' && i + 1 < text.length && text[i + 1] != '$') {
                val end = text.indexOf('$', i + 1)
                if (end > i + 1) {
                    emitPlain()
                    val mathText = text.substring(i + 1, end).trim()
                    out.add(MdSpan(mathText, math = true))
                    i = end + 1
                    continue
                }
            }
            // LaTeX \(...\)
            if (text.startsWith("\\(", i)) {
                val end = text.indexOf("\\)", i + 2)
                if (end > i + 1) {
                    emitPlain()
                    val mathText = text.substring(i + 2, end).trim()
                    out.add(MdSpan(mathText, math = true))
                    i = end + 2
                    continue
                }
            }
            // Code `...`
            if (text[i] == '`') {
                val end = text.indexOf('`', i + 1)
                if (end > i + 1) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 1, end), code = true))
                    i = end + 1
                    continue
                }
            }
            // Bold **...** or __...__
            if (text.startsWith("**", i)) {
                val end = text.indexOf("**", i + 2)
                if (end > i + 2) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 2, end), bold = true))
                    i = end + 2
                    continue
                }
            }
            if (text.startsWith("__", i)) {
                val end = text.indexOf("__", i + 2)
                if (end > i + 2) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 2, end), bold = true))
                    i = end + 2
                    continue
                }
            }
            // Underline <u>...</u> or ++...++
            if (text.startsWith("<u>", i, ignoreCase = true)) {
                val end = text.indexOf("</u>", i + 3, ignoreCase = true)
                if (end > i + 3) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 3, end), underline = true))
                    i = end + 4
                    continue
                }
            }
            if (text.startsWith("++", i)) {
                val end = text.indexOf("++", i + 2)
                if (end > i + 2) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 2, end), underline = true))
                    i = end + 2
                    continue
                }
            }
            // Strike ~~...~~ or <del>...</del>
            if (text.startsWith("~~", i)) {
                val end = text.indexOf("~~", i + 2)
                if (end > i + 2) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 2, end), strike = true))
                    i = end + 2
                    continue
                }
            }
            if (text.startsWith("<del>", i, ignoreCase = true)) {
                val end = text.indexOf("</del>", i + 5, ignoreCase = true)
                if (end > i + 5) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 5, end), strike = true))
                    i = end + 6
                    continue
                }
            }
            // Highlight ==...== or <mark>...</mark>
            if (text.startsWith("==", i)) {
                val end = text.indexOf("==", i + 2)
                if (end > i + 2) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 2, end), mark = true))
                    i = end + 2
                    continue
                }
            }
            if (text.startsWith("<mark>", i, ignoreCase = true)) {
                val end = text.indexOf("</mark>", i + 6, ignoreCase = true)
                if (end > i + 6) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 6, end), mark = true))
                    i = end + 7
                    continue
                }
            }
            // Keycap <kbd>...</kbd>
            if (text.startsWith("<kbd>", i, ignoreCase = true)) {
                val end = text.indexOf("</kbd>", i + 5, ignoreCase = true)
                if (end > i + 5) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 5, end), kbd = true))
                    i = end + 6
                    continue
                }
            }
            // Italic *...* or _..._
            if (text[i] == '*' && i + 1 < text.length && text[i + 1] != '*') {
                val end = text.indexOf('*', i + 1)
                if (end > i + 1) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 1, end), italic = true))
                    i = end + 1
                    continue
                }
            }
            if (text[i] == '_' && i + 1 < text.length && text[i + 1] != '_' && (i == 0 || !text[i - 1].isLetterOrDigit())) {
                val end = text.indexOf('_', i + 1)
                if (end > i + 1 && (end + 1 >= text.length || !text[end + 1].isLetterOrDigit())) {
                    emitPlain()
                    out.add(MdSpan(text.substring(i + 1, end), italic = true))
                    i = end + 1
                    continue
                }
            }

            plain.append(text[i])
            i++
        }
        emitPlain()
        return out
    }
}

/** The codes the computer (or this app) gives for an answer that did not complete. */
object ChatReasons {
    val KNOWN = listOf(
        "off", "not_allowed", "busy", "rate", "too_long", "empty", "no_key", "unreachable", "auth", "provider",
        "canceled", "internal", "connection", "interrupted",
    )

    /** A code this version does not know is shown as a general problem. */
    fun key(code: String?): String = if (code != null && code in KNOWN) code else "internal"
}
