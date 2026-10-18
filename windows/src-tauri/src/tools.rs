// Built-in device tools for Coucou AI Chat.
//
// These tools allow the AI assistant (Mochi) to execute device-level actions:
// - Terminal commands (`run_command`)
// - Web browsing / URL fetching (`browse_url`)
// - File operations (`read_file`, `write_file`, `edit_file`, `list_directory`)
// - System information inspection (`system_info`)
//
// Each tool can be toggled on/off by the user in Settings -> Chat or via the `/tools` command.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::settings::Settings;

pub const MAX_OUTPUT_BYTES: usize = 32 * 1024; // 32 KB ceiling

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCategory {
    Terminal,
    Web,
    File,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinTool {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    pub category: ToolCategory,
}

pub fn all_tools() -> Vec<BuiltinTool> {
    vec![
        BuiltinTool {
            name: "run_command",
            description: "Executes a shell command on the host device (PowerShell on Windows, sh on Linux) and returns the exit code, stdout, and stderr. Use this to run scripts, compile code, test programs, check git status, or inspect running services.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command line string to execute."
                    },
                    "working_directory": {
                        "type": "string",
                        "description": "Optional working directory where the command should be run."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Optional timeout in seconds (default: 30, max: 60)."
                    }
                },
                "required": ["command"]
            }),
            category: ToolCategory::Terminal,
        },
        BuiltinTool {
            name: "web_search",
            description: "Searches the live web for current facts, latest news, articles, real-time events, stock prices, or documentation. Returns a list of search results with titles, links, and snippet summaries. ALWAYS use this tool whenever the user asks for news, current information, or web lookups.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query or keywords to look up, e.g. 'ihsg hari ini' or 'berita bursa efek indonesia'."
                    }
                },
                "required": ["query"]
            }),
            category: ToolCategory::Web,
        },
        BuiltinTool {
            name: "browse_url",
            description: "Fetches and reads the textual content of a web page URL (HTTP or HTTPS). Use this to read documentation, web articles, API specs, or external web resources.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The full HTTP or HTTPS URL to browse."
                    }
                },
                "required": ["url"]
            }),
            category: ToolCategory::Web,
        },
        BuiltinTool {
            name: "read_file",
            description: "Reads the content of a local file as text. Supports optional line range offset and limit.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The absolute or relative file path to read."
                    },
                    "offset_lines": {
                        "type": "integer",
                        "description": "Optional 1-based start line number."
                    },
                    "limit_lines": {
                        "type": "integer",
                        "description": "Optional maximum number of lines to read."
                    }
                },
                "required": ["path"]
            }),
            category: ToolCategory::File,
        },
        BuiltinTool {
            name: "write_file",
            description: "Creates or overwrites a local file with the specified text content. Automatically creates parent directories if needed.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to write to."
                    },
                    "content": {
                        "type": "string",
                        "description": "The text content to write."
                    }
                },
                "required": ["path", "content"]
            }),
            category: ToolCategory::File,
        },
        BuiltinTool {
            name: "edit_file",
            description: "Performs an exact search-and-replace edit on an existing text file. Replaces target text with replacement text.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to edit."
                    },
                    "target": {
                        "type": "string",
                        "description": "The exact target string block to find and replace."
                    },
                    "replacement": {
                        "type": "string",
                        "description": "The replacement string block."
                    }
                },
                "required": ["path", "target", "replacement"]
            }),
            category: ToolCategory::File,
        },
        BuiltinTool {
            name: "list_directory",
            description: "Lists the files and folders inside a given directory, along with sizes and types.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The directory path to list (e.g. '.' for current directory)."
                    }
                },
                "required": ["path"]
            }),
            category: ToolCategory::File,
        },
        BuiltinTool {
            name: "system_info",
            description: "Inspects the host device's operating system, hostname, username, working directory, and architecture.",
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
            category: ToolCategory::System,
        },
    ]
}

