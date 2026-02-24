import test from "node:test";
import assert from "node:assert/strict";

import { bindNavigationAndServiceEvents } from "./navigation-service.js";

class FakeElement {
  constructor() {
    this.handlers = new Map();
    this.hidden = true;
  }

  addEventListener(type, handler) {
    this.handlers.set(type, handler);
  }

  dispatch(type, event = {}) {
    const handler = this.handlers.get(type);
    if (!handler) return;
    handler(event);
  }
}

function createFakeDocument() {
  const handlers = new Map();
  const addCalls = [];
  const removeCalls = [];
  return {
    addCalls,
    removeCalls,
    addEventListener(type, handler) {
      addCalls.push(type);
      if (!handlers.has(type)) {
        handlers.set(type, new Set());
      }
      handlers.get(type).add(handler);
    },
    removeEventListener(type, handler) {
      removeCalls.push(type);
      if (!handlers.has(type)) return;
      handlers.get(type).delete(handler);
    },
    dispatch(type, event = {}) {
      if (!handlers.has(type)) return;
      [...handlers.get(type)].forEach((handler) => handler(event));
    },
  };
}

test("theme panel global listeners bind only while panel is open", () => {
  const originalDocument = globalThis.document;
  const fakeDocument = createFakeDocument();
  globalThis.document = fakeDocument;

  const themeToggle = new FakeElement();
  const themePanel = new FakeElement();
  themePanel.hidden = true;

  try {
    bindNavigationAndServiceEvents({
      dom: {
        navDashboard: null,
        navAccounts: null,
        navApiKeys: null,
        navRequestLogs: null,
        refreshAll: null,
        themeToggle,
        themePanel,
        serviceToggleBtn: null,
      },
      switchPage: () => {},
      refreshAll: () => {},
      toggleThemePanel: () => {
        themePanel.hidden = !themePanel.hidden;
      },
      closeThemePanel: () => {
        themePanel.hidden = true;
      },
      setTheme: () => {},
      handleServiceToggle: () => {},
    });

    assert.deepEqual(fakeDocument.addCalls, []);

    themeToggle.dispatch("click", { stopPropagation() {} });
    assert.equal(themePanel.hidden, false);
    assert.deepEqual(fakeDocument.addCalls, ["click", "keydown"]);

    fakeDocument.dispatch("click");
    assert.equal(themePanel.hidden, true);
    assert.deepEqual(fakeDocument.removeCalls, ["click", "keydown"]);
  } finally {
    globalThis.document = originalDocument;
  }
});
