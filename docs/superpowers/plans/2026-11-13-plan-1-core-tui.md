# shipherd Plan 1 — Core + TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A working `shipherd` binary that runs in a tty7 split pane and shows every coding agent grouped by status, with a live preview and approve / deny / reply.

**Architecture:** One Rust crate (library + thin `main.rs`). A `tty7` adapter wraps the `tty7` CLI behind the `Tty7Cli` trait; `fleet` is pure state; `actions` is the only path that types into panes; `runtime` is a tokio task that owns the fleet, follows `tty7 events --json`, and publishes snapshots on a `watch` channel; `tui` renders snapshots with ratatui. The web dashboard (Plan 2) will subscribe to the same `runtime::Handle`.

**Tech Stack:** Rust stable (1.93 locally), tokio, serde/serde_json, anyhow, async-trait, ratatui (with its re-exported crossterm), tracing + tracing-appender, tempfile (dev).

**Spec:** `docs/superpowers/specs/2026-11-13-shipherd-design.md`

**Rust notes for a first-timer** (read once):
- `cargo test` runs every `#[test]` / `#[tokio::test]` function. Tests sit in a `#[cfg(test)] mod tests { … }` block at the bottom of the file they test; `cfg(test)` means "only compiled for tests".
- `cargo test fleet::` runs only tests whose path starts with `fleet::`.
- `anyhow::Result<T>` is "T or any error"; `?` returns early with the error (like `await` rejecting in TS).
- `#[derive(Debug, Clone, PartialEq)]` auto-generates printing, copying and `==`.
- A `trait` is an interface. `Arc<dyn Tty7Cli>` is a shared pointer to "something implementing the interface", which is how tests swap in `FakeCli`.

---

## File structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | crate + dependencies |
| `src/lib.rs` | module list |
| `src/main.rs` | tty7-on-PATH check, logging, start runtime + TUI |
| `src/logging.rs` | file logging to `~/.local/state/shipherd/shipherd.log` |
| `src/tty7/mod.rs` | `Tty7Cli` trait |
| `src/tty7/types.rs` | serde types for tty7 JSON + `parse_event` |
| `src/tty7/cli.rs` | `RealCli` — spawns `tty7` |
| `src/tty7/fake.rs` | `FakeCli` for tests (`cfg(test)` only) |
| `src/fleet/mod.rs` | `Fleet` state, ordering, `format_elapsed` |
| `src/fleet/git.rs` | repo name + branch lookup |
| `src/actions.rs` | approve / deny / reply with race guard |
| `src/runtime.rs` | core loop, reconnects, previews, `Handle` |
| `src/tui/mod.rs` | terminal setup + async event loop |
| `src/tui/app.rs` | key handling (pure) |
| `src/tui/view.rs` | rendering (pure) |
| `tests/fixtures/*` | scrubbed real tty7 output |
| `.github/workflows/ci.yml` | fmt + clippy + test on macOS and Linux |

---

### Task 1: Scaffold the crate and CI

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs` (via `cargo init`), `.github/workflows/ci.yml`

- [ ] **Step 1: Create the crate and add dependencies**

```bash
cd ~/Code/shipherd
cargo init --name shipherd
cargo add tokio --features full
cargo add serde --features derive
cargo add serde_json anyhow async-trait ratatui tracing tracing-subscriber tracing-appender
cargo add --dev tempfile
```

Expected: `Cargo.toml` lists those dependencies; `src/main.rs` contains a hello-world. `cargo init` keeps the existing `.gitignore`.

- [ ] **Step 2: Create the library root**

`src/lib.rs`:

```rust
//! shipherd — a live overview of the coding agents running in tty7.
```

- [ ] **Step 3: Add CI**

`.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
```

- [ ] **Step 4: Verify**

Run: `cargo build && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: builds with no warnings.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src .github
git commit -m "Scaffold crate and CI" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: tty7 types and fixtures

**Files:**
- Create: `tests/fixtures/agents.json`, `tests/fixtures/agents-no-hooks.json`, `tests/fixtures/events.ndjson`, `tests/fixtures/capture.json`
- Create: `src/tty7/mod.rs`, `src/tty7/types.rs`
- Modify: `src/lib.rs`

The fixtures are real tty7 26.10 output recorded on 2026-11-13, with paths and ids scrubbed (the repo is public). Line 8 of `events.ndjson` (a pane with `"agent":null`) is the one shape **not** observed; it's the assumed shape for a plain shell pane.

- [ ] **Step 1: Write the fixtures**

`tests/fixtures/agents.json`:

```json
{"agents":[{"pane_id":54,"agent":"Claude","state":{"status":"working","message":null,"session_id":"00000000-0000-0000-0000-000000000054","launch_argv":["claude"],"rich":true,"cwd":"/home/dev/code/hermes","activity":10,"turns":2,"inferred":false,"readout":{"permission_mode":"auto","model":"claude-opus-5-5","effort":"medium"}}},{"pane_id":56,"agent":"Claude","state":{"status":"done","message":null,"session_id":"00000000-0000-0000-0000-000000000056","launch_argv":["claude"],"rich":true,"cwd":"/tmp/sh-test","activity":1,"turns":1,"inferred":false,"readout":{"permission_mode":"default","model":"claude-opus-5-5","effort":"medium"}}}]}
```

`tests/fixtures/agents-no-hooks.json`:

```json
{"agents":[],"diagnostics":[{"kind":"agent_status_hooks_unavailable","agent":"claude","hooks_state":"not_installed","action":"install"}]}
```

`tests/fixtures/events.ndjson` (8 lines, one JSON object each):

```
{"layout":{"workspace":"11111111-1111-1111-1111-111111111111","delta":{"pane_facts":{"pane":{"id":56,"cwd":"/tmp/sh-test","title":"","osc_title":"claude","ssh_spec":null,"agent":{"agent":"Claude","session_id":null,"launch_argv":["claude"],"status":null},"shell":null,"live":true}}}}}
{"layout":{"workspace":"11111111-1111-1111-1111-111111111111","delta":{"pane_facts":{"pane":{"id":56,"cwd":"/tmp/sh-test","title":"","osc_title":"✳ Claude Code","ssh_spec":null,"agent":{"agent":"Claude","session_id":"00000000-0000-0000-0000-000000000056","launch_argv":["claude"],"status":"idle"},"shell":null,"live":true}}}}}
{"layout":{"workspace":"11111111-1111-1111-1111-111111111111","delta":{"pane_facts":{"pane":{"id":56,"cwd":"/tmp/sh-test","title":"","osc_title":"◐ Run ls -la","ssh_spec":null,"agent":{"agent":"Claude","session_id":"00000000-0000-0000-0000-000000000056","launch_argv":["claude"],"status":"working"},"shell":null,"live":true}}}}}
{"layout":{"workspace":"11111111-1111-1111-1111-111111111111","delta":{"workspace_touched":{"last_active":1794588547}}}}
{"layout":{"workspace":"11111111-1111-1111-1111-111111111111","delta":{"pane_facts":{"pane":{"id":56,"cwd":"/tmp/sh-test","title":"","osc_title":"✳ Run ls -la","ssh_spec":null,"agent":{"agent":"Claude","session_id":"00000000-0000-0000-0000-000000000056","launch_argv":["claude"],"status":"waiting"},"shell":null,"live":true}}}}}
{"layout":{"workspace":"11111111-1111-1111-1111-111111111111","delta":{"tab_created":{"at":3,"tab":{"id":"22222222-2222-2222-2222-222222222222","name":null,"root":{"Leaf":{"pane":56}}}}}}}
{"layout":{"workspace":"11111111-1111-1111-1111-111111111111","delta":{"pane_facts":{"pane":{"id":56,"cwd":"/tmp/sh-test","title":"","osc_title":"✳ Claude Code","ssh_spec":null,"agent":{"agent":"Claude","session_id":"00000000-0000-0000-0000-000000000056","launch_argv":["claude"],"status":"done"},"shell":null,"live":true}}}}}
{"layout":{"workspace":"11111111-1111-1111-1111-111111111111","delta":{"pane_facts":{"pane":{"id":7,"cwd":"/home/dev/code/hermes","title":"","osc_title":"zsh","ssh_spec":null,"agent":null,"shell":null,"live":true}}}}}
```

