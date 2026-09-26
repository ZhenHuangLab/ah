//! ah — browse Claude Code, Codex and pi session history in the terminal or a browser.

mod discover;
mod live;
mod markdown;
mod model;
mod parse;
mod tools;
mod tui;
mod web;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ah", version, about, args_conflicts_with_subcommands = true)]
struct Cli {
    /// Session id, unique id prefix, or path to a transcript. Without one, pick from a list.
    session: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Serve the web viewer.
    Serve {
        /// Address to listen on, repeatable. Defaults to this machine's Tailscale addresses
        /// and 127.0.0.1.
        #[arg(long = "addr", value_name = "IP:PORT")]
        addrs: Vec<SocketAddr>,
        /// Port for the default addresses.
        #[arg(long, default_value_t = 7447)]
        port: u16,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Some(Cmd::Serve { mut addrs, port }) => {
            if addrs.is_empty() {
                let ts = tailscale_ips();
                if ts.is_empty() {
                    bail!("no Tailscale address on this machine; pass --addr IP:PORT to choose where to listen");
                }
                addrs = ts.into_iter().chain([IpAddr::V4(Ipv4Addr::LOCALHOST)]).map(|ip| SocketAddr::new(ip, port)).collect();
            }
            web::serve(addrs)
        }
        None => tui::run(cli.session.as_deref()),
    }
}

/// The addresses Tailscale assigned to this machine, from 100.64.0.0/10 and fd7a:115c:a1e0::/48.
/// Its MagicDNS name resolves to both, and browsers try the IPv6 one first.
fn tailscale_ips() -> Vec<IpAddr> {
    let ifs = if_addrs::get_if_addrs().unwrap_or_default();
    ifs.into_iter()
        .map(|i| i.ip())
        .filter(|ip| match ip {
            IpAddr::V4(ip) => ip.octets()[0] == 100 && ip.octets()[1] & 0xc0 == 64,
            IpAddr::V6(ip) => ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
        })
        .collect()
}
