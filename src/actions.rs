//! The only code path that types into panes.

use crate::fleet::Fleet;
use crate::tty7::Tty7Cli;
use crate::tty7::types::Status;

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
    NotReady {
        status: Option<Status>,
    },
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
        ("claude", true) => Some(Keystrokes::Text {
            text: "1".into(),
            enter: false,
        }),
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
        Action::Reply { text, .. } => Some(Keystrokes::Text {
            text: text.clone(),
            enter: true,
        }),
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
    async fn deny_presses_escape() {
        let cli = tty7_reports(54, "Claude", "waiting");
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        let outcome = execute(&cli, &fleet, &Action::Deny { pane: 54 }).await;
        assert_eq!(outcome, Outcome::Sent);
        assert_eq!(
            cli.sent(),
            vec![Sent::Keys {
                pane: 54,
                keys: vec!["escape".into()]
            }]
        );
    }

    #[tokio::test]
    async fn approve_is_dropped_when_agent_moved_on() {
        let cli = tty7_reports(54, "Claude", "working");
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        let outcome = execute(&cli, &fleet, &Action::Approve { pane: 54 }).await;
        assert_eq!(
            outcome,
            Outcome::NotReady {
                status: Some(Status::Working)
            }
        );
        assert!(cli.sent().is_empty());
    }

    #[tokio::test]
    async fn reply_is_refused_while_a_prompt_is_open() {
        let cli = tty7_reports(54, "Claude", "waiting");
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        let action = Action::Reply {
            pane: 54,
            text: "hello".into(),
        };
        assert_eq!(
            execute(&cli, &fleet, &action).await,
            Outcome::NotReady {
                status: Some(Status::Waiting)
            }
        );
        assert!(cli.sent().is_empty());
    }

    #[tokio::test]
    async fn reply_to_a_finished_agent_types_text_and_enter() {
        let cli = tty7_reports(54, "Claude", "done");
        let fleet = fleet_with(&[(54, "Claude", Status::Done, 0)]);
        let action = Action::Reply {
            pane: 54,
            text: "next: run the tests".into(),
        };
        assert_eq!(execute(&cli, &fleet, &action).await, Outcome::Sent);
        assert_eq!(
            cli.sent(),
            vec![Sent::Text {
                pane: 54,
                text: "next: run the tests".into(),
                enter: true
            }]
        );
    }

    #[tokio::test]
    async fn unknown_pane_is_rejected_without_asking_tty7() {
        let cli = FakeCli::default();
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        assert_eq!(
            execute(&cli, &fleet, &Action::Approve { pane: 7 }).await,
            Outcome::UnknownPane
        );
        assert!(cli.sent().is_empty());
    }

    #[tokio::test]
    async fn approve_is_unsupported_for_agents_without_a_key_table() {
        let cli = tty7_reports(60, "Codex", "waiting");
        let fleet = fleet_with(&[(60, "Codex", Status::Waiting, 0)]);
        assert_eq!(
            execute(&cli, &fleet, &Action::Approve { pane: 60 }).await,
            Outcome::Unsupported
        );
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
