import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/responsive.css";

import { updateCheck, updateDownload, updateInstall, updateRestart, updateStatus } from "./api";
import { state } from "./state";
import { dom } from "./ui/dom";
import { setStatus, setServiceHint } from "./ui/status";
import { createFeedbackHandlers } from "./ui/feedback";
import { createThemeController } from "./ui/theme";
import { withButtonBusy } from "./ui/button-busy";
import { createStartupMaskController } from "./ui/startup-mask";
import {
  ensureConnected,
  normalizeAddr,
  startService,
  stopService,
  waitForConnection,
} from "./services/connection";
import {
  refreshAccounts,
  refreshUsageList,
  refreshApiKeys,
  refreshApiModels,
  refreshRequestLogs,
  clearRequestLogs,
} from "./services/data";
import {
  ensureAutoRefreshTimer,
  runRefreshTasks,
  stopAutoRefreshTimer,
} from "./services/refresh";
import { createServiceLifecycle } from "./services/service-lifecycle";
import { createLoginFlow } from "./services/login-flow";
import { createManagementActions } from "./services/management-actions";
import { openAccountModal, closeAccountModal } from "./views/accounts";
import { renderApiKeys, openApiKeyModal, closeApiKeyModal, populateApiKeyModelSelect } from "./views/apikeys";
import { openUsageModal, closeUsageModal, renderUsageSnapshot } from "./views/usage";
import { renderRequestLogs } from "./views/requestlogs";
import { renderAllViews, renderAccountsOnly, renderCurrentView } from "./views/renderers";
import { buildRenderActions } from "./views/render-actions";
import { createNavigationHandlers } from "./views/navigation";
import { bindMainEvents } from "./views/event-bindings";

const { showToast, showConfirmDialog } = createFeedbackHandlers({ dom });
const {
  renderThemeButtons,
  setTheme,
  restoreTheme,
  closeThemePanel,
  toggleThemePanel,
} = createThemeController({ dom });

function renderCurrentPageView(page = state.currentPage) {
  renderCurrentView(page, buildMainRenderActions());
}

const { switchPage, updateRequestLogFilterButtons } = createNavigationHandlers({
  state,
  dom,
  closeThemePanel,
  onPageActivated: renderCurrentPageView,
});

const { setStartupMask } = createStartupMaskController({ dom, state });
const UPDATE_AUTO_CHECK_STORAGE_KEY = "codexmanager.update.auto_check";
const UPDATE_CHECK_DELAY_MS = 1200;
let refreshAllInFlight = null;
let updateCheckInFlight = null;

function isTauriRuntime() {
  return Boolean(window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke);
}

function readUpdateAutoCheckSetting() {
  if (typeof localStorage === "undefined") {
    return true;
  }
  const raw = localStorage.getItem(UPDATE_AUTO_CHECK_STORAGE_KEY);
  if (raw == null) {
    return true;
  }
  const normalized = String(raw).trim().toLowerCase();
  return !["0", "false", "off", "no"].includes(normalized);
}

function saveUpdateAutoCheckSetting(enabled) {
  if (typeof localStorage === "undefined") {
    return;
  }
  localStorage.setItem(UPDATE_AUTO_CHECK_STORAGE_KEY, enabled ? "1" : "0");
}

function initUpdateAutoCheckSetting() {
  const enabled = readUpdateAutoCheckSetting();
  if (typeof localStorage !== "undefined" && localStorage.getItem(UPDATE_AUTO_CHECK_STORAGE_KEY) == null) {
    saveUpdateAutoCheckSetting(enabled);
  }
  if (dom.autoCheckUpdate) {
    dom.autoCheckUpdate.checked = enabled;
  }
}

function getPathValue(source, path) {
  const steps = String(path).split(".");
  let cursor = source;
  for (const step of steps) {
    if (!cursor || typeof cursor !== "object" || !(step in cursor)) {
      return undefined;
    }
    cursor = cursor[step];
  }
  return cursor;
}

function pickFirstValue(source, paths) {
  for (const path of paths) {
    const value = getPathValue(source, path);
    if (value !== undefined && value !== null && String(value) !== "") {
      return value;
    }
  }
  return null;
}

function pickBooleanValue(source, paths) {
  const value = pickFirstValue(source, paths);
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    return value !== 0;
  }
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (["1", "true", "yes", "on"].includes(normalized)) {
      return true;
    }
    if (["0", "false", "no", "off"].includes(normalized)) {
      return false;
    }
  }
  return null;
}

