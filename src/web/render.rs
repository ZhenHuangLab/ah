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
            Block::Tool(_) | Block::Thinking(_) => match it.run_end(b) {
                Some(end) => {
                    html.push_str(&group(sid, i, b, &it.blocks[b..end]));
                    b = end - 1;
                }
                None => html.push_str(&thinking(sid, i, b)),
            },
            Block::Image(_) => html.push_str(&img(sid, i, b, None)),
            Block::Notice(n) => html.push_str(&notice(n, &src(sid, i, b))),
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

fn thinking(sid: &str, i: usize, b: usize) -> String {
    format!(
        "<details class=\"thinking\" data-k=\"h{b}\" data-src=\"{}\"><summary>Thinking</summary><div class=\"body md\"></div></details>",
        src(sid, i, b)
    )
}

/// A run of tool calls and thinking as one summary line; its rows load when it is opened.
fn group(sid: &str, i: usize, start: usize, run: &[Block]) -> String {
    let running = if run.iter().any(|b| matches!(b, Block::Tool(t) if tools::pending(t))) { " running" } else { "" };
    format!(
        "<details class=\"tools{running}\" data-k=\"g{start}\" data-src=\"api/s/{sid}/run/{i}/{start}\"><summary>{}</summary><div class=\"body tool-list\"></div></details>",
        escape(&tools::group_summary(run))
    )
}

/// One row per tool call or thinking block in the run starting at block `start`.
pub fn run_rows(sid: &str, i: usize, start: usize, it: &Item) -> Option<String> {
    let end = it.run_end(start)?;
    let sid = escape(sid);
    let mut s = String::new();
    for (b, block) in it.blocks.iter().enumerate().take(end).skip(start) {
        let Block::Tool(t) = block else {
            s.push_str(&thinking(&sid, i, b));
            continue;
        };
        let status = if tools::pending(t) {
            "run"
        } else if tools::failed(t) {
            "err"
        } else {
            "ok"
        };
        s.push_str(&format!(
            "<details class=\"tool {status}\" data-k=\"t{b}\" data-src=\"{}\"><summary><b>{}</b><code>{}</code></summary><div class=\"body tool-body\"></div></details>",
            src(&sid, i, b),
            escape(&t.name),
            escape(&tools::arg(t))
        ));
    }
    Some(s)
}

/// Where the expanded content of a block is fetched from.
fn src(sid: &str, i: usize, b: usize) -> String {
    format!("api/s/{sid}/block/{i}/{b}")
}

fn notice(n: &Notice, src: &str) -> String {
    let kind = n.kind.name();
    let label = escape(&n.label);
    if n.body.trim().is_empty() {
        format!("<div class=\"notice {kind}\"><span>{label}</span></div>")
    } else {
        format!(
            "<details class=\"notice {kind}\" data-k=\"n\" data-src=\"{src}\"><summary>{label}</summary><div class=\"body md\"></div></details>"
        )
    }
}

/// The expanded content of a foldable block: tool details, thinking or a notice body.
pub fn block(sid: &str, i: usize, b: usize, block: &Block) -> Option<String> {
    match block {
        Block::Tool(t) => Some(tool_detail(sid, i, b, t)),
        Block::Thinking(s) => Some(to_html(s)),
        Block::Notice(n) => Some(to_html(&n.body)),
        _ => None,
    }
}

fn img(sid: &str, i: usize, b: usize, k: Option<usize>) -> String {
    let q = k.map(|k| format!("?k={k}")).unwrap_or_default();
    format!("<img class=\"att\" loading=\"lazy\" alt=\"image\" src=\"api/s/{sid}/img/{i}/{b}{q}\">")
}

/// The expanded view of one tool call.
fn tool_detail(sid: &str, i: usize, b: usize, t: &Tool) -> String {
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
