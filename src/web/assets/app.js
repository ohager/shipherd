"use strict";

const L = window.ShipherdLogic;
const COLUMNS = ["waiting", "working", "done"];
const GROUPS = [["waiting", "Needs you"], ["working", "Working"], ["done", "Done"], ["other", "Idle"]];
const STATUS_LABEL = { waiting: "needs you", working: "working", done: "done", idle: "idle", unknown: "unknown" };

// selected: the focused pane (from the address); null shows the board.
// permissionPrompt: what the selected pane's screen shows; null until its first preview arrives.
// loaded: whether the first snapshot has arrived, so a missing pane can be told apart from a closed one.
// grouping: "status" or "workspace", how the board and the focus list are laid out.
// openIdle: the workspaces whose idle fold is open on the workspace board.
const state = { agents: [], banners: [], selected: null, interacting: false, nextId: 1, ws: null, permissionPrompt: null, loaded: false, grouping: "status", openIdle: new Set() };
const GROUPING_KEY = "shipherd.grouping";
let pendingKeys = [];
let flushTimer = null;

const $ = (id) => document.getElementById(id);

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = text;
  return node;
}

function badge(workspace) {
  if (!workspace) return el("span");
  const node = el("span", "badge", workspace);
  const { background, color } = L.workspaceColor(workspace);
  node.style.background = background;
  node.style.color = color;
  return node;
}

// Every agent is "claude", so the task is what tells them apart.
const displayTitle = (agent) => agent.title || agent.agent;
const groupOf = (agent) => (COLUMNS.includes(agent.status) ? agent.status : "other");
const elapsed = (agent) => L.elapsed(agent, Date.now());

const byWorkspace = () => state.grouping === "workspace";
const statusLabel = (agent) => (STATUS_LABEL[agent.status] || agent.status).toUpperCase();

// Approve/deny for agents whose prompt layout comes from their source, not from real screens.
function markUnverified(button, agent) {
  if (agent.answer_verified) return;
  button.classList.add("unverified");
  button.title = `unverified for ${agent.agent}: derived from its source, not tested`;
}

/** The agents in the order the focus list shows them, which is the order j/k walk. */
function visibleOrder() {
  return byWorkspace() ? L.groupByWorkspace(state.agents).flatMap((g) => g.agents) : state.agents;
}

function setGrouping(grouping) {
  state.grouping = grouping;
  localStorage.setItem(GROUPING_KEY, grouping);
  render();
}

function current() {
  return state.agents.find((a) => a.pane === state.selected);
}

// --- connection ---------------------------------------------------------

function connect() {
  const ws = new WebSocket(`ws://${location.host}/ws`);
  state.ws = ws;
  ws.onopen = () => {
    $("conn").textContent = "";
    send({ type: "watch", pane: state.selected });
  };
  ws.onmessage = (event) => receive(JSON.parse(event.data));
  ws.onclose = () => {
    $("conn").textContent = "disconnected, reconnecting…";
    setInteracting(false, false);
    setTimeout(connect, 2000);
  };
}

let noticeTimer = null;
function showNotice(text) {
  $("notice").textContent = text;
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => { $("notice").textContent = ""; }, 5000);
}

function send(msg) {
  if (state.ws && state.ws.readyState === WebSocket.OPEN) state.ws.send(JSON.stringify(msg));
}

function receive(msg) {
  switch (msg.type) {
    case "snapshot":
      state.agents = msg.agents;
      state.banners = msg.banners;
      state.loaded = true;
      render();
      break;
    case "preview":
      if (msg.pane === state.selected) {
        state.permissionPrompt = msg.permission_prompt;
        showPreview(msg.text);
        renderDetail();
      }
      break;
    case "procs":
      if (msg.pane === state.selected) showProcs(msg);
      break;
    case "action_result":
      $("d-result").textContent = msg.ok ? "" : (msg.message || "failed");
      if (!msg.ok && state.selected === null) showNotice(msg.message || "failed");
      break;
    case "interact":
      if (msg.pane === state.selected) setInteracting(msg.on, false);
      break;
  }
}

// --- navigation ---------------------------------------------------------

/** Opening a pane adds a history entry, so the browser's back button returns to the board.
 *  Switching between panes in the focus view replaces it instead of piling up entries. */
