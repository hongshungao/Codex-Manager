import test from "node:test";
import assert from "node:assert/strict";

import { state } from "../state.js";
import { dom } from "../ui/dom.js";
import { renderRequestLogs } from "./requestlogs.js";

class FakeClassList {
  constructor() {
    this.tokens = new Set();
  }

  setFromString(value) {
    this.tokens.clear();
    for (const token of String(value || "").split(/\s+/)) {
      if (token) this.tokens.add(token);
    }
  }

  add(...tokens) {
    for (const token of tokens) {
      if (token) this.tokens.add(token);
    }
  }

  contains(token) {
    return this.tokens.has(token);
  }

  toggle(token, force) {
    if (force === true) {
      this.tokens.add(token);
      return true;
    }
    if (force === false) {
      this.tokens.delete(token);
      return false;
    }
    if (this.tokens.has(token)) {
      this.tokens.delete(token);
      return false;
    }
    this.tokens.add(token);
    return true;
  }

  toString() {
    return [...this.tokens].join(" ");
  }
}

function matchesSelector(node, selector) {
  if (!selector || !node) return false;
  if (selector.startsWith(".")) {
    return node.classList.contains(selector.slice(1));
  }
  const tagWithClass = selector.match(/^([a-zA-Z0-9_-]+)\.([a-zA-Z0-9_-]+)$/);
  if (tagWithClass) {
    return (
      node.tagName === tagWithClass[1].toUpperCase() &&
      node.classList.contains(tagWithClass[2])
    );
  }
  return node.tagName === selector.toUpperCase();
}

class FakeFragment {
  constructor() {
    this.nodeType = 11;
    this.children = [];
  }

  appendChild(node) {
    this.children.push(node);
    return node;
  }
}

class FakeElement {
  constructor(tagName = "div") {
    this.nodeType = 1;
    this.tagName = tagName.toUpperCase();
    this.children = [];
    this.parentElement = null;
    this.dataset = {};
    this.style = {};
    this.hidden = false;
    this.colSpan = 1;
    this.type = "";
    this.title = "";
    this.handlers = new Map();
    this.classList = new FakeClassList();
    this._className = "";
    this._textContent = "";
    this.scrollTop = 0;
    this.clientHeight = 0;
    this.scrollHeight = 0;
  }

  set className(value) {
    this._className = String(value || "");
    this.classList.setFromString(this._className);
  }

  get className() {
    return this.classList.toString();
  }

  set textContent(value) {
    this._textContent = String(value ?? "");
    for (const child of this.children) {
      child.parentElement = null;
    }
    this.children = [];
  }

  get textContent() {
    if (this.children.length === 0) {
      return this._textContent;
    }
    return this._textContent + this.children.map((child) => child.textContent).join("");
  }

  set innerHTML(value) {
    this._textContent = String(value || "");
    for (const child of this.children) {
      child.parentElement = null;
    }
    this.children = [];
  }

  get innerHTML() {
    return this._textContent;
  }

  appendChild(node) {
    if (!node) return node;
    if (node.nodeType === 11) {
      while (node.children.length > 0) {
        this.appendChild(node.children.shift());
      }
      return node;
    }
    if (node.parentElement) {
      node.parentElement.removeChild(node);
    }
    node.parentElement = this;
    this.children.push(node);
    return node;
  }

  removeChild(node) {
    const index = this.children.indexOf(node);
    if (index >= 0) {
      this.children.splice(index, 1);
      node.parentElement = null;
    }
    return node;
  }

  remove() {
    if (!this.parentElement) return;
    this.parentElement.removeChild(this);
  }

  addEventListener(type, handler) {
    if (!this.handlers.has(type)) {
      this.handlers.set(type, new Set());
    }
    this.handlers.get(type).add(handler);
  }

  removeEventListener(type, handler) {
    if (!this.handlers.has(type)) return;
    this.handlers.get(type).delete(handler);
  }

  getListenerCount(type) {
    return this.handlers.has(type) ? this.handlers.get(type).size : 0;
  }

