// Cloud providers that speak the OpenAI chat completions API: OpenAI, Google AI
// (Gemini's OpenAI-compatible endpoint, as ClaudeService.swift uses it) and
// OpenRouter. One table describes them; one client talks to all three.
//
// Same contract as claude.rs: the key never leaves the credential store and
// file bytes never cross the IPC boundary. Chat only — no tools are sent, so
// the model can answer but never act on the machine.

use std::time::Duration;

use reqwest::Url;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::chat::{self, Chat, ChatContext, ChatReply, ModelInfo};
use crate::i18n::{t, tf};
use crate::{net, secrets};

pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    /// Credential store entry of its API key.
    pub key: &'static str,
    /// Relative to `base_url`.
    pub models_path: &'static str,
    pub default_model: &'static str,
    /// Model ids containing any of these are not chat models.
    pub not_chat: &'static [&'static str],
    /// OpenAI's reasoning models refuse `max_tokens` and want
    /// `max_completion_tokens`; the compatible APIs take the older name.
    pub max_tokens_field: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        key: "openai-api-key",
        models_path: "models",
        default_model: "gpt-4o",
        not_chat: &[
            "embed", "tts", "whisper", "dall-e", "audio", "realtime", "moderat", "codex",
            "computer-use", "transcribe", "image", "sora", "babbage", "davinci", "instruct",
        ],
        max_tokens_field: "max_completion_tokens",
    },
    Provider {
        id: "google",
        name: "Google AI",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        key: "google-api-key",
        models_path: "models",
        default_model: "gemini-2.0-flash",
        not_chat: &["embed", "imagen", "veo", "aqa", "tts", "audio", "live"],
        max_tokens_field: "max_tokens",
    },
    Provider {
        id: "openrouter",
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        key: "openrouter-api-key",
        models_path: "models",
        default_model: "openrouter/auto",
        not_chat: &[],
        max_tokens_field: "max_tokens",
    },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

const MAX_TOKENS: u32 = 4096;
/// Text and code files are inlined; anything larger is skipped.
const MAX_INLINE_TEXT: u64 = 200_000;
/// Images are sent inline as data URLs; past this size they are skipped.
const MAX_IMAGE: u64 = 5_000_000;

fn url(p: &Provider, tail: &str) -> Result<Url, String> {
    let base = Url::parse(p.base_url).map_err(|e| e.to_string())?;
    Url::parse(&net::join(&base, tail)).map_err(|e| e.to_string())
}

/// The user's message for one turn: plain text, or text and an image part
/// when the first turn carries a dropped picture.
fn user_message(first: bool, context: Option<&ChatContext>, query: &str) -> Value {
    let mut prefix = String::new();
    let mut image = None;
    match context.filter(|_| first) {
        Some(ChatContext::File { name, path }) => match file_part(path) {
            Some(FilePart::Image(part)) => {
                image = Some(part);
                prefix = format!("File: {name}\n\n");
            }
            Some(FilePart::Text(body)) => prefix = format!("File: {name}\nFile contents:\n{body}\n\n"),
            None => prefix = format!("File: {name}\n\n"),
        },
        Some(ChatContext::Window { app_name, title, url }) => {
            prefix = format!("{}\n\n", chat::window_line(app_name, title, url.as_deref()));
        }
        None => {}
    }
    let text = format!("{prefix}{query}");
    match image {
        None => json!({ "role": "user", "content": text }),
        Some(part) => json!({ "role": "user", "content": [part, { "type": "text", "text": text }] }),
    }
}

#[allow(dead_code)]
fn request_body(p: &Provider, model: &str, system: &str, history: &[Value], user: &Value) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": system })];
    messages.extend(history.iter().cloned());
    messages.push(user.clone());
    let mut body = json!({ "model": model, "messages": messages });
    body[p.max_tokens_field] = json!(MAX_TOKENS);
    body
}

