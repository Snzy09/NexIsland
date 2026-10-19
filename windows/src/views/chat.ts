// Chat view — DOM port of PromptView / ChatBubble / TypingDotsView /
// ModelPickerView from IslandViewContent.swift.
//
// Answers are rendered as Markdown (markdown.ts). A local model streams its
// answer: Rust sends `chat-delta` events with the text visible so far. The
// model name above the text field opens the picker: provider chips, then the
// models of the chosen provider, asked for only once it is picked.

import { h, svg, clear } from "./dom";
import { ICONS } from "./icons";
import { renderMarkdown } from "./markdown";
import { Bridge, onEvent, type ChatContext, type ModelInfo, type Skill } from "../core/bridge";
import {
  activeModel, pickModel, providerDef, visibleProviders, withModel, type ProviderDef,
} from "../core/providers";
import { Sound } from "../core/sound";
import { State, type ChatMessage, type ChatStep } from "../core/state";
import type { ViewHost } from "./views";
import { N_, t, tl } from "../i18n/i18n";

const STRINGS = {
  placeholderFirst: N_("Ask me anything…"),
  placeholderNext: N_("Continue…"),
  send: N_("Send"),
  stop: N_("Stop"),
  switchModel: N_("Switch provider or model"),
  noModel: N_("Choose a model"),
  loading: N_("Loading models…"),
  noKey: N_("No API key — add it in Settings."),
  openSettings: N_("Open Settings"),
};

let nextId = 1;

function stopIcon(): HTMLElement {
  return h("span", { class: "stop-icon" });
}

function formatErrorMessage(errStr: string, model: string, provider: string): string {
  const cleanErr = errStr.replace(/^Error:\s*/i, "").trim();
  let explanation = "";

  const lower = cleanErr.toLowerCase();
  if (lower.includes("500") || lower.includes("internal server error") || lower.includes("kesalahan internal")) {
    explanation = `Server ${provider} mengembalikan **Internal Server Error (500)**.\nKemungkinan model \`${model}\` tidak aktif, tidak kompatibel dengan fungsi/alat yang diminta, atau endpoint sedang bermasalah. Coba gunakan model lain (seperti \`auto\` atau model resmi lainnya) melalui pemilih model di kanan bawah.`;
  } else if (lower.includes("404") || lower.includes("not found") || lower.includes("isn't installed")) {
    explanation = `Model \`${model}\` tidak ditemukan (404) pada endpoint server ${provider}. Pastikan nama model sudah tepat atau pilih dari daftar model yang tersedia di menu model.`;
  } else if (lower.includes("401") || lower.includes("403") || lower.includes("unauthorized") || lower.includes("key missing") || lower.includes("rejected the api key")) {
    explanation = `Akses ditolak (401/403). Kunci API (API Key) untuk ${provider} belum disetel atau tidak valid. Silakan periksa kunci API Anda di **Pengaturan → Chat**.`;
  } else if (lower.includes("429") || lower.includes("rate limit") || lower.includes("credits") || lower.includes("quota")) {
    explanation = `Batas kuota atau frekuensi pemanggilan API (Rate Limit) telah tercapai. Tunggu beberapa saat atau periksa kuota akun API Anda.`;
  } else if (lower.includes("cannot reach") || lower.includes("network error") || lower.includes("unreachable") || lower.includes("connection refused") || lower.includes("timed out")) {
    explanation = `Tidak dapat terhubung ke server ${provider}. Pastikan perangkat terhubung ke internet dan URL endpoint di Pengaturan sudah benar dan aktif.`;
  } else if (lower.includes("tool execution error") || lower.includes("mcp tool error")) {
    explanation = `Gagal mengeksekusi alat (tool) di komputer. Periksa izin eksekusi atau parameter perintah yang diminta oleh AI.`;
  } else if (lower.includes("skill") && lower.includes("not found")) {
    explanation = "Skill yang diminta tidak ditemukan. Gunakan `/skills` untuk melihat daftar skill yang terpasang di folder Coucou.";
  } else {
    explanation = `Model AI tidak dapat memberikan respon (\`${cleanErr || "No response"}\`). Coba periksa pengaturan koneksi atau ajukan kembali pertanyaan Anda.`;
  }

  return `> ⚠️ **Error Respon AI**\n>\n> \`${cleanErr || "Error: Unknown"}\`\n\n💡 **Penjelasan:**\n${explanation}`;
}