function select(pane, { replace = false } = {}) {
  if (state.selected === pane) return;
  const url = L.hashForPane(pane) || location.pathname;
  if (replace) history.replaceState(null, "", url);
  else history.pushState(null, "", url);
  applySelection(pane);
}

function back() {
  select(null);
}

function applySelection(pane) {
  hideCloseConfirm();
  setInteracting(false, true);
  state.selected = pane;
  state.permissionPrompt = null;
  $("d-mirror").textContent = "";
  $("d-procs").textContent = "";
  $("d-result").textContent = "";
  send({ type: "watch", pane });
  render();
}

window.addEventListener("popstate", () => applySelection(L.paneFromHash(location.hash)));

// --- rendering ----------------------------------------------------------

function render() {
  const waiting = state.agents.filter((a) => a.status === "waiting").length;
  $("summary").textContent = `${state.agents.length} agents${waiting ? ` · ${waiting} need you` : ""}`;
  document.title = waiting ? `(${waiting}) shipherd` : "shipherd";
  $("banners").replaceChildren(...state.banners.map((b) => el("div", "banner", b)));
  for (const button of document.querySelectorAll("#grouping button")) {
    button.setAttribute("aria-pressed", String(button.dataset.grouping === state.grouping));
  }

  const focused = state.selected !== null;
  $("board-view").hidden = focused;
  $("focus").hidden = !focused;
  if (focused) {
    renderList();
    renderDetail();
  } else {
    renderBoard();
  }
}

function card(agent) {
  const node = el("article", `card ${agent.status}`);
  node.dataset.pane = agent.pane;

  const top = el("div", "card-top");
  if (byWorkspace()) {
    // The column names the workspace, so the card shows the status instead.
    node.classList.add("by-ws");
    top.append(el("span", `pill ${agent.status}`, agent.has_status ? statusLabel(agent) : "NO STATUS YET"), el("span", "elapsed", agent.has_status ? elapsed(agent) : ""));
  } else {
    top.append(el("span", "dot"), badge(agent.workspace), el("span", "elapsed", elapsed(agent)));
  }
  const where = [agent.repo, agent.branch].filter(Boolean).join(" · ") || agent.cwd || "";
  node.append(top, el("div", "title", displayTitle(agent)), el("div", "where", where));

  if (agent.status === "waiting" && agent.can_answer) {
    const row = el("div", "card-actions");
    const approve = el("button", "approve", "Approve");
    approve.onclick = (event) => { event.stopPropagation(); act("approve", agent.pane); };
    const deny = el("button", "deny", "Deny");
    deny.onclick = (event) => { event.stopPropagation(); act("deny", agent.pane); };
    markUnverified(approve, agent);
    markUnverified(deny, agent);
    row.append(approve, deny);
    node.append(row);
  }
  node.onclick = () => select(agent.pane);
  return node;
}

function renderBoard() {
  const grouped = byWorkspace();
  $("board").hidden = $("idle").hidden = grouped;
  $("ws-board").hidden = !grouped;
  if (grouped) {
    renderWorkspaceBoard();
    return;
  }
  for (const status of COLUMNS) {
    const column = document.querySelector(`.column[data-status="${status}"] .cards`);
    column.replaceChildren(...state.agents.filter((a) => a.status === status).map(card));
  }
  const idle = state.agents.filter((a) => groupOf(a) === "other");
  document.querySelector("#idle .count").textContent = `(${idle.length})`;
  document.querySelector("#idle .cards").replaceChildren(...idle.map(card));
}

function renderWorkspaceBoard() {
  const rows = L.groupByWorkspace(state.agents).map(({ workspace, agents }) => {
    const key = workspace || "";
    const row = el("section", "column ws");
    const heading = el("h2");
    heading.append(badge(workspace || "other"), el("span", "count", String(agents.length)));
    const cards = el("div", "cards");
    cards.append(...agents.filter((a) => groupOf(a) !== "other").map(card));
    row.append(heading, cards);

    // Idle agents fold away; which workspaces have them open survives re-renders.
    const idle = agents.filter((a) => groupOf(a) === "other");
    if (idle.length > 0) {
      const fold = el("details", "ws-idle");
      fold.open = state.openIdle.has(key);
      fold.ontoggle = () => (fold.open ? state.openIdle.add(key) : state.openIdle.delete(key));
      const idleCards = el("div", "cards");
      idleCards.append(...idle.map(card));
      fold.append(el("summary", null, `Idle (${idle.length})`), idleCards);
      row.append(fold);
    }
    return row;
  });
  $("ws-board").replaceChildren(...rows);
}

