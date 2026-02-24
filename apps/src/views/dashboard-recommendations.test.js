import test from "node:test";
import assert from "node:assert/strict";

import { pickBestRecommendations } from "./dashboard-recommendations.js";

test("pickBestRecommendations finds 5h/7d best accounts in one scan result", () => {
  const accounts = [
    { id: "a1", label: "one" },
    { id: "a2", label: "two" },
    { id: "a3", label: "three" },
  ];
  const usageMap = new Map([
    ["a1", { usedPercent: 20, secondaryUsedPercent: 60 }],
    ["a2", { usedPercent: 40, secondaryUsedPercent: 10 }],
    ["a3", { usedPercent: 70, secondaryUsedPercent: 30 }],
  ]);

  const { primaryPick, secondaryPick } = pickBestRecommendations(accounts, usageMap);
  assert.equal(primaryPick?.account.id, "a1");
  assert.equal(primaryPick?.remain, 80);
  assert.equal(secondaryPick?.account.id, "a2");
  assert.equal(secondaryPick?.remain, 90);
});

test("pickBestRecommendations keeps first account when remain ties", () => {
  const accounts = [
    { id: "a1", label: "one" },
    { id: "a2", label: "two" },
  ];
  const usageMap = new Map([
    ["a1", { usedPercent: 20, secondaryUsedPercent: 20 }],
    ["a2", { usedPercent: 20, secondaryUsedPercent: 20 }],
  ]);

  const { primaryPick, secondaryPick } = pickBestRecommendations(accounts, usageMap);
  assert.equal(primaryPick?.account.id, "a1");
  assert.equal(secondaryPick?.account.id, "a1");
});
