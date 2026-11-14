"use strict";
// Run with `node --test tests/web/*.test.js`.
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../../src/web/assets/logic.js");

test("workspace colours are stable per name and differ between names", () => {
  const a = logic.workspaceColor("PR Reviews");
  assert.deepEqual(a, logic.workspaceColor("PR Reviews"));
  assert.notDeepEqual(a, logic.workspaceColor("Default"));
  assert.match(a.background, /^hsl\(\d+ 35% 22%\)$/);
  assert.match(a.color, /^hsl\(\d+ 80% 78%\)$/);
});

test("elapsed time matches the TUI and handles unknown status", () => {
  const now = 1_000_000;
  assert.equal(logic.elapsed({ has_status: true, exact: true, since_ms: now - 41_000 }, now), "41s");
  assert.equal(logic.elapsed({ has_status: true, exact: true, since_ms: now - 302_000 }, now), "5m02s");
  assert.equal(logic.elapsed({ has_status: true, exact: false, since_ms: now - 3_840_000 }, now), "≥1h04m");
  assert.equal(logic.elapsed({ has_status: false, exact: false, since_ms: now }, now), "no status yet");
});

test("the selection moves through the list and stops at its ends", () => {
  const agents = [{ pane: 5 }, { pane: 3 }, { pane: 9 }];
  assert.equal(logic.nextSelection(agents, 5, 1), 3);
  assert.equal(logic.nextSelection(agents, 3, -1), 5);
  assert.equal(logic.nextSelection(agents, 9, 1), 9);
  assert.equal(logic.nextSelection(agents, 5, -1), 5);
  assert.equal(logic.nextSelection(agents, 42, 1), 5);
  assert.equal(logic.nextSelection([], 5, 1), null);
});

test("the address holds the focused pane", () => {
  assert.equal(logic.paneFromHash("#pane-48"), 48);
  assert.equal(logic.paneFromHash(""), null);
  assert.equal(logic.paneFromHash("#pane-x"), null);
  assert.equal(logic.paneFromHash("#pane-4.5"), null);
  assert.equal(logic.hashForPane(48), "#pane-48");
  assert.equal(logic.hashForPane(null), "");
});

const key = (k, mods = {}) => ({ key: k, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, ...mods });

test("browser keys become tty7 keys", () => {
  assert.deepEqual(logic.toKey(key("Enter")), { kind: "key", value: "enter" });
  assert.deepEqual(logic.toKey(key("Tab", { shiftKey: true })), { kind: "key", value: "backtab" });
  assert.deepEqual(logic.toKey(key("c", { ctrlKey: true })), { kind: "key", value: "C-c" });
  assert.deepEqual(logic.toKey(key("x", { altKey: true })), { kind: "key", value: "M-x" });
  assert.deepEqual(logic.toKey(key("ä")), { kind: "text", value: "ä" });
  assert.equal(logic.toKey(key("r", { metaKey: true })), null);
  assert.equal(logic.toKey(key("Shift")), null);
});

test("consecutive text keys are merged", () => {
  const merged = logic.mergeText([
    { kind: "text", value: "h" },
    { kind: "text", value: "i" },
    { kind: "key", value: "enter" },
    { kind: "text", value: "!" },
  ]);
  assert.deepEqual(merged, [
    { kind: "text", value: "hi" },
    { kind: "key", value: "enter" },
    { kind: "text", value: "!" },
  ]);
});
