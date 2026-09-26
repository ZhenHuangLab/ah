//! The conversation view: scrolling, folds, search, selection and copying.

use std::collections::HashSet;
use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::Result;
use base64::Engine;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthChar;

use super::doc::{self, Fold, Opts, View};
use super::text::{Row, truncate, width};
use super::theme;
use crate::live::Live;
use crate::model::{Block, Role, SessionMeta};
use crate::{share, tools};

pub enum Action {
    None,
    Back,
    Quit,
}

/// An item's rows and what they were laid out for.
struct Cached {
    rev: u64,
    width: usize,
    view: View,
    answer: Option<usize>,
    ver: u64,
    rows: Vec<Row>,
}

struct Search {
    query: String,
    /// Row, first column, end column.
    hits: Vec<(usize, usize, usize)>,
    cur: usize,
}

#[derive(Clone, Copy)]
struct Sel {
    anchor: (usize, u16),
    head: (usize, u16),
    moved: bool,
}

pub struct Viewer {
    pub live: Live,
    /// The transcript generation the layout below belongs to.
    generation: u64,
    cache: Vec<Option<Cached>>,
    /// Bumped when a fold inside the item changes.
    ver: Vec<u64>,
    /// First row of each item, plus the total at the end.
    starts: Vec<usize>,
    width: usize,
    height: usize,
    top: usize,
    follow: bool,
    view: View,
    open: HashSet<Fold>,
    focus: Option<Fold>,
    sel: Option<Sel>,
    search: Option<Search>,
    typing: Option<String>,
    help: bool,
    /// Asking how long a new share lasts.
    sharing: bool,
    /// The public host name, which share links need.
    host: Option<String>,
    msg: Option<(String, Instant)>,
}

impl Viewer {
    pub fn open(meta: SessionMeta, host: Option<String>) -> Result<Viewer> {
        let live = Live::open(meta)?;
        Ok(Viewer {
            generation: live.generation,
            live,
            cache: Vec::new(),
            ver: Vec::new(),
            starts: vec![0],
            width: 0,
            height: 0,
            top: 0,
            follow: true,
            view: View::All,
            open: HashSet::new(),
            focus: None,
            sel: None,
            search: None,
            typing: None,
            help: false,
            sharing: false,
            host,
            msg: None,
        })
    }

    pub fn refresh(&mut self) {
        if let Err(e) = self.live.refresh() {
            self.say(format!("reload failed: {e}"));
        }
    }

    fn say(&mut self, s: impl Into<String>) {
        self.msg = Some((s.into(), Instant::now()));
    }

    fn total(&self) -> usize {
        *self.starts.last().unwrap_or(&0)
    }

    fn max_top(&self) -> usize {
        self.total().saturating_sub(self.height)
    }

    /// Index of the item that holds row `n`.
    fn item_at(&self, n: usize) -> usize {
        self.starts.partition_point(|&s| s <= n).saturating_sub(1).min(self.cache.len().saturating_sub(1))
    }

    fn row(&self, n: usize) -> Option<&Row> {
        if n >= self.total() {
            return None;
        }
        let i = self.item_at(n);
        self.cache.get(i)?.as_ref()?.rows.get(n - self.starts[i])
    }

    /// Lays out items that changed, keeping the top row's item in place.
    fn layout(&mut self) {
        if self.generation != self.live.generation {
            // The file was replaced or rewritten: rows, folds and positions describe items
            // that no longer exist.
            self.generation = self.live.generation;
            self.cache.clear();
            self.ver.clear();
            self.open.clear();
            self.focus = None;
            self.sel = None;
        }
        let items = &self.live.t.items;
        let answers = self.live.t.answers();
        let anchor = (!self.cache.is_empty()).then(|| {
            let i = self.item_at(self.top);
            (i, self.top - self.starts[i])
        });
        self.cache.resize_with(items.len(), || None);
        self.ver.resize(items.len(), 0);
        let opts = Opts { width: self.width, view: self.view, open: &self.open };
        let mut changed = false;
        let mut starts = Vec::with_capacity(items.len() + 1);
        let mut total = 0;
        for (i, it) in items.iter().enumerate() {
            let (ver, answer) = (self.ver[i], answers[i]);
            let fresh = self.cache[i]
                .as_ref()
                .is_some_and(|c| c.rev == it.rev && c.width == opts.width && c.view == opts.view && c.answer == answer && c.ver == ver);
            if !fresh {
                let rows = doc::layout(i, it, answer, &opts);
                self.cache[i] = Some(Cached { rev: it.rev, width: opts.width, view: opts.view, answer, ver, rows });
                changed = true;
            }
            starts.push(total);
            total += self.cache[i].as_ref().map_or(0, |c| c.rows.len());
        }
        starts.push(total);
        self.starts = starts;
        if !changed {
            return;
        }
        if let Some((i, off)) = anchor {
            let len = self.starts[i + 1] - self.starts[i];
            self.top = self.starts[i] + off.min(len.saturating_sub(1));
        }
        if let Some(q) = self.search.as_ref().map(|s| s.query.clone()) {
            let cur = self.search.as_ref().map_or(0, |s| s.cur);
            self.find(q, false);
            if let Some(s) = &mut self.search {
                s.cur = cur.min(s.hits.len().saturating_sub(1));
            }
        }
    }

