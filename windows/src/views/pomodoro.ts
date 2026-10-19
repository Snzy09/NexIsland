import { h, svg } from "./dom";
import { ICONS } from "./icons";
import { Sound } from "../core/sound";
import type { ViewActions, ViewHost } from "./views";

export type PomodoroMode = "focus" | "short_break" | "long_break";

export interface PomodoroState {
  mode: PomodoroMode;
  secondsRemaining: number;
  totalSeconds: number;
  isRunning: boolean;
  sessionsCompleted: number;
}

export const pomodoroState: PomodoroState = {
  mode: "focus",
  secondsRemaining: 25 * 60,
  totalSeconds: 25 * 60,
  isRunning: false,
  sessionsCompleted: 0,
};

let timerInterval: number | null = null;
const listeners = new Set<() => void>();

export function subscribePomodoro(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function notifyPomodoro() {
  for (const fn of listeners) fn();
}

export function startPomodoro() {
  if (pomodoroState.isRunning) return;
  pomodoroState.isRunning = true;
  Sound.play("blip");
  notifyPomodoro();

  if (timerInterval != null) clearInterval(timerInterval);
  timerInterval = window.setInterval(() => {
    if (!pomodoroState.isRunning) return;
    if (pomodoroState.secondsRemaining > 0) {
      pomodoroState.secondsRemaining--;
      notifyPomodoro();
    } else {
      // Session completed
      pomodoroState.isRunning = false;
      if (timerInterval != null) clearInterval(timerInterval);
      timerInterval = null;
      Sound.play("finish"); // "chime" does not exist in SOUND_NAMES — use "finish"
      if (pomodoroState.mode === "focus") {
        pomodoroState.sessionsCompleted++;
        setPomodoroMode("short_break");
      } else {
        setPomodoroMode("focus");
      }
      notifyPomodoro();
    }
  }, 1000);
}

export function pausePomodoro() {
  if (!pomodoroState.isRunning) return;
  pomodoroState.isRunning = false;
  if (timerInterval != null) {
    clearInterval(timerInterval);
    timerInterval = null;
  }
  Sound.play("blip");
  notifyPomodoro();
}

export function togglePomodoro() {
  if (pomodoroState.isRunning) {
    pausePomodoro();
  } else {
    startPomodoro();
  }
}

export function resetPomodoro() {
  pausePomodoro();
  pomodoroState.secondsRemaining = pomodoroState.totalSeconds;
  notifyPomodoro();
}

export function setPomodoroMode(mode: PomodoroMode) {
  pausePomodoro();
  pomodoroState.mode = mode;
  if (mode === "focus") {
    pomodoroState.totalSeconds = 25 * 60;
  } else if (mode === "short_break") {
    pomodoroState.totalSeconds = 5 * 60;
  } else {
    pomodoroState.totalSeconds = 15 * 60;
  }
  pomodoroState.secondsRemaining = pomodoroState.totalSeconds;
  notifyPomodoro();
}

export function formatTime(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function formatLiveDate(d: Date): string {
  const days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  return `${days[d.getDay()]} ${d.getDate()} ${months[d.getMonth()]} ${d.getFullYear()} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function buildPomodoro(actions: ViewActions): ViewHost {
  // ── DOM elements ──────────────────────────────────────────────────────────
  const dateTimeEl = h("span", { class: "pomo-datetime" });
  const statusRing = h("span", { class: "pomo-status-ring" });
  const headerRow = h(
    "div",
    { class: "pomo-top-bar" },
    h("span", { class: "pomo-header-title", text: "Pomodoro" }),
    h("div", { class: "pomo-date-wrap" }, statusRing, dateTimeEl),
    h("span", { class: "pomo-dots", text: "•••••" }),
  );

  const navBtn = (title: string, icon: string, view: "overview" | "todolist") =>
    h("button", { class: "pomo-nav-btn", title, onclick: () => { actions.blip(); actions.setView(view); } },
      svg(icon, 14));

  const leftNavBtn = navBtn("Previous Card (Overview)", ICONS.chevronLeft, "overview");
  const rightNavBtn = navBtn("Next Card (TodoList)", ICONS.chevronRight, "todolist");

  const timerBadgeIcon = h("div", { class: "pomo-badge" },
    h("div", { class: "pomo-badge-inner" }, svg(ICONS.timer, 30)));

  const timerDigits = h("span", { class: "pomo-digits", text: "25:00" });
  const resetBtn = h("button", { class: "pomo-reset-btn", title: "Reset Timer",
    onclick: () => { actions.blip(); resetPomodoro(); } }, svg(ICONS.arrowClockwise, 14));
  const subtitleEl = h("div", { class: "pomo-subtitle", text: "Focus time" });

  const centerCol = h("div", { class: "pomo-center-col" },
    h("div", { class: "pomo-digits-row" }, timerDigits, resetBtn),
    subtitleEl);

  const middleRow = h("div", { class: "pomo-middle-row" },
    leftNavBtn, timerBadgeIcon, centerCol, rightNavBtn);

  const shortBreakBtn = h("button", { class: "pomo-pill-btn",
    onclick: () => { actions.blip(); setPomodoroMode("short_break"); } },
    h("span", { class: "btn-icon", text: "☕" }), h("span", { text: "Short Break" }));

  const playPauseIcon = h("span", { text: "▶" });
  const playPauseBtn = h("button", { class: "pomo-play-btn",
    onclick: () => { actions.blip(); togglePomodoro(); } }, playPauseIcon);

  const focusTimeBtn = h("button", { class: "pomo-pill-btn active",
    onclick: () => { actions.blip(); setPomodoroMode("focus"); } },
    h("span", { class: "btn-icon", text: "🎯" }), h("span", { text: "Focus Time" }));

  const controlsRow = h("div", { class: "pomo-controls-row" },
    shortBreakBtn, playPauseBtn, focusTimeBtn);

  const paginationRow = h("div", { class: "pomo-pagination" },
    h("span", { class: "pomo-dot", onclick: () => { actions.blip(); actions.setView("overview"); } }),
    h("span", { class: "pomo-dot-pill active" }),
    h("span", { class: "pomo-dot", onclick: () => { actions.blip(); actions.setView("todolist"); } }),
    h("span", { class: "pomo-dot", onclick: () => { actions.blip(); actions.setView("note"); } }),
  );

  const el = h("div", { class: "view pomodoro-view" },
    h("div", { class: "card pomo-card" }, headerRow, middleRow, controlsRow, paginationRow));

  // ── Cached state to avoid redundant DOM writes ────────────────────────────
  let lastDateStr = "";
  let lastTimeStr = "";
  let lastMode: PomodoroMode | null = null;
  let lastRunning: boolean | null = null;
  // Per-second clock interval — only created once the view is first shown
  let clockInterval: number | null = null;

  function syncDisplay() {
    const now = new Date();
    const dateStr = formatLiveDate(now);
    if (dateStr !== lastDateStr) { dateTimeEl.textContent = dateStr; lastDateStr = dateStr; }

    const timeStr = formatTime(pomodoroState.secondsRemaining);
    if (timeStr !== lastTimeStr) { timerDigits.textContent = timeStr; lastTimeStr = timeStr; }

    if (pomodoroState.mode !== lastMode) {
      lastMode = pomodoroState.mode;
      const isFocus = pomodoroState.mode === "focus";
      const isShort = pomodoroState.mode === "short_break";
      subtitleEl.textContent = isFocus ? "Focus time" : isShort ? "Short break" : "Long break";
      shortBreakBtn.classList.toggle("active", isShort);
      focusTimeBtn.classList.toggle("active", isFocus);
    }

    if (pomodoroState.isRunning !== lastRunning) {
      lastRunning = pomodoroState.isRunning;
      playPauseIcon.textContent = pomodoroState.isRunning ? "⏸" : "▶";
      playPauseBtn.classList.toggle("running", pomodoroState.isRunning);
      timerBadgeIcon.classList.toggle("pulsing", pomodoroState.isRunning);
    }
  }

  // Subscribe to pomodoro state changes (timer ticks)
  subscribePomodoro(syncDisplay);
  // Initial render
  syncDisplay();

  return {
    el,
    sync() {
      syncDisplay();
      // Start the date/time clock only once, on first activation
      if (clockInterval == null) {
        clockInterval = window.setInterval(syncDisplay, 1000);
      }
    },
  };
}
