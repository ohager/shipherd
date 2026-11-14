# shipherd — agent profiles and background work

Approve/deny for Codex, Gemini CLI and OpenCode next to Claude Code, and a view of Claude Code's background agents, which tty7's status hooks don't report.

## Goals

- Answer permission prompts of Codex, Gemini CLI and OpenCode with approve/deny, as for Claude Code.
- Keep agent-specific knowledge in one place per agent, so adding an agent is one new file with its tests.
- Show which Claude Code agents still have background agents running after their main turn ended.

## Non-goals

- Agents other than these four (tty7 has hooks for 22).
- User-defined agent profiles (config files, patterns).
- Background shells (`run_in_background` Bash) and background work of agents other than Claude Code.
- OpenCode's reject-feedback step (see below).

## Verification

None of the three new agents is installed on the development machine. Their prompt layouts and keys are derived from their source and its snapshot tests, and they are marked **unverified** in code, UI and README until someone records real screens. Sources read:

| Agent | Repository | Commit |
|---|---|---|
| Codex | github.com/openai/codex (`codex-rs/tui/src/bottom_pane/approval_overlay.rs`, `request_user_input/`, `chatwidget/plan_implementation.rs`, `keymap.rs`) | `32cec881b72360957d30ff48f94e038945603818` |
| Gemini CLI | github.com/google-gemini/gemini-cli (`packages/cli/src/ui/components/messages/ToolConfirmationMessage.tsx`, `ToolConfirmationQueue.tsx`, `ExitPlanModeDialog.tsx`, `hooks/useSelectionList.ts`) | `4ed06ed0ddc523a6d356a4ad35ddfea331041888` |
| OpenCode | github.com/anomalyco/opencode (formerly sst/opencode; `packages/tui/src/routes/session/permission.tsx`, `util/permission.ts`, `routes/session/form.tsx`) | `f0089856c07eded13396a0a0d05f19f1718be7b9` |

The names tty7 reports for them (`Codex`, `Gemini`, `OpenCode`) are inferred from strings in the tty7 binary; hook ids in `tty7 doctor --json` are `codex`, `gemini`, `opencode`.

## Agent profiles

A strategy per agent behind one trait, in a new module:

```
src/agents/
  mod.rs        AgentProfile, profile_for, Keystrokes, SessionRef, BackgroundTask
  claude.rs     verified
  codex.rs      unverified
  gemini.rs     unverified
  opencode.rs   unverified
```

```rust
pub trait AgentProfile: Sync {
    /// Lowercased first word of the name tty7 reports: "claude", "codex", "gemini", "opencode".
    fn name(&self) -> &'static str;
    /// Checked against real screens (true) or derived from the agent's source (false).
    fn verified(&self) -> bool;
    fn is_permission_prompt(&self, screen: &str) -> bool;
    fn approve(&self) -> Keystrokes;
    fn deny(&self) -> Keystrokes;
    /// Background work the status hooks don't report. Default: none.
    fn background_work(&self, _session: &SessionRef) -> Vec<BackgroundTask> {
        Vec::new()
    }
}

pub fn profile_for(agent: &str) -> Option<&'static dyn AgentProfile>;
```

- `profile_for` compares the first word of the agent name case-insensitively, so `Gemini CLI`, `gemini` and `OpenCode`/`opencode` all match.
- `Keystrokes` moves from `actions.rs` to `agents/mod.rs` unchanged (`Text { text, enter }` or `Key(name)`).
- All detectors look only at the last 20 non-empty lines (trimmed), as today: older output above a prompt may quote anything.

### Changes to existing code

- `actions.rs`: `prompt_keys` and `is_permission_prompt` move to `agents/claude.rs` with their tests. `execute` asks `profile_for(&known.agent)`; no profile means `Outcome::Unsupported`. The screen check before approve/deny uses the profile's `is_permission_prompt`. `supports_prompt_answers` becomes `profile_for(agent).is_some()`.
- `web/session.rs`: previews compute `permission_prompt` with the pane's agent profile (false without a profile).
- `web/protocol.rs`: next to `can_answer`, an `answer_verified: bool` field.
- TUI: after `y`/`n` on an unverified agent the footer says `sent (unverified for codex)`.
- Web: approve/deny buttons of unverified agents carry a small dot and the tooltip "unverified for Codex: derived from its source, not tested".
- Fixtures move to `tests/fixtures/screens/<agent>-<kind>.txt` (the existing Claude screens become `claude-permission.txt`, `claude-question.txt`, `claude-yes-no-question.txt`). The new agents' screens are rebuilt from their snapshot tests and source, each with a comment line naming the source file and commit.

## Prompts per agent

