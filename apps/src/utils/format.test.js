import test from "node:test";
import assert from "node:assert/strict";

import { calcAvailability, computeUsageStats, formatTs } from "./format.js";

test("calcAvailability treats missing primary fields as unavailable", () => {
  const usage = {
    usedPercent: null,
    windowMinutes: 300,
    secondaryUsedPercent: 10,
    secondaryWindowMinutes: 10080,
  };
  const result = calcAvailability(usage);
  assert.equal(result.level, "bad");
});

test("calcAvailability treats missing secondary fields as unavailable", () => {
  const usage = {
    usedPercent: 10,
    windowMinutes: 300,
    secondaryUsedPercent: null,
    secondaryWindowMinutes: 10080,
  };
  const result = calcAvailability(usage);
  assert.equal(result.level, "bad");
});

test("calcAvailability keeps ok when both windows present and under limit", () => {
  const usage = {
    usedPercent: 10,
    windowMinutes: 300,
    secondaryUsedPercent: 5,
    secondaryWindowMinutes: 10080,
  };
  const result = calcAvailability(usage);
  assert.equal(result.level, "ok");
});

test("computeUsageStats returns total/ok/unavailable/lowCount in one pass", () => {
  const accounts = [
    { id: "a1" },
    { id: "a2" },
    { id: "a3" },
  ];
  const usageMap = new Map([
    [
      "a1",
      {
        accountId: "a1",
        usedPercent: 10,
        windowMinutes: 300,
        secondaryUsedPercent: 5,
        secondaryWindowMinutes: 10080,
      },
    ],
    [
      "a2",
      {
        accountId: "a2",
        usedPercent: 95,
        windowMinutes: 300,
        secondaryUsedPercent: 50,
        secondaryWindowMinutes: 10080,
      },
    ],
    [
      "a3",
      {
        accountId: "a3",
        usedPercent: 100,
        windowMinutes: 300,
        secondaryUsedPercent: 100,
        secondaryWindowMinutes: 10080,
      },
    ],
  ]);

  const stats = computeUsageStats(accounts, usageMap);
  assert.equal(stats.total, 3);
  assert.equal(stats.okCount, 2);
  assert.equal(stats.unavailableCount, 1);
  assert.equal(stats.lowCount, 2);
});

test("formatTs supports custom empty label", () => {
  assert.equal(formatTs(0, { emptyLabel: "-" }), "-");
  assert.equal(formatTs(null, { emptyLabel: "-" }), "-");
});