function normalizeUpdateInfo(source) {
  const payload = source && typeof source === "object" ? source : {};
  const explicitAvailable = pickBooleanValue(payload, [
    "hasUpdate",
    "available",
    "updateAvailable",
    "has_upgrade",
    "has_update",
    "needUpdate",
    "need_update",
    "result.hasUpdate",
    "result.available",
    "result.updateAvailable",
  ]);
  const explicitlyLatest = pickBooleanValue(payload, [
    "isLatest",
    "upToDate",
    "noUpdate",
    "result.isLatest",
    "result.upToDate",
  ]);
  const hintedVersion = pickFirstValue(payload, [
    "targetVersion",
    "latestVersion",
    "newVersion",
    "release.version",
    "manifest.version",
    "result.targetVersion",
    "result.latestVersion",
  ]);
  let available = explicitAvailable;
  if (available == null) {
    if (explicitlyLatest === true) {
      available = false;
    } else {
      available = hintedVersion != null;
    }
  }

  const packageTypeValue = pickFirstValue(payload, [
    "packageType",
    "package_type",
    "distributionType",
    "distribution_type",
    "updateType",
    "update_type",
    "installType",
    "install_type",
    "release.packageType",
    "result.packageType",
  ]);
  const packageType = packageTypeValue == null ? "" : String(packageTypeValue).toLowerCase();
  const portableFlag = pickBooleanValue(payload, [
    "isPortable",
    "portable",
    "release.isPortable",
    "result.isPortable",
  ]);
  const hasPortableHint = portableFlag != null || Boolean(packageType);
  const isPortable = portableFlag === true || packageType.includes("portable");
  const versionValue = pickFirstValue(payload, [
    "latestVersion",
    "targetVersion",
    "newVersion",
    "version",
    "release.version",
    "manifest.version",
    "result.latestVersion",
    "result.targetVersion",
    "result.version",
  ]);
  const downloaded = pickBooleanValue(payload, [
    "downloaded",
    "isDownloaded",
    "readyToInstall",
    "ready",
    "result.downloaded",
    "result.readyToInstall",
  ]) === true;
  const canPrepareValue = pickBooleanValue(payload, [
    "canPrepare",
    "result.canPrepare",
  ]);
  const reasonValue = pickFirstValue(payload, [
    "reason",
    "message",
    "error",
    "result.reason",
    "result.message",
  ]);
  return {
    available: Boolean(available),
    version: versionValue == null ? "" : String(versionValue).trim(),
    isPortable,
    hasPortableHint,
    downloaded,
    canPrepare: canPrepareValue !== false,
    reason: reasonValue == null ? "" : String(reasonValue),
  };
}

function buildVersionLabel(version) {
  if (!version) {
    return "";
  }
  const clean = String(version).trim();
  if (!clean) {
    return "";
  }
  return clean.startsWith("v") ? ` ${clean}` : ` v${clean}`;
}

function normalizeErrorMessage(err) {
  const raw = String(err && err.message ? err.message : err).trim();
  if (!raw) {
    return "未知错误";
  }
  return raw.length > 120 ? `${raw.slice(0, 120)}...` : raw;
}

function setUpdateStatusText(message) {
  if (!dom.updateStatusText) return;
  dom.updateStatusText.textContent = message || "尚未检查更新";
}

async function promptUpdateReady(info) {
  const versionLabel = buildVersionLabel(info.version);
  if (info.isPortable) {
    const shouldRestart = await showConfirmDialog({
      title: "更新已下载",
      message: `新版本${versionLabel}已下载完成，重启应用即可更新。是否现在重启？`,
      confirmText: "立即重启",
      cancelText: "稍后",
    });
    if (!shouldRestart) {
      return;
    }
    try {
      await updateRestart();
    } catch (err) {
      console.error("[update] restart failed", err);
      showToast(`重启更新失败：${normalizeErrorMessage(err)}`, "error");
    }
    return;
  }

  const shouldInstall = await showConfirmDialog({
    title: "更新已下载",
    message: `新版本${versionLabel}已下载完成，是否立即安装更新？`,
    confirmText: "立即安装",
    cancelText: "稍后",
  });
  if (!shouldInstall) {
    return;
  }
  try {
    await updateInstall();
  } catch (err) {
    console.error("[update] install failed", err);
    showToast(`安装更新失败：${normalizeErrorMessage(err)}`, "error");
  }
}

