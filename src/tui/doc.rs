//! Transcript items as terminal rows. Tool calls, thinking, notices and long prompts fold;
//! the chat and answers views leave out more of the transcript.

use std::collections::HashSet;

use ratatui::style::Style;
use ratatui::text::Span;

use super::md;
use super::text::{Row, Styled, truncate};
use super::theme;
use crate::model::{Block, Item, Notice, NoticeKind, Role, Tool};
use crate::tools;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Fold {
    /// The run of tool calls starting at block `.1` of item `.0`.
    Group(usize, usize),
    Tool(usize, usize),
    Thinking(usize, usize),
    Notice(usize),
    Prompt(usize),
}

impl Fold {
    pub fn item(self) -> usize {
        match self {
            Fold::Group(i, _) | Fold::Tool(i, _) | Fold::Thinking(i, _) | Fold::Notice(i) | Fold::Prompt(i) => i,
        }
    }
}

/// How much of the transcript is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    /// Everything, with tool calls and thinking folded.
    All,
    /// Prompts and all the agent's messages.
    Chat,
    /// Prompts and the final answer of each turn.
    Answers,
}

pub struct Opts<'a> {
    pub width: usize,
    pub view: View,
    pub open: &'a HashSet<Fold>,
}

/// Prompts longer than this many rows fold.
const PROMPT_ROWS: usize = 16;
/// Lines of a tool section shown when it is expanded; `y` copies the rest.
const SECTION_LINES: usize = 400;

/// Rows of item `i`. Block `answer` holds the final answer of its turn, if the item has it.
pub fn layout(i: usize, item: &Item, answer: Option<usize>, o: &Opts) -> Vec<Row> {
    let mut rows = match item.role {
        Role::User => user(i, item, o),
        Role::Assistant if o.view == View::Answers => match answer.and_then(|b| item.blocks.get(b)) {
            Some(Block::Text(s)) => text(s, o),
            _ => Vec::new(),
        },
        Role::Assistant => assistant(i, item, o),
        // The chat view keeps the notices that explain the shape of the conversation.
        Role::Event => match item.blocks.first() {
            Some(Block::Notice(n)) if o.view == View::All || (o.view == View::Chat && n.kind.structural()) => notice(Fold::Notice(i), n, o),
            _ => Vec::new(),
        },
    };
    if !rows.is_empty() {
        rows.push(Row::blank());
    }
    rows
}

fn fold_row(fold: Fold, open: bool, indent: usize, label: String, style: Style) -> Row {
    let mut r = Row::new(vec![
        Span::raw(" ".repeat(indent)),
        Span::styled(if open { "▾ " } else { "▸ " }, theme::FOLD),
        Span::styled(label, style),
    ]);
    r.fold = Some(fold);
    r.pad = indent as u16 + 2;
    r
}

fn user(i: usize, item: &Item, o: &Opts) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let band = |mut r: Row| {
        r.fill = Some((0, theme::PROMPT_BAND));
        r
    };
    for b in &item.blocks {
        match b {
            Block::Text(s) => {
                let mut body = Styled::plain(s.trim_end(), theme::BRIGHT).rows(o.width.saturating_sub(3));
                let total = body.len();
                let fold = Fold::Prompt(i);
                let open = o.open.contains(&fold);
                if total > PROMPT_ROWS && !open {
                    body.truncate(PROMPT_ROWS - 4);
                }
                for r in body {
                    let prefix = if rows.is_empty() { Span::styled("❯ ", theme::PROMPT_MARK) } else { Span::raw("  ") };
                    rows.push(band(r.indent(prefix, true)));
                }
                if total > PROMPT_ROWS {
                    let label = if open { "show less".to_string() } else { format!("{} more lines", total - (PROMPT_ROWS - 4)) };
                    rows.push(band(fold_row(fold, open, 2, label, theme::FOLD)));
                }
            }
            Block::Image(img) => {
                let prefix = if rows.is_empty() { "❯ " } else { "  " };
                rows.push(band(Row::new(vec![
                    Span::styled(prefix, theme::PROMPT_MARK),
                    Span::styled(format!("▣ image ({})", img.mime), theme::MUTED),
                ])));
            }
            _ => {}
        }
    }
    rows
}

/// A message from the agent, marked with a bullet.
fn text(s: &str, o: &Opts) -> Vec<Row> {
    let rows = md::render(s, o.width.saturating_sub(2), theme::TEXT).into_iter();
    rows.enumerate().map(|(k, r)| r.indent(if k == 0 { Span::styled("⏺ ", theme::BULLET) } else { Span::raw("  ") }, true)).collect()
}

fn assistant(i: usize, item: &Item, o: &Opts) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let all = o.view == View::All;
    let blocks = &item.blocks;
    let mut b = 0;
    while b < blocks.len() {
        let mut add = Vec::new();
        match &blocks[b] {
            Block::Text(s) => add = text(s, o),
            Block::Tool(_) | Block::Thinking(_) => match item.run_end(b) {
                Some(end) => {
                    if all {
                        group(i, b, &blocks[b..end], o, &mut add);
                    }
                    b = end - 1;
                }
                None if all => thinking(Fold::Thinking(i, b), &blocks[b], 0, o, &mut add),
                None => {}
            },
            Block::Image(img) => {
                add.push(Row::new(vec![Span::raw("  "), Span::styled(format!("▣ image ({})", img.mime), theme::MUTED)]));
            }
            Block::Notice(n) if all => add = notice(Fold::Notice(i), n, o),
            _ => {}
        }
        if !add.is_empty() {
            if !rows.is_empty() {
                rows.push(Row::blank());
            }
            rows.append(&mut add);
        }
        b += 1;
    }
    rows
}

