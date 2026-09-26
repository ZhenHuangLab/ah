//! Signing in on the public host name. A login link and the cookie it leaves each carry their
//! expiry and a signature made with a key in the data directory, so nothing else is stored;
//! a new key signs every browser out.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::config;

const LINK_SECS: i64 = 10 * 60;
pub const SESSION_SECS: i64 = 30 * 24 * 60 * 60;
pub const COOKIE: &str = "ah";

pub struct Key(Vec<u8>);

impl Key {
    /// The signing key, made on first use.
    pub fn load() -> Result<Key> {
        let dir = config::data_dir()?;
        let path = dir.join("key");
        if !path.exists() {
            // Written in full under another name and then linked into place, so no process
            // reads part of a key, and when two start at once the first link wins.
            let mut key = [0; 32];
            getrandom::fill(&mut key).context("generating a key")?;
            let tmp = dir.join(format!("key.{}", std::process::id()));
            let mut f = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
            f.write_all(&key).and_then(|_| f.sync_all()).with_context(|| format!("writing {}", tmp.display()))?;
            let linked = fs::hard_link(&tmp, &path);
            let _ = fs::remove_file(&tmp);
            match linked {
                Err(e) if e.kind() != ErrorKind::AlreadyExists => return Err(e).with_context(|| format!("creating {}", path.display())),
                _ => {}
            }
        }
        let key = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        if key.len() != 32 {
            bail!("{} is not a key; delete it, and ah makes a new one", path.display());
        }
        Ok(Key(key))
    }

    /// A link that signs a browser in on `host` within the next ten minutes.
    pub fn login_link(&self, host: &str) -> String {
        format!("https://{host}/login?t={}", self.sign("login", now() + LINK_SECS))
    }

    pub fn check_login(&self, token: &str) -> bool {
        self.check("login", token)
    }

    /// The value of a new session cookie.
    pub fn session(&self) -> String {
        self.sign("session", now() + SESSION_SECS)
    }

    pub fn check_session(&self, token: &str) -> bool {
        self.check("session", token)
    }

    fn mac(&self, purpose: &str, exp: i64) -> Hmac<Sha256> {
        let mut m = Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC takes a key of any length");
        m.update(format!("{purpose}:{exp}").as_bytes());
        m
    }

    fn sign(&self, purpose: &str, exp: i64) -> String {
        format!("{exp}.{}", URL_SAFE_NO_PAD.encode(self.mac(purpose, exp).finalize().into_bytes()))
    }

    fn check(&self, purpose: &str, token: &str) -> bool {
        let Some((exp, sig)) = token.split_once('.') else { return false };
        let (Ok(exp), Ok(sig)) = (exp.parse::<i64>(), URL_SAFE_NO_PAD.decode(sig)) else { return false };
        exp > now() && self.mac(purpose, exp).verify_slice(&sig).is_ok()
    }
}

fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}
