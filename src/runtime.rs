//! The core task: owns the fleet, follows tty7, serves views through a `Handle`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, MissedTickBehavior, interval, interval_at, sleep_until};
use tracing::warn;

use crate::actions::{self, Action, Outcome};
use crate::fleet::git::{self, GitInfo};
use crate::fleet::{Connection, Fleet};
use crate::tty7::Tty7Cli;
use crate::tty7::types::{Event, parse_event};

pub const PREVIEW_LINES: u32 = 200;
const PREVIEW_EVERY: Duration = Duration::from_millis(400);
/// Also how closed panes disappear: the close event's shape hasn't been recorded yet.
const RESYNC_EVERY: Duration = Duration::from_secs(10);
/// Claude Code needs a moment to process a keystroke, and tty7 sends no event for a cancelled prompt.
const RESYNC_AFTER_ACTION: Duration = Duration::from_millis(1500);
const FIRST_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn next_backoff(current: Duration) -> Duration {
    (current * 2).min(MAX_BACKOFF)
}

pub enum Request {
    Action {
        action: Action,
        reply: oneshot::Sender<Outcome>,
    },
    /// Replaces the set of panes `client` shows output for.
    Watch { client: String, panes: HashSet<u32> },
}

/// What a view holds: the latest snapshot and a way to send requests.
#[derive(Clone)]
pub struct Handle {
    pub snapshots: watch::Receiver<Fleet>,
    requests: mpsc::Sender<Request>,
}

impl Handle {
    pub async fn act(&self, action: Action) -> Outcome {
        let stopped = || Outcome::Failed("shipherd core stopped".into());
        let (tx, rx) = oneshot::channel();
        if self
            .requests
            .send(Request::Action { action, reply: tx })
            .await
            .is_err()
        {
            return stopped();
        }
        rx.await.unwrap_or_else(|_| stopped())
    }

    pub async fn watch(&self, client: &str, panes: HashSet<u32>) {
        let request = Request::Watch {
            client: client.into(),
            panes,
        };
        let _ = self.requests.send(request).await;
    }
}

pub fn start(cli: Arc<dyn Tty7Cli>) -> Handle {
    let (snapshots_tx, snapshots) = watch::channel(Fleet::default());
    let (requests, requests_rx) = mpsc::channel(32);
    let core = Core {
        cli,
        fleet: Fleet::default(),
        snapshots: snapshots_tx,
        git_cache: HashMap::new(),
        watched: HashMap::new(),
        streaming: false,
    };
    tokio::spawn(core.run(requests_rx));
    Handle {
        snapshots,
        requests,
    }
}

struct Core {
    cli: Arc<dyn Tty7Cli>,
    fleet: Fleet,
    snapshots: watch::Sender<Fleet>,
    git_cache: HashMap<String, GitInfo>,
    watched: HashMap<String, HashSet<u32>>,
    streaming: bool,
}

impl Core {
    async fn run(mut self, mut requests: mpsc::Receiver<Request>) {
        let mut events: Option<mpsc::Receiver<String>> = None;
        let mut backoff = FIRST_BACKOFF;
        let mut reconnect_at = Instant::now();
        let mut preview_tick = interval(PREVIEW_EVERY);
        // Starts one period from now: the first resync happens right after the event stream starts.
        let mut resync_tick = interval_at(Instant::now() + RESYNC_EVERY, RESYNC_EVERY);
        preview_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        resync_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut follow_up_at: Option<Instant> = None;

        loop {
            tokio::select! {
                _ = sleep_until(reconnect_at), if events.is_none() => {
                    match self.cli.spawn_events() {
                        Ok(rx) => {
                            events = Some(rx);
                            self.streaming = true;
                            self.resync().await;
                        }
                        Err(err) => {
                            warn!("starting tty7 events failed: {err:#}");
                            reconnect_at = Instant::now() + backoff;
                            backoff = next_backoff(backoff);
                            self.resync().await;
                        }
                    }
                }
                line = next_line(&mut events), if events.is_some() => match line {
                    Some(line) => {
                        backoff = FIRST_BACKOFF;
                        self.on_event_line(&line).await;
                    }
                    None => {
                        warn!("tty7 events stream ended; reconnecting in {backoff:?}");
                        events = None;
                        self.streaming = false;
                        self.fleet.connection = Connection::Reconnecting;
                        self.publish();
                        reconnect_at = Instant::now() + backoff;
                        backoff = next_backoff(backoff);
                    }
                },
                _ = resync_tick.tick() => self.resync().await,
                _ = sleep_until(follow_up_at.unwrap_or_else(Instant::now)), if follow_up_at.is_some() => {
                    follow_up_at = None;
                    self.resync().await;
                }
                _ = preview_tick.tick() => self.refresh_previews().await,
                request = requests.recv() => match request {
                    Some(Request::Action { action, reply }) => {
                        let outcome = actions::execute(self.cli.as_ref(), &self.fleet, &action).await;
                        // Now for a refused action (the fleet was stale), and again shortly for a
                        // sent one, whose effect tty7 may never announce with an event.
                        self.resync().await;
                        if outcome == Outcome::Sent {
                            follow_up_at = Some(Instant::now() + RESYNC_AFTER_ACTION);
                        }
                        let _ = reply.send(outcome);
                    }
                    Some(Request::Watch { client, panes }) => {
                        self.watched.insert(client, panes);
                    }
                    // Every Handle is gone: the views have exited.
                    None => break,
                },
            }
        }
    }

