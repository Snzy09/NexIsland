// The chat, whoever answers it: the conversation, the system prompt, and which
// provider a turn goes to. Each provider's own wire format lives in its module:
// claude.rs (Anthropic), openai_compat.rs (OpenAI, Google AI, OpenRouter) and
// local_chat.rs (Ollama, LM Studio, any OpenAI-compatible server).
//
// API keys never leave the credential store and file bytes never cross the IPC
// boundary: the island sends the question and gets the answer's text back.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::settings::Settings;
use crate::{claude, local_chat, openai_compat, secrets};

pub const ANTHROPIC: &str = "anthropic";

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatContext {
    File { name: String, path: String },
    Window { app_name: String, title: String, url: Option<String> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReply {
    pub text: String,
}

/// A model a provider offers, for the picker in the chat view.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
}

// ── Conversation ──────────────────────────────────────────────────────────────

/// The conversation is kept twice: in the wire format of the provider that
/// answered last (Claude's carries web search blocks no other provider would
/// understand), and as plain text turns. Switching provider mid-conversation
/// rebuilds the history from the plain turns, so nothing in one provider's
/// format is ever sent to another.
pub struct Chat {
    inner: Mutex<Conversation>,
    cancelled: std::sync::atomic::AtomicBool,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Conversation::default()),
            cancelled: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[derive(Default)]
struct Conversation {
    /// Bumped by every reset, so an answer that lands after "New chat" is dropped.
    epoch: u64,
    /// The provider `native` belongs to.
    owner: Option<String>,
    native: Vec<Value>,
    /// `{"role", "content": text}` turns, the same whoever answered.
    plain: Vec<Value>,
}

/// What a provider needs to build one turn.
pub struct Turn {
    epoch: u64,
    provider: String,
    /// No earlier turn: the file or window context rides along with this one.
    pub first: bool,
    /// The earlier turns, in this provider's format.
    pub history: Vec<Value>,
}

impl Chat {
    pub fn cancel(&self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        let mut c = self.inner.lock().unwrap();
        c.epoch += 1;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.cancelled.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut c = self.inner.lock().unwrap();
        let epoch = c.epoch + 1;
        *c = Conversation { epoch, ..Default::default() };
    }

    /// Starts a turn with `provider`, converting the history if another
    /// provider answered the previous turns.
    pub fn begin(&self, provider: &str) -> Turn {
        self.cancelled.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut c = self.inner.lock().unwrap();
        if c.owner.as_deref() != Some(provider) {
            c.native = c.plain.clone();
            c.owner = Some(provider.to_string());
        }
        Turn {
            epoch: c.epoch,
            provider: provider.to_string(),
            first: c.plain.is_empty(),
            history: c.native.clone(),
        }
    }

    /// Records a finished turn: the user message and the answer in the
    /// provider's format, and their plain text. Only a successful turn is
    /// recorded, so the history always matches what the model saw.
    pub fn commit(&self, turn: &Turn, user: Value, assistant: Value, user_text: &str, answer: &str) {
        let mut c = self.inner.lock().unwrap();
        if c.epoch != turn.epoch || c.owner.as_deref() != Some(turn.provider.as_str()) {
            return;
        }
        c.native.push(user);
        c.native.push(assistant);
        c.plain.push(json!({ "role": "user", "content": user_text }));
        c.plain.push(json!({ "role": "assistant", "content": answer }));
    }
}

// ── System prompt ─────────────────────────────────────────────────────────────

/// Mochi's instructions. Greets the user by their first name when the account
/// Mochi's instructions. Greets the user by their first name when the account
/// has one worth using (identity.rs), and claims web search or built-in tools where enabled.
pub fn system_prompt(web_search: bool, skill_extra: Option<&str>, tools_enabled: bool) -> String {
    let mut base = system_prompt_for(crate::identity::first_name(), web_search, tools_enabled);
    if let Some(extra) = skill_extra {
        base.push_str("\n\n");
        base.push_str(extra);
    }
    base
}

fn system_prompt_for(first_name: Option<&str>, web_search: bool, tools_enabled: bool) -> String {
    let opening = match first_name {
        Some(name) => format!("You are Mochi, {name}'s personal AI assistant living at the top of their screen."),
        None => "You are Mochi, a personal AI assistant living at the top of the user's screen.".to_string(),
    };
    let abilities = if tools_enabled {
        "You operate as an autonomous AI agent with built-in device tools and MCP (Model Context Protocol) tools available:\n\
- `web_search`: Search the live web for current facts, real-time news, stock prices, documentation, and web sources. ALWAYS call this tool when the user asks for news, current information, or web lookups.\n\
- `run_command`: Execute shell commands (PowerShell on Windows, sh on Linux) to run scripts, compile code, test, inspect files, or manage systems.\n\
- `browse_url`: Fetch and inspect web pages or documentation.\n\
- `read_file`, `write_file`, `edit_file`, `list_directory`: Read, create, and modify project and system files.\n\
- `system_info`: Inspect the device OS, user, and environment.\n\n\
AUTONOMOUS AGENT PROTOCOL:\n\
1. When asked for news, live data, or current events (e.g. 'cari berita...', 'search...'), immediately call `web_search`.\n\
2. You have full permission to read/write files and execute terminal commands to accomplish tasks directly without asking for confirmation.\n\
3. Once you gather the necessary information or execute the requested task, stop calling tools and immediately formulate a thorough, well-structured, final response.\n\
4. Always answer in the user's language (e.g. Bahasa Indonesia if the user wrote in Indonesian).\n\n\
PROMPT CRAFTING & AI AGENT INTEGRATION:\n\
You can directly formulate, optimize, and write prompts for Google Antigravity (AGY CLI, IDE, subagents, skills, MCP) as well as integrated providers (Anthropic Claude, Google Gemini, OpenAI, OpenRouter, Ollama, LM Studio).\n\
When the user asks you to write, craft, or send a prompt to Antigravity or any integrated provider (e.g. 'tulis prompt ke antigravity...', 'buat prompt untuk provider...'), create a complete, ready-to-run prompt with: Objective, Context & Constraints, Recommended Tools/Skills/Subagents to invoke, and Expected Output. Provide the complete prompt in a clean markdown code block."
    } else if web_search {
        "You have web search access and can help with absolutely anything — research, coding, finding places, recommendations, tasks, questions.\n\n\
You can also formulate, optimize, and write prompts for Google Antigravity and any integrated provider (Anthropic, Gemini, OpenAI, Ollama). When requested, structure the prompt with Objective, Context, Tools/Skills needed, and Expected Output."
    } else {
        "You can help with absolutely anything — research, coding, recommendations, tasks, questions. You have no web access: say so when something needs current information.\n\n\
You can also formulate, optimize, and write prompts for Google Antigravity and any integrated provider (Anthropic, Gemini, OpenAI, Ollama). When requested, structure the prompt with Objective, Context, Tools/Skills needed, and Expected Output."
    };
    format!(
        "{opening}\n\n{abilities}\n\n\
Respond in the user's language. Be thorough, accurate, and complete — use as much detail as the task requires. \
Use light Markdown when it helps: short paragraphs, bullet lists, **bold**, `inline code` and fenced code blocks. Avoid tables and big headings: the chat window is small."
    )
}

/// The window context line, as ClaudeService.chat() writes it.
pub fn window_line(app_name: &str, title: &str, url: Option<&str>) -> String {
    let mut text = format!("Context — App: {app_name}, Window: {title}");
    if let Some(url) = url {
        text.push_str(&format!(", URL: {url}"));
    }
    text
}

/// The plain-text record of what the user asked, context included.
pub fn plain_question(first: bool, context: Option<&ChatContext>, query: &str) -> String {
    match context.filter(|_| first) {
        Some(ChatContext::File { name, .. }) => format!("File: {name}\n\n{query}"),
        Some(ChatContext::Window { app_name, title, url }) => {
            format!("{}\n\n{query}", window_line(app_name, title, url.as_deref()))
        }
        None => query.to_string(),
    }
}

// ── Which provider ────────────────────────────────────────────────────────────

/// The model chosen for `provider`, or its default.
pub fn model_for(settings: &Settings, provider: &str) -> String {
    if provider == "antigravity" || provider == "agy" {
        return "active".to_string();
    }
    if provider == ANTHROPIC {
        let m = settings.model.trim();
        return if m.is_empty() { claude::DEFAULT_MODEL.to_string() } else { m.to_string() };
    }
    settings
        .chat_models
        .get(provider)
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .or_else(|| openai_compat::provider(provider).map(|p| p.default_model.to_string()))
        .unwrap_or_default()
}

/// A file rides along only if it is one of Coucou's own copies of a dropped
/// file (files.rs puts them in the inbox). The page names the path, so without
/// this any file the user can read could be sent to a chat provider.
fn checked_context(context: ChatContext) -> Result<ChatContext, String> {
    match context {
        ChatContext::File { name, path } => {
            let inbox = crate::files::inbox_dir();
            if !is_inside(&inbox, std::path::Path::new(&path)) {
                return Err(crate::i18n::t("Only a file dropped on the island can be sent with a question."));
            }
            Ok(ChatContext::File { name, path })
        }
        other => Ok(other),
    }
}

/// True when `path` is a regular file directly inside `dir`, both resolved
/// (no `..`, no symlink pointing out of it).
fn is_inside(dir: &std::path::Path, path: &std::path::Path) -> bool {
    let (Ok(dir), Ok(file)) = (dir.canonicalize(), path.canonicalize()) else { return false };
    file.parent() == Some(dir.as_path())
        && std::fs::symlink_metadata(&file).map(|m| m.is_file()).unwrap_or(false)
}

async fn handle_mcp_command(trimmed: &str) -> Option<ChatReply> {
    if trimmed == "/mcp" {
        let tools = crate::mcp::list_tools();
        let configs = crate::mcp::config_paths();
        let servers = crate::mcp::load_servers();
        let mut text = "### 🔌 Model Context Protocol (MCP) System\n\n".to_string();
        if configs.is_empty() {
            text.push_str("*Configuration:* `%APPDATA%\\Coucou\\mcp.json`\n\n");
        } else {
            text.push_str(&format!("*Config file:* `{}`\n\n", configs[0].display()));
        }

        if servers.is_empty() {
            text.push_str("No MCP servers installed yet.\n\n");
        } else {
            text.push_str(&format!("**Installed Servers ({})**: ", servers.len()));
            let names: Vec<String> = servers.keys().cloned().collect();
            text.push_str(&format!("`{}`\n\n", names.join("`, `")));
        }

        if tools.is_empty() {
            text.push_str("*No active MCP tools detected.*\n\n");
        } else {
            text.push_str(&format!("**{} Active Tool(s):**\n", tools.len()));
            for t in &tools {
                text.push_str(&format!("- **`{}`** (from `{}`): {}\n", t.display_name, t.server, t.description));
            }
            text.push('\n');
        }

        text.push_str("**Online MCP Library & Commands:**\n");
        text.push_str("- `/mcp library` (or `/mcp list`): Browse available online MCP servers\n");
        text.push_str("- `/mcp search <query>`: Search online MCP registry\n");
        text.push_str("- `/mcp install <name>`: Install an MCP server online\n");
        text.push_str("- `/mcp uninstall <name>`: Remove an MCP server\n");
        return Some(ChatReply { text });
    }

    if trimmed == "/mcp library" || trimmed == "/mcp list" {
        let lib = crate::mcp::curated_library();
        let servers = crate::mcp::load_servers();
        let mut text = "### 📚 Online MCP Server Library\n\n".to_string();
        text.push_str("Install any server instantly using `/mcp install <name>`:\n\n");
        text.push_str("| Server | Category | Description | Status |\n");
        text.push_str("|---|---|---|---|\n");
        for e in lib {
            let installed = servers.contains_key(&e.id) || servers.contains_key(&e.name);
            let status = if installed {
                "🟢 Installed".to_string()
            } else {
                format!("`install {}`", e.id)
            };
            text.push_str(&format!("| **{}** | {} | {} | {} |\n", e.id, e.category, e.description, status));
        }
        text.push_str("\n*Tip: Run `/mcp install <name>` or `/mcp search <keyword>` to find more packages.*");
        return Some(ChatReply { text });
    }

    if let Some(query) = trimmed.strip_prefix("/mcp search ") {
        let q = query.trim();
        let results = crate::mcp::search_library(Some(q)).await;
        let mut text = format!("### 🔍 MCP Search Results for \"{q}\"\n\n");
        if results.is_empty() {
            text.push_str("No MCP servers found. You can still install any npm package with `/mcp install <npm-package-name>`.");
        } else {
            text.push_str(&format!("Found **{}** matching server(s):\n\n", results.len()));
            for r in results {
                let status = if r.installed { " [Installed]" } else { "" };
                text.push_str(&format!("- **`{}`** ({}){} - {}\n  *Command:* `/mcp install {}`\n\n", r.id, r.category, status, r.description, r.id));
            }
        }
        return Some(ChatReply { text });
    }

    if let Some(target) = trimmed.strip_prefix("/mcp install ") {
        let name = target.trim();
        if name.is_empty() {
            return Some(ChatReply {
                text: "❌ Please specify an MCP server name or package. Example: `/mcp install memory` or `/mcp install fetch`.".to_string(),
            });
        }
        return match crate::mcp::install_from_library(name, None, None) {
            Ok(outcome) => {
                let mut text = format!("✅ **MCP Server `{}` installed successfully!**\n\n", outcome.server_name);
                text.push_str("- Server registered in `%APPDATA%\\Coucou\\mcp.json`\n");
                text.push_str(&format!("- **Active Tools Discovered:** {}\n", outcome.tools_count));
                text.push_str("\nYou can now use these tools directly in your chat queries!");
                Some(ChatReply { text })
            }
            Err(e) => Some(ChatReply {
                text: format!("❌ Failed to install MCP server `{name}`: {e}"),
            }),
        };
    }

    if let Some(target) = trimmed.strip_prefix("/mcp uninstall ").or_else(|| trimmed.strip_prefix("/mcp remove ")) {
        let name = target.trim();
        if name.is_empty() {
            return Some(ChatReply {
                text: "❌ Please specify an MCP server name to uninstall. Example: `/mcp uninstall memory`.".to_string(),
            });
        }
        return match crate::mcp::remove_server(name) {
            Ok(true) => Some(ChatReply {
                text: format!("✅ **MCP Server `{name}` removed** from configuration."),
            }),
            Ok(false) => Some(ChatReply {
                text: format!("⚠️ Server `{name}` was not found in `%APPDATA%\\Coucou\\mcp.json`."),
            }),
            Err(e) => Some(ChatReply {
                text: format!("❌ Error removing MCP server `{name}`: {e}"),
            }),
        };
    }

    None
}

/// One chat turn with the provider chosen in the settings.
pub async fn send(
    app: &AppHandle,
    chat: &Chat,
    settings: &Settings,
    query: String,
    context: Option<ChatContext>,
) -> Result<ChatReply, String> {
    let trimmed = query.trim();
    if let Some(reply) = handle_mcp_command(trimmed).await {
        return Ok(reply);
    }
    if trimmed == "/tools" {
        let mut text = "### ⚡ Built-in Device Tools\n\n".to_string();
        let master = if settings.chat_tools_enabled { "🟢 ON" } else { "🔴 OFF" };
        text.push_str(&format!("**Master Switch:** {}\n\n", master));
        text.push_str("**Available Tools:**\n");
        let term = if settings.chat_tool_terminal { "🟢 ON" } else { "🔴 OFF" };
        let web = if settings.chat_tool_web { "🟢 ON" } else { "🔴 OFF" };
        let file = if settings.chat_tool_file { "🟢 ON" } else { "🔴 OFF" };
        let sys = if settings.chat_tool_system { "🟢 ON" } else { "🔴 OFF" };
        let phone = if settings.phone_tools_enabled { "🟢 ON" } else { "🔴 OFF" };

        text.push_str(&format!("- **Terminal (`run_command`)**: {}\n", term));
        text.push_str(&format!("- **Web Browsing (`browse_url`)**: {}\n", web));
        text.push_str(&format!("- **File System (`read_file`, `write_file`, `edit_file`, `list_directory`)**: {}\n", file));
        text.push_str(&format!("- **System Info (`system_info`)**: {}\n", sys));
        text.push_str(&format!("- **Mobile Phone Execution (Phone Link)**: {}\n\n", phone));
        text.push_str("*Configure tools in Settings → Chat, or type `/tools on` / `/tools off` to toggle.*");
        return Ok(ChatReply { text });
    }

    if trimmed == "/tools on" || trimmed == "/tools off" {
        let enable = trimmed == "/tools on";
        let mut s = settings.clone();
        s.chat_tools_enabled = enable;
        let _ = crate::settings::save(&s);
        let status = if enable { "enabled 🟢" } else { "disabled 🔴" };
        return Ok(ChatReply {
            text: format!("Built-in device tools are now {status}."),
        });
    }

    if trimmed == "/skills" {
        let skills = crate::skills::list_skills();
        if skills.is_empty() {
            return Ok(ChatReply {
                text: "No skills found. Add `.md` skill files in the Coucou skills directory.".to_string(),
            });
        }
        let mut text = "### 🛠️ Available Skills\n\n".to_string();
        for s in skills {
            text.push_str(&format!("- **`/skill {}`**: {}\n", s.id, s.description));
        }
        text.push_str("\n*Usage: `/skill <name> <prompt>` to run a query with that skill.*");
        return Ok(ChatReply { text });
    }


    if trimmed == "/status" || trimmed == "/agent" || trimmed == "/agents" {
        let mut text = "### 🤖 AI Agent & Hook Status\n\n".to_string();
        text.push_str(&format!("- **Active Main Pill:** `{}`\n", settings.main_pill));
        text.push_str(&format!("- **Chat Provider:** `{}` (Model: `{}`)\n", settings.chat_provider, model_for(settings, &settings.chat_provider)));
        text.push_str(&format!("- **Built-in Tools Master:** {}\n", if settings.chat_tools_enabled { "🟢 ON" } else { "🔴 OFF" }));
        text.push_str(&format!("- **Mobile Phone Link:** {}\n", if settings.phone_link { "🟢 Enabled" } else { "⚪ Disabled" }));
        text.push_str(&format!("- **Mobile Tools Execution:** {}\n\n", if settings.phone_tools_enabled { "🟢 Allowed" } else { "🔴 Blocked" }));

        let hook_file = crate::settings::hook_exe_path();
        let hook_exists = hook_file.exists();
        text.push_str(&format!("- **Coucou Relay Binary:** {}\n\n", if hook_exists { "🟢 Installed" } else { "🔴 Missing" }));

        text.push_str("**Agent Hooks Status:**\n");
        let home = crate::platform::home_dir();
        for agent in [
            crate::agents::Agent::Antigravity,
            crate::agents::Agent::Cursor,
            crate::agents::Agent::Codex,
            crate::agents::Agent::Gemini,
            crate::agents::Agent::Copilot,
        ] {
            let installed = agent.installed(&home);
            let icon = if installed { "🟢" } else { "⚪" };
            text.push_str(&format!("- {icon} **{}**: {}\n", agent.name(), if installed { "Hook Connected" } else { "Not installed" }));
        }
        return Ok(ChatReply { text });
    }

    if trimmed == "/providers" {
        let text = format!("### 🌐 Integrated AI Providers & Agents\n\n\
- **🪐 Google Antigravity**: Autonomous pair-programming agent (`/antigravity <prompt>` or `/agy <prompt>`)\n\
- **Anthropic Claude**: `anthropic` (Claude 3.5 Sonnet / Haiku)\n\
- **Google Gemini**: `google` (Gemini 1.5 Pro / Flash)\n\
- **OpenAI**: `openai` (GPT-4o, GPT-4o-mini)\n\
- **OpenRouter**: `openrouter` (Multi-provider aggregator)\n\
- **Ollama**: `ollama` (Local models, e.g. Llama 3, Qwen)\n\
- **LM Studio**: `lmstudio` (Local OpenAI-compatible API)\n\
- **Custom Endpoint**: `custom` (Any custom base URL)\n\n\
**Active Provider:** `{}` (Model: `{}`)\n\n\
**Direct Dispatch:** Use `/prompt <provider> <your query>` to route immediately to any provider.\n\
**Antigravity Dispatch:** Use `/antigravity <task>` to write or execute directly for Google Antigravity.\n",
            settings.chat_provider, model_for(settings, &settings.chat_provider)
        );
        return Ok(ChatReply { text });
    }

    if trimmed == "/help" {
        let text = "### 💡 Available Commands\n\n\
- **/tools**: Check status of terminal, web, file, and system execution tools\n\
- **/tools on**: Enable built-in autonomous device execution tools\n\
- **/tools off**: Disable device execution tools (safe text-only mode)\n\
- **/skills**: List available skills and prompts\n\
- **/skill `<name>` `<prompt>`**: Run prompt using a specific skill\n\
- **/mcp**: View registered MCP servers and tools\n\
- **/mcp library**: Browse online curated & verified MCP library\n\
- **/mcp search `<query>`**: Search MCP library and npm registry\n\
- **/mcp install `<name>`**: Install an MCP server directly\n\
- **/mcp uninstall `<name>`**: Remove an MCP server\n\
- **/status**: View AI agent status, relay connections, and active pills\n\
- **/providers**: List all integrated AI providers & agents\n\
- **/antigravity `<prompt>`**: Send or write prompt specifically for Google Antigravity\n\
- **/prompt `<provider>` `<text>`**: Direct prompt to a specific provider (google, openai, claude, etc.)\n\
- **/clear**: Clear conversation history and reset session\n\
- **/help**: Show this help guide\n";
        return Ok(ChatReply { text: text.to_string() });
    }

    if trimmed == "/antigravity" || trimmed == "/agy" {
        return Ok(ChatReply { text: crate::antigravity::status_summary() });
    }

    let (query, skill_prompt, override_provider) = if let Some(rest) = query.strip_prefix("/antigravity ") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Ok(ChatReply { text: crate::antigravity::status_summary() });
        }
        return crate::antigravity::dispatch_prompt(rest).await;
    } else if let Some(rest) = query.strip_prefix("/agy ") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Ok(ChatReply { text: crate::antigravity::status_summary() });
        }
        return crate::antigravity::dispatch_prompt(rest).await;
    } else if let Some(rest) = query.strip_prefix("/prompt ") {
        let rest = rest.trim();
        let (target, user_prompt) = match rest.split_once(' ') {
            Some((t, p)) => (t.trim(), p.trim()),
            None => (rest, ""),
        };
        if target.eq_ignore_ascii_case("antigravity") || target.eq_ignore_ascii_case("agy") {
            if user_prompt.is_empty() {
                return Ok(ChatReply { text: crate::antigravity::status_summary() });
            }
            return crate::antigravity::dispatch_prompt(user_prompt).await;
        } else if ["anthropic", "google", "openai", "openrouter", "ollama", "lmstudio", "custom", "antigravity", "agy"].contains(&target.to_lowercase().as_str()) {
            (user_prompt.to_string(), None, Some(target.to_lowercase()))
        } else {
            (user_prompt.to_string(), None, None)
        }
    } else if let Some(rest) = query.strip_prefix("/skill ") {
        let rest = rest.trim();
        let (skill_id, user_prompt) = match rest.split_once(' ') {
            Some((id, p)) => (id.trim(), p.trim()),
            None => (rest, ""),
        };
        if let Some(skill) = crate::skills::find_skill(skill_id) {
            if user_prompt.is_empty() {
                return Ok(ChatReply {
                    text: format!("**Skill activated: {}**\n{}\n\nWhat would you like me to do?", skill.name, skill.description),
                });
            }
            let injection = format!("[Skill: {}]\n{}\n", skill.name, skill.instructions);
            (user_prompt.to_string(), Some(injection), None)
        } else {
            return Err(format!("Skill '{skill_id}' not found. Type `/skills` to see all available skills."));
        }
    } else {
        (query, None, None)
    };

    let context = context.map(checked_context).transpose()?;
    let effective_provider = override_provider.as_deref().unwrap_or(settings.chat_provider.as_str());
    let model = model_for(settings, effective_provider);
    if effective_provider == "antigravity" || effective_provider == "agy" {
        return crate::antigravity::dispatch_prompt(&query).await;
    }
    if effective_provider == ANTHROPIC || effective_provider.is_empty() {
        return claude::send(Some(app), chat, &model, query, context, skill_prompt.as_deref(), Some(settings)).await;
    }
    if let Some(p) = openai_compat::provider(effective_provider) {
        return openai_compat::send(Some(app), chat, p, &model, query, context, skill_prompt.as_deref(), Some(settings)).await;
    }
    if let Some(server) = local_chat::server(settings, effective_provider) {
        return local_chat::send(app, chat, &server, &model, query, context, skill_prompt.as_deref(), Some(settings)).await;
    }
    Err(format!("Unknown chat provider: {effective_provider}"))
}