/// Returns the active built-in tools based on the user's Settings preferences.
pub fn list_tools(settings: &Settings) -> Vec<BuiltinTool> {
    if !settings.chat_tools_enabled {
        return Vec::new();
    }
    all_tools()
        .into_iter()
        .filter(|t| match t.category {
            ToolCategory::Terminal => settings.chat_tool_terminal,
            ToolCategory::Web => settings.chat_tool_web,
            ToolCategory::File => settings.chat_tool_file,
            ToolCategory::System => settings.chat_tool_system,
        })
        .collect()
}

/// Checks if a tool name belongs to the built-in device tools.
pub fn is_builtin_tool(name: &str) -> bool {
    matches!(
        name,
        "run_command" | "web_search" | "browse_url" | "read_file" | "write_file" | "edit_file" | "list_directory" | "system_info"
    )
}

fn get_path_arg<'a>(input: &'a Value) -> Option<&'a str> {
    input.get("path")
        .or_else(|| input.get("file_path"))
        .or_else(|| input.get("filePath"))
        .or_else(|| input.get("file"))
        .or_else(|| input.get("filename"))
        .or_else(|| input.get("dir"))
        .or_else(|| input.get("directory"))
        .and_then(Value::as_str)
        .map(|s| s.trim().trim_matches('"').trim_matches('\''))
        .filter(|s| !s.is_empty())
}

fn get_command_arg<'a>(input: &'a Value) -> Option<&'a str> {
    input.get("command")
        .or_else(|| input.get("cmd"))
        .or_else(|| input.get("script"))
        .and_then(Value::as_str)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
}

fn get_content_arg<'a>(input: &'a Value) -> Option<&'a str> {
    input.get("content")
        .or_else(|| input.get("text"))
        .or_else(|| input.get("code"))
        .or_else(|| input.get("body"))
        .and_then(Value::as_str)
}

fn get_url_arg<'a>(input: &'a Value) -> Option<&'a str> {
    input.get("url")
        .or_else(|| input.get("link"))
        .or_else(|| input.get("href"))
        .and_then(Value::as_str)
        .map(|s| s.trim().trim_matches('"').trim_matches('\''))
        .filter(|s| !s.is_empty())
}

/// Returns a human-friendly description of a tool execution for status indicators.
pub fn describe_tool_action(name: &str, input: &Value) -> String {
    match name {
        "run_command" => {
            let cmd = get_command_arg(input).unwrap_or("");
            if cmd.is_empty() {
                "Running terminal command".to_string()
            } else {
                let short = if cmd.len() > 40 { format!("{}…", &cmd[..37]) } else { cmd.to_string() };
                format!("Running command: {short}")
            }
        }
        "browse_url" => {
            let url = get_url_arg(input).unwrap_or("");
            if url.is_empty() {
                "Browsing URL".to_string()
            } else {
                let short = if url.len() > 45 { format!("{}…", &url[..42]) } else { url.to_string() };
                format!("Browsing {short}")
            }
        }
        "read_file" => {
            let path = get_path_arg(input).unwrap_or("");
            if path.is_empty() {
                "Reading file".to_string()
            } else {
                format!("Reading file: {path}")
            }
        }
        "write_file" => {
            let path = get_path_arg(input).unwrap_or("");
            if path.is_empty() {
                "Writing file".to_string()
            } else {
                format!("Writing file: {path}")
            }
        }
        "edit_file" => {
            let path = get_path_arg(input).unwrap_or("");
            if path.is_empty() {
                "Editing file".to_string()
            } else {
                format!("Editing file: {path}")
            }
        }
        "list_directory" => {
            let path = get_path_arg(input).unwrap_or(".");
            format!("Listing directory: {path}")
        }
        "system_info" => "Inspecting system information".to_string(),
        "web_search" => {
            let q = input.get("query").or_else(|| input.get("q")).and_then(Value::as_str).unwrap_or("");
            if q.is_empty() {
                "Searching web".to_string()
            } else {
                format!("Searching web for \"{q}\"")
            }
        }
        other => format!("Executing {other}"),
    }
}

