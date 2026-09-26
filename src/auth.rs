//! Signing in on the public host name. A login link and the cookie it leaves each carry their
//! expiry and a signature made with a key in the data directory, so nothing else is stored;
//! a new key signs every browser out.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;

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
        self.check("login", token).is_some()
    }

    /// The value of a new session cookie.
    pub fn session(&self) -> String {
        self.sign("session", now() + SESSION_SECS)
    }

    /// Verifies the session and returns how long its requests may remain open.
    pub fn session_remaining(&self, token: &str) -> Option<Duration> {
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

    fn check(&self, purpose: &str, token: &str) -> Option<Duration> {
        let (exp, sig) = token.split_once('.')?;
        let exp = exp.parse::<i64>().ok()?;
        let sig = URL_SAFE_NO_PAD.decode(sig).ok()?;
        self.mac(purpose, exp).verify_slice(&sig).ok()?;
        let expires = jiff::Timestamp::from_second(exp).ok()?;
        let remaining = Duration::try_from(expires.duration_since(jiff::Timestamp::now())).ok()?;
        (!remaining.is_zero()).then_some(remaining)
    }
}

fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn key() -> Key {
        Key(vec![0x42; 32])
    }

    #[test]
    fn tokens_are_bound_to_their_purpose_and_key() {
        let key = key();
        let login = key.sign("login", now() + LINK_SECS);
        let session = key.session();
        assert!(key.check_login(&login));
        assert!(key.session_remaining(&login).is_none());
        assert!(!key.check_login(&session));
        assert!(key.session_remaining(&session).is_some());

        let other = Key(vec![0x43; 32]);
        assert!(!other.check_login(&login));
        assert!(other.session_remaining(&session).is_none());
    }

    #[test]
    fn expired_tokens_are_rejected_at_the_boundary() {
        let key = key();
        for exp in [now() - 1, now()] {
            assert!(!key.check_login(&key.sign("login", exp)));
            assert!(key.session_remaining(&key.sign("session", exp)).is_none());
        }
        let remaining = key.session_remaining(&key.session()).unwrap();
        assert!(!remaining.is_zero());
        assert!(remaining <= Duration::from_secs(SESSION_SECS as u64));
    }

    #[test]
    fn changing_expiry_or_signature_does_not_extend_access() {
        let key = key();
        let token = key.session();
        let (exp, sig) = token.split_once('.').unwrap();
        let later = exp.parse::<i64>().unwrap() + SESSION_SECS;
        assert!(key.session_remaining(&format!("{later}.{sig}")).is_none());

        let mut sig = URL_SAFE_NO_PAD.decode(sig).unwrap();
        sig[0] ^= 1;
        assert!(key.session_remaining(&format!("{exp}.{}", URL_SAFE_NO_PAD.encode(sig))).is_none());
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        let key = key();
        for token in ["", "1", ".", "invalid.signature", "9223372036854775808.AA", "9999999999.AA", "9999999999.%%%"] {
            assert!(!key.check_login(token));
            assert!(key.session_remaining(token).is_none());
        }
    }
}
