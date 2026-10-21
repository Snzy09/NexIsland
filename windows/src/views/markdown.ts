// Markdown in the chat's answers: headings, paragraphs, fenced code with language badge
// and copy button, scientific math output (KaTeX), GitHub alert cards and container cards,
// tables, task lists, bullet and numbered lists, quotes and rules, with bold, italic,
// underline, strikethrough, highlight, kbd keycaps, sub/superscript, `code` and links.
//
// Everything is built as DOM nodes with textContent and sanitized element structures,
// never unescaped HTML, so nothing a model writes can inject markup. Links open only
// when they are http(s), and even then through Rust, which checks the scheme again.

import katex from "katex";
import { Bridge } from "../core/bridge";
import { h, svg } from "./dom";
import { ICONS } from "./icons";
import { N_, t } from "../i18n/i18n";

const STRINGS = {
  copy: N_("Copy"),
  copied: N_("Copied"),
};

export type Block =
  | { kind: "heading"; level: number; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "code"; lang: string; code: string }
  | { kind: "math"; math: string }
  | { kind: "table"; headers: string[]; aligns: ("left" | "center" | "right")[]; rows: string[][] }
  | { kind: "card"; cardType: "note" | "tip" | "important" | "warning" | "caution" | "info"; title: string; text: string }
  | { kind: "task"; checked: boolean; text: string; indent: number }
  /** `prefix` is "•" or "1."; `indent` the nesting level, from 0. */
  | { kind: "item"; prefix: string; text: string; indent: number }
  | { kind: "quote"; text: string }
  | { kind: "rule" };

const isRule = (s: string) => s === "---" || s === "***" || s === "___";
const isBullet = (s: string) => /^[•\-*+] /.test(s);
const ordered = (s: string) => /^(\d+)\.\s+/.exec(s);
const isQuote = (s: string) => s.startsWith("> ") || s === ">";
const isTableDivider = (s: string) => /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)+\|?\s*$/.test(s);
const isTask = (s: string) => /^\s*[•\-*+]\s+\[([ xX])\]\s+(.*)/.exec(s);
const isAlertHeader = (s: string) => /^\s*>\s*\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION|INFO|DANGER)\]\s*(.*)/i.exec(s);
const isCardStart = (s: string) => /^\s*:::(card|note|tip|important|warning|caution|info|danger)\s*(.*)/i.exec(s);

function parseCells(line: string): string[] {
  let content = line.trim();
  if (content.startsWith("|")) content = content.slice(1);
  if (content.endsWith("|")) content = content.slice(0, -1);
  return content.split("|").map((c) => c.trim());
}

function parseAlign(cell: string): "left" | "center" | "right" {
  const c = cell.trim();
  const starts = c.startsWith(":");
  const ends = c.endsWith(":");
  if (starts && ends) return "center";
  if (ends) return "right";
  return "left";
}