function listRow(agent) {
  const row = el("button", `f-row ${agent.status}`);
  row.dataset.pane = agent.pane;
  if (agent.pane === state.selected) {
    row.classList.add("selected");
    row.setAttribute("aria-current", "true");
  }
  row.append(el("span", "dot"));
  if (!byWorkspace()) row.append(badge(agent.workspace));
  row.append(el("span", "f-title", displayTitle(agent)));
  row.onclick = () => select(agent.pane, { replace: true });
  return row;
}

function renderList() {
  const groups = byWorkspace() ? workspaceGroups() : GROUPS.flatMap(([group, label]) => {
    const agents = state.agents.filter((a) => groupOf(a) === group);
    if (agents.length === 0) return [];
    return [el("h3", `f-group ${group}`, label), ...agents.map(listRow)];
  });
  $("f-groups").replaceChildren(...groups);
  const selected = document.querySelector(".f-row.selected");
  if (selected) selected.scrollIntoView({ block: "nearest" });
}

function workspaceGroups() {
  return L.groupByWorkspace(state.agents).flatMap(({ workspace, agents }) => {
    const heading = el("h3", "f-group ws");
    heading.append(badge(workspace || "other"), el("span", null, String(agents.length)));
    return [heading, ...agents.map(listRow)];
  });
}

function renderDetail() {
  const agent = current();
  if (!agent) {
    $("d-crumb").replaceChildren();
    $("d-title").textContent = `%${state.selected}`;
    $("d-pill").textContent = state.loaded ? "PANE CLOSED" : "LOADING";
    $("d-pill").className = "pill";
    $("d-meta").textContent = "";
    $("d-facts").replaceChildren();
    $("d-approve").hidden = $("d-deny").hidden = $("d-reply").hidden = true;
    setInteracting(false, false);
    return;
  }

  const boardLink = el("button", "link", "Board");
  boardLink.onclick = back;
  $("d-crumb").replaceChildren(boardLink, el("span", "sep", "›"), badge(agent.workspace), el("span", "sep", "›"), el("span", null, displayTitle(agent)));
  $("d-title").textContent = displayTitle(agent);
  $("d-pill").textContent = pillText(agent);
  $("d-pill").className = `pill ${agent.status}`;
  $("d-meta").textContent = [`%${agent.pane}`, agent.agent, agent.repo, agent.branch].filter(Boolean).join(" · ");

  const rows = [
    ["directory", agent.cwd || "–"],
    ["command", agent.launch.join(" ") || "–"],
  ];
  if (!agent.has_status) rows.unshift(["status", "no status yet (its hooks haven't fired)"]);
  $("d-facts").replaceChildren(...rows.flatMap(([label, value]) => [el("dt", null, label), el("dd", null, value)]));

  // The server refuses approve/deny on questions anyway; hiding them avoids the confusion.
  const canAnswer = agent.status === "waiting" && agent.can_answer && state.permissionPrompt !== false;
  $("d-approve").hidden = $("d-deny").hidden = !canAnswer;
  for (const id of ["d-approve", "d-deny"]) {
    $(id).classList.remove("unverified");
    $(id).title = "";
    if (canAnswer) markUnverified($(id), agent);
  }
  $("d-reply").hidden = !(agent.status === "done" || agent.status === "idle");
}

function pillText(agent) {
  return agent.has_status ? `${statusLabel(agent)} · ${elapsed(agent)}` : "NO STATUS YET";
}

// Elapsed times are computed in the page, so the server only sends real changes.
setInterval(() => {
  for (const node of document.querySelectorAll(".card")) {
    const agent = state.agents.find((a) => a.pane === Number(node.dataset.pane));
    if (agent && agent.has_status) node.querySelector(".elapsed").textContent = elapsed(agent);
  }
  const agent = current();
  if (agent) $("d-pill").textContent = pillText(agent);
}, 1000);