`tests/fixtures/capture.json`:

```json
{"pane":54,"text":"⏺ Running tests\n  ⎿  $ yarn test\n\n Do you want to proceed?\n ❯ 1. Yes\n","bytes":64}
```

- [ ] **Step 2: Write the failing tests**

`src/tty7/mod.rs`:

```rust
//! Everything that knows about the `tty7` CLI and its JSON.

pub mod types;
```

`src/lib.rs` — add below the doc comment:

```rust
pub mod tty7;
```

`src/tty7/types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const AGENTS: &str = include_str!("../../tests/fixtures/agents.json");
    const NO_HOOKS: &str = include_str!("../../tests/fixtures/agents-no-hooks.json");
    const EVENTS: &str = include_str!("../../tests/fixtures/events.ndjson");
    const CAPTURE: &str = include_str!("../../tests/fixtures/capture.json");

    #[test]
    fn parses_agents_response() {
        let resp: AgentsResponse = serde_json::from_str(AGENTS).unwrap();
        assert_eq!(resp.agents.len(), 2);
        assert_eq!(resp.agents[0].pane_id, 54);
        assert_eq!(resp.agents[0].agent, "Claude");
        assert_eq!(resp.agents[0].state.status, Some(Status::Working));
        assert_eq!(resp.agents[0].state.cwd.as_deref(), Some("/home/dev/code/hermes"));
        assert!(resp.diagnostics.is_empty());
    }

    #[test]
    fn parses_missing_hooks_diagnostic() {
        let resp: AgentsResponse = serde_json::from_str(NO_HOOKS).unwrap();
        assert!(resp.agents.is_empty());
        assert_eq!(
            resp.diagnostics,
            vec![Diagnostic {
                kind: "agent_status_hooks_unavailable".into(),
                agent: Some("claude".into()),
            }]
        );
    }

    #[test]
    fn parses_recorded_event_stream() {
        let events: Vec<Event> = EVENTS.lines().map(|l| parse_event(l).unwrap()).collect();
        let statuses: Vec<(u32, Option<Status>)> = events
            .iter()
            .filter_map(|e| match e {
                Event::PaneFacts(f) => Some((f.id, f.agent.as_ref().and_then(|a| a.status))),
                Event::Other => None,
            })
            .collect();
        assert_eq!(
            statuses,
            vec![
                (56, None),
                (56, Some(Status::Idle)),
                (56, Some(Status::Working)),
                (56, Some(Status::Waiting)),
                (56, Some(Status::Done)),
                (7, None),
            ]
        );
        assert_eq!(events.iter().filter(|e| **e == Event::Other).count(), 2);
    }

    #[test]
    fn unknown_status_does_not_fail() {
        let line = r#"{"layout":{"delta":{"pane_facts":{"pane":{"id":1,"agent":{"agent":"Claude","status":"pondering"},"live":true}}}}}"#;
        let Event::PaneFacts(facts) = parse_event(line).unwrap() else {
            panic!("expected pane facts");
        };
        assert_eq!(facts.agent.unwrap().status, Some(Status::Unknown));
        assert_eq!(facts.cwd, None);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_event("not json").is_err());
    }

    #[test]
    fn parses_capture() {
        let capture: CaptureResponse = serde_json::from_str(CAPTURE).unwrap();
        assert!(capture.text.contains("Do you want to proceed?"));
    }

    #[test]
    fn statuses_sort_in_display_order() {
        let mut statuses = vec![Status::Idle, Status::Done, Status::Waiting, Status::Working];
        statuses.sort();
        assert_eq!(statuses, vec![Status::Waiting, Status::Working, Status::Done, Status::Idle]);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test tty7::types`
Expected: compile errors — `AgentsResponse`, `Status`, `parse_event` etc. not found.

- [ ] **Step 4: Implement the types**

Put this **above** the `#[cfg(test)]` block in `src/tty7/types.rs`:

```rust
use serde::Deserialize;

/// Agent status from tty7's hooks. Declaration order is display order (`derive(Ord)` sorts by it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Waiting,
    Working,
    Done,
    Idle,
    /// A status this version of shipherd doesn't know yet.
    #[serde(other)]
    Unknown,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Waiting => "needs you",
            Status::Working => "working",
            Status::Done => "done",
            Status::Idle => "idle",
            Status::Unknown => "unknown",
        }
    }
}

/// `tty7 agents --json`
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AgentsResponse {
    #[serde(default)]
    pub agents: Vec<AgentEntry>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentEntry {
    pub pane_id: u32,
    pub agent: String,
    pub state: AgentEntryState,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentEntryState {
    pub status: Option<Status>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub launch_argv: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Diagnostic {
    pub kind: String,
    pub agent: Option<String>,
}

/// The `pane` object inside a `layout.delta.pane_facts` event.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PaneFacts {
    pub id: u32,
    pub cwd: Option<String>,
    pub agent: Option<PaneAgent>,
    #[serde(default = "default_live")]
    pub live: bool,
}

fn default_live() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PaneAgent {
    pub agent: String,
    pub status: Option<Status>,
}

/// `tty7 capture --plain --json`
#[derive(Debug, Clone, Deserialize)]
pub struct CaptureResponse {
    pub text: String,
}

/// One line of `tty7 events --json`. Only pane facts matter to shipherd; everything else is `Other`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    PaneFacts(PaneFacts),
    Other,
}

pub fn parse_event(line: &str) -> anyhow::Result<Event> {
    let value: serde_json::Value = serde_json::from_str(line)?;
    match value.pointer("/layout/delta/pane_facts/pane") {
        Some(pane) => Ok(Event::PaneFacts(PaneFacts::deserialize(pane)?)),
        None => Ok(Event::Other),
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test tty7::types`
Expected: 7 passed.

- [ ] **Step 6: Commit**

```bash
git add tests src
git commit -m "Add tty7 JSON types with recorded fixtures" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Fleet state

**Files:**
- Create: `src/fleet/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`src/lib.rs` — add:

```rust
pub mod fleet;
```

`src/fleet/mod.rs`:

