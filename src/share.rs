//! Shared sessions: snapshots of a session's chat or answers that anyone with the link can read.
//! Each is a file in the data directory until it expires or is stopped.

use std::path::PathBuf;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config;
use crate::live::Live;
use crate::web::render;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    /// Prompts and everything the agent wrote to the user.
    Chat,
    /// Prompts and the final answer of each turn.
    Answers,
}

impl View {
    pub fn name(self) -> &'static str {
        match self {
            View::Chat => "chat",
            View::Answers => "answers",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Share {
    pub id: String,
    /// The id of the shared session.
    pub session: String,
    pub view: View,
    /// Milliseconds since the Unix epoch.
    pub created: i64,
    pub expires: Option<i64>,
    /// Title, agent, model and start of the session.
    pub meta: Value,
    pub items: Value,
}

impl Share {
    pub fn title(&self) -> &str {
        self.meta["title"].as_str().unwrap_or_default()
    }

    pub fn url(&self, host: &str) -> String {
        format!("https://{host}/s/{}", self.id)
    }

    /// When the link stops working, in local time.
    pub fn expiry(&self) -> String {
        match self.expires.and_then(|ms| jiff::Timestamp::from_millisecond(ms).ok()) {
            Some(t) => t.to_zoned(jiff::tz::TimeZone::system()).strftime("%Y-%m-%d %H:%M").to_string(),
            None => "never".into(),
        }
    }

    /// The share without its items, for lists.
    pub fn summary(&self, host: &str) -> Value {
        json!({ "id": self.id, "url": self.url(host), "view": self.view, "created": self.created, "expires": self.expires })
    }

    fn expired(&self) -> bool {
        self.expires.is_some_and(|ms| ms <= jiff::Timestamp::now().as_millisecond())
    }
}

/// Shares the session as it is now, for `days` or until stopped.
pub fn create(l: &Live, view: View, days: Option<u32>) -> Result<Share> {
    let mut id = [0; 16];
    getrandom::fill(&mut id).context("generating a share id")?;
    let now = jiff::Timestamp::now().as_millisecond();
    let s = Share {
        id: URL_SAFE_NO_PAD.encode(id),
        session: l.meta.id.clone(),
        view,
        created: now,
        expires: days.map(|d| now + i64::from(d) * DAY_MS),
        meta: json!({ "title": l.meta.title, "agent": l.meta.agent, "model": l.t.info.model, "started": l.meta.started }),
        items: serde_json::to_value(render::shared(l, view))?,
    };
    let path = file(&s.id)?.context("share id")?;
    std::fs::write(&path, serde_json::to_vec(&s)?).with_context(|| format!("writing {}", path.display()))?;
    Ok(s)
}

/// The share `id`, unless it expired or was stopped.
pub fn load(id: &str) -> Option<Share> {
    let path = file(id).ok()??;
    let s: Share = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    if s.expired() {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    Some(s)
}

/// Every share still open, newest first.
pub fn list() -> Result<Vec<Share>> {
    let mut out: Vec<Share> =
        std::fs::read_dir(dir()?)?.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".json").and_then(load)).collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.created));
    Ok(out)
}

/// Stops a share; returns whether it existed.
pub fn stop(id: &str) -> Result<bool> {
    let Some(path) = file(id)? else { return Ok(false) };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

fn dir() -> Result<PathBuf> {
    let dir = config::data_dir()?.join("shares");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// The file of share `id`; none for a string that is not a share id, which could name a path.
fn file(id: &str) -> Result<Option<PathBuf>> {
    if id.len() != 22 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Ok(None);
    }
    Ok(Some(dir()?.join(format!("{id}.json"))))
}
