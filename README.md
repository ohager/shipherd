<p align="center">
  <img src="src/web/assets/shipherd-logo.svg" alt="shipherd" width="320">
</p>

<p align="center">
  <strong>Herd your coding agents.</strong><br>
  A live overview of every agent running in <a href="https://tty7.io">tty7</a>, as a terminal UI and a web dashboard.
</p>

<p align="center">
  <a href="https://github.com/ohager/shipherd/actions/workflows/ci.yml"><img src="https://github.com/ohager/shipherd/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License: Apache-2.0"></a>
  <img src="https://img.shields.io/badge/rust-2024_edition-orange.svg" alt="Rust 2024 edition">
</p>

---


https://github.com/user-attachments/assets/eca3ad17-3242-45b8-9266-2220dcdff14d



Running five Claude Code sessions across three repos? shipherd shows them all in one place, puts the ones waiting for you at the top, and lets you approve, deny or reply without hunting for the right pane.

## Features

- **Everything at a glance.** Agents are grouped as **Needs you → Working → Done → Idle**, longest-waiting first, with repo, branch and time in state.
- **Answer without switching.** Approve or deny a permission prompt, or send a reply, straight from the overview.
- **Group by workspace.** Switch between grouping by status and by tty7 workspace, in the TUI (`g`) and on the dashboard.
- **Web dashboard.** A kanban board with one card per agent, plus a focus view with a live mirror of the agent's screen, its processes and ports.
- **Interact mode.** Type into an agent's pane from the browser when a quick reply isn't enough.
- **Safe by default.** Approve/deny is only sent when a real permission prompt is on screen, and every action re-checks the agent's status first, so a late click doesn't land in the wrong place.
- **Light on tty7.** Only the panes you're looking at are captured, every few seconds, and only while you look.

## Requirements