  dispatch(type, event = {}) {
    if (!this.handlers.has(type)) return undefined;
    const payload = { ...event };
    if (!payload.target) {
      payload.target = this;
    }
    const results = [];
    for (const handler of this.handlers.get(type)) {
      results.push(handler(payload));
    }
    if (results.some((item) => item && typeof item.then === "function")) {
      return Promise.all(results);
    }
    return results;
  }

  contains(node) {
    if (node === this) return true;
    for (const child of this.children) {
      if (child.contains(node)) return true;
    }
    return false;
  }

  closest(selector) {
    let current = this;
    while (current) {
      if (matchesSelector(current, selector)) return current;
      current = current.parentElement;
    }
    return null;
  }

  getBoundingClientRect() {
    if (this.tagName === "TR") {
      return { height: 54 };
    }
    return { height: 0 };
  }

  get offsetHeight() {
    return this.getBoundingClientRect().height;
  }
}

class FakeDocument {
  createElement(tagName) {
    return new FakeElement(tagName);
  }

  createDocumentFragment() {
    return new FakeFragment();
  }
}

function createRequestLogLayout() {
  const wrap = new FakeElement("div");
  wrap.className = "table-wrap requestlog-wrap";
  wrap.clientHeight = 320;
  wrap.scrollHeight = 3000;
  const table = new FakeElement("table");
  const rows = new FakeElement("tbody");
  wrap.appendChild(table);
  table.appendChild(rows);
  return { wrap, rows };
}

function makeLog(index, statusCode = 200) {
  return {
    createdAt: 1735689600000 + index * 1000,
    accountLabel: `账号${index}`,
    accountId: `acc-${index}`,
    keyId: `key-${index}`,
    method: "GET",
    requestPath: `/v1/request/${index}`,
    model: "gpt-5",
    reasoningEffort: "medium",
    statusCode,
    error: statusCode >= 500 ? "server error" : "",
  };
}

function getDataRows(rowsEl) {
  return rowsEl.children.filter((child) => child?.dataset?.logRow === "1");
}

function findPathCopyButton(node) {
  if (!node) return null;
  if (node.tagName === "BUTTON" && node.classList.contains("path-copy")) {
    return node;
  }
  for (const child of node.children) {
    const found = findPathCopyButton(child);
    if (found) return found;
  }
  return null;
}

test("renderRequestLogs keeps status filter behavior", () => {
  const previousDocument = globalThis.document;
  const previousRowsEl = dom.requestLogRows;
  const previousList = state.requestLogList;
  const previousAccounts = state.accountList;
  const previousFilter = state.requestLogStatusFilter;
  globalThis.document = new FakeDocument();
  const { rows } = createRequestLogLayout();
  dom.requestLogRows = rows;
  state.requestLogList = [makeLog(1, 200), makeLog(2, 500), makeLog(3, 503), makeLog(4, 404)];
  state.accountList = [{ id: "acc-2", label: "prospergao@126.com" }];
  state.requestLogStatusFilter = "5xx";

  try {
    renderRequestLogs();
    const renderedRows = getDataRows(rows);
    assert.equal(renderedRows.length, 2);
    assert.equal(renderedRows[0].children[7].textContent, "500");
    assert.equal(renderedRows[1].children[7].textContent, "503");
    assert.equal(renderedRows[0].children[1].textContent, "账号2");
  } finally {
    globalThis.document = previousDocument;
    dom.requestLogRows = previousRowsEl;
    state.requestLogList = previousList;
    state.accountList = previousAccounts;
    state.requestLogStatusFilter = previousFilter;
  }
});

test("renderRequestLogs empty state keeps new column span", () => {
  const previousDocument = globalThis.document;
  const previousRowsEl = dom.requestLogRows;
  const previousList = state.requestLogList;
  const previousFilter = state.requestLogStatusFilter;
  globalThis.document = new FakeDocument();
  const { rows } = createRequestLogLayout();
  dom.requestLogRows = rows;
  state.requestLogStatusFilter = "all";
  state.requestLogList = [];

  try {
    renderRequestLogs();
    const firstRow = rows.children[0];
    assert.ok(firstRow);
    assert.equal(firstRow.children[0].colSpan, 9);
  } finally {
    globalThis.document = previousDocument;
    dom.requestLogRows = previousRowsEl;
    state.requestLogList = previousList;
    state.requestLogStatusFilter = previousFilter;
  }
});

