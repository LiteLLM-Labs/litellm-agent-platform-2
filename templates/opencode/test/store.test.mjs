import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createStore } from "../src/store.mjs";

function tmpStore() {
  const dir = mkdtempSync(join(tmpdir(), "oc-store-test-"));
  const store = createStore(join(dir, "test.db"));
  return { store, cleanup: () => rmSync(dir, { recursive: true }) };
}

test("insertSessionEvent with same event_id is idempotent", () => {
  const { store, cleanup } = tmpStore();
  try {
    store.insertSessionEvent("ses1", { event: "agent.message", data: { content: [] } }, "ev_abc");
    store.insertSessionEvent("ses1", { event: "agent.message", data: { content: [] } }, "ev_abc");
    const events = store.listSessionEvents("ses1");
    assert.equal(events.length, 1);
  } finally {
    cleanup();
  }
});

test("insertSessionEvent without event_id always appends", () => {
  const { store, cleanup } = tmpStore();
  try {
    store.insertSessionEvent("ses1", { event: "user.message", data: {} }, null);
    store.insertSessionEvent("ses1", { event: "user.message", data: {} }, null);
    const events = store.listSessionEvents("ses1");
    assert.equal(events.length, 2);
  } finally {
    cleanup();
  }
});

test("listSessionEvents returns events in insertion order", () => {
  const { store, cleanup } = tmpStore();
  try {
    store.insertSessionEvent("ses1", { event: "user.message", data: { n: 1 } }, null);
    store.insertSessionEvent("ses1", { event: "agent.message", data: { n: 2 } }, "ev_1");
    store.insertSessionEvent("ses1", { event: "session.status_idle", data: { n: 3 } }, "ev_2");
    const events = store.listSessionEvents("ses1");
    assert.equal(events.length, 3);
    assert.equal(events[0].event, "user.message");
    assert.equal(events[1].event, "agent.message");
    assert.equal(events[2].event, "session.status_idle");
  } finally {
    cleanup();
  }
});

test("listSessionEvents scoped to session", () => {
  const { store, cleanup } = tmpStore();
  try {
    store.insertSessionEvent("ses1", { event: "agent.message", data: {} }, "ev_a");
    store.insertSessionEvent("ses2", { event: "agent.message", data: {} }, "ev_b");
    assert.equal(store.listSessionEvents("ses1").length, 1);
    assert.equal(store.listSessionEvents("ses2").length, 1);
    assert.equal(store.listSessionEvents("ses3").length, 0);
  } finally {
    cleanup();
  }
});
