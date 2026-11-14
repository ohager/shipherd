//! JSON messages between the page and the server. Every message has a `type` field.

use serde::{Deserialize, Serialize};

use crate::actions::KeyInput;
use crate::agents;
use crate::fleet::AgentPane;
use crate::tty7::types::Proc;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackgroundView {
    pub kind: String,
    pub description: String,
    pub since_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentView {
    pub pane: u32,
    pub agent: String,
    pub status: &'static str,
    pub since_ms: u64,
    pub exact: bool,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub launch: Vec<String>,
    pub workspace: Option<String>,
    /// False until the agent's hooks report a status; the page shows "no status yet".
    pub has_status: bool,
    /// Whether approve/deny are supported for this agent.
    pub can_answer: bool,
    /// False when the agent's prompt layout comes from its source, not from real screens.
    pub answer_verified: bool,
    /// The board column: `status`, except finished agents with background work are "working".
    pub group: &'static str,
    pub background: Vec<BackgroundView>,
}

impl From<&AgentPane> for AgentView {
    fn from(p: &AgentPane) -> Self {
        let profile = agents::profile_for(&p.agent);
        AgentView {
            pane: p.pane,
            agent: p.agent.to_lowercase(),
            status: p.status.key(),
            since_ms: p.status_since_ms,
            exact: p.since_exact,
            repo: p.repo.clone(),
            branch: p.branch.clone(),
            cwd: p.cwd.clone(),
            title: p.title.clone(),
            launch: p.launch.clone(),
            workspace: p.workspace.clone(),
            has_status: p.has_status,
            can_answer: profile.is_some(),
            answer_verified: profile.is_some_and(|p| p.verified()),
            group: p.board_status().key(),
            background: p
                .background
                .iter()
                .map(|t| BackgroundView {
                    kind: t.kind.clone(),
                    description: t.description.clone(),
                    since_ms: t.since_ms,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// The whole board, already in display order. Sent only when it changes.
    Snapshot {
        agents: Vec<AgentView>,
        banners: Vec<String>,
    },
    Preview {
        pane: u32,
        text: String,
        /// Lets the page offer approve/deny only when they'd answer a permission prompt.
        permission_prompt: bool,
    },
    Procs {
        pane: u32,
        procs: Vec<Proc>,
        ports: Vec<serde_json::Value>,
    },
    ActionResult {
        id: u64,
        ok: bool,
        message: Option<String>,
    },
    /// Interact mode's state as the server sees it; the page follows it.
    Interact { pane: u32, on: bool },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// The pane shown in the detail panel; `None` when it's closed.
    Watch {
        #[serde(default)]
        pane: Option<u32>,
    },
    Approve {
        id: u64,
        pane: u32,
    },
    Deny {
        id: u64,
        pane: u32,
    },
    Reply {
        id: u64,
        pane: u32,
        text: String,
    },
    Interact {
        pane: u32,
        on: bool,
    },
    Keys {
        pane: u32,
        keys: Vec<KeyInput>,
    },
    /// Brings the tty7 window to the front.
    OpenTty7,
    /// Sent only after the page asked the user to confirm.
    Close {
        id: u64,
        pane: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::BackgroundTask;
    use crate::fleet::testing::fleet_with;
    use crate::tty7::types::Status;

    #[test]
    fn agent_view_from_pane() {
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 1_000)]);
        let view = AgentView::from(&fleet.panes[&54]);
        assert_eq!(view.agent, "claude");
        assert_eq!(view.status, "waiting");
        assert_eq!(view.since_ms, 1_000);
        assert!(view.can_answer);
        assert!(view.answer_verified);
    }

    #[test]
    fn server_messages_are_tagged_json() {
        let json = serde_json::to_value(ServerMsg::Interact { pane: 3, on: false }).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "interact", "pane": 3, "on": false})
        );
    }

    #[test]
    fn client_messages_parse() {
        let msg: ClientMsg =
            serde_json::from_str(r#"{"type":"reply","id":4,"pane":54,"text":"go"}"#).unwrap();
        assert_eq!(
            msg,
            ClientMsg::Reply {
                id: 4,
                pane: 54,
                text: "go".into()
            }
        );
        let msg: ClientMsg = serde_json::from_str(r#"{"type":"watch"}"#).unwrap();
        assert_eq!(msg, ClientMsg::Watch { pane: None });
        assert!(serde_json::from_str::<ClientMsg>(r#"{"type":"rm"}"#).is_err());
    }

    #[test]
    fn agent_view_carries_workspace_and_whether_status_is_known() {
        let mut fleet = fleet_with(&[]);
        fleet.set_detected(
            vec![crate::fleet::testing::detected(22, "PR Reviews", "x")],
            0,
        );
        let view = AgentView::from(&fleet.panes[&22]);
        assert_eq!(view.workspace.as_deref(), Some("PR Reviews"));
        assert!(!view.has_status);
    }

    #[test]
    fn open_tty7_parses() {
        let msg: ClientMsg = serde_json::from_str(r#"{"type":"open_tty7"}"#).unwrap();
        assert_eq!(msg, ClientMsg::OpenTty7);
    }

    #[test]
    fn close_parses() {
        let msg: ClientMsg = serde_json::from_str(r#"{"type":"close","id":3,"pane":22}"#).unwrap();
        assert_eq!(msg, ClientMsg::Close { id: 3, pane: 22 });
    }

    #[test]
    fn agent_view_carries_background_work_and_its_board_column() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Done, 0)]);
        let task = BackgroundTask {
            kind: "researcher".into(),
            description: "Locating".into(),
            since_ms: 5,
        };
        fleet.set_background(&std::collections::HashMap::from([(1, vec![task])]));
        let view = AgentView::from(&fleet.panes[&1]);
        assert_eq!(view.status, "done");
        assert_eq!(view.group, "working");
        assert_eq!(view.background.len(), 1);
        assert_eq!(view.background[0].kind, "researcher");
        assert_eq!(view.background[0].since_ms, 5);
    }
}
