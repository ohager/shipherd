use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::bail;
use async_trait::async_trait;
use tokio::sync::mpsc;

use super::Tty7Cli;
use super::types::{AgentsResponse, ProcsResponse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    Text {
        pane: u32,
        text: String,
        enter: bool,
    },
    Keys {
        pane: u32,
        keys: Vec<String>,
    },
}

/// In-memory tty7 for tests. `agents: None` makes `agents()` fail like an unreachable server.
#[derive(Default)]
pub struct FakeCli {
    pub agents: Mutex<Option<AgentsResponse>>,
    pub captures: Mutex<HashMap<u32, String>>,
    pub sent: Mutex<Vec<Sent>>,
    pub event_streams: Mutex<VecDeque<mpsc::Receiver<String>>>,
    pub procs: Mutex<HashMap<u32, ProcsResponse>>,
    /// Makes every capture this slow, like a real Claude Code pane (~300 ms).
    pub capture_delay: Mutex<Duration>,
}

impl FakeCli {
    pub fn with_agents(json: &str) -> Self {
        let fake = Self::default();
        *fake.agents.lock().unwrap() = Some(serde_json::from_str(json).unwrap());
        fake
    }

    /// Queues a stream for the next `spawn_events`. Drop the sender to simulate tty7 exiting.
    pub fn push_event_stream(&self) -> mpsc::Sender<String> {
        let (tx, rx) = mpsc::channel(64);
        self.event_streams.lock().unwrap().push_back(rx);
        tx
    }

    pub fn sent(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }
}

#[async_trait]
impl Tty7Cli for FakeCli {
    async fn agents(&self) -> anyhow::Result<AgentsResponse> {
        let agents = self.agents.lock().unwrap().clone();
        match agents {
            Some(resp) => Ok(resp),
            None => bail!("tty7 server not reachable"),
        }
    }

    async fn capture_plain(&self, pane: u32, _tail: u32) -> anyhow::Result<String> {
        let delay = *self.capture_delay.lock().unwrap();
        tokio::time::sleep(delay).await;
        let text = self.captures.lock().unwrap().get(&pane).cloned();
        match text {
            Some(text) => Ok(text),
            None => bail!("no such pane %{pane}"),
        }
    }

    async fn send_text(&self, pane: u32, text: &str, enter: bool) -> anyhow::Result<()> {
        self.sent.lock().unwrap().push(Sent::Text {
            pane,
            text: text.into(),
            enter,
        });
        Ok(())
    }

    async fn send_keys(&self, pane: u32, keys: &[&str]) -> anyhow::Result<()> {
        let keys = keys.iter().map(|k| k.to_string()).collect();
        self.sent.lock().unwrap().push(Sent::Keys { pane, keys });
        Ok(())
    }

    async fn procs(&self, pane: u32) -> anyhow::Result<ProcsResponse> {
        let procs = self.procs.lock().unwrap().get(&pane).cloned();
        match procs {
            Some(procs) => Ok(procs),
            None => bail!("no such pane %{pane}"),
        }
    }

    fn spawn_events(&self) -> anyhow::Result<mpsc::Receiver<String>> {
        match self.event_streams.lock().unwrap().pop_front() {
            Some(rx) => Ok(rx),
            None => bail!("tty7 events unavailable"),
        }
    }
}
