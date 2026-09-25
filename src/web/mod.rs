//! The web viewer: an HTTP server with a small single-page front end. Sessions are parsed on
//! first view and kept in sync by a file watcher; pages follow them over server-sent events.

mod render;

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::stream::{self, Stream};
use notify::{RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast::{self, error::RecvError};
use tower_http::compression::CompressionLayer;

use crate::discover::{self, Roots, mtime_ms};
use crate::live::Live;
use crate::model::{Block, SessionMeta};

#[derive(rust_embed::Embed)]
#[folder = "assets/"]
struct Assets;

type Shared = Arc<AppState>;

struct AppState {
    roots: Roots,
    index: RwLock<Index>,
    open: Mutex<HashMap<String, Open>>,
    tx: broadcast::Sender<Change>,
}

/// A parsed session. Streams hold clones of `live`; idle ones are dropped after a while.
struct Open {
    live: Arc<Mutex<Live>>,
    path: PathBuf,
    used: Instant,
}

#[derive(Default)]
struct Index {
    by_id: HashMap<String, SessionMeta>,
    by_path: HashMap<PathBuf, String>,
}

impl Index {
    fn insert(&mut self, mut m: SessionMeta) -> SessionMeta {
        if let Some(old) = self.by_path.get(&m.path) {
            m.id = old.clone();
        } else {
            let base = m.id.clone();
            let mut n = 1;
            while self.by_id.contains_key(&m.id) {
                n += 1;
                m.id = format!("{base}~{n}");
            }
        }
        self.by_path.insert(m.path.clone(), m.id.clone());
        self.by_id.insert(m.id.clone(), m.clone());
        m
    }
}

#[derive(Clone)]
enum Change {
    Meta(SessionMeta),
    Gone(String),
    Live(String),
}

pub fn serve(addrs: Vec<SocketAddr>) -> Result<()> {
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run(addrs))
}

async fn run(addrs: Vec<SocketAddr>) -> Result<()> {
    let roots = Roots::detect();
    let list = tokio::task::spawn_blocking({
        let roots = roots.clone();
        move || discover::scan(&roots)
    })
    .await?;
    let mut index = Index::default();
    for m in list {
        index.insert(m);
    }
    let (tx, _) = broadcast::channel(1024);
    let st = Arc::new(AppState { roots, index: RwLock::new(index), open: Mutex::new(HashMap::new()), tx });
    watch(st.clone())?;

    let app = Router::new()
        .route("/", get(|| async { asset("index.html") }))
        .route("/assets/{*path}", get(|Path(p): Path<String>| async move { asset(&p) }))
        .route("/api/sessions", get(sessions))
        .route("/api/events", get(events))
        .route("/api/s/{id}", get(session))
        .route("/api/s/{id}/events", get(session_events))
        .route("/api/s/{id}/tool/{item}/{block}", get(tool))
        .route("/api/s/{id}/img/{item}/{block}", get(image))
        .layer(CompressionLayer::new())
        .with_state(st);

    let mut servers = Vec::new();
    for addr in addrs {
        let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("listening on {addr}"))?;
        eprintln!("ah: serving http://{addr}/");
        let app = app.clone();
        servers.push(tokio::spawn(async move { axum::serve(listener, app).await }));
    }
    tokio::select! {
        _ = tokio::signal::ctrl_c() => Ok(()),
        (res, _, _) = futures_util::future::select_all(servers) => Ok(res??),
    }
}

fn asset(path: &str) -> Response {
    let Some(f) = Assets::get(path) else { return StatusCode::NOT_FOUND.into_response() };
    let cache = if path.starts_with("vendor/") { "public, max-age=604800" } else { "no-cache" };
    ([(header::CONTENT_TYPE, f.metadata.mimetype().to_string()), (header::CACHE_CONTROL, cache.to_string())], f.data).into_response()
}

