use std::path::PathBuf;

use anyhow::Context;
use tracing_appender::non_blocking::WorkerGuard;

/// Logs go to a file because anything printed to the terminal would corrupt the TUI.
/// Keep the returned guard alive until exit; dropping it flushes the log.
pub fn init() -> anyhow::Result<WorkerGuard> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let dir = PathBuf::from(home).join(".local/state/shipherd");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let (writer, guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::never(&dir, "shipherd.log"));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .init();
    Ok(guard)
}