function renderStepsWidget(steps: ChatStep[], active = false): HTMLElement {
  const isInterrupted = steps.some((s) => s.status === "interrupted");
  const spinner = active ? h("div", { class: "chat-steps-spinner" }) : null;
  const count = steps.length;
  const defaultTitle = active
    ? "Thinking…"
    : isInterrupted
      ? "Interrupted"
      : count === 1
        ? steps[0].action
        : `${count} steps completed`;

  const titleText = h("span", {
    class: "chat-steps-title-text",
    text: defaultTitle,
    style: isInterrupted ? "color:#f87171" : "",
  });
  const toggleBtn = h("span", { class: "chat-steps-toggle", text: active ? "▾" : "▸" });
  const header = h(
    "div",
    { class: "chat-steps-header" },
    h("div", { class: "chat-steps-title" }, spinner, titleText),
    toggleBtn,
  );

  const list = h("div", { class: active ? "chat-steps-list" : "chat-steps-list collapsed" });
  for (const s of steps) {
    const icon = h("span", { class: "chat-step-item-icon" });
    const text = h("span", { text: s.action });
    const item = h(
      "div",
      { class: s.status === "running" ? "chat-step-item running" : "chat-step-item done" },
      icon,
      text,
    );
    list.append(item);
  }

  const el = h("div", { class: "chat-steps" }, header, list);

  let isCollapsed = !active;
  header.addEventListener("click", () => {
    isCollapsed = !isCollapsed;
    list.classList.toggle("collapsed", isCollapsed);
    toggleBtn.textContent = isCollapsed ? "▸" : "▾";
  });

  return el;
}

function bubble(message: ChatMessage): HTMLElement {
  if (message.role === "user") {
    return h(
      "div",
      { class: "chat-row user" },
      h("div", { class: "bubble", text: message.content }),
    );
  }
  const reply = h("div", { class: "reply" });
  renderMarkdown(reply, message.content);
  if (message.steps && message.steps.length > 0) {
    const stepsWidget = renderStepsWidget(message.steps, false);
    return h("div", { class: "chat-row assistant-with-steps" }, stepsWidget, reply);
  }
  return h("div", { class: "chat-row" }, reply);
}

function typingDots(): HTMLElement {
  return h(
    "div",
    { class: "chat-row" },
    h("div", { class: "typing" }, h("i"), h("i"), h("i")),
  );
}

/** The coloured chip showing what the question is about (a dropped file). */
function contextChip(label: string): HTMLElement {
  const chip = h("div", { class: "chip" }, h("i", { class: "chip-dot" }), h("span", { text: label }));
  requestAnimationFrame(() => chip.classList.add("settled"));
  return chip;
}

function saveSettings() {
  void Bridge.saveSettings(State.settings);
}

// ── Model picker ──────────────────────────────────────────────────────────────

interface Picker {
  el: HTMLElement;
  open(): void;
  close(): void;
  readonly isOpen: boolean;
}

