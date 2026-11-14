//! Gemini CLI. Derived from packages/cli/src/ui (ToolConfirmationMessage.tsx,
//! useSelectionList.ts) at 4ed06ed0ddc523a6d356a4ad35ddfea331041888; not checked against a real
//! screen.

use super::{AgentProfile, Keystrokes, prompt_tail};

pub struct Gemini;

impl AgentProfile for Gemini {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn verified(&self) -> bool {
        false
    }

    /// Tool confirmations start with "1. Allow once" and end with "No, suggest changes (esc)".
    /// Questions and plan approval have an "Enter to select ·" footer, confirmations none.
    fn is_permission_prompt(&self, screen: &str) -> bool {
        let tail = prompt_tail(screen);
        let allow_once = tail.iter().any(|l| l.starts_with("1. Allow once"));
        let suggest = tail.iter().any(|l| l.contains("No, suggest changes (esc)"));
        let is_question = tail.iter().any(|l| l.contains("Enter to select ·"));
        allow_once && suggest && !is_question
    }

    /// "Allow once" is always option 1, and a digit selects at once with fewer than 10 options.
    /// Never Enter: with `security.autoAddToPolicyByDefault` the default is "all future sessions".
    fn approve(&self) -> Keystrokes {
        Keystrokes::Text {
            text: "1".into(),
            enter: false,
        }
    }

    /// Cancels the tool and the turn, no feedback form.
    fn deny(&self) -> Keystrokes {
        Keystrokes::Key("escape")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELL: &str = include_str!("../../tests/fixtures/screens/gemini-shell.txt");
    const EDIT: &str = include_str!("../../tests/fixtures/screens/gemini-edit.txt");
    const QUESTION: &str = include_str!("../../tests/fixtures/screens/gemini-question.txt");
    const PLAN: &str = include_str!("../../tests/fixtures/screens/gemini-plan.txt");
    const CLAUDE: &str = include_str!("../../tests/fixtures/screens/claude-permission.txt");

    #[test]
    fn recognises_shell_and_edit_confirmations() {
        assert!(Gemini.is_permission_prompt(SHELL));
        assert!(Gemini.is_permission_prompt(EDIT));
    }

    #[test]
    fn ignores_questions_plans_and_other_agents() {
        assert!(!Gemini.is_permission_prompt(QUESTION));
        assert!(!Gemini.is_permission_prompt(PLAN));
        assert!(!Gemini.is_permission_prompt(CLAUDE));
        assert!(!Gemini.is_permission_prompt(
            "Modify in progress: Save and close external editor to continue"
        ));
    }

    #[test]
    fn approves_with_1_and_denies_with_escape() {
        assert_eq!(
            Gemini.approve(),
            Keystrokes::Text {
                text: "1".into(),
                enter: false
            }
        );
        assert_eq!(Gemini.deny(), Keystrokes::Key("escape"));
    }
}
