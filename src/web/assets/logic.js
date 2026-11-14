"use strict";

// Pure functions shared by the page and the Node tests (tests/web/). No DOM access here.
(function (root) {
  const NAMED_KEYS = {
    Enter: "enter", Escape: "escape", Tab: "tab", Backspace: "backspace", Delete: "delete",
    ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right",
    Home: "home", End: "end", PageUp: "pageup", PageDown: "pagedown",
  };

  /** A stable colour per workspace name, so the same workspace looks the same everywhere. */
  function workspaceColor(name) {
    let hash = 0;
    for (const char of name) hash = (hash * 31 + char.codePointAt(0)) >>> 0;
    const hue = hash % 360;
    return { background: `hsl(${hue} 35% 22%)`, color: `hsl(${hue} 80% 78%)` };
  }

  /** Same format as the TUI: `41s`, `5m02s`, `1h04m`, with `≥` when only a lower bound is known. */
  function elapsed(agent, now) {
    if (!agent.has_status) return "no status yet";
    const s = Math.max(0, Math.floor((now - agent.since_ms) / 1000));
    const pad = (n) => String(n).padStart(2, "0");
    const body = s < 60 ? `${s}s`
      : s < 3600 ? `${Math.floor(s / 60)}m${pad(s % 60)}s`
      : `${Math.floor(s / 3600)}h${pad(Math.floor((s % 3600) / 60))}m`;
    return agent.exact ? body : `≥${body}`;
  }

  /** The pane `delta` steps away in display order; the first one when `current` isn't listed. */
  function nextSelection(agents, current, delta) {
    if (agents.length === 0) return null;
    const index = agents.findIndex((a) => a.pane === current);
    if (index === -1) return agents[0].pane;
    const next = Math.min(agents.length - 1, Math.max(0, index + delta));
    return agents[next].pane;
  }

  /** One group per workspace, case-insensitive alphabetical, agents without one last (`null`).
   *  Keeps the given order inside each group, so the server's status order carries over. */
  function groupByWorkspace(agents) {
    const groups = new Map();
    for (const agent of agents) {
      const workspace = agent.workspace || null;
      if (!groups.has(workspace)) groups.set(workspace, []);
      groups.get(workspace).push(agent);
    }
    return [...groups]
      .map(([workspace, members]) => ({ workspace, agents: members }))
      .sort((a, b) => {
        if (a.workspace === null || b.workspace === null) return (a.workspace === null) - (b.workspace === null);
        return a.workspace.localeCompare(b.workspace, undefined, { sensitivity: "base" });
      });
  }

  function paneFromHash(hash) {
    const match = /^#pane-(\d+)$/.exec(hash);
    return match ? Number(match[1]) : null;
  }

  function hashForPane(pane) {
    return pane === null ? "" : `#pane-${pane}`;
  }

  /** Maps a keydown to what tty7 understands, or `null` for keys the browser should keep. */
  function toKey(event) {
    if (event.key === "Tab" && event.shiftKey) return { kind: "key", value: "backtab" };
    if (NAMED_KEYS[event.key]) return { kind: "key", value: NAMED_KEYS[event.key] };
    if ((event.ctrlKey || event.altKey) && !event.metaKey && /^[a-z0-9]$/i.test(event.key)) {
      return { kind: "key", value: `${event.ctrlKey ? "C" : "M"}-${event.key.toLowerCase()}` };
    }
    // Leave browser shortcuts (Cmd+R, Cmd+C, …) alone.
    if (event.metaKey || event.ctrlKey || event.altKey) return null;
    if ([...event.key].length === 1) return { kind: "text", value: event.key };
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

  /** Splits screen text into `{ text }` and `{ url }` pieces, so the mirror can make URLs clickable.
   *  Trailing punctuation and a closing bracket without its opener stay in the text. */
  function linkify(text) {
    const pieces = [];
    const push = (piece) => {
      const last = pieces[pieces.length - 1];
      if (piece.text !== undefined && last && last.text !== undefined) last.text += piece.text;
      else if (piece.text !== "") pieces.push(piece);
    };
    let from = 0;
    for (const match of text.matchAll(/https?:\/\/[^\s<>"'`]+/g)) {
      let url = match[0];
      for (;;) {
        const tail = url[url.length - 1];
        if (/[.,;:!?]/.test(tail)) url = url.slice(0, -1);
        else if (tail === ")" && url.split("(").length < url.split(")").length) url = url.slice(0, -1);
        else if (tail === "]" && url.split("[").length < url.split("]").length) url = url.slice(0, -1);
        else break;
      }
      if (/^https?:\/\/$/.test(url)) continue;
      push({ text: text.slice(from, match.index) });
      push({ url });
      from = match.index + url.length;
    }
    push({ text: text.slice(from) });
    return pieces;
  }

  /** "researcher · Locating prompts · 3m57s" per running background agent. */
  function backgroundLines(agent, now) {
    return (agent.background || []).map((task) =>
      [task.kind, task.description, elapsed({ has_status: true, exact: true, since_ms: task.since_ms }, now)]
        .filter(Boolean).join(" · "));
  }

  const api = { workspaceColor, elapsed, backgroundLines, nextSelection, groupByWorkspace, paneFromHash, hashForPane, toKey, mergeText, linkify };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else root.ShipherdLogic = api;
})(this);
