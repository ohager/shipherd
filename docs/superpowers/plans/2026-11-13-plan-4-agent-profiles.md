# Agent profiles and background agents Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Approve/deny for Codex, Gemini CLI and OpenCode behind one `AgentProfile` trait, and Claude Code's background agents shown in both views (spec: `docs/superpowers/specs/2026-11-13-agent-profiles-design.md`).

**Architecture:** A new `src/agents/` module holds one strategy per agent (`claude.rs`, `codex.rs`, `gemini.rs`, `opencode.rs`) behind `trait AgentProfile`, looked up with `profile_for(agent)`. `actions.rs`, `web/session.rs`, `web/protocol.rs` and the TUI ask the profile instead of hard-coding Claude. Background agents come from Claude Code's subagent transcripts (`agents/background.rs`), scanned by a new background worker in `runtime.rs`; the fleet stores them per pane and sorts such panes as working.

**Tech Stack:** Rust 2024 (tokio, serde_json, ratatui, axum), plain JS with `node --test`, `tempfile` for filesystem tests.

**Checks after every task:** `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`; for tasks touching `src/web/assets/`, also `node --test tests/web/*.test.js`.

**Deviations from the spec (decided while planning, update the spec in Task 12):**
- The transcript directory is found by searching `<config>/projects/*/<session_id>/subagents` instead of rebuilding the cwd slug. Session ids are UUIDs, so the match is unique, and it avoids guessing Claude Code's slug rules (`/.claude/` becomes `--claude`, other characters are unknown). `SessionRef` shrinks to the session id string.
- A background agent's `since` is the modification time of its `.meta.json` (written once when it starts), so the scan is stateless. Transcripts older than 30 minutes are skipped after a `stat`, so only recent ones are read; no mtime cache.

---

## File structure