impl AppState {
    /// The parsed session `id`, loading it on first use. Blocking.
    fn live(&self, id: &str) -> Result<Arc<Mutex<Live>>, StatusCode> {
        if let Some(o) = self.open.lock().unwrap().get_mut(id) {
            o.used = Instant::now();
            return Ok(o.live.clone());
        }
        let meta = self.index.read().unwrap().by_id.get(id).cloned().ok_or(StatusCode::NOT_FOUND)?;
        let path = meta.path.clone();
        let live = Live::open(meta).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut open = self.open.lock().unwrap();
        let o = open.entry(id.to_string()).or_insert_with(|| Open { live: Arc::new(Mutex::new(live)), path, used: Instant::now() });
        Ok(o.live.clone())
    }

    /// Applies a change to `path`: refreshes open sessions and the index.
    fn changed(&self, path: &FsPath) {
        let open: Vec<(String, Arc<Mutex<Live>>)> =
            self.open.lock().unwrap().iter().filter(|(_, o)| o.path == path).map(|(id, o)| (id.clone(), o.live.clone())).collect();
        let mut title = None;
        for (id, live) in open {
            let mut l = live.lock().unwrap();
            if let Ok(true) = l.refresh() {
                let _ = self.tx.send(Change::Live(id));
            }
            title = Some(l.meta.title.clone());
        }
        let Some(agent) = self.roots.classify(path) else { return };
        let Ok(md) = fs::metadata(path) else {
            let mut idx = self.index.write().unwrap();
            if let Some(id) = idx.by_path.remove(path) {
                idx.by_id.remove(&id);
                let _ = self.tx.send(Change::Gone(id));
            }
            return;
        };
        let known = self.index.read().unwrap().by_path.get(path).cloned();
        let meta = match known {
            Some(id) => {
                let mut idx = self.index.write().unwrap();
                let Some(m) = idx.by_id.get_mut(&id) else { return };
                m.size = md.len();
                m.modified = mtime_ms(&md);
                if let Some(t) = title {
                    m.title = t;
                }
                m.clone()
            }
            None => match discover::meta(agent, path, true) {
                Some(m) => self.index.write().unwrap().insert(m),
                None => return,
            },
        };
        let _ = self.tx.send(Change::Meta(meta));
    }

    fn evict(&self) {
        self.open.lock().unwrap().retain(|_, o| Arc::strong_count(&o.live) > 1 || o.used.elapsed() < Duration::from_secs(600));
    }
}

/// Watches the agents' session directories and feeds changed transcripts to `changed`,
/// coalescing bursts of writes.
fn watch(st: Shared) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            for p in ev.paths.into_iter().filter(|p| p.extension().is_some_and(|e| e == "jsonl")) {
                let _ = tx.send(p);
            }
        }
    })?;
    for dir in st.roots.dirs() {
        watcher.watch(dir, RecursiveMode::Recursive).with_context(|| format!("watching {}", dir.display()))?;
    }
    std::thread::spawn(move || {
        let _watcher = watcher;
        let mut swept = Instant::now();
        loop {
            let mut paths = HashSet::new();
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(p) => {
                    paths.insert(p);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let until = Instant::now() + Duration::from_millis(100);
            while let Some(left) = until.checked_duration_since(Instant::now()) {
                match rx.recv_timeout(left) {
                    Ok(p) => {
                        paths.insert(p);
                    }
                    Err(_) => break,
                }
            }
            for p in paths {
                st.changed(&p);
            }
            if swept.elapsed() > Duration::from_secs(60) {
                st.evict();
                swept = Instant::now();
            }
        }
    });
    Ok(())
}

async fn sessions(State(st): State<Shared>) -> Json<serde_json::Value> {
    let mut list: Vec<SessionMeta> = st.index.read().unwrap().by_id.values().cloned().collect();
    list.sort_by_key(|m| std::cmp::Reverse(m.modified));
    let home = std::env::var("HOME").unwrap_or_default();
    Json(json!({ "home": home, "sessions": list }))
}

fn sse(s: impl Stream<Item = Result<Event, Infallible>> + Send + 'static) -> Response {
    Sse::new(s).keep_alive(KeepAlive::default()).into_response()
}

