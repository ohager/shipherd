use serde::{Deserialize, Serialize};

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
    /// Identifier used in the web protocol and CSS classes.
    pub fn key(self) -> &'static str {
        match self {
            Status::Waiting => "waiting",
            Status::Working => "working",
            Status::Done => "done",
            Status::Idle => "idle",
            Status::Unknown => "unknown",
        }
    }

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
    /// The agent's own session id (Claude Code: names its transcript directory).
    #[serde(default)]
    pub session_id: Option<String>,
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
    /// The terminal title the agent sets, e.g. "◐ Run ls -la".
    #[serde(default)]
    pub osc_title: Option<String>,
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
    #[serde(default)]
    pub launch_argv: Vec<String>,
}

/// `tty7 capture --plain --json`
#[derive(Debug, Clone, Deserialize)]
pub struct CaptureResponse {
    pub text: String,
}

/// `tty7 procs --json`. Port entries stay untyped: no listening port has been recorded yet.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ProcsResponse {
    #[serde(default)]
    pub procs: Vec<Proc>,
    #[serde(default)]
    pub ports: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
    #[serde(default)]
    pub foreground: bool,
}

/// `tty7 ls --json`
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct WorkspacesResponse {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
}

/// `tty7 tab ls <workspace> --json`. tty7 detects agents per tab, not per pane.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct TabsResponse {
    #[serde(default)]
    pub tabs: Vec<Tab>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Tab {
    pub label: Option<String>,
    pub agent: Option<String>,
    #[serde(default)]
    pub panes: Vec<u32>,
}

/// `tty7 pane ls --all --json`
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PanesResponse {
    #[serde(default)]
    pub panes: Vec<PaneInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PaneInfo {
    pub pane: u32,
    /// The pane's foreground process, e.g. "claude.exe".
    pub title: Option<String>,
    pub cwd: Option<String>,
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
        assert_eq!(
            resp.agents[0].state.cwd.as_deref(),
            Some("/home/dev/code/hermes")
        );
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
        assert_eq!(
            statuses,
            vec![Status::Waiting, Status::Working, Status::Done, Status::Idle]
        );
    }

    #[test]
    fn pane_facts_carry_title_and_launch_command() {
        let line = EVENTS.lines().nth(2).unwrap();
        let Event::PaneFacts(facts) = parse_event(line).unwrap() else {
            panic!("expected pane facts");
        };
        assert_eq!(facts.osc_title.as_deref(), Some("◐ Run ls -la"));
        assert_eq!(facts.agent.unwrap().launch_argv, vec!["claude".to_string()]);
    }

    #[test]
    fn status_keys_are_stable_identifiers() {
        assert_eq!(Status::Waiting.key(), "waiting");
        assert_eq!(Status::Unknown.key(), "unknown");
    }

    #[test]
    fn parses_procs() {
        let procs: ProcsResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/procs.json")).unwrap();
        assert_eq!(procs.procs.len(), 2);
        assert_eq!(
            procs.procs[1],
            Proc {
                pid: 33569,
                name: "claude.exe".into(),
                foreground: true
            }
        );
        assert!(procs.ports.is_empty());
    }

    #[test]
    fn parses_workspaces_tabs_and_panes() {
        let ws: WorkspacesResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/ls.json")).unwrap();
        assert_eq!(ws.workspaces[1].name, "PR Reviews");
        let tabs: TabsResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/tabs-default.json")).unwrap();
        assert_eq!(tabs.tabs[0].agent.as_deref(), Some("Claude Code"));
        assert_eq!(tabs.tabs[0].panes, vec![54, 61]);
        assert_eq!(tabs.tabs[1].agent, None);
        let panes: PanesResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/panes.json")).unwrap();
        assert_eq!(panes.panes[0].title.as_deref(), Some("claude.exe"));
        assert_eq!(panes.panes[3].cwd.as_deref(), Some("/home/dev/code/e2e"));
    }
}
