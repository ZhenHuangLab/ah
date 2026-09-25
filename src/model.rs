//! Normalized transcript model shared by the parsers, the TUI and the web server.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Codex,
    Pi,
}

impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Pi => "pi",
        }
    }
}

/// One listed session file.
#[derive(Clone, Debug, Serialize)]
pub struct SessionMeta {
    /// Unique key in the index; the agent's session id unless two files share one.
    pub id: String,
    pub agent: Agent,
    #[serde(skip)]
    pub path: PathBuf,
    pub cwd: String,
    pub title: String,
    /// Milliseconds since the Unix epoch.
    pub started: i64,
    pub modified: i64,
    pub size: u64,
}

impl SessionMeta {
    /// Last path component of the working directory, used as a short project label.
    pub fn project(&self) -> &str {
        self.cwd.trim_end_matches('/').rsplit('/').next().unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Event,
}

pub struct Item {
    pub role: Role,
    pub time: Option<i64>,
    pub blocks: Vec<Block>,
    /// Transcript revision at which this item last changed.
    pub rev: u64,
}

impl Item {
    /// Markdown of the text blocks, joined; what "copy message" copies.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for b in &self.blocks {
            let s = match b {
                Block::Text(s) => s.as_str(),
                Block::Notice(n) if n.body.is_empty() => n.label.as_str(),
                Block::Notice(n) => n.body.as_str(),
                _ => continue,
            };
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(s.trim());
        }
        out
    }

    /// The end of the run of tool calls and thinking that starts at block `b`, when the run
    /// contains a tool call. Such runs fold into a single summary line.
    pub fn run_end(&self, b: usize) -> Option<usize> {
        let run = self.blocks.get(b..)?.iter().take_while(|x| matches!(x, Block::Tool(_) | Block::Thinking(_))).count();
        self.blocks[b..b + run].iter().any(|x| matches!(x, Block::Tool(_))).then_some(b + run)
    }

    /// The last non-empty text block, which for an agent turn is usually its answer.
    pub fn last_text(&self) -> Option<&str> {
        self.blocks.iter().rev().find_map(|b| match b {
            Block::Text(s) if !s.trim().is_empty() => Some(s.as_str()),
            _ => None,
        })
    }
}

pub enum Block {
    /// Markdown.
    Text(String),
    Thinking(String),
    Tool(Tool),
    Image(Image),
    Notice(Notice),
}

pub struct Tool {
    pub name: String,
    /// Object arguments, or a string for free-form tools such as Codex `apply_patch`.
    pub input: Value,
    pub output: Option<Output>,
}

pub struct Output {
    pub text: String,
    pub error: bool,
    pub images: Vec<Image>,
}

#[derive(Clone)]
pub struct Image {
    pub mime: String,
    /// Base64 payload.
    pub data: Arc<str>,
}

pub struct Notice {
    pub kind: NoticeKind,
    pub label: String,
    /// Markdown shown when the notice is expanded; may be empty.
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeKind {
    Compaction,
    Rewind,
    Interrupt,
    Error,
    Command,
    Shell,
    Task,
    Model,
    Info,
}

impl NoticeKind {
    pub fn name(self) -> &'static str {
        match self {
            NoticeKind::Compaction => "compaction",
            NoticeKind::Rewind => "rewind",
            NoticeKind::Interrupt => "interrupt",
            NoticeKind::Error => "error",
            NoticeKind::Command => "command",
            NoticeKind::Shell => "shell",
            NoticeKind::Task => "task",
            NoticeKind::Model => "model",
            NoticeKind::Info => "info",
        }
    }

    /// Notices that stay visible in chat-only mode because they explain the shape of the
    /// conversation.
    pub fn structural(self) -> bool {
        matches!(self, NoticeKind::Compaction | NoticeKind::Rewind)
    }
}

fn strip(s: &mut String) {
    if s.contains('\x1b') {
        *s = crate::parse::strip_ansi(s);
    }
}

/// Session facts the parsers pick up while reading.
#[derive(Default)]
pub struct Info {
    pub id: Option<String>,
    pub cwd: Option<String>,
    pub started: Option<i64>,
    /// A user-assigned name (Claude `/rename`, pi `/name`).
    pub title: Option<String>,
    pub model: Option<String>,
    /// Transcript of a subagent spawned by another session.
    pub subagent: bool,
}

#[derive(Default)]
pub struct Transcript {
    pub items: Vec<Item>,
    pub info: Info,
    /// Bumped once per refresh; items touched during it carry the new value.
    pub rev: u64,
    /// Where each tool call is, by id.
    tools: HashMap<String, (usize, usize)>,
    /// Results written before their call (Claude Code can record a denied call that way).
    early: HashMap<String, Output>,
    /// Calls added without a result, for `end_turn`.
    pending: Vec<(usize, usize)>,
    /// The item with the latest text of the current turn.
    answer: Option<usize>,
}