async function runUpdateCheckFlow({ silentIfLatest = false } = {}) {
  if (!isTauriRuntime()) {
    if (!silentIfLatest) {
      showToast("仅桌面端支持检查更新");
    }
    return false;
  }
  if (updateCheckInFlight) {
    return updateCheckInFlight;
  }
  updateCheckInFlight = (async () => {
    try {
      const checkResult = await updateCheck();
      const checkInfo = normalizeUpdateInfo(checkResult);
      if (!checkInfo.available) {
        setUpdateStatusText("当前已是最新版本");
        if (!silentIfLatest) {
          showToast("当前已是最新版本");
        }
        return false;
      }

      if (!checkInfo.canPrepare) {
        setUpdateStatusText(checkInfo.reason || "发现更新但当前平台缺少可用安装包");
        showToast(checkInfo.reason || "发现更新但当前平台缺少可用安装包", "error");
        return false;
      }

      if (!checkInfo.downloaded) {
        showToast(`发现新版本${buildVersionLabel(checkInfo.version)}，正在下载更新...`);
      }
      const downloadResult = checkInfo.downloaded ? null : await updateDownload();
      const downloadInfo = downloadResult ? normalizeUpdateInfo(downloadResult) : null;
      const finalInfo = {
        available: true,
        version: (downloadInfo && downloadInfo.version) || checkInfo.version,
        isPortable: downloadInfo && downloadInfo.hasPortableHint
          ? downloadInfo.isPortable
          : checkInfo.isPortable,
      };
      setUpdateStatusText(`新版本 ${finalInfo.version || ""} 已下载，等待安装`);
      await promptUpdateReady(finalInfo);
      return true;
    } catch (err) {
      console.error("[update] check/download failed", err);
      setUpdateStatusText(`检查失败：${normalizeErrorMessage(err)}`);
      showToast(`检查更新失败：${normalizeErrorMessage(err)}`, "error");
      return false;
    }
  })();

  try {
    return await updateCheckInFlight;
  } finally {
    updateCheckInFlight = null;
  }
}

async function handleCheckUpdateClick() {
  await withButtonBusy(dom.checkUpdate, "检查中...", async () => {
    await nextPaintTick();
    await runUpdateCheckFlow({ silentIfLatest: false });
  });
}

function scheduleStartupUpdateCheck() {
  if (!readUpdateAutoCheckSetting()) {
    return;
  }
  setTimeout(() => {
    void runUpdateCheckFlow({ silentIfLatest: true });
  }, UPDATE_CHECK_DELAY_MS);
}

async function bootstrapUpdateStatus() {
  if (!isTauriRuntime()) {
    setUpdateStatusText("仅桌面端支持更新");
    return;
  }
  try {
    const status = await updateStatus();
    const current = status && status.currentVersion ? String(status.currentVersion) : "";
    if (current) {
      setUpdateStatusText(`当前版本 v${current}`);
    } else {
      setUpdateStatusText("尚未检查更新");
    }
  } catch {
    setUpdateStatusText("尚未检查更新");
  }
}

function nextPaintTick() {
  return new Promise((resolve) => {
    if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
      window.requestAnimationFrame(() => resolve());
      return;
    }
    setTimeout(resolve, 0);
  });
}

async function refreshAll() {
  if (refreshAllInFlight) {
    return refreshAllInFlight;
  }
  refreshAllInFlight = (async () => {
    const ok = await ensureConnected();
    serviceLifecycle.updateServiceToggle();
    if (!ok) return;
    const results = await runRefreshTasks(
      [
        { name: "accounts", run: refreshAccounts },
        { name: "usage", run: refreshUsageList },
        { name: "api-models", run: refreshApiModels },
        { name: "api-keys", run: refreshApiKeys },
        { name: "request-logs", run: () => refreshRequestLogs(state.requestLogQuery) },
      ],
      (taskName, err) => {
        console.error(`[refreshAll] ${taskName} failed`, err);
      },
    );
    // 中文注释：并行刷新时允许“部分失败部分成功”，否则某个慢/失败接口会拖垮整页刷新体验。
    const hasFailedTask = results.some((item) => item.status === "rejected");
    if (hasFailedTask) {
      showToast("部分数据刷新失败，已展示可用数据", "error");
    }
    renderCurrentPageView();
  })();
  try {
    return await refreshAllInFlight;
  } finally {
    refreshAllInFlight = null;
  }
}

