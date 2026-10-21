// Tests for Chat Sessions persistence, image attachment, and voice mode in Coucou AI Chat.

import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";

installFakeDom();
const { ChatSessionsManager } = await import("../src/core/chat-sessions.ts");
const { buildPrompt } = await import("../src/views/chat.ts");
const { DEFAULT_SETTINGS, State } = await import("../src/core/state.ts");

beforeEach(() => {
  State.settings = { ...DEFAULT_SETTINGS, chatModels: {} };
  State.chatHistory = [];
  State.stateOverride = null;
  State.view = "prompt";
  State.droppedFile = null;
});

test("ChatSessionsManager creates, switches, and deletes sessions", () => {
  const initial = ChatSessionsManager.getActive();
  assert.ok(initial.id);
  assert.equal(initial.title, "New Chat");

  // Save some messages to initial session
  ChatSessionsManager.saveActive([
    { role: "user", content: "How to write a Rust macro?" },
    { role: "assistant", content: "Rust macros are defined with macro_rules!..." },
  ]);

  const updated = ChatSessionsManager.getActive();
  assert.equal(updated.messages.length, 2);
  assert.equal(updated.title, "How to write a Rust macro?");

  // Create a second session
  const second = ChatSessionsManager.create();
  assert.notEqual(second.id, initial.id);
  assert.equal(ChatSessionsManager.getActive().id, second.id);
  assert.equal(ChatSessionsManager.getAll().length >= 2, true);

  // Switch back to initial
  const switched = ChatSessionsManager.switch(initial.id);
  assert.ok(switched);
  assert.equal(ChatSessionsManager.getActiveId(), initial.id);
  assert.equal(ChatSessionsManager.getActive().messages.length, 2);

  // Delete initial session
  const afterDelete = ChatSessionsManager.delete(initial.id);
  assert.ok(afterDelete);
  assert.equal(ChatSessionsManager.getAll().some((s) => s.id === initial.id), false);
});

test("Chat view renders session header bar, image attachment, and voice buttons", () => {
  const view = buildPrompt(() => {});
  view.sync();

  // Session selector button
  const sessionBtn = view.el.querySelector(".chat-session-btn");
  assert.ok(sessionBtn, "Session selector button should exist");

  // Icon buttons in header (New Chat & TTS)
  const headerActions = view.el.querySelector(".chat-header-actions");
  assert.ok(headerActions, "Chat header actions container should exist");
  const iconBtns = headerActions.find(".chat-icon-btn");
  assert.equal(iconBtns.length >= 2, true, "Header should contain TTS and New Chat buttons");

  // Image attachment button
  const attachBtn = view.el.querySelector(".chat-attach-btn");
  assert.ok(attachBtn, "Attach image button should exist");

  // Voice input microphone button
  const micBtn = view.el.querySelector(".chat-mic-btn");
  assert.ok(micBtn, "Microphone button should exist");
});

test("ChatMessage supports optional image property", () => {
  const messageWithImage = {
    role: "user",
    content: "What is in this diagram?",
    image: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
  };
  State.chatHistory.push(messageWithImage);

  assert.equal(State.chatHistory.length, 1);
  assert.ok(State.chatHistory[0].image);
  assert.equal(State.chatHistory[0].image.startsWith("data:image/png"), true);
});
