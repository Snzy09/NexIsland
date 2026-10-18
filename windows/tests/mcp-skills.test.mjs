// Tests for MCP and Skills integration in Coucou AI Chat.

import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, internals, sent } from "./tauri.mjs";

installFakeDom();
const { buildPrompt } = await import("../src/views/chat.ts");
const { DEFAULT_SETTINGS, State } = await import("../src/core/state.ts");
const { Bridge } = await import("../src/core/bridge.ts");

let answers;
const plainInvoke = internals.invoke;
internals.invoke = async (cmd, args) => {
  const result = await plainInvoke(cmd, args);
  if (cmd in answers) return typeof answers[cmd] === "function" ? answers[cmd](args) : answers[cmd];
  return result;
};

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

let view;
beforeEach(() => {
  calls.length = 0;
  answers = {};
  State.settings = { ...DEFAULT_SETTINGS, chatModels: {} };
  State.chatHistory = [];
  State.stateOverride = null;
  State.view = "prompt";
  State.droppedFile = null;
  view = buildPrompt(() => {});
  view.sync();
});

const $ = (cls) => view.el.querySelector(cls);

test("chat input has placeholder and send button", () => {
  const input = $(".chat-input");
  assert.equal(input.placeholder, "Ask me anything…");
  assert.ok($(".send-btn"));
});

test("Bridge has MCP and Skills methods wired to invoke", async () => {
  answers.mcp_list_tools = [
    { server: "test-server", name: "test_tool", displayName: "test-server__test_tool", description: "A test tool", inputSchema: {} },
  ];
  answers.skills_list = [
    { id: "code-reviewer", name: "Code Reviewer", description: "Reviews code", instructions: "Review carefully" },
  ];

  const tools = await Bridge.mcpListTools();
  assert.equal(tools?.length, 1);
  assert.equal(tools[0].name, "test_tool");
  assert.deepEqual(sent("mcp_list_tools"), [{}]);

  const skills = await Bridge.skillsList();
  assert.equal(skills?.length, 1);
  assert.equal(skills[0].id, "code-reviewer");
  assert.deepEqual(sent("skills_list"), [{}]);

  await Bridge.openSkillsFolder();
  assert.deepEqual(sent("open_skills_folder"), [{}]);

  await Bridge.openMcpConfig();
  assert.deepEqual(sent("open_mcp_config"), [{}]);
});

test("sending /skills command routes through chatSend", async () => {
  answers.chat_send = (args) => {
    if (args.query === "/skills") {
      return { text: "### Available Skills\n- /skill code-reviewer" };
    }
    return { text: "echo" };
  };

  const input = $(".chat-input");
  input.value = "/skills";
  $(".send-btn").fire("click");
  await flush();

  assert.equal(sent("chat_send").length, 1);
  assert.equal(sent("chat_send")[0].query, "/skills");
  assert.equal(State.chatHistory.length, 2);
  assert.equal(State.chatHistory[0].content, "/skills");
  assert.match(State.chatHistory[1].content, /Available Skills/);
});

test("sending /mcp command routes through chatSend", async () => {
  answers.chat_send = (args) => {
    if (args.query === "/mcp") {
      return { text: "### Model Context Protocol (MCP)\n1 Active Tool(s)" };
    }
    return { text: "echo" };
  };

  const input = $(".chat-input");
  input.value = "/mcp";
  $(".send-btn").fire("click");
  await flush();

  assert.equal(sent("chat_send").length, 1);
  assert.equal(sent("chat_send")[0].query, "/mcp");
  assert.equal(State.chatHistory.length, 2);
  assert.match(State.chatHistory[1].content, /Model Context Protocol/);
});

test("sending /skill query passes through chatSend", async () => {
  answers.chat_send = (args) => {
    if (args.query.startsWith("/skill code-reviewer")) {
      return { text: "Code review: No issues found." };
    }
    return { text: "echo" };
  };

  const input = $(".chat-input");
  input.value = "/skill code-reviewer check this function";
  $(".send-btn").fire("click");
  await flush();

  assert.equal(sent("chat_send").length, 1);
  assert.equal(sent("chat_send")[0].query, "/skill code-reviewer check this function");
  assert.equal(State.chatHistory.length, 2);
  assert.match(State.chatHistory[1].content, /Code review/);
});

test("Bridge has toolsListBuiltin wired to invoke", async () => {
  answers.tools_list_builtin = [
    { name: "run_command", description: "Executes shell command", parameters: {}, category: "terminal" },
    { name: "browse_url", description: "Browses URL", parameters: {}, category: "web" },
  ];

  const tools = await Bridge.toolsListBuiltin();
  assert.equal(tools?.length, 2);
  assert.equal(tools[0].name, "run_command");
  assert.deepEqual(sent("tools_list_builtin"), [{}]);
});