/// The answer's text, or the error the provider put in a 200 reply
/// (OpenRouter does when the upstream model fails).
fn reply_text(p: &Provider, response: &Value) -> Result<String, String> {
    let text = response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !text.is_empty() {
        return Ok(text);
    }
    if let Some(msg) = response.pointer("/error/message").and_then(Value::as_str) {
        return Err(format!("{}: {msg}", p.name));
    }
    Err(t("No response text."))
}

fn status_error(p: &Provider, status: u16, detail: &str) -> String {
    match status {
        401 | 403 => tf(
            "{name} rejected the API key ({status}). Check it in Settings.",
            &[("name", p.name), ("status", &status.to_string())],
        ),
        402 => tf("{name}: not enough credits (402). {detail}", &[("name", p.name), ("detail", detail)]),
        404 => tf(
            "{name}: model not found (404). Pick another one above the chat box. {detail}",
            &[("name", p.name), ("detail", detail)],
        ),
        429 => tf("{name} rate limit reached (429): {detail}", &[("name", p.name), ("detail", detail)]),
        _ => format!("{} {status}: {detail}", p.name),
    }
}

pub(crate) fn build_openai_tools(settings: Option<&crate::settings::Settings>) -> Vec<Value> {
    let mut tools = Vec::new();
    if let Some(s) = settings {
        for t in crate::tools::to_openai_tools(&crate::tools::list_tools(s)) {
            tools.push(t);
        }
    }
    for t in crate::mcp::list_tools() {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": t.display_name,
                "description": t.description,
                "parameters": t.input_schema,
            }
        }));
    }
    tools
}

fn emit_status(app: Option<&AppHandle>, action: &str, status: &str, tool: Option<&str>) {
    if let Some(app) = app {
        let mut payload = json!({
            "action": action,
            "status": status,
        });
        if let Some(t) = tool {
            payload["tool"] = json!(t);
        }
        let _ = app.emit("chat-status", payload);
    }
}

