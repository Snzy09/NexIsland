import { h, svg } from "./dom";
import { ICONS } from "./icons";
import { Sound } from "../core/sound";
import { formatLiveDate } from "./pomodoro";
import type { ViewActions, ViewHost } from "./views";

export interface TodoItem {
  id: string;
  text: string;
  completed: boolean;
  createdAt: number;
}

const STORAGE_KEY = "nexisland_todos_v1";

const DEFAULT_TODOS: TodoItem[] = [
  { id: "1", text: "Revise the report", completed: true, createdAt: Date.now() - 40000 },
  { id: "2", text: "Reply to Dina", completed: true, createdAt: Date.now() - 30000 },
  { id: "3", text: "Post weekly update", completed: true, createdAt: Date.now() - 20000 },
  { id: "4", text: "Review AI code changes", completed: true, createdAt: Date.now() - 10000 },
  { id: "5", text: "Test Pomodoro & Dynamic Island", completed: false, createdAt: Date.now() },
];

export function loadTodos(): TodoItem[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as unknown;
      if (Array.isArray(parsed) && parsed.length > 0) return parsed as TodoItem[];
    }
  } catch { /* ignore */ }
  return [...DEFAULT_TODOS];
}

export function saveTodos(todos: TodoItem[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(todos));
  } catch { /* ignore */ }
}