test("sending /tools query routes to chat_send", async () => {
  answers.chat_send = (args) => {
    if (args.query === "/tools") {
      return { text: "Built-in Device Tools: Terminal, Web, Files, System" };
    }
    return { text: "echo" };
  };

  const input = $(".chat-input");
  input.value = "/tools";
  $(".send-btn").fire("click");
  await flush();

  assert.equal(sent("chat_send").length, 1);
  assert.equal(sent("chat_send")[0].query, "/tools");
  assert.equal(State.chatHistory.length, 2);
  assert.match(State.chatHistory[1].content, /Built-in Device Tools/);
});

test("typing / opens slash command autocomplete menu", async () => {
  const input = $(".chat-input");
  const menu = $(".slash-menu");
  assert.equal(menu.style.display, "none");

  input.value = "/";
  input.fire("input");

  assert.equal(menu.style.display, "flex");
  const items = menu.querySelectorAll(".slash-item");
  assert.ok(items.length >= 6);

  const names = Array.from(items).map((el) => el.querySelector(".slash-item-name")?.textContent);
  assert.ok(names.includes("/tools"));
  assert.ok(names.includes("/skills"));
  assert.ok(names.includes("/mcp"));
  assert.ok(names.includes("/clear"));

  // Filtering:
  input.value = "/to";
  input.fire("input");
  const filtered = Array.from(menu.querySelectorAll(".slash-item")).map(
    (el) => el.querySelector(".slash-item-name")?.textContent,
  );
  assert.deepEqual(filtered, ["/tools", "/tools on", "/tools off"]);

  // Escape closes menu:
  input.fire("keydown", { key: "Escape" });
  assert.equal(menu.style.display, "none");

  // Non-slash text hides menu:
  input.value = "hello";
  input.fire("input");
  assert.equal(menu.style.display, "none");
});

test("/clear command clears chat history and calls chatReset", async () => {
  State.chatHistory = [
    { id: 1, role: "user", content: "hello" },
    { id: 2, role: "assistant", content: "hi there" },
  ];
  view.sync();
  assert.equal(State.chatHistory.length, 2);

  const input = $(".chat-input");
  input.value = "/clear";
  $(".send-btn").fire("click");
  await flush();

  assert.equal(State.chatHistory.length, 0);
  assert.deepEqual(sent("chat_reset"), [{}]);
});

test("Bridge has MCP online library methods wired to invoke", async () => {
  answers.mcp_list_installed = {
    filesystem: { command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem"] },
  };
  answers.mcp_list_library = [
    { id: "memory", name: "Memory", displayName: "Knowledge Graph", description: "Graph memory", category: "Knowledge", package: "@modelcontextprotocol/server-memory", command: "npx", args: ["-y", "@modelcontextprotocol/server-memory"], envKeys: [], defaultEnv: {}, installed: false, official: true },
  ];
  answers.mcp_install = { serverName: "memory", toolsCount: 5, success: true, message: "Installed" };
  answers.mcp_uninstall = true;
  answers.mcp_add_custom = { serverName: "custom", toolsCount: 2, success: true, message: "Added" };

  const installed = await Bridge.mcpListInstalled();
  assert.ok(installed && installed.filesystem);
  assert.deepEqual(sent("mcp_list_installed"), [{}]);

  const lib = await Bridge.mcpListLibrary("memory");
  assert.equal(lib?.length, 1);
  assert.equal(lib[0].id, "memory");
  assert.deepEqual(sent("mcp_list_library"), [{ search: "memory" }]);

  const outcome = await Bridge.mcpInstall("memory", { API_KEY: "xyz" }, ["--arg1"]);
  assert.equal(outcome.serverName, "memory");
  assert.equal(outcome.toolsCount, 5);
  assert.deepEqual(sent("mcp_install"), [{ id: "memory", env: { API_KEY: "xyz" }, args: ["--arg1"] }]);

  const uninstalled = await Bridge.mcpUninstall("filesystem");
  assert.equal(uninstalled, true);
  assert.deepEqual(sent("mcp_uninstall"), [{ name: "filesystem" }]);

  const customOutcome = await Bridge.mcpAddCustom("custom", "npx", ["-y", "pkg"], { PORT: "8080" });
  assert.equal(customOutcome.serverName, "custom");
  assert.deepEqual(sent("mcp_add_custom"), [{ name: "custom", command: "npx", args: ["-y", "pkg"], env: { PORT: "8080" } }]);
});

test("slash menu contains all MCP library commands", async () => {
  const input = $(".chat-input");
  const menu = $(".slash-menu");

  input.value = "/mcp";
  input.fire("input");

  assert.equal(menu.style.display, "flex");
  const names = Array.from(menu.querySelectorAll(".slash-item")).map(
    (el) => el.querySelector(".slash-item-name")?.textContent,
  );
  assert.ok(names.includes("/mcp"));
  assert.ok(names.includes("/mcp library"));
  assert.ok(names.includes("/mcp search"));
  assert.ok(names.includes("/mcp install"));
  assert.ok(names.includes("/mcp uninstall"));
});