/** Provider chips, then the chosen provider's models — ModelPickerView. */
function buildPicker(onChange: () => void): Picker {
  const chips = h("div", { class: "picker-chips" });
  const list = h("div", { class: "picker-list" });
  const el = h("div", { class: "picker" }, chips, h("div", { class: "picker-rule" }), list);

  /** Models already asked for, by provider; a model server is asked again each time. */
  const cache = new Map<string, ModelInfo[]>();
  let isOpen = false;
  let request = 0;

  function drawChips() {
    clear(chips);
    for (const p of visibleProviders(State.settings)) {
      const on = p.id === State.settings.chatProvider;
      const chip = h(
        "button",
        { class: on ? "picker-chip on" : "picker-chip", style: `--accent:${p.accent}` },
        h("i", { class: "picker-dot" }),
        h("span", { text: t(p.name) }),
      );
      chip.addEventListener("click", () => {
        if (p.id === State.settings.chatProvider) return;
        State.settings = { ...State.settings, chatProvider: p.id };
        saveSettings();
        Sound.play("pop");
        drawChips();
        void loadModels();
        onChange();
      });
      chips.append(chip);
    }
  }

  function status(text: string, withSettings = false) {
    clear(list);
    const line = h("div", { class: "picker-status", text });
    if (withSettings) {
      line.append(
        h("button", {
          class: "picker-link",
          text: tl(STRINGS.openSettings),
          onclick: () => void Bridge.openSettingsWindow(),
        }),
      );
    }
    list.append(line);
  }

  function drawModels(p: ProviderDef, models: ModelInfo[]) {
    clear(list);
    const current = activeModel(State.settings);
    for (const m of models) {
      const on = m.id === current;
      const row = h(
        "button",
        { class: on ? "picker-model on" : "picker-model", style: `--accent:${p.accent}`, title: m.id },
        h("span", { class: "picker-model-name", text: m.label }),
        on ? svg(ICONS.check, 11, { stroke: 2.2 }) : null,
      );
      row.addEventListener("click", () => {
        State.settings = withModel(State.settings, p.id, m.id);
        saveSettings();
        Sound.play("blip");
        close();
      });
      list.append(row);
    }
    list.querySelector(".picker-model.on")?.scrollIntoView({ block: "nearest" });
  }

  async function loadModels() {
    const p = providerDef(State.settings.chatProvider);
    const ticket = ++request;
    const cached = p.urlField ? undefined : cache.get(p.id);
    if (cached) {
      drawModels(p, cached);
      return;
    }
    // Nothing is asked of a provider that has no key yet.
    if (p.key && !(await Bridge.secretPresent(p.key))) {
      if (ticket === request) status(t(STRINGS.noKey), true);
      return;
    }
    if (ticket !== request) return;
    status(t(STRINGS.loading));
    try {
      const models = await Bridge.chatModels(p.id);
      if (ticket !== request) return;
      if (!p.urlField) cache.set(p.id, models);
      const keep = pickModel(p, models.map((m) => m.id), activeModel(State.settings));
      if (keep && keep !== activeModel(State.settings)) {
        State.settings = withModel(State.settings, p.id, keep);
        saveSettings();
        onChange();
      }
      drawModels(p, models);
    } catch (err) {
      if (ticket === request) status(String(err).replace(/^Error:\s*/, ""), Boolean(p.key));
    }
  }

  function open() {
    isOpen = true;
    el.classList.add("on");
    drawChips();
    onChange();
    void loadModels();
  }

  function close() {
    isOpen = false;
    request++;
    el.classList.remove("on");
    onChange();
  }

  return {
    el,
    open,
    close,
    get isOpen() {
      return isOpen;
    },
  };
}

// ── Slash commands ────────────────────────────────────────────────────────────

interface SlashItem {
  name: string;
  insertText: string;
  badge: string;
  description: string;
  immediate: boolean;
  icon: string;
}

