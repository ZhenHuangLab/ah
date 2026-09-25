//! Markdown to terminal rows: headings, nested lists, quotes, code blocks, box-drawn tables
//! and math shown as TeX source.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use super::tex;
use super::text::{Row, Styled, spans_width, width};
use super::theme;
use crate::markdown;

enum Container {
    Quote,
    List(Option<u64>),
    Item { marker: String, shown: bool },
}

struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Styled>>,
    head: usize,
    cell: Option<Styled>,
}

struct R {
    width: usize,
    base: Style,
    rows: Vec<Row>,
    inline: Styled,
    styles: Vec<Style>,
    containers: Vec<Container>,
    code: Option<(String, String)>,
    table: Option<Table>,
    gap: bool,
}

/// Renders `src` to rows of at most `width` columns, with `base` as the text style.
pub fn render(src: &str, width: usize, base: Style) -> Vec<Row> {
    let src = markdown::normalize_math(src);
    let mut r = R {
        width: width.max(10),
        base,
        rows: Vec::new(),
        inline: Styled::default(),
        styles: vec![base],
        containers: Vec::new(),
        code: None,
        table: None,
        gap: false,
    };
    for e in markdown::events(&src) {
        r.event(e);
    }
    r.flush();
    r.rows
}

/// A code or preformatted block, with `diff` coloring when asked.
pub fn code_rows(text: &str, lang: &str, width: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    let diff = lang == "diff" || lang == "patch";
    for line in text.trim_end_matches('\n').split('\n') {
        let style = if diff { diff_style(line) } else { theme::CODE_BLOCK };
        for row in Styled::plain(line, style).rows(width.saturating_sub(2).max(4)) {
            let mut row = row.indent(Span::raw(" "), false);
            row.fill = Some((0, theme::CODE_BG));
            rows.push(row);
        }
    }
    rows
}

fn diff_style(line: &str) -> Style {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("***") {
        theme::MUTED
    } else if line.starts_with('+') {
        theme::ADDED
    } else if line.starts_with('-') {
        theme::REMOVED
    } else if line.starts_with("@@") {
        theme::HUNK
    } else {
        theme::CODE_BLOCK
    }
}

impl R {
    fn style(&self) -> Style {
        *self.styles.last().unwrap_or(&self.base)
    }

    fn push_style(&mut self, s: Style) {
        let st = self.style().patch(s);
        self.styles.push(st);
    }

    fn text(&mut self, s: &str, style: Style) {
        match self.table.as_mut().and_then(|t| t.cell.as_mut()) {
            Some(cell) => cell.push(s, style),
            None => self.inline.push(s, style),
        }
    }

    /// Container prefixes for the first row of a block and for the rows after it.
    fn prefixes(&mut self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let (mut first, mut rest) = (Vec::new(), Vec::new());
        for c in &mut self.containers {
            match c {
                Container::Quote => {
                    first.push(Span::styled("│ ", theme::RULE));
                    rest.push(Span::styled("│ ", theme::RULE));
                }
                Container::List(_) => {}
                Container::Item { marker, shown } => {
                    let pad = " ".repeat(width(marker));
                    if *shown {
                        first.push(Span::raw(pad.clone()));
                    } else {
                        first.push(Span::styled(marker.clone(), theme::MUTED));
                        *shown = true;
                    }
                    rest.push(Span::raw(pad));
                }
            }
        }
        (first, rest)
    }

    fn quoted(&self) -> bool {
        self.containers.iter().any(|c| matches!(c, Container::Quote))
    }

    /// Separates blocks with one blank row.
    fn block(&mut self) {
        self.flush();
        if self.gap && !self.rows.is_empty() {
            let spans = if self.quoted() { vec![Span::styled("│", theme::RULE)] } else { Vec::new() };
            self.rows.push(Row::new(spans));
        }
        self.gap = false;
    }

    fn emit(&mut self, body: Vec<Row>, first: Vec<Span<'static>>, rest: Vec<Span<'static>>) {
        for (k, mut row) in body.into_iter().enumerate() {
            let prefix = if k == 0 { &first } else { &rest };
            let w = spans_width(prefix);
            let mut spans = prefix.clone();
            spans.append(&mut row.spans);
            row.spans = spans;
            if let Some((col, _)) = &mut row.fill {
                *col += w as u16;
            }
            self.rows.push(row);
        }
    }