    fn scroll(&mut self, delta: isize) {
        self.top = self.top.saturating_add_signed(delta).min(self.max_top());
        self.follow = self.top >= self.max_top();
    }

    /// Scrolls so row `n` sits a third of the way down.
    fn reveal(&mut self, n: usize) {
        self.top = n.saturating_sub(self.height / 3).min(self.max_top());
        self.follow = false;
    }

    fn fold_row(&self, fold: Fold) -> Option<usize> {
        let i = fold.item();
        let c = self.cache.get(i)?.as_ref()?;
        c.rows.iter().position(|r| r.fold == Some(fold)).map(|k| self.starts[i] + k)
    }

    fn toggle(&mut self, fold: Fold) {
        if !self.open.remove(&fold) {
            self.open.insert(fold);
        }
        if let Some(v) = self.ver.get_mut(fold.item()) {
            *v += 1;
        }
    }

    /// The item under the reading line, a third of the way down.
    fn reading_item(&self) -> usize {
        match self.focus {
            Some(f) => f.item(),
            None => self.item_at((self.top + self.height / 3).min(self.total().saturating_sub(1))),
        }
    }

    // ---------- input ----------

    pub fn key(&mut self, k: KeyEvent) -> Action {
        if let Some(q) = &mut self.typing {
            match k.code {
                KeyCode::Esc => self.typing = None,
                KeyCode::Enter => {
                    let q = self.typing.take().unwrap_or_default();
                    self.find(q, true);
                }
                KeyCode::Backspace => {
                    q.pop();
                }
                KeyCode::Char(c) => q.push(c),
                _ => {}
            }
            return Action::None;
        }
        if std::mem::take(&mut self.sharing) {
            match k.code {
                KeyCode::Char('1') => self.share(Some(1)),
                KeyCode::Char('7') => self.share(Some(7)),
                KeyCode::Char('3') => self.share(Some(30)),
                KeyCode::Char('n') => self.share(None),
                _ => {}
            }
            return Action::None;
        }
        if self.help {
            self.help = false;
            if matches!(k.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                return Action::None;
            }
        }
        self.sel = None;
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.height.max(2) as isize;
        match k.code {
            KeyCode::Char('c') if ctrl => return Action::Quit,
            KeyCode::Char('d') if ctrl => self.scroll(page / 2),
            KeyCode::Char('u') if ctrl => self.scroll(-page / 2),
            KeyCode::Char('f') if ctrl => self.scroll(page - 1),
            KeyCode::Char('b') if ctrl => self.scroll(-(page - 1)),
            KeyCode::Esc if self.search.is_some() => self.search = None,
            KeyCode::Esc | KeyCode::Char('q') => return Action::Back,
            KeyCode::Char('j') | KeyCode::Down => self.scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll(-1),
            KeyCode::Char(' ') | KeyCode::PageDown => self.scroll(page - 1),
            KeyCode::Char('b') | KeyCode::PageUp => self.scroll(-(page - 1)),
            KeyCode::Char('g') | KeyCode::Home => {
                self.top = 0;
                self.follow = false;
            }
            KeyCode::Char('G') | KeyCode::End => self.follow = true,
            KeyCode::Char(']') => self.prompt(true),
            KeyCode::Char('[') => self.prompt(false),
            KeyCode::Tab => self.step_focus(true),
            KeyCode::BackTab => self.step_focus(false),
            KeyCode::Enter => self.toggle_focus(),
            KeyCode::Char('e') => self.toggle_all(),
            KeyCode::Char('t') => self.set_view(if self.view == View::Chat { View::All } else { View::Chat }),
            KeyCode::Char('a') => self.set_view(if self.view == View::Answers { View::All } else { View::Answers }),
            KeyCode::Char('/') => self.typing = Some(String::new()),
            KeyCode::Char('n') => self.step_hit(true),
            KeyCode::Char('N') => self.step_hit(false),
            KeyCode::Char('y') => self.copy_item(),
            KeyCode::Char('s') if self.host.is_some() => self.sharing = true,
            KeyCode::Char('s') => self.say("to share, set the public host name in ~/.config/ah/config.toml"),
            KeyCode::Char('?') => self.help = true,
            _ => {}
        }
        Action::None
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        let y = m.row as usize;
        let n = self.top + y.min(self.height.saturating_sub(1));
        match m.kind {
            MouseEventKind::ScrollDown => self.scroll(3),
            MouseEventKind::ScrollUp => self.scroll(-3),
            MouseEventKind::Down(MouseButton::Left) if y < self.height => {
                self.sel = Some(Sel { anchor: (n, m.column), head: (n, m.column), moved: false });
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if y == 0 {
                    self.scroll(-1);
                } else if y + 1 >= self.height {
                    self.scroll(1);
                }
                let n = self.top + y.min(self.height.saturating_sub(1));
                if let Some(s) = &mut self.sel {
                    s.head = (n, m.column);
                    s.moved = true;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(s) = self.sel.take() else { return };
                if s.moved {
                    let text = self.selected(&s);
                    self.copy(&text);
                    self.sel = Some(s);
                } else if let Some(fold) = self.row(s.anchor.0).and_then(|r| r.fold) {
                    self.focus = Some(fold);
                    self.toggle(fold);
                }
            }
            _ => {}
        }
    }

    fn set_view(&mut self, view: View) {
        self.view = view;
        self.focus = None;
        self.say(match view {
            View::All => "showing everything",
            View::Chat => "chat only: tool calls hidden",
            View::Answers => "answers only: prompts and final answers",
        });
    }

    /// Jumps to the next or previous prompt.
    fn prompt(&mut self, forward: bool) {
        // Only items laid out by the last draw have rows; a key can arrive before the first.
        let items = &self.live.t.items;
        let prompts: Vec<usize> = self
            .starts
            .windows(2)
            .enumerate()
            .filter(|&(i, w)| w[1] > w[0] && items.get(i).is_some_and(|it| it.role == Role::User))
            .map(|(_, w)| w[0])
            .collect();
        let target = if forward { prompts.iter().find(|&&s| s > self.top) } else { prompts.iter().rev().find(|&&s| s < self.top) };
        match target.copied() {
            Some(s) => {
                self.top = s.min(self.max_top());
                self.follow = self.top >= self.max_top();
            }
            None if forward => self.follow = true,
            None => self.top = 0,
        }
    }

    fn step_focus(&mut self, forward: bool) {
        let from = self.focus.and_then(|f| self.fold_row(f));
        let total = self.total();
        let found = if forward {
            let start = from.map_or(self.top, |r| r + 1);
            (start..total).find(|&n| self.row(n).is_some_and(|r| r.fold.is_some()))
        } else {
            let end = from.unwrap_or(self.top + self.height);
            (0..end.min(total)).rev().find(|&n| self.row(n).is_some_and(|r| r.fold.is_some()))
        };
        if let Some(n) = found {
            self.focus = self.row(n).and_then(|r| r.fold);
            if n < self.top || n >= self.top + self.height {
                self.reveal(n);
            }
        }
    }

    fn toggle_focus(&mut self) {
        let visible = self.focus.and_then(|f| self.fold_row(f)).is_some_and(|n| n >= self.top && n < self.top + self.height);
        if !visible {
            self.focus = None;
            self.step_focus(true);
        }
        if let Some(f) = self.focus {
            self.toggle(f);
        }
    }

    /// Opens every group of tool calls and every thinking block, or closes all folds.
    fn toggle_all(&mut self) {
        if self.open.is_empty() {
            for (i, it) in self.live.t.items.iter().enumerate() {
                let mut b = 0;
                while b < it.blocks.len() {
                    match it.run_end(b) {
                        Some(end) => {
                            self.open.insert(Fold::Group(i, b));
                            b = end;
                            continue;
                        }
                        None if matches!(it.blocks[b], Block::Thinking(_)) => {
                            self.open.insert(Fold::Thinking(i, b));
                        }
                        None => {}
                    }
                    b += 1;
                }
            }
            self.say("expanded all");
        } else {
            self.open.clear();
            self.say("collapsed all");
        }
        self.ver.iter_mut().for_each(|v| *v += 1);
    }

    fn find(&mut self, query: String, jump: bool) {
        if query.is_empty() {
            self.search = None;
            return;
        }
        let q: Vec<char> = query.chars().map(lower).collect();
        let mut hits = Vec::new();
        for n in 0..self.total() {
            let Some(row) = self.row(n) else { break };
            let text: Vec<char> = row.text().chars().collect();
            let low: Vec<char> = text.iter().map(|&c| lower(c)).collect();
            let mut i = 0;
            while i + q.len() <= low.len() {
                if low[i..i + q.len()] == q[..] {
                    let a: usize = text[..i].iter().map(|c| c.width().unwrap_or(0)).sum();
                    let b = a + text[i..i + q.len()].iter().map(|c| c.width().unwrap_or(0)).sum::<usize>();
                    hits.push((n, a, b));
                    i += q.len();
                } else {
                    i += 1;
                }
            }
        }
        let cur = hits.iter().position(|h| h.0 >= self.top).unwrap_or(0);
        let count = hits.len();
        self.search = Some(Search { query, hits, cur });
        if jump {
            if count == 0 {
                self.say("no matches (folded content is not searched)");
            } else {
                self.step_hit_to(cur);
            }
        }
    }

    fn step_hit(&mut self, forward: bool) {
        let Some(s) = &self.search else { return };
        let n = s.hits.len();
        if n == 0 {
            return;
        }
        let k = if forward { (s.cur + 1) % n } else { (s.cur + n - 1) % n };
        self.step_hit_to(k);
    }

    fn step_hit_to(&mut self, k: usize) {
        let Some(s) = &mut self.search else { return };
        s.cur = k;
        let (row, n) = (s.hits[k].0, s.hits.len());
        self.reveal(row);
        self.say(format!("match {}/{n}", k + 1));
    }

    fn selected(&self, s: &Sel) -> String {
        let (a, b) = if (s.anchor.0, s.anchor.1) <= (s.head.0, s.head.1) { (s.anchor, s.head) } else { (s.head, s.anchor) };
        let mut out = String::new();
        for n in a.0..=b.0 {
            let Some(row) = self.row(n) else { break };
            let from = if n == a.0 { a.1 as usize } else { 0 }.max(row.pad as usize);
            let to = if n == b.0 { b.1 as usize + 1 } else { usize::MAX };
            if n > a.0 {
                out.push_str(row.join.unwrap_or("\n"));
            }
            out.push_str(cols(&row.text(), from, to).trim_end());
        }
        out
    }

    fn copy_item(&mut self) {
        let items = &self.live.t.items;
        let text = match self.focus {
            Some(Fold::Tool(i, b)) => match items.get(i).and_then(|it| it.blocks.get(b)) {
                Some(Block::Tool(t)) => {
                    tools::sections(t).iter().map(|s| format!("{}\n{}", s.title, s.body)).collect::<Vec<_>>().join("\n\n")
                }
                _ => String::new(),
            },
            _ => {
                let i = self.reading_item();
                // The answers view shows only the final answer of an agent message.
                let answer = if self.view == View::Answers { self.live.t.answers().get(i).copied().flatten() } else { None };
                match (items.get(i), answer) {
                    (Some(it), Some(b)) if it.role == Role::Assistant => match &it.blocks[b] {
                        Block::Text(s) => s.trim().to_string(),
                        _ => String::new(),
                    },
                    (Some(it), _) => it.text(),
                    (None, _) => String::new(),
                }
            }
        };
        if text.is_empty() {
            self.say("nothing to copy here");
        } else {
            self.copy(&text);
        }
    }

    /// The view a share shows: answers from the answers view, the chat from the others.
    fn share_view(&self) -> share::View {
        if self.view == View::Answers { share::View::Answers } else { share::View::Chat }
    }

    /// Shares the session as it is now for `days`, or until stopped, and copies the link.
    fn share(&mut self, days: Option<u32>) {
        let Some(host) = self.host.clone() else { return };
        match share::create(&self.live, self.share_view(), days) {
            Ok(s) => {
                self.copy(&s.url(&host));
                self.say(format!("copied the share link ({}, expires {})", s.view.name(), s.expiry()));
            }
            Err(e) => self.say(format!("sharing failed: {e:#}")),
        }
    }

    /// Sets the clipboard with OSC 52, which also works over SSH.
    fn copy(&mut self, text: &str) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(text);
        let mut out = std::io::stdout();
        let _ = write!(out, "\x1b]52;c;{b64}\x07").and_then(|_| out.flush());
        self.say(format!("copied {} characters", text.chars().count()));
    }