| File | Responsibility |
|---|---|
| `src/agents/mod.rs` (new) | `AgentProfile`, `profile_for`, `Keystrokes`, `BackgroundTask`, `BackgroundTarget`, `prompt_tail`, `background_for` |
| `src/agents/claude.rs` (new) | Claude Code prompt detection and keys (moved from `actions.rs`), `background_work` via `background.rs` |
| `src/agents/codex.rs`, `gemini.rs`, `opencode.rs` (new) | One profile each, unverified |
| `src/agents/background.rs` (new) | Reads Claude Code subagent transcripts: `claude_config_dir`, `scan` |
| `src/actions.rs` | Uses the pane's profile; loses `prompt_keys`, `is_permission_prompt`, `supports_prompt_answers`, `Keystrokes` |
| `src/tty7/types.rs` | `AgentEntryState.session_id` |
| `src/fleet/mod.rs` | `AgentPane.session_id`, `AgentPane.background`, `AgentPane::board_status`, `Fleet::set_background`, ordering by board status |
| `src/runtime.rs` | `background_worker`, background targets, applying results |
| `src/tui/app.rs`, `src/tui/mod.rs`, `src/tui/view.rs` | Profile lookup, "unverified" note, `⟳` marker, background lines in the preview, sections by board status |
| `src/web/protocol.rs`, `src/web/session.rs` | `answer_verified`, `group`, `background` in `AgentView`; per-agent prompt check for previews |
| `src/web/assets/logic.js`, `app.js`, `app.css` | Columns by `group`, `⟳ N background` pill, focus-view background line, unverified marker |
| `tests/fixtures/screens/*.txt` | Prompt screens per agent (Claude's moved here) |
| `tests/web/logic.test.js` | Tests for `backgroundLines` |
| `README.md` | "Supported agents" table |

---

### Task 1: `agents` module with the Claude profile

Moves Claude's prompt knowledge out of `actions.rs` without changing behaviour yet; `actions.rs` switches over in Task 2.

**Files:**
- Create: `src/agents/mod.rs`, `src/agents/claude.rs`
- Modify: `src/lib.rs`
- Move: `tests/fixtures/screen-permission.txt` → `tests/fixtures/screens/claude-permission.txt`, `screen-question.txt` → `screens/claude-question.txt`, `screen-yes-no-question.txt` → `screens/claude-yes-no-question.txt`
- Modify (fixture paths only): `src/actions.rs:271-273`, `src/web/session.rs:327`, `src/tty7/fake.rs:58`

- [ ] **Step 1: Move the fixtures**

```bash
mkdir -p tests/fixtures/screens
git mv tests/fixtures/screen-permission.txt tests/fixtures/screens/claude-permission.txt
git mv tests/fixtures/screen-question.txt tests/fixtures/screens/claude-question.txt
git mv tests/fixtures/screen-yes-no-question.txt tests/fixtures/screens/claude-yes-no-question.txt
```

Update the three `include_str!` paths in `src/actions.rs` tests to `../tests/fixtures/screens/claude-permission.txt` etc., in `src/web/session.rs:327` and `src/tty7/fake.rs:58` to `../../tests/fixtures/screens/claude-permission.txt`.

- [ ] **Step 2: Write `src/agents/mod.rs` with the failing tests**

```rust
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
```

- [ ] **Step 3: Write `src/agents/claude.rs`**

```rust
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
        let is_question = tail.iter().any(|l| l.contains("Tab/Arrow keys to navigate"));
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
```

`background.rs` is written in Task 7. For now create it with only the two functions Claude calls:

```rust
//! Claude Code's subagent transcripts (filled in by Task 7).

use std::path::{Path, PathBuf};

use super::BackgroundTask;

pub fn claude_config_dir() -> Option<PathBuf> {
    None
}

pub fn scan(_root: &Path, _session_id: &str, _now_ms: u64) -> Vec<BackgroundTask> {
    Vec::new()
}
```

Add `pub mod agents;` to `src/lib.rs` (alphabetical, before `pub mod actions;` is fine: `actions`, `agents`, …).

- [ ] **Step 4: Run the tests**

Run: `cargo test agents`
Expected: 4 tests pass (`profiles_match…`, `the_prompt_tail…`, `tells_permission_prompts…`, `approves_with_1…`). `cargo test` overall passes; `actions.rs` still has its own copies, which Task 2 removes.

- [ ] **Step 5: Commit**

```bash
git add -A src/agents src/lib.rs tests/fixtures src/actions.rs src/web/session.rs src/tty7/fake.rs
git commit -m "Add agent profiles with Claude Code's prompt detection and keys"
```

### Task 2: Callers ask the agent's profile

**Files:**
- Modify: `src/actions.rs` (remove `Keystrokes`, `prompt_keys`, `PROMPT_TAIL_LINES`, `is_permission_prompt`, `supports_prompt_answers` and the test `tells_permission_prompts_from_questions`; change `execute`)
- Modify: `src/web/session.rs:102-112`, `src/web/protocol.rs:24-44`, `src/tui/app.rs:155`

- [ ] **Step 1: Change the existing test for unsupported agents to the new names**

In `src/actions.rs` tests, replace `approve_is_unsupported_for_agents_without_a_key_table` with:

```rust
    #[tokio::test]
    async fn approve_is_unsupported_for_agents_without_a_profile() {
        let cli = tty7_reports(60, "Aider", "waiting");
        let fleet = fleet_with(&[(60, "Aider", Status::Waiting, 0)]);
        assert_eq!(
            execute(&cli, &fleet, &Action::Approve { pane: 60 }).await,
            Outcome::Unsupported
        );
    }
```

And in `src/web/protocol.rs` tests, add to `agent_view_from_pane` (after `assert!(view.can_answer);`):

```rust
        assert!(view.answer_verified);
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test`
Expected: compile error, `no field answer_verified on type AgentView`.

- [ ] **Step 3: Switch `execute` to the profile**

In `src/actions.rs`: `use crate::agents::{self, Keystrokes};`, delete the moved items, and change the start of `execute` and the screen check:

```rust
pub async fn execute(cli: &dyn Tty7Cli, fleet: &Fleet, action: &Action) -> Outcome {
    let pane = action.pane();
    let Some(known) = fleet.panes.get(&pane) else {
        return Outcome::UnknownPane;
    };
    let profile = agents::profile_for(&known.agent);
    let keystrokes = match action {
        Action::Approve { .. } => profile.map(|p| p.approve()),
        Action::Deny { .. } => profile.map(|p| p.deny()),
        // … Reply, Keys, Close unchanged …
    };
```

```rust
    if matches!(action, Action::Approve { .. } | Action::Deny { .. }) {
        match cli.capture_plain(pane, PROMPT_CAPTURE_LINES).await {
            Ok(screen) if profile.is_some_and(|p| p.is_permission_prompt(&screen)) => {}
            Ok(_) => return Outcome::NotPermissionPrompt,
            Err(err) => return Outcome::Failed(err.to_string()),
        }
    }
```

Keep `PROMPT_CAPTURE_LINES` in `actions.rs`.

- [ ] **Step 4: `web/protocol.rs`**

```rust
    /// Whether approve/deny are supported for this agent.
    pub can_answer: bool,
    /// False when the agent's prompt layout comes from its source, not from real screens.
    pub answer_verified: bool,
```

```rust
        let profile = agents::profile_for(&p.agent);
        AgentView {
            // … unchanged fields …
            can_answer: profile.is_some(),
            answer_verified: profile.is_some_and(|p| p.verified()),
        }
```

Replace `use crate::actions::{self, KeyInput};` with `use crate::actions::KeyInput;` and `use crate::agents;`.

- [ ] **Step 5: `web/session.rs` previews use the pane's profile**

```rust
        if let Some(pane) = self.selected
            && let Some(agent) = fleet.panes.get(&pane)
            && let Some(text) = agent.preview.as_ref()
            && self.last_preview.as_ref() != Some(text)
        {
            out.push(ServerMsg::Preview {
                pane,
                text: text.clone(),
                permission_prompt: agents::profile_for(&agent.agent)
                    .is_some_and(|p| p.is_permission_prompt(text)),
            });
            self.last_preview = Some(text.clone());
        }
```

Replace the `actions` import in `session.rs` if it's no longer used otherwise (keep it if `actions::Action` is used).

- [ ] **Step 6: `tui/app.rs`**

```rust
        if agents::profile_for(&pane.agent).is_none() {
```

with `use crate::agents;`. The test `y_on_an_unsupported_agent_explains_why` uses a Codex pane (pane 2 in `sample()`); change pane 2's agent in `sample()` to `"Aider"` and the expected flash to `"approve/deny isn't supported for aider yet; answer in its pane"`. (Codex becomes supported in Task 3.) Check `src/tui/view.rs` tests that render `sample`-like fleets with `"Codex"` (line ~326): those only check rendering and keep working.

- [ ] **Step 7: Run all checks**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add src/actions.rs src/web src/tui
git commit -m "Approve, deny and prompt checks go through the agent's profile"
```

### Task 3: Codex profile

**Files:**
- Create: `src/agents/codex.rs`, `tests/fixtures/screens/codex-exec.txt`, `codex-patch.txt`, `codex-question.txt`, `codex-plan.txt`
- Modify: `src/agents/mod.rs` (`mod codex;`, add to `PROFILES`), `src/actions.rs` tests

- [ ] **Step 1: Fixtures** (from `codex-rs/tui` snapshots and source at `32cec881b72360957d30ff48f94e038945603818`; unverified)

`tests/fixtures/screens/codex-exec.txt`:
```
# codex-rs/tui/src/chatwidget/snapshots/*approval_modal_exec.snap @ 32cec881 (unverified)

  Would you like to run the following command?

  Reason: this is a test reason such as one that would be produced by the model

  $ echo hello world

› 1. Yes, proceed (y)
  2. Yes, and don't ask again for commands that start with `echo hello world` (p)
  3. No, and tell Codex what to do differently (esc)

  Press enter to confirm or esc to cancel
```

`codex-patch.txt`:
```
# codex-rs/tui/src/chatwidget/snapshots/*approval_modal_patch.snap @ 32cec881 (unverified)

  Would you like to make the following edits?

  Description: The model wants to apply changes
  Destination: /tmp/project/README.md

› 1. Yes, proceed (y)
  2. Yes, and don't ask again for these files (a)
  3. No, and tell Codex what to do differently (esc)

  Press enter to confirm or esc to cancel
```

`codex-question.txt`:
```
# codex-rs/tui/src/bottom_pane/request_user_input snapshots @ 32cec881 (unverified)

  Question 1/1 (1 unanswered)
  Choose an option.

  › 1. Option 1           First choice.
    2. Option 2           Second choice.
    3. Option 3           Third choice.
    4. None of the above  Optionally, add details in notes (tab)

  tab to add notes | enter to submit answer | ←/→ to navigate questions | esc to interrupt
```

`codex-plan.txt`:
```
# codex-rs/tui/src/chatwidget/plan_implementation.rs @ 32cec881 (unverified)

  Implement this plan?

› 1. Yes, implement this plan          Switch to Default and start coding
  2. Yes, clear context and implement  Fresh thread with this plan
  3. No, stay in Plan mode             Continue planning with the model

  enter select · esc back
```

- [ ] **Step 2: Write `src/agents/codex.rs` with tests first**

```rust
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
```

- [ ] **Step 3: Run to see it fail, then register**

Run: `cargo test agents::codex` → fails to compile until `mod codex;` is added. Add to `src/agents/mod.rs`:

```rust
mod codex;
// …
static PROFILES: &[&dyn AgentProfile] = &[&claude::Claude, &codex::Codex];
```

Extend `profiles_match_the_first_word_of_the_agent_name`:

```rust
        assert_eq!(profile_for("Codex").map(|p| p.name()), Some("codex"));
```

- [ ] **Step 4: Action test with a Codex pane**

In `src/actions.rs` tests:

```rust
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
```

- [ ] **Step 5: Run all checks, commit**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` → pass.

```bash
git add src/agents src/actions.rs tests/fixtures/screens
git commit -m "Codex profile: approve with y, deny with Esc (unverified)"
```

### Task 4: Gemini CLI profile

**Files:**
- Create: `src/agents/gemini.rs`, `tests/fixtures/screens/gemini-shell.txt`, `gemini-edit.txt`, `gemini-question.txt`, `gemini-plan.txt`
- Modify: `src/agents/mod.rs`

- [ ] **Step 1: Fixtures** (from `packages/cli/src/ui` snapshots and source at `4ed06ed0ddc523a6d356a4ad35ddfea331041888`; unverified)

`gemini-shell.txt`:
```
# packages/cli/src/ui/components/__snapshots__/ToolConfirmationQueue.test.tsx.snap @ 4ed06ed0 (unverified)
╭──────────────────────────────────────────────────────────────╮
│ ? Shell  Executes a bash command                              │
│ ╭──────────────────────────────────────────────────────────╮ │
│ │ touch test.txt                                           │ │
│ ╰──────────────────────────────────────────────────────────╯ │
│ Allow execution of [Shell]?                                   │
│                                                               │
│ ● 1. Allow once                                               │
│   2. Allow for this session                                   │
│   3. No, suggest changes (esc)                                │
╰──────────────────────────────────────────────────────────────╯
```

`gemini-edit.txt`:
```
# packages/cli/src/ui/components/__snapshots__/ToolConfirmationFullFrame.test.tsx.snap @ 4ed06ed0 (unverified)
╭──────────────────────────────────────────────────────────────╮
│ ? Edit packages/cli/src/ui/InputPrompt.tsx: return kitty... => ... │
│ ╭─ diff ──────────────────────────────────────────────────╮  │
│ ╰─────────────────────────────────────────────────────────╯  │
│ Apply this change?                                            │
│                                                               │
│ ● 1. Allow once                                               │
│   2. Allow for this session                                   │
│   3. Allow for this file in all future sessions ~/.gemini/policies/auto-saved.toml │
│   4. Modify with external editor                              │
│   5. No, suggest changes (esc)                                │
╰──────────────────────────────────────────────────────────────╯
```

`gemini-question.txt`:
```
# packages/cli/src/ui/components ask_user dialog @ 4ed06ed0 (unverified)
╭─────────────────────────────────────────────────────╮
│ Answer Questions                                    │
│ Which database should the service use?              │
│ ● 1.  Option 1                                      │
│       Desc                                          │
│   2.  Enter a custom value                          │
│ Enter to select · ↑/↓ to navigate · Esc to cancel   │
╰─────────────────────────────────────────────────────╯
```

`gemini-plan.txt`:
```
# packages/cli/src/ui/components/ExitPlanModeDialog.tsx @ 4ed06ed0 (unverified)
╭──────────────────────────────────────────────────────────────────────────╮
│ Ready to start implementation?                                           │
│ ● 1.  Yes, automatically accept edits                                    │
│       Approves plan and allows tools to run automatically                │
│   2.  Yes, manually accept edits                                         │
│   3.  Type your feedback...                                              │
│ Enter to select · ↑/↓ to navigate · Ctrl+G to edit plan · Esc to cancel  │
╰──────────────────────────────────────────────────────────────────────────╯
```

- [ ] **Step 2: Write `src/agents/gemini.rs` with tests**

```rust
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
        let suggest = tail
            .iter()
            .any(|l| l.contains("No, suggest changes (esc)"));
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
        assert!(!Gemini.is_permission_prompt("Modify in progress: Save and close external editor to continue"));
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
```

- [ ] **Step 3: Register and extend the lookup test**

`mod gemini;`, `PROFILES` gains `&gemini::Gemini`; in `profiles_match_the_first_word_of_the_agent_name`:

```rust
        assert_eq!(profile_for("Gemini CLI").map(|p| p.name()), Some("gemini"));
        assert_eq!(profile_for("gemini").map(|p| p.name()), Some("gemini"));
```

- [ ] **Step 4: Run all checks, commit**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` → pass. Note the box lines: `prompt_tail` strips `│`, so `│ ● 1. Allow once        │` becomes `1. Allow once`.

```bash
git add src/agents tests/fixtures/screens
git commit -m "Gemini CLI profile: approve with 1, deny with Esc (unverified)"
```

### Task 5: OpenCode profile

**Files:**
- Create: `src/agents/opencode.rs`, `tests/fixtures/screens/opencode-shell.txt`, `opencode-edit.txt`, `opencode-form.txt`
- Modify: `src/agents/mod.rs`

- [ ] **Step 1: Fixtures** (reconstructed from `packages/tui/src/routes/session/permission.tsx` and `form.tsx` at `f0089856c07eded13396a0a0d05f19f1718be7b9`; unverified)

`opencode-shell.txt`:
```
# packages/tui/src/routes/session/permission.tsx @ f0089856 (reconstructed, unverified)
┃  △ Permission required
┃
┃   $ touch test.txt
┃
┃   Allow once  Always allow  Reject       ctrl+f fullscreen  ⇆ select  enter confirm
```

`opencode-edit.txt`:
```
# packages/tui/src/routes/session/permission.tsx @ f0089856 (reconstructed, unverified)
┃  △ Permission required
┃    → Edit src/foo.ts
┃   - const a = 1
┃   + const a = 2
┃   Allow once  Reject       ctrl+f fullscreen  ⇆ select  enter confirm
```

`opencode-form.txt`:
```
# packages/tui/src/routes/session/form.tsx @ f0089856 (reconstructed, unverified)
┃   Pick a deployment target
┃   Field 1 of 2 · 0/2 completed
┃   Where should this run? Allow once per region, or Reject the request.
┃   1. Option A
┃      description
┃   2. Type your own answer
┃   ⇆ tab  ↑↓ select  enter submit  esc dismiss
```

(The form's question deliberately contains "Allow once" and "Reject" to prove the exclusions matter.)

- [ ] **Step 2: Write `src/agents/opencode.rs` with tests**

```rust
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
```

- [ ] **Step 3: Register and extend the lookup test**

`mod opencode;`, `PROFILES` gains `&opencode::OpenCode`; in the lookup test:

```rust
        assert_eq!(profile_for("OpenCode").map(|p| p.name()), Some("opencode"));
```

- [ ] **Step 4: Run all checks, commit**

```bash
git add src/agents tests/fixtures/screens
git commit -m "OpenCode profile: approve with Enter, deny with Esc (unverified)"
```

### Task 6: Mark unverified approve/deny in the views

**Files:**
- Modify: `src/tui/app.rs` (new `sent_note`), `src/tui/mod.rs:69-72`
- Modify: `src/web/assets/app.js:182-190, 300-302`, `src/web/assets/app.css`

- [ ] **Step 1: Failing TUI test** in `src/tui/app.rs` tests:

```rust
    #[test]
    fn sending_to_an_unverified_agent_says_so() {
        let fleet = fleet_with(&[
            (1, "Claude", Status::Waiting, 0),
            (2, "Codex", Status::Waiting, 0),
        ]);
        assert_eq!(sent_note(&Action::Approve { pane: 1 }, &fleet), None);
        assert_eq!(
            sent_note(&Action::Deny { pane: 2 }, &fleet).as_deref(),
            Some("sent (unverified for codex)")
        );
        assert_eq!(
            sent_note(&Action::Reply { pane: 2, text: "x".into() }, &fleet),
            None
        );
    }
```

Run: `cargo test sending_to_an_unverified` → fails (no `sent_note`).

- [ ] **Step 2: Implement** in `src/tui/app.rs`:

```rust
/// What the footer says after approve/deny went out to an agent whose prompts aren't verified.
pub fn sent_note(action: &Action, fleet: &Fleet) -> Option<String> {
    let (Action::Approve { pane } | Action::Deny { pane }) = action else {
        return None;
    };
    let profile = agents::profile_for(&fleet.panes.get(pane)?.agent)?;
    (!profile.verified()).then(|| format!("sent (unverified for {})", profile.name()))
}
```

In `src/tui/mod.rs`:

```rust
                    Some(Command::Act(action)) => {
                        let note = app::sent_note(&action, &fleet);
                        let outcome = handle.act(action).await;
                        // `message()` is `None` only for a sent action.
                        app.flash = outcome.message().or(note);
                    }
```

(Adjust the path to `sent_note` to how `app` is imported in `tui/mod.rs`.)

- [ ] **Step 3: Web.** In `app.js`, a helper next to `statusLabel`:

```js
// Approve/deny for agents whose prompt layout comes from their source, not from real screens.
function markUnverified(button, agent) {
  if (agent.answer_verified) return;
  button.classList.add("unverified");
  button.title = `unverified for ${agent.agent}: derived from its source, not tested`;
}
```

In `card()`, after creating `approve` and `deny`: `markUnverified(approve, agent); markUnverified(deny, agent);`. In `renderDetail()`, after the `hidden` line:

```js
  for (const id of ["d-approve", "d-deny"]) {
    $(id).classList.remove("unverified");
    $(id).title = "";
    if (canAnswer) markUnverified($(id), agent);
  }
```

In `app.css`:

```css
button.unverified::after { content: "•"; margin-left: 4px; opacity: .7; }
```

- [ ] **Step 4: Run all checks** (`cargo test`, clippy, fmt, `node --test tests/web/*.test.js`). Manually: `cargo run`, open the dashboard; Claude cards show no dot (no other agents are available locally, so the dot itself can only be checked by temporarily setting `verified()` to `false` for Claude; revert afterwards).

- [ ] **Step 5: Commit**

```bash
git add src/tui src/web/assets
git commit -m "Mark approve/deny for unverified agents in the TUI and dashboard"
```

### Task 7: Reading Claude Code's subagent transcripts

**Files:**
- Modify: `src/agents/background.rs` (replace the stub from Task 1)

- [ ] **Step 1: Write the tests first** (at the bottom of `background.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::SystemTime;

    const SESSION: &str = "7d085caf-a845-4fa1-a0d8-db963b1c766d";
    const FINISHED: &str = r#"{"type":"user","message":{"role":"user","content":"go"}}
{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}
"#;
    const RUNNING: &str = r#"{"type":"user","message":{"role":"user","content":"go"}}
{"type":"assistant","message":{"stop_reason":null,"content":[{"type":"thinking","thinking":"…"}]}}
"#;

    fn subagents_dir(root: &Path) -> PathBuf {
        let dir = root
            .join("projects/-Users-dev-code-app")
            .join(SESSION)
            .join("subagents");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_agent(dir: &Path, id: &str, transcript: &str, kind: &str, description: &str) {
        fs::write(dir.join(format!("agent-{id}.jsonl")), transcript).unwrap();
        fs::write(
            dir.join(format!("agent-{id}.meta.json")),
            format!(r#"{{"agentType":"{kind}","description":"{description}","requestShape":"background"}}"#),
        )
        .unwrap();
    }

    fn now_ms() -> u64 {
        to_ms(SystemTime::now())
    }

    #[test]
    fn lists_running_agents_and_skips_finished_ones() {
        let root = tempfile::tempdir().unwrap();
        let dir = subagents_dir(root.path());
        write_agent(&dir, "a1", RUNNING, "researcher", "Locating prompts");
        write_agent(&dir, "a2", FINISHED, "coder", "Done already");
        let tasks = scan(root.path(), SESSION, now_ms());
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].kind, "researcher");
        assert_eq!(tasks[0].description, "Locating prompts");
        assert!(tasks[0].since_ms <= now_ms());
    }

    #[test]
    fn agents_silent_for_30_minutes_count_as_gone() {
        let root = tempfile::tempdir().unwrap();
        let dir = subagents_dir(root.path());
        write_agent(&dir, "a1", RUNNING, "researcher", "Stuck");
        let later = now_ms() + STALE_AFTER.as_millis() as u64 + 1_000;
        assert!(scan(root.path(), SESSION, later).is_empty());
    }

    #[test]
    fn a_missing_directory_means_no_background_work() {
        let root = tempfile::tempdir().unwrap();
        assert!(scan(root.path(), SESSION, now_ms()).is_empty());
    }

    #[test]
    fn session_ids_that_could_leave_the_directory_are_ignored() {
        let root = tempfile::tempdir().unwrap();
        subagents_dir(root.path());
        for bad in ["../x", "", "7D085CAF", "a/b", ".."] {
            assert!(scan(root.path(), bad, now_ms()).is_empty(), "{bad}");
        }
    }

    #[test]
    fn a_half_written_last_line_counts_as_running() {
        let root = tempfile::tempdir().unwrap();
        let dir = subagents_dir(root.path());
        write_agent(&dir, "a1", "{\"type\":\"assistant\",\"mess", "researcher", "Writing");
        assert_eq!(scan(root.path(), SESSION, now_ms()).len(), 1);
    }

    #[test]
    fn only_the_tail_of_a_long_transcript_is_read() {
        let root = tempfile::tempdir().unwrap();
        let dir = subagents_dir(root.path());
        let filler = format!("{}\n", r#"{"type":"user","message":{"content":"x"}}"#).repeat(5_000);
        write_agent(&dir, "a1", &(filler + FINISHED), "coder", "Long");
        assert!(scan(root.path(), SESSION, now_ms()).is_empty());
    }
}
```

Run: `cargo test agents::background` → fails (stub returns nothing, `to_ms`/`STALE_AFTER` missing).

- [ ] **Step 2: Implement**

```rust
//! Claude Code's background agents, read from the subagent transcripts it writes next to each
//! session: `<config>/projects/<project>/<session_id>/subagents/agent-<id>.jsonl` and `.meta.json`.
//! The format is internal to Claude Code; anything unexpected means "no background work".

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use super::BackgroundTask;

/// Stopped agents and crashed sessions never write a final message; this lets them drop out.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(30 * 60);
/// Enough for the last message of a transcript; reading whole transcripts (often MBs) is waste.
const TAIL_BYTES: u64 = 64 * 1024;

/// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn claude_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude"))
}

/// Running subagents of one session, oldest first.
pub fn scan(root: &Path, session_id: &str, now_ms: u64) -> Vec<BackgroundTask> {
    // tty7 reports UUIDs; anything else could point outside the projects directory.
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c) || c == '-')
    {
        return Vec::new();
    }
    let Some(dir) = subagents_dir(root, session_id) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut tasks: Vec<BackgroundTask> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|transcript| running_task(&transcript, now_ms))
        .collect();
    tasks.sort_by_key(|t| t.since_ms);
    tasks
}

/// Session ids are unique, so the project directory (a slug of the cwd) needn't be rebuilt.
fn subagents_dir(root: &Path, session_id: &str) -> Option<PathBuf> {
    fs::read_dir(root.join("projects"))
        .ok()?
        .filter_map(Result::ok)
        .map(|project| project.path().join(session_id).join("subagents"))
        .find(|dir| dir.is_dir())
}

fn running_task(transcript: &Path, now_ms: u64) -> Option<BackgroundTask> {
    let modified = to_ms(fs::metadata(transcript).ok()?.modified().ok()?);
    if now_ms.saturating_sub(modified) > STALE_AFTER.as_millis() as u64 {
        return None;
    }
    if finished(&last_line(transcript)?) {
        return None;
    }
    let meta_path = transcript.with_extension("meta.json");
    let meta: Meta = serde_json::from_slice(&fs::read(&meta_path).ok()?).ok()?;
    let since_ms = fs::metadata(&meta_path)
        .and_then(|m| m.modified())
        .map(to_ms)
        .unwrap_or(modified);
    Some(BackgroundTask {
        kind: meta.agent_type.unwrap_or_else(|| "agent".into()),
        description: meta.description.unwrap_or_default(),
        since_ms,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    agent_type: Option<String>,
    description: Option<String>,
}

fn last_line(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES))).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);
    tail.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(str::to_string)
}

/// A finished subagent's transcript ends with its final answer: an assistant message that
/// stopped at `end_turn`. A half-written line doesn't parse and counts as running.
fn finished(line: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    value["type"] == "assistant" && value["message"]["stop_reason"] == "end_turn"
}

pub(crate) fn to_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
```

- [ ] **Step 3: Run all checks, commit**

Run: `cargo test agents::background` → 6 pass; then clippy and fmt.

```bash
git add src/agents/background.rs
git commit -m "Read Claude Code's subagent transcripts to find running background agents"
```

### Task 8: Fleet keeps session ids and background work

**Files:**
- Modify: `src/tty7/types.rs:58-63`, `src/fleet/mod.rs`

- [ ] **Step 1: Failing tests** in `src/fleet/mod.rs` tests:

```rust
    #[test]
    fn resync_keeps_the_session_id() {
        let mut fleet = Fleet::default();
        fleet.resync(&agents(AGENTS), 0);
        assert_eq!(
            fleet.panes[&54].session_id.as_deref(),
            Some("00000000-0000-0000-0000-000000000054")
        );
    }

    #[test]
    fn finished_agents_with_background_work_sort_as_working() {
        let mut fleet = fleet_with(&[
            (1, "Claude", Status::Working, 5_000),
            (2, "Claude", Status::Done, 1_000),
            (3, "Claude", Status::Done, 2_000),
        ]);
        let task = BackgroundTask {
            kind: "researcher".into(),
            description: "Locating".into(),
            since_ms: 1_500,
        };
        let found = HashMap::from([(3, vec![task.clone()])]);
        assert!(fleet.set_background(&found));
        assert!(!fleet.set_background(&found), "unchanged");
        assert_eq!(fleet.panes[&3].board_status(), Status::Working);
        assert_eq!(fleet.panes[&3].status, Status::Done, "reply stays allowed");
        let order: Vec<u32> = fleet.ordered().iter().map(|p| p.pane).collect();
        assert_eq!(order, vec![3, 1, 2]);
        // Panes missing from a scan lose their background work.
        assert!(fleet.set_background(&HashMap::new()));
        assert!(fleet.panes[&3].background.is_empty());
    }

    #[test]
    fn background_targets_are_panes_with_a_session() {
        let mut fleet = Fleet::default();
        fleet.resync(&agents(AGENTS), 0);
        let targets = fleet.background_targets();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].pane, 54);
        assert_eq!(targets[0].agent, "Claude");
    }
```

(`agents(…)` is the existing fixture helper in the fleet tests; add `use std::collections::HashMap;` and `use crate::agents::BackgroundTask;` to the test module.)

Run: `cargo test fleet` → compile errors.

- [ ] **Step 2: Implement**

`src/tty7/types.rs`, `AgentEntryState`:

```rust
    /// The agent's own session id (Claude Code: names its transcript directory).
    #[serde(default)]
    pub session_id: Option<String>,
```

`src/fleet/mod.rs`: `use std::collections::{BTreeMap, HashMap};` and `use crate::agents::{BackgroundTarget, BackgroundTask};`. In `AgentPane`:

```rust
    /// Reported by `tty7 agents` only, so it arrives with the next resync.
    pub session_id: Option<String>,
    /// Subagents still running after the main turn; tty7's hooks don't report them.
    pub background: Vec<BackgroundTask>,
```

initialised as `session_id: None, background: Vec::new()` in `AgentPane::new`. Add:

```rust
impl AgentPane {
    /// The status the views group by: a finished agent with background work is still working.
    pub fn board_status(&self) -> Status {
        match self.status {
            Status::Done | Status::Idle if !self.background.is_empty() => Status::Working,
            status => status,
        }
    }
}
```

In `Fleet::resync`, next to `pane.launch = …`: `pane.session_id = entry.state.session_id.clone();`.

`ordered()` sorts by `(p.board_status(), p.status_since_ms, p.pane)`.

```rust
    /// Panes whose agent session is known, for the background worker.
    pub fn background_targets(&self) -> Vec<BackgroundTarget> {
        self.panes
            .values()
            .filter_map(|p| {
                Some(BackgroundTarget {
                    pane: p.pane,
                    agent: p.agent.clone(),
                    session_id: p.session_id.clone()?,
                })
            })
            .collect()
    }

    /// Applies one full scan; panes it doesn't mention have no background work. Returns true
    /// when anything changed.
    pub fn set_background(&mut self, found: &HashMap<u32, Vec<BackgroundTask>>) -> bool {
        let mut changed = false;
        for pane in self.panes.values_mut() {
            let next = found.get(&pane.pane).cloned().unwrap_or_default();
            if pane.background != next {
                pane.background = next;
                changed = true;
            }
        }
        changed
    }
```

Note in the ordering test: pane 3 sorts first among "working" because its `status_since_ms` (2 000) is older than pane 1's (5 000).

- [ ] **Step 3: Run all checks, commit**

```bash
git add src/tty7/types.rs src/fleet/mod.rs
git commit -m "Fleet: session ids, background work, and grouping finished agents with it as working"
```

### Task 9: Background worker in the runtime

**Files:**
- Modify: `src/agents/mod.rs` (add `background_for`), `src/runtime.rs`

- [ ] **Step 1: Failing test** in `src/agents/mod.rs` tests:

```rust
    #[test]
    fn background_for_skips_agents_without_a_profile() {
        let targets = vec![BackgroundTarget {
            pane: 7,
            agent: "Aider".into(),
            session_id: "00000000-0000-0000-0000-000000000007".into(),
        }];
        assert!(background_for(&targets, 0).is_empty());
    }
```

- [ ] **Step 2: Implement** in `src/agents/mod.rs`:

```rust
use std::collections::HashMap;

/// One scan over all targets; only panes with background work are in the result.
pub fn background_for(targets: &[BackgroundTarget], now_ms: u64) -> HashMap<u32, Vec<BackgroundTask>> {
    targets
        .iter()
        .filter_map(|t| {
            let tasks = profile_for(&t.agent)?.background_work(&t.session_id, now_ms);
            (!tasks.is_empty()).then_some((t.pane, tasks))
        })
        .collect()
}
```

- [ ] **Step 3: Worker** in `src/runtime.rs`:

```rust
/// Reading transcripts is cheap, but it's file I/O, so it runs off the core loop like git.
const BACKGROUND_EVERY: Duration = Duration::from_secs(5);
```

```rust
/// Looks for background agents of the given panes every few seconds and reports every result.
async fn background_worker(
    targets: watch::Receiver<Vec<BackgroundTarget>>,
    found: mpsc::Sender<HashMap<u32, Vec<BackgroundTask>>>,
) {
    loop {
        let current = targets.borrow().clone();
        let scan = tokio::task::spawn_blocking(move || agents::background_for(&current, now_ms()));
        match scan.await {
            Ok(result) => {
                if found.send(result).await.is_err() {
                    return;
                }
            }
            Err(err) => warn!("scanning for background agents failed: {err}"),
        }
        sleep(BACKGROUND_EVERY).await;
    }
}
```

In `start()`:

```rust
    let (background_targets, background_targets_rx) = watch::channel(Vec::new());
    let (background_tx, background_rx) = mpsc::channel(2);
    tokio::spawn(background_worker(background_targets_rx, background_tx));
```

`Core` gets a field `background_targets: watch::Sender<Vec<BackgroundTarget>>`, set in `start()`. `run()` takes `mut background: mpsc::Receiver<HashMap<u32, Vec<BackgroundTask>>>` as its last argument (passed from `start()`) and gets a branch:

```rust
                Some(found) = background.recv() => {
                    if self.fleet.set_background(&found) {
                        self.publish();
                    }
                }
```

In `publish()`, before `self.snapshots.send_if_modified(…)`:

```rust
        let targets = self.fleet.background_targets();
        self.background_targets.send_if_modified(|current| {
            if *current == targets {
                false
            } else {
                *current = targets;
                true
            }
        });
```

Imports: `use crate::agents::{self, BackgroundTarget, BackgroundTask};`.

- [ ] **Step 4: Run all checks**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`. The runtime tests use fixture session ids (`00000000-…`) that don't exist under `~/.claude`, so the worker finds nothing and the tests are unaffected.

- [ ] **Step 5: Commit**

```bash
git add src/agents/mod.rs src/runtime.rs
git commit -m "Scan for Claude Code background agents every 5 s in a background worker"
```

### Task 10: TUI shows background work

**Files:**
- Modify: `src/tui/view.rs` (`sections`, `agent_line`, `preview_lines`), `src/tui/app.rs` (`App::ordered` if it groups by `status`)

- [ ] **Step 1: Failing tests** in `src/tui/view.rs` tests (use the file's existing `render`/`NOW` helpers):

```rust
    fn with_background(fleet: &mut Fleet, pane: u32) {
        let task = BackgroundTask {
            kind: "researcher".into(),
            description: "Locating prompts".into(),
            since_ms: NOW - 237_000,
        };
        fleet.set_background(&HashMap::from([(pane, vec![task])]));
    }

    #[test]
    fn finished_agents_with_background_work_are_listed_under_working() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Done, NOW - 10_000)]);
        with_background(&mut fleet, 1);
        let screen = render(&fleet, &App::default());
        assert!(screen.contains("WORKING"));
        assert!(!screen.contains("DONE"));
        assert!(screen.contains('⟳'));
    }

    #[test]
    fn the_preview_lists_background_agents_first() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Done, NOW - 10_000)]);
        with_background(&mut fleet, 1);
        fleet.set_preview(1, "last output line".into());
        let app = App { selected: Some(1), ..App::default() };
        let screen = render(&fleet, &app);
        assert!(screen.contains("⟳ researcher · Locating prompts · 3m57s"));
        assert!(screen.contains("last output line"));
    }

    #[test]
    fn by_workspace_rows_say_done_with_background_count() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Done, NOW - 10_000)]);
        with_background(&mut fleet, 1);
        let app = App { grouping: Grouping::Workspace, ..App::default() };
        assert!(render(&fleet, &app).contains("done ⟳1"));
    }
```

(Adapt `render(…)` to the helper name and signature the file's tests already use, e.g. a `TestBackend` of 100×20.)

- [ ] **Step 2: Implement**

- `sections()`: filter with `p.board_status() == status` instead of `p.status == status`.
- `App::ordered` in `src/tui/app.rs`: if it filters or groups by `p.status`, switch to `p.board_status()` so `↑/↓` follow the screen.
- `agent_line()`: the status slot and the dot:

```rust
            Slot::Status => status_text(pane),
```

```rust
/// "done ⟳1" while background agents run after the main turn.
fn status_text(pane: &AgentPane) -> String {
    match pane.background.len() {
        0 => pane.status.label().to_string(),
        n => format!("{} ⟳{n}", pane.status.label()),
    }
}
```

(`fit(&value, 12)` takes `&str`; bind `let value = …` to a `String` for both arms.) The dot becomes `⟳` in the board status colour when there is background work:

```rust
    let marker = if pane.background.is_empty() { "●" } else { "⟳" };
    let line = Line::from(vec![
        Span::styled(marker, Style::new().fg(status_color(pane.board_status()))),
        Span::raw(text),
    ]);
```

- `preview_lines()`: before the screen lines, one line per background agent of the selected pane, counted against `room`:

```rust
    let background: Vec<Line<'static>> = selected
        .map(|p| {
            p.background
                .iter()
                .map(|t| {
                    let age = format_elapsed(now_ms.saturating_sub(t.since_ms), true);
                    Line::styled(
                        format!("⟳ {} · {} · {age}", t.kind, t.description),
                        Style::new().fg(Color::Blue),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let room = room.saturating_sub(background.len());
```

`preview_lines` needs `now_ms`; pass it from the caller as `list_lines` already gets it. Push `background` lines after the separator, then the screen lines.

- [ ] **Step 3: Run all checks, commit**

```bash
git add src/tui
git commit -m "TUI: background agents as ⟳ in the row and listed above the preview"
```

### Task 11: Dashboard shows background work

**Files:**
- Modify: `src/web/protocol.rs`, `src/web/assets/logic.js`, `src/web/assets/app.js`, `src/web/assets/app.css`, `tests/web/logic.test.js`

- [ ] **Step 1: Failing Rust test** in `src/web/protocol.rs` tests:

```rust
    #[test]
    fn agent_view_carries_background_work_and_its_board_column() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Done, 0)]);
        let task = BackgroundTask {
            kind: "researcher".into(),
            description: "Locating".into(),
            since_ms: 5,
        };
        fleet.set_background(&HashMap::from([(1, vec![task])]));
        let view = AgentView::from(&fleet.panes[&1]);
        assert_eq!(view.status, "done");
        assert_eq!(view.group, "working");
        assert_eq!(view.background.len(), 1);
        assert_eq!(view.background[0].kind, "researcher");
        assert_eq!(view.background[0].since_ms, 5);
    }
```

- [ ] **Step 2: Implement** in `protocol.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackgroundView {
    pub kind: String,
    pub description: String,
    pub since_ms: u64,
}
```

`AgentView` gains:

```rust
    /// The board column: `status`, except finished agents with background work are "working".
    pub group: &'static str,
    pub background: Vec<BackgroundView>,
```

filled with `group: p.board_status().key()` and

```rust
            background: p
                .background
                .iter()
                .map(|t| BackgroundView {
                    kind: t.kind.clone(),
                    description: t.description.clone(),
                    since_ms: t.since_ms,
                })
                .collect(),
```

- [ ] **Step 3: Failing JS test** in `tests/web/logic.test.js`:

```js
test("background agents read like the TUI's preview lines", () => {
  const now = 1_000_000;
  const agent = { background: [{ kind: "researcher", description: "Locating prompts", since_ms: now - 237_000 }] };
  assert.deepEqual(logic.backgroundLines(agent, now), ["researcher · Locating prompts · 3m57s"]);
  assert.deepEqual(logic.backgroundLines({ background: [] }, now), []);
  assert.deepEqual(logic.backgroundLines({}, now), []);
});
```

Run: `node --test tests/web/*.test.js` → fails (`backgroundLines` is not a function).

- [ ] **Step 4: Implement** in `logic.js` (and add it to `api`):

```js
  /** "researcher · Locating prompts · 3m57s" per running background agent. */
  function backgroundLines(agent, now) {
    return (agent.background || []).map((task) =>
      [task.kind, task.description, elapsed({ has_status: true, exact: true, since_ms: task.since_ms }, now)]
        .filter(Boolean).join(" · "));
  }
```

In `app.js`:
- `groupOf`: `(COLUMNS.includes(agent.group) ? agent.group : "other")`.
- `renderBoard()`: `state.agents.filter((a) => a.group === status)`.
- `card()`: after the `top` row is built, when there is background work:

```js
  const background = L.backgroundLines(agent, Date.now());
  if (background.length > 0) {
    const pill = el("span", "pill background", `⟳ ${background.length} background`);
    pill.title = background.join("\n");
    top.append(pill);
  }
```

- `renderDetail()`: after the `rows` array is built:

```js
  for (const line of L.backgroundLines(agent, Date.now())) rows.push(["background", line]);
```

- The focus list (`f-row`) and `visibleOrder()`: wherever they group by `agent.status`, use `groupOf(agent)` / `agent.group`, so the order matches the board.

In `app.css`:

```css
.pill.background { background: var(--working); }
```

- [ ] **Step 5: Run all checks** (`cargo test`, clippy, fmt, `node --test tests/web/*.test.js`). Manually: `cargo run`, start a Claude Code agent in another pane, have it launch a background agent and finish its turn; the card moves to WORKING with `⟳ 1 background`, its tooltip names the agent, the focus view lists it, and it moves to DONE once the background agent finishes.

- [ ] **Step 6: Commit**

```bash
git add src/web tests/web
git commit -m "Dashboard: background agents as a pill, in the focus view, and in the working column"
```

### Task 12: README and spec

**Files:**
- Modify: `README.md`, `docs/superpowers/specs/2026-11-13-agent-profiles-design.md`

- [ ] **Step 1: README.** Replace the sentence "Status and replies work for every agent that has tty7 hooks. One-key approve/deny of permission prompts is currently only available for Claude Code." in the Setup section with "Status and replies work for every agent that has tty7 hooks; see [Supported agents](#supported-agents) for approve/deny." and add after the Setup section:

```markdown
## Supported agents

| Agent | Status, reply, interact | Approve / deny | Verified |
|---|---|---|---|
| Claude Code | ✓ | `1` / Esc | ✓ |
| Codex | ✓ | `y` / Esc | – |
| Gemini CLI | ✓ | `1` / Esc | – |
| OpenCode | ✓ | Enter / Esc | – |
| Others with tty7 hooks | ✓ | – | – |

"Verified" means the prompt detection was checked against the agent's real screens. For the others it is derived from their source code; the TUI and dashboard mark approve/deny for them as unverified. If you use one of them, a screen capture of a permission prompt (`tty7 capture %N --plain`) is very welcome as an issue.

For Claude Code, shipherd also shows background agents that keep running after the main turn: the agent stays under *Working* and shows `⟳`, with the background agents listed in the preview and the focus view.
```

Also update the Limitations bullet "Approve/deny key sequences are only known for Claude Code so far; other agents get reply and interact." to "Approve/deny is verified for Claude Code only; Codex, Gemini CLI and OpenCode are derived from their source."

- [ ] **Step 2: Spec.** In the "Background agents" section, replace the `<cwd-slug>` bullet and the `SessionRef` bullet with: "The directory is found by searching `<config>/projects/*/<session_id>/subagents`; session ids are UUIDs, so the match is unique and the cwd slug isn't rebuilt." Replace the "Cost" bullet with: "A background worker scans every 5 s. Transcripts not modified in the last 30 minutes are skipped after a `stat`; the rest have their last 64 KB read. `since` is the modification time of the agent's `.meta.json`, written when it starts." In "Fleet and views", change "`since` = when shipherd first saw it running" accordingly. Change the trait's `background_work` signature to `(&self, session_id: &str, now_ms: u64)`.

- [ ] **Step 3: Commit**

```bash
git add README.md docs/superpowers/specs/2026-11-13-agent-profiles-design.md
git commit -m "README: supported agents and background agents; spec: transcript lookup by session id"
```