const DEFAULT_SLASH_COMMANDS: SlashItem[] = [
  {
    name: "/antigravity",
    insertText: "/antigravity ",
    badge: t("Agent"),
    description: "Dispatch task/prompt to Google Antigravity Agent",
    immediate: false,
    icon: "🪐",
  },
  {
    name: "/agy",
    insertText: "/agy ",
    badge: t("Agent"),
    description: "Quick dispatch prompt to active Antigravity session",
    immediate: false,
    icon: "🪐",
  },
  {
    name: "/providers",
    insertText: "/providers",
    badge: "Providers",
    description: "List integrated AI providers (Antigravity, Gemini, Claude, etc.)",
    immediate: true,
    icon: "🌐",
  },
  {
    name: "/prompt",
    insertText: "/prompt ",
    badge: "Provider",
    description: "Direct prompt to specific provider (/prompt <provider> <text>)",
    immediate: false,
    icon: "🎯",
  },
  {
    name: "/tools",
    insertText: "/tools",
    badge: "Device",
    description: "Status of terminal, web, file and system tools",
    immediate: true,
    icon: "⚡",
  },
  {
    name: "/tools on",
    insertText: "/tools on",
    badge: "Device",
    description: "Enable all built-in device execution tools",
    immediate: true,
    icon: "🟢",
  },
  {
    name: "/tools off",
    insertText: "/tools off",
    badge: "Device",
    description: "Disable device execution tools (safe mode)",
    immediate: true,
    icon: "🔴",
  },
  {
    name: "/skills",
    insertText: "/skills",
    badge: "Skills",
    description: "List all prompt skills and instructions",
    immediate: true,
    icon: "🛠️",
  },
  {
    name: "/skill",
    insertText: "/skill ",
    badge: "Skills",
    description: "Run prompt with specific skill (/skill <name> <prompt>)",
    immediate: false,
    icon: "✨",
  },
  {
    name: "/mcp",
    insertText: "/mcp",
    badge: "MCP",
    description: "Inspect active MCP tools and servers",
    immediate: true,
    icon: "🔌",
  },
  {
    name: "/mcp library",
    insertText: "/mcp library",
    badge: "MCP",
    description: "Browse curated MCP registry & online catalog",
    immediate: true,
    icon: "📚",
  },
  {
    name: "/mcp search",
    insertText: "/mcp search ",
    badge: "MCP",
    description: "Search MCP library & npm registry (/mcp search <query>)",
    immediate: false,
    icon: "🔍",
  },
  {
    name: "/mcp install",
    insertText: "/mcp install ",
    badge: "MCP",
    description: "Install server from MCP library (/mcp install <name>)",
    immediate: false,
    icon: "📦",
  },
  {
    name: "/mcp uninstall",
    insertText: "/mcp uninstall ",
    badge: "MCP",
    description: "Remove MCP server (/mcp uninstall <name>)",
    immediate: false,
    icon: "🗑️",
  },
  {
    name: "/status",
    insertText: "/status",
    badge: t("Agent"),
    description: "Live agent status (Antigravity, Cursor, etc.)",
    immediate: true,
    icon: "🤖",
  },
  {
    name: "/clear",
    insertText: "/clear",
    badge: "Reset",
    description: "Clear chat messages and reset session",
    immediate: true,
    icon: "🧹",
  },
  {
    name: "/help",
    insertText: "/help",
    badge: "Help",
    description: "Show available slash commands guide",
    immediate: true,
    icon: "💡",
  },
];

interface LiveStepsComponent {
  el: HTMLElement;
  addStep(action: string, status: "running" | "done"): void;
  finishCurrentStep(): void;
  setTitle(title: string): void;
  done(summary?: string): void;
  interrupt(): void;
  getSteps(): ChatStep[];
}