    // ---------- drawing ----------

    pub fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        self.width = area.width as usize;
        self.height = area.height.saturating_sub(1) as usize;
        self.layout();
        self.top = if self.follow { self.max_top() } else { self.top.min(self.max_top()) };
        let focus = self.focus.and_then(|fo| self.fold_row(fo));
        let sel = self.sel.map(|s| if (s.anchor.0, s.anchor.1) <= (s.head.0, s.head.1) { (s.anchor, s.head) } else { (s.head, s.anchor) });
        let buf = f.buffer_mut();
        for y in 0..self.height {
            let n = self.top + y;
            let Some(row) = self.row(n) else { break };
            let line = Rect::new(area.x, area.y + y as u16, area.width, 1);
            if let Some((col, st)) = row.fill
                && col < area.width
            {
                buf.set_style(Rect::new(area.x + col, line.y, area.width - col, 1), st);
            }
            let mut x = area.x;
            for s in &row.spans {
                if x >= area.right() {
                    break;
                }
                x = buf.set_stringn(x, line.y, &s.content, (area.right() - x) as usize, s.style).0;
            }
            if focus == Some(n) {
                buf.set_style(line, theme::FOCUS);
            }
            if let Some(s) = &self.search {
                let from = s.hits.partition_point(|h| h.0 < n);
                for (k, h) in s.hits.iter().enumerate().skip(from).take_while(|(_, h)| h.0 == n) {
                    let style = if k == s.cur { theme::MATCH_ON } else { theme::MATCH };
                    highlight(buf, line, h.1, h.2, style);
                }
            }
            if let Some((a, b)) = sel
                && n >= a.0
                && n <= b.0
            {
                let c0 = if n == a.0 { a.1 as usize } else { 0 };
                let c1 = if n == b.0 { b.1 as usize + 1 } else { self.width };
                highlight(buf, line, c0, c1, Style::new().add_modifier(Modifier::REVERSED));
            }
        }
        self.draw_bar(buf, area);
        if self.help {
            draw_box(buf, area, "keys", HELP);
        }
        if self.sharing {
            let what = match self.share_view() {
                share::View::Chat => "the agent's messages",
                share::View::Answers => "the final answers",
            };
            let rows = [
                ("", "Anyone with the link can read the prompts and"),
                ("", &*format!("{what} as they are now. Check them for keys,")),
                ("", "tokens and private paths first."),
                ("", ""),
                ("1  7  3", "copy a link that lasts 1, 7 or 30 days"),
                ("n", "copy a link that lasts until stopped"),
                ("esc", "cancel"),
            ];
            draw_box(buf, area, "share this session", &rows);
        }
    }

    fn draw_bar(&mut self, buf: &mut Buffer, area: Rect) {
        let y = area.bottom().saturating_sub(1);
        let bar = Rect::new(area.x, y, area.width, 1);
        buf.set_style(bar, theme::BAR);
        if let Some(q) = &self.typing {
            buf.set_stringn(area.x, y, format!(" /{q}▏  enter search · esc cancel"), area.width as usize, theme::BAR);
            return;
        }
        let total = self.total();
        let pos = match ((self.top + self.height).min(total) * 100).checked_div(total) {
            Some(p) => format!("{p}%"),
            None => "empty".to_string(),
        };
        let live = jiff::Timestamp::now().as_millisecond() - self.live.meta.modified < 120_000;
        let view = match self.view {
            View::All => "all",
            View::Chat => "chat only",
            View::Answers => "answers only",
        };
        let mut right = format!(" {pos} · {view}");
        if live {
            right.push_str(" · live");
        }
        if self.follow {
            right.push_str(" · following");
        }
        right.push_str(" · ? keys ");
        let room = (area.width as usize).saturating_sub(width(&right) + 1);
        if let Some((m, at)) = &self.msg {
            if at.elapsed() < Duration::from_secs(3) {
                buf.set_stringn(area.x, y, format!(" {}", truncate(m, room)), room, theme::BAR_KEY);
                buf.set_stringn(area.x + room as u16 + 1, y, &right, width(&right), theme::BAR);
                return;
            }
            self.msg = None;
        }
        let m = &self.live.meta;
        let agent = format!(" {} ", m.agent.name());
        let x = buf.set_stringn(area.x, y, &agent, room, theme::BAR.patch(theme::agent(m.agent.name()))).0;
        let used = (x - area.x) as usize;
        buf.set_stringn(x, y, format!("· {}", truncate(&m.title, room.saturating_sub(used + 2))), room.saturating_sub(used), theme::BAR);
        buf.set_stringn(area.x + room as u16 + 1, y, &right, width(&right), theme::BAR);
    }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// The part of `s` between display columns `from` and `to`.
