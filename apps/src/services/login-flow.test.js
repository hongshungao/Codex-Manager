import test from "node:test";
import assert from "node:assert/strict";

import { withButtonBusy } from "../ui/button-busy.js";
import { createAbortError } from "../utils/request.js";
import { createLoginFlow } from "./login-flow.js";

function createFakeButton() {
  const classes = new Set();
  return {
    textContent: "授权中",
    style: { minWidth: "" },
    dataset: {},
    disabled: false,
    classList: {
      add(name) {
        classes.add(name);
      },
      remove(name) {
        classes.delete(name);
      },
      contains(name) {
        return classes.has(name);
      },
    },
    getBoundingClientRect() {
      return { width: 120 };
    },
  };
}

function createFakeInput(value = "") {
  return { value, textContent: "" };
}

test("handleCancelLogin aborts login polling and clears busy state", async () => {
  const dom = {
    submitLogin: createFakeButton(),
    loginUrl: createFakeInput(""),
    loginHint: createFakeInput(""),
    inputNote: createFakeInput(""),
    inputTags: createFakeInput(""),
    inputGroup: createFakeInput("TEAM"),
    manualCallbackUrl: createFakeInput(""),
  };
  const state = { activeLoginId: null };

  let startCalls = 0;
  let openBrowserCalls = 0;
  const api = {
    serviceLoginStart: async () => {
      startCalls += 1;
      return {
        authUrl: "https://example.com/auth",
        loginId: "login-1",
      };
    },
    openInBrowser: async () => {
      openBrowserCalls += 1;
    },
    serviceLoginStatus: async (_loginId, options = {}) => {
      return new Promise((_, reject) => {
        if (options.signal && options.signal.aborted) {
          reject(createAbortError());
          return;
        }
        const onAbort = () => reject(createAbortError());
        options.signal?.addEventListener("abort", onAbort, { once: true });
      });
    },
    serviceLoginComplete: async () => ({ ok: true }),
  };

  const flow = createLoginFlow({
    dom,
    state,
    api,
    withButtonBusy,
    ensureConnected: async () => true,
    refreshAll: async () => {},
    closeAccountModal: () => {},
  });

  const running = flow.handleLogin();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(dom.submitLogin.dataset.busy, "1");

  flow.handleCancelLogin();
  await running;

  assert.equal(startCalls, 1);
  assert.equal(openBrowserCalls, 1);
  assert.equal(state.activeLoginId, null);
  assert.equal(dom.submitLogin.dataset.busy, "0");
  assert.equal(dom.submitLogin.disabled, false);
  assert.equal(dom.submitLogin.classList.contains("is-loading"), false);
});
