use std::io::ErrorKind;
use std::process::{Command, exit};
use std::sync::Arc;

use shipherd::tty7::cli::RealCli;
use shipherd::{logging, runtime, tui};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(err) = Command::new("tty7").arg("--version").output()
        && err.kind() == ErrorKind::NotFound
    {
        eprintln!("shipherd needs the tty7 CLI on your PATH. Install tty7 from https://tty7.io");
        exit(1);
    }
    let _log_guard = logging::init()?;
    let handle = runtime::start(Arc::new(RealCli));
    tui::run(handle).await
}