test("renderRequestLogs renders first batch and appends on scroll", () => {
  const previousDocument = globalThis.document;
  const previousRowsEl = dom.requestLogRows;
  const previousList = state.requestLogList;
  const previousFilter = state.requestLogStatusFilter;
  globalThis.document = new FakeDocument();
  const { wrap, rows } = createRequestLogLayout();
  dom.requestLogRows = rows;
  state.requestLogStatusFilter = "all";
  state.requestLogList = Array.from({ length: 260 }, (_, index) => makeLog(index, 200));

  try {
    renderRequestLogs();
    const firstPassCount = getDataRows(rows).length;
    assert.ok(firstPassCount > 0);
    assert.ok(firstPassCount < state.requestLogList.length);

    wrap.scrollTop = 2800;
    wrap.dispatch("scroll", { target: wrap });
    const afterScrollCount = getDataRows(rows).length;
    assert.ok(afterScrollCount > firstPassCount);
  } finally {
    globalThis.document = previousDocument;
    dom.requestLogRows = previousRowsEl;
    state.requestLogList = previousList;
    state.requestLogStatusFilter = previousFilter;
  }
});

test("renderRequestLogs recycles head rows with top spacer", () => {
  const previousDocument = globalThis.document;
  const previousRowsEl = dom.requestLogRows;
  const previousList = state.requestLogList;
  const previousFilter = state.requestLogStatusFilter;
  globalThis.document = new FakeDocument();
  const { wrap, rows } = createRequestLogLayout();
  dom.requestLogRows = rows;
  state.requestLogStatusFilter = "all";
  state.requestLogList = Array.from({ length: 520 }, (_, index) => makeLog(index, 200));

  try {
    renderRequestLogs();
    for (let i = 0; i < 6; i += 1) {
      wrap.scrollTop = 2800;
      wrap.dispatch("scroll", { target: wrap });
    }

    const renderedRows = getDataRows(rows);
    assert.ok(renderedRows.length <= 240);
    const minIndex = Math.min(...renderedRows.map((row) => Number(row.dataset.logIndex)));
    assert.ok(minIndex > 0);
    const spacerRow = rows.children.find((child) => child?.dataset?.spacerTop === "1");
    assert.ok(spacerRow);
    const spacerHeight = Number.parseInt(spacerRow.children[0].style.height || "0", 10);
    assert.ok(spacerHeight > 0);
  } finally {
    globalThis.document = previousDocument;
    dom.requestLogRows = previousRowsEl;
    state.requestLogList = previousList;
    state.requestLogStatusFilter = previousFilter;
  }
});

test("renderRequestLogs avoids rebuild when filter/data are unchanged", () => {
  const previousDocument = globalThis.document;
  const previousRowsEl = dom.requestLogRows;
  const previousList = state.requestLogList;
  const previousFilter = state.requestLogStatusFilter;
  globalThis.document = new FakeDocument();
  const { rows } = createRequestLogLayout();
  dom.requestLogRows = rows;
  state.requestLogStatusFilter = "all";
  state.requestLogList = Array.from({ length: 60 }, (_, index) => makeLog(index, 200));

  try {
    renderRequestLogs();
    const firstPassRows = getDataRows(rows);
    const firstRowRef = firstPassRows[0];
    const spacerRef = rows.children.find((child) => child?.dataset?.spacerTop === "1");

    renderRequestLogs();
    const secondPassRows = getDataRows(rows);
    const secondSpacerRef = rows.children.find((child) => child?.dataset?.spacerTop === "1");

    assert.equal(secondPassRows.length, firstPassRows.length);
    assert.equal(secondPassRows[0], firstRowRef);
    assert.equal(secondSpacerRef, spacerRef);
  } finally {
    globalThis.document = previousDocument;
    dom.requestLogRows = previousRowsEl;
    state.requestLogList = previousList;
    state.requestLogStatusFilter = previousFilter;
  }
});

