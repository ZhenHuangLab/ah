//! Codex rollouts (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`).
//!
//! `response_item` records are the model-visible history and exist in every rollout version;
//! the parallel `event_msg` stream is only used for interruptions.

use serde_json::Value;

use super::{Parser, data_url, iso_ms, output, take_str, ts};
use crate::model::{Block, Image, NoticeKind, Role, Transcript};

/// User-role messages that Codex injects as context rather than typed prompts.
const INJECTED: &[&str] = &[
    "<environment_context>",
    "# AGENTS.md instructions",
    "<user_instructions>",
    "<INSTRUCTIONS>",
    "<turn_aborted>",
    "<permissions instructions>",
    "<collaboration_mode>",
];

#[derive(Default)]
pub struct Codex;

impl Parser for Codex {
    fn line(&mut self, t: &mut Transcript, mut v: Value) {
        let time = ts(&v);
        let mut p = v["payload"].take();
        match v["type"].as_str().unwrap_or("") {
            "session_meta" => {
                if t.info.id.is_none() {
                    t.info.id = p["id"].as_str().map(String::from);
                    t.info.cwd = p["cwd"].as_str().map(String::from);
                    t.info.started = p["timestamp"].as_str().and_then(iso_ms).or(time);
                    t.info.subagent = p["source"].get("subagent").is_some();
                }
            }
            "turn_context" => {
                if let Some(m) = p["model"].as_str() {
                    t.info.model = Some(m.to_string());
                }
            }
            "response_item" => item(t, time, p),
            "compacted" => {
                let msg = take_str(&mut p["message"]);
                t.notice(time, NoticeKind::Compaction, "Context compacted", msg);
            }
            "event_msg" if p["type"].as_str() == Some("turn_aborted") => {
                let label = match p["reason"].as_str() {
                    Some("interrupted") | None => "Interrupted by user".to_string(),
                    Some(r) => format!("Turn aborted ({r})"),
                };
                t.notice(time, NoticeKind::Interrupt, label, "");
            }
            _ => {}
        }
    }
}

fn item(t: &mut Transcript, time: Option<i64>, mut p: Value) {
    let kind = p["type"].as_str().unwrap_or("").to_string();
    let call_id = p["call_id"].as_str().unwrap_or("").to_string();
    match kind.as_str() {
        "message" => message(t, time, p),
        "reasoning" => {
            let mut parts = texts(&mut p["summary"]);
            if parts.iter().all(|s| s.trim().is_empty()) {
                parts = texts(&mut p["content"]);
            }
            let s = parts.join("\n\n");
            if !s.trim().is_empty() {
                t.assistant(time, Block::Thinking(s));
            }
        }
        "function_call" => {
            let args = take_str(&mut p["arguments"]);
            let input = serde_json::from_str(&args).unwrap_or(Value::String(args));
            t.tool(time, &call_id, take_str(&mut p["name"]), input, None);
        }
        "custom_tool_call" => {
            let input = Value::String(take_str(&mut p["input"]));
            t.tool(time, &call_id, take_str(&mut p["name"]), input, None);
        }
        "local_shell_call" => t.tool(time, &call_id, "shell", p["action"].take(), None),
        "tool_search_call" => t.tool(time, &call_id, "tool_search", p["arguments"].take(), None),
        "web_search_call" => {
            let done = output(String::new(), false, Vec::new());
            t.tool(time, "", "web_search", p["action"].take(), Some(done));
        }
        "function_call_output" | "custom_tool_call_output" | "local_shell_call_output" => {
            let (text, images, error) = result(p["output"].take());
            t.attach(&call_id, output(text, error, images));
        }
        "tool_search_output" => {
            let text = serde_json::to_string_pretty(&p["tools"]).unwrap_or_default();
            t.attach(&call_id, output(text, false, Vec::new()));
        }
        _ => {}
    }
}

fn message(t: &mut Transcript, time: Option<i64>, mut p: Value) {
    let role = p["role"].as_str().unwrap_or("").to_string();
    let Value::Array(parts) = p["content"].take() else { return };
    let mut texts = Vec::new();
    let mut images = Vec::new();
    for mut c in parts {
        match c["type"].as_str().unwrap_or("") {
            "input_text" | "output_text" | "text" => {
                let s = take_str(&mut c["text"]);
                let bare = s.trim();
                if !(bare.starts_with("<image") || bare == "</image>") {
                    texts.push(s);
                }
            }
            "input_image" => images.extend(c["image_url"].as_str().and_then(data_url)),
            _ => {}
        }
    }
    match role.as_str() {
        "user" => {
            texts.retain(|s| {
                let s = s.trim_start();
                !s.is_empty() && !INJECTED.iter().any(|p| s.starts_with(p))
            });
            if texts.is_empty() && images.is_empty() {
                return;
            }
            let mut blocks = Vec::new();
            if !texts.is_empty() {
                blocks.push(Block::Text(texts.join("\n\n")));
            }
            blocks.extend(images.into_iter().map(Block::Image));
            t.push(Role::User, time, blocks);
        }
        "assistant" => {
            for s in texts.into_iter().filter(|s| !s.trim().is_empty()) {
                t.assistant(time, Block::Text(s));
            }
        }
        _ => {}
    }
}

fn texts(v: &mut Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter_mut().map(|x| take_str(&mut x["text"])).collect(),
        _ => Vec::new(),
    }
}

/// Text, images and error flag of a tool output, across the shapes Codex has used.
fn result(v: Value) -> (String, Vec<Image>, bool) {
    match v {
        Value::String(s) => {
            // Older rollouts wrap shell output as `{"output": ..., "metadata": {"exit_code": n}}`.
            if s.starts_with('{')
                && let Ok(mut o) = serde_json::from_str::<Value>(&s)
                && let Some(text) = o["output"].as_str().map(String::from)
            {
                let code = o["metadata"]["exit_code"].take().as_i64().unwrap_or(0);
                return (text, Vec::new(), code != 0);
            }
            let error = exit_code(&s).is_some_and(|c| c != 0);
            (s, Vec::new(), error)
        }
        Value::Array(parts) => {
            let mut texts = Vec::new();
            let mut images = Vec::new();
            for mut c in parts {
                match c["type"].as_str().unwrap_or("") {
                    "input_image" => images.extend(c["image_url"].as_str().and_then(data_url)),
                    _ => texts.push(take_str(&mut c["text"])),
                }
            }
            let text = texts.join("\n");
            let error = exit_code(&text).is_some_and(|c| c != 0);
            (text, images, error)
        }
        Value::Object(mut o) => {
            let text = o.get_mut("content").map(take_str).unwrap_or_default();
            let error = o.get("success").and_then(Value::as_bool) == Some(false);
            (text, Vec::new(), error)
        }
        _ => (String::new(), Vec::new(), false),
    }
}

/// The exit code reported by `exec_command` output ("Process exited with code N").
fn exit_code(s: &str) -> Option<i64> {
    let mut n = s.len().min(512);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    let head = &s[..n];
    let rest = &head[head.find("exited with code ")? + "exited with code ".len()..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len());
    rest[..end].parse().ok()
}
