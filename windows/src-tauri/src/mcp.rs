// Model Context Protocol (MCP) client & registry integration for Coucou.
//
// Discovers MCP servers from standard configuration files:
//   - %APPDATA%\Coucou\mcp.json (or ~/.config/coucou/mcp.json on Linux)
//   - %APPDATA%\Claude\claude_desktop_config.json
//
// Connects to online MCP catalogs (npm registry, curated verified servers),
// installs servers directly into mcp.json, spawns stdio processes,
// enumerates tools (`tools/list`), and executes tools (`tools/call`).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::platform;

const MCP_TIMEOUT: Duration = Duration::from_secs(25);
const CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub server: String,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRegistryEntry {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub package: String,
    pub command: String,
    pub args: Vec<String>,
    pub env_keys: Vec<String>,
    pub default_env: HashMap<String, String>,
    pub installed: bool,
    pub official: bool,
    pub website: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpInstallOutcome {
    pub server_name: String,
    pub tools_count: usize,
    pub success: bool,
    pub message: String,
}

struct ToolCache {
    tools: Vec<McpTool>,
    updated_at: Instant,
}

static CACHE: OnceLock<Mutex<Option<ToolCache>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<ToolCache>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Known paths where MCP server configurations are stored.
pub fn config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. Coucou's own mcp.json
    let coucou_cfg = platform::config_dir().join("mcp.json");
    if coucou_cfg.exists() {
        paths.push(coucou_cfg);
    }

    // 2. Claude Desktop's claude_desktop_config.json
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let claude_cfg = PathBuf::from(appdata).join("Claude").join("claude_desktop_config.json");
        if claude_cfg.exists() {
            paths.push(claude_cfg);
        }
    }
    #[cfg(target_os = "linux")]
    if let Some(home) = std::env::var_os("HOME") {
        let claude_cfg = PathBuf::from(home).join(".config").join("Claude").join("claude_desktop_config.json");
        if claude_cfg.exists() {
            paths.push(claude_cfg);
        }
    }

    paths
}

/// Coucou's own mcp.json path (created if user edits or configures it).
pub fn coucou_mcp_path() -> PathBuf {
    platform::config_dir().join("mcp.json")
}

/// Reads and merges all discovered MCP server configurations.
pub fn load_servers() -> HashMap<String, McpServerConfig> {
    let mut servers = HashMap::new();
    for path in config_paths() {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(cfg) = serde_json::from_slice::<McpConfig>(&bytes) {
                for (name, server) in cfg.mcp_servers {
                    servers.insert(name, server);
                }
            }
        }
    }
    servers
}

/// Saves a server configuration to Coucou's mcp.json.
pub fn save_server(name: &str, config: McpServerConfig) -> Result<(), String> {
    let path = coucou_mcp_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {e}"))?;
    }
    let mut cfg = if path.exists() {
        let bytes = std::fs::read(&path).map_err(|e| format!("Read error: {e}"))?;
        serde_json::from_slice::<McpConfig>(&bytes).unwrap_or_default()
    } else {
        McpConfig::default()
    };
    cfg.mcp_servers.insert(name.to_string(), config);
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Write error: {e}"))?;
    invalidate_cache();
    Ok(())
}

/// Removes a server configuration from Coucou's mcp.json.
pub fn remove_server(name: &str) -> Result<bool, String> {
    let path = coucou_mcp_path();
    if !path.exists() {
        return Ok(false);
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("Read error: {e}"))?;
    let mut cfg: McpConfig = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let removed = cfg.mcp_servers.remove(name).is_some();
    if removed {
        let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| format!("Write error: {e}"))?;
        invalidate_cache();
    }
    Ok(removed)
}

