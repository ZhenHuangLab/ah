//! The session list: sessions from this directory first, then everything by recency.

use std::fs;
use std::path::Path;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::text::{truncate, width};
use super::theme;
use crate::discover::{self, Roots, mtime_ms};
use crate::model::SessionMeta;

pub enum Pick {
    None,
    Open(SessionMeta),
    Quit,
}

pub struct Picker {
    all: Vec<SessionMeta>,
    cwd: String,
    filter: String,
    shown: Vec<usize>,
    sel: usize,
    top: usize,
    height: usize,
    pub msg: Option<String>,
}

impl Picker {
    pub fn new(all: Vec<SessionMeta>, cwd: String) -> Picker {
        let mut p = Picker { all, cwd, filter: String::new(), shown: Vec::new(), sel: 0, top: 0, height: 0, msg: None };
        p.sort();
        p
    }

    fn here(&self, m: &SessionMeta) -> bool {
        m.cwd == self.cwd
    }

    /// Re-sorts and re-filters, keeping the selected session selected.
    fn sort(&mut self) {
        let keep = self.shown.get(self.sel).map(|&i| self.all[i].path.clone());
        let cwd = self.cwd.clone();
        self.all.sort_by(|a, b| (b.cwd == cwd).cmp(&(a.cwd == cwd)).then(b.modified.cmp(&a.modified)));
        let words: Vec<String> = self.filter.to_lowercase().split_whitespace().map(String::from).collect();
        self.shown = (0..self.all.len())
            .filter(|&i| {
                let m = &self.all[i];
                let hay = format!("{} {} {} {}", m.title, m.cwd, m.id, m.agent.name()).to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect();
        self.sel = keep.and_then(|p| self.shown.iter().position(|&i| self.all[i].path == p)).unwrap_or(0);
    }

    /// Picks up a changed session file.
    pub fn changed(&mut self, roots: &Roots, path: &Path) {
        if let Some(m) = self.all.iter_mut().find(|m| m.path == path) {
            match fs::metadata(path) {
                Ok(md) => {
                    m.modified = mtime_ms(&md);
                    m.size = md.len();
                }
                Err(_) => self.all.retain(|m| m.path != path),
            }
        } else if let Some(m) = roots.classify(path).and_then(|agent| discover::meta(agent, path, true)) {
            self.all.push(m);
        } else {
            return;
        }
        self.sort();
    }

    fn select(&mut self, sel: isize) {
        self.sel = sel.clamp(0, self.shown.len().saturating_sub(1) as isize) as usize;
    }

    pub fn key(&mut self, k: KeyEvent) -> Pick {
        self.msg = None;
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.height.max(1) as isize;
        let sel = self.sel as isize;
        match k.code {
            KeyCode::Char('c') if ctrl => return Pick::Quit,
            KeyCode::Char('n') if ctrl => self.select(sel + 1),
            KeyCode::Char('p') if ctrl => self.select(sel - 1),
            KeyCode::Char('u') if ctrl => {
                self.filter.clear();
                self.sort();
            }
            KeyCode::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.sort();
            }
            KeyCode::Esc => return Pick::Quit,
            KeyCode::Down => self.select(sel + 1),
            KeyCode::Up => self.select(sel - 1),
            KeyCode::PageDown => self.select(sel + page),
            KeyCode::PageUp => self.select(sel - page),
            KeyCode::Home => self.select(0),
            KeyCode::End => self.select(isize::MAX / 2),
            KeyCode::Enter => return self.pick(),
            KeyCode::Backspace => {
                self.filter.pop();
                self.sort();
            }
            KeyCode::Char(c) if !ctrl => {
                self.filter.push(c);
                self.sort();
            }
            _ => {}
        }
        Pick::None
    }

    fn pick(&self) -> Pick {
        match self.shown.get(self.sel) {
            Some(&i) => Pick::Open(self.all[i].clone()),
            None => Pick::None,
        }
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Pick {
        match m.kind {
            MouseEventKind::ScrollDown => self.select(self.sel as isize + 3),
            MouseEventKind::ScrollUp => self.select(self.sel as isize - 3),
            MouseEventKind::Down(MouseButton::Left) => {
                let row = m.row as usize;
                if row >= 1 && row <= self.height && self.top + row - 1 < self.shown.len() {
                    self.sel = self.top + row - 1;
                    return self.pick();
                }
            }
            _ => {}
        }
        Pick::None
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let buf = f.buffer_mut();
        self.height = area.height.saturating_sub(2) as usize;
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + self.height {
            self.top = self.sel + 1 - self.height;
        }
        let w = area.width as usize;
        let head = format!(" ah · {} of {} sessions · * this directory", self.shown.len(), self.all.len());
        buf.set_stringn(area.x, area.y, head, w, theme::DIM);
        let now = jiff::Timestamp::now().as_millisecond();
        for (k, &i) in self.shown.iter().enumerate().skip(self.top).take(self.height) {
            let m = &self.all[i];
            let y = area.y + 1 + (k - self.top) as u16;
            let on = k == self.sel;
            let base = if on { theme::SELECTED } else { theme::TEXT };
            buf.set_style(Rect::new(area.x, y, area.width, 1), base);
            let cells = [
                (if on { "❯" } else { " " }.to_string(), 2, base.patch(theme::PROMPT_MARK)),
                ((if self.here(m) { "*" } else { " " }).to_string(), 1, base.patch(theme::TOOLS)),
                (format!("{:>4}", age(now - m.modified)), 6, base.patch(theme::DIM)),
                (m.agent.name().to_string(), 7, base.patch(theme::agent(m.agent.name()))),
                (truncate(m.project(), 18), 20, base.patch(theme::MUTED)),
            ];
            let mut x = area.x;
            for (text, cw, style) in cells {
                if x >= area.right() {
                    break;
                }
                buf.set_stringn(x, y, &text, cw.min((area.right() - x) as usize), style);
                x += cw as u16;
            }
            if x < area.right() {
                let room = (area.right() - x) as usize;
                buf.set_stringn(x, y, truncate(&m.title, room), room, base.patch(if on { theme::BRIGHT } else { theme::TEXT }));
            }
        }
        let y = area.bottom().saturating_sub(1);
        buf.set_style(Rect::new(area.x, y, area.width, 1), theme::BAR);
        let hint = match &self.msg {
            Some(m) => format!(" {m} "),
            None => " ↑↓ move · enter open · esc quit ".to_string(),
        };
        let prompt = format!(" › {}▏", self.filter);
        buf.set_stringn(area.x, y, &prompt, w, theme::BAR_KEY);
        let hw = width(&hint);
        if width(&prompt) + hw < w {
            buf.set_stringn(area.right() - hw as u16, y, &hint, hw, theme::BAR);
        }
    }
}

fn age(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}
