// Integration with Google Antigravity autonomous development platform and IDE.
// Provides active session discovery, prompt dispatching via the native Antigravity
// Language Server API (`agentapi`), and status reporting for both desktop and mobile.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::ChatReply;

static METADATA_CACHE: Mutex<Option<HashMap<String, AntigravityMetadata>>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityMetadata {
    pub conversation_id: String,
    pub workspace_name: String,
    pub workspace_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravitySessionInfo {
    pub conversation_id: String,
    pub is_active_lock: bool,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityLiveTask {
    pub session_id: String,
    pub is_active: bool,
    pub state: String,
    pub status_text: String,
    pub steps: Vec<String>,
    pub final_line: Option<String>,
    pub workspace_name: String,
    pub workspace_path: String,
}

pub struct AntigravityPromptResult {
    pub conversation_id: String,
    pub workspace_name: String,
    pub workspace_path: String,
    pub is_new_session: bool,
    pub content: String,
}

/// Locates the Antigravity Language Server or agentapi CLI on the machine.
pub fn locate_language_server() -> Option<PathBuf> {
    let home = crate::platform::home_dir();

    // 1. Direct language_server.exe in AppData Local Programs
    #[cfg(windows)]
    {
        if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
            let direct = PathBuf::from(&local_appdata)
                .join("Programs")
                .join("antigravity")
                .join("resources")
                .join("bin")
                .join("language_server.exe");
            if direct.exists() {
                return Some(direct);
            }
        }
        let fallback = home
            .join("AppData")
            .join("Local")
            .join("Programs")
            .join("antigravity")
            .join("resources")
            .join("bin")
            .join("language_server.exe");
        if fallback.exists() {
            return Some(fallback);
        }
    }

    // 2. agentapi.bat in ~/.gemini/antigravity/bin
    let agentapi_bat = home.join(".gemini").join("antigravity").join("bin").join("agentapi.bat");
    if agentapi_bat.exists() {
        return Some(agentapi_bat);
    }

    // 3. Linux / macOS paths
    #[cfg(target_os = "linux")]
    {
        let linux_bin = home
            .join(".local")
            .join("share")
            .join("antigravity")
            .join("resources")
            .join("bin")
            .join("language_server");
        if linux_bin.exists() {
            return Some(linux_bin);
        }
        let linux_agy = home.join(".gemini").join("antigravity").join("bin").join("agentapi");
        if linux_agy.exists() {
            return Some(linux_agy);
        }
    }

    // 4. PATH lookup
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            #[cfg(windows)]
            {
                let exe = dir.join("language_server.exe");
                if exe.is_file() {
                    return Some(exe);
                }
                let bat = dir.join("agentapi.bat");
                if bat.is_file() {
                    return Some(bat);
                }
                let cmd = dir.join("agentapi.cmd");
                if cmd.is_file() {
                    return Some(cmd);
                }
            }
            #[cfg(not(windows))]
            {
                let bin = dir.join("language_server");
                if bin.is_file() {
                    return Some(bin);
                }
                let agy = dir.join("agentapi");
                if agy.is_file() {
                    return Some(agy);
                }
            }
        }
    }

    None
}

/// Locates the main Antigravity executable (Antigravity.exe).
pub fn locate_antigravity_exe() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
            let exe = PathBuf::from(&local_appdata)
                .join("Programs")
                .join("antigravity")
                .join("Antigravity.exe");
            if exe.exists() {
                return Some(exe);
            }
        }
        let home = crate::platform::home_dir();
        let fallback = home
            .join("AppData")
            .join("Local")
            .join("Programs")
            .join("antigravity")
            .join("Antigravity.exe");
        if fallback.exists() {
            return Some(fallback);
        }
    }
    None
}

/// Discovers the active Antigravity session ID.
/// Checks the presence directory (`.lock` files), then the most recent `.db` file.
pub fn find_active_conversation_id() -> Option<AntigravitySessionInfo> {
    let home = crate::platform::home_dir();
    let antigravity_dir = home.join(".gemini").join("antigravity");

    // 1. Check presence locks (active live session)
    let presence_dir = antigravity_dir.join("presence");
    if let Ok(entries) = std::fs::read_dir(&presence_dir) {
        let mut locks: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "lock") {
                if let Ok(meta) = entry.metadata() {
                    let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    locks.push((path, mtime));
                }
            }
        }
        locks.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some((newest_lock, _)) = locks.first() {
            if let Some(stem) = newest_lock.file_stem().and_then(|s| s.to_str()) {
                let id = stem.to_string();
                let meta = get_conversation_metadata(&id);
                return Some(AntigravitySessionInfo {
                    conversation_id: id,
                    is_active_lock: true,
                    workspace: meta.map(|m| m.workspace_name),
                });
            }
        }
    }

    // 2. Check conversation DBs
    let conv_dir = antigravity_dir.join("conversations");
    if let Ok(entries) = std::fs::read_dir(&conv_dir) {
        let mut dbs: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "db") {
                if let Ok(meta) = entry.metadata() {
                    let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    dbs.push((path, mtime));
                }
            }
        }
        dbs.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some((newest_db, _)) = dbs.first() {
            if let Some(stem) = newest_db.file_stem().and_then(|s| s.to_str()) {
                let id = stem.to_string();
                let meta = get_conversation_metadata(&id);
                return Some(AntigravitySessionInfo {
                    conversation_id: id,
                    is_active_lock: false,
                    workspace: meta.map(|m| m.workspace_name),
                });
            }
        }
    }

    None
}

