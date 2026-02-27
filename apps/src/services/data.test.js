import test from "node:test";
import assert from "node:assert/strict";

import { state } from "../state.js";
import { refreshRequestLogs } from "./data.js";

function deferred() {
  let resolve = null;
  let reject = null;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  // 中文注释：避免某些取消路径下 deferred promise 未被 await 时触发 unhandledRejection。
  promise.catch(() => {});
  return { promise, resolve, reject };
}

test("refreshRequestLogs aborts stale request when query changes", async () => {
  const oldWindow = globalThis.window;
  const oldFetch = globalThis.fetch;
  const first = deferred();
  const second = deferred();
  const seenQueries = [];

  try {
    globalThis.window = {
      __TAURI__: {
        core: {
          invoke: async (method) => {
            if (method === "service_rpc_token") {
              return "test-token";
            }
            throw new Error(`unexpected invoke: ${method}`);
          },
        },
      },
    };
    globalThis.fetch = async (_url, options) => {
      const signal = options && options.signal;
      const query = JSON.parse(options.body).params.query;
      seenQueries.push(query);
      if (query === "old") {
        await first.promise;
        return {
          ok: true,
          json: async () => ({ result: { items: [{ id: "old" }] } }),
        };
      }
      await second.promise;
      return {
        ok: true,
        json: async () => ({ result: { items: [{ id: "new" }] } }),
      };
    };

    state.serviceAddr = "localhost:48760";
    state.requestLogList = [];

    const oldTask = refreshRequestLogs("old", { latestOnly: true });
    await Promise.resolve();
    const newTask = refreshRequestLogs("new", { latestOnly: true });

    first.reject(new DOMException("The operation was aborted.", "AbortError"));
    second.resolve();

    const oldApplied = await oldTask;
    const newApplied = await newTask;

    assert.equal(oldApplied, false);
    assert.equal(newApplied, true);
    assert.ok(seenQueries.includes("new"));
    assert.equal(state.requestLogList.length, 1);
    assert.equal(state.requestLogList[0].id, "new");
    assert.ok(state.requestLogList[0].__identity);
  } finally {
    globalThis.window = oldWindow;
    globalThis.fetch = oldFetch;
  }
});
