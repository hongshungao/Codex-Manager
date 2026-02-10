import test from "node:test";
import assert from "node:assert/strict";

import { createConnectionService } from "./connection.js";
import { normalizeAddr } from "./connection.js";

test("normalizeAddr defaults to localhost", () => {
  assert.equal(normalizeAddr("5050"), "localhost:5050");
  assert.equal(normalizeAddr("localhost:5050"), "localhost:5050");
});

test("startService retries initialize before surfacing error", async () => {
  let initCalls = 0;
  const api = {
    serviceStart: async () => {},
    serviceInitialize: async () => {
      initCalls += 1;
      if (initCalls < 3) throw new Error("not ready");
      return { server_name: "gpttools-service", version: "test" };
    },
    serviceStop: async () => {},
  };
  const state = { serviceConnected: false, serviceAddr: "" };
  const statusCalls = [];
  const hintCalls = [];

  const service = createConnectionService({
    api,
    state,
    setStatus: (message, ok) => statusCalls.push([message, ok]),
    setServiceHint: (text, isError) => hintCalls.push([text, isError]),
    wait: async () => {},
  });

  const ok = await service.startService("5050", { retries: 2 });

  assert.equal(ok, true);
  assert.equal(initCalls, 3);
  assert.equal(state.serviceConnected, true);
  assert.equal(state.serviceAddr, "localhost:5050");
  assert.equal(hintCalls.some((item) => item[1] === true), false);
  assert.ok(statusCalls.length > 0);
});

test("startService reports failure after retries are exhausted", async () => {
  let initCalls = 0;
  const api = {
    serviceStart: async () => {},
    serviceInitialize: async () => {
      initCalls += 1;
      throw new Error("down");
    },
    serviceStop: async () => {},
  };
  const state = { serviceConnected: false, serviceAddr: "" };
  const hintCalls = [];

  const service = createConnectionService({
    api,
    state,
    setStatus: () => {},
    setServiceHint: (text, isError) => hintCalls.push([text, isError]),
    wait: async () => {},
  });

  const ok = await service.startService("5050", { retries: 1 });

  assert.equal(ok, false);
  assert.equal(initCalls, 2);
  assert.ok(hintCalls.some((item) => item[1] === true));
});

test("waitForConnection can be silent and succeed after retries", async () => {
  let initCalls = 0;
  const api = {
    serviceInitialize: async () => {
      initCalls += 1;
      if (initCalls < 2) throw new Error("down");
      return { server_name: "gpttools-service", version: "test" };
    },
    serviceStart: async () => {},
    serviceStop: async () => {},
  };
  const state = { serviceConnected: false, serviceAddr: "" };
  const hintCalls = [];

  const service = createConnectionService({
    api,
    state,
    setStatus: () => {},
    setServiceHint: (text, isError) => hintCalls.push([text, isError]),
    wait: async () => {},
  });

  const ok = await service.waitForConnection({ retries: 1, silent: true });

  assert.equal(ok, true);
  assert.equal(initCalls, 2);
  assert.equal(hintCalls.some((item) => item[1] === true), false);
});

test("waitForConnection accepts JSON-RPC wrapped initialize result", async () => {
  const api = {
    serviceInitialize: async () => ({
      jsonrpc: "2.0",
      id: 1,
      result: { server_name: "gpttools-service", version: "test" },
    }),
    serviceStart: async () => {},
    serviceStop: async () => {},
  };
  const state = { serviceConnected: false, serviceAddr: "" };
  const service = createConnectionService({
    api,
    state,
    setStatus: () => {},
    setServiceHint: () => {},
    wait: async () => {},
  });

  const ok = await service.waitForConnection({ retries: 0, silent: true });

  assert.equal(ok, true);
  assert.equal(state.serviceConnected, true);
});

test("waitForConnection shows retry reason even when silent", async () => {
  let initCalls = 0;
  const api = {
    serviceInitialize: async () => {
      initCalls += 1;
      if (initCalls < 3) throw new Error("connection timed out");
      return { server_name: "gpttools-service", version: "test" };
    },
    serviceStart: async () => {},
    serviceStop: async () => {},
  };
  const state = { serviceConnected: false, serviceAddr: "" };
  const hintCalls = [];

  const service = createConnectionService({
    api,
    state,
    setStatus: () => {},
    setServiceHint: (text, isError) => hintCalls.push([text, isError]),
    wait: async () => {},
  });

  const ok = await service.waitForConnection({ retries: 2, silent: true });

  assert.equal(ok, true);
  assert.equal(initCalls, 3);
  assert.ok(hintCalls.some((item) => item[0] && item[0].includes("正在重试：")));
  assert.equal(hintCalls.some((item) => item[1] === true), false);
});