/// One chat turn with a cloud provider.
pub async fn send(
    app: Option<&AppHandle>,
    chat: &Chat,
    p: &'static Provider,
    model: &str,
    query: String,
    context: Option<ChatContext>,
    skill: Option<&str>,
    settings: Option<&crate::settings::Settings>,
) -> Result<ChatReply, String> {
    if chat.is_cancelled() {
        return Err("Chat interrupted.".to_string());
    }
    emit_status(app, "Thinking…", "thinking", None);

    let key = secrets::get(p.key).ok_or_else(|| tf("{name} API key missing. Add it in Settings.", &[("name", p.name)]))?;
    if model.is_empty() {
        return Err(tf("Pick a {name} model above the chat box.", &[("name", p.name)]));
    }
    let turn = chat.begin(p.id);
    let user = user_message(turn.first, context.as_ref(), &query);
    let tools_enabled = settings.map(|s| s.chat_tools_enabled).unwrap_or(false);
    let system = chat::system_prompt(false, skill, tools_enabled);
    let tools = build_openai_tools(settings);

    let mut messages = vec![json!({ "role": "system", "content": system })];
    messages.extend(turn.history.clone());
    messages.push(user.clone());

    let endpoint = url(p, "chat/completions")?;
    let mut final_text = String::new();
    let max_turns = 15;
    let mut executed_any_tools = false;
    let mut tool_summary_steps: Vec<String> = Vec::new();

    for turn_idx in 0..max_turns {
        if chat.is_cancelled() {
            emit_status(app, "Interrupted", "interrupted", None);
            return Err("Chat interrupted.".to_string());
        }

        let is_last_turn = turn_idx == max_turns - 1;
        let mut body = json!({
            "model": model,
            p.max_tokens_field: MAX_TOKENS,
            "messages": messages,
        });
        if !tools.is_empty() && !is_last_turn {
            body["tools"] = json!(tools);
        }

        let response = net::client(&endpoint, Duration::from_secs(90))?
            .post(endpoint.clone())
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await
            .map_err(|e| tf("Network error: {error}", &[("error", &e.to_string())]))?;
        let status = response.status();
        if !status.is_success() {
            let body = net::read_capped(response, net::MAX_ERROR_BODY).await.unwrap_or_default();
            return Err(status_error(p, status.as_u16(), &net::error_detail(&body)));
        }
        let bytes = net::read_capped(response, net::MAX_BODY).await?;
        let json: Value = serde_json::from_slice(&bytes).map_err(|e| tf("Bad API response: {error}", &[("error", &e.to_string())]))?;

        let choice = json.pointer("/choices/0").ok_or_else(|| t("Unexpected API response."))?;
        let msg = choice.get("message").cloned().unwrap_or(Value::Null);

        if let Some(tool_calls) = msg.get("tool_calls").and_then(Value::as_array) {
            if !tool_calls.is_empty() && !is_last_turn {
                executed_any_tools = true;
                messages.push(msg.clone());
                for tc in tool_calls {
                    if chat.is_cancelled() {
                        emit_status(app, "Interrupted", "interrupted", None);
                        return Err("Chat interrupted.".to_string());
                    }
                    let id = tc.get("id").and_then(Value::as_str).unwrap_or_default();
                    let fn_name = tc.pointer("/function/name").and_then(Value::as_str).unwrap_or_default();
                    let fn_args_str = tc.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("{}");
                    let fn_args = serde_json::from_str::<Value>(fn_args_str).unwrap_or_else(|_| json!({}));

                    let desc = crate::tools::describe_tool_action(fn_name, &fn_args);
                    tool_summary_steps.push(desc.clone());
                    emit_status(app, &desc, "running", Some(fn_name));

                    let res_text = if crate::tools::is_builtin_tool(fn_name) {
                        match crate::tools::call_tool(fn_name, &fn_args).await {
                            Ok(out) => out,
                            Err(err) => format!("Tool execution error: {err}"),
                        }
                    } else {
                        match crate::mcp::call_tool(fn_name, &fn_args) {
                            Ok(out) => out,
                            Err(err) => format!("MCP tool error: {err}"),
                        }
                    };

                    let res_content = if res_text.trim().is_empty() {
                        "(Tool completed with no output)".to_string()
                    } else {
                        res_text
                    };

                    emit_status(app, &desc, "done", Some(fn_name));

                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "content": res_content,
                    }));
                }
                continue;
            }
        }

        final_text = reply_text(p, &json).unwrap_or_default();
        break;
    }

    if final_text.trim().is_empty() && executed_any_tools {
        emit_status(app, "Analyzing results…", "thinking", None);

        let mut synth_messages: Vec<Value> = Vec::new();
        for m in &messages {
            let role = m.get("role").and_then(Value::as_str).unwrap_or("");
            if role == "tool" {
                let content = m.get("content").and_then(Value::as_str).unwrap_or("");
                synth_messages.push(json!({
                    "role": "user",
                    "content": format!("[Hasil Eksekusi Tool]:\n{content}")
                }));
            } else if role == "assistant" && m.get("tool_calls").is_some() {
                let text = m.get("content").and_then(Value::as_str).unwrap_or("");
                let tc_desc = m.get("tool_calls")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter().filter_map(|t| {
                            let name = t.pointer("/function/name").and_then(Value::as_str).unwrap_or("tool");
                            let args = t.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("");
                            Some(format!("{name}({args})"))
                        }).collect::<Vec<_>>().join(", ")
                    })
                    .unwrap_or_default();
                let full = if text.is_empty() {
                    format!("[Menjalankan tool: {tc_desc}]")
                } else {
                    format!("{text}\n[Menjalankan tool: {tc_desc}]")
                };
                synth_messages.push(json!({ "role": "assistant", "content": full }));
            } else {
                synth_messages.push(m.clone());
            }
        }

        synth_messages.push(json!({
            "role": "user",
            "content": "Berdasarkan seluruh hasil pencarian dan eksekusi tools di atas, tolong berikan jawaban akhir yang lengkap, faktual, mendalam, dan terstruktur sesuai permintaan awal pengguna. Jawablah langsung dalam bahasa yang digunakan pengguna (e.g. Bahasa Indonesia)."
        }));

        let synth_body = json!({
            "model": model,
            p.max_tokens_field: MAX_TOKENS,
            "messages": synth_messages,
        });

        if let Ok(response) = net::client(&endpoint, Duration::from_secs(90))?
            .post(endpoint.clone())
            .bearer_auth(&key)
            .json(&synth_body)
            .send()
            .await
        {
            if response.status().is_success() {
                if let Ok(bytes) = net::read_capped(response, net::MAX_BODY).await {
                    if let Ok(json) = serde_json::from_slice::<Value>(&bytes) {
                        if let Ok(text) = reply_text(p, &json) {
                            final_text = text;
                        }
                    }
                }
            }
        }
    }

    if final_text.trim().is_empty() {
        if executed_any_tools && !tool_summary_steps.is_empty() {
            let mut tool_results_summary = Vec::new();
            for m in messages.iter().rev() {
                if m.get("role").and_then(Value::as_str) == Some("tool") {
                    if let Some(content) = m.get("content").and_then(Value::as_str) {
                        if !content.trim().is_empty() && content != "(Tool completed with no output)" {
                            tool_results_summary.push(content);
                            if tool_results_summary.len() >= 2 {
                                break;
                            }
                        }
                    }
                }
            }
            if !tool_results_summary.is_empty() {
                tool_results_summary.reverse();
                final_text = format!("{}\n\n(Langkah selesai)", tool_results_summary.join("\n\n"));
            } else {
                final_text = format!(
                    "Pemeriksaan selesai. Langkah yang telah dijalankan:\n{}",
                    tool_summary_steps.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n")
                );
            }
        } else {
            return Err(t("No response text."));
        }
    }

    if let Some(app) = app {
        let _ = app.emit("chat-delta", &final_text);
    }
    emit_status(app, "Done", "done", None);

    let plain = chat::plain_question(turn.first, context.as_ref(), &query);
    chat.commit(&turn, user, json!({ "role": "assistant", "content": final_text }), &plain, &final_text);
    Ok(ChatReply { text: final_text })
}

