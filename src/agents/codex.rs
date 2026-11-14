//! OpenAI Codex CLI. Derived from codex-rs/tui (approval_overlay.rs, keymap.rs) at
//! 32cec881b72360957d30ff48f94e038945603818; not checked against a real screen.

use super::{AgentProfile, Keystrokes, prompt_tail};

pub struct Codex;

impl AgentProfile for Codex {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn verified(&self) -> bool {
        false
    }

    /// Titles "Would you like to …?", "Do you want to approve network access …?" or
    /// "<server> needs your approval.", option 1 "Yes, proceed" / "Yes, just this once", and the
    /// footer "Press enter to confirm or esc to cancel". Questions show "Question N/M" and
    /// "esc to interrupt".
    fn is_permission_prompt(&self, screen: &str) -> bool {
        let tail = prompt_tail(screen);
        let asks = tail.iter().any(|l| {
            l.starts_with("Would you like to ")
                || l.starts_with("Do you want to approve network access")
                || l.ends_with(" needs your approval.")
        });
        let offers_yes = tail
            .iter()
            .any(|l| l.starts_with("1. Yes, proceed") || l.starts_with("1. Yes, just this once"));
        let footer = tail
            .iter()
            .any(|l| l.contains("Press enter to confirm or esc to cancel"));
        let is_question = tail.iter().any(|l| {
            (l.starts_with("Question ") && l.contains('/'))
                || l.contains("esc to interrupt")
                || l.starts_with("Implement this plan?")
        });
        asks && offers_yes && footer && !is_question
    }

    /// The hotkey shown on option 1; it stays right when the options in between change.
    fn approve(&self) -> Keystrokes {
        Keystrokes::Text {
            text: "y".into(),
            enter: false,
        }
    }

    /// "No, and tell Codex what to do differently (esc)": aborts the turn, no feedback dialog.
    fn deny(&self) -> Keystrokes {
        Keystrokes::Key("escape")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXEC: &str = include_str!("../../tests/fixtures/screens/codex-exec.txt");
    const PATCH: &str = include_str!("../../tests/fixtures/screens/codex-patch.txt");
    const QUESTION: &str = include_str!("../../tests/fixtures/screens/codex-question.txt");
    const PLAN: &str = include_str!("../../tests/fixtures/screens/codex-plan.txt");
    const CLAUDE: &str = include_str!("../../tests/fixtures/screens/claude-permission.txt");

    #[test]
    fn recognises_command_and_edit_approvals() {
        assert!(Codex.is_permission_prompt(EXEC));
        assert!(Codex.is_permission_prompt(PATCH));
    }

    #[test]
    fn ignores_questions_plans_and_other_agents() {
        assert!(!Codex.is_permission_prompt(QUESTION));
        assert!(!Codex.is_permission_prompt(PLAN));
        assert!(!Codex.is_permission_prompt(CLAUDE));
        assert!(!Codex.is_permission_prompt(""));
    }

    #[test]
    fn approves_with_y_and_denies_with_escape() {
        assert_eq!(
            Codex.approve(),
            Keystrokes::Text {
                text: "y".into(),
                enter: false
            }
        );
        assert_eq!(Codex.deny(), Keystrokes::Key("escape"));
    }
}
