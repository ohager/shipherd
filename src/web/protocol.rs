//! JSON messages between the page and the server. Every message has a `type` field.

use serde::{Deserialize, Serialize};

use crate::actions::{self, KeyInput};
use crate::fleet::AgentPane;
use crate::tty7::types::Proc;

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
    /// Whether approve/deny are supported for this agent.
    pub can_answer: bool,
}

impl From<&AgentPane> for AgentView {
    fn from(p: &AgentPane) -> Self {
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
            can_answer: actions::supports_prompt_answers(&p.agent),
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
    Interact {
        pane: u32,
        on: bool,
    },
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