/// Session list changes.
async fn events(State(st): State<Shared>) -> Response {
    let s = stream::unfold(st.tx.subscribe(), |mut rx| async move {
        loop {
            let ev = match rx.recv().await {
                Ok(Change::Meta(m)) => Event::default().event("meta").json_data(&m).ok()?,
                Ok(Change::Gone(id)) => Event::default().event("gone").data(id),
                Ok(Change::Live(_)) => continue,
                Err(RecvError::Lagged(_)) => Event::default().event("reload").data(""),
                Err(RecvError::Closed) => return None,
            };
            return Some((Ok(ev), rx));
        }
    });
    sse(s)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, StatusCode> + Send + 'static) -> Result<T, StatusCode> {
    tokio::task::spawn_blocking(f).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

async fn session(State(st): State<Shared>, Path(id): Path<String>) -> Result<Response, StatusCode> {
    blocking(move || {
        let live = st.live(&id)?;
        let l = live.lock().unwrap();
        Ok(Json(render::snapshot(&l)).into_response())
    })
    .await
}

#[derive(Deserialize)]
struct Since {
    #[serde(rename = "gen")]
    generation: u64,
    rev: u64,
}

/// Items of one session that changed after the client's revision. A `reset` event asks the
/// client to reload the snapshot (the transcript was rebuilt or the server restarted).
async fn session_events(State(st): State<Shared>, Path(id): Path<String>, Query(since): Query<Since>) -> Result<Response, StatusCode> {
    let live = blocking({
        let st = st.clone();
        let id = id.clone();
        move || st.live(&id)
    })
    .await?;
    struct S {
        live: Arc<Mutex<Live>>,
        rx: broadcast::Receiver<Change>,
        id: String,
        generation: u64,
        rev: u64,
        first: bool,
    }
    let init = S { live, rx: st.tx.subscribe(), id, generation: since.generation, rev: since.rev, first: true };
    let s = stream::unfold(init, |mut s| async move {
        loop {
            if !std::mem::take(&mut s.first) {
                match s.rx.recv().await {
                    Ok(Change::Live(id)) if id == s.id => {}
                    Ok(_) => continue,
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => return None,
                }
            }
            let ev = {
                let l = s.live.lock().unwrap();
                if l.generation != s.generation {
                    s.generation = l.generation;
                    s.rev = l.t.rev;
                    Some(Event::default().event("reset").data(""))
                } else if l.t.rev > s.rev {
                    let data = json!({ "rev": l.t.rev, "meta": render::meta(&l), "items": render::since(&l, s.rev) });
                    s.rev = l.t.rev;
                    Event::default().event("items").json_data(&data).ok()
                } else {
                    None
                }
            };
            if let Some(ev) = ev {
                return Some((Ok(ev), s));
            }
        }
    });
    Ok(sse(s))
}

async fn tool(State(st): State<Shared>, Path((id, i, b)): Path<(String, usize, usize)>) -> Result<Html<String>, StatusCode> {
    blocking(move || {
        let live = st.live(&id)?;
        let l = live.lock().unwrap();
        match l.t.items.get(i).and_then(|it| it.blocks.get(b)) {
            Some(Block::Tool(t)) => Ok(Html(render::tool_detail(&id, i, b, t))),
            _ => Err(StatusCode::NOT_FOUND),
        }
    })
    .await
}

#[derive(Deserialize)]
struct ImageQuery {
    k: Option<usize>,
}

async fn image(
    State(st): State<Shared>,
    Path((id, i, b)): Path<(String, usize, usize)>,
    Query(q): Query<ImageQuery>,
) -> Result<Response, StatusCode> {
    blocking(move || {
        let live = st.live(&id)?;
        let l = live.lock().unwrap();
        let (mime, bytes) = l.t.items.get(i).and_then(|it| render::image(it, b, q.k)).ok_or(StatusCode::NOT_FOUND)?;
        Ok(([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "public, max-age=86400".into())], bytes).into_response())
    })
    .await
}