function createLiveStepsComponent(): LiveStepsComponent {
  const steps: ChatStep[] = [];
  const spinner = h("div", { class: "chat-steps-spinner" });
  const titleText = h("span", { class: "chat-steps-title-text", text: "Thinking…" });
  const toggleBtn = h("span", { class: "chat-steps-toggle", text: "▾" });
  const header = h(
    "div",
    { class: "chat-steps-header" },
    h("div", { class: "chat-steps-title" }, spinner, titleText),
    toggleBtn,
  );
  const list = h("div", { class: "chat-steps-list" });
  const el = h("div", { class: "chat-steps" }, header, list);

  let isCollapsed = false;
  header.addEventListener("click", () => {
    isCollapsed = !isCollapsed;
    list.classList.toggle("collapsed", isCollapsed);
    toggleBtn.textContent = isCollapsed ? "▸" : "▾";
  });

  let activeItem: HTMLElement | null = null;

  return {
    el,
    getSteps: () => [...steps],
    setTitle(title: string) {
      titleText.textContent = title;
    },
    addStep(action: string, status: "running" | "done") {
      titleText.textContent = action;
      if (activeItem) {
        activeItem.classList.remove("running");
        activeItem.classList.add("done");
      }
      if (steps.length > 0 && steps[steps.length - 1].status === "running") {
        steps[steps.length - 1].status = "done";
      }
      steps.push({ action, status });
      const icon = h("span", { class: "chat-step-item-icon" });
      const text = h("span", { text: action });
      const item = h(
        "div",
        { class: status === "running" ? "chat-step-item running" : "chat-step-item done" },
        icon,
        text,
      );
      list.append(item);
      if (status === "running") {
        activeItem = item;
      } else {
        activeItem = null;
      }
    },
    finishCurrentStep() {
      if (activeItem) {
        activeItem.classList.remove("running");
        activeItem.classList.add("done");
        activeItem = null;
      }
      if (steps.length > 0 && steps[steps.length - 1].status === "running") {
        steps[steps.length - 1].status = "done";
      }
    },
    done(summary?: string) {
      this.finishCurrentStep();
      spinner.style.display = "none";
      if (summary) {
        titleText.textContent = summary;
      } else if (steps.length > 0) {
        titleText.textContent = `${steps.length} steps completed`;
      }
    },
    interrupt() {
      this.finishCurrentStep();
      spinner.style.display = "none";
      titleText.textContent = "Interrupted";
      titleText.style.color = "#f87171";
      if (steps.length > 0) {
        steps[steps.length - 1].status = "interrupted";
      } else {
        steps.push({ action: "Interrupted", status: "interrupted" });
      }
    },
  };
}

// ── View ──────────────────────────────────────────────────────────────────────

