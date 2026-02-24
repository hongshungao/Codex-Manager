import test from "node:test";
import assert from "node:assert/strict";

import { createNavigationHandlers } from "./navigation.js";

function createToggleNode() {
  const calls = [];
  return {
    calls,
    classList: {
      toggle(className, active) {
        calls.push({ className, active });
      },
    },
  };
}

function countToggleCalls(nodes) {
  return nodes.reduce((sum, node) => sum + node.calls.length, 0);
}

test("switchPage short-circuits same page and only activates on real page changes", () => {
  const navDashboard = createToggleNode();
  const navAccounts = createToggleNode();
  const navApiKeys = createToggleNode();
  const navRequestLogs = createToggleNode();
  const pageDashboard = createToggleNode();
  const pageAccounts = createToggleNode();
  const pageApiKeys = createToggleNode();
  const pageRequestLogs = createToggleNode();
  const toggleNodes = [
    navDashboard,
    navAccounts,
    navApiKeys,
    navRequestLogs,
    pageDashboard,
    pageAccounts,
    pageApiKeys,
    pageRequestLogs,
  ];

  const state = { currentPage: "dashboard", requestLogStatusFilter: "all" };
  const dom = {
    navDashboard,
    navAccounts,
    navApiKeys,
    navRequestLogs,
    pageDashboard,
    pageAccounts,
    pageApiKeys,
    pageRequestLogs,
    pageTitle: { textContent: "" },
  };
  const activatedPages = [];
  let closeThemePanelCount = 0;

  const { switchPage } = createNavigationHandlers({
    state,
    dom,
    closeThemePanel: () => {
      closeThemePanelCount += 1;
    },
    onPageActivated: (page) => {
      activatedPages.push(page);
    },
  });

  switchPage("dashboard");
  assert.equal(countToggleCalls(toggleNodes), 0);
  assert.equal(closeThemePanelCount, 1);
  assert.deepEqual(activatedPages, []);

  switchPage("accounts");
  assert.equal(state.currentPage, "accounts");
  assert.equal(closeThemePanelCount, 2);
  assert.deepEqual(activatedPages, ["accounts"]);
  assert.equal(dom.pageTitle.textContent, "账号管理");
  assert.ok(countToggleCalls(toggleNodes) > 0);
});
