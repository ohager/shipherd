//! The current set of agent panes. Pure state: no I/O, time is passed in.

pub mod git;

use std::collections::BTreeMap;

use crate::tty7::detect::DetectedPane;
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
    /// The agent's current task, from its terminal title.
    pub title: Option<String>,
    pub launch: Vec<String>,
    pub workspace: Option<String>,
    /// False until the agent's status hooks report something; such agents show as idle.
    pub has_status: bool,
}

impl AgentPane {
    fn new(
        pane: u32,
        agent: &str,
        status: Status,
        cwd: Option<String>,
        now_ms: u64,
        exact: bool,
    ) -> Self {
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
            title: None,
            launch: Vec::new(),
            workspace: None,
            has_status: false,
        }
    }

    fn update(
        &mut self,
        agent: &str,
        status: Status,
        cwd: Option<String>,
        now_ms: u64,
        exact: bool,
    ) {
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
    /// The latest detection pass; kept so a resync doesn't drop agents that report no status.
    pub detected: Vec<DetectedPane>,
}

impl Fleet {
    /// Replaces the pane set with a full `tty7 agents` listing. Status changes found this way
    /// happened at some unknown point since the last update, so they're marked inexact.
    pub fn resync(&mut self, resp: &AgentsResponse, now_ms: u64) {
        let mut next = BTreeMap::new();
        for entry in &resp.agents {
            let status = entry.state.status.unwrap_or(Status::Idle);
            let cwd = entry.state.cwd.clone();
            let mut pane = match self.panes.remove(&entry.pane_id) {
                Some(mut existing) => {
                    existing.update(&entry.agent, status, cwd, now_ms, false);
                    existing
                }
                None => AgentPane::new(entry.pane_id, &entry.agent, status, cwd, now_ms, false),
            };
            pane.launch = entry.state.launch_argv.clone();
            pane.has_status = entry.state.status.is_some();
            next.insert(entry.pane_id, pane);
        }
        // Whatever is left is either detected without a status, or closed.
        let mut previous = std::mem::replace(&mut self.panes, next);
        for detected in &self.detected {
            self.panes.entry(detected.pane).or_insert_with(|| {
                previous
                    .remove(&detected.pane)
                    .unwrap_or_else(|| detected_pane(detected, now_ms))
            });
        }
        self.diagnostics = resp.diagnostics.clone();
        self.apply_detected_details();
    }

    /// Adds agents tty7 detected but `tty7 agents` doesn't list. Panes that stopped being detected
    /// are dropped on the next resync, which knows which panes `tty7 agents` still reports.
    pub fn set_detected(&mut self, detected: Vec<DetectedPane>, now_ms: u64) {
        for d in &detected {
            self.panes
                .entry(d.pane)
                .or_insert_with(|| detected_pane(d, now_ms));
        }
        self.detected = detected;
        self.apply_detected_details();
    }

    fn apply_detected_details(&mut self) {
        for d in &self.detected {
            if let Some(pane) = self.panes.get_mut(&d.pane) {
                pane.workspace = Some(d.workspace.clone());
                if pane.title.is_none() {
                    pane.title = d.label.clone();
                }
            }
        }
    }

