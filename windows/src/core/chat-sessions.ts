// Chat Sessions Persistence — multi-session chat manager for Coucou / NexIsland.
//
// Automatically persists chat sessions to localStorage, supports creating new chats,
// switching between sessions, renaming, and deleting old conversations.

import type { ChatMessage } from "./state";

export interface ChatSessionItem {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: ChatMessage[];
}

const STORAGE_KEY = "nexisland_chat_sessions_v1";
const ACTIVE_SESSION_KEY = "nexisland_active_session_id_v1";

const memoryStore = new Map<string, string>();

function safeGetItem(key: string): string | null {
  try {
    if (typeof localStorage !== "undefined" && localStorage) {
      return localStorage.getItem(key);
    }
  } catch {}
  return memoryStore.get(key) ?? null;
}

function safeSetItem(key: string, value: string): void {
  try {
    if (typeof localStorage !== "undefined" && localStorage) {
      localStorage.setItem(key, value);
      return;
    }
  } catch {}
  memoryStore.set(key, value);
}

function generateId(): string {
  return "session_" + Date.now().toString(36) + "_" + Math.random().toString(36).slice(2, 7);
}

function deriveTitle(messages: ChatMessage[]): string {
  const firstUser = messages.find((m) => m.role === "user");
  if (!firstUser || !firstUser.content.trim()) return "New Chat";
  const clean = firstUser.content.replace(/\s+/g, " ").trim();
  return clean.length > 32 ? clean.slice(0, 32) + "…" : clean;
}

export class ChatSessionsManager {
  private static cachedSessions: ChatSessionItem[] | null = null;
  private static activeId: string | null = null;

  static getAll(): ChatSessionItem[] {
    if (this.cachedSessions) return this.cachedSessions;
    try {
      const raw = safeGetItem(STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed) && parsed.length > 0) {
          this.cachedSessions = parsed;
          return this.cachedSessions;
        }
      }
    } catch {
      // Ignore parse failure, fall back to default session
    }

    const defaultSession: ChatSessionItem = {
      id: generateId(),
      title: "New Chat",
      createdAt: Date.now(),
      updatedAt: Date.now(),
      messages: [],
    };
    this.cachedSessions = [defaultSession];
    this.saveToStorage();
    return this.cachedSessions;
  }

  static getActiveId(): string {
    if (this.activeId) return this.activeId;
    const stored = safeGetItem(ACTIVE_SESSION_KEY);
    const sessions = this.getAll();
    if (stored && sessions.some((s) => s.id === stored)) {
      this.activeId = stored;
    } else {
      this.activeId = sessions[0]?.id || generateId();
      safeSetItem(ACTIVE_SESSION_KEY, this.activeId);
    }
    return this.activeId;
  }

  static getActive(): ChatSessionItem {
    const id = this.getActiveId();
    const sessions = this.getAll();
    const found = sessions.find((s) => s.id === id);
    if (found) return found;

    // If active session not found, create or select first
    if (sessions.length > 0) {
      this.activeId = sessions[0].id;
      safeSetItem(ACTIVE_SESSION_KEY, this.activeId);
      return sessions[0];
    }

    const newSess = this.create();
    return newSess;
  }

  static saveActive(messages: ChatMessage[], explicitTitle?: string): ChatSessionItem {
    const active = this.getActive();
    active.messages = messages;
    active.updatedAt = Date.now();
    if (explicitTitle) {
      active.title = explicitTitle;
    } else if (active.title === "New Chat" && messages.length > 0) {
      active.title = deriveTitle(messages);
    }

    this.saveToStorage();
    return active;
  }

  static create(): ChatSessionItem {
    const sessions = this.getAll();
    const newSession: ChatSessionItem = {
      id: generateId(),
      title: "New Chat",
      createdAt: Date.now(),
      updatedAt: Date.now(),
      messages: [],
    };

    sessions.unshift(newSession);
    this.activeId = newSession.id;
    safeSetItem(ACTIVE_SESSION_KEY, this.activeId);
    this.saveToStorage();
    return newSession;
  }

  static switch(id: string): ChatSessionItem | null {
    const sessions = this.getAll();
    const target = sessions.find((s) => s.id === id);
    if (!target) return null;

    this.activeId = target.id;
    safeSetItem(ACTIVE_SESSION_KEY, this.activeId);
    return target;
  }

  static delete(id: string): ChatSessionItem {
    let sessions = this.getAll();
    sessions = sessions.filter((s) => s.id !== id);
    if (sessions.length === 0) {
      const fresh: ChatSessionItem = {
        id: generateId(),
        title: "New Chat",
        createdAt: Date.now(),
        updatedAt: Date.now(),
        messages: [],
      };
      sessions = [fresh];
    }
    this.cachedSessions = sessions;
    if (this.activeId === id) {
      this.activeId = sessions[0].id;
      safeSetItem(ACTIVE_SESSION_KEY, this.activeId);
    }
    this.saveToStorage();
    return this.getActive();
  }

  static rename(id: string, newTitle: string): void {
    const sessions = this.getAll();
    const s = sessions.find((item) => item.id === id);
    if (s && newTitle.trim()) {
      s.title = newTitle.trim();
      s.updatedAt = Date.now();
      this.saveToStorage();
    }
  }

  private static saveToStorage(): void {
    if (!this.cachedSessions) return;
    try {
      safeSetItem(STORAGE_KEY, JSON.stringify(this.cachedSessions));
    } catch (e) {
      console.warn("Failed to persist chat sessions to storage:", e);
    }
  }
}
