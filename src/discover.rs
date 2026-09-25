//! Finding session files and reading the metadata shown in session lists.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::UNIX_EPOCH;

use notify::RecommendedWatcher;

use crate::markdown;
use crate::model::{Agent, SessionMeta, Transcript};
use crate::parse;

/// Where each agent keeps its transcripts. Missing directories are left out.
#[derive(Clone, Debug)]
pub struct Roots {
    pub claude: Option<PathBuf>,
    pub codex: Vec<PathBuf>,
    pub pi: Option<PathBuf>,
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

impl Roots {
    pub fn detect() -> Roots {
        let home = env_dir("HOME").unwrap_or_default();
        let claude = env_dir("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude")).join("projects");
        let codex = env_dir("CODEX_HOME").unwrap_or_else(|| home.join(".codex"));
        let pi = env_dir("PI_CODING_AGENT_SESSION_DIR")
            .unwrap_or_else(|| env_dir("PI_CODING_AGENT_DIR").unwrap_or_else(|| home.join(".pi/agent")).join("sessions"));
        Roots {
            claude: Some(claude).filter(|p| p.is_dir()),
            codex: [codex.join("sessions"), codex.join("archived_sessions")].into_iter().filter(|p| p.is_dir()).collect(),
            pi: Some(pi).filter(|p| p.is_dir()),
        }
    }

    pub fn dirs(&self) -> Vec<&Path> {
        self.claude.iter().chain(&self.codex).chain(&self.pi).map(PathBuf::as_path).collect()
    }

    /// The agent of a top-level session file. Subagent transcripts are not top-level.
    pub fn classify(&self, path: &Path) -> Option<Agent> {
        if path.extension()? != "jsonl" {
            return None;
        }
        let depth = |root: &Path| path.strip_prefix(root).ok().map(|rel| rel.components().count());
        if self.claude.as_deref().and_then(depth) == Some(2) {
            return Some(Agent::Claude);
        }
        if self.codex.iter().any(|r| path.starts_with(r)) && path.file_name()?.to_str()?.starts_with("rollout-") {
            return Some(Agent::Codex);
        }
        if self.pi.as_deref().and_then(depth) == Some(2) {
            return Some(Agent::Pi);
        }
        None
    }

    /// Every top-level session file.
    pub fn files(&self) -> Vec<(Agent, PathBuf)> {
        let mut out = Vec::new();
        let mut walk = |dir: &Path, agent: Agent, max_depth: usize| {
            let mut stack = vec![(dir.to_path_buf(), 0)];
            while let Some((d, depth)) = stack.pop() {
                let Ok(rd) = fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    let Ok(ft) = e.file_type() else { continue };
                    if ft.is_dir() && depth + 1 < max_depth {
                        stack.push((p, depth + 1));
                    } else if ft.is_file() && self.classify(&p) == Some(agent) {
                        out.push((agent, p));
                    }
                }
            }
        };
        if let Some(r) = &self.claude {
            walk(r, Agent::Claude, 2);
        }
        for r in &self.codex {
            walk(r, Agent::Codex, 4);
        }
        if let Some(r) = &self.pi {
            walk(r, Agent::Pi, 2);
        }
        out
    }
}

/// A file watcher that sends the path of each transcript written, created, renamed or removed.
/// On Linux, opening a file is reported as well; those events are dropped, or reading a changed
/// transcript would report it changed again.
pub fn watcher(tx: Sender<PathBuf>) -> notify::Result<RecommendedWatcher> {
    notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if ev.kind.is_access() {
            return;
        }
        for p in ev.paths.into_iter().filter(|p| p.extension().is_some_and(|e| e == "jsonl")) {
            let _ = tx.send(p);
        }
    })
}

/// Guesses the agent of an arbitrary transcript from its first record.
pub fn sniff(path: &Path) -> Option<Agent> {
    let mut first = Vec::new();
    BufReader::new(File::open(path).ok()?).read_until(b'\n', &mut first).ok()?;
    let v = parse::record(&first)?;
    Some(match v["type"].as_str() {
        Some("session_meta") => Agent::Codex,
        Some("session") if v.get("version").is_some() => Agent::Pi,
        _ => Agent::Claude,
    })
}

pub fn mtime_ms(md: &fs::Metadata) -> i64 {
    md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64)
}