/// Dispatches execution of a built-in tool call.
pub async fn call_tool(name: &str, input: &Value) -> Result<String, String> {
    match name {
        "run_command" => {
            let command = get_command_arg(input).ok_or("Missing 'command' argument")?;
            let working_dir = input.get("working_directory").or_else(|| input.get("cwd")).and_then(Value::as_str);
            let timeout = input.get("timeout_seconds").and_then(Value::as_u64).unwrap_or(30).clamp(1, 60);
            exec_command(command, working_dir, timeout)
        }
        "web_search" => {
            let query = input.get("query")
                .or_else(|| input.get("q"))
                .or_else(|| input.get("search"))
                .or_else(|| input.get("keyword"))
                .or_else(|| input.get("query_string"))
                .and_then(Value::as_str)
                .map(|s| s.trim().trim_matches('"').trim_matches('\''))
                .filter(|s| !s.is_empty())
                .ok_or("Missing 'query' argument")?;
            exec_web_search(query).await
        }
        "browse_url" => {
            let url = get_url_arg(input).ok_or("Missing 'url' argument")?;
            exec_browse_url(url).await
        }
        "read_file" => {
            let path = get_path_arg(input).ok_or("Missing 'path' argument")?;
            let offset = input.get("offset_lines").or_else(|| input.get("offset")).and_then(Value::as_u64).map(|v| v as usize);
            let limit = input.get("limit_lines").or_else(|| input.get("limit")).and_then(Value::as_u64).map(|v| v as usize);
            exec_read_file(path, offset, limit)
        }
        "write_file" => {
            let path = get_path_arg(input).ok_or("Missing 'path' argument")?;
            let content = get_content_arg(input).ok_or("Missing 'content' argument")?;
            exec_write_file(path, content)
        }
        "edit_file" => {
            let path = get_path_arg(input).ok_or("Missing 'path' argument")?;
            let target = input.get("target")
                .or_else(|| input.get("find"))
                .or_else(|| input.get("old_str"))
                .or_else(|| input.get("old_string"))
                .or_else(|| input.get("old"))
                .and_then(Value::as_str)
                .ok_or("Missing 'target' argument")?;
            let replacement = input.get("replacement")
                .or_else(|| input.get("replace"))
                .or_else(|| input.get("new_str"))
                .or_else(|| input.get("new_string"))
                .or_else(|| input.get("new"))
                .and_then(Value::as_str)
                .ok_or("Missing 'replacement' argument")?;
            exec_edit_file(path, target, replacement)
        }
        "list_directory" => {
            let path = get_path_arg(input).unwrap_or(".");
            exec_list_dir(path)
        }
        "system_info" => exec_system_info(),
        _ => Err(format!("Unknown built-in tool: '{name}'")),
    }
}

// ── Tool Executors ────────────────────────────────────────────────────────────