```rust
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::tty7::types::{PaneAgent, PaneFacts};

    pub fn facts(id: u32, agent: &str, status: Option<Status>, cwd: &str) -> PaneFacts {
        PaneFacts {
            id,
            cwd: Some(cwd.into()),
            agent: Some(PaneAgent { agent: agent.into(), status }),
            live: true,
        }
    }

    /// Builds a fleet where each `(pane, agent, status, since_ms)` entered its status at `since_ms`.
    pub fn fleet_with(panes: &[(u32, &str, Status, u64)]) -> Fleet {
        let mut fleet = Fleet::default();
        for &(id, agent, status, since) in panes {
            fleet.apply_pane_facts(&facts(id, agent, Some(status), "/tmp/x"), since);
        }
        fleet
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{facts, fleet_with};
    use super::*;
    use crate::tty7::types::PaneFacts;

    const AGENTS: &str = include_str!("../../tests/fixtures/agents.json");
    const NO_HOOKS: &str = include_str!("../../tests/fixtures/agents-no-hooks.json");

    fn agents(json: &str) -> AgentsResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn resync_marks_existing_statuses_as_inexact() {
        let mut fleet = Fleet::default();
        fleet.resync(&agents(AGENTS), 1_000);
        let pane = &fleet.panes[&54];
        assert_eq!(pane.status, Status::Working);
        assert_eq!(pane.status_since_ms, 1_000);
        assert!(!pane.since_exact);
        assert_eq!(pane.cwd.as_deref(), Some("/home/dev/code/hermes"));
    }

    #[test]
    fn new_pane_from_event_is_exact() {
        let mut fleet = Fleet::default();
        fleet.apply_pane_facts(&facts(56, "Claude", Some(Status::Working), "/tmp/a"), 5_000);
        assert!(fleet.panes[&56].since_exact);
        assert_eq!(fleet.panes[&56].status_since_ms, 5_000);
    }

    #[test]
    fn status_change_resets_since_and_same_status_keeps_it() {
        let mut fleet = Fleet::default();
        fleet.resync(&agents(AGENTS), 1_000);
        fleet.apply_pane_facts(&facts(54, "Claude", Some(Status::Working), "/home/dev/code/hermes"), 2_000);
        assert_eq!(fleet.panes[&54].status_since_ms, 1_000);
        assert!(!fleet.panes[&54].since_exact);

        fleet.apply_pane_facts(&facts(54, "Claude", Some(Status::Waiting), "/home/dev/code/hermes"), 3_000);
        assert_eq!(fleet.panes[&54].status, Status::Waiting);
        assert_eq!(fleet.panes[&54].status_since_ms, 3_000);
        assert!(fleet.panes[&54].since_exact);
    }

    #[test]
    fn missing_status_counts_as_idle() {
        let mut fleet = Fleet::default();
        fleet.apply_pane_facts(&facts(56, "Claude", None, "/tmp/a"), 1);
        assert_eq!(fleet.panes[&56].status, Status::Idle);
    }

    #[test]
    fn pane_without_agent_or_not_live_is_removed() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Working, 0), (2, "Claude", Status::Done, 0)]);
        fleet.apply_pane_facts(&PaneFacts { id: 1, cwd: None, agent: None, live: true }, 10);
        let mut gone = facts(2, "Claude", Some(Status::Done), "/tmp/x");
        gone.live = false;
        fleet.apply_pane_facts(&gone, 10);
        assert!(fleet.panes.is_empty());
    }

    #[test]
    fn resync_drops_closed_panes_and_marks_changes_inexact() {
        let mut fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0), (99, "Claude", Status::Working, 0)]);
        fleet.resync(&agents(AGENTS), 9_000);
        assert!(!fleet.panes.contains_key(&99));
        assert_eq!(fleet.panes[&54].status, Status::Working);
        assert_eq!(fleet.panes[&54].status_since_ms, 9_000);
        assert!(!fleet.panes[&54].since_exact);
    }

    #[test]
    fn resync_keeps_diagnostics() {
        let mut fleet = Fleet::default();
        fleet.resync(&agents(NO_HOOKS), 0);
        assert_eq!(fleet.hooks_missing(), vec!["claude".to_string()]);
    }

    #[test]
    fn cwd_change_clears_git_info() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Working, 0)]);
        fleet.set_git(1, Some("x".into()), Some("main".into()));
        fleet.apply_pane_facts(&facts(1, "Claude", Some(Status::Working), "/elsewhere"), 1);
        assert_eq!(fleet.panes[&1].repo, None);
        assert_eq!(fleet.panes[&1].branch, None);
    }

    #[test]
    fn ordered_groups_by_status_then_longest_waiting_first() {
        let fleet = fleet_with(&[
            (1, "Claude", Status::Done, 100),
            (2, "Claude", Status::Working, 300),
            (3, "Claude", Status::Waiting, 500),
            (4, "Claude", Status::Working, 200),
            (5, "Claude", Status::Waiting, 400),
        ]);
        let order: Vec<u32> = fleet.ordered().iter().map(|p| p.pane).collect();
        assert_eq!(order, vec![5, 3, 4, 2, 1]);
        assert_eq!(fleet.count(Status::Waiting), 2);
    }

    #[test]
    fn set_preview_reports_changes() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Working, 0)]);
        assert!(fleet.set_preview(1, "a".into()));
        assert!(!fleet.set_preview(1, "a".into()));
        assert!(!fleet.set_preview(42, "a".into()));
    }

    #[test]
    fn formats_elapsed_time() {
        assert_eq!(format_elapsed(41_000, true), "41s");
        assert_eq!(format_elapsed(302_000, true), "5m02s");
        assert_eq!(format_elapsed(3_840_000, true), "1h04m");
        assert_eq!(format_elapsed(120_000, false), "≥2m00s");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test fleet::`
Expected: compile errors — `Fleet`, `format_elapsed` not found.

- [ ] **Step 3: Implement the fleet**

Put this above the test modules in `src/fleet/mod.rs`:

```rust
//! The current set of agent panes. Pure state: no I/O, time is passed in.

use std::collections::BTreeMap;

use crate::tty7::types::{AgentsResponse, Diagnostic, PaneFacts, Status};

const HOOKS_UNAVAILABLE: &str = "agent_status_hooks_unavailable";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Connection {
    #[default]
    Connected,
    /// The `tty7 events` stream ended; shipherd is restarting it.
    Reconnecting,
    /// `tty7 agents` failed, so the tty7 server is probably down.
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPane {
    pub pane: u32,
    pub agent: String,
    pub status: Status,
    /// When shipherd saw this status begin (ms since the Unix epoch).
    pub status_since_ms: u64,
    /// False when the status was already running when shipherd first saw it, so the real start is earlier.
    pub since_exact: bool,
    pub cwd: Option<String>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub preview: Option<String>,
}

impl AgentPane {
    fn new(pane: u32, agent: &str, status: Status, cwd: Option<String>, now_ms: u64, exact: bool) -> Self {
        AgentPane {
            pane,
            agent: agent.to_string(),
            status,
            status_since_ms: now_ms,
            since_exact: exact,
            cwd,
            repo: None,
            branch: None,
            preview: None,
        }
    }

    fn update(&mut self, agent: &str, status: Status, cwd: Option<String>, now_ms: u64, exact: bool) {
        if self.status != status {
            self.status = status;
            self.status_since_ms = now_ms;
            self.since_exact = exact;
        }
        self.agent = agent.to_string();
        if self.cwd != cwd {
            self.cwd = cwd;
            self.repo = None;
            self.branch = None;
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fleet {
    pub panes: BTreeMap<u32, AgentPane>,
    pub diagnostics: Vec<Diagnostic>,
    pub connection: Connection,
}

impl Fleet {
    /// Replaces the pane set with a full `tty7 agents` listing. Status changes found this way
    /// happened at some unknown point since the last update, so they're marked inexact.
    pub fn resync(&mut self, resp: &AgentsResponse, now_ms: u64) {
        let mut next = BTreeMap::new();
        for entry in &resp.agents {
            let status = entry.state.status.unwrap_or(Status::Idle);
            let cwd = entry.state.cwd.clone();
            let pane = match self.panes.remove(&entry.pane_id) {
                Some(mut existing) => {
                    existing.update(&entry.agent, status, cwd, now_ms, false);
                    existing
                }
                None => AgentPane::new(entry.pane_id, &entry.agent, status, cwd, now_ms, false),
            };
            next.insert(entry.pane_id, pane);
        }
        self.panes = next;
        self.diagnostics = resp.diagnostics.clone();
    }

    pub fn apply_pane_facts(&mut self, facts: &PaneFacts, now_ms: u64) {
        let Some(agent) = facts.agent.as_ref().filter(|_| facts.live) else {
            self.panes.remove(&facts.id);
            return;
        };
        let status = agent.status.unwrap_or(Status::Idle);
        let cwd = facts.cwd.clone();
        match self.panes.get_mut(&facts.id) {
            Some(existing) => existing.update(&agent.agent, status, cwd, now_ms, true),
            None => {
                let pane = AgentPane::new(facts.id, &agent.agent, status, cwd, now_ms, true);
                self.panes.insert(facts.id, pane);
            }
        }
    }

    /// Display order shared by every view: grouped by status, longest in that status first.
    pub fn ordered(&self) -> Vec<&AgentPane> {
        let mut panes: Vec<&AgentPane> = self.panes.values().collect();
        panes.sort_by_key(|p| (p.status, p.status_since_ms, p.pane));
        panes
    }

    pub fn count(&self, status: Status) -> usize {
        self.panes.values().filter(|p| p.status == status).count()
    }

    /// Agent names whose status hooks tty7 reports as not installed.
    pub fn hooks_missing(&self) -> Vec<String> {
        self.diagnostics
            .iter()
            .filter(|d| d.kind == HOOKS_UNAVAILABLE)
            .map(|d| d.agent.clone().unwrap_or_else(|| "an agent".into()))
            .collect()
    }

    /// Returns true when the preview actually changed.
    pub fn set_preview(&mut self, pane: u32, text: String) -> bool {
        match self.panes.get_mut(&pane) {
            Some(p) if p.preview.as_deref() != Some(text.as_str()) => {
                p.preview = Some(text);
                true
            }
            _ => false,
        }
    }

    pub fn set_git(&mut self, pane: u32, repo: Option<String>, branch: Option<String>) {
        if let Some(p) = self.panes.get_mut(&pane) {
            p.repo = repo;
            p.branch = branch;
        }
    }
}

/// `41s`, `5m02s`, `1h04m`; prefixed with `≥` when the start time is only a lower bound.
pub fn format_elapsed(ms: u64, exact: bool) -> String {
    let s = ms / 1000;
    let body = if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    };
    if exact {
        body
    } else {
        format!("≥{body}")
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test fleet::`
Expected: 11 passed.

