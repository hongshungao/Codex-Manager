import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/responsive.css";

import { state } from "./state";
import { dom } from "./ui/dom";
import { setStatus, setServiceHint } from "./ui/status";
import * as api from "./api";
import {
  ensureConnected,
  normalizeAddr,
  startService,
  stopService,
  waitForConnection,
} from "./services/connection";
import { refreshAccounts, refreshUsageList, refreshApiKeys } from "./services/data";
import { renderDashboard } from "./views/dashboard";
import { renderAccounts, openAccountModal, closeAccountModal } from "./views/accounts";
import { renderApiKeys, openApiKeyModal, closeApiKeyModal } from "./views/apikeys";
import { openUsageModal, closeUsageModal, renderUsageSnapshot } from "./views/usage";

function switchPage(page) {
  state.currentPage = page;
  dom.navDashboard.classList.toggle("active", page === "dashboard");
  dom.navAccounts.classList.toggle("active", page === "accounts");
  dom.navApiKeys.classList.toggle("active", page === "apikeys");
  dom.pageDashboard.classList.toggle("active", page === "dashboard");
  dom.pageAccounts.classList.toggle("active", page === "accounts");
  dom.pageApiKeys.classList.toggle("active", page === "apikeys");
  dom.pageTitle.textContent =
    page === "dashboard" ? "仪表盘" : page === "accounts" ? "账号管理" : "平台 Key";
}

function updateServiceToggle() {
  if (!dom.serviceToggleBtn) return;
  if (state.serviceBusy) return;
  dom.serviceToggleBtn.textContent = state.serviceConnected ? "停止服务" : "启动服务";
}

function setServiceBusy(busy, mode) {
  state.serviceBusy = busy;
  if (!dom.serviceToggleBtn) return;
  dom.serviceToggleBtn.disabled = busy;
  dom.serviceToggleBtn.classList.toggle("is-loading", busy);
  if (busy) {
    dom.serviceToggleBtn.textContent = mode === "stop" ? "停止中..." : "启动中...";
  } else {
    updateServiceToggle();
  }
}

async function refreshAll() {
  const ok = await ensureConnected();
  updateServiceToggle();
  if (!ok) return;
  await refreshAccounts();
  await refreshUsageList();
  await refreshApiKeys();
  renderDashboard();
  renderAccounts({
    onUpdateSort: updateAccountSort,
    onOpenUsage: handleOpenUsageModal,
    onDelete: deleteAccount,
  });
  renderApiKeys({
    onDisable: disableApiKey,
    onDelete: deleteApiKey,
  });
}

async function updateAccountSort(accountId, sort) {
  const ok = await ensureConnected();
  if (!ok) return;
  await api.serviceAccountUpdate(accountId, sort);
  await refreshAll();
}

async function deleteAccount(account) {
  if (!account || !account.id) return;
  const confirmed = window.confirm(
    `确定删除账号 ${account.label} 吗？删除后不可恢复。`,
  );
  if (!confirmed) return;
  const ok = await ensureConnected();
  if (!ok) return;
  const res = await api.serviceAccountDelete(account.id);
  if (res && res.error === "unknown_method") {
    const fallback = await api.localAccountDelete(account.id);
    if (fallback && fallback.ok) {
      await refreshAll();
      return;
    }
    const msg = fallback && fallback.error ? fallback.error : "删除失败";
    alert(msg);
    return;
  }
  if (res && res.ok) {
    await refreshAll();
  } else {
    const msg = res && res.error ? res.error : "删除失败";
    alert(msg);
  }
}

async function handleOpenUsageModal(account) {
  openUsageModal(account);
  await refreshUsageForAccount();
}

async function refreshUsageForAccount() {
  if (!state.currentUsageAccount) return;
  const ok = await ensureConnected();
  if (!ok) return;
  dom.refreshUsageSingle.disabled = true;
  try {
    await api.serviceUsageRefresh(state.currentUsageAccount.id);
    const res = await api.serviceUsageRead(state.currentUsageAccount.id);
    const snap = res ? res.snapshot : null;
    renderUsageSnapshot(snap);
  } catch (err) {
    dom.usageDetail.textContent = String(err);
  }
  dom.refreshUsageSingle.disabled = false;
}

async function createApiKey() {
  const ok = await ensureConnected();
  if (!ok) return;
  const res = await api.serviceApiKeyCreate(dom.inputApiKeyName.value.trim() || null);
  if (res && res.error) {
    alert(res.error);
    return;
  }
  dom.apiKeyValue.value = res && res.key ? res.key : "";
  await refreshApiKeys();
  renderApiKeys({
    onDisable: disableApiKey,
    onDelete: deleteApiKey,
  });
}

async function deleteApiKey(item) {
  if (!item || !item.id) return;
  const confirmed = window.confirm(`确定删除平台 Key ${item.id} 吗？`);
  if (!confirmed) return;
  const ok = await ensureConnected();
  if (!ok) return;
  await api.serviceApiKeyDelete(item.id);
  await refreshApiKeys();
  renderApiKeys({
    onDisable: disableApiKey,
    onDelete: deleteApiKey,
  });
}