/// One turn of the phone's conversation (phone_link/chat_backend.rs): the same providers as
/// [`send`], with no file or window context and no `AppHandle`. `on_delta` gets the streamed
/// text so far, which only the model servers produce; the cloud providers answer all at once.
pub async fn send_for_phone(
    settings: &Settings,
    chat: &Chat,
    provider: &str,
    model: &str,
    query: String,
    on_delta: impl FnMut(String),
) -> Result<ChatReply, String> {
    let trimmed = query.trim();
    if trimmed == "/tools" {
        let mut text = "### ⚡ Built-in Device Tools\n\n".to_string();
        let master = if settings.chat_tools_enabled { "🟢 ON" } else { "🔴 OFF" };
        let phone = if settings.phone_tools_enabled { "🟢 ALLOWED" } else { "🔴 BLOCKED" };
        text.push_str(&format!("Master: {} | Phone Execution: {}\n", master, phone));
        return Ok(ChatReply { text });
    }
    if trimmed == "/tools on" || trimmed == "/tools off" {
        let enable = trimmed == "/tools on";
        let mut s = settings.clone();
        s.chat_tools_enabled = enable;
        let _ = crate::settings::save(&s);
        let status = if enable { "enabled 🟢" } else { "disabled 🔴" };
        return Ok(ChatReply {
            text: format!("Built-in device tools are now {status}."),
        });
    }
    if trimmed == "/skills" {
        let skills = crate::skills::list_skills();
        let mut text = "### 🛠️ Available Skills\n\n".to_string();
        for s in skills {
            text.push_str(&format!("- `/skill {}`: {}\n", s.id, s.description));
        }
        return Ok(ChatReply { text });
    }
    if let Some(reply) = handle_mcp_command(trimmed).await {
        return Ok(reply);
    }
    if trimmed == "/status" || trimmed == "/agent" || trimmed == "/agents" {
        let mut text = "### 🤖 AI Agent & Hook Status\n\n".to_string();
        text.push_str(&format!("- **Active Main Pill:** `{}`\n", settings.main_pill));
        text.push_str(&format!("- **Chat Provider:** `{}` (Model: `{}`)\n", settings.chat_provider, model_for(settings, &settings.chat_provider)));
        text.push_str(&format!("- **Built-in Tools Master:** {}\n", if settings.chat_tools_enabled { "🟢 ON" } else { "🔴 OFF" }));
        text.push_str(&format!("- **Mobile Phone Link:** {}\n", if settings.phone_link { "🟢 Enabled" } else { "⚪ Disabled" }));
        text.push_str(&format!("- **Mobile Tools Execution:** {}\n\n", if settings.phone_tools_enabled { "🟢 Allowed" } else { "🔴 Blocked" }));

        let hook_file = crate::settings::hook_exe_path();
        let hook_exists = hook_file.exists();
        text.push_str(&format!("- **Coucou Relay Binary:** {}\n\n", if hook_exists { "🟢 Installed" } else { "🔴 Missing" }));

        text.push_str("**Agent Hooks Status:**\n");
        let home = crate::platform::home_dir();
        for agent in [
            crate::agents::Agent::Antigravity,
            crate::agents::Agent::Cursor,
            crate::agents::Agent::Codex,
            crate::agents::Agent::Gemini,
            crate::agents::Agent::Copilot,
        ] {
            let installed = agent.installed(&home);
            let icon = if installed { "🟢" } else { "⚪" };
            text.push_str(&format!("- {icon} **{}**: {}\n", agent.name(), if installed { "Hook Connected" } else { "Not installed" }));
        }
        return Ok(ChatReply { text });
    }
    if trimmed == "/providers" {
        let text = format!("### 🌐 Integrated AI Providers & Agents\n\n\
- **🪐 Google Antigravity**: Autonomous pair-programming agent (`/antigravity <prompt>` or `/agy <prompt>`)\n\
- **Anthropic Claude**: `anthropic` (Claude 3.5 Sonnet / Haiku)\n\
- **Google Gemini**: `google` (Gemini 1.5 Pro / Flash)\n\
- **OpenAI**: `openai` (GPT-4o, GPT-4o-mini)\n\
- **OpenRouter**: `openrouter` (Multi-provider aggregator)\n\
- **Ollama**: `ollama` (Local models, e.g. Llama 3, Qwen)\n\
- **LM Studio**: `lmstudio` (Local OpenAI-compatible API)\n\
- **Custom Endpoint**: `custom` (Any custom base URL)\n\n\
**Active Provider:** `{}` (Model: `{}`)\n\n\
**Direct Dispatch:** Use `/prompt <provider> <your query>` to route immediately to any provider.\n\
**Antigravity Dispatch:** Use `/antigravity <task>` to write or execute directly for Google Antigravity.\n",
            settings.chat_provider, model_for(settings, &settings.chat_provider)
        );
        return Ok(ChatReply { text });
    }
    if trimmed == "/help" {
        let text = "### 💡 Available Commands\n\n\
- **/tools**: Check status of terminal, web, file, and system execution tools\n\
- **/tools on**: Enable built-in autonomous device execution tools\n\
- **/tools off**: Disable device execution tools (safe text-only mode)\n\
- **/skills**: List available skills and prompts\n\
- **/skill `<name>` `<prompt>`**: Run prompt using a specific skill\n\
- **/mcp**: View registered MCP servers and tools\n\
- **/mcp library**: Browse online curated & verified MCP library\n\
- **/mcp search `<query>`**: Search MCP library and npm registry\n\
- **/mcp install `<name>`**: Install an MCP server directly\n\
- **/mcp uninstall `<name>`**: Remove an MCP server\n\
- **/status**: View AI agent status, relay connections, and active pills\n\
- **/providers**: List all integrated AI providers & agents\n\
- **/antigravity `<prompt>`**: Send or write prompt specifically for Google Antigravity\n\
- **/prompt `<provider>` `<text>`**: Direct prompt to a specific provider (google, openai, claude, etc.)\n\
- **/clear**: Clear conversation history and reset session\n\
- **/help**: Show this help guide\n";
        return Ok(ChatReply { text: text.to_string() });
    }

    if trimmed == "/antigravity" || trimmed == "/agy" {
        return Ok(ChatReply { text: crate::antigravity::status_summary() });
    }

    let (query, skill_prompt, override_provider) = if let Some(rest) = query.strip_prefix("/antigravity ") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Ok(ChatReply { text: crate::antigravity::status_summary() });
        }
        return crate::antigravity::dispatch_prompt(rest).await;
    } else if let Some(rest) = query.strip_prefix("/agy ") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Ok(ChatReply { text: crate::antigravity::status_summary() });
        }
        return crate::antigravity::dispatch_prompt(rest).await;
    } else if let Some(rest) = query.strip_prefix("/prompt ") {
        let rest = rest.trim();
        let (target, user_prompt) = match rest.split_once(' ') {
            Some((t, p)) => (t.trim(), p.trim()),
            None => (rest, ""),
        };
        if target.eq_ignore_ascii_case("antigravity") || target.eq_ignore_ascii_case("agy") {
            if user_prompt.is_empty() {
                return Ok(ChatReply { text: crate::antigravity::status_summary() });
            }
            return crate::antigravity::dispatch_prompt(user_prompt).await;
        } else if ["anthropic", "google", "openai", "openrouter", "ollama", "lmstudio", "custom", "antigravity", "agy"].contains(&target.to_lowercase().as_str()) {
            (user_prompt.to_string(), None, Some(target.to_lowercase()))
        } else {
            (user_prompt.to_string(), None, None)
        }
    } else if let Some(rest) = query.strip_prefix("/skill ") {
        let (skill_id, user_prompt) = match rest.trim().split_once(' ') {
            Some((id, p)) => (id.trim(), p.trim()),
            None => (rest.trim(), ""),
        };
        if let Some(skill) = crate::skills::find_skill(skill_id) {
            let injection = format!("[Skill: {}]\n{}\n", skill.name, skill.instructions);
            (user_prompt.to_string(), Some(injection), None)
        } else {
            (query, None, None)
        }
    } else {
        (query, None, None)
    };

    let tool_settings = if settings.phone_tools_enabled { Some(settings) } else { None };
    let effective_provider = override_provider.as_deref().unwrap_or(provider);
    let effective_model = if override_provider.is_some() {
        model_for(settings, effective_provider)
    } else {
        model.to_string()
    };

    if effective_provider == "antigravity" || effective_provider == "agy" {
        return crate::antigravity::dispatch_prompt(&query).await;
    }
    if effective_provider == ANTHROPIC {
        return claude::send(None, chat, &effective_model, query, None, skill_prompt.as_deref(), tool_settings).await;
    }
    if let Some(p) = openai_compat::provider(effective_provider) {
        return openai_compat::send(None, chat, p, &effective_model, query, None, skill_prompt.as_deref(), tool_settings).await;
    }
    if let Some(server) = local_chat::server(settings, effective_provider) {
        return local_chat::send_with(None, chat, &server, &effective_model, query, None, on_delta, skill_prompt.as_deref(), tool_settings).await;
    }
    Err(format!("Unknown chat provider: {effective_provider}"))
}