- [ ] **Step 5: Commit**

```bash
git add src
git commit -m "Add fleet state with ordering and time in state" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Git repo and branch lookup

**Files:**
- Create: `src/fleet/git.rs`
- Modify: `src/fleet/mod.rs` (add `pub mod git;` at the top, below the `//!` comment)

- [ ] **Step 1: Write the failing tests**

`src/fleet/mod.rs` — add below the `//!` doc comment:

```rust
pub mod git;
```

`src/fleet/git.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = StdCommand::new("git").arg("-C").arg(dir).args(args).status().unwrap().success();
        assert!(ok, "git {args:?} failed");
    }

    #[tokio::test]
    async fn finds_repo_name_and_branch() {
        let dir = tempfile::Builder::new().prefix("shiprepo").tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "feature-x"]);
        git(
            dir.path(),
            &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"],
        );
        let sub = dir.path().join("src");
        std::fs::create_dir(&sub).unwrap();

        let info = lookup(sub.to_str().unwrap()).await;

        assert!(info.repo.as_deref().unwrap().starts_with("shiprepo"), "{info:?}");
        assert_eq!(info.branch.as_deref(), Some("feature-x"));
    }

    #[tokio::test]
    async fn falls_back_to_directory_name_outside_git() {
        let dir = tempfile::Builder::new().prefix("plaindir").tempdir().unwrap();
        let info = lookup(dir.path().to_str().unwrap()).await;
        assert!(info.repo.as_deref().unwrap().starts_with("plaindir"), "{info:?}");
        assert_eq!(info.branch, None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test fleet::git`
Expected: compile error — `lookup` not found.

- [ ] **Step 3: Implement the lookup**

Above the test module in `src/fleet/git.rs`:

```rust
use std::path::Path;

use tokio::process::Command;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitInfo {
    pub repo: Option<String>,
    pub branch: Option<String>,
}

/// Repo name is the git toplevel's directory name, or the cwd's own name outside a repo.
pub async fn lookup(cwd: &str) -> GitInfo {
    let toplevel = git(cwd, &["rev-parse", "--show-toplevel"]).await;
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    let repo_path = toplevel.as_deref().unwrap_or(cwd);
    let repo = Path::new(repo_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    GitInfo { repo, branch }
}

async fn git(cwd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(cwd).args(args).output().await.ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test fleet::git`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src
git commit -m "Look up repo name and branch per cwd" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: The `Tty7Cli` trait, real CLI and fake

**Files:**
- Modify: `src/tty7/mod.rs`
- Create: `src/tty7/cli.rs`, `src/tty7/fake.rs`

There's no unit test for `RealCli` (it needs a running tty7); Step 4 adds an `#[ignore]`d smoke test you run by hand.

- [ ] **Step 1: Define the trait**

Replace `src/tty7/mod.rs` with:

```rust
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
```

- [ ] **Step 2: Implement `RealCli`**

`src/tty7/cli.rs`:

```rust
use std::process::Stdio;

use anyhow::{bail, Context};
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use super::types::{AgentsResponse, CaptureResponse};
use super::Tty7Cli;

/// Runs the real `tty7` binary. Arguments are passed as a list, never through a shell.
pub struct RealCli;

impl RealCli {
    async fn run(args: &[String], stdin: Option<&str>) -> anyhow::Result<Vec<u8>> {
        let mut cmd = Command::new("tty7");
        cmd.args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
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
        let mut args = vec!["send".to_string(), format!("%{pane}"), "--stdin".into(), "-q".into()];
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
        let stdout = child.stdout.take().context("tty7 events stdout unavailable")?;
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
        RealCli.agents().await.expect("tty7 agents --json should succeed");
    }
}
```

- [ ] **Step 3: Implement `FakeCli`**

`src/tty7/fake.rs`:

```rust
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use anyhow::bail;
use async_trait::async_trait;
use tokio::sync::mpsc;

use super::types::AgentsResponse;
use super::Tty7Cli;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    Text { pane: u32, text: String, enter: bool },
    Keys { pane: u32, keys: Vec<String> },
}

/// In-memory tty7 for tests. `agents: None` makes `agents()` fail like an unreachable server.
#[derive(Default)]
pub struct FakeCli {
    pub agents: Mutex<Option<AgentsResponse>>,
    pub captures: Mutex<HashMap<u32, String>>,
    pub sent: Mutex<Vec<Sent>>,
    pub event_streams: Mutex<VecDeque<mpsc::Receiver<String>>>,
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
        let text = self.captures.lock().unwrap().get(&pane).cloned();
        match text {
            Some(text) => Ok(text),
            None => bail!("no such pane %{pane}"),
        }
    }

    async fn send_text(&self, pane: u32, text: &str, enter: bool) -> anyhow::Result<()> {
        self.sent.lock().unwrap().push(Sent::Text { pane, text: text.into(), enter });
        Ok(())
    }

    async fn send_keys(&self, pane: u32, keys: &[&str]) -> anyhow::Result<()> {
        let keys = keys.iter().map(|k| k.to_string()).collect();
        self.sent.lock().unwrap().push(Sent::Keys { pane, keys });
        Ok(())
    }

    fn spawn_events(&self) -> anyhow::Result<mpsc::Receiver<String>> {
        match self.event_streams.lock().unwrap().pop_front() {
            Some(rx) => Ok(rx),
            None => bail!("tty7 events unavailable"),
        }
    }
}
```

- [ ] **Step 4: Verify against the real tty7**

Run: `cargo test && cargo test -- --ignored talks_to_real_tty7`
Expected: all tests pass, including the ignored one (tty7 is running on this machine).

- [ ] **Step 5: Commit**

```bash
git add src
git commit -m "Add Tty7Cli trait with real and fake implementations" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Actions with the race guard

**Files:**
- Create: `src/actions.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`src/lib.rs` — add:

```rust
pub mod actions;
```

`src/actions.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::testing::fleet_with;
    use crate::tty7::fake::{FakeCli, Sent};

    fn tty7_reports(pane: u32, agent: &str, status: &str) -> FakeCli {
        FakeCli::with_agents(&format!(
            r#"{{"agents":[{{"pane_id":{pane},"agent":"{agent}","state":{{"status":"{status}","cwd":"/tmp/x"}}}}]}}"#
        ))
    }

    #[tokio::test]
    async fn approve_types_1_without_enter() {
        let cli = tty7_reports(54, "Claude", "waiting");
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        let outcome = execute(&cli, &fleet, &Action::Approve { pane: 54 }).await;
        assert_eq!(outcome, Outcome::Sent);
        assert_eq!(cli.sent(), vec![Sent::Text { pane: 54, text: "1".into(), enter: false }]);
    }

    #[tokio::test]
    async fn deny_presses_escape() {
        let cli = tty7_reports(54, "Claude", "waiting");
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        let outcome = execute(&cli, &fleet, &Action::Deny { pane: 54 }).await;
        assert_eq!(outcome, Outcome::Sent);
        assert_eq!(cli.sent(), vec![Sent::Keys { pane: 54, keys: vec!["escape".into()] }]);
    }

    #[tokio::test]
    async fn approve_is_dropped_when_agent_moved_on() {
        let cli = tty7_reports(54, "Claude", "working");
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        let outcome = execute(&cli, &fleet, &Action::Approve { pane: 54 }).await;
        assert_eq!(outcome, Outcome::NotReady { status: Some(Status::Working) });
        assert!(cli.sent().is_empty());
    }

    #[tokio::test]
    async fn reply_is_refused_while_a_prompt_is_open() {
        let cli = tty7_reports(54, "Claude", "waiting");
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        let action = Action::Reply { pane: 54, text: "hello".into() };
        assert_eq!(
            execute(&cli, &fleet, &action).await,
            Outcome::NotReady { status: Some(Status::Waiting) }
        );
        assert!(cli.sent().is_empty());
    }

    #[tokio::test]
    async fn reply_to_a_finished_agent_types_text_and_enter() {
        let cli = tty7_reports(54, "Claude", "done");
        let fleet = fleet_with(&[(54, "Claude", Status::Done, 0)]);
        let action = Action::Reply { pane: 54, text: "next: run the tests".into() };
        assert_eq!(execute(&cli, &fleet, &action).await, Outcome::Sent);
        assert_eq!(
            cli.sent(),
            vec![Sent::Text { pane: 54, text: "next: run the tests".into(), enter: true }]
        );
    }

    #[tokio::test]
    async fn unknown_pane_is_rejected_without_asking_tty7() {
        let cli = FakeCli::default();
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        assert_eq!(execute(&cli, &fleet, &Action::Approve { pane: 7 }).await, Outcome::UnknownPane);
        assert!(cli.sent().is_empty());
    }

    #[tokio::test]
    async fn approve_is_unsupported_for_agents_without_a_key_table() {
        let cli = tty7_reports(60, "Codex", "waiting");
        let fleet = fleet_with(&[(60, "Codex", Status::Waiting, 0)]);
        assert_eq!(execute(&cli, &fleet, &Action::Approve { pane: 60 }).await, Outcome::Unsupported);
        assert!(!supports_prompt_answers("Codex"));
        assert!(supports_prompt_answers("Claude"));
    }

    #[tokio::test]
    async fn tty7_failure_is_reported() {
        let cli = FakeCli::default();
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        let outcome = execute(&cli, &fleet, &Action::Approve { pane: 54 }).await;
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test actions::`
Expected: compile errors — `Action`, `Outcome`, `execute` not found.