impl Transcript {
    pub fn push(&mut self, role: Role, time: Option<i64>, blocks: Vec<Block>) -> usize {
        // A prompt starts a new turn: none of the agents runs a call past it.
        if role == Role::User {
            self.end_turn();
            self.answer = None;
        }
        let idx = self.items.len();
        self.items.push(Item { role, time, blocks: Vec::new(), rev: self.rev });
        for b in blocks {
            self.add_block(idx, b);
        }
        idx
    }

    pub fn notice(&mut self, time: Option<i64>, kind: NoticeKind, label: impl Into<String>, body: impl Into<String>) -> usize {
        let n = Notice { kind, label: label.into(), body: body.into() };
        self.push(Role::Event, time, vec![Block::Notice(n)])
    }

    /// Appends a block to the trailing assistant item, starting one if needed.
    pub fn assistant(&mut self, time: Option<i64>, block: Block) {
        let idx = match self.items.last() {
            Some(it) if it.role == Role::Assistant => self.items.len() - 1,
            _ => self.push(Role::Assistant, time, Vec::new()),
        };
        self.add_block(idx, block);
    }

    fn add_block(&mut self, idx: usize, mut block: Block) {
        // Some agents store colored text; escapes would show up as garbage in both views.
        match &mut block {
            Block::Text(s) | Block::Thinking(s) => strip(s),
            Block::Notice(n) => {
                strip(&mut n.label);
                strip(&mut n.body);
            }
            _ => {}
        }
        let item = &mut self.items[idx];
        let answer = item.role == Role::Assistant && matches!(&block, Block::Text(s) if !s.trim().is_empty());
        item.blocks.push(block);
        item.rev = self.rev;
        if answer {
            // The turn's final answer is now here; the item that held it changes as well.
            if let Some(j) = self.answer.filter(|&j| j != idx) {
                self.items[j].rev = self.rev;
            }
            self.answer = Some(idx);
        }
    }

    /// Appends a tool call to the trailing assistant item and remembers it for `attach`.
    pub fn tool(&mut self, time: Option<i64>, id: &str, name: impl Into<String>, input: Value, output: Option<Output>) {
        let output = output.or_else(|| self.early.remove(id));
        let waiting = output.is_none();
        self.assistant(time, Block::Tool(Tool { name: name.into(), input, output }));
        let idx = self.items.len() - 1;
        let at = (idx, self.items[idx].blocks.len() - 1);
        if !id.is_empty() {
            self.tools.insert(id.to_string(), at);
        }
        if waiting {
            self.pending.push(at);
        }
    }

    /// Records a tool result against its call, or keeps it for a call not seen yet.
    pub fn attach(&mut self, id: &str, output: Output) {
        let Some(&(i, b)) = self.tools.get(id) else {
            if !id.is_empty() {
                self.early.insert(id.to_string(), output);
            }
            return;
        };
        let rev = self.rev;
        let item = &mut self.items[i];
        if let Block::Tool(t) = &mut item.blocks[b] {
            t.output = Some(output);
            item.rev = rev;
        }
    }

    /// Marks calls still waiting for a result as having none, since their turn is over. A
    /// result that turns up later still replaces the mark.
    pub fn end_turn(&mut self) {
        let rev = self.rev;
        for (i, b) in std::mem::take(&mut self.pending) {
            let item = &mut self.items[i];
            if let Block::Tool(t) = &mut item.blocks[b]
                && t.output.is_none()
            {
                t.output = Some(Output { text: "No result was recorded.".into(), error: true, images: Vec::new() });
                item.rev = rev;
            }
        }
    }

    /// The notice of item `idx`, marked as changed.
    pub fn notice_mut(&mut self, idx: usize) -> Option<&mut Notice> {
        let rev = self.rev;
        let item = self.items.get_mut(idx)?;
        item.rev = rev;
        item.blocks.iter_mut().find_map(|b| match b {
            Block::Notice(n) => Some(n),
            _ => None,
        })
    }

    /// For each item, the block with the final answer of its turn when the item holds it: the
    /// last text the agent wrote before the next prompt. Earlier text in a turn is mostly notes
    /// on progress between tool calls.
    pub fn answers(&self) -> Vec<Option<usize>> {
        let mut out = vec![None; self.items.len()];
        let mut last = None;
        for (i, it) in self.items.iter().enumerate() {
            match it.role {
                Role::User => {
                    if let Some((j, b)) = last.take() {
                        out[j] = Some(b);
                    }
                }
                Role::Assistant => {
                    if let Some(b) = it.blocks.iter().rposition(|b| matches!(b, Block::Text(s) if !s.trim().is_empty())) {
                        last = Some((i, b));
                    }
                }
                Role::Event => {}
            }
        }
        if let Some((j, b)) = last {
            out[j] = Some(b);
        }
        out
    }

    pub fn first_prompt(&self) -> Option<&str> {
        self.items.iter().filter(|i| i.role == Role::User).find_map(|i| {
            i.blocks.iter().find_map(|b| match b {
                Block::Text(s) if !s.trim().is_empty() => Some(s.as_str()),
                _ => None,
            })
        })
    }
}
