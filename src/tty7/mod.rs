//! Everything that knows about the `tty7` CLI and its JSON.

pub mod cli;
pub mod detect;
#[cfg(test)]
pub mod fake;
pub mod types;

use async_trait::async_trait;
use tokio::sync::mpsc;

use types::{AgentsResponse, PanesResponse, ProcsResponse, TabsResponse, WorkspacesResponse};

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
    /// Processes and listening ports inside a pane.
    async fn procs(&self, pane: u32) -> anyhow::Result<ProcsResponse>;
    async fn workspaces(&self) -> anyhow::Result<WorkspacesResponse>;
    async fn tabs(&self, workspace: &str) -> anyhow::Result<TabsResponse>;
    /// Every pane on the machine, across all workspaces.
    async fn panes(&self) -> anyhow::Result<PanesResponse>;
    /// Brings the tty7 window to the front (launching it if needed) without opening a tab.
    async fn activate_gui(&self) -> anyhow::Result<()>;
    /// Always closes exactly `pane`: without an argument tty7 would close the caller's own pane.
    async fn close_pane(&self, pane: u32) -> anyhow::Result<()>;
    fn spawn_events(&self) -> anyhow::Result<mpsc::Receiver<String>>;
}