- [ ] **Step 3: Implement the actions**

Above the test module in `src/actions.rs`:

```rust
//! The only code path that types into panes.

use crate::fleet::Fleet;
use crate::tty7::types::Status;
use crate::tty7::Tty7Cli;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Approve { pane: u32 },
    Deny { pane: u32 },
    Reply { pane: u32, text: String },
}

impl Action {
    pub fn pane(&self) -> u32 {
        match self {
            Action::Approve { pane } | Action::Deny { pane } | Action::Reply { pane, .. } => *pane,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    /// The agent's live status doesn't allow this action; `None` means the pane is gone.
    NotReady { status: Option<Status> },
    UnknownPane,
    Unsupported,
    Failed(String),
}

enum Keystrokes {
    Text { text: String, enter: bool },
    Key(&'static str),
}

/// How to answer an agent's permission prompt; `None` when its prompt layout is unknown.
/// Claude Code (verified 2026-11-13): "1" is always "Yes", but "No" moves between options 3 and 4,
/// so deny uses Esc ("Esc to cancel").
fn prompt_keys(agent: &str, approve: bool) -> Option<Keystrokes> {
    match (agent.to_ascii_lowercase().as_str(), approve) {
        ("claude", true) => Some(Keystrokes::Text { text: "1".into(), enter: false }),
        ("claude", false) => Some(Keystrokes::Key("escape")),
        _ => None,
    }
}

pub fn supports_prompt_answers(agent: &str) -> bool {
    prompt_keys(agent, true).is_some()
}

/// Free text during `waiting` would land in a selection menu, where typed digits pick options.
fn allowed_statuses(action: &Action) -> &'static [Status] {
    match action {
        Action::Approve { .. } | Action::Deny { .. } => &[Status::Waiting],
        Action::Reply { .. } => &[Status::Done, Status::Idle],
    }
}

pub async fn execute(cli: &dyn Tty7Cli, fleet: &Fleet, action: &Action) -> Outcome {
    let pane = action.pane();
    let Some(known) = fleet.panes.get(&pane) else {
        return Outcome::UnknownPane;
    };
    let keystrokes = match action {
        Action::Approve { .. } => prompt_keys(&known.agent, true),
        Action::Deny { .. } => prompt_keys(&known.agent, false),
        Action::Reply { text, .. } => Some(Keystrokes::Text { text: text.clone(), enter: true }),
    };
    let Some(keystrokes) = keystrokes else {
        return Outcome::Unsupported;
    };

    // The fleet can be a few hundred ms stale; ask tty7 right before typing.
    let current = match cli.agents().await {
        Ok(resp) => resp
            .agents
            .into_iter()
            .find(|a| a.pane_id == pane)
            .and_then(|a| a.state.status),
        Err(err) => return Outcome::Failed(err.to_string()),
    };
    if !current.is_some_and(|s| allowed_statuses(action).contains(&s)) {
        return Outcome::NotReady { status: current };
    }

    let result = match keystrokes {
        Keystrokes::Text { text, enter } => cli.send_text(pane, &text, enter).await,
        Keystrokes::Key(key) => cli.send_keys(pane, &[key]).await,
    };
    match result {
        Ok(()) => Outcome::Sent,
        Err(err) => Outcome::Failed(err.to_string()),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test actions::`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src
git commit -m "Add approve/deny/reply actions with race guard" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: The runtime (core loop)

**Files:**
- Create: `src/runtime.rs`
- Modify: `src/lib.rs`

How it works: one tokio task owns the `Fleet`. A `tokio::select!` loop waits on whichever happens first — an event line, a reconnect timer, the 10 s resync tick, the 400 ms preview tick, or a request from a view — handles it, and publishes the fleet if it changed. Views hold a `Handle`: a `watch::Receiver<Fleet>` (always the latest snapshot) plus a request sender.

- [ ] **Step 1: Write the failing tests**

`src/lib.rs` — add:

```rust
pub mod runtime;
```

`src/runtime.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tty7::fake::{FakeCli, Sent};
    use crate::tty7::types::Status;

    const AGENTS: &str = include_str!("../tests/fixtures/agents.json");
    const EVENTS: &str = include_str!("../tests/fixtures/events.ndjson");
    const WAITING_54: &str =
        r#"{"agents":[{"pane_id":54,"agent":"Claude","state":{"status":"waiting","cwd":"/tmp/x"}}]}"#;

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
        assert_eq!(next_backoff(Duration::from_secs(16)), Duration::from_secs(30));
        assert_eq!(next_backoff(Duration::from_secs(30)), Duration::from_secs(30));
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
        wait_for(&handle, |f| f.panes.get(&56).map(|p| p.status) == Some(Status::Waiting)).await;
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

        assert_eq!(handle.act(Action::Approve { pane: 54 }).await, Outcome::Sent);
        assert_eq!(cli.sent(), vec![Sent::Text { pane: 54, text: "1".into(), enter: false }]);
    }

    #[tokio::test]
    async fn watched_panes_get_previews() {
        let cli = Arc::new(FakeCli::with_agents(AGENTS));
        cli.captures.lock().unwrap().insert(54, "hello from 54".into());
        let _events = cli.push_event_stream();
        let handle = start(cli.clone());
        wait_for(&handle, |f| f.panes.contains_key(&54)).await;

        handle.watch("test", HashSet::from([54])).await;

        let fleet = wait_for(&handle, |f| f.panes[&54].preview.is_some()).await;
        assert_eq!(fleet.panes[&54].preview.as_deref(), Some("hello from 54"));
        assert_eq!(fleet.panes[&56].preview, None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test runtime::`
Expected: compile errors — `start`, `Handle`, `next_backoff` not found.

- [ ] **Step 3: Implement the runtime**

Above the test module in `src/runtime.rs`:

```rust
//! The core task: owns the fleet, follows tty7, serves views through a `Handle`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{interval, interval_at, sleep_until, Instant, MissedTickBehavior};
use tracing::warn;

use crate::actions::{self, Action, Outcome};
use crate::fleet::git::{self, GitInfo};
use crate::fleet::{Connection, Fleet};
use crate::tty7::types::{parse_event, Event};
use crate::tty7::Tty7Cli;

pub const PREVIEW_LINES: u32 = 200;
const PREVIEW_EVERY: Duration = Duration::from_millis(400);
/// Also how closed panes disappear: the close event's shape hasn't been recorded yet.
const RESYNC_EVERY: Duration = Duration::from_secs(10);
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
    Action { action: Action, reply: oneshot::Sender<Outcome> },
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
        if self.requests.send(Request::Action { action, reply: tx }).await.is_err() {
            return stopped();
        }
        rx.await.unwrap_or_else(|_| stopped())
    }

    pub async fn watch(&self, client: &str, panes: HashSet<u32>) {
        let request = Request::Watch { client: client.into(), panes };
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
    Handle { snapshots, requests }
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
                _ = preview_tick.tick() => self.refresh_previews().await,
                request = requests.recv() => match request {
                    Some(Request::Action { action, reply }) => {
                        let outcome = actions::execute(self.cli.as_ref(), &self.fleet, &action).await;
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test runtime::`
Expected: 6 passed. `reconnects_when_the_event_stream_ends` takes about 1 s (the first backoff).

