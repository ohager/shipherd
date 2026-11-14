use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::bail;
use async_trait::async_trait;
use tokio::sync::mpsc;

use super::Tty7Cli;
use super::types::{
    AgentsResponse, PanesResponse, ProcsResponse, TabsResponse, WorkspacesResponse,
};

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
    Close {
        pane: u32,
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
    pub workspaces: Mutex<WorkspacesResponse>,
    /// Makes `workspaces()` (the start of each detection pass) this slow.
    pub detect_delay: Mutex<Duration>,
    /// Tabs by workspace id; unknown workspaces have none.
    pub tabs: Mutex<HashMap<String, TabsResponse>>,
    pub panes: Mutex<PanesResponse>,
    pub activations: Mutex<u32>,
}

impl FakeCli {
    pub fn with_agents(json: &str) -> Self {
        let fake = Self::default();
        *fake.agents.lock().unwrap() = Some(serde_json::from_str(json).unwrap());
        fake
    }

    /// Makes `pane` show a Claude Code permission prompt, which approve/deny check for.
    pub fn showing_permission_prompt(self, pane: u32) -> Self {
        let screen = include_str!("../../tests/fixtures/screens/claude-permission.txt");
        self.captures.lock().unwrap().insert(pane, screen.into());
        self
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

    async fn workspaces(&self) -> anyhow::Result<WorkspacesResponse> {
        let delay = *self.detect_delay.lock().unwrap();
        tokio::time::sleep(delay).await;
        Ok(self.workspaces.lock().unwrap().clone())
    }

    async fn tabs(&self, workspace: &str) -> anyhow::Result<TabsResponse> {
        Ok(self
            .tabs
            .lock()
            .unwrap()
            .get(workspace)
            .cloned()
            .unwrap_or_default())
    }

    async fn panes(&self) -> anyhow::Result<PanesResponse> {
        Ok(self.panes.lock().unwrap().clone())
    }

    async fn activate_gui(&self) -> anyhow::Result<()> {
        *self.activations.lock().unwrap() += 1;
        Ok(())
    }

    async fn close_pane(&self, pane: u32) -> anyhow::Result<()> {
        self.sent.lock().unwrap().push(Sent::Close { pane });
        Ok(())
    }

    fn spawn_events(&self) -> anyhow::Result<mpsc::Receiver<String>> {
        match self.event_streams.lock().unwrap().pop_front() {
            Some(rx) => Ok(rx),
            None => bail!("tty7 events unavailable"),
        }
    }
}