/// Runs a single JSON-RPC 2.0 request/response exchange on a fresh child process.
fn run_rpc(
    server: &McpServerConfig,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    #[cfg(windows)]
    let mut cmd = {
        let raw = server.command.trim();
        let lower = raw.to_lowercase();
        if lower == "npx" || lower == "npm" || lower == "pnpm" || lower == "yarn" || lower == "uvx" || lower.ends_with(".cmd") || lower.ends_with(".bat") {
            let mut c = Command::new("cmd.exe");
            c.arg("/c").arg(raw);
            c.args(&server.args);
            c
        } else {
            let mut c = Command::new(raw);
            c.args(&server.args);
            c
        }
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new(&server.command);
        c.args(&server.args);
        c
    };

    cmd.envs(&server.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn MCP server '{}': {e}", server.command))?;
    let mut stdin = child.stdin.take().ok_or_else(|| "Failed to capture MCP stdin".to_string())?;
    let stdout = child.stdout.take().ok_or_else(|| "Failed to capture MCP stdout".to_string())?;
    let mut reader = BufReader::new(stdout);

    // 1. Handshake: initialize
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "Coucou", "version": "0.5.0" }
        }
    });
    let mut init_line = serde_json::to_string(&init_req).map_err(|e| e.to_string())?;
    init_line.push('\n');
    stdin.write_all(init_line.as_bytes()).map_err(|e| format!("Write error: {e}"))?;
    stdin.flush().map_err(|e| format!("Flush error: {e}"))?;

    // Read initialize response (skip any npm banners, warnings, or empty lines)
    let start_init = Instant::now();
    let mut init_done = false;
    let mut line = String::new();
    while start_init.elapsed() < MCP_TIMEOUT {
        line.clear();
        if reader.read_line(&mut line).is_err() {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if v.get("id").and_then(Value::as_i64) == Some(1) {
                init_done = true;
                break;
            }
        }
    }
    if !init_done {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("MCP server '{}' failed to respond to initialize", server.command));
    }

    // 2. Handshake: initialized notification
    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    let mut notif_line = serde_json::to_string(&notif).map_err(|e| e.to_string())?;
    notif_line.push('\n');
    let _ = stdin.write_all(notif_line.as_bytes());
    let _ = stdin.flush();

    // 3. Send requested method
    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": method,
        "params": params
    });
    let mut req_line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    req_line.push('\n');
    stdin.write_all(req_line.as_bytes()).map_err(|e| format!("Write error: {e}"))?;
    stdin.flush().map_err(|e| format!("Flush error: {e}"))?;

    // Read response line (skip empty lines or notifications if any)
    let start_method = Instant::now();
    let mut resp_value: Option<Value> = None;
    while start_method.elapsed() < MCP_TIMEOUT {
        line.clear();
        if reader.read_line(&mut line).is_err() {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if v.get("id").and_then(Value::as_i64) == Some(2) {
                resp_value = Some(v);
                break;
            }
        }
    }

    // Clean up child process
    let _ = child.kill();
    let _ = child.wait();

    let resp = resp_value.ok_or_else(|| "MCP request timed out".to_string())?;
    if let Some(err) = resp.get("error") {
        return Err(err.get("message").and_then(Value::as_str).unwrap_or("MCP error").to_string());
    }
    resp.get("result").cloned().ok_or_else(|| "Missing result in MCP response".to_string())
}

/// Discovers tools from all configured MCP servers.
pub fn list_tools() -> Vec<McpTool> {
    {
        let lock = cache().lock().unwrap();
        if let Some(c) = &*lock {
            if c.updated_at.elapsed() < CACHE_TTL {
                return c.tools.clone();
            }
        }
    }

    let servers = load_servers();
    let mut tools = Vec::new();

    for (server_name, config) in servers {
        if let Ok(res) = run_rpc(&config, "tools/list", json!({})) {
            if let Some(raw_tools) = res.get("tools").and_then(Value::as_array) {
                for t in raw_tools {
                    let raw_name = t.get("name").and_then(Value::as_str).unwrap_or_default();
                    if raw_name.is_empty() {
                        continue;
                    }
                    let desc = t.get("description").and_then(Value::as_str).unwrap_or_default();
                    let schema = t.get("inputSchema").cloned().unwrap_or_else(|| json!({"type": "object"}));
                    let display_name = format!("{server_name}__{raw_name}");
                    tools.push(McpTool {
                        server: server_name.clone(),
                        name: raw_name.to_string(),
                        display_name,
                        description: desc.to_string(),
                        input_schema: schema,
                    });
                }
            }
        }
    }

    let mut lock = cache().lock().unwrap();
    *lock = Some(ToolCache {
        tools: tools.clone(),
        updated_at: Instant::now(),
    });
    tools
}