async function handleRefreshAllClick() {
  await withButtonBusy(dom.refreshAll, "刷新中...", async () => {
    // 中文注释：先让浏览器绘制 loading 态，避免用户感知“点击后卡住”。
    await nextPaintTick();
    await refreshAll();
  });
}

async function refreshAccountsAndUsage() {
  const ok = await ensureConnected();
  serviceLifecycle.updateServiceToggle();
  if (!ok) return false;

  const results = await runRefreshTasks(
    [
      { name: "accounts", run: refreshAccounts },
      { name: "usage", run: refreshUsageList },
    ],
    (taskName, err) => {
      console.error(`[refreshAccountsAndUsage] ${taskName} failed`, err);
    },
  );
  return !results.some((item) => item.status === "rejected");
}

const serviceLifecycle = createServiceLifecycle({
  state,
  dom,
  setServiceHint,
  normalizeAddr,
  startService,
  stopService,
  waitForConnection,
  refreshAll,
  ensureAutoRefreshTimer,
  stopAutoRefreshTimer,
  onStartupState: (loading, message) => setStartupMask(loading, message),
});

const loginFlow = createLoginFlow({
  dom,
  state,
  withButtonBusy,
  ensureConnected,
  refreshAll,
  closeAccountModal,
});

const managementActions = createManagementActions({
  dom,
  state,
  ensureConnected,
  withButtonBusy,
  showToast,
  showConfirmDialog,
  clearRequestLogs,
  refreshRequestLogs,
  renderRequestLogs,
  refreshAccountsAndUsage,
  renderAccountsView,
  openUsageModal,
  renderUsageSnapshot,
  refreshApiModels,
  refreshApiKeys,
  populateApiKeyModelSelect,
  renderApiKeys,
});

const {
  handleClearRequestLogs,
  updateAccountSort,
  deleteAccount,
  handleOpenUsageModal,
  refreshUsageForAccount,
  createApiKey,
  deleteApiKey,
  toggleApiKeyStatus,
  updateApiKeyModel,
  copyApiKey,
} = managementActions;

function buildMainRenderActions() {
  return buildRenderActions({
    updateAccountSort,
    handleOpenUsageModal,
    deleteAccount,
    toggleApiKeyStatus,
    deleteApiKey,
    updateApiKeyModel,
    copyApiKey,
  });
}

function renderAccountsView() {
  renderAccountsOnly(buildMainRenderActions());
}

function bindEvents() {
  bindMainEvents({
    dom,
    state,
    switchPage,
    openAccountModal,
    openApiKeyModal,
    closeAccountModal,
    handleLogin: loginFlow.handleLogin,
    showToast,
    handleManualCallback: loginFlow.handleManualCallback,
    closeUsageModal,
    refreshUsageForAccount,
    closeApiKeyModal,
    createApiKey,
    handleClearRequestLogs,
    refreshRequestLogs,
    renderRequestLogs,
    refreshAll: handleRefreshAllClick,
    ensureConnected,
    refreshApiModels,
    populateApiKeyModelSelect,
    toggleThemePanel,
    closeThemePanel,
    setTheme,
    handleServiceToggle: serviceLifecycle.handleServiceToggle,
    renderAccountsView,
    updateRequestLogFilterButtons,
  });

  if (dom.autoCheckUpdate && dom.autoCheckUpdate.dataset.bound !== "1") {
    dom.autoCheckUpdate.dataset.bound = "1";
    dom.autoCheckUpdate.addEventListener("change", () => {
      const enabled = Boolean(dom.autoCheckUpdate.checked);
      saveUpdateAutoCheckSetting(enabled);
    });
  }
  if (dom.checkUpdate && dom.checkUpdate.dataset.bound !== "1") {
    dom.checkUpdate.dataset.bound = "1";
    dom.checkUpdate.addEventListener("click", () => {
      void handleCheckUpdateClick();
    });
  }
}

function bootstrap() {
  setStartupMask(true, "正在初始化界面...");
  setStatus("", false);
  setServiceHint("请输入端口并点击启动", false);
  renderThemeButtons();
  restoreTheme();
  initUpdateAutoCheckSetting();
  void bootstrapUpdateStatus();
  serviceLifecycle.restoreServiceAddr();
  serviceLifecycle.updateServiceToggle();
  bindEvents();
  renderAllViews(buildMainRenderActions());
  updateRequestLogFilterButtons();
  scheduleStartupUpdateCheck();
  void serviceLifecycle.autoStartService().finally(() => {
    setStartupMask(false);
  });
}

window.addEventListener("DOMContentLoaded", bootstrap);