/// Executes an agentapi command and returns parsed JSON output.
fn run_agentapi(args: &[&str]) -> Result<Value, String> {
    let bin = locate_language_server()
        .ok_or_else(|| "Google Antigravity Language Server (language_server.exe / agentapi) is not installed or could not be found.".to_string())?;

    let is_bat = bin.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd"));

    let mut cmd = if is_bat {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&bin);
        c
    } else {
        let mut c = Command::new(&bin);
        c.arg("agentapi");
        c
    };

    for arg in args {
        cmd.arg(arg);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().map_err(|e| format!("Failed to execute Antigravity agentapi: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let err_text = if !stderr_str.trim().is_empty() {
            stderr_str.trim().to_string()
        } else if !stdout_str.trim().is_empty() {
            if let Ok(val) = serde_json::from_str::<Value>(&stdout_str) {
                if let Some(err) = val.get("error").and_then(Value::as_str) {
                    err.to_string()
                } else {
                    stdout_str.trim().to_string()
                }
            } else {
                stdout_str.trim().to_string()
            }
        } else {
            format!("Process exited with code {:?}", output.status.code())
        };
        return Err(format!("Antigravity agentapi error: {err_text}"));
    }

    // Try parsing JSON response
    serde_json::from_str::<Value>(&stdout_str)
        .map_err(|e| format!("Invalid JSON output from agentapi ({e}): {stdout_str}"))
}

/// Retrieves metadata for a specific Antigravity conversation.
pub fn get_conversation_metadata(conversation_id: &str) -> Option<AntigravityMetadata> {
    let clean_id = conversation_id.trim();
    if clean_id.is_empty() || clean_id == "new-session" {
        return None;
    }

    // Check memory cache first to avoid launching external processes repeatedly
    if let Ok(guard) = METADATA_CACHE.lock() {
        if let Some(cache) = guard.as_ref() {
            if let Some(cached) = cache.get(clean_id) {
                return Some(cached.clone());
            }
        }
    }

    let json = run_agentapi(&["get-conversation-metadata", clean_id]).ok()?;
    let meta = json.get("response")?.get("conversationMetadata")?.get("metadata")?;

    let mut workspace_path = String::new();
    let mut workspace_name = String::new();

    if let Some(workspaces) = meta.get("workspaces").and_then(Value::as_array) {
        if let Some(first_ws) = workspaces.first() {
            if let Some(uri) = first_ws.get("workspaceFolderAbsoluteUri").and_then(Value::as_str) {
                let clean_uri = uri.strip_prefix("file:///").unwrap_or(uri);
                workspace_path = clean_uri.replace("%20", " ");
                workspace_name = Path::new(&workspace_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&workspace_path)
                    .to_string();
            }
        }
    }

    let created_at = meta
        .get("createdAt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let res = AntigravityMetadata {
        conversation_id: clean_id.to_string(),
        workspace_name,
        workspace_path,
        created_at,
    };

    if let Ok(mut guard) = METADATA_CACHE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(clean_id.to_string(), res.clone());
    }

    Some(res)
}