    pub fn apply_pane_facts(&mut self, facts: &PaneFacts, now_ms: u64) {
        let Some(agent) = facts.agent.as_ref().filter(|_| facts.live) else {
            self.panes.remove(&facts.id);
            return;
        };
        let status = agent.status.unwrap_or(Status::Idle);
        let cwd = facts.cwd.clone();
        let pane = self.panes.entry(facts.id).or_insert_with(|| {
            AgentPane::new(facts.id, &agent.agent, status, cwd.clone(), now_ms, true)
        });
        pane.update(&agent.agent, status, cwd, now_ms, true);
        pane.has_status |= agent.status.is_some();
        pane.launch = agent.launch_argv.clone();
        if let Some(title) = facts.osc_title.as_deref().and_then(clean_title) {
            pane.title = Some(title);
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

    /// Warnings every view shows above the agent list.
    pub fn banners(&self) -> Vec<String> {
        let mut banners: Vec<String> = self
            .hooks_missing()
            .into_iter()
            .map(|agent| {
                format!("status hooks not installed for {agent}: tty7 Settings → Integrations")
            })
            .collect();
        match self.connection {
            Connection::Connected => {}
            Connection::Reconnecting => {
                banners.push("tty7 event stream lost, reconnecting…".into())
            }
            Connection::Unreachable => banners.push("tty7 server not reachable, retrying…".into()),
        }
        banners
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
    if exact { body } else { format!("≥{body}") }
}

/// Its real status and start are unknown, so it counts as idle since it was first seen.
fn detected_pane(d: &DetectedPane, now_ms: u64) -> AgentPane {
    AgentPane::new(d.pane, &d.agent, Status::Idle, d.cwd.clone(), now_ms, false)
}

/// Agents prefix their terminal title with a spinner or status glyph ("◐ Run ls -la").
fn clean_title(raw: &str) -> Option<String> {
    let mut chars = raw.chars();
    let title = match (chars.next(), chars.next()) {
        (Some(first), Some(' ')) if !first.is_alphanumeric() => chars.as_str(),
        _ => raw,
    }
    .trim();
    (!title.is_empty()).then(|| title.to_string())
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::tty7::types::{PaneAgent, PaneFacts};

    pub fn facts(id: u32, agent: &str, status: Option<Status>, cwd: &str) -> PaneFacts {
        PaneFacts {
            id,
            cwd: Some(cwd.into()),
            osc_title: None,
            agent: Some(PaneAgent {
                agent: agent.into(),
                status,
                launch_argv: Vec::new(),
            }),
            live: true,
        }
    }

    pub fn detected(pane: u32, workspace: &str, label: &str) -> DetectedPane {
        DetectedPane {
            pane,
            agent: "Claude".into(),
            workspace: workspace.into(),
            label: Some(label.into()),
            cwd: Some("/tmp/detected".into()),
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
    use super::testing::{detected, facts, fleet_with};
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
        fleet.apply_pane_facts(
            &facts(54, "Claude", Some(Status::Working), "/home/dev/code/hermes"),
            2_000,
        );
        assert_eq!(fleet.panes[&54].status_since_ms, 1_000);
        assert!(!fleet.panes[&54].since_exact);

        fleet.apply_pane_facts(
            &facts(54, "Claude", Some(Status::Waiting), "/home/dev/code/hermes"),
            3_000,
        );
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
        let mut fleet = fleet_with(&[
            (1, "Claude", Status::Working, 0),
            (2, "Claude", Status::Done, 0),
        ]);
        fleet.apply_pane_facts(
            &PaneFacts {
                id: 1,
                cwd: None,
                osc_title: None,
                agent: None,
                live: true,
            },
            10,
        );
        let mut gone = facts(2, "Claude", Some(Status::Done), "/tmp/x");
        gone.live = false;
        fleet.apply_pane_facts(&gone, 10);
        assert!(fleet.panes.is_empty());
    }

    #[test]
    fn resync_drops_closed_panes_and_marks_changes_inexact() {
        let mut fleet = fleet_with(&[
            (54, "Claude", Status::Waiting, 0),
            (99, "Claude", Status::Working, 0),
        ]);
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

    #[test]
    fn titles_lose_their_spinner_glyph() {
        assert_eq!(clean_title("◐ Run ls -la").as_deref(), Some("Run ls -la"));
        assert_eq!(clean_title("✳ Claude Code").as_deref(), Some("Claude Code"));
        assert_eq!(clean_title("claude").as_deref(), Some("claude"));
        assert_eq!(clean_title("◐ ").as_deref(), None);
    }

    #[test]
    fn events_set_title_and_launch_and_resync_keeps_the_title() {
        let mut fleet = Fleet::default();
        let mut with_title = facts(56, "Claude", Some(Status::Working), "/tmp/a");
        with_title.osc_title = Some("◐ Run ls -la".into());
        with_title.agent.as_mut().unwrap().launch_argv = vec!["claude".into()];
        fleet.apply_pane_facts(&with_title, 1);
        assert_eq!(fleet.panes[&56].title.as_deref(), Some("Run ls -la"));
        assert_eq!(fleet.panes[&56].launch, vec!["claude".to_string()]);

        fleet.resync(
            &agents(r#"{"agents":[{"pane_id":56,"agent":"Claude","state":{"status":"working","cwd":"/tmp/a","launch_argv":["claude","--resume"]}}]}"#),
            2,
        );
        assert_eq!(fleet.panes[&56].title.as_deref(), Some("Run ls -la"));
        assert_eq!(
            fleet.panes[&56].launch,
            vec!["claude".to_string(), "--resume".to_string()]
        );
    }

    #[test]
    fn banners_cover_hooks_and_connection() {
        let mut fleet = Fleet::default();
        fleet.resync(&agents(NO_HOOKS), 0);
        fleet.connection = Connection::Unreachable;
        assert_eq!(
            fleet.banners(),
            vec![
                "status hooks not installed for claude: tty7 Settings → Integrations".to_string(),
                "tty7 server not reachable, retrying…".to_string(),
            ]
        );
    }

    #[test]
    fn detected_agents_without_a_status_are_listed_as_idle() {
        let mut fleet = Fleet::default();
        fleet.resync(&agents(AGENTS), 1_000);
        fleet.set_detected(
            vec![detected(22, "PR Reviews", "Routinen ausführen")],
            2_000,
        );

        let pane = &fleet.panes[&22];
        assert_eq!(pane.status, Status::Idle);
        assert!(!pane.has_status);
        assert!(!pane.since_exact);
        assert_eq!(pane.workspace.as_deref(), Some("PR Reviews"));
        assert_eq!(pane.title.as_deref(), Some("Routinen ausführen"));
        assert_eq!(pane.cwd.as_deref(), Some("/tmp/detected"));
        assert!(fleet.panes[&54].has_status);
    }

    #[test]
    fn resync_keeps_detected_agents_and_their_start_time() {
        let mut fleet = Fleet::default();
        fleet.set_detected(vec![detected(22, "PR Reviews", "x")], 1_000);
        fleet.resync(&agents(AGENTS), 5_000);
        assert_eq!(fleet.panes[&22].status_since_ms, 1_000);
        assert_eq!(fleet.panes.len(), 3);
    }

    #[test]
    fn reported_status_wins_over_detection() {
        let mut fleet = Fleet::default();
        fleet.set_detected(
            vec![detected(54, "Default", "Herdr and tty7 comparison")],
            1_000,
        );
        fleet.resync(&agents(AGENTS), 2_000);
        let pane = &fleet.panes[&54];
        assert_eq!(pane.status, Status::Working);
        assert!(pane.has_status);
        assert_eq!(pane.workspace.as_deref(), Some("Default"));
        assert_eq!(pane.cwd.as_deref(), Some("/home/dev/code/hermes"));
    }

    #[test]
    fn panes_no_longer_detected_disappear_on_resync() {
        let mut fleet = Fleet::default();
        fleet.set_detected(vec![detected(22, "PR Reviews", "x")], 1_000);
        fleet.set_detected(Vec::new(), 2_000);
        fleet.resync(&agents(AGENTS), 3_000);
        assert!(!fleet.panes.contains_key(&22));
    }

    #[test]
    fn a_status_event_marks_a_detected_agent_as_reporting() {
        let mut fleet = Fleet::default();
        fleet.set_detected(vec![detected(22, "PR Reviews", "x")], 1_000);
        fleet.apply_pane_facts(
            &facts(22, "Claude", Some(Status::Waiting), "/tmp/detected"),
            2_000,
        );
        assert!(fleet.panes[&22].has_status);
        assert_eq!(fleet.panes[&22].status, Status::Waiting);
        assert!(fleet.panes[&22].since_exact);
    }
}
