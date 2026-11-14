# shipherd — design

A live overview of the coding agents running in [tty7](https://tty7.io), as a terminal UI and a web dashboard, with quick replies and an interactive detail view.

## Goals

- See every agent on this machine at a glance, with the ones that need you first.
- Answer a waiting agent (approve, deny, reply) without leaving the overview.
- In the browser, open any agent and watch its live output; optionally type into it.

## Non-goals (v1)

- Remote machines (`tty7 -m`).
- A separate background service (`shipherd serve`). The code is structured for it, but v1 is one process.
- Starting new agents, closing panes, creating worktrees.
- Windows support.
- Focusing a pane in the tty7 GUI (the tty7 CLI has no such verb).

## Overview

One Rust binary, `shipherd`, run in a tty7 split pane. It draws the TUI and serves the web dashboard on `127.0.0.1` from the same process. It talks to tty7 only through the `tty7` CLI (child processes with `--json`), never through tty7's internal socket.

```
tty7-server
   ▲ commands (agents, capture, send)   ▼ NDJSON (events --json)
┌──────────────────────── core ────────────────────────┐
│ tty7 adapter  →  fleet (state)  ←  actions           │
└──────────── watch channel ▼ ▲ action requests ───────┘
        TUI (ratatui)            Web (axum + WebSocket)
```

## Components

### `tty7` adapter
- The only module that knows tty7's JSON shapes; parses them into typed structs.
- All CLI access goes through one trait (`Tty7Cli`) with methods `agents()`, `events()` (a stream of parsed events), `capture(pane, tail)`, `send_text(pane, text)`, `send_keys(pane, keys)`, `procs(pane)`. A real implementation spawns `tty7`; tests use a fake.
- Processes are spawned directly with argument lists — never through a shell.
- Unknown event kinds or fields: logged and skipped.

### `fleet` (state)
- Holds the current set of agent panes: pane id, agent name, status (`idle` / `working` / `waiting` / `done`), status-since timestamp, cwd, repo name, git branch, diagnostics, latest preview.
- Applies events as a pure function: `(state, event) → state`.
- **Time in state:** tty7 26.10's `agents --json` reports `pane_id` and a nested `state.status` but no status-since timestamp (the docs mention `since_ms`/`for`; the installed version doesn't emit them). shipherd records the time it observes each status change. Agents already in a status when shipherd starts show that time as "since shipherd started" (rendered with a `≥` prefix, e.g. `≥2m`). If a later tty7 version emits a timestamp, the adapter prefers it.
- Owns ordering for both views: grouped by status in the order **waiting → working → done → idle**, then longest-in-state first within a group.
- **Grouping by workspace** (the alternative layout in both views): one group per workspace that has agents, alphabetical, agents without a workspace last as "other"; inside a group the same status order and longest-in-state first.
- Git branch is read once per cwd (`git rev-parse --abbrev-ref HEAD`) and cached; re-read when a pane's cwd changes. Repo name is the basename of `git rev-parse --show-toplevel`, else the cwd basename.
- **Watched panes:** the views register which panes they display output for (the TUI's selected row, any open web detail panel). For each watched pane, `capture %N --plain --tail 200` runs about every 400 ms (`--plain`: the raw ANSI capture of a full-screen agent like Claude Code was 3 MB for `--tail 5`, because `--tail` counts raw lines before redraws are applied); nothing is polled for unwatched panes. Captures are only published when they change.
- **Nothing slow in the core loop:** captures, agent detection and git lookups run in background tasks and report back over channels, so keystrokes never wait for them. After a sent keystroke the capture worker skips its pause, so the mirror shows the change after about one capture (~330 ms for a Claude Code pane).
- **Periodic resync:** `agents --json` every 10 s, which drops panes that have closed. (The close event's shape hasn't been recorded yet, so removal doesn't depend on it.)
- Publishes snapshots on a `tokio::sync::watch` channel. Views subscribe and redraw on change. (When `shipherd serve` is split out later, the TUI replaces this channel with the WebSocket client.)

### `actions`
The single path for all input into panes.

| Action | Effect |
|---|---|
| approve | per-agent key sequence (Claude Code: `1`, verified 2026-11-13) |
| deny | per-agent key sequence (Claude Code: `escape`, verified 2026-11-13) |
| reply | `tty7 send %N --stdin --enter` with the text on stdin |
| keys (interact mode) | `tty7 send %N --key …` |
| close | `tty7 pane close %N`, always with the explicit pane id (without one, tty7 closes the caller's own pane) |

- **Per-agent key table:** approve/deny are only offered for agents with a known mapping; v1 ships Claude Code only. Its prompt numbers vary ("No" was option 4 in one test), so deny uses Esc rather than a number. Other agents get reply and interact.
- **Race guard:** before each action, re-fetch `tty7 agents --json` and check the pane's current status. Approve/deny require `waiting`. Reply requires `done` or `idle`: during `waiting` the agent shows a selection menu where typed characters pick options (typing `1` approves), so free text there is unsafe. Otherwise drop the action and report "agent moved on (now …)". Interact-mode keys skip the guard (the user is driving).
- **Permission prompts only:** tty7 reports questions and permission prompts both as `waiting`. Before approve/deny, the pane's screen is captured; only a Claude Code permission prompt ("Do you want to …?", "1. Yes", no "Tab/Arrow keys to navigate" footer) is answered. Otherwise the action is refused with "that's a question, not a permission prompt". The page also hides approve/deny in the detail panel while a question is showing.
- **Close:** no status check (closing a working agent is allowed); the views always confirm first. Only agent panes in the fleet can be closed, so shipherd's own pane and plain shells are safe.
- **Validation:** the pane id must be in the current `fleet`; anything else is rejected.
- Reply text is never passed as an argument (it would be visible in `ps`).

### TUI (ratatui)
- Layout: header (`⛵ shipherd · N agents · M need you`, web URL), a list grouped by status (NEEDS YOU / WORKING / DONE / IDLE) with agent, repo, branch, time in state; a preview of the selected agent's last lines below; a key-hint footer.
- Keys: `↑/↓` or `j/k` select, `y` approve, `n` deny, `r` reply (inline input line, Enter sends, Esc cancels), `x` close (footer asks "close %N (agent · workspace)? y/n"; only `y` confirms, any other key cancels), `w` open the web dashboard in the browser (URL includes the token), `g` switch the grouping between status and workspace, `q` quit.
- **By workspace** (`g`): sections are titled `default (3)`, rows show the status (dot and label) instead of the workspace, the header shows "by workspace". The selection stays on the same agent when switching; `↑/↓` follow the order on screen. Not remembered across restarts.
- The list scrolls to keep the selected agent visible. In narrow panes columns drop out in this order: branch, workspace, repo; agent and time always show. The header leaves out the URL rather than cutting it, and the key hints shorten.
- Banners for: hooks not installed (with the agent name from diagnostics), tty7 server unreachable, reconnecting.

### Web (axum)
- Bound to `127.0.0.1` only. Port 7777 by default (`--port`); if it's busy, falls back to a port picked by the OS. The actual URL is shown in the TUI header.
- Frontend: hand-written HTML/CSS/JS (no build step), embedded in the binary.
- **Workspace badge:** a pill with the workspace name, coloured from a hash of the name (stable per workspace). Used on cards, in the focus list, in the detail header and in the breadcrumb.
- **Board:** kanban columns NEEDS YOU / WORKING / DONE (idle agents in a collapsed row underneath). Cards show the workspace badge, time in state, the agent's task title as the main line (its terminal title without the spinner glyph, or the tab label; every agent is "claude", so the task is what identifies it), and repo · branch. NEEDS YOU cards have Approve/Deny buttons when supported. 
- **By workspace:** a `Status | Workspace` switch in the header (remembered in `localStorage`). One column per workspace with its badge colour and agent count, scrolling sideways when there are many (columns at least ~260px). Cards are ordered by status, carry a coloured bar on the left and a status pill (`NEEDS YOU · 41s`), idle cards are dimmed, and the workspace badge is left out (the column says it). The focus list follows the chosen layout, and `j/k` move in the order shown.
- Preview lines on cards would require capturing every pane several times a second, so there are none.
- **Focus view** (opened by clicking a card; replaces the board): a narrow list of all agents on the left, grouped by status with badge and task, the selection marked with a blue bar; `↑/↓` or `j/k` switch agents and Esc returns to the board (both only while interact mode is off). On the right: breadcrumb (Board › badge › task), the task as heading with a status pill ("NEEDS YOU · 2m"), `%pane · agent · repo · branch`, directory and command, ports and processes from `tty7 procs` on one line (full list in a tooltip; port entries untyped until a real one has been recorded), the live mirror (plain text from `capture --plain`) filling the remaining height, Approve / Deny / Reply, Interact, Close pane. The focused pane is in the address (`/#pane-48`): reloading keeps it, the browser's back button returns to the board, switching panes inside the focus view replaces the history entry.
- **Page logic:** pure functions (badge colours, elapsed time, list navigation, address parsing, key mapping) live in `logic.js`, shared by the page and Node's built-in test runner (`node --test tests/web/*.test.js`).
- **Interact mode:** off by default, per pane. While on, the panel has an amber border and an "INTERACTING" label, and browser keystrokes are mapped to `tty7 send --key` names (printable characters as text). It switches off after 2 minutes without a keystroke, or when the panel closes.
- Transport: one WebSocket per page. Server → client: snapshots, preview updates, action results. Client → server: watch/unwatch pane, actions.

## Security (web)

Interact mode can type into shells, so the web server is treated as a local remote-control endpoint.

- Bind `127.0.0.1` only.
- A random 256-bit token is generated per run. `w` opens `http://127.0.0.1:<port>/?token=…`; the server checks it, sets an `HttpOnly; SameSite=Strict; Path=/` cookie, and redirects to `/` so the token leaves the address bar. Every HTTP request and the WebSocket upgrade require the cookie (or the token on the first request).
- `Host` must be `127.0.0.1:<port>` or `localhost:<port>` (defends against DNS rebinding).
- The WebSocket upgrade requires `Origin` equal to `http://127.0.0.1:<port>` or `http://localhost:<port>`.
- No CORS headers.
- The token is never logged.
- `w` passes the login link to `open`/`xdg-open`, so the token is briefly visible in `ps` to other local users.

## Error handling

| Situation | Behaviour |
|---|---|
| `tty7` not on PATH | exit with a clear message and an install link |
| tty7 server unreachable | banner; retried on the event-stream backoff (1 s → 30 s) and the 10 s resync |
| `events` child exits | restart with exponential backoff (1 s → 30 s cap), "reconnecting" banner, full resync via `agents --json` on recovery |
| status hooks not installed | banner pointing to Settings → Integrations; agents still listed |
| `capture` / `send` fails because the pane is gone | remove the pane from `fleet`; detail panel shows "pane closed" |
| unknown event / JSON field | log and skip |
| web port busy | fall back to a port picked by the OS |

Logs go to `~/.local/state/shipherd/shipherd.log` (never to the terminal; it would corrupt the TUI).

## Testing

- **Adapter:** parsing tests against recorded real `tty7 … --json` output stored in `tests/fixtures/`.
- **fleet:** pure reducer tests — event sequences → expected state, ordering, time in state, watched-pane bookkeeping.
- **actions:** with a fake `Tty7Cli` — race guard, reply via stdin, per-agent key table, rejection of unknown pane ids.
- **Web security:** HTTP-level tests that the server rejects a missing or wrong token, a foreign `Host`, a foreign `Origin`, and accepts the correct ones.
- **TUI:** rendering against ratatui's `TestBackend` asserting on the rendered text (no snapshot library).
- **Web:** auth checks and session logic as pure unit tests; the router via `tower::ServiceExt::oneshot`; two end-to-end WebSocket tests with `tokio-tungstenite`.
- **CI:** GitHub Actions on macOS and Linux — `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`.

## Tech

Rust (stable), `tokio`, `serde` / `serde_json`, `ratatui` + `crossterm`, `axum` (WebSocket), `rand`, `tracing` + `tracing-appender` for file logging. Licence: Apache-2.0.
