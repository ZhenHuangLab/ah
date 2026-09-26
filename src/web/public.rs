//! The listener a tunnel forwards the public host name to. Shared sessions are open to anyone
//! with their link; everything else needs the cookie that a login link leaves.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Form, Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::time::{Instant, sleep_until, timeout_at};

use super::{Assets, Shared, no_store};
use crate::auth::{COOKIE, Key, SESSION_SECS};
use crate::markdown::escape;
use crate::share;

pub const PORT: u16 = 7448;

pub struct Access {
    pub host: String,
    pub key: Key,
}

const NOINDEX: (header::HeaderName, &str) = (header::HeaderName::from_static("x-robots-tag"), "noindex");

/// Routes open to anyone: shared sessions and signing in.
pub fn open(access: Arc<Access>) -> Router<Shared> {
    Router::new()
        .route("/s/{id}", get(share_page))
        .route("/api/share/{id}", get(share_json))
        .route("/login", get(confirm_login).post(login))
        .layer(axum::middleware::from_fn(no_store))
        .route(
            "/og.png",
            get(|| async { ([(header::CONTENT_TYPE, "image/png")], include_bytes!("../../docs/social-preview.png").as_slice()) }),
        )
        .route("/robots.txt", get(|| async { "User-agent: *\nDisallow: /\n" }))
        .with_state(access)
}

/// The session must cover both the request and the entire response, including event streams.
pub async fn signed_in(State(access): State<Arc<Access>>, req: Request, next: Next) -> Response {
    let Some(remaining) = cookie(req.headers()).and_then(|c| access.key.session_remaining(c)) else { return unauthorized() };
    let deadline = Instant::now() + remaining;
    let Ok(response) = timeout_at(deadline, next.run(req)).await else { return unauthorized() };
    let (parts, body) = response.into_parts();
    Response::from_parts(parts, Body::from_stream(body.into_data_stream().take_until(sleep_until(deadline))))
}

fn unauthorized() -> Response {
    page(StatusCode::UNAUTHORIZED, "<p>Sign in with a link from <code>ah login</code>, run on the machine that serves ah.</p>")
}

fn cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}

#[derive(Deserialize)]
struct LoginQuery {
    t: String,
}

/// Asks before signing in. Chat apps fetch links to preview them, and a preview of a login link
/// must not sign the preview service in; only the button's POST does.
async fn confirm_login(State(access): State<Arc<Access>>, Query(q): Query<LoginQuery>) -> Response {
    if !access.key.check_login(&q.t) {
        return page(StatusCode::FORBIDDEN, "<p>This login link has expired. Run <code>ah login</code> for a new one.</p>");
    }
    let form = format!(
        "<form method=\"post\"><p>Sign this browser in to ah on {}? It stays signed in for 30 days.</p>\
         <input type=\"hidden\" name=\"t\" value=\"{}\"><button style=\"font:inherit;padding:6px 16px\">Sign in</button></form>",
        escape(&access.host),
        escape(&q.t)
    );
    page(StatusCode::OK, &form)
}

async fn login(State(access): State<Arc<Access>>, Form(q): Form<LoginQuery>) -> Response {
    if !access.key.check_login(&q.t) {
        return page(StatusCode::FORBIDDEN, "<p>This login link has expired. Run <code>ah login</code> for a new one.</p>");
    }
    let set = format!("{COOKIE}={}; Max-Age={SESSION_SECS}; Path=/; HttpOnly; Secure; SameSite=Lax", access.key.session());
    (StatusCode::SEE_OTHER, [(header::SET_COOKIE, set), (header::LOCATION, "/".into())]).into_response()
}

fn page(status: StatusCode, body: &str) -> Response {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>ah</title>\
         <meta name=\"robots\" content=\"noindex\"><body style=\"font:16px/1.5 system-ui,sans-serif;max-width:32em;margin:20vh auto;padding:0 1em\">{body}"
    );
    (status, Html(body)).into_response()
}

/// The viewer page, told which share to show. A share that is gone still gets the page, which
/// says so.
async fn share_page(State(access): State<Arc<Access>>, Path(id): Path<String>) -> Response {
    let index = Assets::get("index.html").expect("index.html is embedded");
    let index = String::from_utf8_lossy(&index.data);
    let sh = tokio::task::spawn_blocking({
        let id = id.clone();
        move || share::load(&id)
    })
    .await
    .ok()
    .flatten();
    let mut head =
        format!("<base href=\"../\">\n<meta name=\"ah-share\" content=\"{}\">\n<meta name=\"robots\" content=\"noindex\">\n", escape(&id));
    let status = match &sh {
        Some(s) => {
            let title = escape(s.title());
            let agent = match s.meta["agent"].as_str() {
                Some("claude") => "Claude Code",
                Some("codex") => "Codex",
                _ => "pi",
            };
            let desc = format!("A {agent} session shared with ah. Read and share what your coding agents did.");
            head.push_str(&format!(
                "<title>{title} · ah</title>\n<meta name=\"description\" content=\"{desc}\">\n\
                 <meta property=\"og:type\" content=\"article\">\n<meta property=\"og:site_name\" content=\"ah\">\n\
                 <meta property=\"og:title\" content=\"{title}\">\n<meta property=\"og:description\" content=\"{desc}\">\n\
                 <meta property=\"og:image\" content=\"https://{}/og.png\">\n<meta name=\"twitter:card\" content=\"summary_large_image\">",
                access.host
            ));
            StatusCode::OK
        }
        None => {
            head.push_str("<title>ah</title>");
            StatusCode::NOT_FOUND
        }
    };
    (status, [NOINDEX], Html(index.replacen("<title>ah</title>", &head, 1))).into_response()
}

async fn share_json(Path(id): Path<String>) -> Response {
    match tokio::task::spawn_blocking(move || share::load(&id)).await.ok().flatten() {
        Some(s) => {
            let body = json!({ "view": s.view, "created": s.created, "expires": s.expires, "meta": s.meta, "items": s.items });
            ([NOINDEX], Json(body)).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