    async fn on_event_line(&mut self, line: &str) {
        match parse_event(line) {
            Ok(Event::PaneFacts(facts)) => {
                self.fleet.apply_pane_facts(&facts, now_ms());
                self.enrich_git().await;
                self.publish();
            }
            Ok(Event::Other) => {}
            Err(err) => warn!("skipping unparseable tty7 event: {err:#}"),
        }
    }

    async fn resync(&mut self) {
        match self.cli.agents().await {
            Ok(resp) => {
                self.fleet.resync(&resp, now_ms());
                self.fleet.connection = if self.streaming {
                    Connection::Connected
                } else {
                    Connection::Reconnecting
                };
                self.enrich_git().await;
            }
            Err(err) => {
                warn!("tty7 agents failed: {err:#}");
                self.fleet.connection = Connection::Unreachable;
            }
        }
        self.publish();
    }

    async fn refresh_previews(&mut self) {
        let panes: HashSet<u32> = self.watched.values().flatten().copied().collect();
        let mut changed = false;
        let mut failed = false;
        for pane in panes {
            if !self.fleet.panes.contains_key(&pane) {
                continue;
            }
            match self.cli.capture_plain(pane, PREVIEW_LINES).await {
                Ok(text) => changed |= self.fleet.set_preview(pane, text),
                Err(err) => {
                    warn!("capture of %{pane} failed: {err:#}");
                    failed = true;
                }
            }
        }
        // A failed capture usually means the pane closed; a resync removes it.
        if failed {
            self.resync().await;
        } else if changed {
            self.publish();
        }
    }

    async fn enrich_git(&mut self) {
        let missing: Vec<(u32, String)> = self
            .fleet
            .panes
            .values()
            .filter(|p| p.repo.is_none())
            .filter_map(|p| p.cwd.clone().map(|cwd| (p.pane, cwd)))
            .collect();
        for (pane, cwd) in missing {
            if !self.git_cache.contains_key(&cwd) {
                let info = git::lookup(&cwd).await;
                self.git_cache.insert(cwd.clone(), info);
            }
            let info = self.git_cache[&cwd].clone();
            self.fleet.set_git(pane, info.repo, info.branch);
        }
    }

    /// Only notifies views when something they'd render actually changed. tty7 sends a
    /// `pane_facts` event on every spinner frame of an agent's title, so most events change nothing.
    fn publish(&self) {
        self.snapshots.send_if_modified(|current| {
            if *current == self.fleet {
                false
            } else {
                *current = self.fleet.clone();
                true
            }
        });
    }
}