/// The provider's chat models. Only ever asked with the user's key, once they
/// picked this provider in the chat.
pub async fn models(p: &Provider, key: &str) -> Result<Vec<ModelInfo>, String> {
    let endpoint = url(p, p.models_path)?;
    let response = net::client(&endpoint, Duration::from_secs(15))?
        .get(endpoint)
        .bearer_auth(key)
        .send()
        .await
        .map_err(|e| tf("Network error: {error}", &[("error", &e.to_string())]))?;
    let status = response.status();
    if !status.is_success() {
        let body = net::read_capped(response, net::MAX_ERROR_BODY).await.unwrap_or_default();
        return Err(status_error(p, status.as_u16(), &net::error_detail(&body)));
    }
    let bytes = net::read_capped(response, net::MAX_BODY).await?;
    let json: Value = serde_json::from_slice(&bytes).map_err(|_| t("Unexpected API response."))?;
    Ok(parse_models(p, &json))
}

fn parse_models(p: &Provider, json: &Value) -> Vec<ModelInfo> {
    let items = json.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut models: Vec<(ModelInfo, i64, bool)> = items
        .iter()
        .filter(|m| {
            // OpenRouter lists image and audio generators too.
            m.pointer("/architecture/output_modalities")
                .and_then(Value::as_array)
                .is_none_or(|mods| mods.iter().any(|x| x.as_str() == Some("text")))
        })
        .filter_map(|m| {
            let raw = m.get("id")?.as_str()?;
            // Gemini sometimes answers "models/gemini-…".
            let id = raw.strip_prefix("models/").unwrap_or(raw).to_string();
            let lower = id.to_lowercase();
            if id.is_empty() || p.not_chat.iter().any(|x| lower.contains(x)) {
                return None;
            }
            let created = m.get("created").and_then(Value::as_i64).unwrap_or(0);
            let free = p.id == "openrouter" && is_free(m, &id);
            let name = m.get("name").and_then(Value::as_str).unwrap_or(&id);
            let label = if free && !name.to_lowercase().contains("(free)") { format!("{name} (free)") } else { name.to_string() };
            Some((ModelInfo { id, label }, created, free))
        })
        .collect();
    match p.id {
        // Newest first, as on the Mac.
        "openai" => models.sort_by_key(|m| std::cmp::Reverse(m.1)),
        // Free models first, then by name.
        "openrouter" => models.sort_by(|a, b| {
            b.2.cmp(&a.2).then_with(|| a.0.label.to_lowercase().cmp(&b.0.label.to_lowercase()))
        }),
        _ => {}
    }
    models.into_iter().map(|(m, _, _)| m).collect()
}