/** Parses markdown into structured blocks. */
export function parseMarkdown(input: string): Block[] {
  const blocks: Block[] = [];
  const lines = input.replace(/\r\n?/g, "\n").split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];

    // Fenced code block or fenced math
    if (line.startsWith("```")) {
      const lang = line.slice(3).trim();
      const code: string[] = [];
      for (i++; i < lines.length && !lines[i].startsWith("```"); i++) code.push(lines[i]);
      if (lang === "math" || lang === "latex") {
        blocks.push({ kind: "math", math: code.join("\n") });
      } else {
        blocks.push({ kind: "code", lang, code: code.join("\n") });
      }
      i++;
      continue;
    }

    const trimmed = line.trim();

    // Display / block math $$ ... $$
    if (trimmed.startsWith("$$")) {
      if (trimmed.length > 2 && trimmed.endsWith("$$") && trimmed !== "$$") {
        blocks.push({ kind: "math", math: trimmed.slice(2, -2).trim() });
        i++;
        continue;
      }
      const mathLines: string[] = [];
      const firstLineContent = trimmed.slice(2).trim();
      if (firstLineContent) mathLines.push(firstLineContent);
      for (i++; i < lines.length; i++) {
        const curTrim = lines[i].trim();
        if (curTrim.endsWith("$$")) {
          const rest = curTrim.slice(0, -2).trim();
          if (rest) mathLines.push(rest);
          break;
        }
        mathLines.push(lines[i]);
      }
      blocks.push({ kind: "math", math: mathLines.join("\n") });
      i++;
      continue;
    }

    // Standalone \[ ... \]
    if (trimmed.startsWith("\\[")) {
      if (trimmed.endsWith("\\]")) {
        blocks.push({ kind: "math", math: trimmed.slice(2, -2).trim() });
        i++;
        continue;
      }
      const mathLines: string[] = [];
      const firstLineContent = trimmed.slice(2).trim();
      if (firstLineContent) mathLines.push(firstLineContent);
      for (i++; i < lines.length; i++) {
        const curTrim = lines[i].trim();
        if (curTrim.endsWith("\\]")) {
          const rest = curTrim.slice(0, -2).trim();
          if (rest) mathLines.push(rest);
          break;
        }
        mathLines.push(lines[i]);
      }
      blocks.push({ kind: "math", math: mathLines.join("\n") });
      i++;
      continue;
    }

    // Container card :::card or :::tip ... :::
    const cardMatch = isCardStart(line);
    if (cardMatch) {
      const rawType = cardMatch[1].toLowerCase();
      const cardType = (rawType === "card" ? "info" : rawType) as "note" | "tip" | "important" | "warning" | "caution" | "info";
      const title = cardMatch[2].trim() || (cardType.charAt(0).toUpperCase() + cardType.slice(1));
      const cardLines: string[] = [];
      for (i++; i < lines.length && !lines[i].trim().startsWith(":::"); i++) {
        cardLines.push(lines[i]);
      }
      blocks.push({ kind: "card", cardType, title, text: cardLines.join("\n") });
      i++;
      continue;
    }

    // GitHub Alert card > [!NOTE]
    const alertMatch = isAlertHeader(line);
    if (alertMatch) {
      const rawType = alertMatch[1].toLowerCase();
      const cardType = (rawType === "danger" ? "caution" : rawType) as "note" | "tip" | "important" | "warning" | "caution" | "info";
      const title = alertMatch[2].trim() || (cardType.charAt(0).toUpperCase() + cardType.slice(1));
      const cardLines: string[] = [];
      for (i++; i < lines.length && (lines[i].startsWith("> ") || lines[i] === ">"); i++) {
        const text = lines[i].startsWith("> ") ? lines[i].slice(2) : "";
        cardLines.push(text);
      }
      blocks.push({ kind: "card", cardType, title, text: cardLines.join("\n") });
      continue;
    }

    // Table detection: line has | and next line is table divider
    if (line.includes("|") && i + 1 < lines.length && isTableDivider(lines[i + 1])) {
      const headers = parseCells(line);
      const aligns = parseCells(lines[i + 1]).map(parseAlign);
      const rows: string[][] = [];
      i += 2;
      while (i < lines.length && lines[i].includes("|") && lines[i].trim() !== "") {
        rows.push(parseCells(lines[i]));
        i++;
      }
      blocks.push({ kind: "table", headers, aligns, rows });
      continue;
    }

    // Headings
    const hashes = /^#+/.exec(line)?.[0].length ?? 0;
    if (hashes && (line.length === hashes || line[hashes] === " ")) {
      const text = line.slice(hashes).trim();
      if (text) blocks.push({ kind: "heading", level: Math.min(hashes, 6), text });
      i++;
      continue;
    }

    const stripped = line.trim();
    if (isRule(stripped)) {
      blocks.push({ kind: "rule" });
      i++;
      continue;
    }

    // Task list items
    const taskMatch = isTask(stripped);
    if (taskMatch) {
      const indent = Math.floor((line.length - line.trimStart().length) / 2);
      blocks.push({ kind: "task", checked: taskMatch[1].toLowerCase() === "x", text: taskMatch[2], indent });
      i++;
      continue;
    }

    // Normal quote
    if (isQuote(stripped)) {
      blocks.push({ kind: "quote", text: stripped.startsWith("> ") ? stripped.slice(2) : "" });
      i++;
      continue;
    }

    const indent = Math.floor((line.length - line.trimStart().length) / 2);
    if (isBullet(stripped)) {
      const match = /^[•\-*+]\s+/.exec(stripped);
      const prefixLen = match ? match[0].length : 2;
      blocks.push({ kind: "item", prefix: "•", text: stripped.slice(prefixLen), indent });
      i++;
      continue;
    }
    const number = ordered(stripped);
    if (number) {
      blocks.push({ kind: "item", prefix: `${number[1]}.`, text: stripped.slice(number[0].length), indent });
      i++;
      continue;
    }
    if (!stripped) {
      i++;
      continue;
    }

    // Paragraph
    const para = [line];
    for (i++; i < lines.length; i++) {
      const next = lines[i];
      const s = next.trim();
      if (
        !s ||
        s.startsWith("#") ||
        s.startsWith("```") ||
        s.startsWith("$$") ||
        s.startsWith("\\[") ||
        isQuote(s) ||
        isBullet(s) ||
        isRule(s) ||
        ordered(s) ||
        isTask(s) ||
        isCardStart(s) ||
        (next.includes("|") && i + 1 < lines.length && isTableDivider(lines[i + 1]))
      ) {
        break;
      }
      para.push(next);
    }
    blocks.push({ kind: "paragraph", text: para.join("\n") });
  }
  return blocks;
}