/// The models `provider` offers. Asked only when the user opens the picker on
/// that provider, and only once it has a key (or, for a local server, an
/// address): nothing is sent anywhere before that.
pub async fn models(settings: &Settings, provider: &str) -> Result<Vec<ModelInfo>, String> {
    if provider == "antigravity" || provider == "agy" {
        return Ok(vec![ModelInfo {
            id: "active".to_string(),
            label: "Active Session (Auto-detect)".to_string(),
        }]);
    }
    let no_key = || crate::i18n::t("No API key — add it in Settings.");
    if provider == ANTHROPIC {
        let key = secrets::get(claude::KEY).ok_or_else(no_key)?;
        return claude::models(&key).await;
    }
    if let Some(p) = openai_compat::provider(provider) {
        let key = secrets::get(p.key).ok_or_else(no_key)?;
        return openai_compat::models(p, &key).await;
    }
    if let Some(server) = local_chat::server(settings, provider) {
        return local_chat::models(&server).await;
    }
    Err(format!("Unknown chat provider: {provider}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn_texts(history: &[Value]) -> Vec<(String, Value)> {
        history
            .iter()
            .map(|m| (m["role"].as_str().unwrap().to_string(), m["content"].clone()))
            .collect()
    }

    #[test]
    fn a_turn_is_recorded_only_once_it_succeeds() {
        let chat = Chat::default();
        let t = chat.begin("anthropic");
        assert!(t.first);
        assert!(t.history.is_empty());
        // A failed turn records nothing: the next one is still the first.
        let t = chat.begin("anthropic");
        assert!(t.first);
        chat.commit(&t, json!({"role":"user","content":[{"type":"text","text":"hi"}]}), json!({"role":"assistant","content":[{"type":"text","text":"hello"}]}), "hi", "hello");
        let t = chat.begin("anthropic");
        assert!(!t.first);
        assert_eq!(t.history.len(), 2);
        assert!(t.history[0]["content"].is_array());
    }

    #[test]
    fn switching_provider_never_sends_claude_blocks_to_another_one() {
        let chat = Chat::default();
        let t = chat.begin("anthropic");
        let blocks = json!([
            {"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{"query":"x"}},
            {"type":"web_search_tool_result","tool_use_id":"srvtoolu_1","content":[]},
            {"type":"text","text":"Found it."}
        ]);
        chat.commit(&t, json!({"role":"user","content":[{"type":"text","text":"look"}]}), json!({"role":"assistant","content":blocks}), "look", "Found it.");

        let t = chat.begin("openai");
        assert!(!t.first);
        assert_eq!(
            turn_texts(&t.history),
            vec![("user".into(), json!("look")), ("assistant".into(), json!("Found it."))]
        );
        let raw = serde_json::to_string(&t.history).unwrap();
        assert!(!raw.contains("web_search"), "{raw}");

        // And back: Claude gets the plain turns too, including OpenAI's answer.
        chat.commit(&t, json!({"role":"user","content":"more"}), json!({"role":"assistant","content":"Sure."}), "more", "Sure.");
        let t = chat.begin("anthropic");
        assert_eq!(t.history.len(), 4);
        assert_eq!(t.history[3], json!({"role":"assistant","content":"Sure."}));
        assert!(!serde_json::to_string(&t.history).unwrap().contains("web_search"));
    }

    #[test]
    fn an_answer_that_lands_after_a_reset_or_a_switch_is_dropped() {
        let chat = Chat::default();
        let t = chat.begin("openai");
        chat.reset();
        chat.commit(&t, json!({"role":"user","content":"q"}), json!({"role":"assistant","content":"a"}), "q", "a");
        assert!(chat.begin("openai").first);

        let t = chat.begin("openai");
        let _other = chat.begin("google");
        chat.commit(&t, json!({"role":"user","content":"q"}), json!({"role":"assistant","content":"a"}), "q", "a");
        assert!(chat.begin("google").first);
    }

    #[test]
    fn the_prompt_greets_by_first_name_and_claims_web_search_only_for_claude() {
        let p = system_prompt_for(Some("Louis"), true, false);
        assert!(p.starts_with("You are Mochi, Louis's personal AI assistant living at the top of their screen."));
        assert!(p.contains("web search access"));
        assert!(p.contains("light Markdown"));
        let p = system_prompt_for(None, false, false);
        assert!(p.starts_with("You are Mochi, a personal AI assistant living at the top of the user's screen."));
        assert!(!p.contains("web search"));
        assert!(p.contains("no web access"));
        let p_tools = system_prompt_for(Some("Louis"), true, true);
        assert!(p_tools.contains("built-in device tools"));
    }

    #[test]
    fn context_goes_with_the_first_question_only() {
        let file = ChatContext::File { name: "a.txt".into(), path: "/x/a.txt".into() };
        assert_eq!(plain_question(true, Some(&file), "why?"), "File: a.txt\n\nwhy?");
        assert_eq!(plain_question(false, Some(&file), "why?"), "why?");
        let win = ChatContext::Window { app_name: "Code".into(), title: "main.rs".into(), url: None };
        assert_eq!(plain_question(true, Some(&win), "q"), "Context — App: Code, Window: main.rs\n\nq");
        assert_eq!(window_line("Edge", "Docs", Some("https://x.dev")), "Context — App: Edge, Window: Docs, URL: https://x.dev");
    }

    #[test]
    fn the_model_comes_from_the_settings_or_the_provider_default() {
        let mut s = Settings::default();
        assert_eq!(model_for(&s, "anthropic"), claude::DEFAULT_MODEL);
        assert_eq!(model_for(&s, "openai"), openai_compat::provider("openai").unwrap().default_model);
        assert_eq!(model_for(&s, "ollama"), "");
        s.chat_models.insert("openai".into(), " gpt-x ".into());
        s.chat_models.insert("ollama".into(), "llama3.2".into());
        s.model = "claude-haiku-4-5".into();
        assert_eq!(model_for(&s, "openai"), "gpt-x");
        assert_eq!(model_for(&s, "ollama"), "llama3.2");
        assert_eq!(model_for(&s, "anthropic"), "claude-haiku-4-5");
    }

    #[test]
    fn only_files_in_the_inbox_ride_along() {
        let base = std::env::temp_dir().join(format!("coucou-chat-ctx-{}", std::process::id()));
        let inbox = base.join("inbox");
        std::fs::create_dir_all(inbox.join("sub")).unwrap();
        std::fs::write(inbox.join("a.txt"), b"a").unwrap();
        std::fs::write(base.join("secret.txt"), b"s").unwrap();
        std::fs::write(inbox.join("sub").join("b.txt"), b"b").unwrap();
        assert!(is_inside(&inbox, &inbox.join("a.txt")));
        assert!(!is_inside(&inbox, &base.join("secret.txt")));
        assert!(!is_inside(&inbox, &inbox.join("..").join("secret.txt")));
        assert!(!is_inside(&inbox, &inbox.join("sub").join("b.txt")));
        assert!(!is_inside(&inbox, &inbox.join("missing.txt")));
        assert!(!is_inside(&inbox, &inbox));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("secret.txt"), inbox.join("link.txt")).unwrap();
            assert!(!is_inside(&inbox, &inbox.join("link.txt")));
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