async function disableApiKey(item) {
  if (!item || !item.id) return;
  const ok = await ensureConnected();
  if (!ok) return;
  await api.serviceApiKeyDisable(item.id);
  await refreshApiKeys();
  renderApiKeys({
    onDisable: disableApiKey,
    onDelete: deleteApiKey,
  });
}

async function handleLogin() {
  const ok = await ensureConnected();
  if (!ok) return;
  dom.loginUrl.value = "生成授权链接中...";
  try {
    const res = await api.serviceLoginStart({
      loginType: "chatgpt",
      openBrowser: false,
      note: dom.inputNote.value.trim(),
      tags: dom.inputTags.value.trim(),
      groupName: dom.inputGroup.value.trim(),
    });
    if (res && res.error) {
      dom.loginHint.textContent = `登录失败：${res.error}`;
      dom.loginUrl.value = "";
      return;
    }
    dom.loginUrl.value = res && res.authUrl ? res.authUrl : "";
    if (res && res.authUrl) {
      await api.openInBrowser(res.authUrl);
      if (res.warning) {
        dom.loginHint.textContent = `注意：${res.warning}。如无法回调，可在下方粘贴回调链接手动解析。`;
      } else {
        dom.loginHint.textContent = "已打开浏览器，请完成授权。";
      }
    } else {
      dom.loginHint.textContent = "未获取到授权链接，请重试。";
    }
    state.activeLoginId = res && res.loginId ? res.loginId : null;
    const success = await waitForLogin(state.activeLoginId);
    if (success) {
      await refreshAll();
      closeAccountModal();
    } else {
      dom.loginHint.textContent = "登录失败，请重试。";
    }
  } catch (err) {
    dom.loginUrl.value = "";
    dom.loginHint.textContent = "登录失败，请检查 service 状态。";
  }
}