export function buildTodoList(actions: ViewActions): ViewHost {
  let todos: TodoItem[] = loadTodos();
  // Track list version so we don't rebuild DOM unless data actually changed
  let lastTodosJson = "";
  let lastDateStr = "";
  let clockInterval: number | null = null;

  // ── DOM elements ──────────────────────────────────────────────────────────
  const dateTimeEl = h("span", { class: "pomo-datetime" });

  const headerRow = h(
    "div",
    { class: "pomo-top-bar" },
    h("div", { class: "pomo-header-title todo-title" },
      h("span", { text: "Todo" }),
      h("span", { style: "color:#22c55e", text: "List" }),
    ),
    h("div", { class: "pomo-date-wrap" },
      h("span", { class: "pomo-status-ring" }),
      dateTimeEl,
    ),
    h("span", { class: "pomo-dots", text: "•••••" }),
  );

  const leftNavBtn = h("button", {
    class: "pomo-nav-btn", title: "Previous Card (Pomodoro)",
    onclick: () => { actions.blip(); actions.setView("pomodoro"); },
  }, svg(ICONS.chevronLeft, 14));

  const rightNavBtn = h("button", {
    class: "pomo-nav-btn", title: "Next Card (Notes)",
    onclick: () => { actions.blip(); actions.setView("note"); },
  }, svg(ICONS.chevronRight, 14));

  const countDisplay = h("div", { class: "todo-count", text: "4/5" });
  const countSub = h("div", { class: "todo-count-sub", text: "Task Done Today" });

  const addTaskBtn = h("button", {
    class: "todo-action-btn",
    onclick: () => { actions.blip(); showInputRow(); },
  }, h("span", { text: "+" }), h("span", { text: "Add Task" }));

  const editTaskBtn = h("button", {
    class: "todo-action-btn",
    onclick: () => {
      actions.blip();
      if (todos.some((t) => t.completed)) {
        todos = todos.filter((t) => !t.completed);
        saveTodos(todos);
        renderTasks();
        syncStats();
      }
    },
  }, h("span", { text: "✎" }), h("span", { text: "Clear Done" }));

  const leftCol = h("div", { class: "todo-left-col" },
    countDisplay, countSub,
    h("div", { class: "todo-actions-row" }, addTaskBtn, editTaskBtn));

  const taskListEl = h("div", { class: "todo-task-list" });

  const inputField = h("input", {
    type: "text",
    class: "todo-new-input",
    placeholder: "Type a task & press Enter...",
  }) as HTMLInputElement;

  inputField.addEventListener("keydown", (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      const text = inputField.value.trim();
      if (text) {
        todos.push({ id: String(Date.now()), text, completed: false, createdAt: Date.now() });
        saveTodos(todos);
        inputField.value = "";
        hideInputRow();
        renderTasks();
        syncStats();
        Sound.play("pop");
      }
    } else if (e.key === "Escape") {
      hideInputRow();
    }
  });

  const inputRow = h("div", { class: "todo-input-wrap", style: "display:none" },
    inputField,
    h("button", { class: "todo-input-cancel", onclick: () => hideInputRow() }, "✕"),
  );

  const rightCol = h("div", { class: "todo-right-col" }, inputRow, taskListEl);

  const middleRow = h("div", { class: "pomo-middle-row todo-middle-row" },
    leftNavBtn, leftCol, rightCol, rightNavBtn);

  const paginationRow = h("div", { class: "pomo-pagination" },
    h("span", { class: "pomo-dot", onclick: () => { actions.blip(); actions.setView("overview"); } }),
    h("span", { class: "pomo-dot", onclick: () => { actions.blip(); actions.setView("pomodoro"); } }),
    h("span", { class: "pomo-dot-pill active" }),
    h("span", { class: "pomo-dot", onclick: () => { actions.blip(); actions.setView("note"); } }),
  );

  const el = h("div", { class: "view todolist-view" },
    h("div", { class: "card pomo-card todo-card" },
      headerRow, middleRow, paginationRow));

  // ── Helpers ───────────────────────────────────────────────────────────────
  function showInputRow() {
    inputRow.style.display = "flex";
    inputField.focus();
  }

  function hideInputRow() {
    inputRow.style.display = "none";
  }

  function syncStats() {
    const done = todos.filter((t) => t.completed).length;
    countDisplay.textContent = `${done}/${todos.length}`;
  }

  function renderTasks() {
    // Skip rebuilding if nothing changed
    const todosJson = JSON.stringify(todos);
    if (todosJson === lastTodosJson) return;
    lastTodosJson = todosJson;

    while (taskListEl.firstChild) taskListEl.removeChild(taskListEl.firstChild);

    if (todos.length === 0) {
      taskListEl.append(h("div", { class: "todo-empty-hint", text: "No tasks yet. Click '+ Add Task' to start!" }));
      return;
    }

    for (const item of todos) {
      const checkCircle = h("div", {
        class: item.completed ? "todo-check-circle completed" : "todo-check-circle",
        onclick: (e: Event) => {
          e.stopPropagation();
          actions.blip();
          item.completed = !item.completed;
          saveTodos(todos);
          renderTasks();
          syncStats();
          if (item.completed) Sound.play("approve");
        },
      }, item.completed ? "✔" : "");

      const textEl = h("span", {
        class: item.completed ? "todo-item-text completed" : "todo-item-text",
        text: item.text,
      });

      const delBtn = h("button", {
        class: "todo-item-del", title: "Delete Task",
        onclick: (e: Event) => {
          e.stopPropagation();
          todos = todos.filter((t) => t.id !== item.id);
          saveTodos(todos);
          renderTasks();
          syncStats();
        },
      }, "×");

      taskListEl.append(h("div", {
        class: item.completed ? "todo-item-pill completed" : "todo-item-pill",
      }, checkCircle, textEl, delBtn));
    }
  }

  function syncClock() {
    const dateStr = formatLiveDate(new Date());
    if (dateStr !== lastDateStr) {
      dateTimeEl.textContent = dateStr;
      lastDateStr = dateStr;
    }
  }

  // Initial render
  renderTasks();
  syncStats();
  syncClock();

  return {
    el,
    sync() {
      syncClock();
      syncStats();
      renderTasks(); // only redraws if todos changed
      // Start clock once when view activates
      if (clockInterval == null) {
        clockInterval = window.setInterval(syncClock, 1000);
      }
    },
  };
}
