use std::process::Stdio;

use anyhow::{Context, bail};
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use super::Tty7Cli;
use super::types::{AgentsResponse, CaptureResponse};

/// Runs the real `tty7` binary. Arguments are passed as a list, never through a shell.
pub struct RealCli;

impl RealCli {
    async fn run(args: &[String], stdin: Option<&str>) -> anyhow::Result<Vec<u8>> {
        let mut cmd = Command::new("tty7");
        cmd.args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        let mut child = cmd.spawn().context("failed to start tty7")?;
        if let Some(text) = stdin {
            let mut pipe = child.stdin.take().context("tty7 stdin unavailable")?;
            pipe.write_all(text.as_bytes()).await?;
            // tty7 reads stdin until EOF, so it must be closed before waiting.
            drop(pipe);
        }
        let out = child.wait_with_output().await?;
        if !out.status.success() {
            bail!(
                "tty7 {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out.stdout)
    }
}

#[async_trait]
impl Tty7Cli for RealCli {
    async fn agents(&self) -> anyhow::Result<AgentsResponse> {
        let out = Self::run(&["agents".into(), "--json".into()], None).await?;
        Ok(serde_json::from_slice(&out)?)
    }

    async fn capture_plain(&self, pane: u32, tail: u32) -> anyhow::Result<String> {
        let args = vec![
            "capture".to_string(),
            format!("%{pane}"),
            "--plain".into(),
            "--json".into(),
            "--tail".into(),
            tail.to_string(),
        ];
        let out = Self::run(&args, None).await?;
        Ok(serde_json::from_slice::<CaptureResponse>(&out)?.text)
    }

    async fn send_text(&self, pane: u32, text: &str, enter: bool) -> anyhow::Result<()> {
        let mut args = vec![
            "send".to_string(),
            format!("%{pane}"),
            "--stdin".into(),
            "-q".into(),
        ];
        if enter {
            args.push("--enter".into());
        }
        Self::run(&args, Some(text)).await.map(|_| ())
    }

    async fn send_keys(&self, pane: u32, keys: &[&str]) -> anyhow::Result<()> {
        let mut args = vec!["send".to_string(), format!("%{pane}"), "-q".into()];
        for key in keys {
            args.push("--key".into());
            args.push((*key).to_string());
        }
        Self::run(&args, None).await.map(|_| ())
    }

    fn spawn_events(&self) -> anyhow::Result<mpsc::Receiver<String>> {
        let mut child = Command::new("tty7")
            .args(["events", "--json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("failed to start tty7 events")?;
        let stdout = child
            .stdout
            .take()
            .context("tty7 events stdout unavailable")?;
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).await.is_err() {
                    break;
                }
            }
            let _ = child.kill().await;
        });
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a running tty7: `cargo test -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn talks_to_real_tty7() {
        RealCli
            .agents()
            .await
            .expect("tty7 agents --json should succeed");
    }
}
