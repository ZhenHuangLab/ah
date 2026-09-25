//! pi sessions (`~/.pi/agent/sessions/--<cwd>--/<timestamp>_<id>.jsonl`).
//!
//! Entries form a tree through `id`/`parentId`; going back with `/tree` appends a new branch to
//! the same file. Entries are shown in file order, and a notice marks each point where the
//! conversation continues from somewhere other than the previous entry, so abandoned branches
//! stay visible as history.

use std::collections::HashMap;

use serde_json::Value;

use super::{fence, one_line, output, take_str, tokens, ts, Parser};
use crate::model::{Block, Image, NoticeKind, Role, Transcript};

#[derive(Default)]
pub struct Pi {
    last: Option<String>,
    /// Item count after each entry, to name the point a rewind returns to.
    at: HashMap<String, usize>,
}

impl Parser for Pi {
    fn line(&mut self, t: &mut Transcript, mut v: Value) {
        let time = ts(&v);
        let kind = v["type"].as_str().unwrap_or("").to_string();
        if kind == "session" {
            t.info.id = v["id"].as_str().map(String::from);
            t.info.cwd = v["cwd"].as_str().map(String::from);
            t.info.started = time;
            return;
        }
        let id = v["id"].as_str().map(String::from);
        let parent = v["parentId"].as_str();
        let mut rewind = None;
        if id.is_some() && self.last.is_some() && parent != self.last.as_deref() {
            let label = self.rewind_label(t, parent);
            rewind = Some(t.notice(time, NoticeKind::Rewind, label, ""));
        }
        match kind.as_str() {
            "message" => message(t, time, v["message"].take()),
            "compaction" => {
                let mut label = String::from("Context compacted");
                if let Some(n) = v["tokensBefore"].as_u64() {
                    label.push_str(&format!(" · {} tokens", tokens(n)));
                }
                t.notice(time, NoticeKind::Compaction, label, take_str(&mut v["summary"]));
            }
            "branch_summary" => {
                let summary = take_str(&mut v["summary"]);
                match rewind.and_then(|i| t.notice_mut(i)) {
                    Some(n) => n.body = summary,
                    None => {
                        t.notice(time, NoticeKind::Rewind, "Branch summary", summary);
                    }
                }
            }
            "custom_message" if v["display"].as_bool() == Some(true) => {
                let (body, _) = content(v["content"].take());
                t.notice(time, NoticeKind::Info, take_str(&mut v["customType"]), body);
            }
            "model_change" => {
                let model = v["modelId"].as_str().unwrap_or("").to_string();
                let label = format!("{}/{model}", v["provider"].as_str().unwrap_or(""));
                t.info.model = Some(model);
                t.notice(time, NoticeKind::Model, label, "");
            }
            "session_info" => {
                if let Some(name) = v["name"].as_str() {
                    t.info.title = Some(name.to_string());
                }
            }
            _ => {}
        }
        if let Some(id) = id {
            self.at.insert(id.clone(), t.items.len());
            self.last = Some(id);
        }
    }
}

impl Pi {
    fn rewind_label(&self, t: &Transcript, parent: Option<&str>) -> String {
        let Some(parent) = parent else { return "Started over from the beginning".into() };
        let Some(&n) = self.at.get(parent) else { return "Rewound to an earlier point".into() };
        let prompt = t.items[..n].iter().rev().find(|i| i.role == Role::User).and_then(|i| {
            i.blocks.iter().find_map(|b| match b {
                Block::Text(s) => Some(s.as_str()),
                _ => None,
            })
        });
        match prompt {
            Some(p) => format!("Rewound to after “{}”", one_line(p, 60)),
            None => "Rewound to the start".into(),
        }
    }
}

fn message(t: &mut Transcript, time: Option<i64>, mut m: Value) {
    match m["role"].as_str().unwrap_or("") {
        "user" => {
            let (text, images) = content(m["content"].take());
            let mut blocks = Vec::new();
            if !text.trim().is_empty() {
                blocks.push(Block::Text(text));
            }
            blocks.extend(images.into_iter().map(Block::Image));
            if !blocks.is_empty() {
                t.push(Role::User, time, blocks);
            }
        }
        "assistant" => {
            if let Some(model) = m["model"].as_str() {
                t.info.model = Some(model.to_string());
            }
            if let Value::Array(blocks) = m["content"].take() {
                for mut b in blocks {
                    match b["type"].as_str().unwrap_or("") {
                        "text" => {
                            let s = take_str(&mut b["text"]);
                            if !s.trim().is_empty() {
                                t.assistant(time, Block::Text(s));
                            }
                        }
                        "thinking" => {
                            let s = take_str(&mut b["thinking"]);
                            if !s.trim().is_empty() && b["redacted"].as_bool() != Some(true) {
                                t.assistant(time, Block::Thinking(s));
                            }
                        }
                        "toolCall" => {
                            let id = b["id"].as_str().unwrap_or("").to_string();
                            t.tool(time, &id, take_str(&mut b["name"]), b["arguments"].take(), None);
                        }
                        _ => {}
                    }
                }
            }
            let error = m["errorMessage"].as_str().unwrap_or("");
            match m["stopReason"].as_str() {
                Some("error") => {
                    t.notice(time, NoticeKind::Error, one_line(error, 200), "");
                }
                Some("aborted") => {
                    t.notice(time, NoticeKind::Interrupt, "Interrupted", "");
                }
                _ => {}
            }
        }
        "toolResult" => {
            let id = m["toolCallId"].as_str().unwrap_or("").to_string();
            let error = m["isError"].as_bool().unwrap_or(false);
            let (text, images) = content(m["content"].take());
            t.attach(&id, output(text, error, images));
        }
        "bashExecution" => {
            let cmd = take_str(&mut m["command"]);
            let out = super::strip_ansi(&take_str(&mut m["output"]));
            let body = if out.trim().is_empty() { String::new() } else { fence(&out, "") };
            t.notice(time, NoticeKind::Shell, format!("! {cmd}"), body);
        }
        "custom" if m["display"].as_bool() == Some(true) => {
            let (body, _) = content(m["content"].take());
            t.notice(time, NoticeKind::Info, take_str(&mut m["customType"]), body);
        }
        _ => {}
    }
}

/// Text and images of a string-or-blocks content value.
fn content(v: Value) -> (String, Vec<Image>) {
    match v {
        Value::String(s) => (s, Vec::new()),
        Value::Array(blocks) => {
            let mut texts = Vec::new();
            let mut images = Vec::new();
            for mut b in blocks {
                match b["type"].as_str() {
                    Some("text") => texts.push(take_str(&mut b["text"])),
                    Some("image") => {
                        let mime = b["mimeType"].as_str().unwrap_or("image/png").to_string();
                        images.push(super::image(&mime, take_str(&mut b["data"])));
                    }
                    _ => {}
                }
            }
            (texts.join("\n"), images)
        }
        _ => (String::new(), Vec::new()),
    }
}
