//! The only code path that types into panes.

use serde::Deserialize;

use crate::agents::{self, Keystrokes};
use crate::fleet::Fleet;
use crate::tty7::Tty7Cli;
use crate::tty7::types::Status;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Approve {
        pane: u32,
    },
    Deny {
        pane: u32,
    },
    Reply {
        pane: u32,
        text: String,
    },
    /// Interact mode: raw keystrokes, no status check (the user is driving).
    Keys {
        pane: u32,
        keys: Vec<KeyInput>,
    },
    /// Hangs up the pane's shell, ending the agent session. The views confirm before sending it.
    Close {
        pane: u32,
    },
}

impl Action {
    pub fn pane(&self) -> u32 {
        match self {
            Action::Approve { pane }
            | Action::Deny { pane }
            | Action::Reply { pane, .. }
            | Action::Keys { pane, .. }
            | Action::Close { pane } => *pane,
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
    /// The request itself was invalid (unknown key name, control characters in text).
    Rejected(String),
    /// Approve/deny would have answered a question (typing "1" picks its first option).
    NotPermissionPrompt,
    Failed(String),
}

impl Outcome {
    /// One-line explanation for the views; `None` when there's nothing worth saying.
    pub fn message(&self) -> Option<String> {
        match self {
            Outcome::Sent => None,
            Outcome::NotReady {
                status: Some(status),
            } => Some(format!("agent moved on (now {})", status.label())),
            Outcome::NotReady { status: None } => Some("agent is gone".into()),
            Outcome::UnknownPane => Some("that pane is gone".into()),
            Outcome::Unsupported => Some("not supported for this agent".into()),
            Outcome::Rejected(reason) => Some(format!("rejected: {reason}")),
            Outcome::NotPermissionPrompt => Some(
                "that's a question, not a permission prompt; answer it with Interact or in its pane"
                    .into(),
            ),
            Outcome::Failed(err) => Some(format!("failed: {err}")),
        }
    }
}

/// One keystroke group from the browser: literal text, or a key name tty7 understands.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum KeyInput {
    Text(String),
    Key(String),
}

const NAMED_KEYS: &[&str] = &[
    "enter",
    "escape",
    "tab",
    "backtab",
    "space",
    "backspace",
    "delete",
    "up",
    "down",
    "right",
    "left",
    "home",
    "end",
    "pageup",
    "pagedown",
];
const MAX_TEXT: usize = 4096;

/// Control characters must arrive as named keys, so text can't smuggle in escape sequences.
fn validate(keys: &[KeyInput]) -> Result<(), String> {
    if keys.is_empty() {
        return Err("no keys".into());
    }
    for key in keys {
        match key {
            KeyInput::Text(text) => {
                if text.is_empty() || text.len() > MAX_TEXT || text.chars().any(char::is_control) {
                    return Err("invalid text".into());
                }
            }
            KeyInput::Key(name) => {
                if !(NAMED_KEYS.contains(&name.as_str()) || is_modified_key(name)) {
                    return Err(format!("unknown key {name}"));
                }
            }
        }
    }
    Ok(())
}

/// `C-x` (Ctrl) or `M-x` (Alt) with a single ASCII letter or digit.
fn is_modified_key(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(
        (chars.next(), chars.next(), chars.next(), chars.next()),
        (Some('C' | 'M'), Some('-'), Some(c), None) if c.is_ascii_alphanumeric()
    )
}

async fn send_interactive(cli: &dyn Tty7Cli, pane: u32, keys: &[KeyInput]) -> Outcome {
    if let Err(reason) = validate(keys) {
        return Outcome::Rejected(reason);
    }
    for key in keys {
        let result = match key {
            KeyInput::Text(text) => cli.send_text(pane, text, false).await,
            KeyInput::Key(name) => cli.send_keys(pane, &[name.as_str()]).await,
        };
        if let Err(err) = result {
            return Outcome::Failed(err.to_string());
        }
    }
    Outcome::Sent
}

const PROMPT_CAPTURE_LINES: u32 = 40;

/// Free text during `waiting` would land in a selection menu, where typed digits pick options.
fn allowed_statuses(action: &Action) -> &'static [Status] {
    match action {
        Action::Approve { .. } | Action::Deny { .. } => &[Status::Waiting],
        Action::Reply { .. } => &[Status::Done, Status::Idle],
        Action::Keys { .. } | Action::Close { .. } => &[],
    }
}

