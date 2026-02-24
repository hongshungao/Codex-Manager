import assert from "node:assert/strict";
import test from "node:test";

import { bindFilterEvents } from "../src/views/event-bindings/filter-events.js";

class FakeElement {
  constructor() {
    this.handlers = new Map();
  }

  addEventListener(type, handler) {
    this.handlers.set(type, handler);
  }

  dispatch(type, event) {
    const handler = this.handlers.get(type);
    if (!handler) return;
    return handler(event);
  }
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

test("account search input uses debounce before rendering", async () => {
  const accountSearch = new FakeElement();
  const state = {
    requestLogQuery: "",
    requestLogStatusFilter: "all",
    accountSearch: "",
    accountFilter: "all",
    accountGroupFilter: "all",
  };
  let renderCount = 0;

  bindFilterEvents({
    dom: {
      refreshRequestLogs: null,
      clearRequestLogs: null,
      requestLogSearch: null,
      filterLogAll: null,
      filterLog2xx: null,
      filterLog4xx: null,
      filterLog5xx: null,
      accountSearch,
      accountGroupFilter: null,
      filterAll: null,
      filterActive: null,
      filterLow: null,
    },
    state,
    handleClearRequestLogs: () => {},
    refreshRequestLogs: async () => true,
    renderRequestLogs: () => {},
    renderAccountsView: () => {
      renderCount += 1;
    },
    updateRequestLogFilterButtons: () => {},
  });

  accountSearch.dispatch("input", { target: { value: "a" } });
  await wait(80);
  accountSearch.dispatch("input", { target: { value: "ab" } });
  await wait(80);
  accountSearch.dispatch("input", { target: { value: "abc" } });
  await wait(120);
  assert.equal(renderCount, 0);

  await wait(140);
  assert.equal(renderCount, 1);
  assert.equal(state.accountSearch, "abc");
});