fn thinking(fold: Fold, block: &Block, indent: usize, o: &Opts, rows: &mut Vec<Row>) {
    let Block::Thinking(s) = block else { return };
    let open = o.open.contains(&fold);
    rows.push(fold_row(fold, open, indent, "✻ Thinking".into(), theme::THINKING));
    if open {
        let pad = " ".repeat(indent + 4);
        for r in md::render(s, o.width.saturating_sub(indent + 6), theme::THINKING) {
            rows.push(r.indent(Span::raw(pad.clone()), true));
        }
    }
}

/// A run of tool calls and thinking as one summary line, listing each when open.
fn group(i: usize, start: usize, run: &[Block], o: &Opts, rows: &mut Vec<Row>) {
    let fold = Fold::Group(i, start);
    let open = o.open.contains(&fold);
    let mut head = fold_row(fold, open, 0, tools::group_summary(run), theme::TOOLS);
    if run.iter().any(|b| matches!(b, Block::Tool(t) if tools::pending(t))) {
        head.spans.push(Span::styled(" …", theme::RUN));
    }
    rows.push(head);
    if !open {
        return;
    }
    for (k, block) in run.iter().enumerate() {
        let b = start + k;
        let Block::Tool(t) = block else {
            thinking(Fold::Thinking(i, b), block, 2, o, rows);
            continue;
        };
        let fold = Fold::Tool(i, b);
        let open = o.open.contains(&fold);
        let status = if tools::pending(t) {
            theme::RUN
        } else if tools::failed(t) {
            theme::ERR
        } else {
            theme::OK
        };
        let room = o.width.saturating_sub(10 + t.name.chars().count());
        let mut r = Row::new(vec![
            Span::raw("  "),
            Span::styled(if open { "▾ " } else { "▸ " }, theme::FOLD),
            Span::styled("● ", status),
            Span::styled(t.name.clone(), theme::STRONG),
            Span::raw("  "),
            Span::styled(truncate(&tools::arg(t), room), theme::MUTED),
        ]);
        r.fold = Some(fold);
        r.pad = 4;
        rows.push(r);
        if open {
            sections(t, o.width, rows);
        }
    }
}

fn sections(t: &Tool, width: usize, rows: &mut Vec<Row>) {
    let inner = width.saturating_sub(6);
    for sec in tools::sections(t) {
        let title_style = if sec.error { theme::ERR } else { theme::DIM };
        rows.push(Row::new(vec![Span::raw("      "), Span::styled(sec.title.clone(), title_style)]));
        let (body, more) = head_lines(&sec.body, SECTION_LINES);
        let body = if sec.markdown { md::render(body, inner, theme::TEXT) } else { md::code_rows(body, &sec.lang, inner) };
        for r in body {
            rows.push(r.indent(Span::raw("      "), true));
        }
        if more > 0 {
            rows.push(Row::new(vec![Span::raw("      "), Span::styled(format!("… {more} more lines (y copies all)"), theme::DIM)]));
        }
    }
    if t.output.is_none() {
        rows.push(Row::new(vec![Span::raw("      "), Span::styled("running…", theme::RUN)]));
    }
}

/// The first `n` lines of `s` and how many lines were left out.
fn head_lines(s: &str, n: usize) -> (&str, usize) {
    match s.match_indices('\n').nth(n.saturating_sub(1)) {
        Some((end, _)) => (&s[..end], s[end + 1..].lines().count()),
        None => (s, 0),
    }
}

fn notice(fold: Fold, n: &Notice, o: &Opts) -> Vec<Row> {
    let (icon, style) = match n.kind {
        NoticeKind::Compaction => ("✻ ", theme::MUTED),
        NoticeKind::Rewind => ("↩ ", theme::WARN),
        NoticeKind::Interrupt => ("⎿ ", theme::ERR),
        NoticeKind::Error => ("✗ ", theme::ERR),
        NoticeKind::Command => ("❯ ", theme::DIM),
        NoticeKind::Shell => ("", theme::DIM),
        NoticeKind::Task => ("✻ ", theme::MUTED),
        NoticeKind::Model => ("⎿ model ", theme::DIM),
        NoticeKind::Info => ("⎿ ", theme::DIM),
    };
    let label = truncate(&format!("{icon}{}", n.label), o.width.saturating_sub(6));
    if n.body.trim().is_empty() {
        let mut r = Row::new(vec![Span::raw("  "), Span::styled(label, style)]);
        r.pad = 2;
        return vec![r];
    }
    let open = o.open.contains(&fold);
    let mut rows = vec![fold_row(fold, open, 0, label, style)];
    if open {
        for r in md::render(&n.body, o.width.saturating_sub(6), theme::MUTED) {
            rows.push(r.indent(Span::raw("    "), true));
        }
    }
    rows
}