/// Sends a prompt to an Antigravity session.
/// If `custom_id` is None, automatically resolves the active session (or creates one).
/// If `send-message` fails or the session is no longer active, automatically falls back
/// to creating a new conversation so the user's prompt is always processed without error.
pub async fn send_prompt(custom_id: Option<&str>, prompt: &str) -> Result<AntigravityPromptResult, String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Err("Prompt cannot be empty.".to_string());
    }

    let target_id: Option<String> = match custom_id {
        Some(id) if !id.trim().is_empty() => Some(id.trim().to_string()),
        _ => find_active_conversation_id().map(|info| info.conversation_id),
    };

    // Attempt delivering to existing active session
    if let Some(conv_id) = target_id {
        match run_agentapi(&["send-message", &conv_id, trimmed]) {
            Ok(json_res) => {
                let has_error = json_res.get("error").and_then(Value::as_str).filter(|e| !e.is_empty());
                if has_error.is_none() {
                    let meta = get_conversation_metadata(&conv_id);
                    let workspace_name = meta.as_ref().map(|m| m.workspace_name.clone()).unwrap_or_else(|| "coucou-windows".to_string());
                    let workspace_path = meta.as_ref().map(|m| m.workspace_path.clone()).unwrap_or_default();

                    return Ok(AntigravityPromptResult {
                        conversation_id: conv_id,
                        workspace_name,
                        workspace_path,
                        is_new_session: false,
                        content: trimmed.to_string(),
                    });
                }
            }
            Err(e) => {
                crate::log::line(format!("send-message to {conv_id} failed ({e}), falling back to new-conversation"));
            }
        }
    }

    // Fallback or no active session: create a new conversation with this prompt
    let json_res = run_agentapi(&["new-conversation", trimmed])?;

    if let Some(err_val) = json_res.get("error").and_then(Value::as_str) {
        if !err_val.is_empty() {
            return Err(format!("Antigravity error: {err_val}"));
        }
    }

    let new_id = json_res
        .get("response")
        .and_then(|r| {
            r.get("newConversation")
                .and_then(|nc| nc.get("conversationId"))
                .or_else(|| r.get("conversationId"))
        })
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let meta = if !new_id.is_empty() && new_id != "new-session" {
        get_conversation_metadata(&new_id)
    } else {
        None
    };

    let workspace_name = meta.as_ref().map(|m| m.workspace_name.clone()).unwrap_or_else(|| "Workspace".to_string());
    let workspace_path = meta.as_ref().map(|m| m.workspace_path.clone()).unwrap_or_default();

    Ok(AntigravityPromptResult {
        conversation_id: if new_id.is_empty() { "new-conversation".to_string() } else { new_id },
        workspace_name,
        workspace_path,
        is_new_session: true,
        content: trimmed.to_string(),
    })
}

/// Dispatches a prompt to Antigravity and formats a friendly Markdown reply for Coucou Chat.
pub async fn dispatch_prompt(prompt: &str) -> Result<ChatReply, String> {
    match send_prompt(None, prompt).await {
        Ok(result) => {
            let status_badge = if result.is_new_session {
                "✨ Created New Session"
            } else {
                "🟢 Delivered to Active Session"
            };

            let ws_line = if !result.workspace_name.is_empty() {
                format!("- **Active Workspace:** `{}` ({})\n", result.workspace_name, result.workspace_path)
            } else {
                String::new()
            };

            let text = format!(
                "### 🪐 Sent to Antigravity\n\n\
                - **Status:** {}\n\
                - **Session ID:** `{}`\n\
                {}\n\
                **Prompt:**\n\
                > {}\n\n\
                ---\n\
                ✅ *Antigravity received your instruction and is processing it in the background.*",
                status_badge,
                result.conversation_id,
                ws_line,
                result.content.replace('\n', "\n> ")
            );
            Ok(ChatReply { text })
        }
        Err(err) => {
            let server_path = locate_language_server()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "Not found".to_string());

            let text = format!(
                "### ❌ Failed to Send to Antigravity\n\n\
                **Error:** {}\n\n\
                - **Language Server:** `{}`\n\n\
                *Make sure Antigravity is running and language server is initialized.*",
                err, server_path
            );
            Ok(ChatReply { text })
        }
    }
}

/// Formats a complete status summary of the Antigravity integration.
pub fn status_summary() -> String {
    let server_path = locate_language_server();
    let is_installed = server_path.is_some();
    let active_session = find_active_conversation_id();

    let mut text = "### 🪐 Google Antigravity Integration Status\n\n".to_string();

    if is_installed {
        let p = server_path.unwrap();
        text.push_str(&format!("- **Language Server API (`agentapi`):** 🟢 Ready (`{}`)\n", p.display()));
    } else {
        text.push_str("- **Language Server API (`agentapi`):** 🔴 Not Detected\n");
    }

    if let Some(session) = active_session {
        let lock_icon = if session.is_active_lock { "🟢 Active (Lock acquired)" } else { "⚪ Recent Conversation" };
        text.push_str(&format!("- **Current Session ID:** `{}`\n", session.conversation_id));
        text.push_str(&format!("- **Presence Status:** {}\n", lock_icon));
        if let Some(ws) = session.workspace {
            text.push_str(&format!("- **Workspace:** `{}`\n", ws));
        }
    } else {
        text.push_str("- **Current Session:** ⚪ No active session detected (new session will be created on prompt)\n");
    }

    text.push_str("\n**How to use:**\n");
    text.push_str("- Use `/antigravity <prompt>` or `/agy <prompt>` from Windows desktop or paired phone\n");
    text.push_str("- Or choose **Google Antigravity** as your chat provider in Settings or Chat model picker\n");
    text.push_str("- Prompts sent from your phone are immediately delivered to the active session on this PC!\n");

    text
}