pub async fn execute(cli: &dyn Tty7Cli, fleet: &Fleet, action: &Action) -> Outcome {
    let pane = action.pane();
    let Some(known) = fleet.panes.get(&pane) else {
        return Outcome::UnknownPane;
    };
    let profile = agents::profile_for(&known.agent);
    let keystrokes = match action {
        Action::Approve { .. } => profile.map(|p| p.approve()),
        Action::Deny { .. } => profile.map(|p| p.deny()),
        Action::Reply { text, .. } => Some(Keystrokes::Text {
            text: text.clone(),
            enter: true,
        }),
        Action::Keys { keys, .. } => return send_interactive(cli, pane, keys).await,
        Action::Close { .. } => {
            return match cli.close_pane(pane).await {
                Ok(()) => Outcome::Sent,
                Err(err) => Outcome::Failed(err.to_string()),
            };
        }
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
    // tty7 reports questions and permission prompts both as `waiting`; only the screen tells them apart.
    if matches!(action, Action::Approve { .. } | Action::Deny { .. }) {
        match cli.capture_plain(pane, PROMPT_CAPTURE_LINES).await {
            Ok(screen) if profile.is_some_and(|p| p.is_permission_prompt(&screen)) => {}
            Ok(_) => return Outcome::NotPermissionPrompt,
            Err(err) => return Outcome::Failed(err.to_string()),
        }
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

    const PERMISSION: &str = include_str!("../tests/fixtures/screens/claude-permission.txt");
    const QUESTION: &str = include_str!("../tests/fixtures/screens/claude-question.txt");

    /// The pane shows a permission prompt unless a test replaces its screen.
    fn tty7_reports(pane: u32, agent: &str, status: &str) -> FakeCli {
        let cli = FakeCli::with_agents(&format!(
            r#"{{"agents":[{{"pane_id":{pane},"agent":"{agent}","state":{{"status":"{status}","cwd":"/tmp/x"}}}}]}}"#
        ));
        cli.captures.lock().unwrap().insert(pane, PERMISSION.into());
        cli
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
    async fn approve_is_unsupported_for_agents_without_a_profile() {
        let cli = tty7_reports(60, "Aider", "waiting");
        let fleet = fleet_with(&[(60, "Aider", Status::Waiting, 0)]);
        assert_eq!(
            execute(&cli, &fleet, &Action::Approve { pane: 60 }).await,
            Outcome::Unsupported
        );
    }

    #[tokio::test]
    async fn approve_on_codex_types_y_after_checking_its_screen() {
        let cli = tty7_reports(60, "Codex", "waiting");
        cli.captures.lock().unwrap().insert(
            60,
            include_str!("../tests/fixtures/screens/codex-exec.txt").into(),
        );
        let fleet = fleet_with(&[(60, "Codex", Status::Waiting, 0)]);
        assert_eq!(
            execute(&cli, &fleet, &Action::Approve { pane: 60 }).await,
            Outcome::Sent
        );
        assert_eq!(
            cli.sent(),
            vec![Sent::Text {
                pane: 60,
                text: "y".into(),
                enter: false
            }]
        );
    }

    #[tokio::test]
    async fn codex_refuses_a_claude_screen() {
        // tty7_reports puts Claude's permission prompt on the pane.
        let cli = tty7_reports(60, "Codex", "waiting");
        let fleet = fleet_with(&[(60, "Codex", Status::Waiting, 0)]);
        assert_eq!(
            execute(&cli, &fleet, &Action::Approve { pane: 60 }).await,
            Outcome::NotPermissionPrompt
        );
        assert!(cli.sent().is_empty());
    }

    #[tokio::test]
    async fn tty7_failure_is_reported() {
        let cli = FakeCli::default();
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        let outcome = execute(&cli, &fleet, &Action::Approve { pane: 54 }).await;
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    }

    #[tokio::test]
    async fn keys_are_sent_in_order_without_a_status_check() {
        let cli = tty7_reports(54, "Claude", "working");
        let fleet = fleet_with(&[(54, "Claude", Status::Working, 0)]);
        let action = Action::Keys {
            pane: 54,
            keys: vec![
                KeyInput::Text("ls".into()),
                KeyInput::Key("enter".into()),
                KeyInput::Key("C-c".into()),
            ],
        };
        assert_eq!(execute(&cli, &fleet, &action).await, Outcome::Sent);
        assert_eq!(
            cli.sent(),
            vec![
                Sent::Text {
                    pane: 54,
                    text: "ls".into(),
                    enter: false
                },
                Sent::Keys {
                    pane: 54,
                    keys: vec!["enter".into()]
                },
                Sent::Keys {
                    pane: 54,
                    keys: vec!["C-c".into()]
                },
            ]
        );
    }

    #[tokio::test]
    async fn invalid_keys_are_rejected_before_anything_is_sent() {
        let cli = tty7_reports(54, "Claude", "working");
        let fleet = fleet_with(&[(54, "Claude", Status::Working, 0)]);
        for keys in [
            vec![KeyInput::Text("ok".into()), KeyInput::Key("rm -rf".into())],
            vec![KeyInput::Text("line\nbreak".into())],
            vec![KeyInput::Key("C-ctrl".into())],
            vec![],
        ] {
            let outcome = execute(&cli, &fleet, &Action::Keys { pane: 54, keys }).await;
            assert!(matches!(outcome, Outcome::Rejected(_)), "{outcome:?}");
        }
        assert!(cli.sent().is_empty());
    }

    #[test]
    fn key_input_uses_the_browser_json_shape() {
        let keys: Vec<KeyInput> = serde_json::from_str(
            r#"[{"kind":"text","value":"hi"},{"kind":"key","value":"enter"}]"#,
        )
        .unwrap();
        assert_eq!(
            keys,
            vec![KeyInput::Text("hi".into()), KeyInput::Key("enter".into())]
        );
    }

    #[test]
    fn outcomes_explain_themselves() {
        assert_eq!(Outcome::Sent.message(), None);
        assert_eq!(
            Outcome::NotReady {
                status: Some(Status::Working)
            }
            .message()
            .as_deref(),
            Some("agent moved on (now working)")
        );
        assert_eq!(
            Outcome::NotReady { status: None }.message().as_deref(),
            Some("agent is gone")
        );
        assert_eq!(
            Outcome::Rejected("unknown key x".into())
                .message()
                .as_deref(),
            Some("rejected: unknown key x")
        );
    }

    #[tokio::test]
    async fn approve_and_deny_refuse_to_answer_a_question() {
        let cli = tty7_reports(54, "Claude", "waiting");
        cli.captures.lock().unwrap().insert(54, QUESTION.into());
        let fleet = fleet_with(&[(54, "Claude", Status::Waiting, 0)]);
        for action in [Action::Approve { pane: 54 }, Action::Deny { pane: 54 }] {
            assert_eq!(
                execute(&cli, &fleet, &action).await,
                Outcome::NotPermissionPrompt
            );
        }
        assert!(cli.sent().is_empty());
        assert_eq!(
            Outcome::NotPermissionPrompt.message().as_deref(),
            Some(
                "that's a question, not a permission prompt; answer it with Interact or in its pane"
            )
        );
    }

    #[tokio::test]
    async fn close_closes_exactly_that_pane_whatever_its_status() {
        let cli = tty7_reports(54, "Claude", "working");
        let fleet = fleet_with(&[(54, "Claude", Status::Working, 0)]);
        assert_eq!(
            execute(&cli, &fleet, &Action::Close { pane: 54 }).await,
            Outcome::Sent
        );
        assert_eq!(cli.sent(), vec![Sent::Close { pane: 54 }]);
    }

    #[tokio::test]
    async fn close_only_works_on_agent_panes() {
        // shipherd's own pane and plain shells are never in the fleet.
        let cli = tty7_reports(54, "Claude", "working");
        let fleet = fleet_with(&[(54, "Claude", Status::Working, 0)]);
        assert_eq!(
            execute(&cli, &fleet, &Action::Close { pane: 61 }).await,
            Outcome::UnknownPane
        );
        assert!(cli.sent().is_empty());
    }
}