/// Executes an MCP tool call by name.
pub fn call_tool(tool_name: &str, arguments: &Value) -> Result<String, String> {
    let servers = load_servers();
    let tools = list_tools();

    // Match by display_name (e.g. server__tool) or bare tool name
    let matched = tools.iter().find(|t| t.display_name == tool_name || t.name == tool_name)
        .ok_or_else(|| format!("MCP tool '{tool_name}' not found"))?;

    let server_cfg = servers.get(&matched.server)
        .ok_or_else(|| format!("MCP server '{}' not configured", matched.server))?;

    let result = run_rpc(
        server_cfg,
        "tools/call",
        json!({
            "name": matched.name,
            "arguments": arguments
        }),
    )?;

    // Extract text content from tool result
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        let texts: Vec<&str> = content
            .iter()
            .filter_map(|c| {
                if c.get("type").and_then(Value::as_str) == Some("text") {
                    c.get("text").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .collect();
        if !texts.is_empty() {
            return Ok(texts.join("\n"));
        }
    }

    Ok(result.to_string())
}

/// Clears the tools cache so subsequent queries refresh immediately.
pub fn invalidate_cache() {
    let mut lock = cache().lock().unwrap();
    *lock = None;
}

// ── Online Library & Registry ───────────────────────────────────────────────

/// Verified curated library of standard Model Context Protocol servers.
pub fn curated_library() -> Vec<McpRegistryEntry> {
    vec![
        McpRegistryEntry {
            id: "memory".to_string(),
            name: "Memory".to_string(),
            display_name: "Knowledge Graph Memory".to_string(),
            description: "Persistent knowledge-graph memory across chats, retaining concepts, entities, and user context.".to_string(),
            category: "Knowledge & Memory".to_string(),
            package: "@modelcontextprotocol/server-memory".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-memory".to_string()],
            env_keys: Vec::new(),
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/memory".to_string()),
        },
        McpRegistryEntry {
            id: "fetch".to_string(),
            name: "Fetch".to_string(),
            display_name: "Web Fetch & Markdown".to_string(),
            description: "Fetches any web page, cleans content and converts HTML into clean markdown for the AI.".to_string(),
            category: "Web & Search".to_string(),
            package: "@modelcontextprotocol/server-fetch".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-fetch".to_string()],
            env_keys: Vec::new(),
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/fetch".to_string()),
        },
        McpRegistryEntry {
            id: "filesystem".to_string(),
            name: "Filesystem".to_string(),
            display_name: "Local Filesystem".to_string(),
            description: "Read, write, list, and search files in allowed local system directories.".to_string(),
            category: "Files & Storage".to_string(),
            package: "@modelcontextprotocol/server-filesystem".to_string(),
            command: "npx".to_string(),
            #[cfg(windows)]
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string(), "C:\\".to_string()],
            #[cfg(not(windows))]
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string(), "/tmp".to_string()],
            env_keys: Vec::new(),
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem".to_string()),
        },
        McpRegistryEntry {
            id: "brave-search".to_string(),
            name: "Brave Search".to_string(),
            display_name: "Brave Web Search".to_string(),
            description: "Fast web and news search powered by the independent Brave Search API index.".to_string(),
            category: "Web & Search".to_string(),
            package: "@modelcontextprotocol/server-brave-search".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-brave-search".to_string()],
            env_keys: vec!["BRAVE_API_KEY".to_string()],
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/brave-search".to_string()),
        },
        McpRegistryEntry {
            id: "github".to_string(),
            name: "GitHub".to_string(),
            display_name: "GitHub Integration".to_string(),
            description: "Search repositories, inspect source files, read/create issues, PRs, and commit history.".to_string(),
            category: "Developer".to_string(),
            package: "@modelcontextprotocol/server-github".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-github".to_string()],
            env_keys: vec!["GITHUB_PERSONAL_ACCESS_TOKEN".to_string()],
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/github".to_string()),
        },
        McpRegistryEntry {
            id: "git".to_string(),
            name: "Git".to_string(),
            display_name: "Local Git Operations".to_string(),
            description: "Read git commits, branches, staged changes, status, and diffs on local repositories.".to_string(),
            category: "Developer".to_string(),
            package: "mcp-server-git".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "mcp-server-git".to_string()],
            env_keys: Vec::new(),
            default_env: HashMap::new(),
            installed: false,
            official: false,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/git".to_string()),
        },
        McpRegistryEntry {
            id: "puppeteer".to_string(),
            name: "Puppeteer".to_string(),
            display_name: "Browser Automation".to_string(),
            description: "Headless Chrome browser automation, webpage navigation, screenshot capture, and interaction.".to_string(),
            category: "Automation".to_string(),
            package: "@modelcontextprotocol/server-puppeteer".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-puppeteer".to_string()],
            env_keys: Vec::new(),
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/puppeteer".to_string()),
        },
        McpRegistryEntry {
            id: "sequential-thinking".to_string(),
            name: "Sequential Thinking".to_string(),
            display_name: "Step-by-Step Reasoning".to_string(),
            description: "Dynamic reasoning thought-chains and hypothesis testing for complex, multi-step problem solving.".to_string(),
            category: "Reasoning".to_string(),
            package: "@modelcontextprotocol/server-sequential-thinking".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-sequential-thinking".to_string()],
            env_keys: Vec::new(),
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking".to_string()),
        },
        McpRegistryEntry {
            id: "sqlite".to_string(),
            name: "SQLite".to_string(),
            display_name: "SQLite Database".to_string(),
            description: "Query local SQLite databases, inspect table schemas, and run SQL SELECT statements.".to_string(),
            category: "Database".to_string(),
            package: "@modelcontextprotocol/server-sqlite".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-sqlite".to_string()],
            env_keys: Vec::new(),
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/sqlite".to_string()),
        },
        McpRegistryEntry {
            id: "postgres".to_string(),
            name: "PostgreSQL".to_string(),
            display_name: "PostgreSQL Database".to_string(),
            description: "Inspect schemas and execute read-only queries on PostgreSQL databases.".to_string(),
            category: "Database".to_string(),
            package: "@modelcontextprotocol/server-postgres".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-postgres".to_string(), "postgresql://localhost/mydb".to_string()],
            env_keys: Vec::new(),
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/postgres".to_string()),
        },
        McpRegistryEntry {
            id: "google-maps".to_string(),
            name: "Google Maps".to_string(),
            display_name: "Google Maps Platform".to_string(),
            description: "Geocoding, directions, distance calculation, elevation, and place details using Google Maps APIs.".to_string(),
            category: "Location".to_string(),
            package: "@modelcontextprotocol/server-google-maps".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-google-maps".to_string()],
            env_keys: vec!["GOOGLE_MAPS_API_KEY".to_string()],
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/google-maps".to_string()),
        },
        McpRegistryEntry {
            id: "slack".to_string(),
            name: "Slack".to_string(),
            display_name: "Slack Workspace".to_string(),
            description: "Read channel messages, send messages, post replies, and manage Slack conversations.".to_string(),
            category: "Productivity".to_string(),
            package: "@modelcontextprotocol/server-slack".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-slack".to_string()],
            env_keys: vec!["SLACK_BOT_TOKEN".to_string(), "SLACK_TEAM_ID".to_string()],
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/slack".to_string()),
        },
        McpRegistryEntry {
            id: "notion".to_string(),
            name: "Notion".to_string(),
            display_name: "Notion Workspace".to_string(),
            description: "Search and read Notion pages, databases, blocks, and workspace documents.".to_string(),
            category: "Productivity".to_string(),
            package: "@notionhq/notion-mcp-server".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@notionhq/notion-mcp-server".to_string()],
            env_keys: vec!["NOTION_API_TOKEN".to_string()],
            default_env: HashMap::new(),
            installed: false,
            official: false,
            website: Some("https://github.com/michaellatman/mcp-notion".to_string()),
        },
        McpRegistryEntry {
            id: "sentry".to_string(),
            name: "Sentry".to_string(),
            display_name: "Sentry Error Tracking".to_string(),
            description: "Inspect application errors, stacktraces, and issues tracked in Sentry.".to_string(),
            category: "Developer".to_string(),
            package: "@sentry/mcp-server".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@sentry/mcp-server".to_string()],
            env_keys: vec!["SENTRY_AUTH_TOKEN".to_string()],
            default_env: HashMap::new(),
            installed: false,
            official: false,
            website: Some("https://github.com/getsentry/mcp-server".to_string()),
        },
        McpRegistryEntry {
            id: "everything".to_string(),
            name: "Everything Demo".to_string(),
            display_name: "Reference MCP Server".to_string(),
            description: "Reference testing server exposing sample tools, prompts, and resources for protocol debugging.".to_string(),
            category: "Testing".to_string(),
            package: "@modelcontextprotocol/server-everything".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-everything".to_string()],
            env_keys: Vec::new(),
            default_env: HashMap::new(),
            installed: false,
            official: true,
            website: Some("https://github.com/modelcontextprotocol/servers/tree/main/src/everything".to_string()),
        },
    ]
}