async fn next_line(events: &mut Option<mpsc::Receiver<String>>) -> Option<String> {
    match events {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tty7::fake::{FakeCli, Sent};
    use crate::tty7::types::Status;

    const AGENTS: &str = include_str!("../tests/fixtures/agents.json");
    const EVENTS: &str = include_str!("../tests/fixtures/events.ndjson");
    const WAITING_54: &str = r#"{"agents":[{"pane_id":54,"agent":"Claude","state":{"status":"waiting","cwd":"/tmp/x"}}]}"#;

    async fn wait_for(handle: &Handle, pred: impl Fn(&Fleet) -> bool) -> Fleet {
        let mut rx = handle.snapshots.clone();
        let fleet = tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|f| pred(f)))
            .await
            .expect("timed out waiting for snapshot")
            .expect("runtime stopped");
        fleet.clone()
    }

    #[test]
    fn backoff_doubles_up_to_thirty_seconds() {
        assert_eq!(next_backoff(Duration::from_secs(1)), Duration::from_secs(2));
        assert_eq!(
            next_backoff(Duration::from_secs(16)),
            Duration::from_secs(30)
        );
        assert_eq!(
            next_backoff(Duration::from_secs(30)),
            Duration::from_secs(30)
        );
    }

    #[tokio::test]
    async fn loads_agents_then_applies_events() {
        let cli = Arc::new(FakeCli::with_agents(AGENTS));
        let events = cli.push_event_stream();
        let handle = start(cli.clone());

        let fleet = wait_for(&handle, |f| f.panes.contains_key(&54)).await;
        assert_eq!(fleet.panes[&54].repo.as_deref(), Some("hermes"));

        for line in EVENTS.lines().take(5) {
            events.send(line.to_string()).await.unwrap();
        }
        wait_for(&handle, |f| {
            f.panes.get(&56).map(|p| p.status) == Some(Status::Waiting)
        })
        .await;
    }

    #[tokio::test]
    async fn reconnects_when_the_event_stream_ends() {
        let cli = Arc::new(FakeCli::with_agents(AGENTS));
        let first = cli.push_event_stream();
        let handle = start(cli.clone());
        wait_for(&handle, |f| f.panes.contains_key(&54)).await;

        let _second = cli.push_event_stream();
        drop(first);

        wait_for(&handle, |f| f.connection == Connection::Reconnecting).await;
        wait_for(&handle, |f| f.connection == Connection::Connected).await;
    }

    #[tokio::test]
    async fn unreachable_server_is_reported() {
        let cli = Arc::new(FakeCli::default());
        let _events = cli.push_event_stream();
        let handle = start(cli.clone());
        wait_for(&handle, |f| f.connection == Connection::Unreachable).await;
    }

    #[tokio::test]
    async fn actions_round_trip_through_the_handle() {
        let cli = Arc::new(FakeCli::with_agents(WAITING_54));
        let _events = cli.push_event_stream();
        let handle = start(cli.clone());
        wait_for(&handle, |f| f.panes.contains_key(&54)).await;

        assert_eq!(
            handle.act(Action::Approve { pane: 54 }).await,
            Outcome::Sent
        );
        assert_eq!(
            cli.sent(),
            vec![Sent::Text {
                pane: 54,
                text: "1".into(),
                enter: false
            }]
        );
    }

    #[tokio::test]
    async fn actions_refresh_the_fleet_right_away() {
        let cli = Arc::new(FakeCli::with_agents(WAITING_54));
        let _events = cli.push_event_stream();
        let handle = start(cli.clone());
        wait_for(&handle, |f| f.panes.contains_key(&54)).await;

        // tty7 sends no event when a prompt is cancelled; only `agents` shows the new status.
        *cli.agents.lock().unwrap() = Some(
            serde_json::from_str(
                r#"{"agents":[{"pane_id":54,"agent":"Claude","state":{"status":"done","cwd":"/tmp/x"}}]}"#,
            )
            .unwrap(),
        );
        handle.act(Action::Approve { pane: 54 }).await;

        assert_eq!(handle.snapshots.borrow().panes[&54].status, Status::Done);
    }

    #[tokio::test]
    async fn status_changes_shortly_after_an_action_are_picked_up() {
        let cli = Arc::new(FakeCli::with_agents(WAITING_54));
        let _events = cli.push_event_stream();
        let handle = start(cli.clone());
        wait_for(&handle, |f| f.panes.contains_key(&54)).await;

        assert_eq!(handle.act(Action::Deny { pane: 54 }).await, Outcome::Sent);
        // Claude Code takes a moment to process Esc, and tty7 sends no event for it.
        *cli.agents.lock().unwrap() = Some(
            serde_json::from_str(
                r#"{"agents":[{"pane_id":54,"agent":"Claude","state":{"status":"done","cwd":"/tmp/x"}}]}"#,
            )
            .unwrap(),
        );

        let fleet = tokio::time::timeout(Duration::from_secs(3), async {
            let mut rx = handle.snapshots.clone();
            rx.wait_for(|f| f.panes[&54].status == Status::Done)
                .await
                .unwrap()
                .clone()
        })
        .await
        .expect("follow-up resync should land well before the 10 s periodic one");
        assert_eq!(fleet.panes[&54].status, Status::Done);
    }

    #[tokio::test]
    async fn watched_panes_get_previews() {
        let cli = Arc::new(FakeCli::with_agents(AGENTS));
        cli.captures
            .lock()
            .unwrap()
            .insert(54, "hello from 54".into());
        let _events = cli.push_event_stream();
        let handle = start(cli.clone());
        wait_for(&handle, |f| f.panes.contains_key(&54)).await;

        handle.watch("test", HashSet::from([54])).await;

        let fleet = wait_for(&handle, |f| f.panes[&54].preview.is_some()).await;
        assert_eq!(fleet.panes[&54].preview.as_deref(), Some("hello from 54"));
        assert_eq!(fleet.panes[&56].preview, None);
    }
}