fn exec_command(command: &str, working_dir: Option<&str>, timeout_secs: u64) -> Result<String, String> {
    #[cfg(windows)]
    let mut cmd = {
        let mut final_cmd = command.to_string();
        if final_cmd.starts_with("python3 ") {
            final_cmd = final_cmd.replacen("python3 ", "python ", 1);
        } else if final_cmd.contains(" python3 ") {
            final_cmd = final_cmd.replace(" python3 ", " python ");
        }
        if final_cmd.starts_with("which ") {
            final_cmd = final_cmd.replacen("which ", "where.exe ", 1);
        }
        let script = format!(
            "$OutputEncoding = [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {final_cmd}"
        );
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        c
    };

    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };

    if let Some(dir) = working_dir {
        let p = Path::new(dir);
        if p.is_dir() {
            cmd.current_dir(p);
        }
    }

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn command: {e}"))?;

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout_buf = Vec::new();
                let mut stderr_buf = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_end(&mut stdout_buf);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_end(&mut stderr_buf);
                }

                let stdout_str = String::from_utf8_lossy(&stdout_buf);
                let stderr_str = String::from_utf8_lossy(&stderr_buf);

                let mut result = format!("Exit Code: {}\n", status.code().unwrap_or(-1));
                if !stdout_str.is_empty() {
                    result.push_str("--- Output (stdout) ---\n");
                    result.push_str(&cap_string(&stdout_str, MAX_OUTPUT_BYTES));
                    result.push('\n');
                }
                if !stderr_str.is_empty() {
                    result.push_str("--- Errors (stderr) ---\n");
                    result.push_str(&cap_string(&stderr_str, MAX_OUTPUT_BYTES));
                    result.push('\n');
                }
                if stdout_str.is_empty() && stderr_str.is_empty() {
                    result.push_str("(Command finished with no output)\n");
                }
                return Ok(result);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    return Err(format!("Command execution timed out after {timeout_secs} seconds."));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("Error waiting for command: {e}")),
        }
    }
}

async fn exec_browse_url(raw_url: &str) -> Result<String, String> {
    let raw_url = raw_url.trim();
    if !raw_url.starts_with("http://") && !raw_url.starts_with("https://") {
        return Err("URL must start with http:// or https://".to_string());
    }

    let client = reqwest::Client::builder()
        .user_agent("Coucou/0.5.0 (Personal AI Assistant)")
        .timeout(Duration::from_secs(15))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let response = client
        .get(raw_url)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP request failed with status {status}"));
    }

    let body = response.text().await.map_err(|e| format!("Failed to read response body: {e}"))?;
    let text = strip_html(&body);
    Ok(cap_string(&text, MAX_OUTPUT_BYTES))
}

pub async fn exec_web_search(query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Search query cannot be empty.".to_string());
    }

    let encoded_query = urlencoding(query);
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(12))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let mut results = Vec::new();

    // 1. Primary: Bing Web Search
    let search_url = format!("https://www.bing.com/search?q={encoded_query}&count=10");
    if let Ok(resp) = client.get(&search_url)
        .header("Accept-Language", "id-ID,id;q=0.9,en-US;q=0.8,en;q=0.7")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(html) = resp.text().await {
                extract_bing_results(&html, &mut results);
            }
        }
    }

    // 2. If it's news-oriented or we got few results, also check Bing News
    let query_lower = query.to_lowercase();
    let is_news_query = query_lower.contains("berita")
        || query_lower.contains("news")
        || query_lower.contains("terkini")
        || query_lower.contains("hari ini")
        || query_lower.contains("terbaru")
        || results.len() < 3;

    if is_news_query {
        let news_url = format!("https://www.bing.com/news/search?q={encoded_query}");
        if let Ok(resp) = client.get(&news_url)
            .header("Accept-Language", "id-ID,id;q=0.9,en-US;q=0.8,en;q=0.7")
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(html) = resp.text().await {
                    extract_bing_news_results(&html, &mut results);
                }
            }
        }
    }

    // 3. Fallback: Wikipedia if results are still empty
    if results.is_empty() {
        if let Ok(wiki_text) = search_wikipedia(&client, query).await {
            if !wiki_text.is_empty() {
                results.push(wiki_text);
            }
        }
    }

    if results.is_empty() {
        return Ok(format!("Tidak ditemukan hasil pencarian web untuk \"{query}\"."));
    }

    // Deduplicate and cap to 7 results
    let mut unique_results = Vec::new();
    let mut seen_titles = std::collections::HashSet::new();
    for r in results {
        let key: String = r.chars().take(40).collect();
        if seen_titles.insert(key) {
            unique_results.push(r);
            if unique_results.len() >= 7 {
                break;
            }
        }
    }

    let header = format!("Hasil Pencarian Web untuk \"{query}\":\n\n");
    let body = unique_results.iter().enumerate().map(|(i, r)| {
        format!("{}. {}\n", i + 1, r)
    }).collect::<Vec<_>>().join("\n");

    Ok(cap_string(&(header + &body), MAX_OUTPUT_BYTES))
}