| Agent | Permission prompt when the tail has … | Not a permission prompt when it has … | Approve | Deny |
|---|---|---|---|---|
| Claude (verified) | a line starting `Do you want to` and a line `1. Yes` (after an optional `❯`) | `Tab/Arrow keys to navigate` | text `1` | `escape` |
| Codex | a line starting `Would you like to `, starting `Do you want to approve network access`, or ending ` needs your approval.`; a line `1. Yes, proceed` or `1. Yes, just this once` (after an optional `›`); `Press enter to confirm or esc to cancel` | `Question N/M`, `esc to interrupt`, `Implement this plan?` | text `y` | `escape` |
| Gemini | `1. Allow once` (after an optional `●`) and `No, suggest changes (esc)` | `Enter to select ·` (questions, plan approval) | text `1` | `escape` |
| OpenCode | `Permission required` and one line containing both `Allow once` and `Reject` | `esc dismiss`, `Type your own answer`, `Field N of M` | `enter` | `escape` |

Decisions and known risks:

- **Codex** approves with `y`, the hotkey shown in the option itself, rather than `1`: it stays right when the options between "Yes" and "No" change (prefix, session, none). Option 1 is always "Yes, proceed" / "Yes, just this once". Esc aborts the turn without a feedback dialog. MCP elicitation prompts ("Yes, provide the requested info") are not recognised.
- **Gemini** approves with `1`, which selects at once with fewer than 10 options; "Allow once" is always first. Never Enter: with `security.autoAddToPolicyByDefault` the default selection is "… all future sessions". In an untrusted folder only "Allow once" and "No, suggest changes (esc)" show, which still matches. While an external editor is open ("Modify in progress") there are no options, so nothing is recognised and approve/deny are refused.
- **OpenCode** has no digit shortcuts; options are buttons on one line and the selection is shown only by colour. Enter confirms the selection, which defaults to "Allow once". If the user moved the selection in the pane before, Enter answers with that option; plain text can't show it. This is the largest risk and stays documented in `opencode.rs`. In a sub-session, Reject opens a "Reject permission" feedback step; shipherd only sends `escape`, the step stays as `waiting` and is answered with reply or interact.

## Background agents (Claude Code)

Claude Code's background agents keep working after the main turn has ended; tty7 then reports the pane as `done`. Claude Code writes each subagent's transcript next to the session:

```
<config>/projects/<cwd-slug>/<session_id>/subagents/agent-<id>.jsonl
<config>/projects/<cwd-slug>/<session_id>/subagents/agent-<id>.meta.json   {"agentType", "description", "requestShape", …}
```

- `<config>` is `$CLAUDE_CONFIG_DIR`, else `~/.claude`. `<cwd-slug>` is the pane's cwd with `/` and `.` replaced by `-`.
- `SessionRef { session_id, cwd }` comes from `tty7 agents --json` (`state.session_id`, `state.cwd`); the adapter parses both.
- **Path safety:** a `session_id` with characters outside `[0-9a-f-]` is ignored; the slug is built only from the cwd tty7 reports and checked to be a single path component.
- **Running:** the transcript's last line is not an `assistant` message with `stop_reason: "end_turn"`, and the file changed in the last 30 minutes. (Checked 2026-11-13 against 30 transcripts: every finished one ends that way; a running one ended in a `thinking` block with `stop_reason: null`.) The 30-minute cutoff drops agents that were stopped or whose session crashed, which never write `end_turn`.
- **Cost:** a background worker, like the git and capture workers, checks the modification times every 5 s for Claude panes. It reads the last 64 KB of a transcript only when its modification time changed, and the `.meta.json` once per agent.
- **Unknown format or missing directory:** no background work; logged once per pane, no banner. The format is internal to Claude Code and may change.

### Fleet and views

- Each pane gets `background: Vec<BackgroundTask { kind, description, since }>` (`kind` = `agentType`, `since` = when shipherd first saw it running).
- **Grouping:** a `done` or `idle` agent with background work sorts as **working**, in both views and both layouts. Its status stays `done`/`idle`, so reply stays allowed.
- **TUI:** the row's status reads `done · ⟳1`; the preview area lists the background agents (`researcher · Locating … · 3m 57s`).
- **Web:** cards get a `⟳ 1 background` pill with the descriptions in its tooltip; the focus view gets a line `background: researcher · Locating … · 3m 57s` per agent.

## Testing

- **Profiles:** per agent, fixture screens for a shell permission prompt, an edit prompt, a question and a plan approval; the detector accepts only the permission prompts. Approve and deny keys per agent. `profile_for` with name variants (`Claude`, `Gemini CLI`, `OpenCode`, `opencode`, unknown).
- **Actions:** with the fake `Tty7Cli`, approve on a Codex pane sends `y`; unknown agents get `Unsupported`; the screen check uses the pane's profile.
- **Background work:** a `tempfile` directory with finished, running and stale transcripts; a `session_id` with `../` is rejected.
- **Fleet:** `done` plus background work sorts under working, and replies are still allowed.
- **Web:** `answer_verified` in the agent view; the pill and tooltip logic in `logic.js` under `node --test`.

## README

- A "Supported agents" table: agent, status/reply/interact, approve/deny, verified.
- The setup section's "approve/deny … only Claude Code" sentence points to that table.
- A note that background agents are shown for Claude Code.
