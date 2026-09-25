//! A transcript kept in sync with its file. Agents only append, so a refresh parses the bytes
//! added since the last one; a truncated, replaced or rewritten file is parsed again from the
//! start.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

use crate::discover::mtime_ms;
use crate::model::{SessionMeta, Transcript};
use crate::parse::{self, Parser};

/// Bytes before the read offset compared on each refresh to notice a rewrite in place.
const TAIL: usize = 64;

/// A session without writes for this long is not waiting on a tool call, whatever its last
/// record says: the agent stopped before the result came. Single tool calls do run for most
/// of an hour, so the limit is a day.
const IDLE_MS: i64 = 24 * 60 * 60 * 1000;

/// Generations are unique across restarts so web clients notice a restarted server.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn next_generation() -> u64 {
    let _ = GENERATION.compare_exchange(0, jiff::Timestamp::now().as_millisecond() as u64 * 1000, Ordering::SeqCst, Ordering::SeqCst);
    GENERATION.fetch_add(1, Ordering::SeqCst)
}

pub struct Live {
    pub meta: SessionMeta,
    pub t: Transcript,
    /// Changes whenever the transcript is rebuilt from scratch.
    pub generation: u64,
    parser: Box<dyn Parser>,
    offset: u64,
    inode: u64,
    /// The last bytes read, which end at `offset`.
    tail: Vec<u8>,
}

impl Live {
    pub fn open(meta: SessionMeta) -> Result<Live> {
        let parser = parse::new(meta.agent);
        let mut live =
            Live { meta, t: Transcript::default(), generation: next_generation(), parser, offset: 0, inode: 0, tail: Vec::new() };
        live.refresh()?;
        if jiff::Timestamp::now().as_millisecond() - live.meta.modified > IDLE_MS {
            live.t.end_turn();
        }
        Ok(live)
    }

    /// Parses complete lines appended since the last call. Returns whether the transcript
    /// changed; changed items carry `t.rev`.
    pub fn refresh(&mut self) -> Result<bool> {
        let mut f = File::open(&self.meta.path).with_context(|| format!("reading {}", self.meta.path.display()))?;
        let md = f.metadata()?;
        let mut changed = false;
        if md.len() < self.offset || (self.inode != 0 && md.ino() != self.inode) || !self.tail_matches(&mut f) {
            self.t = Transcript::default();
            self.parser = parse::new(self.meta.agent);
            self.generation = next_generation();
            self.offset = 0;
            self.tail.clear();
            changed = true;
        }
        self.inode = md.ino();
        self.meta.size = md.len();
        self.meta.modified = mtime_ms(&md);
        if md.len() == self.offset {
            return Ok(changed);
        }
        f.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::with_capacity((md.len() - self.offset) as usize);
        f.take(md.len() - self.offset).read_to_end(&mut buf)?;
        // A trailing partial line is left for the next refresh.
        let Some(end) = buf.iter().rposition(|&b| b == b'\n').map(|p| p + 1) else { return Ok(changed) };
        self.t.rev += 1;
        for line in buf[..end].split(|&b| b == b'\n') {
            if let Some(v) = parse::record(line) {
                self.parser.line(&mut self.t, v);
            }
        }
        self.offset += end as u64;
        self.tail = buf[end.saturating_sub(TAIL)..end].to_vec();
        if let Some(title) = &self.t.info.title {
            self.meta.title.clone_from(title);
        }
        Ok(true)
    }

    /// Whether the file still holds the bytes last read; rewriting a file in place (pi does
    /// so when it upgrades an old session) changes them even when the file grows.
    fn tail_matches(&self, f: &mut File) -> bool {
        let mut cur = vec![0; self.tail.len()];
        f.seek(SeekFrom::Start(self.offset - self.tail.len() as u64)).and_then(|_| f.read_exact(&mut cur)).is_ok() && cur == self.tail
    }
}
