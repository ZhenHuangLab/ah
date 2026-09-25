//! How tool calls are summarized and shown, shared by the TUI and the web view. Tool names
//! differ between agents, so arguments are recognized by their keys.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::model::Tool;
use crate::parse::one_line;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Shell,
    Read,
    Edit,
    Search,
    Web,
    Agent,
    Other,
}

fn kind(name: &str) -> Kind {
    match name.to_ascii_lowercase().as_str() {
        "bash" | "shell" | "exec_command" | "local_shell" | "write_stdin" | "shell_command" | "bashoutput" | "killshell" => Kind::Shell,
        "read" | "read_file" | "view_image" | "read_symbol" | "read_enclosing" | "notebookread" | "read_many_files" => Kind::Read,
        "edit" | "multiedit" | "write" | "apply_patch" | "notebookedit" | "write_file" | "edit_file" | "create_file" => Kind::Edit,
        "grep" | "glob" | "find" | "ls" | "ffgrep" | "fffind" | "signal_grep" | "symbol_search" | "ast_grep_search" | "tool_search"
        | "codebase_search" | "file_search" | "list_dir" => Kind::Search,
        "websearch" | "webfetch" | "web_search" | "web_fetch" | "fetch_content" | "get_search_content" | "xai_x_search" => Kind::Web,
        "agent"
        | "task"
        | "subagent"
        | "spawn_agent"
        | "get_subagent_result"
        | "steer_subagent"
        | "wait_agent"
        | "sendmessage"
        | "send_message" => Kind::Agent,
        _ => Kind::Other,
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// "Ran 3 commands, read 2 files, edited 1 file".
pub fn group_summary<'a>(tools: impl IntoIterator<Item = &'a Tool>) -> String {
    let mut counts: BTreeMap<Kind, usize> = BTreeMap::new();
    let mut others: Vec<&str> = Vec::new();
    let mut failed = 0;
    for t in tools {
        let k = kind(&t.name);
        *counts.entry(k).or_default() += 1;
        if k == Kind::Other && !others.contains(&t.name.as_str()) {
            others.push(&t.name);
        }
        failed += t.output.as_ref().is_some_and(|o| o.error) as usize;
    }
    let mut parts: Vec<String> = counts
        .iter()
        .map(|(k, &n)| match k {
            Kind::Shell => format!("ran {}", plural(n, "command", "commands")),
            Kind::Read => format!("read {}", plural(n, "file", "files")),
            Kind::Edit => format!("edited {}", plural(n, "file", "files")),
            Kind::Search => format!("searched {}", plural(n, "time", "times")),
            Kind::Web => plural(n, "web request", "web requests"),
            Kind::Agent => plural(n, "agent call", "agent calls"),
            Kind::Other => match others.as_slice() {
                [a] => format!("used {a}"),
                [a, b] => format!("used {a} and {b}"),
                [a, b, rest @ ..] => format!("used {a}, {b} and {} more", rest.len()),
                [] => String::new(),
            },
        })
        .collect();
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    let mut s = parts.join(", ");
    if let Some(first) = s.get(..1) {
        s.replace_range(..1, &first.to_uppercase());
    }
    s
}

/// The argument that best identifies a call, on one line.
pub fn arg(t: &Tool) -> String {
    let o = match &t.input {
        Value::String(s) => return patch_files(s).unwrap_or_else(|| one_line(s, 200)),
        Value::Object(o) => o,
        _ => return String::new(),
    };
    if let Some(c) = o.get("command").or_else(|| o.get("cmd")) {
        return one_line(&command(c), 200);
    }
    if let Some(p) = str_field(o, "patch") {
        return patch_files(p).unwrap_or_default();
    }
    if let Some(p) = str_field(o, "pattern") {
        return match str_field(o, "path") {
            Some(dir) => one_line(&format!("{p}  in {dir}"), 200),
            None => one_line(p, 200),
        };
    }
    const KEYS: &[&str] = &[
        "file_path",
        "path",
        "filePath",
        "notebook_path",
        "query",
        "url",
        "description",
        "prompt",
        "skill",
        "subject",
        "message",
        "agent_id",
        "task_id",
        "to",
        "target",
    ];
    KEYS.iter().find_map(|k| str_field(o, k)).or_else(|| o.values().find_map(Value::as_str)).map(|s| one_line(s, 200)).unwrap_or_default()
}

fn str_field<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    o.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// A shell command given as a string or as an argv array such as `["bash", "-lc", "…"]`.
fn command(c: &Value) -> String {
    match c {
        Value::String(s) => s.clone(),
        Value::Array(a) => {
            let parts: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
            match parts.as_slice() {
                [_, flag, rest @ ..] if matches!(*flag, "-lc" | "-c") => rest.join(" "),
                _ => parts.join(" "),
            }
        }
        _ => String::new(),
    }
}

fn patch_files(s: &str) -> Option<String> {
    let files: Vec<&str> = s
        .lines()
        .filter_map(|l| ["*** Update File: ", "*** Add File: ", "*** Delete File: "].iter().find_map(|p| l.strip_prefix(p)))
        .collect();
    (!files.is_empty()).then(|| files.join(", "))
}

pub fn pending(t: &Tool) -> bool {
    t.output.is_none()
}

pub fn failed(t: &Tool) -> bool {
    t.output.as_ref().is_some_and(|o| o.error)
}

/// One part of a tool call's details.
pub struct Section {
    pub title: String,
    /// Highlighting language for code bodies (`diff`, `bash`, a file extension, …).
    pub lang: String,
    pub body: String,
    pub markdown: bool,
    pub error: bool,
}

impl Section {
    fn code(title: impl Into<String>, lang: impl Into<String>, body: impl Into<String>) -> Section {
        Section { title: title.into(), lang: lang.into(), body: body.into(), markdown: false, error: false }
    }
}

/// Input and output of a call, formatted for reading.
pub fn sections(t: &Tool) -> Vec<Section> {
    let mut out = input_sections(t);
    if let Some(o) = &t.output
        && !o.text.trim().is_empty()
    {
        let title = if o.error { "error" } else { "output" };
        let mut s = Section::code(title, "", o.text.as_str());
        s.markdown = !o.error && kind(&t.name) == Kind::Agent;
        s.error = o.error;
        out.push(s);
    }
    out
}

fn input_sections(t: &Tool) -> Vec<Section> {
    let o = match &t.input {
        Value::String(s) => {
            let lang = if s.contains("*** Begin Patch") { "diff" } else { "" };
            return vec![Section::code("input", lang, s.as_str())];
        }
        Value::Object(o) => o,
        Value::Null => return Vec::new(),
        v => return vec![Section::code("input", "json", v.to_string())],
    };
    if let Some(c) = o.get("command").or_else(|| o.get("cmd")) {
        let title = str_field(o, "description").unwrap_or("command");
        return vec![Section::code(title, "bash", command(c))];
    }
    let path = str_field(o, "file_path").or_else(|| str_field(o, "path")).unwrap_or("");
    let edits = edit_pairs(o);
    if !edits.is_empty() {
        let body = edits.iter().map(|(old, new)| diff(old, new)).collect::<Vec<_>>().join("\n");
        return vec![Section::code(path, "diff", body)];
    }
    if let Some(content) = str_field(o, "content").filter(|_| !path.is_empty()) {
        return vec![Section::code(path, extension(path), content)];
    }
    if let Some(p) = str_field(o, "patch") {
        return vec![Section::code("patch", "diff", p)];
    }
    if let Some(p) = str_field(o, "prompt") {
        let title = str_field(o, "description").or_else(|| str_field(o, "subagent_type")).unwrap_or("prompt");
        let mut s = Section::code(title, "", p);
        s.markdown = true;
        return vec![s];
    }
    vec![Section::code("input", "json", serde_json::to_string_pretty(&t.input).unwrap_or_default())]
}

/// Old/new text pairs of the edit tools (Claude `Edit`/`MultiEdit`, pi `edit`).
fn edit_pairs(o: &Map<String, Value>) -> Vec<(String, String)> {
    let pair = |e: &Map<String, Value>| {
        let old = str_field(e, "old_string").or_else(|| str_field(e, "oldText"));
        let new = e.get("new_string").or_else(|| e.get("newText")).and_then(Value::as_str);
        old.zip(new).map(|(a, b)| (a.to_string(), b.to_string()))
    };
    if let Some(p) = pair(o) {
        return vec![p];
    }
    match o.get("edits") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_object).filter_map(pair).collect(),
        _ => Vec::new(),
    }
}

fn extension(path: &str) -> &str {
    path.rsplit_once('.').map(|(_, e)| e).filter(|e| !e.contains('/')).unwrap_or("")
}

/// A line diff with `-`/`+`/` ` prefixes; falls back to all-removed/all-added for large inputs.
fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut out = String::new();
    let mut line = |p: char, s: &str| {
        out.push(p);
        out.push_str(s);
        out.push('\n');
    };
    if a.len() * b.len() > 1_000_000 {
        a.iter().for_each(|l| line('-', l));
        b.iter().for_each(|l| line('+', l));
    } else {
        // lcs[i][j]: common subsequence length of a[i..] and b[j..].
        let w = b.len() + 1;
        let mut lcs = vec![0u32; (a.len() + 1) * w];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lcs[i * w + j] = if a[i] == b[j] { lcs[(i + 1) * w + j + 1] + 1 } else { lcs[(i + 1) * w + j].max(lcs[i * w + j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                line(' ', a[i]);
                i += 1;
                j += 1;
            } else if i < a.len() && (j == b.len() || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
                line('-', a[i]);
                i += 1;
            } else {
                line('+', b[j]);
                j += 1;
            }
        }
    }
    out.pop();
    out
}