- [ ] **Step 5: Run the whole suite and clippy**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "Add runtime core loop with reconnects and previews" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: TUI key handling

**Files:**
- Create: `src/tui/mod.rs` (module declarations only for now), `src/tui/app.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`src/lib.rs` — add:

```rust
pub mod tui;
```

`src/tui/mod.rs`:

```rust
//! The terminal UI.

pub mod app;
```

`src/tui/app.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::testing::fleet_with;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn press(app: &mut App, fleet: &Fleet, codes: &[KeyCode]) -> Option<Command> {
        let mut last = None;
        for code in codes {
            last = app.handle_key(key(*code), fleet);
        }
        last
    }

    fn sample() -> Fleet {
        fleet_with(&[
            (1, "Claude", Status::Waiting, 100),
            (2, "Codex", Status::Waiting, 200),
            (3, "Claude", Status::Working, 50),
            (4, "Claude", Status::Done, 10),
        ])
    }

    #[test]
    fn selection_starts_at_the_top_and_moves() {
        let fleet = sample();
        let mut app = App::default();
        app.sync_selection(&fleet);
        assert_eq!(app.selected, Some(1));
        press(&mut app, &fleet, &[KeyCode::Char('j'), KeyCode::Down]);
        assert_eq!(app.selected, Some(3));
        press(&mut app, &fleet, &[KeyCode::Char('j'), KeyCode::Char('j'), KeyCode::Char('j')]);
        assert_eq!(app.selected, Some(4));
        press(&mut app, &fleet, &[KeyCode::Up]);
        assert_eq!(app.selected, Some(3));
    }

    #[test]
    fn selection_follows_a_pane_that_still_exists() {
        let mut app = App { selected: Some(3), ..App::default() };
        app.sync_selection(&sample());
        assert_eq!(app.selected, Some(3));
        app.sync_selection(&fleet_with(&[(9, "Claude", Status::Done, 0)]));
        assert_eq!(app.selected, Some(9));
    }

    #[test]
    fn y_and_n_answer_a_waiting_claude() {
        let fleet = sample();
        let mut app = App { selected: Some(1), ..App::default() };
        assert_eq!(
            press(&mut app, &fleet, &[KeyCode::Char('y')]),
            Some(Command::Act(Action::Approve { pane: 1 }))
        );
        assert_eq!(
            press(&mut app, &fleet, &[KeyCode::Char('n')]),
            Some(Command::Act(Action::Deny { pane: 1 }))
        );
    }

    #[test]
    fn y_on_an_agent_that_is_not_waiting_explains_why() {
        let fleet = sample();
        let mut app = App { selected: Some(3), ..App::default() };
        assert_eq!(press(&mut app, &fleet, &[KeyCode::Char('y')]), None);
        assert_eq!(app.flash.as_deref(), Some("claude isn't waiting on you"));
    }

    #[test]
    fn y_on_an_unsupported_agent_explains_why() {
        let fleet = sample();
        let mut app = App { selected: Some(2), ..App::default() };
        assert_eq!(press(&mut app, &fleet, &[KeyCode::Char('y')]), None);
        assert_eq!(
            app.flash.as_deref(),
            Some("approve/deny isn't supported for codex yet; answer in its pane")
        );
    }

    #[test]
    fn r_types_a_reply_for_a_finished_agent() {
        let fleet = sample();
        let mut app = App { selected: Some(4), ..App::default() };
        press(&mut app, &fleet, &[KeyCode::Char('r'), KeyCode::Char('o'), KeyCode::Char('k'), KeyCode::Char('x')]);
        assert_eq!(app.reply.as_deref(), Some("okx"));
        press(&mut app, &fleet, &[KeyCode::Backspace]);
        assert_eq!(
            press(&mut app, &fleet, &[KeyCode::Enter]),
            Some(Command::Act(Action::Reply { pane: 4, text: "ok".into() }))
        );
        assert_eq!(app.reply, None);
    }

    #[test]
    fn reply_keys_do_not_trigger_shortcuts() {
        let fleet = sample();
        let mut app = App { selected: Some(4), ..App::default() };
        assert_eq!(press(&mut app, &fleet, &[KeyCode::Char('r'), KeyCode::Char('q')]), None);
        assert_eq!(app.reply.as_deref(), Some("q"));
    }

    #[test]
    fn esc_cancels_and_empty_replies_are_dropped() {
        let fleet = sample();
        let mut app = App { selected: Some(4), ..App::default() };
        press(&mut app, &fleet, &[KeyCode::Char('r'), KeyCode::Char('a'), KeyCode::Esc]);
        assert_eq!(app.reply, None);
        assert_eq!(press(&mut app, &fleet, &[KeyCode::Char('r'), KeyCode::Enter]), None);
    }

    #[test]
    fn r_is_refused_while_a_prompt_is_open() {
        let fleet = sample();
        let mut app = App { selected: Some(1), ..App::default() };
        press(&mut app, &fleet, &[KeyCode::Char('r')]);
        assert_eq!(app.reply, None);
        assert_eq!(app.flash.as_deref(), Some("claude has a prompt open; answer it with y or n"));
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let fleet = sample();
        let mut app = App::default();
        assert_eq!(press(&mut app, &fleet, &[KeyCode::Char('q')]), Some(Command::Quit));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle_key(ctrl_c, &fleet), Some(Command::Quit));
    }

    #[test]
    fn describes_outcomes() {
        assert_eq!(describe(&Outcome::Sent), None);
        assert_eq!(
            describe(&Outcome::NotReady { status: Some(Status::Working) }).as_deref(),
            Some("agent moved on (now working)")
        );
        assert_eq!(describe(&Outcome::NotReady { status: None }).as_deref(), Some("agent is gone"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test tui::app`
Expected: compile errors — `App`, `Command`, `describe` not found.

- [ ] **Step 3: Implement key handling**

Above the test module in `src/tui/app.rs`:

```rust
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::actions::{self, Action, Outcome};
use crate::fleet::Fleet;
use crate::tty7::types::Status;

/// TUI state that isn't part of the fleet. Selection is a pane id, not an index, so it
/// stays on the same agent when the order changes.
#[derive(Debug, Default)]
pub struct App {
    pub selected: Option<u32>,
    /// `Some` while typing a reply.
    pub reply: Option<String>,
    /// One-line message in the footer, cleared on the next key press.
    pub flash: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Act(Action),
    Quit,
}

impl App {
    pub fn sync_selection(&mut self, fleet: &Fleet) {
        if !self.selected.is_some_and(|id| fleet.panes.contains_key(&id)) {
            self.selected = fleet.ordered().first().map(|p| p.pane);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, fleet: &Fleet) -> Option<Command> {
        if self.reply.is_some() {
            return self.handle_reply_key(key);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Command::Quit);
        }
        match key.code {
            KeyCode::Char('q') => Some(Command::Quit),
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(fleet, 1);
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(fleet, -1);
                None
            }
            KeyCode::Char('y') => self.answer_prompt(fleet, true),
            KeyCode::Char('n') => self.answer_prompt(fleet, false),
            KeyCode::Char('r') => {
                self.start_reply(fleet);
                None
            }
            _ => None,
        }
    }

    fn handle_reply_key(&mut self, key: KeyEvent) -> Option<Command> {
        let buffer = self.reply.as_mut()?;
        match key.code {
            KeyCode::Esc => self.reply = None,
            KeyCode::Backspace => {
                buffer.pop();
            }
            KeyCode::Char(c) => buffer.push(c),
            KeyCode::Enter => {
                let text = self.reply.take().unwrap_or_default();
                if text.trim().is_empty() {
                    return None;
                }
                return self.selected.map(|pane| Command::Act(Action::Reply { pane, text }));
            }
            _ => {}
        }
        None
    }

    fn move_selection(&mut self, fleet: &Fleet, delta: isize) {
        let ordered = fleet.ordered();
        if ordered.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .and_then(|id| ordered.iter().position(|p| p.pane == id))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, ordered.len() as isize - 1) as usize;
        self.selected = Some(ordered[next].pane);
    }

    fn answer_prompt(&mut self, fleet: &Fleet, approve: bool) -> Option<Command> {
        let pane = fleet.panes.get(&self.selected?)?;
        let agent = pane.agent.to_lowercase();
        if pane.status != Status::Waiting {
            self.flash = Some(format!("{agent} isn't waiting on you"));
            return None;
        }
        if !actions::supports_prompt_answers(&pane.agent) {
            self.flash = Some(format!("approve/deny isn't supported for {agent} yet; answer in its pane"));
            return None;
        }
        let pane = pane.pane;
        Some(Command::Act(if approve {
            Action::Approve { pane }
        } else {
            Action::Deny { pane }
        }))
    }

    fn start_reply(&mut self, fleet: &Fleet) {
        let Some(pane) = self.selected.and_then(|id| fleet.panes.get(&id)) else {
            return;
        };
        match pane.status {
            Status::Done | Status::Idle => self.reply = Some(String::new()),
            Status::Waiting => {
                self.flash = Some(format!(
                    "{} has a prompt open; answer it with y or n",
                    pane.agent.to_lowercase()
                ))
            }
            other => {
                self.flash = Some(format!(
                    "{} is {}; reply once it's done",
                    pane.agent.to_lowercase(),
                    other.label()
                ))
            }
        }
    }
}

/// Footer text for an action's result; `None` when there's nothing worth saying.
pub fn describe(outcome: &Outcome) -> Option<String> {
    match outcome {
        Outcome::Sent => None,
        Outcome::NotReady { status: Some(status) } => Some(format!("agent moved on (now {})", status.label())),
        Outcome::NotReady { status: None } => Some("agent is gone".into()),
        Outcome::UnknownPane => Some("that pane is gone".into()),
        Outcome::Unsupported => Some("not supported for this agent".into()),
        Outcome::Failed(err) => Some(format!("failed: {err}")),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test tui::app`
Expected: 11 passed.

- [ ] **Step 5: Commit**

```bash
git add src
git commit -m "Add TUI key handling" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: TUI rendering

**Files:**
- Create: `src/tui/view.rs`
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Write the failing tests**

`src/tui/mod.rs` — add below `pub mod app;`:

```rust
pub mod view;
```

`src/tui/view.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::testing::fleet_with;
    use crate::tty7::types::AgentsResponse;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    const NOW: u64 = 1_000_000;

    fn draw(fleet: &Fleet, app: &App) -> String {
        let (width, height) = (60, 20);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(frame, fleet, app, NOW)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn sample() -> Fleet {
        fleet_with(&[(1, "Claude", Status::Waiting, NOW - 125_000), (2, "Codex", Status::Working, NOW - 41_000)])
    }

    #[test]
    fn header_counts_agents_and_waiting_ones() {
        let screen = draw(&sample(), &App::default());
        let header = screen.lines().next().unwrap();
        assert!(header.contains("shipherd"), "{screen}");
        assert!(header.contains("2 agents"), "{screen}");
        assert!(header.contains("1 need you"), "{screen}");
    }

    #[test]
    fn groups_appear_in_status_order_with_elapsed_time() {
        let screen = draw(&sample(), &App::default());
        let needs_you = screen.find("NEEDS YOU").expect("NEEDS YOU heading");
        let working = screen.find("WORKING").expect("WORKING heading");
        assert!(needs_you < working, "{screen}");
        assert!(screen.contains("2m05s"), "{screen}");
        assert!(screen.contains("41s"), "{screen}");
        assert!(screen.contains("codex"), "{screen}");
    }

    #[test]
    fn statuses_seen_at_startup_show_a_lower_bound() {
        let mut fleet = Fleet::default();
        let resp: AgentsResponse = serde_json::from_str(include_str!("../../tests/fixtures/agents.json")).unwrap();
        fleet.resync(&resp, NOW - 60_000);
        assert!(draw(&fleet, &App::default()).contains("≥1m00s"));
    }

    #[test]
    fn banners_for_missing_hooks_and_lost_connection() {
        let mut fleet = Fleet::default();
        let resp: AgentsResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/agents-no-hooks.json")).unwrap();
        fleet.resync(&resp, NOW);
        fleet.connection = Connection::Reconnecting;
        let screen = draw(&fleet, &App::default());
        assert!(screen.contains("status hooks not installed for claude"), "{screen}");
        assert!(screen.contains("reconnecting"), "{screen}");
        assert!(screen.contains("no agents running in tty7"), "{screen}");
    }

    #[test]
    fn preview_shows_the_last_lines_of_the_selected_agent() {
        let mut fleet = sample();
        fleet.set_preview(1, "first\nmiddle\nDo you want to proceed?\n\n\n".into());
        let app = App { selected: Some(1), ..App::default() };
        let screen = draw(&fleet, &app);
        assert!(screen.contains("Do you want to proceed?"), "{screen}");
    }

    #[test]
    fn reply_input_and_flash_are_visible() {
        let app = App { selected: Some(1), reply: Some("hi there".into()), ..App::default() };
        assert!(draw(&sample(), &app).contains("reply> hi there"));

        let app = App { flash: Some("agent moved on (now working)".into()), ..App::default() };
        assert!(draw(&sample(), &app).contains("agent moved on (now working)"));
    }

    #[test]
    fn footer_lists_the_keys() {
        assert!(draw(&sample(), &App::default()).contains("y approve · n deny · r reply · q quit"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test tui::view`
Expected: compile error — `render` not found.

- [ ] **Step 3: Implement rendering**

Above the test module in `src/tui/view.rs`:

```rust
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::app::App;
use crate::fleet::{format_elapsed, AgentPane, Connection, Fleet};
use crate::tty7::types::Status;

const GROUPS: [(Status, &str); 5] = [
    (Status::Waiting, "NEEDS YOU"),
    (Status::Working, "WORKING"),
    (Status::Done, "DONE"),
    (Status::Idle, "IDLE"),
    (Status::Unknown, "OTHER"),
];

fn status_color(status: Status) -> Color {
    match status {
        Status::Waiting => Color::Yellow,
        Status::Working => Color::Blue,
        Status::Done => Color::Green,
        Status::Idle | Status::Unknown => Color::DarkGray,
    }
}

fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

pub fn render(frame: &mut Frame, fleet: &Fleet, app: &App, now_ms: u64) {
    let banners = banners(fleet);
    let [header, banner_area, list, preview, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(banners.len() as u16),
        Constraint::Min(3),
        Constraint::Percentage(40),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    frame.render_widget(Paragraph::new(header_line(fleet)), header);
    frame.render_widget(Paragraph::new(banners), banner_area);
    frame.render_widget(Paragraph::new(list_lines(fleet, app, now_ms)), list);
    frame.render_widget(Paragraph::new(preview_lines(fleet, app, preview.height)), preview);
    frame.render_widget(Paragraph::new(footer_line(app)), footer);
}

fn header_line(fleet: &Fleet) -> Line<'static> {
    let mut spans = vec![
        Span::styled("⛵ shipherd", Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {} agents", fleet.panes.len()), dim()),
    ];
    let needs_you = fleet.count(Status::Waiting);
    if needs_you > 0 {
        spans.push(Span::styled(format!(" · {needs_you} need you"), Style::new().fg(Color::Yellow)));
    }
    Line::from(spans)
}

fn banners(fleet: &Fleet) -> Vec<Line<'static>> {
    let warn = Style::new().fg(Color::Black).bg(Color::Yellow);
    let mut lines: Vec<Line<'static>> = fleet
        .hooks_missing()
        .into_iter()
        .map(|agent| Line::styled(format!(" status hooks not installed for {agent}: tty7 Settings → Integrations "), warn))
        .collect();
    match fleet.connection {
        Connection::Connected => {}
        Connection::Reconnecting => lines.push(Line::styled(" tty7 event stream lost, reconnecting… ", warn)),
        Connection::Unreachable => lines.push(Line::styled(" tty7 server not reachable, retrying… ", warn)),
    }
    lines
}

fn list_lines(fleet: &Fleet, app: &App, now_ms: u64) -> Vec<Line<'static>> {
    let ordered = fleet.ordered();
    if ordered.is_empty() {
        return vec![Line::styled("no agents running in tty7", dim())];
    }
    let mut lines = Vec::new();
    for (status, title) in GROUPS {
        let group: Vec<&AgentPane> = ordered.iter().copied().filter(|p| p.status == status).collect();
        if group.is_empty() {
            continue;
        }
        let heading = Style::new().fg(status_color(status)).add_modifier(Modifier::BOLD);
        lines.push(Line::styled(title, heading));
        for pane in group {
            lines.push(agent_line(pane, app.selected == Some(pane.pane), now_ms));
        }
    }
    lines
}

fn agent_line(pane: &AgentPane, selected: bool, now_ms: u64) -> Line<'static> {
    let elapsed = format_elapsed(now_ms.saturating_sub(pane.status_since_ms), pane.since_exact);
    let text = format!(
        " {:<7} {:<12} {:<14} {:>7}",
        pane.agent.to_lowercase(),
        pane.repo.as_deref().unwrap_or("-"),
        pane.branch.as_deref().unwrap_or("-"),
        elapsed
    );
    let line = Line::from(vec![
        Span::styled("●", Style::new().fg(status_color(pane.status))),
        Span::raw(text),
    ]);
    if selected {
        line.style(Style::new().bg(Color::Rgb(31, 42, 58)))
    } else {
        line
    }
}

fn preview_lines(fleet: &Fleet, app: &App, height: u16) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled("─".repeat(200), dim())];
    let input = app.reply.as_ref().map(|buffer| {
        Line::from(vec![
            Span::styled("reply> ", Style::new().fg(Color::Cyan)),
            Span::raw(format!("{buffer}▏")),
        ])
    });
    let room = (height as usize).saturating_sub(1 + usize::from(input.is_some()));
    let preview = app
        .selected
        .and_then(|id| fleet.panes.get(&id))
        .and_then(|p| p.preview.as_deref())
        .unwrap_or("");
    let all: Vec<&str> = preview.lines().collect();
    // Full-screen agents pad their output with blank lines; show the last lines with content.
    let end = all.iter().rposition(|l| !l.trim().is_empty()).map_or(0, |i| i + 1);
    let start = end.saturating_sub(room);
    lines.extend(all[start..end].iter().map(|l| Line::raw(l.to_string())));
    lines.extend(input);
    lines
}

fn footer_line(app: &App) -> Line<'static> {
    match (&app.flash, &app.reply) {
        (Some(flash), _) => Line::styled(flash.clone(), Style::new().fg(Color::Yellow)),
        (None, Some(_)) => Line::styled("enter send · esc cancel", dim()),
        (None, None) => Line::styled("↑↓ select · y approve · n deny · r reply · q quit", dim()),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test tui::view`
Expected: 7 passed.

If `buffer[(x, y)]` doesn't compile with the installed ratatui version, use `buffer.cell((x, y)).unwrap().symbol()` instead.

- [ ] **Step 5: Commit**

```bash
git add src
git commit -m "Add TUI rendering" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Wire it up — event loop, logging, main

**Files:**
- Modify: `src/tui/mod.rs`, `src/main.rs`, `src/lib.rs`
- Create: `src/logging.rs`

This task is glue around a real terminal, so it's verified by running it rather than by unit tests.

- [ ] **Step 1: Logging**

`src/lib.rs` — add:

```rust
pub mod logging;
```

`src/logging.rs`:

```rust
use std::path::PathBuf;

use anyhow::Context;
use tracing_appender::non_blocking::WorkerGuard;

/// Logs go to a file because anything printed to the terminal would corrupt the TUI.
/// Keep the returned guard alive until exit; dropping it flushes the log.
pub fn init() -> anyhow::Result<WorkerGuard> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let dir = PathBuf::from(home).join(".local/state/shipherd");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let (writer, guard) = tracing_appender::non_blocking(tracing_appender::rolling::never(&dir, "shipherd.log"));
    tracing_subscriber::fmt().with_writer(writer).with_ansi(false).init();
    Ok(guard)
}
```

- [ ] **Step 2: The TUI event loop**

Replace `src/tui/mod.rs` with:

```rust
//! The terminal UI.

pub mod app;
pub mod view;

use std::collections::HashSet;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use crate::runtime::{now_ms, Handle};
use app::{App, Command};

pub async fn run(handle: Handle) -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, handle).await;
    ratatui::restore();
    result
}