/// Dynamically searches npm registry for MCP server packages.
async fn search_npm_registry(query: &str) -> Option<Vec<McpRegistryEntry>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;

    let mut url = reqwest::Url::parse("https://registry.npmjs.org/-/v1/search").ok()?;
    let text = format!("mcp-server {query}");
    url.query_pairs_mut().append_pair("text", &text).append_pair("size", "6");

    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let objects = data.get("objects").and_then(Value::as_array)?;

    let mut results = Vec::new();
    for obj in objects {
        if let Some(pkg) = obj.get("package") {
            let pkg_name = pkg.get("name").and_then(Value::as_str)?;
            let desc = pkg.get("description").and_then(Value::as_str).unwrap_or("Community MCP server");
            let clean_id = pkg_name
                .split('/')
                .last()
                .unwrap_or(pkg_name)
                .replace("mcp-server-", "")
                .replace("-mcp-server", "")
                .replace("server-", "");
            let links = pkg.get("links");
            let website = links.and_then(|l| l.get("homepage").or_else(|| l.get("npm"))).and_then(Value::as_str).map(|s| s.to_string());

            results.push(McpRegistryEntry {
                id: clean_id.clone(),
                name: clean_id.clone(),
                display_name: clean_id,
                description: desc.to_string(),
                category: "Community".to_string(),
                package: pkg_name.to_string(),
                command: "npx".to_string(),
                args: vec!["-y".to_string(), pkg_name.to_string()],
                env_keys: Vec::new(),
                default_env: HashMap::new(),
                installed: false,
                official: false,
                website,
            });
        }
    }

    Some(results)
}

