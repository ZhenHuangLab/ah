//! Transcript items as HTML fragments and JSON for the web front end.

use base64::Engine;
use serde::Serialize;
use serde_json::{Value, json};

use crate::live::Live;
use crate::markdown::{escape, plain, to_html};
use crate::model::{Block, Image, Item, Notice, NoticeKind, Role, Tool};
use crate::parse::one_line;
use crate::tools;

/// Tool output beyond this is cut in the detail view.
const DETAIL_CAP: usize = 512 * 1024;

#[derive(Serialize)]
pub struct ItemJson {
    i: usize,
    role: Role,
    time: Option<i64>,
    html: String,
    /// Markdown for "copy".
    md: String,
    /// One line of plain text for the navigation rail.
    preview: String,
    kind: Option<NoticeKind>,
}

pub fn meta(l: &Live) -> Value {
    let mut v = serde_json::to_value(&l.meta).unwrap_or_default();
    v["model"] = json!(l.t.info.model);
    v
}

pub fn snapshot(l: &Live) -> Value {
    json!({
        "gen": l.generation,
        "rev": l.t.rev,
        "meta": meta(l),
        "items": since(l, 0),
    })
}

/// Items changed after revision `rev`.
pub fn since(l: &Live, rev: u64) -> Vec<ItemJson> {
    let sid = escape(&l.meta.id);
    l.t.items.iter().enumerate().filter(|(_, it)| it.rev > rev).map(|(i, it)| item(&sid, i, it)).collect()
}

fn item(sid: &str, i: usize, it: &Item) -> ItemJson {
    let mut html = String::new();
    let mut b = 0;
    while b < it.blocks.len() {
        match &it.blocks[b] {
            Block::Text(s) if it.role == Role::User => {
                html.push_str(&format!("<div class=\"prompt\">{}</div>", escape(s.trim_end())));
            }
            Block::Text(s) => {
                html.push_str("<div class=\"md\">");
                html.push_str(&to_html(s));
                html.push_str("</div>");
            }
            Block::Thinking(s) => {
                html.push_str(&format!("<details class=\"thinking\" data-k=\"h{b}\"><summary>Thinking</summary><div class=\"md\">"));
                html.push_str(&to_html(s));
                html.push_str("</div></details>");
            }
            Block::Tool(_) => {
                let start = b;
                while b + 1 < it.blocks.len() && matches!(it.blocks[b + 1], Block::Tool(_)) {
                    b += 1;
                }
                html.push_str(&group(sid, i, start, &it.blocks[start..=b]));
            }
            Block::Image(_) => html.push_str(&img(sid, i, b, None)),
            Block::Notice(n) => html.push_str(&notice(n)),
        }
        b += 1;
    }
    let (md, preview) = match it.role {
        Role::User => {
            let text = it.text();
            let preview = one_line(&text, 240);
            (text, preview)
        }
        Role::Assistant => (it.text(), it.last_text().map(|s| plain(s, 240)).unwrap_or_default()),
        Role::Event => (String::new(), String::new()),
    };
    let kind = it.blocks.iter().find_map(|b| match b {
        Block::Notice(n) => Some(n.kind),
        _ => None,
    });
    ItemJson { i, role: it.role, time: it.time, html, md, preview, kind }
}

fn group(sid: &str, i: usize, start: usize, blocks: &[Block]) -> String {
    let list: Vec<&Tool> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Tool(t) => Some(t),
            _ => None,
        })
        .collect();
    let running = if list.iter().any(|t| tools::pending(t)) { " running" } else { "" };
    let mut s = format!(
        "<details class=\"tools{running}\" data-k=\"g{start}\"><summary>{}</summary><div class=\"tool-list\">",
        escape(&tools::group_summary(list.iter().copied()))
    );
    for (k, t) in list.iter().enumerate() {
        let b = start + k;
        let status = if tools::pending(t) {
            "run"
        } else if tools::failed(t) {
            "err"
        } else {
            "ok"
        };
        s.push_str(&format!(
            "<details class=\"tool {status}\" data-k=\"t{b}\" data-src=\"api/s/{sid}/tool/{i}/{b}\"><summary><b>{}</b><code>{}</code></summary><div class=\"tool-body\"></div></details>",
            escape(&t.name),
            escape(&tools::arg(t))
        ));
    }
    s.push_str("</div></details>");
    s
}

fn notice(n: &Notice) -> String {
    let kind = n.kind.name();
    let label = escape(&n.label);
    if n.body.trim().is_empty() {
        format!("<div class=\"notice {kind}\"><span>{label}</span></div>")
    } else {
        format!(
            "<details class=\"notice {kind}\" data-k=\"n\"><summary>{label}</summary><div class=\"md\">{}</div></details>",
            to_html(&n.body)
        )
    }
}

fn img(sid: &str, i: usize, b: usize, k: Option<usize>) -> String {
    let q = k.map(|k| format!("?k={k}")).unwrap_or_default();
    format!("<img class=\"att\" loading=\"lazy\" alt=\"image\" src=\"api/s/{sid}/img/{i}/{b}{q}\">")
}

/// The expanded view of one tool call.
pub fn tool_detail(sid: &str, i: usize, b: usize, t: &Tool) -> String {
    let sid = escape(sid);
    let mut s = String::new();
    for sec in tools::sections(t) {
        let cut = cap(&sec.body, DETAIL_CAP);
        s.push_str(&format!("<div class=\"sec{}\"><div class=\"sec-t\">{}</div>", if sec.error { " err" } else { "" }, escape(&sec.title)));
        if sec.markdown {
            s.push_str(&format!("<div class=\"md\">{}</div>", to_html(cut)));
        } else {
            let class = if sec.lang.is_empty() { "nohighlight".to_string() } else { format!("language-{}", escape(&sec.lang)) };
            s.push_str(&format!("<pre><code class=\"{class}\">{}</code></pre>", escape(cut)));
        }
        if cut.len() < sec.body.len() {
            s.push_str(&format!("<div class=\"cut\">… {} more bytes not shown</div>", sec.body.len() - cut.len()));
        }
        s.push_str("</div>");
    }
    if let Some(o) = &t.output {
        for k in 0..o.images.len() {
            s.push_str(&img(&sid, i, b, Some(k)));
        }
    }
    if t.output.is_none() {
        s.push_str("<div class=\"sec\"><div class=\"sec-t\">running…</div></div>");
    }
    s
}

fn cap(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut n = max;
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

/// The image at `(i, b)`: an image block, or output image `k` of a tool call.
pub fn image(it: &Item, b: usize, k: Option<usize>) -> Option<(String, Vec<u8>)> {
    let img: &Image = match (it.blocks.get(b)?, k) {
        (Block::Image(img), None) => img,
        (Block::Tool(t), Some(k)) => t.output.as_ref()?.images.get(k)?,
        _ => return None,
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(img.data.as_bytes()).ok()?;
    Some((img.mime.clone(), bytes))
}