function showPreview(text) {
  const mirror = $("d-mirror");
  const atBottom = mirror.scrollTop + mirror.clientHeight >= mirror.scrollHeight - 4;
  mirror.replaceChildren(...L.linkify(text).map((piece) => {
    if (piece.url === undefined) return piece.text;
    const link = el("a", null, piece.url);
    link.href = piece.url;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    return link;
  }));
  if (atBottom) mirror.scrollTop = mirror.scrollHeight;
}

function showProcs(msg) {
  const procs = msg.procs.map((p) => (p.foreground ? `${p.name}*` : p.name)).join(", ");
  const ports = msg.ports.map((p) => (p.port !== undefined ? p.port : JSON.stringify(p))).join(", ");
  // One line on screen; the full list is in the tooltip.
  const text = `ports: ${ports || "–"}   processes: ${procs || "–"}`;
  $("d-procs").textContent = text;
  $("d-procs").title = text;
}

function act(kind, pane, text) {
  const msg = { type: kind, id: state.nextId++, pane };
  if (text !== undefined) msg.text = text;
  send(msg);
}

// --- closing a pane ---------------------------------------------------

function askCloseConfirm() {
  const agent = current();
  if (!agent) return;
  const where = [agent.agent, agent.workspace, agent.title && `"${agent.title}"`].filter(Boolean).join(" · ");
  const busy = agent.status === "working" || agent.status === "waiting" ? " It's in the middle of a task." : "";
  $("d-confirm-text").textContent = `Close %${agent.pane} · ${where}? This ends the session.${busy}`;
  $("d-confirm").hidden = false;
  $("d-close-pane").hidden = true;
}

function hideCloseConfirm() {
  $("d-confirm").hidden = true;
  $("d-close-pane").hidden = false;
}

// --- keyboard -----------------------------------------------------------

function setInteracting(on, notify) {
  const changed = state.interacting !== on;
  state.interacting = on;
  $("d-interact").checked = on;
  $("detail").classList.toggle("interacting", on);
  $("d-interact-label").hidden = !on;
  if (notify && changed && state.selected !== null) send({ type: "interact", pane: state.selected, on });
}

// Batches keystrokes so fast typing becomes one message instead of one per key.
function flushKeys() {
  flushTimer = null;
  const keys = L.mergeText(pendingKeys);
  pendingKeys = [];
  if (keys.length && state.interacting && state.selected !== null) {
    send({ type: "keys", pane: state.selected, keys });
  }
}

function navigate(event) {
  if (state.selected === null || event.metaKey || event.ctrlKey || event.altKey) return;
  if (event.target.closest("input, textarea")) return;
  const delta = { ArrowDown: 1, j: 1, ArrowUp: -1, k: -1 }[event.key];
  if (delta) {
    event.preventDefault();
    const next = L.nextSelection(visibleOrder(), state.selected, delta);
    if (next !== null) select(next, { replace: true });
  } else if (event.key === "Escape") {
    event.preventDefault();
    back();
  }
}

document.addEventListener("keydown", (event) => {
  // While interacting every key, Esc and arrows included, belongs to the agent.
  if (!state.interacting) {
    navigate(event);
    return;
  }
  const key = L.toKey(event);
  if (!key) return;
  event.preventDefault();
  pendingKeys.push(key);
  if (!flushTimer) flushTimer = setTimeout(flushKeys, 60);
});

// --- wiring -------------------------------------------------------------

for (const button of document.querySelectorAll("#grouping button")) {
  button.onclick = () => setGrouping(button.dataset.grouping);
}
$("open-tty7").onclick = () => send({ type: "open_tty7" });
$("f-back").onclick = back;
$("d-close-pane").onclick = askCloseConfirm;
$("d-confirm-no").onclick = hideCloseConfirm;
$("d-confirm-yes").onclick = () => {
  act("close", state.selected);
  hideCloseConfirm();
};
$("d-approve").onclick = () => act("approve", state.selected);
$("d-deny").onclick = () => act("deny", state.selected);
$("d-reply").onsubmit = (event) => {
  event.preventDefault();
  const input = $("d-reply-text");
  const text = input.value.trim();
  if (!text) return;
  act("reply", state.selected, text);
  input.value = "";
};
$("d-interact").onchange = (event) => {
  setInteracting(event.target.checked, true);
  event.target.blur();
};

state.grouping = localStorage.getItem(GROUPING_KEY) === "workspace" ? "workspace" : "status";
state.selected = L.paneFromHash(location.hash);
render();
connect();
