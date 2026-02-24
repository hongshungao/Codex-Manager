import test from "node:test";
import assert from "node:assert/strict";

import { buildGroupFilterOptions, filterAccounts } from "./accounts.js";
import { buildAccountDerivedMap } from "./accounts/state.js";

test("buildGroupFilterOptions returns all option and dynamic groups", () => {
  const accounts = [
    { id: "a", groupName: "TEAM" },
    { id: "b", groupName: "TEAM" },
    { id: "c", groupName: "PERSONAL" },
    { id: "d", groupName: "" },
    { id: "e", groupName: null },
  ];

  const options = buildGroupFilterOptions(accounts);
  assert.equal(options.length, 3);
  assert.deepEqual(options.map((item) => item.value), ["all", "PERSONAL", "TEAM"]);
  assert.equal(options[0].count, 5);
  assert.equal(options[1].count, 1);
  assert.equal(options[2].count, 2);
});

test("filterAccounts supports group and status composite filtering", () => {
  const accounts = [
    { id: "a", label: "alpha", groupName: "TEAM" },
    { id: "b", label: "beta", groupName: "TEAM" },
    { id: "c", label: "gamma", groupName: "PERSONAL" },
  ];
  const usage = [
    { accountId: "a", usedPercent: 10, secondaryUsedPercent: 15, windowMinutes: 300, secondaryWindowMinutes: 10080 },
    { accountId: "b", usedPercent: 90, secondaryUsedPercent: 90, windowMinutes: 300, secondaryWindowMinutes: 10080 },
    { accountId: "c", usedPercent: 20, secondaryUsedPercent: 30, windowMinutes: 300, secondaryWindowMinutes: 10080 },
  ];

  const teamOnly = filterAccounts(accounts, usage, "", "all", "TEAM");
  assert.deepEqual(teamOnly.map((item) => item.id), ["a", "b"]);

  const teamAndActive = filterAccounts(accounts, usage, "", "active", "TEAM");
  assert.deepEqual(teamAndActive.map((item) => item.id), ["a", "b"]);

  const teamKeywordActive = filterAccounts(accounts, usage, "alp", "active", "TEAM");
  assert.deepEqual(teamKeywordActive.map((item) => item.id), ["a"]);
});

test("filterAccounts accepts precomputed derived usage state", () => {
  const accounts = [
    { id: "a", label: "alpha", groupName: "TEAM" },
    { id: "b", label: "beta", groupName: "TEAM" },
  ];
  const usage = [
    { accountId: "a", usedPercent: 85, secondaryUsedPercent: 10, windowMinutes: 300, secondaryWindowMinutes: 10080 },
    { accountId: "b", usedPercent: 40, secondaryUsedPercent: 40, windowMinutes: 300, secondaryWindowMinutes: 10080 },
  ];

  const derived = buildAccountDerivedMap(accounts, usage);
  const low = filterAccounts(accounts, derived, "", "low", "TEAM");
  assert.deepEqual(low.map((item) => item.id), ["a"]);
});



