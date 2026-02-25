import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const appsRoot = path.resolve(__dirname, "..");
const indexHtml = fs.readFileSync(path.join(appsRoot, "index.html"), "utf8");

const mustHave = [
  "topNav",
  "navDashboard",
  "navAccounts",
  "navApiKeys",
  "pageDashboard",
  "pageAccounts",
  "pageTitle",
  "metricTotal",
  "metricAvailable",
  "metricUnavailable",
  "metricTodayTokens",
  "metricCachedInputTokens",
  "metricReasoningOutputTokens",
  "metricTodayCost",
  "currentAccountCard",
  "recommendations",
  "accountsToolbar",
  "accountSearch",
];

test("dashboard/accounts structure includes new hooks", () => {
  for (const id of mustHave) {
    assert.ok(indexHtml.includes(`id=\"${id}\"`), `missing id ${id}`);
  }
});

const tableMustContain = [
  'table class="data-table account-table"',
  'table class="data-table api-table"',
  'id="accountRows"',
  'id="apiKeyRows"',
];

test("list pages use table markup", () => {
  for (const snippet of tableMustContain) {
    assert.ok(indexHtml.includes(snippet), `missing ${snippet}`);
  }
});