async fn event_loop(terminal: &mut DefaultTerminal, handle: Handle) -> anyhow::Result<()> {
    let mut keys = spawn_key_reader();
    let mut snapshots = handle.snapshots.clone();
    // Redraws once a second even without changes, so elapsed times keep counting.
    let mut clock = tokio::time::interval(Duration::from_secs(1));
    let mut app = App::default();
    let mut watched = None;

    loop {
        let fleet = snapshots.borrow_and_update().clone();
        app.sync_selection(&fleet);
        if app.selected != watched {
            watched = app.selected;
            handle.watch("tui", watched.into_iter().collect::<HashSet<_>>()).await;
        }
        terminal.draw(|frame| view::render(frame, &fleet, &app, now_ms()))?;

        tokio::select! {
            changed = snapshots.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
            }
            _ = clock.tick() => {}
            key = keys.recv() => {
                let Some(key) = key else { return Ok(()) };
                app.flash = None;
                match app.handle_key(key, &fleet) {
                    Some(Command::Quit) => return Ok(()),
                    Some(Command::Act(action)) => {
                        let outcome = handle.act(action).await;
                        app.flash = app::describe(&outcome);
                    }
                    None => {}
                }
            }
        }
    }
}

/// crossterm's `event::read` blocks, so it runs on its own thread and forwards key presses.
fn spawn_key_reader() -> mpsc::UnboundedReceiver<KeyEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || loop {
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                if tx.send(key).is_err() {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    });
    rx
}
```

- [ ] **Step 3: main**

Replace `src/main.rs` with:

```rust
use std::io::ErrorKind;
use std::process::{exit, Command};
use std::sync::Arc;

