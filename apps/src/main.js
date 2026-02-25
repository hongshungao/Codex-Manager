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
  refreshRequestLogTodaySummary,
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
let pendingUpdateCandidate = null;

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

function setCurrentVersionText(version) {
  if (!dom.updateCurrentVersion) return;
  const clean = version == null ? "" : String(version).trim();
  if (!clean) {
    dom.updateCurrentVersion.textContent = "--";
    return;
  }
  dom.updateCurrentVersion.textContent = clean.startsWith("v") ? clean : `v${clean}`;
}

function setCheckUpdateButtonLabel() {
  if (!dom.checkUpdate) return;
  if (pendingUpdateCandidate && pendingUpdateCandidate.version) {
    const version = String(pendingUpdateCandidate.version).trim();
    const display = version.startsWith("v") ? version : `v${version}`;
    dom.checkUpdate.textContent = `更新到 ${display}`;
    return;
  }
  dom.checkUpdate.textContent = "检查更新";
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
        pendingUpdateCandidate = null;
        setCheckUpdateButtonLabel();
        setUpdateStatusText("当前已是最新版本");
        if (!silentIfLatest) {
          showToast("当前已是最新版本");
        }
        return false;
      }

      pendingUpdateCandidate = {
        version: checkInfo.version,
        isPortable: checkInfo.isPortable,
        canPrepare: checkInfo.canPrepare,
      };
      setCheckUpdateButtonLabel();

      if (!checkInfo.canPrepare) {
        const msg = checkInfo.reason || `发现新版本${buildVersionLabel(checkInfo.version)}，当前仅可查看版本`;
        setUpdateStatusText(msg);
        if (!silentIfLatest) {
          showToast(msg);
        }
        return true;
      }

      const tip = `发现新版本${buildVersionLabel(checkInfo.version)}，再次点击可更新`;
      setUpdateStatusText(tip);
      if (!silentIfLatest) {
        showToast(tip);
      }
      return true;
    } catch (err) {
      console.error("[update] check/download failed", err);
      pendingUpdateCandidate = null;
      setCheckUpdateButtonLabel();
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

async function runUpdateApplyFlow() {
  if (!pendingUpdateCandidate || !pendingUpdateCandidate.canPrepare) {
    showToast("当前更新只支持版本检查，请稍后重试");
    return false;
  }
  const checkVersionLabel = buildVersionLabel(pendingUpdateCandidate.version);
  try {
    showToast(`正在下载新版本${checkVersionLabel}...`);
    const downloadResult = await updateDownload();
    const downloadInfo = normalizeUpdateInfo(downloadResult);
    const finalInfo = {
      version: downloadInfo.version || pendingUpdateCandidate.version,
      isPortable: downloadInfo.hasPortableHint ? downloadInfo.isPortable : pendingUpdateCandidate.isPortable,
    };
    setUpdateStatusText(`新版本 ${finalInfo.version || ""} 已下载，等待安装`);
    await promptUpdateReady(finalInfo);
    pendingUpdateCandidate = null;
    setCheckUpdateButtonLabel();
    return true;
  } catch (err) {
    console.error("[update] apply failed", err);
    setUpdateStatusText(`更新失败：${normalizeErrorMessage(err)}`);
    showToast(`更新失败：${normalizeErrorMessage(err)}`, "error");
    return false;
  }
}

async function handleCheckUpdateClick() {
  const hasPreparedCheck = Boolean(pendingUpdateCandidate && pendingUpdateCandidate.version);
  const busyText = hasPreparedCheck ? "更新中..." : "检查中...";
  await withButtonBusy(dom.checkUpdate, busyText, async () => {
    await nextPaintTick();
    if (hasPreparedCheck) {
      await runUpdateApplyFlow();
      return;
    }
    await runUpdateCheckFlow({ silentIfLatest: false });
  });
  setCheckUpdateButtonLabel();
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
    setCurrentVersionText("--");
    setUpdateStatusText("仅桌面端支持更新");
    return;
  }
  try {
    const status = await updateStatus();
    const current = status && status.currentVersion ? String(status.currentVersion) : "";
    setCurrentVersionText(current);
    if (current) {
      setUpdateStatusText("尚未检查更新");
    } else {
      setUpdateStatusText("尚未检查更新");
    }
    setCheckUpdateButtonLabel();
  } catch {
    setCurrentVersionText("--");
    setUpdateStatusText("尚未检查更新");
    setCheckUpdateButtonLabel();
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
        { name: "request-log-today-summary", run: refreshRequestLogTodaySummary },
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