    fn flush(&mut self) {
        if self.inline.is_blank() {
            self.inline = Styled::default();
            return;
        }
        let inline = std::mem::take(&mut self.inline);
        let (first, rest) = self.prefixes();
        let avail = self.width.saturating_sub(spans_width(&first)).max(8);
        let body = inline.rows(avail);
        self.emit(body, first, rest);
    }

    fn event(&mut self, e: Event) {
        if let Some((_, buf)) = &mut self.code {
            match e {
                Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, text) = self.code.take().unwrap_or_default();
                    let (first, rest) = self.prefixes();
                    let avail = self.width.saturating_sub(spans_width(&first));
                    let body = code_rows(&text, &lang, avail);
                    self.emit(body, first, rest);
                    self.gap = true;
                }
                _ => {}
            }
            return;
        }
        match e {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(&t, self.style()),
            Event::Code(t) => self.text(&t, self.style().patch(theme::CODE)),
            Event::InlineMath(t) => self.text(&tex::unicode(&t), self.style().patch(theme::MATH)),
            Event::DisplayMath(t) => {
                self.flush();
                let (first, rest) = self.prefixes();
                let avail = self.width.saturating_sub(spans_width(&first) + 2);
                let math = tex::unicode(&t);
                let lines: Vec<&str> = math.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
                let body: Vec<Row> = Styled::plain(&lines.join("\n"), theme::MATH)
                    .rows(avail)
                    .into_iter()
                    .map(|r| r.indent(Span::raw("  "), false))
                    .collect();
                self.emit(body, first, rest);
            }
            Event::Html(t) | Event::InlineHtml(t) => self.text(&t, self.style().patch(theme::MUTED)),
            Event::SoftBreak => self.text(" ", self.style()),
            Event::HardBreak => self.text("\n", self.style()),
            Event::Rule => {
                self.block();
                let (first, _) = self.prefixes();
                let w = self.width.saturating_sub(spans_width(&first));
                let mut spans = first;
                spans.push(Span::styled("─".repeat(w), theme::RULE));
                self.rows.push(Row::new(spans));
                self.gap = true;
            }
            Event::TaskListMarker(done) => self.text(if done { "☑ " } else { "☐ " }, theme::MUTED),
            Event::FootnoteReference(name) => self.text(&format!("[^{name}]"), theme::MUTED),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.block(),
            Tag::Heading { level, .. } => {
                self.block();
                let s = match level {
                    HeadingLevel::H1 => theme::HEADING.add_modifier(Modifier::UNDERLINED),
                    HeadingLevel::H2 => theme::HEADING,
                    _ => theme::STRONG,
                };
                self.push_style(s);
            }
            Tag::BlockQuote(_) => {
                self.block();
                self.containers.push(Container::Quote);
                self.push_style(theme::QUOTE);
            }
            Tag::CodeBlock(kind) => {
                self.block();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::HtmlBlock => self.block(),
            Tag::List(start) => {
                let nested = self.containers.iter().any(|c| matches!(c, Container::Item { .. }));
                if nested {
                    self.flush();
                } else {
                    self.block();
                }
                self.containers.push(Container::List(start));
            }
            Tag::Item => {
                self.flush();
                let depth = self.containers.iter().filter(|c| matches!(c, Container::List(_))).count();
                let marker = match self.containers.last_mut() {
                    Some(Container::List(Some(n))) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => (if depth > 1 { "◦ " } else { "• " }).to_string(),
                };
                self.containers.push(Container::Item { marker, shown: false });
            }
            Tag::Table(aligns) => {
                self.block();
                self.table = Some(Table { aligns, rows: Vec::new(), head: 0, cell: None });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableCell => {
                if let Some(t) = &mut self.table {
                    t.cell = Some(Styled::default());
                }
            }
            Tag::Emphasis => self.push_style(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(theme::STRONG),
            Tag::Strikethrough => self.push_style(Style::new().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => self.push_style(theme::LINK),
            Tag::Image { .. } => {
                self.text("[image: ", theme::MUTED);
                self.push_style(theme::MUTED);
            }
            Tag::FootnoteDefinition(name) => {
                self.block();
                self.text(&format!("[^{name}]: "), theme::MUTED);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::HtmlBlock | TagEnd::FootnoteDefinition => {
                self.flush();
                self.gap = true;
            }
            TagEnd::Heading(_) => {
                self.flush();
                self.styles.pop();
                self.gap = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.containers.pop();
                self.styles.pop();
                self.gap = true;
            }
            TagEnd::List(_) => {
                self.flush();
                self.containers.pop();
                self.gap = !self.containers.iter().any(|c| matches!(c, Container::Item { .. }));
            }
            TagEnd::Item => {
                self.flush();
                self.containers.pop();
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.head = t.rows.len();
                }
            }
            TagEnd::TableCell => {
                if let Some(t) = &mut self.table {
                    let cell = t.cell.take().unwrap_or_default();
                    if let Some(row) = t.rows.last_mut() {
                        row.push(cell);
                    }
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.table(t);
                }
                self.gap = true;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.styles.pop();
            }
            TagEnd::Image => {
                self.styles.pop();
                self.text("]", theme::MUTED);
            }
            _ => {}
        }
    }

    fn table(&mut self, t: Table) {
        let ncol = t.rows.iter().map(Vec::len).max().unwrap_or(0);
        if ncol == 0 {
            return;
        }
        let (first, rest) = self.prefixes();
        let indent: usize = spans_width(&first);
        let avail = self.width.saturating_sub(indent + 3 * ncol + 1).max(ncol);
        let mut widths: Vec<usize> = (0..ncol)
            .map(|c| {
                t.rows.iter().filter_map(|r| r.get(c)).map(|s| s.text.split('\n').map(width).max().unwrap_or(0)).max().unwrap_or(0).max(1)
            })
            .collect();
        // Narrow the widest columns until the table fits.
        while widths.iter().sum::<usize>() > avail {
            let (i, &w) = widths.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
            if w <= 3 {
                break;
            }
            widths[i] = w - 1;
        }
        let border = |l: &str, m: &str, r: &str| {
            let mut s = String::from(l);
            for (i, w) in widths.iter().enumerate() {
                s.push_str(&"─".repeat(w + 2));
                s.push_str(if i + 1 == ncol { r } else { m });
            }
            Row::new(vec![Span::styled(s, theme::RULE)])
        };
        let mut body = vec![border("┌", "┬", "┐")];
        let cells: Vec<Vec<Vec<Row>>> =
            t.rows.iter().map(|r| (0..ncol).map(|c| r.get(c).map(|s| s.rows(widths[c])).unwrap_or_default()).collect()).collect();
        let tall = cells.iter().any(|r| r.iter().any(|c| c.len() > 1));
        for (ri, row) in cells.into_iter().enumerate() {
            let height = row.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for line in 0..height {
                let mut spans = vec![Span::styled("│", theme::RULE)];
                for (c, cell) in row.iter().enumerate() {
                    let content = cell.get(line).map(|r| r.spans.clone()).unwrap_or_default();
                    let used = spans_width(&content);
                    let space = widths[c].saturating_sub(used);
                    let (left, right) = match t.aligns.get(c) {
                        Some(Alignment::Right) => (space, 0),
                        Some(Alignment::Center) => (space / 2, space - space / 2),
                        _ if ri < t.head => (space / 2, space - space / 2),
                        _ => (0, space),
                    };
                    spans.push(Span::raw(" ".repeat(left + 1)));
                    for mut s in content {
                        if ri < t.head {
                            s.style = s.style.add_modifier(Modifier::BOLD);
                        }
                        spans.push(s);
                    }
                    spans.push(Span::raw(" ".repeat(right + 1)));
                    spans.push(Span::styled("│", theme::RULE));
                }
                body.push(Row::new(spans));
            }
            let last = ri + 1 == t.rows.len();
            if !last && (ri + 1 == t.head || tall) {
                body.push(border("├", "┼", "┤"));
            }
        }
        body.push(border("└", "┴", "┘"));
        self.emit(body, first, rest);
    }
}
