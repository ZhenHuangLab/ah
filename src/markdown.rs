//! Markdown handling shared by both front ends: math delimiter normalization, HTML rendering
//! for the web view and plain-text previews.

use std::borrow::Cow;

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};

pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
}

pub fn parser(src: &str) -> Parser<'_> {
    Parser::new_ext(src, options())
}

/// Rewrites `\(…\)` and `\[…\]` to `$…$` and `$$…$$` outside code so the parser's math
/// extension picks them up; models use both conventions.
pub fn normalize_math(src: &str) -> Cow<'_, str> {
    if !src.contains("\\(") && !src.contains("\\[") {
        return Cow::Borrowed(src);
    }
    let mut out = String::with_capacity(src.len());
    let mut fence: Option<(char, usize)> = None;
    for line in src.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        if let Some(c) = marker {
            let n = trimmed.chars().take_while(|&x| x == c).count();
            if n >= 3 {
                match fence {
                    None => fence = Some((c, n)),
                    Some((fc, fn_)) if fc == c && n >= fn_ && trimmed[n..].trim().is_empty() => fence = None,
                    _ => {}
                }
                out.push_str(line);
                continue;
            }
        }
        if fence.is_some() {
            out.push_str(line);
        } else {
            convert_line(line, &mut out);
        }
    }
    Cow::Owned(out)
}

fn convert_line(line: &str, out: &mut String) {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'`' => {
                // Copy a code span verbatim: a run of n backticks up to the next run of n.
                let n = b[i..].iter().take_while(|&&c| c == b'`').count();
                let close = find_run(&b[i + n..], n).map(|p| i + n + p + n);
                let end = close.unwrap_or(i + n);
                out.push_str(&line[i..end]);
                i = end;
            }
            b'\\' if i + 1 < b.len() => {
                match b[i + 1] {
                    b'(' => {
                        out.push('$');
                        i += 2;
                        while i < b.len() && b[i] == b' ' {
                            i += 1;
                        }
                    }
                    b')' => {
                        while out.ends_with(' ') {
                            out.pop();
                        }
                        out.push('$');
                        i += 2;
                    }
                    b'[' | b']' => {
                        out.push_str("$$");
                        i += 2;
                    }
                    b'\\' => {
                        out.push_str("\\\\");
                        i += 2;
                    }
                    _ => {
                        out.push('\\');
                        i += 1;
                    }
                }
            }
            _ => {
                let next = b[i..].iter().position(|&c| c == b'`' || c == b'\\').map_or(b.len(), |p| i + p.max(1));
                out.push_str(&line[i..next]);
                i = next;
            }
        }
    }
}

/// Position of the first run of exactly `n` backticks.
fn find_run(b: &[u8], n: usize) -> Option<usize> {
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            let m = b[i..].iter().take_while(|&&c| c == b'`').count();
            if m == n {
                return Some(i);
            }
            i += m;
        } else {
            i += 1;
        }
    }
    None
}

/// Inline tags models use in tables and prose that are safe to pass through.
const SAFE_INLINE: &[&str] = &["<br>", "<br/>", "<br />", "<sup>", "</sup>", "<sub>", "</sub>", "<kbd>", "</kbd>", "<b>", "</b>", "<i>", "</i>", "<u>", "</u>"];

fn safe_url(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    !(lower.starts_with("javascript:") || lower.starts_with("vbscript:") || lower.starts_with("data:"))
}

/// Renders Markdown to HTML. Raw HTML is shown as text, math is left in `span.math` elements
/// for KaTeX and code blocks carry `language-*` classes for highlighting.
pub fn to_html(src: &str) -> String {
    let src = normalize_math(src);
    let events = parser(&src).map(|e| match e {
        Event::Start(Tag::HtmlBlock) => Event::Html(CowStr::Borrowed("<pre class=\"raw\">")),
        Event::End(TagEnd::HtmlBlock) => Event::Html(CowStr::Borrowed("</pre>")),
        Event::InlineHtml(h) if SAFE_INLINE.contains(&h.to_ascii_lowercase().as_str()) => Event::InlineHtml(h),
        Event::Html(h) | Event::InlineHtml(h) => Event::Text(h),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) if !safe_url(&dest_url) => {
            Event::Start(Tag::Link { link_type, dest_url: CowStr::Borrowed("#"), title, id })
        }
        Event::Start(Tag::Image { link_type, dest_url, title, id }) if !safe_url(&dest_url) => {
            Event::Start(Tag::Image { link_type, dest_url: CowStr::Borrowed(""), title, id })
        }
        e => e,
    });
    let mut out = String::with_capacity(src.len() * 3 / 2);
    html::push_html(&mut out, events);
    out
}

/// The text content of Markdown on one line, cut to `max` characters.
pub fn plain(src: &str, max: usize) -> String {
    let mut out = String::new();
    for e in parser(src) {
        match e {
            Event::Text(s) | Event::Code(s) | Event::InlineMath(s) | Event::DisplayMath(s) => out.push_str(&s),
            Event::SoftBreak | Event::HardBreak | Event::End(_) => out.push(' '),
            _ => {}
        }
        if out.len() > max * 4 {
            break;
        }
    }
    crate::parse::one_line(&out, max)
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
