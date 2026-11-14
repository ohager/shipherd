//! Claude Code, verified 2026-11-13 against real screens.

use super::{AgentProfile, BackgroundTask, Keystrokes, background, prompt_tail};

pub struct Claude;

impl AgentProfile for Claude {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn verified(&self) -> bool {
        true
    }

    /// Permission prompts ask "Do you want to …?" with "1. Yes" and end in "Tab to amend".
    /// Questions can offer "1. Yes" too, but they navigate with "Tab/Arrow keys".
    fn is_permission_prompt(&self, screen: &str) -> bool {
        let tail = prompt_tail(screen);
        let asks = tail.iter().any(|l| l.starts_with("Do you want to"));
        let offers_yes = tail.iter().any(|l| l.starts_with("1. Yes"));
        let is_question = tail
            .iter()
            .any(|l| l.contains("Tab/Arrow keys to navigate"));
        asks && offers_yes && !is_question
    }

    /// "1" is always "Yes".
    fn approve(&self) -> Keystrokes {
        Keystrokes::Text {
            text: "1".into(),
            enter: false,
        }
    }

    /// "No" moves between options 3 and 4, so deny uses Esc ("Esc to cancel").
    fn deny(&self) -> Keystrokes {
        Keystrokes::Key("escape")
    }

    fn background_work(&self, session_id: &str, now_ms: u64) -> Vec<BackgroundTask> {
        match background::claude_config_dir() {
            Some(root) => background::scan(&root, session_id, now_ms),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERMISSION: &str = include_str!("../../tests/fixtures/screens/claude-permission.txt");
    const QUESTION: &str = include_str!("../../tests/fixtures/screens/claude-question.txt");
    const YES_NO_QUESTION: &str =
        include_str!("../../tests/fixtures/screens/claude-yes-no-question.txt");

    #[test]
    fn tells_permission_prompts_from_questions() {
        assert!(Claude.is_permission_prompt(PERMISSION));
        assert!(!Claude.is_permission_prompt(QUESTION));
        assert!(!Claude.is_permission_prompt(YES_NO_QUESTION));
        assert!(!Claude.is_permission_prompt("❯ \n"));
    }

    #[test]
    fn approves_with_1_and_denies_with_escape() {
        assert_eq!(
            Claude.approve(),
            Keystrokes::Text {
                text: "1".into(),
                enter: false
            }
        );
        assert_eq!(Claude.deny(), Keystrokes::Key("escape"));
    }
}
