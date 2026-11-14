//! What shipherd knows about each kind of agent: how its permission prompts look, which keys
//! answer them, and what background work it hides from tty7's status hooks.

pub mod background;
mod claude;

/// What to type into a pane to answer a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keystrokes {
    Text { text: String, enter: bool },
    Key(&'static str),
}

/// A subagent still running after its parent's turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundTask {
    /// The subagent type, e.g. "researcher".
    pub kind: String,
    pub description: String,
    /// When it started (ms since the Unix epoch).
    pub since_ms: u64,
}

/// A pane whose agent may have background work, as the background worker needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundTarget {
    pub pane: u32,
    pub agent: String,
    pub session_id: String,
}

pub trait AgentProfile: Sync {
    /// Lowercased first word of the name tty7 reports: "claude", "codex", "gemini", "opencode".
    fn name(&self) -> &'static str;
    /// Checked against real screens (true) or derived from the agent's source (false).
    fn verified(&self) -> bool;
    fn is_permission_prompt(&self, screen: &str) -> bool;
    fn approve(&self) -> Keystrokes;
    fn deny(&self) -> Keystrokes;
    /// Background work the status hooks don't report. Blocking file I/O: call off the core loop.
    fn background_work(&self, _session_id: &str, _now_ms: u64) -> Vec<BackgroundTask> {
        Vec::new()
    }
}

static PROFILES: &[&dyn AgentProfile] = &[&claude::Claude];

/// Matches the first word of tty7's agent name case-insensitively ("Gemini CLI" → gemini).
pub fn profile_for(agent: &str) -> Option<&'static dyn AgentProfile> {
    let first = agent.split_whitespace().next()?.to_lowercase();
    PROFILES.iter().copied().find(|p| p.name() == first)
}

/// A prompt occupies the bottom of the screen; older output above it may quote anything.
const PROMPT_TAIL_LINES: usize = 20;
/// Box borders and selection markers agents draw around and in front of options.
const BORDERS: &[char] = &['│', '┃'];
const MARKERS: &[char] = &['❯', '›', '●', '▶'];

/// The last non-empty lines of a screen, without box borders, selection markers or padding.
pub(crate) fn prompt_tail(screen: &str) -> Vec<&str> {
    let lines: Vec<&str> = screen
        .lines()
        .map(|l| {
            l.trim()
                .trim_matches(BORDERS)
                .trim()
                .trim_start_matches(MARKERS)
                .trim()
        })
        .filter(|l| !l.is_empty())
        .collect();
    lines[lines.len().saturating_sub(PROMPT_TAIL_LINES)..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_match_the_first_word_of_the_agent_name() {
        assert_eq!(profile_for("Claude").map(|p| p.name()), Some("claude"));
        assert_eq!(profile_for("claude code").map(|p| p.name()), Some("claude"));
        assert!(profile_for("Aider").is_none());
        assert!(profile_for("").is_none());
    }

    #[test]
    fn the_prompt_tail_drops_borders_markers_and_old_lines() {
        let mut screen = String::new();
        for i in 0..30 {
            screen += &format!("old {i}\n");
        }
        screen += "│ ● 1. Allow once   │\n\n  › 2. No\n";
        let tail = prompt_tail(&screen);
        assert_eq!(tail.len(), PROMPT_TAIL_LINES);
        assert_eq!(tail[tail.len() - 2..], ["1. Allow once", "2. No"]);
    }
}
