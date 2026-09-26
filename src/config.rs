//! Settings from `~/.config/ah/config.toml`, and the directory where ah keeps its own files.

use std::io::ErrorKind;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::discover::env_dir;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub public: Option<Public>,
}

/// Access from the internet through a tunnel, such as Cloudflare Tunnel, that forwards a host
/// name to the public listener.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Public {
    /// The host name the tunnel serves, such as `ah.example.com`.
    pub host: String,
}

impl Config {
    pub fn load() -> Result<Config> {
        let path = dir("XDG_CONFIG_HOME", ".config").join("config.toml");
        match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("reading {}", path.display())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn host(&self) -> Option<&str> {
        self.public.as_ref().map(|p| p.host.as_str())
    }
}

/// Where ah keeps its signing key and shared sessions, readable by the user alone.
pub fn data_dir() -> Result<PathBuf> {
    let dir = dir("XDG_DATA_HOME", ".local/share");
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

fn dir(var: &str, under_home: &str) -> PathBuf {
    env_dir(var).unwrap_or_else(|| env_dir("HOME").unwrap_or_default().join(under_home)).join("ah")
}
