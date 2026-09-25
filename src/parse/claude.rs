//! Claude Code transcripts (`~/.claude/projects/<project>/<session>.jsonl`).
//!
//! The file is read in order, which keeps everything from before a compaction. Streamed
//! assistant blocks arrive as separate records and simply extend the current turn; tool results
//! arrive in `user` records and are attached to their calls.

use serde_json::Value;

use super::{Parser, fence, one_line, output, strip_ansi, tag, take_str, task, tokens, ts};
use crate::model::{Block, Image, NoticeKind, Role, Transcript};

#[derive(Default)]
pub struct Claude {
    /// `isSidechain` of the first record. Subagent files are all sidechain; in a main file,
    /// sidechain records belong to (old-style) inline subagents and are skipped.
    sidechain: Option<bool>,
    command: Option<usize>,
    shell: Option<usize>,
    compaction: Option<usize>,
}

impl Parser for Claude {
    fn line(&mut self, t: &mut Transcript, mut v: Value) {
        let time = ts(&v);
        let kind = v["type"].as_str().unwrap_or("").to_string();
        match kind.as_str() {
            "custom-title" => {
                if let Some(s) = v["customTitle"].as_str() {
                    t.info.title = Some(s.to_string());
                }
                return;
            }
            "user" | "assistant" | "system" | "attachment" => {}
            _ => return,
        }
        let side = v["isSidechain"].as_bool().unwrap_or(false);
        if *self.sidechain.get_or_insert(side) != side {
            return;
        }
        t.info.subagent = side;
        if t.info.cwd.is_none() {
            t.info.cwd = v["cwd"].as_str().map(String::from);
        }
        if t.info.started.is_none() {
            t.info.started = time;
        }
        match kind.as_str() {
            "user" => self.user(t, time, v),
            "assistant" => self.assistant(t, time, &mut v),
            "system" => self.system(t, time, &mut v),
            _ => self.attachment(t, time, &mut v),
        }
    }
}