- macOS or Linux
- [tty7](https://tty7.io) with its `tty7` CLI on your `PATH`
- Status hooks installed for your agents (see [Setup](#setup))

## Installation

Prebuilt binaries for macOS (Apple Silicon and Intel) and Linux (x86_64 and arm64):

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ohager/shipherd/releases/latest/download/shipherd-installer.sh | sh
```

The installer puts `shipherd` in `~/.local/bin`. You can also download a tarball for your platform from the [releases page](https://github.com/ohager/shipherd/releases); each one comes with a `.sha256` checksum.

To build from source instead, with a recent stable Rust toolchain:

```bash
git clone https://github.com/ohager/shipherd.git
cd shipherd
cargo install --path .
```

## Setup

shipherd doesn't connect to your agents itself. It reads what tty7 knows about them, so the setup happens in tty7.

1. **Check the tty7 CLI.** shipherd calls `tty7` for everything and exits with a hint if the CLI isn't on your `PATH`:
   ```bash
   tty7 --version
   ```
2. **Install the status hooks for your agents.** In the tty7 app, open **Settings → Integrations** and install the hooks for each agent you use (Claude Code, Codex, Gemini CLI, …). The hooks are how an agent tells tty7 whether it is working, waiting for you, or done. To check what is installed:
   ```bash
   tty7 doctor   # look at the "agent hooks" line
   ```
3. **Run your agents in tty7 panes.** Start them as usual in any tab and any workspace. shipherd sees every agent pane on the machine, not just the ones in its own workspace. To see what tty7 reports right now:
   ```bash
   tty7 agents
   ```
4. **Start shipherd in a tty7 pane** as well, for example in a split next to your agents (see [Usage](#usage)).

What happens without hooks:
- If an agent has no hooks installed, shipherd shows a banner such as `status hooks not installed for claude: tty7 Settings → Integrations`. The agent is still listed, but without a status.
- An agent that was started before its hooks were installed shows up as *idle*, marked "no status yet", until it reports its first status.

Status and replies work for every agent that has tty7 hooks; see [Supported agents](#supported-agents) for approve/deny.

## Supported agents

| Agent | Status, reply, interact | Approve / deny | Verified |
|---|---|---|---|
| Claude Code | ✓ | `1` / Esc | ✓ |
| Codex | ✓ | `y` / Esc | – |
| Gemini CLI | ✓ | `1` / Esc | – |
| OpenCode | ✓ | Enter / Esc | – |
| Others with tty7 hooks | ✓ | – | – |

"Verified" means the prompt detection was checked against the agent's real screens. For the others it is derived from their source code, and the TUI and dashboard mark their approve/deny as unverified. If you use one of them, a capture of a permission prompt (`tty7 capture %N --plain`) is very welcome as an issue.

For Claude Code, shipherd also shows background agents that keep running after the main turn. The agent stays under *Working* and shows `⟳`, and its background agents are listed in the preview and in the focus view.

## Usage

Open a split pane in tty7 and run:

```bash
shipherd              # web dashboard on port 7777
shipherd --port 8080  # or pick another port
```

If the port is taken, shipherd falls back to a free one. The actual URL is shown in the TUI header.

### Terminal UI

| Key | Action |
|---|---|
| `↑` `↓` / `j` `k` | Select an agent |
| `y` | Approve a permission prompt |
| `n` | Deny a permission prompt |
| `r` | Reply (Enter sends, Esc cancels) |
| `x` | Close the agent's pane (asks first) |
| `g` | Group by status / by workspace |
| `w` | Open the web dashboard in your browser |
| `q` | Quit |

Below the list you see the last lines of the selected agent's screen.

### Web dashboard

Press `w` in the TUI to open the dashboard; the link logs your browser in.

- **Board:** columns for *Needs you*, *Working* and *Done*, idle agents folded underneath. Use the `Status | Workspace` switch in the header to get one column per workspace instead.
- **Focus view:** click a card to see the agent's live screen, its directory, command, processes and ports, with Approve / Deny / Reply, Interact and Close pane. `j`/`k` switch agents, `Esc` goes back to the board. The address (`/#pane-48`) keeps the selection across reloads.
- **Interact:** sends your keystrokes to the pane. The panel turns amber while it's on, and it switches itself off after two minutes without a keystroke.

## Security

Interact mode can type into a shell, so the dashboard is treated as a local remote control:

- It listens on `127.0.0.1` only.
- Each run creates a random 256-bit token. The login link swaps it for an `HttpOnly`, `SameSite=Strict` cookie and removes it from the address bar.
- `Host` and `Origin` headers are checked (protection against DNS rebinding and cross-site WebSockets). No CORS headers are sent.
- Reply text goes to tty7 on stdin, never as a command-line argument, and the token is never logged.

One caveat: `w` hands the login link to `open` / `xdg-open`, so the token is briefly visible to other local users in `ps`.

## How it works

shipherd is a single Rust binary. It talks to tty7 only through the `tty7` CLI (`agents`, `events`, `capture`, `send`, `procs`, all with `--json`), keeps the fleet state in one place, and feeds both views from it.

```
tty7-server
   ▲ commands (agents, capture, send)   ▼ NDJSON (events --json)
┌──────────────────────── core ────────────────────────┐
│ tty7 adapter  →  fleet (state)  ←  actions           │
└──────────── watch channel ▼ ▲ action requests ───────┘
        TUI (ratatui)            Web (axum + WebSocket)
```

The web frontend is plain HTML, CSS and JavaScript, embedded in the binary; there is no build step. Logs go to `~/.local/state/shipherd/shipherd.log`.

For the full design, see [docs/superpowers/specs/2026-11-13-shipherd-design.md](docs/superpowers/specs/2026-11-13-shipherd-design.md).

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
node --test tests/web/*.test.js   # dashboard logic
```

CI runs the same checks on macOS and Linux. Parser tests use real tty7 output recorded in `tests/fixtures/`.

### Releasing

Releases are built by [cargo-dist](https://github.com/axodotdev/cargo-dist) (`dist-workspace.toml`, `.github/workflows/release.yml`). To cut one:

1. Bump `version` in `Cargo.toml` and commit.
2. Tag the commit and push the tag:
   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

The workflow builds all four targets on native runners, then creates the GitHub release with the tarballs, checksums and `shipherd-installer.sh`. A tag with a suffix such as `v0.2.0-rc.1` is published as a prerelease.

After changing the dist config, run `dist generate` to regenerate the workflow, and `dist plan` to preview what a release will contain.

## Limitations

- Approve/deny is verified for Claude Code only; for Codex, Gemini CLI and OpenCode it is derived from their source.
- Background agents are only shown for Claude Code, and not while a subagent has ended its turn but still waits for a background shell of its own.
- Local machine only (no remote tty7 hosts yet).
- No Windows support.

## License

[Apache-2.0](LICENSE)