/// Reads records until the first prompt to fill in id, working directory and title, and
/// looks for a name given to the session near the end of the file. With `listed`, returns
/// `None` for subagent transcripts and sessions without any prompt, which are left out of
/// session lists.
pub fn meta(agent: Agent, path: &Path, listed: bool) -> Option<SessionMeta> {
    let md = fs::metadata(path).ok()?;
    let mut rd = BufReader::with_capacity(1 << 16, File::open(path).ok()?);
    let mut parser = parse::new(agent);
    let mut t = Transcript::default();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match rd.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if let Some(v) = parse::record(&buf) {
            parser.line(&mut t, v);
        }
        if listed && t.info.subagent {
            return None;
        }
        if t.info.cwd.is_some() && t.first_prompt().is_some() {
            break;
        }
    }
    let prompt = t.first_prompt().map(|p| markdown::plain(p, 200));
    if listed && prompt.is_none() {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let id = match agent {
        Agent::Claude => stem.to_string(),
        Agent::Codex => t.info.id.clone().unwrap_or_else(|| stem.to_string()),
        Agent::Pi => t.info.id.clone().unwrap_or_else(|| stem.rsplit('_').next().unwrap_or(stem).to_string()),
    };
    let modified = mtime_ms(&md);
    Some(SessionMeta {
        id,
        agent,
        path: path.to_path_buf(),
        cwd: t.info.cwd.clone().unwrap_or_default(),
        title: tail_title(agent, path, md.len()).or(t.info.title).or(prompt).unwrap_or_else(|| stem.to_string()),
        started: t.info.started.unwrap_or(modified),
        modified,
        size: md.len(),
    })
}

/// How much of the end of a file is searched for the session's name. Claude Code writes the
/// name again whenever half this much has been appended, so the end always holds it.
const TAIL: u64 = 64 * 1024;

/// The latest name given to the session (Claude `/rename`, pi `/name`) within the end of the
/// file. Opening the session reads the whole file and finds a name set further back.
fn tail_title(agent: Agent, path: &Path, len: u64) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let start = len.saturating_sub(TAIL);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.take(TAIL).read_to_end(&mut buf).ok()?;
    let mut lines = buf.split(|&b| b == b'\n');
    if start > 0 {
        lines.next();
    }
    let mut parser = parse::new(agent);
    let mut t = Transcript::default();
    for v in lines.filter_map(parse::record) {
        parser.line(&mut t, v);
    }
    t.info.title
}

/// Lists every session, reading files in parallel. Sorted newest first.
pub fn scan(roots: &Roots) -> Vec<SessionMeta> {
    let files = roots.files();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    let chunk = files.len().div_ceil(threads).max(1);
    let mut out: Vec<SessionMeta> = std::thread::scope(|s| {
        let handles: Vec<_> =
            files.chunks(chunk).map(|c| s.spawn(move || c.iter().filter_map(|(a, p)| meta(*a, p, true)).collect::<Vec<_>>())).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    out.sort_by_key(|m| std::cmp::Reverse(m.modified));
    dedupe(&mut out);
    out
}

/// Gives sessions that share an agent id distinct keys.
fn dedupe(list: &mut [SessionMeta]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for m in list.iter_mut() {
        let n = seen.entry(m.id.clone()).or_default();
        *n += 1;
        if *n > 1 {
            m.id = format!("{}~{}", m.id, n);
        }
    }
}

/// Finds a session by id, unique id prefix, or transcript path.
pub fn resolve(list: &[SessionMeta], query: &str) -> Result<SessionMeta, String> {
    let path = Path::new(query);
    if path.is_file() {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        if let Some(m) = list.iter().find(|m| m.path == path) {
            return Ok(m.clone());
        }
        let agent = sniff(&path).ok_or("not a readable transcript")?;
        return meta(agent, &path, false).ok_or_else(|| "not a readable transcript".into());
    }
    if let Some(m) = list.iter().find(|m| m.id == query) {
        return Ok(m.clone());
    }
    let hits: Vec<_> = list.iter().filter(|m| m.id.starts_with(query)).collect();
    match hits.as_slice() {
        [m] => Ok((*m).clone()),
        [] => Err(format!("no session matches {query:?}")),
        _ => Err(format!("{} sessions match {query:?}; give more of the id", hits.len())),
    }
}