function parseCallbackUrl(raw) {
  const value = String(raw || "").trim();
  if (!value) {
    return { error: "请粘贴回调链接" };
  }
  let url;
  try {
    url = new URL(value);
  } catch (err) {
    try {
      url = new URL(`http://${value}`);
    } catch (error) {
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

async function handleManualCallback() {
  const parsed = parseCallbackUrl(dom.manualCallbackUrl.value);
  if (parsed.error) {
    dom.loginHint.textContent = parsed.error;
    return;
  }
  const ok = await ensureConnected();
  if (!ok) return;
  dom.loginHint.textContent = "解析回调中...";
  try {
    const res = await api.serviceLoginComplete(
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
}

async function waitForLogin(loginId) {
  if (!loginId) return false;
  const deadline = Date.now() + 2 * 60 * 1000;
  while (Date.now() < deadline) {
    const res = await api.serviceLoginStatus(loginId);
    if (res && res.status === "success") return true;
    if (res && res.status === "failed") {
      dom.loginHint.textContent = `登录失败：${res.error || "unknown"}`;
      return false;
    }
    await new Promise((r) => setTimeout(r, 1500));
  }
  dom.loginHint.textContent = "登录超时，请重试。";
  return false;
}

async function handleStartService() {
  setServiceBusy(true, "start");
  const started = await startService(dom.serviceAddrInput.value, {
    skipInitialize: true,
  });
  dom.serviceAddrInput.value = state.serviceAddr;
  localStorage.setItem("gpttools.service.addr", state.serviceAddr);
  if (!started) {
    setServiceBusy(false);
    updateServiceToggle();
    return;
  }
  const probeId = state.serviceProbeId + 1;
  state.serviceProbeId = probeId;
  void waitForConnection({ retries: 12, delayMs: 400, silent: true }).then(
    (ok) => {
      if (state.serviceProbeId !== probeId) return;
      setServiceBusy(false);
      updateServiceToggle();
      if (!ok) {
        const reason = state.serviceLastError ? `：${state.serviceLastError}` : "";
        setServiceHint(`连接失败${reason}，请检查端口或 service 状态`, true);
        return;
      }
      void refreshAll();
      if (!state.autoRefreshTimer) {
        state.autoRefreshTimer = setInterval(refreshAll, 30000);
      }
    },
  );
}

async function handleStopService() {
  setServiceBusy(true, "stop");
  state.serviceProbeId += 1;
  await stopService();
  setServiceBusy(false);
  updateServiceToggle();
  if (state.autoRefreshTimer) {
    clearInterval(state.autoRefreshTimer);
    state.autoRefreshTimer = null;
  }
}

async function handleServiceToggle() {
  if (state.serviceBusy) return;
  if (state.serviceConnected) {
    await handleStopService();
  } else {
    await handleStartService();
  }
}

function restoreServiceAddr() {
  const savedAddr = localStorage.getItem("gpttools.service.addr");
  if (savedAddr) {
    state.serviceAddr = savedAddr;
    dom.serviceAddrInput.value = savedAddr;
    syncServiceAddrFromInput();
    return;
  }
  dom.serviceAddrInput.value = "48760";
  syncServiceAddrFromInput();
}

function syncServiceAddrFromInput() {
  if (!dom.serviceAddrInput) return;
  const raw = dom.serviceAddrInput.value;
  if (!raw) return;
  try {
    state.serviceAddr = normalizeAddr(raw);
  } catch (err) {
    // ignore invalid input during bootstrap
  }
}

async function autoStartService() {
  if (!dom.serviceAddrInput) return;
  syncServiceAddrFromInput();
  const probeId = state.serviceProbeId + 1;
  state.serviceProbeId = probeId;
  const ok = await waitForConnection({
    retries: 1,
    delayMs: 200,
    silent: true,
  });
  if (state.serviceProbeId !== probeId) return;
  if (ok) {
    updateServiceToggle();
    void refreshAll();
    if (!state.autoRefreshTimer) {
      state.autoRefreshTimer = setInterval(refreshAll, 30000);
    }
    return;
  }
  await handleStartService();
}

function bindEvents() {
  dom.navDashboard.addEventListener("click", () => switchPage("dashboard"));
  dom.navAccounts.addEventListener("click", () => switchPage("accounts"));
  dom.navApiKeys.addEventListener("click", () => switchPage("apikeys"));
  dom.addAccountBtn.addEventListener("click", openAccountModal);
  dom.createApiKeyBtn.addEventListener("click", openApiKeyModal);
  dom.closeAccountModal.addEventListener("click", closeAccountModal);
  dom.cancelLogin.addEventListener("click", closeAccountModal);
  dom.submitLogin.addEventListener("click", handleLogin);
  dom.copyLoginUrl.addEventListener("click", () => {
    if (!dom.loginUrl.value) return;
    dom.loginUrl.select();
    dom.loginUrl.setSelectionRange(0, dom.loginUrl.value.length);
    try {
      document.execCommand("copy");
      dom.loginHint.textContent = "授权链接已复制。";
    } catch (err) {
      dom.loginHint.textContent = "复制失败，请手动复制链接。";
    }
  });
  dom.manualCallbackSubmit.addEventListener("click", handleManualCallback);
  dom.closeUsageModal.addEventListener("click", closeUsageModal);
  dom.refreshUsageSingle.addEventListener("click", refreshUsageForAccount);
  dom.closeApiKeyModal.addEventListener("click", closeApiKeyModal);
  dom.cancelApiKey.addEventListener("click", closeApiKeyModal);
  dom.submitApiKey.addEventListener("click", createApiKey);
  dom.copyApiKey.addEventListener("click", () => {
    if (!dom.apiKeyValue.value) return;
    dom.apiKeyValue.select();
    dom.apiKeyValue.setSelectionRange(0, dom.apiKeyValue.value.length);
    document.execCommand("copy");
  });
  dom.globalRefresh.addEventListener("click", refreshAll);
  if (dom.refreshAll) {
    dom.refreshAll.addEventListener("click", refreshAll);
  }
  dom.serviceToggleBtn.addEventListener("click", handleServiceToggle);

  if (dom.accountSearch) {
    dom.accountSearch.addEventListener("input", (event) => {
      state.accountSearch = event.target.value;
      renderAccounts({
        onUpdateSort: updateAccountSort,
        onOpenUsage: handleOpenUsageModal,
        onDelete: deleteAccount,
      });
    });
  }

  const updateFilterButtons = () => {
    if (dom.filterAll) dom.filterAll.classList.toggle("active", state.accountFilter === "all");
    if (dom.filterActive) dom.filterActive.classList.toggle("active", state.accountFilter === "active");
    if (dom.filterLow) dom.filterLow.classList.toggle("active", state.accountFilter === "low");
  };

  const setFilter = (filter) => {
    state.accountFilter = filter;
    updateFilterButtons();
    renderAccounts({
      onUpdateSort: updateAccountSort,
      onOpenUsage: handleOpenUsageModal,
      onDelete: deleteAccount,
    });
  };

  if (dom.filterAll) dom.filterAll.addEventListener("click", () => setFilter("all"));
  if (dom.filterActive) dom.filterActive.addEventListener("click", () => setFilter("active"));
  if (dom.filterLow) dom.filterLow.addEventListener("click", () => setFilter("low"));
}

function bootstrap() {
  setStatus("", false);
  setServiceHint("请输入端口并点击启动", false);
  restoreServiceAddr();
  updateServiceToggle();
  void autoStartService();
  bindEvents();
  renderDashboard();
  renderAccounts({
    onUpdateSort: updateAccountSort,
    onOpenUsage: handleOpenUsageModal,
    onDelete: deleteAccount,
  });
  renderApiKeys({
    onDisable: disableApiKey,
    onDelete: deleteApiKey,
  });
}

window.addEventListener("DOMContentLoaded", bootstrap);