export function buildPrompt(onHeightChange: () => void): ViewHost {
  const chipRow = h("div", { class: "chip-row" });
  const log = h("div", { class: "chat-log" });
  const input = h("input", {
    type: "text",
    class: "chat-input",
    placeholder: t(STRINGS.placeholderFirst),
    spellcheck: "false",
  }) as HTMLInputElement;
  const send = h("button", { class: "send-btn", title: tl(STRINGS.send) }, svg(ICONS.arrowUp, 11));
  const bar = h("div", { class: "chat-bar" }, input, send);

  const slashMenu = h("div", { class: "slash-menu" });
  slashMenu.style.display = "none";
  const barWrap = h("div", { class: "chat-bar-wrap" }, slashMenu, bar);

  let customSkills: Skill[] = [];
  let skillsRequested = false;

  let selectedSlashIndex = 0;
  let currentSlashMatches: SlashItem[] = [];

  function hideSlashMenu() {
    slashMenu.style.display = "none";
    currentSlashMatches = [];
    selectedSlashIndex = 0;
  }

  function pickSlashItem(item: SlashItem) {
    input.value = item.insertText;
    hideSlashMenu();
    if (item.immediate) {
      void submit();
    } else {
      input.focus();
      const len = input.value.length;
      input.setSelectionRange(len, len);
    }
  }

  function scrollActiveIntoView() {
    const activeEl = slashMenu.querySelector<HTMLElement>(".slash-item.active");
    activeEl?.scrollIntoView({ block: "nearest" });
  }

  function renderSlashHighlight() {
    const rows = slashMenu.querySelectorAll<HTMLElement>(".slash-item");
    rows.forEach((row, idx) => {
      row.classList.toggle("active", idx === selectedSlashIndex);
    });
    scrollActiveIntoView();
  }

  function renderSlashItems() {
    clear(slashMenu);

    const header = h(
      "div",
      { class: "slash-menu-header" },
      h("span", { text: "Slash Commands" }),
      h("span", { class: "slash-menu-hint", text: "↑↓ Navigate  •  ↵ / Tab Select  •  Esc Close" }),
    );
    slashMenu.append(header);

    currentSlashMatches.forEach((item, index) => {
      const isActive = index === selectedSlashIndex;
      const row = h(
        "button",
        {
          class: isActive ? "slash-item active" : "slash-item",
          type: "button",
        },
        h("span", { class: "slash-item-icon", text: item.icon }),
        h("span", { class: "slash-item-name", text: item.name }),
        h("span", { class: "slash-item-badge", text: item.badge }),
        h("span", { class: "slash-item-desc", text: item.description }),
        h("span", { class: "slash-item-key", text: item.immediate ? "↵" : "Tab" }),
      );

      row.addEventListener("mouseenter", () => {
        selectedSlashIndex = index;
        renderSlashHighlight();
      });

      row.addEventListener("mousedown", (e) => {
        e.preventDefault();
      });

      row.addEventListener("click", (e) => {
        e.preventDefault();
        pickSlashItem(item);
      });

      slashMenu.append(row);
    });

    scrollActiveIntoView();
  }

  function updateSlashMenu() {
    const text = input.value;
    if (!text.startsWith("/")) {
      hideSlashMenu();
      return;
    }

    if (text.startsWith("/sk") && !skillsRequested) {
      skillsRequested = true;
      void Bridge.skillsList().then((list) => {
        if (Array.isArray(list) && list.length > 0) {
          customSkills = list;
          if (input.value.startsWith("/sk")) updateSlashMenu();
        }
      }).catch(() => {});
    }

    const query = text.toLowerCase().trim();
    const all = [...DEFAULT_SLASH_COMMANDS];
    for (const s of customSkills) {
      all.push({
        name: `/skill ${s.id}`,
        insertText: `/skill ${s.id} `,
        badge: "Skill",
        description: s.description || s.name,
        immediate: false,
        icon: "✨",
      });
    }

    currentSlashMatches = all.filter((cmd) => {
      if (query === "/") return true;
      return cmd.name.toLowerCase().startsWith(query) || cmd.name.toLowerCase().includes(query);
    });

    if (currentSlashMatches.length === 0) {
      hideSlashMenu();
      return;
    }

    if (selectedSlashIndex >= currentSlashMatches.length) {
      selectedSlashIndex = 0;
    }

    renderSlashItems();
    slashMenu.style.display = "flex";
  }

  const modelDot = h("i", { class: "model-dot" });
  const modelName = h("span", { class: "model-name" });
  const modelBtn = h(
    "button",
    { class: "model-btn", title: tl(STRINGS.switchModel) },
    modelDot,
    modelName,
    svg(ICONS.chevronUpDown, 9, { stroke: 2 }),
  );
  const modelRow = h("div", { class: "model-row" }, modelBtn);

  const body = h("div", { class: "chat-body" });
  const picker = buildPicker(() => {
    body.classList.toggle("picking", picker.isOpen);
    drawModelButton();
  });
  body.append(chipRow, log, picker.el, modelRow, barWrap);

  const el = h("div", { class: "view" }, h("div", { class: "card wash chat-card" }, body));
  (el.querySelector(".card") as HTMLElement).style.setProperty("--wash", "rgba(99,102,241,0.5)");

  let sending = false;
  let renderedCount = -1;
  // A local model answers token by token: where its text so far is shown.
  let live: HTMLElement | null = null;
  let currentSteps: LiveStepsComponent | null = null;
  let wasInterrupted = false;

  function activeSteps(): LiveStepsComponent | null {
    return currentSteps;
  }

  function setSendButtonMode(isStopping: boolean) {
    clear(send);
    if (isStopping) {
      send.classList.add("stopping");
      send.title = t(STRINGS.stop);
      send.append(stopIcon());
    } else {
      send.classList.remove("stopping");
      send.title = t(STRINGS.send);
      send.append(svg(ICONS.arrowUp, 11));
    }
  }

  function interrupt() {
    if (!sending) return;
    wasInterrupted = true;
    void Bridge.chatCancel();
    currentSteps?.interrupt();
    setSendButtonMode(false);
    sending = false;
    State.stateOverride = null;
    drawModelButton();

    if (live) {
      const notice = h("div", { class: "chat-interrupted", text: " (Interrupted)" });
      live.append(notice);
      State.chatHistory.push({
        id: nextId++,
        role: "assistant",
        content: (live.textContent || "").trim() || "*(Interrupted)*",
        steps: currentSteps ? currentSteps.getSteps() : undefined,
      });
      live = null;
    } else {
      State.chatHistory.push({
        id: nextId++,
        role: "assistant",
        content: "*(Interrupted)*",
        steps: currentSteps ? currentSteps.getSteps() : undefined,
      });
    }

    currentSteps = null;
    renderedCount = -1;
    State.notify();
    onHeightChange();
    input.disabled = false;
    input.focus();
  }

  function drawModelButton() {
    const p = providerDef(State.settings?.chatProvider || "anthropic");
    modelDot.style.background = p?.accent || "#E07950";
    modelName.textContent = activeModel(State.settings) || t(STRINGS.noModel);
    modelBtn.classList.toggle("open", picker.isOpen);
    modelBtn.disabled = sending;
  }

  modelBtn.addEventListener("click", () => {
    if (sending) return;
    if (picker.isOpen) picker.close();
    else {
      hideSlashMenu();
      picker.open();
    }
  });

  interface StatusEvent {
    action: string;
    status: string;
    tool?: string;
  }

  void onEvent<StatusEvent>("chat-status", (data) => {
    if (!sending) return;
    if (!currentSteps) {
      currentSteps = createLiveStepsComponent();
      const typing = log.querySelector(".typing")?.parentElement;
      if (typing) {
        log.insertBefore(currentSteps.el, typing);
      } else {
        log.append(currentSteps.el);
      }
    }

    if (data.status === "thinking") {
      currentSteps.setTitle(data.action || "Thinking…");
    } else if (data.status === "running") {
      currentSteps.addStep(data.action, "running");
    } else if (data.status === "done") {
      if (!data.tool) {
        currentSteps.done();
      } else {
        currentSteps.finishCurrentStep();
      }
    } else if (data.status === "interrupted") {
      currentSteps.interrupt();
    }

    log.scrollTop = log.scrollHeight;
    onHeightChange();
  });

  void onEvent<string>("chat-delta", (text) => {
    if (!sending || !text) return; // nothing visible yet: the dots stay
    if (!live) {
      live = h("div", { class: "reply" });
      log.querySelector(".typing")?.parentElement?.remove();
      log.append(h("div", { class: "chat-row" }, live));
    }
    renderMarkdown(live, text);
    log.scrollTop = log.scrollHeight;
  });

  async function submit() {
    const query = input.value.trim();
    if (!query || sending) return;
    if (picker.isOpen) picker.close();
    hideSlashMenu();
    input.value = "";

    if (query === "/clear") {
      State.chatHistory = [];
      renderedCount = -1;
      clear(log);
      currentSteps = null;
      sending = false;
      setSendButtonMode(false);
      drawModelButton();
      Sound.play("blip");
      void Bridge.chatReset().catch(() => {});
      State.notify();
      onHeightChange();
      return;
    }

    sending = true;
    wasInterrupted = false;
    currentSteps = null;
    setSendButtonMode(true);
    drawModelButton();
    Sound.play("send");

    State.chatHistory.push({ id: nextId++, role: "user", content: query });
    State.stateOverride = "thinking";
    State.notify();
    onHeightChange();

    const file = State.droppedFile;
    const context: ChatContext | null =
      State.chatHistory.length === 1 && file ? { kind: "file", name: file.name, path: file.path } : null;

    try {
      const reply = await Bridge.chatSend(query, context);
      if (!wasInterrupted) {
        activeSteps()?.done();
        const steps = activeSteps() ? activeSteps()!.getSteps() : undefined;
        State.chatHistory.push({
          id: nextId++,
          role: "assistant",
          content: reply.text,
          steps: steps && steps.length > 0 ? steps : undefined,
        });
        State.stateOverride = null;
        Sound.play("finish");
      }
    } catch (err) {
      State.stateOverride = null;
      const errStr = String(err);
      if (errStr.includes("Chat interrupted") || wasInterrupted) {
        activeSteps()?.interrupt();
      } else {
        activeSteps()?.done();
        Sound.play("error");

        const curModel = activeModel(State.settings) || "AI";
        const curProvider = providerDef(State.settings.chatProvider)?.name || State.settings.chatProvider || "Server";
        const errorContent = formatErrorMessage(errStr, curModel, curProvider);

        const partial = (live?.textContent || "").trim();
        const content = partial ? `${partial}\n\n${errorContent}` : errorContent;

        State.chatHistory.push({
          id: nextId++,
          role: "assistant",
          content,
          steps: activeSteps() ? activeSteps()!.getSteps() : undefined,
        });
      }
    } finally {
      sending = false;
      live = null;
      currentSteps = null;
      renderedCount = -1; // the finished answer replaces the streamed one
      setSendButtonMode(false);
      drawModelButton();
      State.notify();
      onHeightChange();
      input.focus();
    }
  }

  send.addEventListener("click", () => {
    if (sending) {
      interrupt();
    } else {
      void submit();
    }
  });

  input.addEventListener("input", () => {
    updateSlashMenu();
  });

  input.addEventListener("blur", () => {
    setTimeout(() => {
      hideSlashMenu();
    }, 150);
  });

  input.addEventListener("keydown", (e) => {
    const key = (e as KeyboardEvent).key;
    const isMenuOpen = slashMenu.style.display !== "none" && currentSlashMatches.length > 0;

    if (isMenuOpen) {
      if (key === "ArrowDown") {
        e.preventDefault();
        selectedSlashIndex = (selectedSlashIndex + 1) % currentSlashMatches.length;
        renderSlashHighlight();
        return;
      }
      if (key === "ArrowUp") {
        e.preventDefault();
        selectedSlashIndex = (selectedSlashIndex - 1 + currentSlashMatches.length) % currentSlashMatches.length;
        renderSlashHighlight();
        return;
      }
      if (key === "Tab") {
        e.preventDefault();
        const item = currentSlashMatches[selectedSlashIndex];
        if (item) pickSlashItem(item);
        return;
      }
      if (key === "Enter") {
        e.preventDefault();
        const item = currentSlashMatches[selectedSlashIndex];
        if (item) {
          pickSlashItem(item);
        } else {
          void submit();
        }
        return;
      }
      if (key === "Escape") {
        e.preventDefault();
        hideSlashMenu();
        return;
      }
    }

    if (key === "Escape") {
      if (sending) {
        e.preventDefault();
        e.stopPropagation();
        interrupt();
        return;
      }
      if (picker.isOpen) {
        e.preventDefault();
        picker.close();
        return;
      }
    }

    if (key === "Enter") {
      e.preventDefault();
      void submit();
    }
    e.stopPropagation(); // Escape closes the island, not the chat
  });

  el.addEventListener("keydown", (e) => {
    if ((e as KeyboardEvent).key === "Escape" && sending) {
      (e as KeyboardEvent).preventDefault();
      (e as KeyboardEvent).stopPropagation();
      interrupt();
    }
  });

  return {
    el,
    sync() {
      const file = State.droppedFile;
      const wantChip = file?.name ?? "";
      if (chipRow.dataset.label !== wantChip) {
        chipRow.dataset.label = wantChip;
        clear(chipRow);
        if (wantChip) chipRow.append(contextChip(wantChip));
      }

      const thinking = State.stateOverride === "thinking";
      const count = State.chatHistory.length + (thinking ? 0.5 : 0);
      if (count !== renderedCount && !live) {
        renderedCount = count;
        clear(log);
        for (const m of State.chatHistory) log.append(bubble(m));
        if (currentSteps?.el) {
          log.append(currentSteps.el);
        }
        if (thinking) log.append(typingDots());
        log.scrollTop = log.scrollHeight;
      }

      // Leaving the chat folds the picker away.
      if (State.view !== "prompt") {
        if (picker.isOpen) picker.close();
        hideSlashMenu();
      }
      drawModelButton();

      input.placeholder = t(State.chatHistory.length === 0 ? STRINGS.placeholderFirst : STRINGS.placeholderNext);
      input.disabled = sending;
    },
    focus() {
      input.focus();
      input.select();
    },
  };
}
