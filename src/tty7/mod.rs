//! Everything that knows about the `tty7` CLI and its JSON.

pub mod cli;
#[cfg(test)]
pub mod fake;
pub mod types;

use async_trait::async_trait;
use tokio::sync::mpsc;

use types::AgentsResponse;

/// Every interaction with tty7 goes through this trait so tests can substitute `FakeCli`.
#[async_trait]
pub trait Tty7Cli: Send + Sync {
    async fn agents(&self) -> anyhow::Result<AgentsResponse>;
    /// The last `tail` lines of a pane as plain text (redraws applied, no escape codes).
    async fn capture_plain(&self, pane: u32, tail: u32) -> anyhow::Result<String>;
    /// Types `text` into the pane via stdin, so it never shows up in `ps`.
    async fn send_text(&self, pane: u32, text: &str, enter: bool) -> anyhow::Result<()>;
    /// Sends named keys (`escape`, `enter`, `up`, `C-c`, …).
    async fn send_keys(&self, pane: u32, keys: &[&str]) -> anyhow::Result<()>;
    /// Starts `tty7 events --json`. The receiver yields raw lines and closes when the process exits.
    fn spawn_events(&self) -> anyhow::Result<mpsc::Receiver<String>>;
}
