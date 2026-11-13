"use strict";

const COLUMNS = ["waiting", "working", "done"];
const STATUS_LABEL = { waiting: "needs you", working: "working", done: "done", idle: "idle", unknown: "unknown" };
const NAMED_KEYS = {
  Enter: "enter", Escape: "escape", Tab: "tab", Backspace: "backspace", Delete: "delete",
  ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right",
  Home: "home", End: "end", PageUp: "pageup", PageDown: "pagedown",
};

// permissionPrompt: what the selected pane's screen shows; null until its first preview arrives.
const state = { agents: [], banners: [], selected: null, interacting: false, nextId: 1, ws: null, permissionPrompt: null };
let pendingKeys = [];
let flushTimer = null;

const $ = (id) => document.getElementById(id);

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = text;
  return node;
}

function elapsed(agent) {
  if (!agent.has_status) return "no status yet";
  const s = Math.max(0, Math.floor((Date.now() - agent.since_ms) / 1000));
  const pad = (n) => String(n).padStart(2, "0");
  const body = s < 60 ? `${s}s`
    : s < 3600 ? `${Math.floor(s / 60)}m${pad(s % 60)}s`
    : `${Math.floor(s / 3600)}h${pad(Math.floor((s % 3600) / 60))}m`;
  return agent.exact ? body : `≥${body}`;
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

// --- board --------------------------------------------------------------

function card(agent) {
  const node = el("article", `card ${agent.status}`);
  node.dataset.pane = agent.pane;
  if (agent.pane === state.selected) node.classList.add("selected");

  const top = el("div", "card-top");
  top.append(el("span", "dot"), el("strong", null, agent.agent), el("span", "elapsed", elapsed(agent)));
  node.append(top, el("div", "where", [agent.workspace, agent.repo, agent.branch].filter(Boolean).join(" · ") || agent.cwd || ""));
  if (agent.title) node.append(el("div", "title", agent.title));

  if (agent.status === "waiting" && agent.can_answer) {
    const row = el("div", "card-actions");
    const approve = el("button", "approve", "Approve");
    approve.onclick = (event) => { event.stopPropagation(); act("approve", agent.pane); };
    const deny = el("button", "deny", "Deny");
    deny.onclick = (event) => { event.stopPropagation(); act("deny", agent.pane); };
    row.append(approve, deny);
    node.append(row);
  }
  node.onclick = () => select(agent.pane);
  return node;
}

function render() {
  const waiting = state.agents.filter((a) => a.status === "waiting").length;
  $("summary").textContent = `${state.agents.length} agents${waiting ? ` · ${waiting} need you` : ""}`;
  document.title = waiting ? `(${waiting}) shipherd` : "shipherd";
  $("banners").replaceChildren(...state.banners.map((b) => el("div", "banner", b)));

  for (const status of COLUMNS) {
    const column = document.querySelector(`.column[data-status="${status}"] .cards`);
    column.replaceChildren(...state.agents.filter((a) => a.status === status).map(card));
  }
  const idle = state.agents.filter((a) => !COLUMNS.includes(a.status));
  document.querySelector("#idle .count").textContent = `(${idle.length})`;
  document.querySelector("#idle .cards").replaceChildren(...idle.map(card));
  renderDetail();
}

// Elapsed times are computed in the page, so the server only sends real changes.
setInterval(() => {
  for (const node of document.querySelectorAll(".card")) {
    const agent = state.agents.find((a) => a.pane === Number(node.dataset.pane));
    if (agent) node.querySelector(".elapsed").textContent = elapsed(agent);
  }
  const agent = current();
  const cell = $("d-elapsed");
  if (agent && cell) cell.textContent = elapsed(agent);
}, 1000);

// --- detail panel -------------------------------------------------------

function select(pane) {
  if (state.selected === pane) return;
  setInteracting(false, true);
  state.selected = pane;
  state.permissionPrompt = null;
  $("d-mirror").textContent = "";
  $("d-procs").textContent = "";
  $("d-result").textContent = "";
  send({ type: "watch", pane });
  render();
}

function closeDetail() {
  setInteracting(false, true);
  state.selected = null;
  send({ type: "watch", pane: null });
  render();
}

function renderDetail() {
  $("detail").hidden = state.selected === null;
  if (state.selected === null) return;

  const agent = current();
  if (!agent) {
    $("d-result").textContent = "pane closed";
    $("d-approve").hidden = $("d-deny").hidden = $("d-reply").hidden = true;
    setInteracting(false, false);
    return;
  }

  $("d-agent").textContent = agent.agent;
  $("d-where").textContent = [agent.repo, agent.branch].filter(Boolean).join(" · ");
  const rows = [
    ["status", agent.has_status ? (STATUS_LABEL[agent.status] || agent.status) : "no status yet (its hooks haven't fired)"],
    ["workspace", agent.workspace || "–"],
    ["in state", elapsed(agent), "d-elapsed"],
    ["directory", agent.cwd || "–"],
    ["command", agent.launch.join(" ") || "–"],
    ["task", agent.title || "–"],
  ];
  $("d-facts").replaceChildren(...rows.flatMap(([label, value, id]) => {
    const dd = el("dd", null, value);
    if (id) dd.id = id;
    return [el("dt", null, label), dd];
  }));

  // The server refuses approve/deny on questions anyway; hiding them avoids the confusion.
  const canAnswer = agent.status === "waiting" && agent.can_answer && state.permissionPrompt !== false;
  $("d-approve").hidden = $("d-deny").hidden = !canAnswer;
  $("d-reply").hidden = !(agent.status === "done" || agent.status === "idle");
}

function showPreview(text) {
  const mirror = $("d-mirror");
  const atBottom = mirror.scrollTop + mirror.clientHeight >= mirror.scrollHeight - 4;
  mirror.textContent = text;
  if (atBottom) mirror.scrollTop = mirror.scrollHeight;
}

function showProcs(msg) {
  const procs = msg.procs.map((p) => (p.foreground ? `${p.name}*` : p.name)).join(", ");
  const ports = msg.ports.map((p) => (p.port !== undefined ? p.port : JSON.stringify(p))).join(", ");
  $("d-procs").textContent = `processes: ${procs || "–"}   ports: ${ports || "–"}`;
}

function act(kind, pane, text) {
  const msg = { type: kind, id: state.nextId++, pane };
  if (text !== undefined) msg.text = text;
  send(msg);
}

// --- interact mode ------------------------------------------------------

function setInteracting(on, notify) {
  const changed = state.interacting !== on;
  state.interacting = on;
  $("d-interact").checked = on;
  $("detail").classList.toggle("interacting", on);
  $("d-interact-label").hidden = !on;
  if (notify && changed && state.selected !== null) send({ type: "interact", pane: state.selected, on });
}

function toKey(event) {
  if (event.key === "Tab" && event.shiftKey) return { kind: "key", value: "backtab" };
  if (NAMED_KEYS[event.key]) return { kind: "key", value: NAMED_KEYS[event.key] };
  if ((event.ctrlKey || event.altKey) && !event.metaKey && /^[a-z0-9]$/i.test(event.key)) {
    return { kind: "key", value: `${event.ctrlKey ? "C" : "M"}-${event.key.toLowerCase()}` };
  }
  // Leave browser shortcuts (Cmd+R, Cmd+C, …) alone.
  if (event.metaKey || event.ctrlKey || event.altKey) return null;
  if (event.key.length === 1) return { kind: "text", value: event.key };
  return null;
}

function mergeText(keys) {
  const merged = [];
  for (const key of keys) {
    const last = merged[merged.length - 1];
    if (key.kind === "text" && last && last.kind === "text") last.value += key.value;
    else merged.push({ ...key });
  }
  return merged;
}

// Batches keystrokes so fast typing becomes one message instead of one per key.
function flushKeys() {
  flushTimer = null;
  const keys = mergeText(pendingKeys);
  pendingKeys = [];
  if (keys.length && state.interacting && state.selected !== null) {
    send({ type: "keys", pane: state.selected, keys });
  }
}

document.addEventListener("keydown", (event) => {
  if (!state.interacting) return;
  const key = toKey(event);
  if (!key) return;
  event.preventDefault();
  pendingKeys.push(key);
  if (!flushTimer) flushTimer = setTimeout(flushKeys, 60);
});

// --- wiring -------------------------------------------------------------

$("open-tty7").onclick = () => send({ type: "open_tty7" });
$("d-close").onclick = closeDetail;
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

connect();
