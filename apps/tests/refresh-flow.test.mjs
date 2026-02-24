import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const appsRoot = path.resolve(__dirname, "..");
const mainJs = fs.readFileSync(path.join(appsRoot, "src", "main.js"), "utf8");

test("main refresh flow uses centralized refresh helpers", () => {
  assert.ok(mainJs.includes("runRefreshTasks"), "main.js should use runRefreshTasks");
  assert.ok(mainJs.includes("ensureAutoRefreshTimer"), "main.js should use ensureAutoRefreshTimer");
  assert.ok(mainJs.includes("stopAutoRefreshTimer"), "main.js should use stopAutoRefreshTimer");
});

test("refreshAll renders current page only", () => {
  const refreshAllStart = mainJs.indexOf("async function refreshAll()");
  const refreshAllEnd = mainJs.indexOf("async function refreshAccountsAndUsage()", refreshAllStart);
  const refreshAllSource = mainJs.slice(refreshAllStart, refreshAllEnd);

  assert.ok(refreshAllSource.includes("renderCurrentPageView"), "refreshAll should render current page");
  assert.ok(!refreshAllSource.includes("renderAllViews"), "refreshAll should avoid full renderAllViews redraw");
});

test("refreshAll uses single-flight guard", () => {
  const refreshAllStart = mainJs.indexOf("async function refreshAll()");
  const refreshAllEnd = mainJs.indexOf("async function refreshAccountsAndUsage()", refreshAllStart);
  const refreshAllSource = mainJs.slice(refreshAllStart, refreshAllEnd);

  assert.ok(mainJs.includes("let refreshAllInFlight = null"), "main.js should define refreshAll single-flight state");
  assert.ok(refreshAllSource.includes("if (refreshAllInFlight)"), "refreshAll should reuse in-flight refresh");
  assert.ok(refreshAllSource.includes("refreshAllInFlight = (async () =>"), "refreshAll should store current run promise");
});