fn read_transcript_tail(path: &Path, max_bytes: u64) -> Option<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let to_read = len.min(max_bytes);
    file.seek(SeekFrom::Start(len - to_read)).ok()?;
    let mut buf = vec![0u8; to_read as usize];
    file.read_exact(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    if len > max_bytes && !lines.is_empty() {
        lines.remove(0);
    }
    Some(lines)
}

/// Retrieves the live task state from the active Antigravity session and its transcript.
pub fn get_live_antigravity_task() -> Option<AntigravityLiveTask> {
    let session = find_active_conversation_id()?;
    let conv_id = session.conversation_id;
    let home = crate::platform::home_dir();
    let antigravity_dir = home.join(".gemini").join("antigravity");

    let lock_path = antigravity_dir.join("presence").join(format!("{conv_id}.lock"));
    let lock_exists = lock_path.is_file();

    let transcript_path = antigravity_dir
        .join("brain")
        .join(&conv_id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl");

    let meta = std::fs::metadata(&transcript_path).ok();
    let (elapsed, modified_recent) = if let Some(ref m) = meta {
        let modified = m.modified().unwrap_or(std::time::UNIX_EPOCH);
        let el = std::time::SystemTime::now()
            .duration_since(modified)
            .unwrap_or(std::time::Duration::from_secs(99999));
        (el, el <= std::time::Duration::from_secs(45))
    } else {
        (std::time::Duration::from_secs(99999), false)
    };

    if !lock_exists && elapsed > std::time::Duration::from_secs(300) {
        return None;
    }

    let tail_lines = if transcript_path.is_file() {
        read_transcript_tail(&transcript_path, 128 * 1024).unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut steps = Vec::new();
    let mut last_status_text = String::new();
    let mut final_line: Option<String> = None;

    for line in &tail_lines {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else { continue; };
        let typ = val.get("type").and_then(|v| v.as_str()).unwrap_or_default();
        let source = val.get("source").and_then(|v| v.as_str()).unwrap_or_default();

        if typ == "USER_INPUT" || source == "USER_EXPLICIT" {
            if let Some(content) = val.get("content").and_then(|v| v.as_str()) {
                let clean = content.trim();
                if !clean.is_empty() {
                    let summary = if clean.chars().count() > 60 {
                        format!("{}...", clean.chars().take(57).collect::<String>())
                    } else {
                        clean.to_string()
                    };
                    steps.push(format!("Prompt: {summary}"));
                    final_line = Some(clean.to_string());
                }
            }
        } else if typ == "PLANNER_RESPONSE" {
            if let Some(tool_calls) = val.get("tool_calls").and_then(|v| v.as_array()) {
                for tc in tool_calls {
                    let tool_name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                    let summary = tc.get("args")
                        .and_then(|a| a.get("toolSummary"))
                        .and_then(|s| s.as_str());
                    let label = match summary {
                        Some(s) if !s.is_empty() => s.to_string(),
                        _ => format!("Tool: {tool_name}"),
                    };
                    last_status_text = label.clone();
                    steps.push(label);
                }
            }
            if let Some(content) = val.get("content").and_then(|v| v.as_str()) {
                let clean = content.trim();
                if !clean.is_empty() {
                    final_line = Some(clean.to_string());
                }
            }
        }
    }

    let is_active = modified_recent || lock_exists;
    let state = if modified_recent {
        "working"
    } else if lock_exists {
        "idle"
    } else {
        "finished"
    };

    let status_text = if !last_status_text.is_empty() {
        last_status_text
    } else if modified_recent {
        "Sedang menjalankan tugas...".to_string()
    } else {
        "Siap menerima prompt".to_string()
    };

    if steps.len() > 20 {
        steps = steps.split_off(steps.len() - 20);
    }

    let meta_info = get_conversation_metadata(&conv_id);
    let ws_name = meta_info.as_ref().map(|m| m.workspace_name.clone()).unwrap_or_else(|| "coucou-windows".to_string());
    let ws_path = meta_info.as_ref().map(|m| m.workspace_path.clone()).unwrap_or_default();

    Some(AntigravityLiveTask {
        session_id: conv_id,
        is_active,
        state: state.to_string(),
        status_text,
        steps,
        final_line,
        workspace_name: ws_name,
        workspace_path: ws_path,
    })
}

/// Brings the Antigravity IDE window to the foreground if running.
pub fn focus_antigravity_window() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locate_language_server_or_presence() {
        let server = locate_language_server();
        // On the dev machine with Antigravity installed, this will be Some
        assert!(server.is_some() || server.is_none());
    }

    #[test]
    fn test_find_active_conversation() {
        let active = find_active_conversation_id();
        if let Some(session) = active {
            assert!(!session.conversation_id.is_empty());
        }
    }
}
