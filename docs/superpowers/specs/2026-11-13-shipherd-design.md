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
- Git branch is read once per cwd (`git rev-parse --abbrev-ref HEAD`) and cached; re-read when a pane's cwd changes. Repo name is the basename of `git rev-parse --show-toplevel`, else the cwd basename.
- **Watched panes:** the views register which panes they display output for (the TUI's selected row, any open web detail panel). For each watched pane, `capture %N --plain --tail 200` runs about every 400 ms (`--plain`: the raw ANSI capture of a full-screen agent like Claude Code was 3 MB for `--tail 5`, because `--tail` counts raw lines before redraws are applied); nothing is polled for unwatched panes. Captures are only published when they change.
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

- **Per-agent key table:** approve/deny are only offered for agents with a known mapping; v1 ships Claude Code only. Its prompt numbers vary ("No" was option 4 in one test), so deny uses Esc rather than a number. Other agents get reply and interact.
- **Race guard:** before each action, re-fetch `tty7 agents --json` and check the pane's current status. Approve/deny require `waiting`. Reply requires `done` or `idle`: during `waiting` the agent shows a selection menu where typed characters pick options (typing `1` approves), so free text there is unsafe. Otherwise drop the action and report "agent moved on (now …)". Interact-mode keys skip the guard (the user is driving).
- **Validation:** the pane id must be in the current `fleet`; anything else is rejected.
- Reply text is never passed as an argument (it would be visible in `ps`).

### TUI (ratatui)
- Layout: header (`⛵ shipherd · N agents · M need you`, web URL), a list grouped by status (NEEDS YOU / WORKING / DONE / IDLE) with agent, repo, branch, time in state; a preview of the selected agent's last lines below; a key-hint footer.
- Keys: `↑/↓` or `j/k` select, `y` approve, `n` deny, `r` reply (inline input line, Enter sends, Esc cancels), `w` open the web dashboard in the browser (URL includes the token), `q` quit.
- Banners for: hooks not installed (with the agent name from diagnostics), tty7 server unreachable, reconnecting.

### Web (axum)
- Bound to `127.0.0.1` only. Port 7777 by default (`--port`); if it's busy, falls back to a port picked by the OS. The actual URL is shown in the TUI header.
- Frontend: hand-written HTML/CSS/JS (no build step), embedded in the binary.
- **Board:** kanban columns NEEDS YOU / WORKING / DONE (idle agents in a collapsed row underneath). Cards show agent, repo · branch, time in state, a few preview lines; NEEDS YOU cards have Approve/Deny buttons when supported.
- **Detail panel** (opened by clicking a card): live output mirror (plain text from `capture --plain`, about 400 ms refresh; colours are out of scope for v1, see the capture note above), pane facts (cwd, branch, launch command, processes and listening ports from `tty7 procs`, time in state), Approve / Deny / Reply, and an **Interact** toggle.
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
- **CI:** GitHub Actions on macOS and Linux — `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`.

## Tech

Rust (stable), `tokio`, `serde` / `serde_json`, `ratatui` + `crossterm`, `axum` (WebSocket), `rand`, `tracing` + `tracing-appender` for file logging. Licence: Apache-2.0.
