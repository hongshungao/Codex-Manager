import test from "node:test";
import assert from "node:assert/strict";

import {
  ensureAutoRefreshTimer,
  runRefreshTasks,
  stopAutoRefreshTimer,
} from "./refresh.js";

test("runRefreshTasks continues when one task fails", async () => {
  const errors = [];
  const results = await runRefreshTasks(
    [
      {
        name: "accounts",
        run: async () => "ok",
      },
      {
        name: "usage",
        run: async () => {
          throw new Error("usage failed");
        },
      },
      {
        name: "models",
        run: async () => "ok",
      },
    ],
    (name, err) => errors.push([name, err && err.message]),
  );

  assert.equal(results.length, 3);
  assert.equal(results[0].status, "fulfilled");
  assert.equal(results[1].status, "rejected");
  assert.equal(results[2].status, "fulfilled");
  assert.deepEqual(errors, [["usage", "usage failed"]]);
});

test("ensureAutoRefreshTimer creates one timer only", async () => {
  const state = { autoRefreshTimer: null };
  let tickCount = 0;

  const started = ensureAutoRefreshTimer(state, async () => {
    tickCount += 1;
  }, 10);
  assert.equal(started, true);
  assert.ok(state.autoRefreshTimer);

  const startedAgain = ensureAutoRefreshTimer(state, async () => {
    tickCount += 1;
  }, 10);
  assert.equal(startedAgain, false);

  await new Promise((resolve) => setTimeout(resolve, 35));
  const stopped = stopAutoRefreshTimer(state);
  assert.equal(stopped, true);
  assert.equal(state.autoRefreshTimer, null);
  assert.ok(tickCount >= 1);
});

test("ensureAutoRefreshTimer skips overlapping ticks", async () => {
  const state = { autoRefreshTimer: null };
  let inFlight = 0;
  let maxInFlight = 0;

  const started = ensureAutoRefreshTimer(state, async () => {
    inFlight += 1;
    maxInFlight = Math.max(maxInFlight, inFlight);
    await new Promise((resolve) => setTimeout(resolve, 25));
    inFlight -= 1;
  }, 5);

  assert.equal(started, true);
  await new Promise((resolve) => setTimeout(resolve, 80));
  stopAutoRefreshTimer(state);
  await new Promise((resolve) => setTimeout(resolve, 40));
  assert.equal(maxInFlight, 1);
});