/// Searches the curated library and online npm registry for MCP servers.
pub async fn search_library(query: Option<&str>) -> Vec<McpRegistryEntry> {
    let installed_servers = load_servers();
    let q = query.map(|s| s.trim().to_lowercase()).unwrap_or_default();

    let mut list = curated_library();
    for item in &mut list {
        item.installed = installed_servers.contains_key(&item.id) || installed_servers.contains_key(&item.name);
    }

    if q.is_empty() {
        return list;
    }

    let mut filtered: Vec<McpRegistryEntry> = list
        .into_iter()
        .filter(|item| {
            item.name.to_lowercase().contains(&q)
                || item.display_name.to_lowercase().contains(&q)
                || item.description.to_lowercase().contains(&q)
                || item.category.to_lowercase().contains(&q)
                || item.package.to_lowercase().contains(&q)
        })
        .collect();

    if filtered.len() < 6 {
        if let Some(online_items) = search_npm_registry(&q).await {
            for item in online_items {
                if !filtered.iter().any(|f| f.package == item.package || f.id == item.id) {
                    let mut entry = item;
                    entry.installed = installed_servers.contains_key(&entry.id);
                    filtered.push(entry);
                }
            }
        }
    }

    filtered
}

/// Installs an MCP server by library ID or npm package name into Coucou's mcp.json.
pub fn install_from_library(
    id_or_pkg: &str,
    custom_env: Option<HashMap<String, String>>,
    custom_args: Option<Vec<String>>,
) -> Result<McpInstallOutcome, String> {
    let target = id_or_pkg.trim();
    if target.is_empty() {
        return Err("Server name or package cannot be empty".to_string());
    }

    let lib = curated_library();
    let entry = lib.iter().find(|e| {
        e.id.eq_ignore_ascii_case(target)
            || e.name.eq_ignore_ascii_case(target)
            || e.package.eq_ignore_ascii_case(target)
    });

    let (name, command, args, mut env) = if let Some(e) = entry {
        let args = custom_args.unwrap_or_else(|| e.args.clone());
        let env = e.default_env.clone();
        (e.id.clone(), e.command.clone(), args, env)
    } else {
        let clean_name = target
            .split('/')
            .last()
            .unwrap_or(target)
            .replace("mcp-server-", "")
            .replace("-mcp-server", "")
            .replace("server-", "");
        let args = custom_args.unwrap_or_else(|| vec!["-y".to_string(), target.to_string()]);
        (clean_name, "npx".to_string(), args, HashMap::new())
    };

    if let Some(c_env) = custom_env {
        for (k, v) in c_env {
            if !v.trim().is_empty() {
                env.insert(k, v);
            }
        }
    }

    save_server(&name, McpServerConfig { command, args, env })?;
    invalidate_cache();
    let tools = list_tools();
    let count = tools.iter().filter(|t| t.server == name).count();

    Ok(McpInstallOutcome {
        server_name: name.clone(),
        tools_count: count,
        success: true,
        message: format!("Successfully installed MCP server '{name}' with {count} active tool(s)"),
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mcp_config() {
        let sample = r#"{
            "mcpServers": {
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\temp"]
                }
            }
        }"#;
        let cfg: McpConfig = serde_json::from_str(sample).unwrap();
        assert!(cfg.mcp_servers.contains_key("filesystem"));
        assert_eq!(cfg.mcp_servers["filesystem"].command, "npx");
    }

    #[test]
    fn test_curated_library() {
        let lib = curated_library();
        assert!(lib.len() >= 10);
        assert!(lib.iter().any(|e| e.id == "memory"));
        assert!(lib.iter().any(|e| e.id == "filesystem"));
        assert!(lib.iter().any(|e| e.id == "fetch"));
    }
}