test("renderRequestLogs appends new tail logs without rebuilding existing rows", () => {
  const previousDocument = globalThis.document;
  const previousRowsEl = dom.requestLogRows;
  const previousList = state.requestLogList;
  const previousFilter = state.requestLogStatusFilter;
  globalThis.document = new FakeDocument();
  const { rows } = createRequestLogLayout();
  dom.requestLogRows = rows;
  state.requestLogStatusFilter = "all";
  state.requestLogList = Array.from({ length: 60 }, (_, index) => makeLog(index, 200));

  try {
    renderRequestLogs();
    const beforeRows = getDataRows(rows);
    const firstRowRef = beforeRows[0];

    state.requestLogList = [
      ...state.requestLogList,
      ...Array.from({ length: 20 }, (_, index) => makeLog(index + 60, 200)),
    ];
    renderRequestLogs();

    const afterRows = getDataRows(rows);
    assert.equal(afterRows[0], firstRowRef);
    assert.equal(afterRows.length, 80);
  } finally {
    globalThis.document = previousDocument;
    dom.requestLogRows = previousRowsEl;
    state.requestLogList = previousList;
    state.requestLogStatusFilter = previousFilter;
  }
});

test("request log copy uses delegated tbody click handler", async () => {
  const previousDocument = globalThis.document;
  const previousRowsEl = dom.requestLogRows;
  const previousList = state.requestLogList;
  const previousFilter = state.requestLogStatusFilter;
  const previousNavigator = globalThis.navigator;
  let copied = "";
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    writable: true,
    value: {
      clipboard: {
        writeText: async (value) => {
          copied = value;
        },
      },
    },
  });
  globalThis.document = new FakeDocument();
  const { rows } = createRequestLogLayout();
  dom.requestLogRows = rows;
  state.requestLogStatusFilter = "all";
  state.requestLogList = [makeLog(1, 200)];

  try {
    renderRequestLogs();
    const firstRow = getDataRows(rows)[0];
    const copyButton = findPathCopyButton(firstRow);
    assert.ok(copyButton);
    assert.equal(copyButton.getListenerCount("click"), 0);
    assert.equal(rows.getListenerCount("click"), 1);

    await rows.dispatch("click", { target: copyButton });
    assert.equal(copied, "/v1/request/1");
    assert.equal(copyButton.textContent, "已复制");
  } finally {
    Object.defineProperty(globalThis, "navigator", {
      configurable: true,
      writable: true,
      value: previousNavigator,
    });
    globalThis.document = previousDocument;
    dom.requestLogRows = previousRowsEl;
    state.requestLogList = previousList;
    state.requestLogStatusFilter = previousFilter;
  }
});

test("renderRequestLogs resolves account label from account list and composite id", () => {
  const previousDocument = globalThis.document;
  const previousRowsEl = dom.requestLogRows;
  const previousList = state.requestLogList;
  const previousAccounts = state.accountList;
  const previousFilter = state.requestLogStatusFilter;
  globalThis.document = new FakeDocument();
  const { rows } = createRequestLogLayout();
  dom.requestLogRows = rows;
  state.requestLogStatusFilter = "all";
  state.accountList = [
    { id: "auth0|BCfCgLzzLw3FOSaYqN2jDimX::Frank Smith", label: "prospergao@126.com" },
  ];
  state.requestLogList = [
    {
      ...makeLog(10, 200),
      accountLabel: "",
      accountId: "auth0|BCfCgLzzLw3FOSaYqN2jDimX::Frank Smith",
    },
    {
      ...makeLog(11, 200),
      accountLabel: "",
      accountId: "auth0|fallback-only::Frank Smith",
    },
  ];

  try {
    renderRequestLogs();
    const renderedRows = getDataRows(rows);
    assert.equal(renderedRows[0].children[1].textContent, "prospergao@126.com");
    assert.equal(renderedRows[1].children[1].textContent, "Frank Smith");
  } finally {
    globalThis.document = previousDocument;
    dom.requestLogRows = previousRowsEl;
    state.requestLogList = previousList;
    state.accountList = previousAccounts;
    state.requestLogStatusFilter = previousFilter;
  }
});
