//! Per-agent transcript parsers. Each consumes one JSONL record at a time so the same code
//! serves a full load, an incremental tail and the head scan used for listing.

mod claude;
mod codex;
mod pi;

use std::sync::Arc;

use serde_json::Value;

use crate::model::{Agent, Image, Output, Transcript};

pub trait Parser: Send {
    fn line(&mut self, t: &mut Transcript, v: Value);
}

pub fn new(agent: Agent) -> Box<dyn Parser> {
    match agent {
        Agent::Claude => Box::<claude::Claude>::default(),
        Agent::Codex => Box::<codex::Codex>::default(),
        Agent::Pi => Box::<pi::Pi>::default(),
    }
}

/// The record's `timestamp`, as epoch milliseconds.
fn ts(v: &Value) -> Option<i64> {
    match &v["timestamp"] {
        Value::String(s) => iso_ms(s),
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

pub fn iso_ms(s: &str) -> Option<i64> {
    s.parse::<jiff::Timestamp>().ok().map(|t| t.as_millisecond())
}

fn take_str(v: &mut Value) -> String {
    match v.take() {
        Value::String(s) => s,
        _ => String::new(),
    }
}

/// Content between `<tag>` and `</tag>`, when present.
fn tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = s.find(&open)? + open.len();
    let end = s[start..].find(&close).map_or(s.len(), |e| start + e);
    Some(s[start..end].trim_matches('\n'))
}

fn output(text: String, error: bool, images: Vec<Image>) -> Output {
    Output { text: strip_ansi(&text), error, images }
}

fn image(mime: &str, data: String) -> Image {
    Image { mime: mime.to_string(), data: Arc::from(data) }
}

/// Parses `data:<mime>;base64,<payload>`.
fn data_url(url: &str) -> Option<Image> {
    let rest = url.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    Some(image(mime, data.to_string()))
}

/// Removes terminal escape sequences (CSI, OSC and two-byte escapes).
pub fn strip_ansi(s: &str) -> String {
    if !s.contains('\x1b') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('[') => {
                for c in it.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = it.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Collapses whitespace and truncates to `max` characters.
pub fn one_line(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut n = 0;
    for w in s.split_whitespace() {
        if n > 0 {
            out.push(' ');
            n += 1;
        }
        for c in w.chars() {
            if n >= max {
                out.push('…');
                return out;
            }
            out.push(c);
            n += 1;
        }
    }
    out
}

/// Wraps `body` in a code fence longer than any backtick run inside it.
pub fn fence(body: &str, lang: &str) -> String {
    let (mut run, mut max) = (0, 0);
    for c in body.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        max = max.max(run);
    }
    let f = "`".repeat(max.max(2) + 1);
    format!("{f}{lang}\n{}\n{f}", body.trim_end_matches('\n'))
}

/// Token counts as `638.3k` / `1.2M`.
pub fn tokens(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}