impl Claude {
    fn user(&mut self, t: &mut Transcript, time: Option<i64>, mut v: Value) {
        let content = v["message"]["content"].take();
        if v["isCompactSummary"].as_bool() == Some(true) {
            let (text, _) = content_parts(content);
            match self.compaction.take().and_then(|i| t.notice_mut(i)) {
                Some(n) => n.body = text,
                None => {
                    t.notice(time, NoticeKind::Compaction, "Context compacted", text);
                }
            }
            return;
        }
        let mut texts = Vec::new();
        let mut images = Vec::new();
        match content {
            Value::String(s) => texts.push(s),
            Value::Array(blocks) => {
                for mut b in blocks {
                    match b["type"].as_str() {
                        Some("tool_result") => {
                            let id = b["tool_use_id"].as_str().unwrap_or("").to_string();
                            let error = b["is_error"].as_bool().unwrap_or(false);
                            let (text, imgs) = content_parts(b["content"].take());
                            t.attach(&id, output(text, error, imgs));
                        }
                        Some("text") => texts.push(take_str(&mut b["text"])),
                        Some("image") => images.extend(image(&mut b)),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        // Meta records are context injected by Claude Code (caveats, skill bodies, image notes).
        if v["isMeta"].as_bool() == Some(true) || (texts.is_empty() && images.is_empty()) {
            return;
        }
        let text = texts.join("\n\n");
        if self.special(t, time, &text) {
            return;
        }
        let mut blocks = Vec::new();
        if !text.trim().is_empty() {
            blocks.push(Block::Text(text));
        }
        blocks.extend(images.into_iter().map(Block::Image));
        t.push(Role::User, time, blocks);
    }

    /// Handles user-side records that are not prompts. Returns true when `text` was consumed.
    fn special(&mut self, t: &mut Transcript, time: Option<i64>, text: &str) -> bool {
        let s = text.trim_start();
        if s.starts_with("<local-command-caveat>") {
            return true;
        }
        if s.starts_with("<command-name>") || s.starts_with("<command-message>") {
            let name = tag(s, "command-name").unwrap_or("");
            let args = tag(s, "command-args").unwrap_or("");
            let label = format!("{name} {args}").trim().to_string();
            self.command = Some(t.notice(time, NoticeKind::Command, label, ""));
            return true;
        }
        for name in ["local-command-stdout", "local-command-stderr"] {
            if s.starts_with(&format!("<{name}>")) {
                let out = strip_ansi(tag(s, name).unwrap_or(""));
                let out = out.trim();
                if !out.is_empty() {
                    let last = t.items.len().checked_sub(1);
                    match self.command.filter(|&i| Some(i) == last).and_then(|i| t.notice_mut(i)) {
                        Some(n) => {
                            if !n.body.is_empty() {
                                n.body.push('\n');
                            }
                            n.body.push_str(out);
                        }
                        None => {
                            t.notice(time, NoticeKind::Command, one_line(out, 120), out);
                        }
                    }
                }
                return true;
            }
        }
        if let Some(cmd) = tag(s, "bash-input").filter(|_| s.starts_with("<bash-input>")) {
            self.shell = Some(t.notice(time, NoticeKind::Shell, format!("! {}", cmd.trim()), ""));
            return true;
        }
        if s.starts_with("<bash-stdout>") || s.starts_with("<bash-stderr>") {
            let mut out = tag(s, "bash-stdout").unwrap_or("").to_string();
            let err = tag(s, "bash-stderr").unwrap_or("");
            if !err.is_empty() {
                out.push('\n');
                out.push_str(err);
            }
            let out = strip_ansi(out.trim());
            if !out.is_empty()
                && let Some(n) = self.shell.and_then(|i| t.notice_mut(i))
            {
                n.body = fence(&out, "");
            }
            return true;
        }
        if task(t, time, s) {
            return true;
        }
        if s.starts_with("[Request interrupted by user") {
            t.notice(time, NoticeKind::Interrupt, "Interrupted by user", "");
            return true;
        }
        false
    }

    fn assistant(&mut self, t: &mut Transcript, time: Option<i64>, v: &mut Value) {
        let api_error = v["isApiErrorMessage"].as_bool() == Some(true);
        let msg = &mut v["message"];
        if api_error {
            let (text, _) = content_parts(msg["content"].take());
            t.notice(time, NoticeKind::Error, one_line(&text, 200), "");
            return;
        }
        if let Some(m) = msg["model"].as_str().filter(|m| !m.starts_with('<')) {
            t.info.model = Some(m.to_string());
        }
        let Value::Array(blocks) = msg["content"].take() else { return };
        for mut b in blocks {
            let kind = b["type"].as_str().unwrap_or("").to_string();
            match kind.as_str() {
                "text" => {
                    let s = take_str(&mut b["text"]);
                    if !s.trim().is_empty() {
                        t.assistant(time, Block::Text(s));
                    }
                }
                "thinking" => {
                    let s = take_str(&mut b["thinking"]);
                    if !s.trim().is_empty() {
                        t.assistant(time, Block::Thinking(s));
                    }
                }
                "tool_use" | "server_tool_use" => {
                    let id = b["id"].as_str().unwrap_or("").to_string();
                    let name = take_str(&mut b["name"]);
                    t.tool(time, &id, name, b["input"].take(), None);
                }
                "image" => {
                    if let Some(i) = image(&mut b) {
                        t.assistant(time, Block::Image(i));
                    }
                }
                k if k.ends_with("_tool_result") => {
                    let id = b["tool_use_id"].as_str().unwrap_or("").to_string();
                    let text = serde_json::to_string_pretty(&b["content"]).unwrap_or_default();
                    t.attach(&id, output(text, false, Vec::new()));
                }
                _ => {}
            }
        }
    }

    fn system(&mut self, t: &mut Transcript, time: Option<i64>, v: &mut Value) {
        match v["subtype"].as_str().unwrap_or("") {
            "compact_boundary" => {
                let m = &v["compactMetadata"];
                let mut label = String::from("Context compacted");
                if let Some(trigger) = m["trigger"].as_str() {
                    label.push_str(" · ");
                    label.push_str(trigger);
                }
                if let Some(pre) = m["preTokens"].as_u64() {
                    label.push_str(" · ");
                    label.push_str(&tokens(pre));
                    if let Some(post) = m["postTokens"].as_u64() {
                        label.push_str(" → ");
                        label.push_str(&tokens(post));
                    }
                    label.push_str(" tokens");
                }
                self.compaction = Some(t.notice(time, NoticeKind::Compaction, label, ""));
            }
            "local_command" => {
                let s = take_str(&mut v["content"]);
                self.special(t, time, &s);
            }
            "informational" | "api_error" => {
                let s = take_str(&mut v["content"]);
                if !s.trim().is_empty() {
                    let kind = match v["level"].as_str() {
                        Some("error") => NoticeKind::Error,
                        _ => NoticeKind::Info,
                    };
                    t.notice(time, kind, one_line(&s, 200), "");
                }
            }
            _ => {}
        }
    }

    /// Messages typed while Claude was busy are stored as `queued_command` attachments.
    fn attachment(&mut self, t: &mut Transcript, time: Option<i64>, v: &mut Value) {
        let a = &mut v["attachment"];
        if a["type"].as_str() != Some("queued_command") {
            return;
        }
        let (prompt, images) = content_parts(a["prompt"].take());
        if prompt.trim().is_empty() || self.special(t, time, &prompt) {
            return;
        }
        let mut blocks = vec![Block::Text(prompt)];
        blocks.extend(images.into_iter().map(Block::Image));
        t.push(Role::User, time, blocks);
    }
}

/// Text and images of a string-or-blocks content value.
fn content_parts(v: Value) -> (String, Vec<Image>) {
    match v {
        Value::String(s) => (s, Vec::new()),
        Value::Array(blocks) => {
            let mut texts = Vec::new();
            let mut images = Vec::new();
            for mut b in blocks {
                match b["type"].as_str() {
                    Some("text") => texts.push(take_str(&mut b["text"])),
                    Some("image") => images.extend(image(&mut b)),
                    _ => {}
                }
            }
            (texts.join("\n"), images)
        }
        _ => (String::new(), Vec::new()),
    }
}

fn image(b: &mut Value) -> Option<Image> {
    let src = &mut b["source"];
    if src["type"].as_str() != Some("base64") {
        return None;
    }
    let mime = src["media_type"].as_str().unwrap_or("image/png").to_string();
    Some(super::image(&mime, take_str(&mut src["data"])))
}
