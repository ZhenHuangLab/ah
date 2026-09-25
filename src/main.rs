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

use anyhow::{bail, Result};
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
        /// Address to listen on, repeatable. Defaults to this machine's Tailscale address
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
                let Some(ts) = tailscale_ip() else {
                    bail!("no Tailscale address on this machine; pass --addr IP:PORT to choose where to listen");
                };
                addrs = vec![SocketAddr::new(IpAddr::V4(ts), port), SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)];
            }
            web::serve(addrs)
        }
        None => tui::run(cli.session.as_deref()),
    }
}

/// The IPv4 address Tailscale assigned to this machine (from 100.64.0.0/10).
fn tailscale_ip() -> Option<Ipv4Addr> {
    if_addrs::get_if_addrs().ok()?.into_iter().find_map(|i| match i.ip() {
        IpAddr::V4(ip) if ip.octets()[0] == 100 && ip.octets()[1] & 0xc0 == 64 => Some(ip),
        _ => None,
    })
}
