# By-workspace grouping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Both views can group agents by workspace instead of by status (spec: "Grouping by workspace", TUI "By workspace", Web "By workspace").

**Architecture:** The server and protocol stay unchanged; each view groups the agents it already has. Rust: `Fleet::by_workspace()` next to `Fleet::ordered()`, a `Grouping` in the TUI's `App` that decides both the drawn sections and the `↑/↓` order. JS: a pure `groupByWorkspace()` in `logic.js` used by the board and the focus list; `j/k` walk the flattened visible order.

**Tech Stack:** Rust (ratatui), plain JS, `node --test`.

---

### Task 1: `Fleet::by_workspace`

**Files:** Modify `src/fleet/mod.rs`

- [ ] Test `by_workspace_sorts_workspaces_and_keeps_status_order_inside`: panes in "reviews" and "Default" plus one without a workspace → groups `[Some("Default"), Some("reviews"), None]` (case-insensitive alphabetical, no workspace last), each group in `ordered()` order.
- [ ] Run, see it fail (no such method).
- [ ] Implement:
  ```rust
  /// One group per workspace, alphabetical, agents without a workspace last; each in `ordered()` order.
  pub fn by_workspace(&self) -> Vec<(Option<&str>, Vec<&AgentPane>)>
  ```
- [ ] Run `cargo test fleet`, commit.

### Task 2: TUI `Grouping` and `g`

**Files:** Modify `src/tui/app.rs`

- [ ] Tests: `g_switches_the_grouping`; `arrows_follow_the_workspace_order` (selection moves in by-workspace order); `switching_keeps_the_selection`.
- [ ] Implement `pub enum Grouping { Status, Workspace }` (default `Status`), `App.grouping`, `App::ordered(&self, fleet) -> Vec<&AgentPane>` (flattened groups), used by `move_selection` and `sync_selection`; `g` toggles.
- [ ] `cargo test tui::app`, commit.

### Task 3: TUI rendering

**Files:** Modify `src/tui/view.rs`

- [ ] Tests: `by_workspace_sections_are_titled_with_counts` (`Default (1)` before `reviews (2)`, rows show `needs you`/`working`, not the workspace); `header_says_by_workspace`; footer hint contains `g group`, short hint `↑↓ y n r x g w q`.
- [ ] Implement: `list_lines` builds `(heading, panes)` sections from `app.grouping`; `agent_line` takes a `Slot` (`Workspace` | `Status`) for its 12-wide column; header adds `  by workspace`; hints updated.
- [ ] `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, commit.

### Task 4: `groupByWorkspace` in `logic.js`

**Files:** Modify `src/web/assets/logic.js`, `tests/web/logic.test.js`

- [ ] Tests: groups sorted case-insensitively with `null` workspace last as "other"; inside a group waiting → working → done → idle → unknown, then longest in state (smaller `since_ms`) first; agents without status after those with one of the same status is not required (server order is kept for ties).
- [ ] Implement `groupByWorkspace(agents) -> [{ workspace, agents }]`.
- [ ] `node --test tests/web/*.test.js`, commit.

### Task 5: Web board and focus list

**Files:** Modify `src/web/assets/index.html`, `app.css`, `app.js`

- [ ] Header switch `Status | Workspace` (two buttons, `aria-pressed`), stored in `localStorage["shipherd.grouping"]`.
- [ ] `#ws-board`: one `.column.ws` per group (badge-coloured heading with count), horizontal scroll, min 260px. Cards in this mode: `.card.by-ws` with a status bar (`border-left`), status pill instead of the badge, idle cards dimmed.
- [ ] Focus list groups by workspace in that mode; `j/k` use the flattened visible order.
- [ ] Check in the browser (headless screenshot), commit.

### Task 6: Finish

- [ ] Full test run (Rust + JS), clippy, fmt; finishing-a-development-branch.