use shipherd::tty7::cli::RealCli;
use shipherd::{logging, runtime, tui};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(err) = Command::new("tty7").arg("--version").output() {
        if err.kind() == ErrorKind::NotFound {
            eprintln!("shipherd needs the tty7 CLI on your PATH. Install tty7 from https://tty7.io");
            exit(1);
        }
    }
    let _log_guard = logging::init()?;
    let handle = runtime::start(Arc::new(RealCli));
    tui::run(handle).await
}
```

- [ ] **Step 4: Full check**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: no warnings; all tests pass (the real-tty7 test stays ignored).

- [ ] **Step 5: Manual verification in tty7**

1. In tty7, split a pane and run `cargo run --release` in `~/Code/shipherd`.
   Expected: header `⛵ shipherd  N agents`, your Claude sessions listed under their status groups with repo and branch; times shown as `≥…` for agents that were already running.
2. In another pane: `mkdir -p /tmp/sh-test && cd /tmp/sh-test && claude`, then ask it to *"Use the Bash tool to run exactly: touch approve-me.txt"*.
   Expected: that agent moves to NEEDS YOU within a second; the header shows `1 need you`; selecting it shows the "Do you want to proceed?" prompt in the preview.
3. Press `y`. Expected: `/tmp/sh-test/approve-me.txt` exists and the agent moves to WORKING, then DONE.
4. Ask for another command and press `n` when it waits. Expected: the prompt is cancelled and the agent moves to DONE.
5. With the agent DONE, press `r`, type `say hi`, Enter. Expected: the agent starts working on "say hi".
6. Press `y` on a WORKING agent. Expected: footer says `claude isn't waiting on you`.
7. `q` quits and leaves the terminal clean. `~/.local/state/shipherd/shipherd.log` exists.

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "Wire up TUI event loop, logging and main" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## What Plan 2 covers (not in this plan)

The web dashboard: axum server on `127.0.0.1` with token + Host + Origin checks, the kanban board, the detail panel with live mirror and pane facts (`tty7 procs`), interact mode (`send_keys` with the 2-minute auto-off), `w` in the TUI, and `--port`. It subscribes to the same `runtime::Handle` (`watch` per browser client).
