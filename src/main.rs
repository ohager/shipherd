use std::io::ErrorKind;
use std::process::{Command, exit};
use std::sync::Arc;

use anyhow::{Context, bail};
use shipherd::tty7::cli::RealCli;
use shipherd::{logging, runtime, tui, web};

const DEFAULT_PORT: u16 = 7777;

fn parse_port() -> anyhow::Result<u16> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next(), args.next()) {
        (None, _, _) => Ok(DEFAULT_PORT),
        (Some("--port"), Some(port), None) => port.parse().context("--port expects a number"),
        _ => bail!("usage: shipherd [--port N]"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = parse_port()?;
    if let Err(err) = Command::new("tty7").arg("--version").output()
        && err.kind() == ErrorKind::NotFound
    {
        eprintln!("shipherd needs the tty7 CLI on your PATH. Install tty7 from https://tty7.io");
        exit(1);
    }
    let _log_guard = logging::init()?;
    let handle = runtime::start(Arc::new(RealCli));
    let server = web::start(handle.clone(), port).await?;
    let links = tui::WebLinks {
        url: server.url(),
        login: server.login_url(),
    };
    tui::run(handle, Some(links)).await
}