// ── Inline ────────────────────────────────────────────────────────────────────

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong"; children: Inline[] }
  | { kind: "em"; children: Inline[] }
  | { kind: "u"; children: Inline[] }
  | { kind: "del"; children: Inline[] }
  | { kind: "mark"; children: Inline[] }
  | { kind: "kbd"; text: string }
  | { kind: "sup"; text: string }
  | { kind: "sub"; text: string }
  | { kind: "math"; math: string; display?: boolean }
  /** `url` is null when the target is not a web address: the text stays, the link goes. */
  | { kind: "link"; url: string | null; children: Inline[] };

/**
 * Rich inline formatting parser:
 * `code`, $$math$$, \[math\], $math$, \(math\), <kbd>key</kbd>, <u>under</u>, ++under++,
 * ~~strike~~, <del>strike</del>, <s>strike</s>, ==mark==, <mark>mark</mark>,
 * ^sup^, <sup>sup</sup>, ~sub~, <sub>sub</sub>, **bold**, __bold__,
 * *italic*, _italic_ and [text](url).
 */
const INLINE =
  /`([^`\n]+)`|\$\$([^\$]+?)\$\$|\\\[([\s\S]+?)\\\]|\$([^\$\n]+?)\$|\\\((.+?)\\\)|<kbd>([^<]+)<\/kbd>|<u>([^<]+)<\/u>|\+\+([^+]+)\+\+|~~([^~]+)~~|<del>([^<]+)<\/del>|<s>([^<]+)<\/s>|==([^=]+)==|<mark>([^<]+)<\/mark>|\^([^\s^]+)\^|<sup>([^<]+)<\/sup>|~([^\s~]+)~|<sub>([^<]+)<\/sub>|\*\*([^*]+?)\*\*|__([^_]+?)__|\*([^*\s][^*]*?)\*|_([^_\s][^_]*?)_(?!\w)|\[([^\]\n]+)\]\(([^)\s]+)\)/g;

/** The address if it is an http(s) one; anything else (javascript:, file:, data:…) is not a link. */
export function safeWebUrl(raw: string): string | null {
  try {
    const url = new URL(raw.trim());
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  const pushText = (t: string) => {
    if (!t) return;
    const last = out[out.length - 1];
    if (last?.kind === "text") last.text += t;
    else out.push({ kind: "text", text: t });
  };
  const re = new RegExp(INLINE.source, "g");
  let at = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    // snake_case check for _ (group 21)
    if (m[21] != null && m.index > 0 && /\w/.test(text[m.index - 1])) {
      re.lastIndex = m.index + 1;
      continue;
    }
    pushText(text.slice(at, m.index));
    at = m.index + m[0].length;
    if (m[1] != null) out.push({ kind: "code", text: m[1] });
    else if (m[2] != null || m[3] != null) out.push({ kind: "math", math: (m[2] ?? m[3]).trim(), display: true });
    else if (m[4] != null || m[5] != null) out.push({ kind: "math", math: (m[4] ?? m[5]).trim(), display: false });
    else if (m[6] != null) out.push({ kind: "kbd", text: m[6] });
    else if (m[7] != null || m[8] != null) out.push({ kind: "u", children: parseInline(m[7] ?? m[8]) });
    else if (m[9] != null || m[10] != null || m[11] != null) out.push({ kind: "del", children: parseInline(m[9] ?? m[10] ?? m[11]) });
    else if (m[12] != null || m[13] != null) out.push({ kind: "mark", children: parseInline(m[12] ?? m[13]) });
    else if (m[14] != null || m[15] != null) out.push({ kind: "sup", text: m[14] ?? m[15] });
    else if (m[16] != null || m[17] != null) out.push({ kind: "sub", text: m[16] ?? m[17] });
    else if (m[18] != null || m[19] != null) out.push({ kind: "strong", children: parseInline(m[18] ?? m[19]) });
    else if (m[20] != null || m[21] != null) out.push({ kind: "em", children: parseInline(m[20] ?? m[21]) });
    else if (m[22] != null) out.push({ kind: "link", url: safeWebUrl(m[23]), children: parseInline(m[22]) });
  }
  pushText(text.slice(at));
  return out;
}

function renderMath(target: HTMLElement, math: string, displayMode: boolean) {
  try {
    const html = katex.renderToString(math.trim(), {
      displayMode,
      throwOnError: false,
    });
    if (typeof DOMParser !== "undefined") {
      const doc = new DOMParser().parseFromString(html, "text/html");
      while (doc.body.firstChild) {
        target.append(doc.body.firstChild);
      }
      return;
    }
    target.innerHTML = html;
  } catch {
    target.append(h("span", { class: displayMode ? "md-math-block" : "md-math-inline", text: math.trim() }));
  }
}

function appendInline(into: HTMLElement, nodes: Inline[]) {
  for (const node of nodes) {
    switch (node.kind) {
      case "text":
        into.append(document.createTextNode(node.text));
        break;
      case "code":
        into.append(h("code", { class: "md-code", text: node.text }));
        break;
      case "strong":
      case "em":
      case "u":
      case "del":
      case "mark": {
        const tagMap = {
          strong: "strong",
          em: "em",
          u: "u",
          del: "del",
          mark: "mark",
        } as const;
        const el = h(tagMap[node.kind], { class: `md-${node.kind}` });
        appendInline(el, node.children);
        into.append(el);
        break;
      }
      case "kbd":
        into.append(h("kbd", { class: "md-kbd", text: node.text }));
        break;
      case "sup":
        into.append(h("sup", { class: "md-sup", text: node.text }));
        break;
      case "sub":
        into.append(h("sub", { class: "md-sub", text: node.text }));
        break;
      case "math": {
        const isBlock = !!node.display;
        const el = h("span", { class: isBlock ? "md-math-block" : "md-math-inline" });
        renderMath(el, node.math, isBlock);
        into.append(el);
        break;
      }
      case "link": {
        if (!node.url) {
          appendInline(into, node.children);
          break;
        }
        const url = node.url;
        const a = h("a", { class: "md-link", href: url, title: url });
        a.addEventListener("click", (e) => {
          e.preventDefault();
          void Bridge.openUrl(url);
        });
        appendInline(a, node.children);
        into.append(a);
        break;
      }
    }
  }
}

function inline(text: string, into: HTMLElement) {
  appendInline(into, parseInline(text));
}

// ── Blocks ────────────────────────────────────────────────────────────────────

async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // No clipboard API (or no permission): the old way, through a hidden field.
    const area = h("textarea", { style: "position:fixed;opacity:0" }) as HTMLTextAreaElement;
    area.value = text;
    document.body.append(area);
    area.select();
    document.execCommand("copy");
    area.remove();
  }
}

/** Puts the code on the clipboard; the check mark shows for a moment. */
function copyButton(text: string): HTMLElement {
  const btn = h("button", { class: "md-copy", title: t(STRINGS.copy), "aria-label": t(STRINGS.copy) }, svg(ICONS.copy, 11));
  btn.addEventListener("click", async (e) => {
    e.stopPropagation();
    await writeClipboard(text);
    btn.replaceChildren(svg(ICONS.check, 11, { stroke: 2.2 }));
    btn.classList.add("done");
    btn.title = t(STRINGS.copied);
    window.setTimeout(() => {
      btn.replaceChildren(svg(ICONS.copy, 11));
      btn.classList.remove("done");
      btn.title = t(STRINGS.copy);
    }, 1500);
  });
  return btn;
}

function render(block: Block): HTMLElement {
  switch (block.kind) {
    case "heading": {
      const el = h("div", { class: block.level <= 2 ? "md-h md-h1" : `md-h md-h${block.level}` });
      inline(block.text, el);
      return el;
    }
    case "paragraph": {
      const el = h("div", { class: "md-p" });
      inline(block.text, el);
      return el;
    }
    case "code": {
      const pre = h("pre", { text: block.code });
      const copy = copyButton(block.code);
      if (block.lang) {
        const langBadge = h("span", { class: "md-lang", text: block.lang.toUpperCase() });
        return h("div", { class: "md-pre" }, langBadge, pre, copy);
      }
      return h("div", { class: "md-pre" }, pre, copy);
    }
    case "math": {
      const wrap = h("div", { class: "md-math-wrap" });
      renderMath(wrap, block.math, true);
      return wrap;
    }
    case "table": {
      const thead = h("thead");
      const trHead = h("tr");
      for (let c = 0; c < block.headers.length; c++) {
        const align = block.aligns[c] || "left";
        const th = h("th", { class: `md-th align-${align}` });
        inline(block.headers[c], th);
        trHead.append(th);
      }
      thead.append(trHead);

      const tbody = h("tbody");
      for (const row of block.rows) {
        const tr = h("tr");
        for (let c = 0; c < block.headers.length; c++) {
          const align = block.aligns[c] || "left";
          const td = h("td", { class: `md-td align-${align}` });
          inline(row[c] || "", td);
          tr.append(td);
        }
        tbody.append(tr);
      }

      const table = h("table", { class: "md-table" }, thead, tbody);
      return h("div", { class: "md-table-wrap" }, table);
    }
    case "card": {
      const iconMap: Record<string, string> = {
        note: ICONS.bubble,
        info: ICONS.bubble,
        tip: ICONS.star,
        important: ICONS.bang,
        warning: ICONS.bang,
        caution: ICONS.octagonX,
      };
      const icon = svg(iconMap[block.cardType] || ICONS.bubble, 13);
      const titleSpan = h("span", { class: "md-card-title", text: block.title });
      const header = h("div", { class: "md-card-header" }, icon, titleSpan);
      const body = h("div", { class: "md-card-body" });
      inline(block.text, body);
      return h("div", { class: `md-card md-card-${block.cardType}` }, header, body);
    }
    case "task": {
      const checkMark = h("span", { class: `md-task-check ${block.checked ? "checked" : ""}` }, block.checked ? "✓" : "");
      const label = h("span", { class: `md-task-text ${block.checked ? "done" : ""}` });
      inline(block.text, label);
      const item = h("div", { class: `md-task-item ${block.checked ? "checked" : ""}` }, checkMark, label);
      if (block.indent) item.style.paddingLeft = `${block.indent * 14}px`;
      return item;
    }
    case "item": {
      const text = h("span", { class: "md-p" });
      inline(block.text, text);
      const item = h("div", { class: "md-item" }, h("span", { class: "md-bullet", text: block.prefix }), text);
      if (block.indent) item.style.paddingLeft = `${block.indent * 12}px`;
      return item;
    }
    case "quote": {
      const text = h("span", { class: "md-p" });
      inline(block.text, text);
      return h("div", { class: "md-quote" }, h("i"), text);
    }
    case "rule":
      return h("hr", { class: "md-rule" });
  }
}

/** Replaces what `into` shows with the Markdown rendered. */
export function renderMarkdown(into: HTMLElement, markdown: string) {
  into.replaceChildren(...parseMarkdown(markdown).map(render));
}
