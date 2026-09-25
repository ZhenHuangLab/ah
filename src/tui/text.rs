//! Styled text, terminal rows and line wrapping by display width (CJK characters are two
//! columns wide and may break anywhere; other text breaks at Unicode break opportunities).

use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::doc::Fold;

/// One terminal row of the document.
#[derive(Clone, Default)]
pub struct Row {
    pub spans: Vec<Span<'static>>,
    /// Background from this column to the right edge (prompt bands, code blocks).
    pub fill: Option<(u16, Style)>,
    /// The fold this row opens and closes.
    pub fold: Option<Fold>,
    /// Leading decoration columns that copying leaves out.
    pub pad: u16,
    /// For a wrapped continuation, the text that joined it to the previous row.
    pub join: Option<&'static str>,
}

impl Row {
    pub fn new(spans: Vec<Span<'static>>) -> Row {
        Row { spans, ..Row::default() }
    }

    pub fn blank() -> Row {
        Row::default()
    }

    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Adds `prefix` in front, keeping fills and copy padding aligned.
    pub fn indent(mut self, prefix: Span<'static>, pad: bool) -> Row {
        let w = prefix.width() as u16;
        self.spans.insert(0, prefix);
        if let Some((col, _)) = &mut self.fill {
            *col += w;
        }
        if pad {
            self.pad += w;
        }
        self
    }
}

/// Text with style runs.
#[derive(Default)]
pub struct Styled {
    pub text: String,
    runs: Vec<(usize, Style)>,
}

impl Styled {
    pub fn plain(s: &str, style: Style) -> Styled {
        let mut st = Styled::default();
        st.push(s, style);
        st
    }

    /// Appends `s`, expanding tabs and dropping control characters other than newlines.
    pub fn push(&mut self, s: &str, style: Style) {
        if s.is_empty() {
            return;
        }
        if self.runs.last().map(|r| r.1) != Some(style) {
            self.runs.push((self.text.len(), style));
        }
        if s.chars().any(|c| c.is_control() && c != '\n') {
            for c in s.chars() {
                match c {
                    '\t' => self.text.push_str("    "),
                    '\n' => self.text.push('\n'),
                    c if c.is_control() => {}
                    c => self.text.push(c),
                }
            }
        } else {
            self.text.push_str(s);
        }
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// Spans covering bytes `a..b`.
    pub fn spans(&self, a: usize, b: usize) -> Vec<Span<'static>> {
        let mut out = Vec::new();
        for (k, &(start, style)) in self.runs.iter().enumerate() {
            let end = self.runs.get(k + 1).map_or(self.text.len(), |r| r.0);
            let (s, e) = (start.max(a), end.min(b));
            if s < e {
                out.push(Span::styled(self.text[s..e].to_string(), style));
            }
        }
        out
    }

    /// Wrapped into rows of at most `width` columns.
    pub fn rows(&self, width: usize) -> Vec<Row> {
        wrap(&self.text, width).into_iter().map(|p| Row { spans: self.spans(p.start, p.end), join: p.join, ..Row::default() }).collect()
    }
}

pub fn width(s: &str) -> usize {
    s.width()
}

pub fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

pub struct Piece {
    pub start: usize,
    pub end: usize,
    pub join: Option<&'static str>,
}

/// Greedy line breaking; `\n` always breaks.
pub fn wrap(text: &str, width: usize) -> Vec<Piece> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut base = 0;
    for line in text.split('\n') {
        wrap_line(line, base, width, &mut out);
        base += line.len() + 1;
    }
    out
}

fn wrap_line(line: &str, base: usize, width: usize, out: &mut Vec<Piece>) {
    let mut join = None;
    let (mut start, mut w, mut prev) = (0, 0, 0);
    for (pos, _) in unicode_linebreak::linebreaks(line) {
        let chunk = &line[prev..pos];
        let trimmed = chunk.trim_end().width();
        if w > 0 && w + trimmed > width {
            let seg = &line[start..prev];
            let kept = seg.trim_end().len();
            out.push(Piece { start: base + start, end: base + start + kept, join });
            join = Some(if kept < seg.len() { " " } else { "" });
            start = prev;
            w = 0;
        }
        if trimmed > width {
            for (i, c) in chunk.char_indices() {
                let cw = c.width().unwrap_or(0);
                if w > 0 && w + cw > width {
                    out.push(Piece { start: base + start, end: base + prev + i, join });
                    join = Some("");
                    start = prev + i;
                    w = 0;
                }
                w += cw;
            }
        } else {
            w += chunk.width();
        }
        prev = pos;
    }
    let kept = line[start..].trim_end().len();
    out.push(Piece { start: base + start, end: base + start + kept, join });
}

/// Cuts `s` to `max` columns, adding an ellipsis when it was longer.
pub fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}