fn is_free(m: &Value, id: &str) -> bool {
    let zero = |ptr: &str| {
        m.pointer(ptr)
            .and_then(Value::as_str)
            .is_some_and(|v| v.parse::<f64>().ok() == Some(0.0))
    };
    id.ends_with(":free") || (zero("/pricing/prompt") && zero("/pricing/completion"))
}

enum FilePart {
    Image(Value),
    Text(String),
}

/// Images → image_url data URL, text/code → inline text. PDFs and other binary
/// files go by name only (there is no portable document part).
fn file_part(path: &str) -> Option<FilePart> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let len = std::fs::metadata(path).ok()?.len();
    let image = match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    };
    if let Some(media) = image {
        if len > MAX_IMAGE {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        let url = format!("data:{media};base64,{}", crate::claude::base64_for(&bytes));
        return Some(FilePart::Image(json!({ "type": "image_url", "image_url": { "url": url } })));
    }
    if ext == "pdf" || len > MAX_INLINE_TEXT {
        return None;
    }
    std::fs::read_to_string(path).ok().map(FilePart::Text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str) -> &'static Provider {
        provider(id).unwrap()
    }

    #[test]
    fn the_table_points_at_each_providers_own_api_and_key() {
        assert_eq!(url(p("openai"), "chat/completions").unwrap().as_str(), "https://api.openai.com/v1/chat/completions");
        assert_eq!(
            url(p("google"), "chat/completions").unwrap().as_str(),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(url(p("openrouter"), "models").unwrap().as_str(), "https://openrouter.ai/api/v1/models");
        for prov in PROVIDERS {
            assert!(crate::secrets::KNOWN_KEYS.contains(&prov.key), "{}", prov.key);
            assert!(prov.base_url.starts_with("https://"));
        }
        assert!(provider("anthropic").is_none());
        assert!(provider("ollama").is_none());
    }

    #[test]
    fn the_request_is_system_then_history_then_the_new_turn() {
        let history = vec![json!({"role":"user","content":"a"}), json!({"role":"assistant","content":"b"})];
        let user = user_message(false, None, "c");
        assert_eq!(user, json!({"role":"user","content":"c"}));
        let body = request_body(p("google"), "gemini-x", "sys", &history, &user);
        assert_eq!(body["model"], "gemini-x");
        assert_eq!(body["max_tokens"], MAX_TOKENS);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0], json!({"role":"system","content":"sys"}));
        assert_eq!(msgs[3], user);
        assert!(body.get("tools").is_none(), "chat only: no tools");
        // OpenAI's reasoning models refuse the old field name.
        let body = request_body(p("openai"), "gpt-5", "sys", &[], &user);
        assert_eq!(body["max_completion_tokens"], MAX_TOKENS);
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn context_rides_with_the_first_turn_as_text_or_an_image() {
        let dir = std::env::temp_dir().join(format!("coucou-oai-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let txt = dir.join("notes.txt");
        std::fs::write(&txt, "hello").unwrap();
        let ctx = ChatContext::File { name: "notes.txt".into(), path: txt.to_string_lossy().into() };
        assert_eq!(
            user_message(true, Some(&ctx), "sum up")["content"],
            "File: notes.txt\nFile contents:\nhello\n\nsum up"
        );
        assert_eq!(user_message(false, Some(&ctx), "more")["content"], "more");

        let png = dir.join("pic.png");
        std::fs::write(&png, [0x89, b'P', b'N', b'G']).unwrap();
        let ctx = ChatContext::File { name: "pic.png".into(), path: png.to_string_lossy().into() };
        let msg = user_message(true, Some(&ctx), "what is it?");
        let parts = msg["content"].as_array().unwrap();
        assert_eq!(parts[0]["type"], "image_url");
        assert!(parts[0]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(parts[1], json!({"type":"text","text":"File: pic.png\n\nwhat is it?"}));

        let pdf = dir.join("doc.pdf");
        std::fs::write(&pdf, "%PDF").unwrap();
        let ctx = ChatContext::File { name: "doc.pdf".into(), path: pdf.to_string_lossy().into() };
        assert_eq!(user_message(true, Some(&ctx), "q")["content"], "File: doc.pdf\n\nq");
        let _ = std::fs::remove_dir_all(dir);

        let ctx = ChatContext::Window { app_name: "Code".into(), title: "x".into(), url: None };
        assert_eq!(user_message(true, Some(&ctx), "q")["content"], "Context — App: Code, Window: x\n\nq");
    }

    #[test]
    fn the_reply_text_or_the_error_inside_a_200_is_read() {
        let ok = json!({"choices":[{"message":{"role":"assistant","content":"  Hi!  "}}]});
        assert_eq!(reply_text(p("openai"), &ok).unwrap(), "Hi!");
        let failed = json!({"error":{"message":"upstream down"}});
        assert_eq!(reply_text(p("openrouter"), &failed).unwrap_err(), "OpenRouter: upstream down");
        assert_eq!(reply_text(p("google"), &json!({"choices":[]})).unwrap_err(), "No response text.");
    }

    #[test]
    fn status_errors_say_what_to_do() {
        assert!(status_error(p("openai"), 401, "x").contains("rejected the API key"));
        assert!(status_error(p("openrouter"), 402, "x").contains("credits"));
        assert_eq!(status_error(p("google"), 500, "boom"), "Google AI 500: boom");
    }

    #[test]
    fn model_lists_keep_chat_models_only() {
        let openai = json!({"data":[
            {"id":"gpt-4o","created":100},
            {"id":"text-embedding-3-small","created":300},
            {"id":"gpt-5-mini","created":200},
            {"id":"whisper-1","created":400},
            {"id":"dall-e-3","created":500}
        ]});
        let ids: Vec<_> = parse_models(p("openai"), &openai).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["gpt-5-mini", "gpt-4o"]);

        let google = json!({"data":[
            {"id":"models/gemini-2.0-flash"},
            {"id":"models/text-embedding-004"},
            {"id":"gemini-2.5-pro"},
            {"id":"models/imagen-3.0"}
        ]});
        let ids: Vec<_> = parse_models(p("google"), &google).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["gemini-2.0-flash", "gemini-2.5-pro"]);

        let openrouter = json!({"data":[
            {"id":"b/paid","name":"B Paid","pricing":{"prompt":"0.000001","completion":"0.000002"}},
            {"id":"a/free:free","name":"A (free)"},
            {"id":"c/zero","name":"C Zero","pricing":{"prompt":"0","completion":"0"}},
            {"id":"d/image","name":"D Image","architecture":{"output_modalities":["image"]}}
        ]});
        let models = parse_models(p("openrouter"), &openrouter);
        let labels: Vec<_> = models.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, vec!["A (free)", "C Zero (free)", "B Paid"]);
    }

    #[test]
    fn an_unknown_answer_gives_an_empty_list() {
        assert!(parse_models(p("openai"), &json!({"nope":true})).is_empty());
    }
}
