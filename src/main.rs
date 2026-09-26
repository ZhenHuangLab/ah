//! ah — read and share Claude Code, Codex and pi sessions in the terminal or a browser.

mod auth;
mod config;
mod discover;
mod live;
mod markdown;
mod model;
mod parse;
mod share;
mod tools;
mod tui;
mod web;

use std::io::IsTerminal;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;

use config::Config;

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
    /// Print a link that signs a browser in on the public host name.
    Login,
    /// List or stop shared sessions.
    #[command(subcommand)]
    Share(ShareCmd),
}

#[derive(Subcommand)]
enum ShareCmd {
    /// List the open shares.
    List,
    /// Stop a share; its link stops working.
    Stop {
        /// The share id: the last part of its link.
        id: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;
    match cli.cmd {
        Some(Cmd::Serve { mut addrs, port }) => {
            if addrs.is_empty() {
                let ts = tailscale_ips();
                if ts.is_empty() && config.public.is_none() {
                    bail!("no Tailscale address on this machine; pass --addr IP:PORT to choose where to listen");
                }
                addrs = ts.into_iter().chain([IpAddr::V4(Ipv4Addr::LOCALHOST)]).map(|ip| SocketAddr::new(ip, port)).collect();
            }
            web::serve(addrs, config.host().map(String::from))
        }
        Some(Cmd::Login) => {
            let link = auth::Key::load()?.login_link(public_host(&config)?);
            if std::io::stdout().is_terminal() {
                // Dark modules on white, so the code scans on dark and light terminals alike.
                let qr = QrCode::new(&link)?.render::<Dense1x2>().build();
                for line in qr.lines() {
                    println!("\x1b[30;107m{line}\x1b[0m");
                }
                println!();
            }
            println!("{link}");
            eprintln!("Open it within 10 minutes; the browser stays signed in for 30 days.");
            Ok(())
        }
        Some(Cmd::Share(ShareCmd::List)) => {
            let host = public_host(&config)?;
            for s in share::list()? {
                println!("{}  {:7}  expires {}  {}\n  {}", s.id, s.view.name(), s.expiry(), s.title(), s.url(host));
            }
            Ok(())
        }
        Some(Cmd::Share(ShareCmd::Stop { id })) => {
            let id = id.rsplit('/').next().unwrap_or_default();
            if !share::stop(id)? {
                bail!("no share {id}");
            }
            Ok(())
        }
        None => tui::run(cli.session.as_deref(), config.host()),
    }
}

fn public_host(config: &Config) -> Result<&str> {
    config.host().context("set the public host name in ~/.config/ah/config.toml:\n\n[public]\nhost = \"ah.example.com\"")
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