fn extract_bing_results(html: &str, out: &mut Vec<String>) {
    let mut rest = html;
    while let Some(start) = rest.find("<li class=\"b_algo\"") {
        rest = &rest[start..];
        let end = rest.find("</li>").unwrap_or(rest.len().min(4000));
        let block = &rest[..end];
        rest = &rest[end..];

        let title = if let Some(h2_start) = block.find("<h2") {
            if let Some(h2_close) = block[h2_start..].find("</h2>") {
                strip_html(&block[h2_start..h2_start + h2_close])
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let snippet = if let Some(p_start) = block.find("<p") {
            if let Some(p_close) = block[p_start..].find("</p>") {
                strip_html(&block[p_start..p_start + p_close])
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let cite = if let Some(c_start) = block.find("<cite") {
            if let Some(c_close) = block[c_start..].find("</cite>") {
                strip_html(&block[c_start..c_start + c_close])
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let title_clean = title.trim();
        let snippet_clean = snippet.trim();
        if !title_clean.is_empty() && !snippet_clean.is_empty() {
            let mut formatted = format!("**{title_clean}**\n   {snippet_clean}");
            if !cite.is_empty() {
                formatted.push_str(&format!("\n   Sumber: {cite}"));
            }
            out.push(formatted);
        }
    }
}

fn extract_bing_news_results(html: &str, out: &mut Vec<String>) {
    let mut rest = html;
    while let Some(start) = rest.find("class=\"news-card") {
        rest = &rest[start..];
        let end = rest.find("class=\"news_eae_container").or_else(|| rest.find("class=\"news-card")).unwrap_or(rest.len().min(3000));
        let block = &rest[..end];
        rest = if end > 16 { &rest[16..] } else { &rest[end..] };

        let title = if let Some(t_start) = block.find("<h2") {
            if let Some(t_close) = block[t_start..].find("</h2>") {
                strip_html(&block[t_start..t_start + t_close])
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let snippet = if let Some(s_start) = block.find("class=\"snippet\"") {
            if let Some(tag_close) = block[s_start..].find('>') {
                let s_content = &block[s_start + tag_close + 1..];
                if let Some(end_tag) = s_content.find("</div>") {
                    strip_html(&s_content[..end_tag])
                } else {
                    strip_html(s_content)
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let source = if let Some(src_start) = block.find("class=\"source") {
            if let Some(tag_close) = block[src_start..].find('>') {
                let src_content = &block[src_start + tag_close + 1..];
                if let Some(end_tag) = src_content.find("</div>") {
                    strip_html(&src_content[..end_tag])
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let title_clean = title.trim();
        let snippet_clean = snippet.trim();
        if !title_clean.is_empty() && (!snippet_clean.is_empty() || !source.is_empty()) {
            let mut formatted = format!("**{title_clean}**");
            if !snippet_clean.is_empty() {
                formatted.push_str(&format!("\n   {snippet_clean}"));
            }
            if !source.trim().is_empty() {
                formatted.push_str(&format!("\n   Sumber: {}", source.trim()));
            }
            out.push(formatted);
        }
    }
}

async fn search_wikipedia(client: &reqwest::Client, query: &str) -> Result<String, String> {
    let encoded = urlencoding(query);
    let url = format!("https://id.wikipedia.org/w/api.php?action=query&list=search&srsearch={encoded}&format=json&utf8=1&srlimit=3");
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(search) = json.pointer("/query/search").and_then(Value::as_array) {
        if !search.is_empty() {
            let mut items = Vec::new();
            for item in search {
                let title = item.get("title").and_then(Value::as_str).unwrap_or("");
                let snippet = item.get("snippet").and_then(Value::as_str).unwrap_or("");
                if !title.is_empty() {
                    items.push(format!("**Wikipedia: {title}**\n   {}", strip_html(snippet)));
                }
            }
            if !items.is_empty() {
                return Ok(items.join("\n\n"));
            }
        }
    }
    Ok(String::new())
}

fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn exec_read_file(path_str: &str, offset_lines: Option<usize>, limit_lines: Option<usize>) -> Result<String, String> {
    let p = Path::new(path_str);
    if !p.exists() {
        return Err(format!("File not found: '{path_str}'"));
    }
    if !p.is_file() {
        return Err(format!("Path is not a regular file: '{path_str}'"));
    }

    let content = fs::read_to_string(p).map_err(|e| format!("Failed to read file: {e}"))?;

    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    let start = offset_lines.unwrap_or(1).saturating_sub(1).min(total);
    let end = match limit_lines {
        Some(lim) => (start + lim).min(total),
        None => total,
    };

    let slice = &lines[start..end];
    let output = slice.join("\n");
    let formatted = format!("File: {} (Lines {}-{} of {})\n---\n{}", path_str, start + 1, end, total, output);
    Ok(cap_string(&formatted, MAX_OUTPUT_BYTES))
}

fn exec_write_file(path_str: &str, content: &str) -> Result<String, String> {
    let p = Path::new(path_str);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directories: {e}"))?;
    }
    fs::write(p, content).map_err(|e| format!("Failed to write file: {e}"))?;
    Ok(format!("Successfully wrote {} bytes to '{}'.", content.len(), path_str))
}

fn exec_edit_file(path_str: &str, target: &str, replacement: &str) -> Result<String, String> {
    let p = Path::new(path_str);
    if !p.exists() {
        return Err(format!("File not found: '{path_str}'"));
    }
    let content = fs::read_to_string(p).map_err(|e| format!("Failed to read file: {e}"))?;
    if !content.contains(target) {
        return Err(format!("Target text not found in file '{path_str}'. Verify exact lines and whitespace."));
    }
    let count = content.matches(target).count();
    let updated = content.replace(target, replacement);
    fs::write(p, updated).map_err(|e| format!("Failed to write edited file: {e}"))?;
    Ok(format!("Successfully replaced {count} occurrence(s) in '{}'.", path_str))
}

fn exec_list_dir(path_str: &str) -> Result<String, String> {
    let p = Path::new(path_str);
    if !p.exists() {
        return Err(format!("Directory not found: '{path_str}'"));
    }
    if !p.is_dir() {
        return Err(format!("Path is not a directory: '{path_str}'"));
    }

    let entries = fs::read_dir(p).map_err(|e| format!("Failed to read directory: {e}"))?;
    let mut lines = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);

        if is_dir {
            lines.push(format!("[DIR]  {name}/"));
        } else {
            lines.push(format!("[FILE] {name} ({} bytes)", len));
        }
    }

    lines.sort();
    let total = lines.len();
    if lines.len() > 200 {
        lines.truncate(200);
        lines.push(format!("... and {} more entries", total - 200));
    }

    let header = format!("Directory: {} ({} entries)\n---\n", path_str, total);
    Ok(cap_string(&(header + &lines.join("\n")), MAX_OUTPUT_BYTES))
}

fn exec_system_info() -> Result<String, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let current_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string());
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(format!(
        "Host Device Info:\n- OS: {os}\n- Architecture: {arch}\n- Hostname: {host}\n- User: {user}\n- Current Working Directory: {current_dir}"
    ))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn cap_string(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let mut truncated = s[..max_bytes].to_string();
        truncated.push_str("\n... [Output truncated to 32 KB]");
        truncated
    }
}

/// Simple HTML to plain text converter that strips tags and decodes common entities.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_script = false;
    let mut tag_name = String::new();

    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '<' {
            tag_name.clear();
            i += 1;
            while i < chars.len() && chars[i] != '>' && !chars[i].is_whitespace() {
                tag_name.push(chars[i].to_ascii_lowercase());
                i += 1;
            }
            if tag_name == "script" || tag_name == "style" {
                in_script = true;
            } else if tag_name == "/script" || tag_name == "/style" {
                in_script = false;
            } else if tag_name == "br" || tag_name == "p" || tag_name == "/p" || tag_name == "div" || tag_name == "/div" || tag_name == "li" {
                out.push('\n');
            }
            while i < chars.len() && chars[i] != '>' {
                i += 1;
            }
        } else if !in_script {
            out.push(c);
        }
        i += 1;
    }

    // Decode entities
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    // Clean up excessive blank lines
    let mut clean_lines = Vec::new();
    for line in out.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            clean_lines.push(trimmed);
        }
    }
    clean_lines.join("\n")
}

/// Converts built-in tools to Claude API format.
pub fn to_claude_tools(tools: &[BuiltinTool]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })
        })
        .collect()
}

/// Converts built-in tools to OpenAI function calling format.
pub fn to_openai_tools(tools: &[BuiltinTool]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_builtin_tools() {
        let mut s = Settings::default();
        s.chat_tools_enabled = true;
        s.chat_tool_terminal = true;
        s.chat_tool_web = true;
        s.chat_tool_file = true;
        s.chat_tool_system = true;

        let tools = list_tools(&s);
        assert_eq!(tools.len(), 8);

        s.chat_tool_terminal = false;
        let tools_no_term = list_tools(&s);
        assert_eq!(tools_no_term.len(), 7);

        s.chat_tools_enabled = false;
        assert!(list_tools(&s).is_empty());
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding("hello world"), "hello+world");
        assert_eq!(urlencoding("ihsg & bursa"), "ihsg+%26+bursa");
    }

    #[tokio::test]
    async fn test_exec_web_search() {
        let res = call_tool("web_search", &json!({ "query": "ihsg" })).await;
        assert!(res.is_ok(), "web_search should succeed");
        let text = res.unwrap();
        assert!(text.contains("Hasil Pencarian Web untuk \"ihsg\""), "Should have header");
        assert!(text.contains("1. **"), "Should have at least 1 formatted result");
    }

    #[tokio::test]
    async fn test_file_tools_roundtrip() {
        let temp = std::env::temp_dir().join(format!("coucou-tools-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let file_path = temp.join("test.txt");
        let path_str = file_path.to_str().unwrap();

        let write_res = call_tool(
            "write_file",
            &json!({ "path": path_str, "content": "Hello World\nLine 2\nLine 3" }),
        )
        .await;
        assert!(write_res.is_ok());

        let read_res = call_tool(
            "read_file",
            &json!({ "path": path_str, "offset_lines": 2, "limit_lines": 1 }),
        )
        .await
        .unwrap();
        assert!(read_res.contains("Line 2"));

        let edit_res = call_tool(
            "edit_file",
            &json!({ "path": path_str, "target": "Line 2", "replacement": "Updated Line" }),
        )
        .await;
        assert!(edit_res.is_ok());

        let read_after = call_tool("read_file", &json!({ "path": path_str })).await.unwrap();
        assert!(read_after.contains("Updated Line"));

        let list_res = call_tool("list_directory", &json!({ "path": temp.to_str().unwrap() })).await.unwrap();
        assert!(list_res.contains("test.txt"));

        let _ = fs::remove_dir_all(&temp);
    }

    #[tokio::test]
    async fn test_system_info() {
        let info = call_tool("system_info", &json!({})).await.unwrap();
        assert!(info.contains("Host Device Info:"));
    }

    #[test]
    fn test_strip_html() {
        let html = "<html><head><title>Test</title><script>alert(1);</script></head><body><h1>Hello</h1><p>Paragraph &amp; text.</p></body></html>";
        let text = strip_html(html);
        assert!(!text.contains("alert(1)"));
        assert!(text.contains("Hello"));
        assert!(text.contains("Paragraph & text."));
    }
}