fn cols(s: &str, from: usize, to: usize) -> &str {
    let (mut a, mut b, mut w) = (s.len(), s.len(), 0);
    for (i, c) in s.char_indices() {
        if w >= from && a == s.len() {
            a = i;
        }
        if w >= to {
            b = i;
            break;
        }
        w += c.width().unwrap_or(0);
    }
    if a > b { "" } else { &s[a..b] }
}

fn highlight(buf: &mut Buffer, line: Rect, from: usize, to: usize, style: Style) {
    let from = (from as u16).min(line.width);
    let to = (to.min(line.width as usize) as u16).max(from);
    buf.set_style(Rect::new(line.x + from, line.y, to - from, 1), style);
}

const HELP: &[(&str, &str)] = &[
    ("j k  ↑ ↓", "scroll a line"),
    ("space b  PgDn PgUp", "scroll a page"),
    ("ctrl-d ctrl-u", "half a page"),
    ("g G", "top, bottom (and follow)"),
    ("[ ]", "previous, next prompt"),
    ("t", "chat only: hide tool calls"),
    ("a", "answers only: prompts, final answers"),
    ("tab shift-tab", "move between folds"),
    ("enter  click", "open or close a fold"),
    ("e", "expand or collapse all"),
    ("/  n N", "search, next and previous match"),
    ("y", "copy the message (or focused tool)"),
    ("s", "share the chat or answers by a link"),
    ("drag", "select text and copy it"),
    ("q esc", "back to the list"),
];

/// A box over the middle of the screen: a title, then rows of keys and what they do, or of
/// text alone when the keys are empty.
fn draw_box(buf: &mut Buffer, area: Rect, title: &str, rows: &[(&str, &str)]) {
    let w = 64.min(area.width);
    let h = (rows.len() as u16 + 4).min(area.height);
    if w < 8 || h < 3 {
        return;
    }
    let r = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    buf.set_style(r, theme::BAR);
    for y in r.y..r.bottom() {
        buf.set_stringn(r.x, y, " ".repeat(w as usize), w as usize, theme::BAR);
    }
    buf.set_stringn(r.x + 2, r.y + 1, title, w as usize - 4, theme::BAR_KEY);
    for (k, (keys, what)) in rows.iter().enumerate() {
        let y = r.y + 2 + k as u16;
        if y + 1 >= r.bottom() {
            break;
        }
        if keys.is_empty() {
            buf.set_stringn(r.x + 2, y, what, w as usize - 4, theme::BAR);
            continue;
        }
        buf.set_stringn(r.x + 2, y, keys, (w as usize - 4).min(22), theme::BAR_KEY);
        buf.set_stringn(r.x + 24, y, what, w.saturating_sub(26) as usize, theme::BAR);
    }
}
