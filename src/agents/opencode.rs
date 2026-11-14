//! OpenCode. Derived from packages/tui/src/routes/session/permission.tsx and form.tsx at
//! f0089856c07eded13396a0a0d05f19f1718be7b9 (github.com/anomalyco/opencode); not checked against a
//! real screen.

use super::{AgentProfile, Keystrokes, prompt_tail};

pub struct OpenCode;

impl AgentProfile for OpenCode {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn verified(&self) -> bool {
        false
    }

    /// "△ Permission required" above one line of buttons: "Allow once  [Always allow]  Reject".
    /// Forms from the question tool say "esc dismiss", "Type your own answer" or "Field N of M".
    fn is_permission_prompt(&self, screen: &str) -> bool {
        let tail = prompt_tail(screen);
        let header = tail.iter().any(|l| l.contains("Permission required"));
        let buttons = tail
            .iter()
            .any(|l| l.contains("Allow once") && l.contains("Reject"));
        let is_form = tail.iter().any(|l| {
            l.contains("esc dismiss")
                || l.contains("Type your own answer")
                || (l.starts_with("Field ") && l.contains(" of "))
        });
        header && buttons && !is_form
    }

    /// No digit shortcuts: Enter confirms the selected button, which starts on "Allow once".
    /// If the user moved the selection in the pane, Enter picks that button instead; plain text
    /// doesn't show the selection (it's only a colour).
    fn approve(&self) -> Keystrokes {
        Keystrokes::Key("enter")
    }

    /// Esc maps to Reject. In a sub-session it opens a "Reject permission" feedback step, which
    /// stays `waiting` and is answered with reply or interact.
    fn deny(&self) -> Keystrokes {
        Keystrokes::Key("escape")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELL: &str = include_str!("../../tests/fixtures/screens/opencode-shell.txt");
    const EDIT: &str = include_str!("../../tests/fixtures/screens/opencode-edit.txt");
    const FORM: &str = include_str!("../../tests/fixtures/screens/opencode-form.txt");
    const CLAUDE: &str = include_str!("../../tests/fixtures/screens/claude-permission.txt");

    #[test]
    fn recognises_shell_and_edit_permissions() {
        assert!(OpenCode.is_permission_prompt(SHELL));
        assert!(OpenCode.is_permission_prompt(EDIT));
    }

    #[test]
    fn ignores_forms_and_other_agents() {
        assert!(!OpenCode.is_permission_prompt(FORM));
        assert!(!OpenCode.is_permission_prompt(CLAUDE));
    }

    #[test]
    fn approves_with_enter_and_denies_with_escape() {
        assert_eq!(OpenCode.approve(), Keystrokes::Key("enter"));
        assert_eq!(OpenCode.deny(), Keystrokes::Key("escape"));
    }
}
