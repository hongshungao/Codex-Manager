import * as api from "../api.js";
import { createAbortError, isAbortError, sleep } from "../utils/request.js";

function parseCallbackUrl(raw) {
  const value = String(raw || "").trim();
  if (!value) {
    return { error: "请粘贴回调链接" };
  }
  let url;
  try {
    url = new URL(value);
  } catch (_err) {
    try {
      url = new URL(`http://${value}`);
    } catch (_error) {
      return { error: "回调链接格式不正确" };
    }
  }
  const code = url.searchParams.get("code");
  const state = url.searchParams.get("state");
  if (!code || !state) {
    return { error: "回调链接缺少 code/state" };
  }
  const redirectUri = `${url.origin}${url.pathname}`;
  return { code, state, redirectUri };
}

async function waitForLogin(loginId, { dom, signal, api: apiClient }) {
  if (!loginId) return false;
  if (signal && signal.aborted) {
    throw createAbortError();
  }
  const deadline = Date.now() + 2 * 60 * 1000;
  while (Date.now() < deadline) {
    const res = await apiClient.serviceLoginStatus(loginId, {
      signal,
      timeoutMs: 6000,
    });
    if (res && res.status === "success") return true;
    if (res && res.status === "failed") {
      dom.loginHint.textContent = `登录失败：${res.error || "unknown"}`;
      return false;
    }
    await sleep(1500, signal);
  }
  dom.loginHint.textContent = "登录超时，请重试。";
  return false;
}

export function createLoginFlow({
  dom,
  state,
  withButtonBusy,
  ensureConnected,
  refreshAll,
  closeAccountModal,
  api: apiClient = api,
}) {
  let activeLoginAbortController = null;

  function abortActiveLogin() {
    if (activeLoginAbortController) {
      activeLoginAbortController.abort();
      activeLoginAbortController = null;
    }
    state.activeLoginId = null;
  }

  async function handleLogin() {
    abortActiveLogin();
    const controller = new AbortController();
    activeLoginAbortController = controller;
    await withButtonBusy(dom.submitLogin, "授权中...", async () => {
      const ok = await ensureConnected();
      if (!ok) return;
      dom.loginUrl.value = "生成授权链接中...";
      try {
        const res = await apiClient.serviceLoginStart({
          loginType: "chatgpt",
          openBrowser: false,
          note: dom.inputNote.value.trim(),
          tags: dom.inputTags.value.trim(),
          groupName: dom.inputGroup.value.trim(),
        });
        if (controller.signal.aborted) {
          return;
        }
        if (res && res.error) {
          dom.loginHint.textContent = `登录失败：${res.error}`;
          dom.loginUrl.value = "";
          return;
        }
        dom.loginUrl.value = res && res.authUrl ? res.authUrl : "";
        if (res && res.authUrl) {
          await apiClient.openInBrowser(res.authUrl);
          if (controller.signal.aborted) {
            return;
          }
          if (res.warning) {
            dom.loginHint.textContent = `注意：${res.warning}。如无法回调，可在下方粘贴回调链接手动解析。`;
          } else {
            dom.loginHint.textContent = "已打开浏览器，请完成授权。";
          }
        } else {
          dom.loginHint.textContent = "未获取到授权链接，请重试。";
        }
        if (controller.signal.aborted) {
          return;
        }
        state.activeLoginId = res && res.loginId ? res.loginId : null;
        const success = await waitForLogin(state.activeLoginId, {
          dom,
          signal: controller.signal,
          api: apiClient,
        });
        if (success) {
          await refreshAll();
          closeAccountModal();
        } else {
          dom.loginHint.textContent = "登录失败，请重试。";
        }
      } catch (err) {
        if (isAbortError(err)) {
          return;
        }
        dom.loginUrl.value = "";
        dom.loginHint.textContent = "登录失败，请检查 service 状态。";
      } finally {
        if (activeLoginAbortController === controller) {
          activeLoginAbortController = null;
        }
      }
    });
  }

  function handleCancelLogin() {
    abortActiveLogin();
  }

  async function handleManualCallback() {
    const parsed = parseCallbackUrl(dom.manualCallbackUrl.value);
    if (parsed.error) {
      dom.loginHint.textContent = parsed.error;
      return;
    }
    await withButtonBusy(dom.manualCallbackSubmit, "解析中...", async () => {
      const ok = await ensureConnected();
      if (!ok) return;
      dom.loginHint.textContent = "解析回调中...";
      try {
        const res = await apiClient.serviceLoginComplete(
          parsed.state,
          parsed.code,
          parsed.redirectUri,
        );
        if (res && res.ok) {
          dom.loginHint.textContent = "登录成功，正在刷新...";
          await refreshAll();
          closeAccountModal();
          return;
        }
        const msg = res && res.error ? res.error : "解析失败";
        dom.loginHint.textContent = `登录失败：${msg}`;
      } catch (err) {
        dom.loginHint.textContent = `登录失败：${String(err)}`;
      }
    });
  }

  return {
    handleLogin,
    handleCancelLogin,
    handleManualCallback,
  };
}
