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
      // Completed!
      pomodoroState.isRunning = false;
      if (timerInterval != null) clearInterval(timerInterval);
      timerInterval = null;
      Sound.play("chime");

      if (pomodoroState.mode === "focus") {
        pomodoroState.sessionsCompleted++;
        // Switch to short break
        setPomodoroMode("short_break");
      } else {
        // Switch back to focus
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
  const dayName = days[d.getDay()];
  const dateNum = d.getDate();
  const monthName = months[d.getMonth()];
  const year = d.getFullYear();
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${dayName} ${dateNum} ${monthName} ${year} ${hh}:${mm}`;
}

export function buildPomodoro(actions: ViewActions): ViewHost {
  // Top Header Bar
  const titleEl = h("span", { class: "pomo-header-title", text: "Pomodoro" });
  const statusRing = h("span", { class: "pomo-status-ring" });
  const dateTimeEl = h("span", { class: "pomo-datetime" });
  const dotsEl = h("span", { class: "pomo-dots", text: "•••••" });

  const headerRow = h(
    "div",
    { class: "pomo-top-bar" },
    titleEl,
    h("div", { class: "pomo-date-wrap" }, statusRing, dateTimeEl),
    dotsEl,
  );

  // Left Arrow Button (navigates back to Overview)
  const leftNavBtn = h(
    "button",
    {
      class: "pomo-nav-btn",
      title: "Previous Card (Overview)",
      onclick: () => {
        actions.blip();
        actions.setView("overview");
      },
    },
    svg(ICONS.chevronLeft, 14),
  );

  // Right Arrow Button (navigates to TodoList)
  const rightNavBtn = h(
    "button",
    {
      class: "pomo-nav-btn",
      title: "Next Card (TodoList)",
      onclick: () => {
        actions.blip();
        actions.setView("todolist");
      },
    },
    svg(ICONS.chevronRight, 14),
  );

  // Glowing Timer Badge Icon (Red Circular Ring with Stopwatch)
  const timerBadgeIcon = h(
    "div",
    { class: "pomo-badge" },
    h("div", { class: "pomo-badge-inner" }, svg(ICONS.timer, 30)),
  );

  // Center Timer Digits & Subtitle
  const timerDigits = h("span", { class: "pomo-digits", text: "25:00" });
  const resetBtn = h(
    "button",
    {
      class: "pomo-reset-btn",
      title: "Reset Timer",
      onclick: () => {
        actions.blip();
        resetPomodoro();
      },
    },
    svg(ICONS.arrowClockwise, 14),
  );

  const digitsRow = h("div", { class: "pomo-digits-row" }, timerDigits, resetBtn);
  const subtitleEl = h("div", { class: "pomo-subtitle", text: "Focus time" });

  const centerCol = h("div", { class: "pomo-center-col" }, digitsRow, subtitleEl);

  // Middle Row
  const middleRow = h(
    "div",
    { class: "pomo-middle-row" },
    leftNavBtn,
    timerBadgeIcon,
    centerCol,
    rightNavBtn,
  );

  // Control Buttons (Bottom Row)
  const shortBreakBtn = h(
    "button",
    {
      class: "pomo-pill-btn",
      onclick: () => {
        actions.blip();
        setPomodoroMode("short_break");
      },
    },
    h("span", { class: "btn-icon", text: "☕" }),
    h("span", { text: "Short Break" }),
  );

  const playPauseIcon = h("span", { text: "▶" });
  const playPauseBtn = h(
    "button",
    {
      class: "pomo-play-btn",
      onclick: () => {
        actions.blip();
        togglePomodoro();
      },
    },
    playPauseIcon,
  );

  const focusTimeBtn = h(
    "button",
    {
      class: "pomo-pill-btn active",
      onclick: () => {
        actions.blip();
        setPomodoroMode("focus");
      },
    },
    h("span", { class: "btn-icon", text: "🎯" }),
    h("span", { text: "Focus Time" }),
  );

  const controlsRow = h(
    "div",
    { class: "pomo-controls-row" },
    shortBreakBtn,
    playPauseBtn,
    focusTimeBtn,
  );

  // Bottom Pagination Dots: • ▬ • • (Slide 1 active)
  const dot0 = h("span", { class: "pomo-dot", onclick: () => { actions.blip(); actions.setView("overview"); } });
  const dot1 = h("span", { class: "pomo-dot-pill active" });
  const dot2 = h("span", { class: "pomo-dot", onclick: () => { actions.blip(); actions.setView("todolist"); } });
  const dot3 = h("span", { class: "pomo-dot", onclick: () => { actions.blip(); actions.setView("note"); } });

  const paginationRow = h(
    "div",
    { class: "pomo-pagination" },
    dot0,
    dot1,
    dot2,
    dot3,
  );

  // Outer Container
  const el = h(
    "div",
    { class: "view pomodoro-view" },
    h(
      "div",
      { class: "card pomo-card" },
      headerRow,
      middleRow,
      controlsRow,
      paginationRow,
    ),
  );

  function sync() {
    // Update live date-time
    const now = new Date();
    dateTimeEl.textContent = formatLiveDate(now);

    // Update time digits
    timerDigits.textContent = formatTime(pomodoroState.secondsRemaining);

    // Update subtitle
    if (pomodoroState.mode === "focus") {
      subtitleEl.textContent = "Focus time";
      shortBreakBtn.classList.remove("active");
      focusTimeBtn.classList.add("active");
    } else if (pomodoroState.mode === "short_break") {
      subtitleEl.textContent = "Short break";
      shortBreakBtn.classList.add("active");
      focusTimeBtn.classList.remove("active");
    } else {
      subtitleEl.textContent = "Long break";
      shortBreakBtn.classList.remove("active");
      focusTimeBtn.classList.remove("active");
    }

    // Update play/pause button
    if (pomodoroState.isRunning) {
      playPauseIcon.textContent = "⏸";
      playPauseBtn.classList.add("running");
      timerBadgeIcon.classList.add("pulsing");
    } else {
      playPauseIcon.textContent = "▶";
      playPauseBtn.classList.remove("running");
      timerBadgeIcon.classList.remove("pulsing");
    }
  }

  // Subscribe to pomodoro changes
  subscribePomodoro(sync);

  // Keep date-time updated every second
  window.setInterval(sync, 1000);

  // Initial render
  sync();

  return {
    el,
    sync,
  };
}
