import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

import { state } from "../src/state.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function withMockInvoke(handler) {
  const previousWindow = globalThis.window;
  globalThis.window = {
    __TAURI__: {
      core: {
        invoke: handler,
      },
    },
  };
  return () => {
    globalThis.window = previousWindow;
  };
}

async function importDataServiceModule() {
  const thisFile = fileURLToPath(import.meta.url);
  const servicesDir = path.resolve(path.dirname(thisFile), "../src/services");
  const dataPath = path.resolve(servicesDir, "data.js");
  const stateUrl = pathToFileURL(path.resolve(servicesDir, "../state.js")).href;
  const apiUrl = pathToFileURL(path.resolve(servicesDir, "../api.js")).href;
  let source = await readFile(dataPath, "utf8");
  source = source.replace(/from\s+"..\/state(?:\.js)?";/g, `from "${stateUrl}";`);
  source = source.replace(/from\s+"..\/api(?:\.js)?";/g, `from "${apiUrl}";`);
  const moduleUrl = `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
  return import(moduleUrl);
}

test("requestlog refresh reuses in-flight request for same query", async () => {
  const { refreshRequestLogs } = await importDataServiceModule();
  const calls = [];
  const pending = deferred();
  const restore = withMockInvoke(async (method, params) => {
    if (method !== "service_requestlog_list") {
      throw new Error(`unexpected method: ${method}`);
    }
    calls.push(params && params.query);
    return pending.promise;
  });

  try {
    state.requestLogList = [];
    const first = refreshRequestLogs("same-query", { latestOnly: false });
    const second = refreshRequestLogs("same-query", { latestOnly: false });
    await new Promise((resolve) => setTimeout(resolve, 5));
    assert.equal(calls.length, 1);

    pending.resolve({ items: [{ id: "one" }] });
    assert.equal(await first, true);
    assert.equal(await second, true);
    assert.equal(state.requestLogList.length, 1);
    assert.equal(state.requestLogList[0].id, "one");
    assert.equal(state.requestLogList[0].__identity, "one");
  } finally {
    restore();
  }
});

test("requestlog refresh keeps latest request effective", async () => {
  const { refreshRequestLogs } = await importDataServiceModule();
  const oldPending = deferred();
  const newPending = deferred();
  const restore = withMockInvoke(async (method, params) => {
    if (method !== "service_requestlog_list") {
      throw new Error(`unexpected method: ${method}`);
    }
    const query = params && params.query;
    if (query === "old") {
      return oldPending.promise;
    }
    if (query === "new") {
      return newPending.promise;
    }
    throw new Error(`unexpected query: ${query}`);
  });

  try {
    state.requestLogList = [];
    const oldRequest = refreshRequestLogs("old");
    const newRequest = refreshRequestLogs("new");

    newPending.resolve({ items: [{ id: "newest" }] });
    assert.equal(await newRequest, true);
    assert.equal(state.requestLogList.length, 1);
    assert.equal(state.requestLogList[0].id, "newest");
    assert.equal(state.requestLogList[0].__identity, "newest");

    oldPending.resolve({ items: [{ id: "stale" }] });
    assert.equal(await oldRequest, false);
    assert.equal(state.requestLogList.length, 1);
    assert.equal(state.requestLogList[0].id, "newest");
    assert.equal(state.requestLogList[0].__identity, "newest");
  } finally {
    restore();
  }
});
