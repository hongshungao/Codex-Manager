import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/responsive.css";
import "./styles/performance.css";

import {
  appLightweightModeOnCloseToTrayGet,
  appLightweightModeOnCloseToTraySet,
  appSettingsGet,
  appSettingsSet,
  serviceGatewayBackgroundTasksSet,
  serviceGatewayHeaderPolicySet,
  serviceGatewayUpstreamProxySet,
  serviceGatewayRouteStrategySet,
  serviceUsageRefresh,
  updateCheck,
  updateDownload,
  updateInstall,
  updateRestart,
  updateStatus,
} from "./api";
import { state } from "./state";
import { dom } from "./ui/dom";
import { setStatus, setServiceHint } from "./ui/status";
import { createFeedbackHandlers } from "./ui/feedback";
import { createThemeController } from "./ui/theme";
import {
  formatEnvOverridesText,
  normalizeEnvOverrideCatalog,
  normalizeEnvOverrides,
  normalizeStringList,
  parseEnvOverridesText,
} from "./ui/env-overrides";
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
  refreshAccountsPage,
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
import { renderAccountsRefreshProgress } from "./views/accounts/render";
import {
  clearRefreshAllProgress,
  setRefreshAllProgress,
} from "./services/management/account-actions";
import { renderApiKeys, openApiKeyModal, closeApiKeyModal, populateApiKeyModelSelect } from "./views/apikeys";
import { openUsageModal, closeUsageModal, renderUsageSnapshot } from "./views/usage";
import { renderRequestLogs } from "./views/requestlogs";
import { renderAccountsOnly, renderCurrentView } from "./views/renderers";
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
} = createThemeController({
  dom,
  onThemeChange: (theme) => saveAppSettingsPatch({ theme }),
});

function renderCurrentPageView(page = state.currentPage) {
  renderCurrentView(page, buildMainRenderActions());
}

async function reloadAccountsPage(options = {}) {
  const silent = options.silent === true;
  const render = options.render !== false;
  const ensureConnection = options.ensureConnection !== false;

  if (ensureConnection) {
    const ok = await ensureConnected();
    serviceLifecycle.updateServiceToggle();
    if (!ok) {
      return false;
    }
  }

  try {
    const applied = await refreshAccountsPage({ latestOnly: options.latestOnly !== false });
    if (applied !== false && render) {
      renderAccountsView();
    }
    return applied !== false;
  } catch (err) {
    console.error("[accounts] page refresh failed", err);
    if (!silent) {
      showToast(`账号分页刷新失败：${normalizeErrorMessage(err)}`, "error");
    }
    return false;
  }
}

const { switchPage, updateRequestLogFilterButtons } = createNavigationHandlers({
  state,
  dom,
  closeThemePanel,
  onPageActivated: (page) => {
    renderCurrentPageView(page);
    if (page === "accounts") {
      void reloadAccountsPage({ silent: true, latestOnly: true });
    }
  },
});

const { setStartupMask } = createStartupMaskController({ dom, state });
const UI_LOW_TRANSPARENCY_BODY_CLASS = "cm-low-transparency";
const UI_LOW_TRANSPARENCY_TOGGLE_ID = "lowTransparencyMode";
const UI_LOW_TRANSPARENCY_CARD_ID = "settingsLowTransparencyCard";
const ROUTE_STRATEGY_ORDERED = "ordered";
const ROUTE_STRATEGY_BALANCED = "balanced";
const SERVICE_LISTEN_MODE_LOOPBACK = "loopback";
const SERVICE_LISTEN_MODE_ALL_INTERFACES = "all_interfaces";
const DEFAULT_BACKGROUND_TASKS_SETTINGS = {
  usagePollingEnabled: true,
  usagePollIntervalSecs: 600,
  gatewayKeepaliveEnabled: true,
  gatewayKeepaliveIntervalSecs: 180,
  tokenRefreshPollingEnabled: true,
  tokenRefreshPollIntervalSecs: 60,
  usageRefreshWorkers: 4,
  httpWorkerFactor: 4,
  httpWorkerMin: 8,
  httpStreamWorkerFactor: 1,
  httpStreamWorkerMin: 2,
};
const BACKGROUND_TASKS_RESTART_KEYS_DEFAULT = [
  "usageRefreshWorkers",
  "httpWorkerFactor",
  "httpWorkerMin",
  "httpStreamWorkerFactor",
  "httpStreamWorkerMin",
];
const BACKGROUND_TASKS_RESTART_KEY_LABELS = {
  usageRefreshWorkers: "用量刷新并发线程数",
  httpWorkerFactor: "普通请求并发因子",
  httpWorkerMin: "普通请求最小并发",
  httpStreamWorkerFactor: "流式请求并发因子",
  httpStreamWorkerMin: "流式请求最小并发",
};
const API_MODELS_REMOTE_REFRESH_STORAGE_KEY = "codexmanager.apikey.models.last_remote_refresh_at";
const API_MODELS_REMOTE_REFRESH_INTERVAL_MS = 6 * 60 * 60 * 1000;
const UPDATE_CHECK_DELAY_MS = 1200;
const LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_STORAGE_KEY = "codexmanager.app.lightweight_mode_on_close_to_tray";
let refreshAllInFlight = null;
let refreshAllProgressClearTimer = null;
let updateCheckInFlight = null;
let pendingUpdateCandidate = null;
let serviceListenModeSyncInFlight = null;
let routeStrategySyncInFlight = null;
let routeStrategySyncedProbeId = -1;
let cpaNoCookieHeaderModeSyncInFlight = null;
let cpaNoCookieHeaderModeSyncedProbeId = -1;
let upstreamProxySyncInFlight = null;
let upstreamProxySyncedProbeId = -1;
let backgroundTasksSyncInFlight = null;
let backgroundTasksSyncedProbeId = -1;
let apiModelsRemoteRefreshInFlight = null;
let appSettingsSnapshot = buildDefaultAppSettingsSnapshot();

function buildDefaultAppSettingsSnapshot() {
  return {
    updateAutoCheck: true,
    closeToTrayOnClose: false,
    closeToTraySupported: isTauriRuntime(),
    lowTransparency: false,
    theme: "tech",
    serviceAddr: "localhost:48760",
    serviceListenMode: normalizeServiceListenMode(null),
    routeStrategy: normalizeRouteStrategy(null),
    cpaNoCookieHeaderModeEnabled: false,
    upstreamProxyUrl: "",
    backgroundTasks: normalizeBackgroundTasksSettings(DEFAULT_BACKGROUND_TASKS_SETTINGS),
    envOverrides: {},
    envOverrideCatalog: [],
    envOverrideReservedKeys: [],
    envOverrideUnsupportedKeys: [],
    webAccessPasswordConfigured: false,
  };
}

function normalizeThemeSetting(value) {
  const normalized = String(value || "").trim().toLowerCase();
  return normalized || "tech";
}

function normalizeAppSettingsSnapshot(source) {
  const payload = source && typeof source === "object" ? source : {};
  const defaults = buildDefaultAppSettingsSnapshot();
  let serviceAddr = defaults.serviceAddr;
  try {
    serviceAddr = normalizeAddr(payload.serviceAddr || defaults.serviceAddr);
  } catch {
    serviceAddr = defaults.serviceAddr;
  }
  return {
    updateAutoCheck: normalizeBooleanSetting(payload.updateAutoCheck, defaults.updateAutoCheck),
    closeToTrayOnClose: normalizeBooleanSetting(
      payload.closeToTrayOnClose,
      defaults.closeToTrayOnClose,
    ),
    closeToTraySupported: normalizeBooleanSetting(
      payload.closeToTraySupported,
      defaults.closeToTraySupported,
    ),
    lowTransparency: normalizeBooleanSetting(payload.lowTransparency, defaults.lowTransparency),
    theme: normalizeThemeSetting(payload.theme),
    serviceAddr,
    serviceListenMode: normalizeServiceListenMode(payload.serviceListenMode),
    routeStrategy: normalizeRouteStrategy(payload.routeStrategy),
    cpaNoCookieHeaderModeEnabled: normalizeCpaNoCookieHeaderMode(
      payload.cpaNoCookieHeaderModeEnabled,
    ),
    upstreamProxyUrl: normalizeUpstreamProxyUrl(payload.upstreamProxyUrl),
    backgroundTasks: normalizeBackgroundTasksSettings(payload.backgroundTasks),
    envOverrides: normalizeEnvOverrides(payload.envOverrides),
    envOverrideCatalog: normalizeEnvOverrideCatalog(payload.envOverrideCatalog),
    envOverrideReservedKeys: normalizeStringList(payload.envOverrideReservedKeys),
    envOverrideUnsupportedKeys: normalizeStringList(payload.envOverrideUnsupportedKeys),
    webAccessPasswordConfigured: normalizeBooleanSetting(
      payload.webAccessPasswordConfigured,
      defaults.webAccessPasswordConfigured,
    ),
  };
}

function setAppSettingsSnapshot(snapshot) {
  appSettingsSnapshot = normalizeAppSettingsSnapshot(snapshot);
  state.serviceAddr = appSettingsSnapshot.serviceAddr;
  return appSettingsSnapshot;
}

function patchAppSettingsSnapshot(patch = {}) {
  const next = {
    ...appSettingsSnapshot,
    ...(patch && typeof patch === "object" ? patch : {}),
  };
  if (patch && Object.prototype.hasOwnProperty.call(patch, "backgroundTasks")) {
    next.backgroundTasks = patch.backgroundTasks;
  }
  if (patch && Object.prototype.hasOwnProperty.call(patch, "envOverrides")) {
    next.envOverrides = patch.envOverrides;
  }
  if (patch && Object.prototype.hasOwnProperty.call(patch, "envOverrideCatalog")) {
    next.envOverrideCatalog = patch.envOverrideCatalog;
  }
  if (patch && Object.prototype.hasOwnProperty.call(patch, "envOverrideReservedKeys")) {
    next.envOverrideReservedKeys = patch.envOverrideReservedKeys;
  }
  if (patch && Object.prototype.hasOwnProperty.call(patch, "envOverrideUnsupportedKeys")) {
    next.envOverrideUnsupportedKeys = patch.envOverrideUnsupportedKeys;
  }
  return setAppSettingsSnapshot(next);
}

async function loadAppSettings() {
  try {
    return setAppSettingsSnapshot(await appSettingsGet());
  } catch (err) {
    console.warn("[app-settings] load failed", err);
    return setAppSettingsSnapshot(appSettingsSnapshot);
  }
}

async function saveAppSettingsPatch(patch = {}) {
  const payload = patch && typeof patch === "object" ? patch : {};
  return setAppSettingsSnapshot(await appSettingsSet(payload));
}
function buildRefreshAllTasks(options = {}) {
  const refreshRemoteUsage = options.refreshRemoteUsage === true;
  const refreshRemoteModels = options.refreshRemoteModels === true;
  return [
    { name: "accounts", label: "账号列表", run: refreshAccounts },
    { name: "usage", label: "账号用量", run: () => refreshUsageList({ refreshRemote: refreshRemoteUsage }) },
    { name: "api-models", label: "模型列表", run: () => refreshApiModels({ refreshRemote: refreshRemoteModels }) },
    { name: "api-keys", label: "平台密钥", run: refreshApiKeys },
    { name: "request-logs", label: "请求日志", run: () => refreshRequestLogs(state.requestLogQuery) },
    { name: "request-log-today-summary", label: "今日摘要", run: refreshRequestLogTodaySummary },
  ];
}

function isTauriRuntime() {
  return Boolean(window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke);
}

function applyBrowserModeUi() {
  if (isTauriRuntime()) {
    return false;
  }
  if (typeof document !== "undefined" && document.body) {
    document.body.classList.add("cm-browser");
  }

  // 中文注释：浏览器模式不支持桌面端启停与更新，隐藏相关 UI，避免误操作。
  const serviceSetup = dom.serviceAddrInput ? dom.serviceAddrInput.closest(".service-setup") : null;
  if (serviceSetup) {
    serviceSetup.style.display = "none";
  }
  const updateCard = dom.checkUpdate ? dom.checkUpdate.closest(".settings-top-item, .settings-card") : null;
  if (updateCard) {
    updateCard.style.display = "none";
  }
  const closeToTrayCard = dom.closeToTrayOnClose ? dom.closeToTrayOnClose.closest(".settings-top-item, .settings-card") : null;
  if (closeToTrayCard) {
    closeToTrayCard.style.display = "none";
  }
  const lightweightCard = dom.lightweightModeOnCloseToTray
    ? dom.lightweightModeOnCloseToTray.closest(".settings-top-item, .settings-card")
    : null;
  if (lightweightCard) {
    lightweightCard.style.display = "none";
  }

  return true;
}

function readUpdateAutoCheckSetting() {
  return Boolean(appSettingsSnapshot.updateAutoCheck);
}

function saveUpdateAutoCheckSetting(enabled) {
  patchAppSettingsSnapshot({ updateAutoCheck: Boolean(enabled) });
}

function initUpdateAutoCheckSetting() {
  const enabled = readUpdateAutoCheckSetting();
  if (dom.autoCheckUpdate) {
    dom.autoCheckUpdate.checked = enabled;
  }
}

function readCloseToTrayOnCloseSetting() {
  return Boolean(appSettingsSnapshot.closeToTrayOnClose);
}

function saveCloseToTrayOnCloseSetting(enabled) {
  patchAppSettingsSnapshot({ closeToTrayOnClose: Boolean(enabled) });
}

function setCloseToTrayOnCloseToggle(enabled) {
  if (dom.closeToTrayOnClose) {
    dom.closeToTrayOnClose.checked = Boolean(enabled);
  }
}

async function applyCloseToTrayOnCloseSetting(enabled, { silent = true } = {}) {
  const normalized = Boolean(enabled);
  try {
    const settings = await saveAppSettingsPatch({
      closeToTrayOnClose: normalized,
    });
    const applied = Boolean(settings.closeToTrayOnClose);
    const supported = Boolean(settings.closeToTraySupported);
    if (dom.closeToTrayOnClose) {
      dom.closeToTrayOnClose.disabled = !supported;
    }
    saveCloseToTrayOnCloseSetting(applied);
    setCloseToTrayOnCloseToggle(applied);
    if (!silent) {
      if (normalized && !applied && !supported) {
        showToast("系统托盘不可用，无法启用关闭时最小化到托盘", "error");
      } else {
        showToast(applied ? "已开启：关闭窗口将最小化到托盘" : "已关闭：关闭窗口将直接退出");
      }
    }
    return Boolean(applied);
  } catch (err) {
    if (!silent) {
      showToast(`设置失败：${normalizeErrorMessage(err)}`, "error");
    }
    throw err;
  }
}

function initCloseToTrayOnCloseSetting() {
  const enabled = readCloseToTrayOnCloseSetting();
  setCloseToTrayOnCloseToggle(enabled);
  if (dom.closeToTrayOnClose) {
    dom.closeToTrayOnClose.disabled = !Boolean(appSettingsSnapshot.closeToTraySupported);
  }
}

function readLightweightModeOnCloseToTraySetting() {
  if (typeof localStorage === "undefined") {
    return false;
  }
  const raw = localStorage.getItem(LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_STORAGE_KEY);
  if (raw == null) {
    return false;
  }
  const normalized = String(raw).trim().toLowerCase();
  return ["1", "true", "yes", "on"].includes(normalized);
}

function saveLightweightModeOnCloseToTraySetting(enabled) {
  if (typeof localStorage === "undefined") {
    return;
  }
  localStorage.setItem(LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_STORAGE_KEY, enabled ? "1" : "0");
}

function setLightweightModeOnCloseToTrayToggle(enabled) {
  if (dom.lightweightModeOnCloseToTray) {
    dom.lightweightModeOnCloseToTray.checked = Boolean(enabled);
  }
}

async function applyLightweightModeOnCloseToTraySetting(enabled, { silent = true } = {}) {
  const normalized = Boolean(enabled);
  if (!isTauriRuntime()) {
    return normalized;
  }
  try {
    const applied = await appLightweightModeOnCloseToTraySet(normalized);
    if (!silent) {
      showToast(
        applied
          ? "已开启：关闭到托盘时会释放 WebView 内存（再次打开可能更慢）"
          : "已关闭：托盘隐藏时保留 WebView（再次打开更快）",
      );
    }
    return Boolean(applied);
  } catch (err) {
    if (!silent) {
      showToast(`设置失败：${normalizeErrorMessage(err)}`, "error");
    }
    throw err;
  }
}

async function initLightweightModeOnCloseToTraySetting() {
  const hasLocalSetting = typeof localStorage !== "undefined"
    && localStorage.getItem(LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_STORAGE_KEY) != null;
  let enabled = readLightweightModeOnCloseToTraySetting();
  if (!hasLocalSetting) {
    saveLightweightModeOnCloseToTraySetting(enabled);
  }
  if (isTauriRuntime()) {
    try {
      const serviceValue = await appLightweightModeOnCloseToTrayGet();
      if (!hasLocalSetting) {
        enabled = serviceValue === true;
      }
    } catch {
      // ignore and fallback to local value
    }
  }
  setLightweightModeOnCloseToTrayToggle(enabled);
  let applied = enabled;
  try {
    applied = await applyLightweightModeOnCloseToTraySetting(enabled, { silent: true });
  } catch {
    applied = enabled;
  }
  saveLightweightModeOnCloseToTraySetting(applied);
  setLightweightModeOnCloseToTrayToggle(applied);
}

function readLowTransparencySetting() {
  return Boolean(appSettingsSnapshot.lowTransparency);
}

function saveLowTransparencySetting(enabled) {
  patchAppSettingsSnapshot({ lowTransparency: Boolean(enabled) });
}

function applyLowTransparencySetting(enabled) {
  if (typeof document === "undefined" || !document.body) {
    return;
  }
  document.body.classList.toggle(UI_LOW_TRANSPARENCY_BODY_CLASS, enabled);
}

function ensureLowTransparencySettingCard() {
  if (typeof document === "undefined") {
    return null;
  }
  const existing = document.getElementById(UI_LOW_TRANSPARENCY_TOGGLE_ID);
  if (existing) {
    return existing;
  }

  const settingsGrid = document.querySelector("#pageSettings .settings-grid");
  if (!settingsGrid) {
    return null;
  }

  const existingCard = document.getElementById(UI_LOW_TRANSPARENCY_CARD_ID);
  if (existingCard) {
    return document.getElementById(UI_LOW_TRANSPARENCY_TOGGLE_ID);
  }

  const card = document.createElement("div");
  card.className = "panel settings-card settings-card-span-2";
  card.id = UI_LOW_TRANSPARENCY_CARD_ID;
  card.innerHTML = `
    <div class="panel-header">
      <div>
        <h3>视觉性能</h3>
        <p>减少模糊/透明特效，降低掉帧</p>
      </div>
    </div>
    <div class="settings-row">
      <label class="update-auto-check switch-control" for="${UI_LOW_TRANSPARENCY_TOGGLE_ID}">
        <input id="${UI_LOW_TRANSPARENCY_TOGGLE_ID}" type="checkbox" />
        <span class="switch-track" aria-hidden="true">
          <span class="switch-thumb"></span>
        </span>
        <span>性能模式/低透明度</span>
      </label>
    </div>
    <div class="hint">开启后会关闭/降级 blur、backdrop-filter 等效果（更省 GPU，但质感会更“硬”）。</div>
  `;

  const themeCard = document.getElementById("themePanel")?.closest(".settings-card");
  if (themeCard && themeCard.parentElement === settingsGrid) {
    settingsGrid.insertBefore(card, themeCard);
  } else {
    settingsGrid.appendChild(card);
  }

  return document.getElementById(UI_LOW_TRANSPARENCY_TOGGLE_ID);
}

function initLowTransparencySetting() {
  const enabled = readLowTransparencySetting();
  applyLowTransparencySetting(enabled);
  const toggle = ensureLowTransparencySettingCard();
  if (toggle) {
    toggle.checked = enabled;
  }
}

function normalizeServiceListenMode(value) {
  const raw = String(value || "").trim().toLowerCase();
  if (["all_interfaces", "all-interfaces", "all", "0.0.0.0"].includes(raw)) {
    return SERVICE_LISTEN_MODE_ALL_INTERFACES;
  }
  return SERVICE_LISTEN_MODE_LOOPBACK;
}

function serviceListenModeLabel(mode) {
  return normalizeServiceListenMode(mode) === SERVICE_LISTEN_MODE_ALL_INTERFACES
    ? "全部网卡（0.0.0.0）"
    : "仅本机（localhost / 127.0.0.1）";
}

function buildServiceListenModeHint(mode, requiresRestart = true) {
  const normalized = normalizeServiceListenMode(mode);
  const suffix = normalized === SERVICE_LISTEN_MODE_ALL_INTERFACES
    ? "局域网访问请使用本机实际 IP。"
    : "外部设备将无法直接访问。";
  if (requiresRestart) {
    return `已保存为${serviceListenModeLabel(normalized)}，重启服务后生效；${suffix}`;
  }
  return `当前为${serviceListenModeLabel(normalized)}；${suffix}`;
}

function setServiceListenModeSelect(mode) {
  if (!dom.serviceListenModeSelect) {
    return;
  }
  dom.serviceListenModeSelect.value = normalizeServiceListenMode(mode);
}

function setServiceListenModeHint(message) {
  if (!dom.serviceListenModeHint) {
    return;
  }
  dom.serviceListenModeHint.textContent = String(message || "").trim()
    || "保存后重启服务生效；局域网访问请使用本机实际 IP。";
}

function initServiceListenModeSetting() {
  const mode = normalizeServiceListenMode(appSettingsSnapshot.serviceListenMode);
  setServiceListenModeSelect(mode);
  setServiceListenModeHint(buildServiceListenModeHint(mode, true));
}

async function applyServiceListenModeToService(mode, { silent = true } = {}) {
  const normalized = normalizeServiceListenMode(mode);
  if (serviceListenModeSyncInFlight) {
    return serviceListenModeSyncInFlight;
  }
  serviceListenModeSyncInFlight = (async () => {
    const settings = await saveAppSettingsPatch({
      serviceListenMode: normalized,
    });
    const resolved = {
      mode: normalizeServiceListenMode(settings.serviceListenMode),
      requiresRestart: true,
    };
    setServiceListenModeSelect(resolved.mode);
    setServiceListenModeHint(buildServiceListenModeHint(resolved.mode, resolved.requiresRestart));
    if (!silent) {
      showToast(`监听模式已保存为${serviceListenModeLabel(resolved.mode)}，重启服务后生效`);
    }
    return true;
  })();

  try {
    return await serviceListenModeSyncInFlight;
  } catch (err) {
    if (!silent) {
      showToast(`保存失败：${normalizeErrorMessage(err)}`, "error");
      setServiceListenModeHint(`保存失败：${normalizeErrorMessage(err)}`);
    }
    return false;
  } finally {
    serviceListenModeSyncInFlight = null;
  }
}

async function syncServiceListenModeOnStartup() {
  initServiceListenModeSetting();
}

function normalizeRouteStrategy(strategy) {
  const raw = String(strategy || "").trim().toLowerCase();
  if (["balanced", "round_robin", "round-robin", "rr"].includes(raw)) {
    return ROUTE_STRATEGY_BALANCED;
  }
  return ROUTE_STRATEGY_ORDERED;
}

function routeStrategyLabel(strategy) {
  return normalizeRouteStrategy(strategy) === ROUTE_STRATEGY_BALANCED ? "均衡轮询" : "顺序优先";
}

function updateRouteStrategyHint(strategy) {
  if (!dom.routeStrategyHint) return;
  let hintText = "按账号顺序优先请求，优先使用可用账号（不可用账号不会参与选路）。";
  if (normalizeRouteStrategy(strategy) === ROUTE_STRATEGY_BALANCED) {
    hintText = "按密钥 + 模型均衡轮询起点，优先使用可用账号（不可用账号不会参与选路）。";
  }
  dom.routeStrategyHint.title = hintText;
  dom.routeStrategyHint.setAttribute("aria-label", `网关选路策略说明：${hintText}`);
}

function readRouteStrategySetting() {
  return normalizeRouteStrategy(appSettingsSnapshot.routeStrategy);
}

function saveRouteStrategySetting(strategy) {
  patchAppSettingsSnapshot({
    routeStrategy: normalizeRouteStrategy(strategy),
  });
}

function setRouteStrategySelect(strategy) {
  const normalized = normalizeRouteStrategy(strategy);
  if (dom.routeStrategySelect) {
    dom.routeStrategySelect.value = normalized;
  }
  updateRouteStrategyHint(normalized);
}

function initRouteStrategySetting() {
  const mode = readRouteStrategySetting();
  setRouteStrategySelect(mode);
}

function resolveRouteStrategyFromPayload(payload) {
  const picked = pickFirstValue(payload, ["strategy", "result.strategy"]);
  return normalizeRouteStrategy(picked);
}

async function applyRouteStrategyToService(strategy, { silent = true } = {}) {
  const normalized = normalizeRouteStrategy(strategy);
  if (routeStrategySyncInFlight) {
    return routeStrategySyncInFlight;
  }
  routeStrategySyncInFlight = (async () => {
    const connected = await ensureConnected();
    serviceLifecycle.updateServiceToggle();
    if (!connected) {
      if (!silent) {
        showToast("服务未连接，稍后会自动应用选路策略", "error");
      }
      return false;
    }
    const response = await serviceGatewayRouteStrategySet(normalized);
    const applied = resolveRouteStrategyFromPayload(response);
    saveRouteStrategySetting(applied);
    setRouteStrategySelect(applied);
    routeStrategySyncedProbeId = state.serviceProbeId;
    if (!silent) {
      showToast(`已切换为${routeStrategyLabel(applied)}`);
    }
    return true;
  })();

  try {
    return await routeStrategySyncInFlight;
  } catch (err) {
    if (!silent) {
      showToast(`切换失败：${normalizeErrorMessage(err)}`, "error");
    }
    return false;
  } finally {
    routeStrategySyncInFlight = null;
  }
}

async function syncRouteStrategyOnStartup() {
  if (!isTauriRuntime()) {
    return;
  }
  await applyRouteStrategyToService(readRouteStrategySetting(), { silent: true });
}

function normalizeCpaNoCookieHeaderMode(value) {
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
  return false;
}

function readCpaNoCookieHeaderModeSetting() {
  return normalizeCpaNoCookieHeaderMode(appSettingsSnapshot.cpaNoCookieHeaderModeEnabled);
}

function saveCpaNoCookieHeaderModeSetting(enabled) {
  patchAppSettingsSnapshot({
    cpaNoCookieHeaderModeEnabled: normalizeCpaNoCookieHeaderMode(enabled),
  });
}

function setCpaNoCookieHeaderModeToggle(enabled) {
  if (dom.cpaNoCookieHeaderMode) {
    dom.cpaNoCookieHeaderMode.checked = Boolean(enabled);
  }
}

function initCpaNoCookieHeaderModeSetting() {
  const enabled = readCpaNoCookieHeaderModeSetting();
  setCpaNoCookieHeaderModeToggle(enabled);
}

function resolveCpaNoCookieHeaderModeFromPayload(payload) {
  const value = pickBooleanValue(payload, [
    "cpaNoCookieHeaderModeEnabled",
    "enabled",
    "result.cpaNoCookieHeaderModeEnabled",
    "result.enabled",
  ]);
  return Boolean(value);
}

async function applyCpaNoCookieHeaderModeToService(enabled, { silent = true } = {}) {
  const normalized = normalizeCpaNoCookieHeaderMode(enabled);
  if (cpaNoCookieHeaderModeSyncInFlight) {
    return cpaNoCookieHeaderModeSyncInFlight;
  }
  cpaNoCookieHeaderModeSyncInFlight = (async () => {
    const connected = await ensureConnected();
    serviceLifecycle.updateServiceToggle();
    if (!connected) {
      if (!silent) {
        showToast("服务未连接，稍后会自动应用头策略开关", "error");
      }
      return false;
    }
    const response = await serviceGatewayHeaderPolicySet(normalized);
    const applied = resolveCpaNoCookieHeaderModeFromPayload(response);
    saveCpaNoCookieHeaderModeSetting(applied);
    setCpaNoCookieHeaderModeToggle(applied);
    cpaNoCookieHeaderModeSyncedProbeId = state.serviceProbeId;
    if (!silent) {
      showToast(applied ? "已启用请求头收敛策略" : "已关闭请求头收敛策略");
    }
    return true;
  })();

  try {
    return await cpaNoCookieHeaderModeSyncInFlight;
  } catch (err) {
    if (!silent) {
      showToast(`切换失败：${normalizeErrorMessage(err)}`, "error");
    }
    return false;
  } finally {
    cpaNoCookieHeaderModeSyncInFlight = null;
  }
}

async function syncCpaNoCookieHeaderModeOnStartup() {
  if (!isTauriRuntime()) {
    return;
  }
  await applyCpaNoCookieHeaderModeToService(readCpaNoCookieHeaderModeSetting(), { silent: true });
}

function normalizeUpstreamProxyUrl(value) {
  if (value == null) {
    return "";
  }
  return String(value).trim();
}

function readUpstreamProxyUrlSetting() {
  return normalizeUpstreamProxyUrl(appSettingsSnapshot.upstreamProxyUrl);
}

function saveUpstreamProxyUrlSetting(value) {
  patchAppSettingsSnapshot({
    upstreamProxyUrl: normalizeUpstreamProxyUrl(value),
  });
}

function setUpstreamProxyInput(value) {
  if (!dom.upstreamProxyUrlInput) {
    return;
  }
  dom.upstreamProxyUrlInput.value = normalizeUpstreamProxyUrl(value);
}

function setUpstreamProxyHint(message) {
  if (!dom.upstreamProxyHint) {
    return;
  }
  dom.upstreamProxyHint.textContent = message;
}

function initUpstreamProxySetting() {
  const proxyUrl = readUpstreamProxyUrlSetting();
  setUpstreamProxyInput(proxyUrl);
  setUpstreamProxyHint("保存后立即生效。");
}

function resolveUpstreamProxyUrlFromPayload(payload) {
  const picked = pickFirstValue(payload, ["proxyUrl", "result.proxyUrl", "url", "result.url"]);
  return normalizeUpstreamProxyUrl(picked);
}

async function applyUpstreamProxyToService(proxyUrl, { silent = true } = {}) {
  const normalized = normalizeUpstreamProxyUrl(proxyUrl);
  if (upstreamProxySyncInFlight) {
    return upstreamProxySyncInFlight;
  }
  upstreamProxySyncInFlight = (async () => {
    const connected = await ensureConnected();
    serviceLifecycle.updateServiceToggle();
    if (!connected) {
      if (!silent) {
        showToast("服务未连接，稍后会自动应用上游代理", "error");
      }
      return false;
    }
    const response = await serviceGatewayUpstreamProxySet(normalized || null);
    const applied = resolveUpstreamProxyUrlFromPayload(response);
    saveUpstreamProxyUrlSetting(applied);
    setUpstreamProxyInput(applied);
    setUpstreamProxyHint("保存后立即生效。");
    upstreamProxySyncedProbeId = state.serviceProbeId;
    if (!silent) {
      showToast(applied ? "上游代理已保存并生效" : "已清空上游代理，恢复直连");
    }
    return true;
  })();

  try {
    return await upstreamProxySyncInFlight;
  } catch (err) {
    if (!silent) {
      showToast(`保存失败：${normalizeErrorMessage(err)}`, "error");
      setUpstreamProxyHint(`保存失败：${normalizeErrorMessage(err)}`);
    }
    return false;
  } finally {
    upstreamProxySyncInFlight = null;
  }
}

async function syncUpstreamProxyOnStartup() {
  if (!isTauriRuntime()) {
    return;
  }
  await applyUpstreamProxyToService(readUpstreamProxyUrlSetting(), { silent: true });
}

function normalizeBooleanSetting(value, fallback = false) {
  if (value == null) {
    return Boolean(fallback);
  }
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
  return Boolean(fallback);
}

function normalizePositiveInteger(value, fallback, min = 1) {
  const fallbackValue = Math.max(min, Math.floor(Number(fallback) || min));
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return fallbackValue;
  }
  const intValue = Math.floor(numeric);
  if (intValue < min) {
    return min;
  }
  return intValue;
}

function normalizeBackgroundTasksSettings(input) {
  const source = input && typeof input === "object" ? input : {};
  return {
    usagePollingEnabled: normalizeBooleanSetting(
      source.usagePollingEnabled,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.usagePollingEnabled,
    ),
    usagePollIntervalSecs: normalizePositiveInteger(
      source.usagePollIntervalSecs,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.usagePollIntervalSecs,
      1,
    ),
    gatewayKeepaliveEnabled: normalizeBooleanSetting(
      source.gatewayKeepaliveEnabled,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.gatewayKeepaliveEnabled,
    ),
    gatewayKeepaliveIntervalSecs: normalizePositiveInteger(
      source.gatewayKeepaliveIntervalSecs,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.gatewayKeepaliveIntervalSecs,
      1,
    ),
    tokenRefreshPollingEnabled: normalizeBooleanSetting(
      source.tokenRefreshPollingEnabled,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.tokenRefreshPollingEnabled,
    ),
    tokenRefreshPollIntervalSecs: normalizePositiveInteger(
      source.tokenRefreshPollIntervalSecs,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.tokenRefreshPollIntervalSecs,
      1,
    ),
    usageRefreshWorkers: normalizePositiveInteger(
      source.usageRefreshWorkers,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.usageRefreshWorkers,
      1,
    ),
    httpWorkerFactor: normalizePositiveInteger(
      source.httpWorkerFactor,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.httpWorkerFactor,
      1,
    ),
    httpWorkerMin: normalizePositiveInteger(
      source.httpWorkerMin,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.httpWorkerMin,
      1,
    ),
    httpStreamWorkerFactor: normalizePositiveInteger(
      source.httpStreamWorkerFactor,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.httpStreamWorkerFactor,
      1,
    ),
    httpStreamWorkerMin: normalizePositiveInteger(
      source.httpStreamWorkerMin,
      DEFAULT_BACKGROUND_TASKS_SETTINGS.httpStreamWorkerMin,
      1,
    ),
  };
}

function readBackgroundTasksSetting() {
  return normalizeBackgroundTasksSettings(appSettingsSnapshot.backgroundTasks);
}

function saveBackgroundTasksSetting(settings) {
  patchAppSettingsSnapshot({
    backgroundTasks: normalizeBackgroundTasksSettings(settings),
  });
}

function setBackgroundTasksForm(settings) {
  const normalized = normalizeBackgroundTasksSettings(settings);
  if (dom.backgroundUsagePollingEnabled) {
    dom.backgroundUsagePollingEnabled.checked = normalized.usagePollingEnabled;
  }
  if (dom.backgroundUsagePollIntervalSecs) {
    dom.backgroundUsagePollIntervalSecs.value = String(normalized.usagePollIntervalSecs);
  }
  if (dom.backgroundGatewayKeepaliveEnabled) {
    dom.backgroundGatewayKeepaliveEnabled.checked = normalized.gatewayKeepaliveEnabled;
  }
  if (dom.backgroundGatewayKeepaliveIntervalSecs) {
    dom.backgroundGatewayKeepaliveIntervalSecs.value = String(normalized.gatewayKeepaliveIntervalSecs);
  }
  if (dom.backgroundTokenRefreshPollingEnabled) {
    dom.backgroundTokenRefreshPollingEnabled.checked = normalized.tokenRefreshPollingEnabled;
  }
  if (dom.backgroundTokenRefreshPollIntervalSecs) {
    dom.backgroundTokenRefreshPollIntervalSecs.value = String(normalized.tokenRefreshPollIntervalSecs);
  }
  if (dom.backgroundUsageRefreshWorkers) {
    dom.backgroundUsageRefreshWorkers.value = String(normalized.usageRefreshWorkers);
  }
  if (dom.backgroundHttpWorkerFactor) {
    dom.backgroundHttpWorkerFactor.value = String(normalized.httpWorkerFactor);
  }
  if (dom.backgroundHttpWorkerMin) {
    dom.backgroundHttpWorkerMin.value = String(normalized.httpWorkerMin);
  }
  if (dom.backgroundHttpStreamWorkerFactor) {
    dom.backgroundHttpStreamWorkerFactor.value = String(normalized.httpStreamWorkerFactor);
  }
  if (dom.backgroundHttpStreamWorkerMin) {
    dom.backgroundHttpStreamWorkerMin.value = String(normalized.httpStreamWorkerMin);
  }
}

function readBackgroundTasksForm() {
  const integerFields = [
    ["usagePollIntervalSecs", dom.backgroundUsagePollIntervalSecs, "用量轮询间隔"],
    ["gatewayKeepaliveIntervalSecs", dom.backgroundGatewayKeepaliveIntervalSecs, "网关保活间隔"],
    ["tokenRefreshPollIntervalSecs", dom.backgroundTokenRefreshPollIntervalSecs, "令牌刷新间隔"],
    ["usageRefreshWorkers", dom.backgroundUsageRefreshWorkers, "用量刷新线程数"],
    ["httpWorkerFactor", dom.backgroundHttpWorkerFactor, "普通请求线程因子"],
    ["httpWorkerMin", dom.backgroundHttpWorkerMin, "普通请求最小线程数"],
    ["httpStreamWorkerFactor", dom.backgroundHttpStreamWorkerFactor, "流式请求线程因子"],
    ["httpStreamWorkerMin", dom.backgroundHttpStreamWorkerMin, "流式请求最小线程数"],
  ];
  const numbers = {};
  for (const [key, input, label] of integerFields) {
    const raw = input ? String(input.value || "").trim() : "";
    const parsed = Number(raw);
    if (!Number.isFinite(parsed) || parsed <= 0 || Math.floor(parsed) !== parsed) {
      return { ok: false, error: `${label} 需填写正整数` };
    }
    numbers[key] = parsed;
  }
  return {
    ok: true,
    settings: normalizeBackgroundTasksSettings({
      usagePollingEnabled: dom.backgroundUsagePollingEnabled
        ? Boolean(dom.backgroundUsagePollingEnabled.checked)
        : DEFAULT_BACKGROUND_TASKS_SETTINGS.usagePollingEnabled,
      usagePollIntervalSecs: numbers.usagePollIntervalSecs,
      gatewayKeepaliveEnabled: dom.backgroundGatewayKeepaliveEnabled
        ? Boolean(dom.backgroundGatewayKeepaliveEnabled.checked)
        : DEFAULT_BACKGROUND_TASKS_SETTINGS.gatewayKeepaliveEnabled,
      gatewayKeepaliveIntervalSecs: numbers.gatewayKeepaliveIntervalSecs,
      tokenRefreshPollingEnabled: dom.backgroundTokenRefreshPollingEnabled
        ? Boolean(dom.backgroundTokenRefreshPollingEnabled.checked)
        : DEFAULT_BACKGROUND_TASKS_SETTINGS.tokenRefreshPollingEnabled,
      tokenRefreshPollIntervalSecs: numbers.tokenRefreshPollIntervalSecs,
      usageRefreshWorkers: numbers.usageRefreshWorkers,
      httpWorkerFactor: numbers.httpWorkerFactor,
      httpWorkerMin: numbers.httpWorkerMin,
      httpStreamWorkerFactor: numbers.httpStreamWorkerFactor,
      httpStreamWorkerMin: numbers.httpStreamWorkerMin,
    }),
  };
}

function resolveBackgroundTasksSettingsFromPayload(payload) {
  return normalizeBackgroundTasksSettings({
    usagePollingEnabled: pickBooleanValue(payload, [
      "usagePollingEnabled",
      "result.usagePollingEnabled",
    ]),
    usagePollIntervalSecs: pickFirstValue(payload, [
      "usagePollIntervalSecs",
      "result.usagePollIntervalSecs",
    ]),
    gatewayKeepaliveEnabled: pickBooleanValue(payload, [
      "gatewayKeepaliveEnabled",
      "result.gatewayKeepaliveEnabled",
    ]),
    gatewayKeepaliveIntervalSecs: pickFirstValue(payload, [
      "gatewayKeepaliveIntervalSecs",
      "result.gatewayKeepaliveIntervalSecs",
    ]),
    tokenRefreshPollingEnabled: pickBooleanValue(payload, [
      "tokenRefreshPollingEnabled",
      "result.tokenRefreshPollingEnabled",
    ]),
    tokenRefreshPollIntervalSecs: pickFirstValue(payload, [
      "tokenRefreshPollIntervalSecs",
      "result.tokenRefreshPollIntervalSecs",
    ]),
    usageRefreshWorkers: pickFirstValue(payload, [
      "usageRefreshWorkers",
      "result.usageRefreshWorkers",
    ]),
    httpWorkerFactor: pickFirstValue(payload, [
      "httpWorkerFactor",
      "result.httpWorkerFactor",
    ]),
    httpWorkerMin: pickFirstValue(payload, [
      "httpWorkerMin",
      "result.httpWorkerMin",
    ]),
    httpStreamWorkerFactor: pickFirstValue(payload, [
      "httpStreamWorkerFactor",
      "result.httpStreamWorkerFactor",
    ]),
    httpStreamWorkerMin: pickFirstValue(payload, [
      "httpStreamWorkerMin",
      "result.httpStreamWorkerMin",
    ]),
  });
}

function resolveBackgroundTasksRestartKeys(payload) {
  const raw = pickFirstValue(payload, [
    "requiresRestartKeys",
    "result.requiresRestartKeys",
  ]);
  if (!Array.isArray(raw)) {
    return [...BACKGROUND_TASKS_RESTART_KEYS_DEFAULT];
  }
  return raw
    .map((item) => String(item || "").trim())
    .filter((item) => item.length > 0);
}

function updateBackgroundTasksHint(requiresRestartKeys) {
  if (!dom.backgroundTasksHint) {
    return;
  }
  const keys = Array.isArray(requiresRestartKeys) ? requiresRestartKeys : [];
  if (keys.length === 0) {
    dom.backgroundTasksHint.textContent = "保存后立即生效。";
    return;
  }
  const labels = keys.map((key) => BACKGROUND_TASKS_RESTART_KEY_LABELS[key] || key);
  dom.backgroundTasksHint.textContent = `已保存。以下参数需重启服务生效：${labels.join("、")}。`;
}

function initBackgroundTasksSetting() {
  const settings = readBackgroundTasksSetting();
  setBackgroundTasksForm(settings);
  updateBackgroundTasksHint(BACKGROUND_TASKS_RESTART_KEYS_DEFAULT);
}

async function applyBackgroundTasksToService(settings, { silent = true } = {}) {
  const normalized = normalizeBackgroundTasksSettings(settings);
  if (backgroundTasksSyncInFlight) {
    return backgroundTasksSyncInFlight;
  }
  backgroundTasksSyncInFlight = (async () => {
    const connected = await ensureConnected();
    serviceLifecycle.updateServiceToggle();
    if (!connected) {
      if (!silent) {
        showToast("服务未连接，稍后会自动应用后台任务配置", "error");
      }
      return false;
    }
    const response = await serviceGatewayBackgroundTasksSet(normalized);
    const applied = resolveBackgroundTasksSettingsFromPayload(response);
    const restartKeys = resolveBackgroundTasksRestartKeys(response);
    saveBackgroundTasksSetting(applied);
    setBackgroundTasksForm(applied);
    updateBackgroundTasksHint(restartKeys);
    backgroundTasksSyncedProbeId = state.serviceProbeId;
    if (!silent) {
      showToast("后台任务配置已保存");
    }
    return true;
  })();

  try {
    return await backgroundTasksSyncInFlight;
  } catch (err) {
    if (!silent) {
      showToast(`保存失败：${normalizeErrorMessage(err)}`, "error");
    }
    return false;
  } finally {
    backgroundTasksSyncInFlight = null;
  }
}

async function syncBackgroundTasksOnStartup() {
  if (!isTauriRuntime()) {
    return;
  }
  await applyBackgroundTasksToService(readBackgroundTasksSetting(), { silent: true });
}

function readEnvOverridesSetting() {
  return normalizeEnvOverrides(appSettingsSnapshot.envOverrides);
}

function saveEnvOverridesSetting(value) {
  patchAppSettingsSnapshot({
    envOverrides: normalizeEnvOverrides(value),
  });
}

function setEnvOverridesInput(value) {
  if (!dom.envOverridesInput) {
    return;
  }
  dom.envOverridesInput.value = formatEnvOverridesText(value);
}

function setEnvOverridesHint(message) {
  if (!dom.envOverridesHint) {
    return;
  }
  dom.envOverridesHint.textContent = String(message || "").trim()
    || "保存后会回灌到当前 service 进程；启动类配置通常需要重启。";
}

function renderEnvOverrideChipList(element, entries, { mode } = {}) {
  if (!element) {
    return;
  }
  element.replaceChildren();
  const items = Array.isArray(entries) ? entries : [];
  if (items.length === 0) {
    const empty = document.createElement("span");
    empty.className = "hint";
    empty.textContent = "无";
    element.appendChild(empty);
    return;
  }
  for (const item of items) {
    const chip = document.createElement("code");
    chip.className = "settings-env-chip";
    chip.textContent = item.key || item;
    if (item && typeof item === "object") {
      chip.dataset.mode = item.applyMode || mode || "";
      chip.dataset.scope = item.scope || "";
      chip.title = `${item.scope || "service"} / ${item.applyMode || mode || "runtime"}`;
    } else if (mode) {
      chip.dataset.mode = mode;
    }
    element.appendChild(chip);
  }
}

function renderEnvOverrideCatalog() {
  const catalog = Array.isArray(appSettingsSnapshot.envOverrideCatalog)
    ? appSettingsSnapshot.envOverrideCatalog
    : [];
  renderEnvOverrideChipList(
    dom.envOverrideCatalogRuntime,
    catalog.filter((item) => item.applyMode === "runtime"),
    { mode: "runtime" },
  );
  renderEnvOverrideChipList(
    dom.envOverrideCatalogRestart,
    catalog.filter((item) => item.applyMode === "restart"),
    { mode: "restart" },
  );
  renderEnvOverrideChipList(dom.envOverrideReservedKeys, appSettingsSnapshot.envOverrideReservedKeys);
  renderEnvOverrideChipList(dom.envOverrideUnsupportedKeys, appSettingsSnapshot.envOverrideUnsupportedKeys);
}

function validateEnvOverridesForSave(overrides) {
  const keys = Object.keys(normalizeEnvOverrides(overrides));
  const unsupported = new Set(appSettingsSnapshot.envOverrideUnsupportedKeys || []);
  const reserved = new Set(appSettingsSnapshot.envOverrideReservedKeys || []);
  const unsupportedKeys = keys.filter((key) => unsupported.has(key));
  if (unsupportedKeys.length > 0) {
    return `以下键不能改为数据库配置：${unsupportedKeys.join("、")}`;
  }
  const reservedKeys = keys.filter((key) => reserved.has(key));
  if (reservedKeys.length > 0) {
    return `以下键已有专用设置项，请在对应卡片中修改：${reservedKeys.join("、")}`;
  }
  return "";
}

function buildEnvOverridesSaveHint(previousOverrides, nextOverrides) {
  const previous = normalizeEnvOverrides(previousOverrides);
  const next = normalizeEnvOverrides(nextOverrides);
  const changedKeys = [...new Set([...Object.keys(previous), ...Object.keys(next)])]
    .filter((key) => previous[key] !== next[key]);
  if (changedKeys.length === 0) {
    return "配置未变化。";
  }

  const catalog = new Map(
    (Array.isArray(appSettingsSnapshot.envOverrideCatalog) ? appSettingsSnapshot.envOverrideCatalog : [])
      .map((item) => [item.key, item]),
  );
  const restartKeys = [];
  const unknownKeys = [];
  for (const key of changedKeys) {
    const item = catalog.get(key);
    if (!item) {
      unknownKeys.push(key);
      continue;
    }
    if (item.applyMode === "restart") {
      restartKeys.push(key);
    }
  }

  if (restartKeys.length === 0 && unknownKeys.length === 0) {
    return "已保存并回灌到当前 service 进程。";
  }

  const parts = ["已保存"];
  if (restartKeys.length > 0) {
    const preview = restartKeys.slice(0, 6).join("、");
    const suffix = restartKeys.length > 6 ? ` 等 ${restartKeys.length} 项` : "";
    parts.push(`以下键需重启相关进程后完整生效：${preview}${suffix}`);
  }
  if (unknownKeys.length > 0) {
    const preview = unknownKeys.slice(0, 4).join("、");
    const suffix = unknownKeys.length > 4 ? ` 等 ${unknownKeys.length} 项` : "";
    parts.push(`未识别键会按原样保存：${preview}${suffix}`);
  }
  return `${parts.join("；")}。`;
}

function initEnvOverridesSetting() {
  setEnvOverridesInput(readEnvOverridesSetting());
  renderEnvOverrideCatalog();
  setEnvOverridesHint("保存后会回灌到当前 service 进程；启动类配置通常需要重启。");
}

function buildWebAccessPasswordStatusText(configured) {
  return configured
    ? "当前已启用 Web 访问密码。修改后会立即覆盖旧密码。"
    : "当前未启用 Web 访问密码。";
}

function updateWebAccessPasswordState(configured) {
  const enabled = Boolean(configured);
  patchAppSettingsSnapshot({ webAccessPasswordConfigured: enabled });
  const text = buildWebAccessPasswordStatusText(enabled);
  if (dom.webAccessPasswordHint) {
    dom.webAccessPasswordHint.textContent = text;
  }
  if (dom.webAccessPasswordQuickStatus) {
    dom.webAccessPasswordQuickStatus.textContent = text;
  }
}

function readWebAccessPasswordPair(source = "settings") {
  const useQuick = source === "quick";
  const password = useQuick
    ? (dom.webAccessPasswordQuickInput ? dom.webAccessPasswordQuickInput.value : "")
    : (dom.webAccessPasswordInput ? dom.webAccessPasswordInput.value : "");
  const confirm = useQuick
    ? (dom.webAccessPasswordQuickConfirm ? dom.webAccessPasswordQuickConfirm.value : "")
    : (dom.webAccessPasswordConfirm ? dom.webAccessPasswordConfirm.value : "");
  return {
    password: String(password || ""),
    confirm: String(confirm || ""),
  };
}

function syncWebAccessPasswordInputs(source = "settings") {
  const pair = readWebAccessPasswordPair(source);
  if (dom.webAccessPasswordInput) {
    dom.webAccessPasswordInput.value = pair.password;
  }
  if (dom.webAccessPasswordConfirm) {
    dom.webAccessPasswordConfirm.value = pair.confirm;
  }
  if (dom.webAccessPasswordQuickInput) {
    dom.webAccessPasswordQuickInput.value = pair.password;
  }
  if (dom.webAccessPasswordQuickConfirm) {
    dom.webAccessPasswordQuickConfirm.value = pair.confirm;
  }
}

function clearWebAccessPasswordInputs() {
  if (dom.webAccessPasswordInput) {
    dom.webAccessPasswordInput.value = "";
  }
  if (dom.webAccessPasswordConfirm) {
    dom.webAccessPasswordConfirm.value = "";
  }
  if (dom.webAccessPasswordQuickInput) {
    dom.webAccessPasswordQuickInput.value = "";
  }
  if (dom.webAccessPasswordQuickConfirm) {
    dom.webAccessPasswordQuickConfirm.value = "";
  }
}

function openWebSecurityModal() {
  if (!dom.modalWebSecurity) {
    return;
  }
  syncWebAccessPasswordInputs("settings");
  updateWebAccessPasswordState(appSettingsSnapshot.webAccessPasswordConfigured);
  dom.modalWebSecurity.classList.add("active");
}

function closeWebSecurityModal() {
  if (!dom.modalWebSecurity) {
    return;
  }
  dom.modalWebSecurity.classList.remove("active");
}

async function saveWebAccessPassword(source = "settings") {
  const pair = readWebAccessPasswordPair(source);
  const password = pair.password.trim();
  if (!password) {
    showToast("请输入 Web 访问密码；如需关闭保护请点击清除", "error");
    return false;
  }
  if (pair.password !== pair.confirm) {
    showToast("两次输入的 Web 访问密码不一致", "error");
    return false;
  }
  try {
    const settings = await saveAppSettingsPatch({
      webAccessPassword: pair.password,
    });
    updateWebAccessPasswordState(settings.webAccessPasswordConfigured);
    clearWebAccessPasswordInputs();
    if (source === "quick") {
      closeWebSecurityModal();
    }
    showToast("Web 访问密码已保存");
    return true;
  } catch (err) {
    showToast(`保存失败：${normalizeErrorMessage(err)}`, "error");
    return false;
  }
}

async function clearWebAccessPassword(source = "settings") {
  try {
    const settings = await saveAppSettingsPatch({
      webAccessPassword: "",
    });
    updateWebAccessPasswordState(settings.webAccessPasswordConfigured);
    clearWebAccessPasswordInputs();
    if (source === "quick") {
      closeWebSecurityModal();
    }
    showToast("Web 访问密码已清除");
    return true;
  } catch (err) {
    showToast(`清除失败：${normalizeErrorMessage(err)}`, "error");
    return false;
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
  if (pendingUpdateCandidate && pendingUpdateCandidate.version && pendingUpdateCandidate.canPrepare) {
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

      if (!checkInfo.canPrepare) {
        pendingUpdateCandidate = null;
        setCheckUpdateButtonLabel();
        const msg = checkInfo.reason || `发现新版本${buildVersionLabel(checkInfo.version)}，当前仅可查看版本`;
        setUpdateStatusText(msg);
        if (!silentIfLatest) {
          showToast(msg);
        }
        return true;
      }

      pendingUpdateCandidate = {
        version: checkInfo.version,
        isPortable: checkInfo.isPortable,
        canPrepare: true,
      };
      setCheckUpdateButtonLabel();

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
  const hasPreparedCheck = Boolean(
    pendingUpdateCandidate && pendingUpdateCandidate.version && pendingUpdateCandidate.canPrepare
  );
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

function readLastApiModelsRemoteRefreshAt() {
  if (typeof localStorage === "undefined") {
    return 0;
  }
  const raw = localStorage.getItem(API_MODELS_REMOTE_REFRESH_STORAGE_KEY);
  const parsed = Number(raw);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

function writeLastApiModelsRemoteRefreshAt(ts = Date.now()) {
  if (typeof localStorage === "undefined") {
    return;
  }
  localStorage.setItem(API_MODELS_REMOTE_REFRESH_STORAGE_KEY, String(Math.max(0, Math.floor(ts))));
}

function shouldRefreshApiModelsRemote(force = false) {
  if (force) {
    return true;
  }
  const hasLocalCache = Array.isArray(state.apiModelOptions) && state.apiModelOptions.length > 0;
  if (!hasLocalCache) {
    return true;
  }
  const lastRefreshAt = readLastApiModelsRemoteRefreshAt();
  if (lastRefreshAt <= 0) {
    return true;
  }
  return (Date.now() - lastRefreshAt) >= API_MODELS_REMOTE_REFRESH_INTERVAL_MS;
}

async function maybeRefreshApiModelsCache(options = {}) {
  const force = options && options.force === true;
  if (!shouldRefreshApiModelsRemote(force)) {
    return false;
  }
  if (apiModelsRemoteRefreshInFlight) {
    return apiModelsRemoteRefreshInFlight;
  }
  apiModelsRemoteRefreshInFlight = (async () => {
    const connected = await ensureConnected();
    if (!connected) {
      return false;
    }
    await refreshApiModels({ refreshRemote: true });
    writeLastApiModelsRemoteRefreshAt(Date.now());
    if (dom.modalApiKey && dom.modalApiKey.classList.contains("active")) {
      populateApiKeyModelSelect();
    }
    if (state.currentPage === "apikeys") {
      renderCurrentPageView("apikeys");
    }
    return true;
  })();
  try {
    return await apiModelsRemoteRefreshInFlight;
  } catch (err) {
    console.error("[api-models] remote refresh failed", err);
    return false;
  } finally {
    apiModelsRemoteRefreshInFlight = null;
  }
}

async function refreshAll(options = {}) {
  if (refreshAllInFlight) {
    return refreshAllInFlight;
  }
  refreshAllInFlight = (async () => {
    const tasks = buildRefreshAllTasks(options);
    const total = tasks.length;
    let completed = 0;
    const setProgress = (next) => {
      renderAccountsRefreshProgress(setRefreshAllProgress(next));
    };
    setProgress({ active: true, manual: false, total, completed: 0, remaining: total, lastTaskLabel: "" });

    const ok = await ensureConnected();
    serviceLifecycle.updateServiceToggle();
    if (!ok) return [];
    if (isTauriRuntime() && routeStrategySyncedProbeId !== state.serviceProbeId) {
      await applyRouteStrategyToService(readRouteStrategySetting(), { silent: true });
    }
    if (isTauriRuntime() && cpaNoCookieHeaderModeSyncedProbeId !== state.serviceProbeId) {
      await applyCpaNoCookieHeaderModeToService(readCpaNoCookieHeaderModeSetting(), { silent: true });
    }
    if (isTauriRuntime() && upstreamProxySyncedProbeId !== state.serviceProbeId) {
      await applyUpstreamProxyToService(readUpstreamProxyUrlSetting(), { silent: true });
    }
    if (isTauriRuntime() && backgroundTasksSyncedProbeId !== state.serviceProbeId) {
      await applyBackgroundTasksToService(readBackgroundTasksSetting(), { silent: true });
    }

    // 中文注释：全并发会制造瞬时抖动（同时多次 RPC/DOM 更新）；这里改为有限并发并统一限流上限。
    const results = await runRefreshTasks(
      tasks.map((task) => ({
        ...task,
        run: async () => {
          try {
            return await task.run();
          } finally {
            completed += 1;
            setProgress({
              active: true,
              manual: false,
              total,
              completed,
              remaining: total - completed,
              lastTaskLabel: task.label || task.name,
            });
            await nextPaintTick();
          }
        },
      })),
      (taskName, err) => {
        console.error(`[refreshAll] ${taskName} failed`, err);
      },
      {
        concurrency: options.concurrency,
      },
    );
    if (options.refreshRemoteModels === true) {
      const modelTask = results.find((item) => item.name === "api-models");
      if (modelTask && modelTask.status === "fulfilled") {
        writeLastApiModelsRemoteRefreshAt(Date.now());
      }
    }
    // 中文注释：并行刷新时允许“部分失败部分成功”，否则某个慢/失败接口会拖垮整页刷新体验。
    const failedTasks = results.filter((item) => item.status === "rejected");
    if (failedTasks.length > 0) {
      const taskLabelMap = new Map(tasks.map((task) => [task.name, task.label || task.name]));
      const failedLabels = [...new Set(failedTasks.map((task) => taskLabelMap.get(task.name) || task.name))];
      const failedLabelText = failedLabels.length > 3
        ? `${failedLabels.slice(0, 3).join("、")} 等${failedLabels.length}项`
        : failedLabels.join("、");
      const firstFailedMessage = normalizeErrorMessage(failedTasks[0].reason);
      // 中文注释：自动刷新触发的失败仅记日志，避免每分钟弹错打断；手动刷新才提示具体失败项。
      if (options.manual === true) {
        const detail = firstFailedMessage ? `（示例错误：${firstFailedMessage}）` : "";
        showToast(`部分数据刷新失败：${failedLabelText}，已展示可用数据${detail}`, "error");
      } else {
        console.warn(
          `[refreshAll] 部分失败：${failedLabelText}；首个错误：${firstFailedMessage || "未知"}`,
        );
      }
    }
    renderCurrentPageView();
  })();
  try {
    return await refreshAllInFlight;
  } finally {
    refreshAllInFlight = null;
    if (refreshAllProgressClearTimer) {
      clearTimeout(refreshAllProgressClearTimer);
    }
    refreshAllProgressClearTimer = setTimeout(() => {
      renderAccountsRefreshProgress(clearRefreshAllProgress());
      refreshAllProgressClearTimer = null;
    }, 450);
  }
}

async function handleRefreshAllClick() {
  await withButtonBusy(dom.refreshAll, "刷新中...", async () => {
    // 中文注释：先让浏览器绘制 loading 态，避免用户感知“点击后卡住”。
    if (refreshAllProgressClearTimer) {
      clearTimeout(refreshAllProgressClearTimer);
      refreshAllProgressClearTimer = null;
    }
    renderAccountsRefreshProgress(setRefreshAllProgress({
      active: true,
      manual: true,
      total: 1,
      completed: 0,
      remaining: 1,
      lastTaskLabel: "",
    }));
    await nextPaintTick();
    const ok = await ensureConnected();
    serviceLifecycle.updateServiceToggle();
    if (!ok) {
      return;
    }
    let accounts = Array.isArray(state.accountList) ? state.accountList.filter((item) => item && item.id) : [];
  if (accounts.length === 0) {
    try {
      await refreshAccounts();
      await refreshAccountsPage({ latestOnly: true }).catch(() => false);
    } catch (err) {
      console.error("[refreshUsageOnly] load accounts failed", err);
    }
      accounts = Array.isArray(state.accountList) ? state.accountList.filter((item) => item && item.id) : [];
    }
    const total = accounts.length;
    if (total <= 0) {
      renderAccountsRefreshProgress(setRefreshAllProgress({
        active: true,
        manual: true,
        total: 1,
        completed: 1,
        remaining: 0,
        lastTaskLabel: "无可刷新账号",
      }));
      return;
    }
    renderAccountsRefreshProgress(setRefreshAllProgress({
      active: true,
      manual: true,
      total,
      completed: 0,
      remaining: total,
      lastTaskLabel: "",
    }));

    let completed = 0;
    let failed = 0;
    try {
      for (const account of accounts) {
        const label = String(account.label || account.id || "").trim() || "未知账号";
        try {
          await serviceUsageRefresh(account.id);
        } catch (err) {
          failed += 1;
          console.error(`[refreshUsageOnly] account refresh failed: ${account.id}`, err);
        } finally {
          completed += 1;
          renderAccountsRefreshProgress(setRefreshAllProgress({
            active: true,
            manual: true,
            total,
            completed,
            remaining: Math.max(0, total - completed),
            lastTaskLabel: label,
          }));
        }
      }
      await refreshUsageList({ refreshRemote: false });
      renderCurrentPageView("accounts");
      if (failed > 0) {
        showToast(`用量刷新完成，失败 ${failed}/${total}`, "error");
      }
    } catch (err) {
      console.error("[refreshUsageOnly] failed", err);
      showToast("账号用量刷新失败，请稍后重试", "error");
    } finally {
      if (refreshAllProgressClearTimer) {
        clearTimeout(refreshAllProgressClearTimer);
      }
      refreshAllProgressClearTimer = setTimeout(() => {
        renderAccountsRefreshProgress(clearRefreshAllProgress());
        refreshAllProgressClearTimer = null;
      }, 450);
    }
  });
}

async function refreshAccountsAndUsage() {
  const options = arguments[0] || {};
  const includeUsage = options.includeUsage !== false;
  const includeAccountPage = options.includeAccountPage !== false && state.currentPage === "accounts";
  const ok = await ensureConnected();
  serviceLifecycle.updateServiceToggle();
  if (!ok) return false;

  const tasks = [{ name: "accounts", run: refreshAccounts }];
  if (includeUsage) {
    tasks.push({ name: "usage", run: refreshUsageList });
  }
  const results = await runRefreshTasks(
    tasks,
    (taskName, err) => {
      console.error(`[refreshAccountsAndUsage] ${taskName} failed`, err);
    },
  );
  const failed = results.some((item) => item.status === "rejected");
  if (failed) {
    return false;
  }
  if (includeAccountPage) {
    try {
      await refreshAccountsPage({ latestOnly: true });
    } catch (err) {
      console.error("[refreshAccountsAndUsage] account-page failed", err);
      return false;
    }
  }
  return true;
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
  maybeRefreshApiModelsCache,
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
  renderCurrentPageView,
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
  setManualPreferredAccount,
  deleteAccount,
  importAccountsFromFiles,
  importAccountsFromDirectory,
  deleteSelectedAccounts,
  deleteUnavailableFreeAccounts,
  exportAccountsByFile,
  handleOpenUsageModal,
  refreshUsageForAccount,
  createApiKey,
  deleteApiKey,
  toggleApiKeyStatus,
  updateApiKeyModel,
  copyApiKey,
  refreshApiModelsNow,
} = managementActions;

function buildMainRenderActions() {
  return buildRenderActions({
    updateAccountSort,
    handleOpenUsageModal,
    setManualPreferredAccount,
    deleteAccount,
    refreshAccountsPage: () => reloadAccountsPage({ latestOnly: true, silent: false }),
    toggleApiKeyStatus,
    deleteApiKey,
    updateApiKeyModel,
    copyApiKey,
  });
}

function renderAccountsView() {
  renderAccountsOnly(buildMainRenderActions());
}

async function persistServiceAddrInput({ silent = true } = {}) {
  if (!dom.serviceAddrInput) {
    return false;
  }
  let normalized = "";
  try {
    normalized = normalizeAddr(dom.serviceAddrInput.value || "");
  } catch (err) {
    if (!silent) {
      showToast(`服务地址格式不正确：${normalizeErrorMessage(err)}`, "error");
    }
    return false;
  }
  dom.serviceAddrInput.value = normalized;
  state.serviceAddr = normalized;
  patchAppSettingsSnapshot({ serviceAddr: normalized });
  try {
    await saveAppSettingsPatch({
      serviceAddr: normalized,
    });
    return true;
  } catch (err) {
    if (!silent) {
      showToast(`保存服务地址失败：${normalizeErrorMessage(err)}`, "error");
    }
    return false;
  }
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
    handleCancelLogin: loginFlow.handleCancelLogin,
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
    refreshApiModelsNow,
    populateApiKeyModelSelect,
    importAccountsFromFiles,
    importAccountsFromDirectory,
    deleteSelectedAccounts,
    deleteUnavailableFreeAccounts,
    exportAccountsByFile,
    toggleThemePanel,
    closeThemePanel,
    setTheme,
    handleServiceToggle: serviceLifecycle.handleServiceToggle,
    renderAccountsView,
    refreshAccountsPage: (options) => reloadAccountsPage(options),
    updateRequestLogFilterButtons,
  });

  if (dom.autoCheckUpdate && dom.autoCheckUpdate.dataset.bound !== "1") {
    dom.autoCheckUpdate.dataset.bound = "1";
    dom.autoCheckUpdate.addEventListener("change", () => {
      const previousEnabled = readUpdateAutoCheckSetting();
      const enabled = Boolean(dom.autoCheckUpdate.checked);
      saveUpdateAutoCheckSetting(enabled);
      void saveAppSettingsPatch({
        updateAutoCheck: enabled,
      }).catch((err) => {
        saveUpdateAutoCheckSetting(previousEnabled);
        dom.autoCheckUpdate.checked = previousEnabled;
        showToast(`保存失败：${normalizeErrorMessage(err)}`, "error");
      });
    });
  }
  if (dom.checkUpdate && dom.checkUpdate.dataset.bound !== "1") {
    dom.checkUpdate.dataset.bound = "1";
    dom.checkUpdate.addEventListener("click", () => {
      void handleCheckUpdateClick();
    });
  }
  if (dom.closeToTrayOnClose && dom.closeToTrayOnClose.dataset.bound !== "1") {
    dom.closeToTrayOnClose.dataset.bound = "1";
    dom.closeToTrayOnClose.addEventListener("change", () => {
      const previousEnabled = readCloseToTrayOnCloseSetting();
      const enabled = Boolean(dom.closeToTrayOnClose.checked);
      void applyCloseToTrayOnCloseSetting(enabled, { silent: false }).then((applied) => {
        saveCloseToTrayOnCloseSetting(applied);
        setCloseToTrayOnCloseToggle(applied);
      }).catch(() => {
        saveCloseToTrayOnCloseSetting(previousEnabled);
        setCloseToTrayOnCloseToggle(previousEnabled);
      });
    });
  }
  if (dom.lightweightModeOnCloseToTray && dom.lightweightModeOnCloseToTray.dataset.bound !== "1") {
    dom.lightweightModeOnCloseToTray.dataset.bound = "1";
    dom.lightweightModeOnCloseToTray.addEventListener("change", () => {
      const previousEnabled = readLightweightModeOnCloseToTraySetting();
      const enabled = Boolean(dom.lightweightModeOnCloseToTray.checked);
      void applyLightweightModeOnCloseToTraySetting(enabled, { silent: false }).then((applied) => {
        saveLightweightModeOnCloseToTraySetting(applied);
        setLightweightModeOnCloseToTrayToggle(applied);
      }).catch(() => {
        saveLightweightModeOnCloseToTraySetting(previousEnabled);
        setLightweightModeOnCloseToTrayToggle(previousEnabled);
      });
    });
  }
  if (dom.routeStrategySelect && dom.routeStrategySelect.dataset.bound !== "1") {
    dom.routeStrategySelect.dataset.bound = "1";
    dom.routeStrategySelect.addEventListener("change", () => {
      const previousSelected = readRouteStrategySetting();
      const selected = normalizeRouteStrategy(dom.routeStrategySelect.value);
      saveRouteStrategySetting(selected);
      setRouteStrategySelect(selected);
      void saveAppSettingsPatch({
        routeStrategy: selected,
      }).then((settings) => {
        const resolved = normalizeRouteStrategy(settings.routeStrategy);
        saveRouteStrategySetting(resolved);
        setRouteStrategySelect(resolved);
        if (isTauriRuntime()) {
          return applyRouteStrategyToService(resolved, { silent: false });
        }
        showToast(`已切换为${routeStrategyLabel(resolved)}`);
        return true;
      }).catch((err) => {
        saveRouteStrategySetting(previousSelected);
        setRouteStrategySelect(previousSelected);
        showToast(`切换失败：${normalizeErrorMessage(err)}`, "error");
      });
    });
  }
  if (dom.serviceListenModeSelect && dom.serviceListenModeSelect.dataset.bound !== "1") {
    dom.serviceListenModeSelect.dataset.bound = "1";
    dom.serviceListenModeSelect.addEventListener("change", () => {
      const previousSelected = normalizeServiceListenMode(appSettingsSnapshot.serviceListenMode);
      const selected = normalizeServiceListenMode(dom.serviceListenModeSelect.value);
      setServiceListenModeSelect(selected);
      setServiceListenModeHint(buildServiceListenModeHint(selected, true));
      void applyServiceListenModeToService(selected, { silent: false }).then((ok) => {
        if (!ok) {
          setServiceListenModeSelect(previousSelected);
          setServiceListenModeHint(buildServiceListenModeHint(previousSelected, true));
        }
      });
    });
  }
  if (dom.cpaNoCookieHeaderMode && dom.cpaNoCookieHeaderMode.dataset.bound !== "1") {
    dom.cpaNoCookieHeaderMode.dataset.bound = "1";
    dom.cpaNoCookieHeaderMode.addEventListener("change", () => {
      const previousEnabled = readCpaNoCookieHeaderModeSetting();
      const enabled = Boolean(dom.cpaNoCookieHeaderMode.checked);
      saveCpaNoCookieHeaderModeSetting(enabled);
      setCpaNoCookieHeaderModeToggle(enabled);
      void saveAppSettingsPatch({
        cpaNoCookieHeaderModeEnabled: enabled,
      }).then((settings) => {
        const resolved = normalizeCpaNoCookieHeaderMode(settings.cpaNoCookieHeaderModeEnabled);
        saveCpaNoCookieHeaderModeSetting(resolved);
        setCpaNoCookieHeaderModeToggle(resolved);
        if (isTauriRuntime()) {
          return applyCpaNoCookieHeaderModeToService(resolved, { silent: false });
        }
        showToast(resolved ? "已启用请求头收敛策略" : "已关闭请求头收敛策略");
        return true;
      }).catch((err) => {
        saveCpaNoCookieHeaderModeSetting(previousEnabled);
        setCpaNoCookieHeaderModeToggle(previousEnabled);
        showToast(`切换失败：${normalizeErrorMessage(err)}`, "error");
      });
    });
  }
  if (dom.upstreamProxySave && dom.upstreamProxySave.dataset.bound !== "1") {
    dom.upstreamProxySave.dataset.bound = "1";
    dom.upstreamProxySave.addEventListener("click", () => {
      void withButtonBusy(dom.upstreamProxySave, "保存中...", async () => {
        const previousValue = readUpstreamProxyUrlSetting();
        const value = normalizeUpstreamProxyUrl(dom.upstreamProxyUrlInput ? dom.upstreamProxyUrlInput.value : "");
        saveUpstreamProxyUrlSetting(value);
        setUpstreamProxyInput(value);
        try {
          const settings = await saveAppSettingsPatch({
            upstreamProxyUrl: value,
          });
          const resolved = normalizeUpstreamProxyUrl(settings.upstreamProxyUrl);
          saveUpstreamProxyUrlSetting(resolved);
          setUpstreamProxyInput(resolved);
          if (isTauriRuntime()) {
            await applyUpstreamProxyToService(resolved, { silent: false });
            return;
          }
          setUpstreamProxyHint("保存后立即生效。");
          showToast(resolved ? "上游代理已保存并生效" : "已清空上游代理，恢复直连");
        } catch (err) {
          saveUpstreamProxyUrlSetting(previousValue);
          setUpstreamProxyInput(previousValue);
          setUpstreamProxyHint(`保存失败：${normalizeErrorMessage(err)}`);
          showToast(`保存失败：${normalizeErrorMessage(err)}`, "error");
        }
      });
    });
  }
  if (dom.backgroundTasksSave && dom.backgroundTasksSave.dataset.bound !== "1") {
    dom.backgroundTasksSave.dataset.bound = "1";
    dom.backgroundTasksSave.addEventListener("click", () => {
      void withButtonBusy(dom.backgroundTasksSave, "保存中...", async () => {
        const previousSettings = readBackgroundTasksSetting();
        const parsed = readBackgroundTasksForm();
        if (!parsed.ok) {
          showToast(parsed.error, "error");
          return;
        }
        const nextSettings = parsed.settings;
        saveBackgroundTasksSetting(nextSettings);
        setBackgroundTasksForm(nextSettings);
        try {
          const settings = await saveAppSettingsPatch({
            backgroundTasks: nextSettings,
          });
          const resolved = normalizeBackgroundTasksSettings(settings.backgroundTasks);
          saveBackgroundTasksSetting(resolved);
          setBackgroundTasksForm(resolved);
          if (isTauriRuntime()) {
            await applyBackgroundTasksToService(resolved, { silent: false });
            return;
          }
          updateBackgroundTasksHint([]);
          showToast("后台任务配置已保存");
        } catch (err) {
          saveBackgroundTasksSetting(previousSettings);
          setBackgroundTasksForm(previousSettings);
          updateBackgroundTasksHint(BACKGROUND_TASKS_RESTART_KEYS_DEFAULT);
          showToast(`保存失败：${normalizeErrorMessage(err)}`, "error");
        }
      });
    });
  }
  if (dom.envOverridesSave && dom.envOverridesSave.dataset.bound !== "1") {
    dom.envOverridesSave.dataset.bound = "1";
    dom.envOverridesSave.addEventListener("click", () => {
      void withButtonBusy(dom.envOverridesSave, "保存中...", async () => {
        const previousOverrides = readEnvOverridesSetting();
        const parsed = parseEnvOverridesText(dom.envOverridesInput ? dom.envOverridesInput.value : "");
        if (!parsed.ok) {
          setEnvOverridesHint(parsed.error);
          showToast(parsed.error, "error");
          return;
        }
        const validationError = validateEnvOverridesForSave(parsed.overrides);
        if (validationError) {
          setEnvOverridesHint(validationError);
          showToast(validationError, "error");
          return;
        }
        saveEnvOverridesSetting(parsed.overrides);
        setEnvOverridesInput(parsed.overrides);
        try {
          const settings = await saveAppSettingsPatch({
            envOverrides: parsed.overrides,
          });
          const resolved = normalizeEnvOverrides(settings.envOverrides);
          saveEnvOverridesSetting(resolved);
          setEnvOverridesInput(resolved);
          renderEnvOverrideCatalog();
          setEnvOverridesHint(buildEnvOverridesSaveHint(previousOverrides, resolved));
          showToast("高级环境变量已保存");
        } catch (err) {
          saveEnvOverridesSetting(previousOverrides);
          setEnvOverridesInput(previousOverrides);
          setEnvOverridesHint(`保存失败：${normalizeErrorMessage(err)}`);
          showToast(`保存失败：${normalizeErrorMessage(err)}`, "error");
        }
      });
    });
  }
  if (dom.serviceAddrInput && dom.serviceAddrInput.dataset.bound !== "1") {
    dom.serviceAddrInput.dataset.bound = "1";
    dom.serviceAddrInput.addEventListener("change", () => {
      void persistServiceAddrInput({ silent: false });
    });
    dom.serviceAddrInput.addEventListener("keydown", (event) => {
      if (event.key !== "Enter") {
        return;
      }
      event.preventDefault();
      void persistServiceAddrInput({ silent: false });
    });
  }
  const lowTransparencyToggle = typeof document === "undefined"
    ? null
    : document.getElementById(UI_LOW_TRANSPARENCY_TOGGLE_ID);
  if (lowTransparencyToggle && lowTransparencyToggle.dataset.bound !== "1") {
    lowTransparencyToggle.dataset.bound = "1";
    lowTransparencyToggle.addEventListener("change", () => {
      const previousEnabled = readLowTransparencySetting();
      const enabled = Boolean(lowTransparencyToggle.checked);
      saveLowTransparencySetting(enabled);
      applyLowTransparencySetting(enabled);
      void saveAppSettingsPatch({
        lowTransparency: enabled,
      }).catch((err) => {
        saveLowTransparencySetting(previousEnabled);
        lowTransparencyToggle.checked = previousEnabled;
        applyLowTransparencySetting(previousEnabled);
        showToast(`保存失败：${normalizeErrorMessage(err)}`, "error");
      });
    });
  }
  const syncPairs = [
    [dom.webAccessPasswordInput, "settings"],
    [dom.webAccessPasswordConfirm, "settings"],
    [dom.webAccessPasswordQuickInput, "quick"],
    [dom.webAccessPasswordQuickConfirm, "quick"],
  ];
  for (const [input, source] of syncPairs) {
    if (!input || input.dataset.bound === "1") {
      continue;
    }
    input.dataset.bound = "1";
    input.addEventListener("input", () => {
      syncWebAccessPasswordInputs(source);
    });
  }
  if (dom.webAccessPasswordSave && dom.webAccessPasswordSave.dataset.bound !== "1") {
    dom.webAccessPasswordSave.dataset.bound = "1";
    dom.webAccessPasswordSave.addEventListener("click", () => {
      void withButtonBusy(dom.webAccessPasswordSave, "保存中...", async () => {
        await saveWebAccessPassword("settings");
      });
    });
  }
  if (dom.webAccessPasswordClear && dom.webAccessPasswordClear.dataset.bound !== "1") {
    dom.webAccessPasswordClear.dataset.bound = "1";
    dom.webAccessPasswordClear.addEventListener("click", () => {
      void withButtonBusy(dom.webAccessPasswordClear, "清除中...", async () => {
        await clearWebAccessPassword("settings");
      });
    });
  }
  if (dom.webAccessPasswordQuickSave && dom.webAccessPasswordQuickSave.dataset.bound !== "1") {
    dom.webAccessPasswordQuickSave.dataset.bound = "1";
    dom.webAccessPasswordQuickSave.addEventListener("click", () => {
      void withButtonBusy(dom.webAccessPasswordQuickSave, "保存中...", async () => {
        await saveWebAccessPassword("quick");
      });
    });
  }
  if (dom.webAccessPasswordQuickClear && dom.webAccessPasswordQuickClear.dataset.bound !== "1") {
    dom.webAccessPasswordQuickClear.dataset.bound = "1";
    dom.webAccessPasswordQuickClear.addEventListener("click", () => {
      void withButtonBusy(dom.webAccessPasswordQuickClear, "清除中...", async () => {
        await clearWebAccessPassword("quick");
      });
    });
  }
  if (dom.webSecurityQuickAction && dom.webSecurityQuickAction.dataset.bound !== "1") {
    dom.webSecurityQuickAction.dataset.bound = "1";
    dom.webSecurityQuickAction.addEventListener("click", () => {
      openWebSecurityModal();
    });
  }
  if (dom.closeWebSecurityModal && dom.closeWebSecurityModal.dataset.bound !== "1") {
    dom.closeWebSecurityModal.dataset.bound = "1";
    dom.closeWebSecurityModal.addEventListener("click", () => {
      closeWebSecurityModal();
    });
  }
  if (dom.modalWebSecurity && dom.modalWebSecurity.dataset.bound !== "1") {
    dom.modalWebSecurity.dataset.bound = "1";
    dom.modalWebSecurity.addEventListener("click", (event) => {
      if (event.target === dom.modalWebSecurity) {
        closeWebSecurityModal();
      }
    });
  }
  if (typeof document !== "undefined" && document.body && document.body.dataset.webSecurityBound !== "1") {
    document.body.dataset.webSecurityBound = "1";
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && dom.modalWebSecurity?.classList.contains("active")) {
        closeWebSecurityModal();
      }
    });
  }
}

async function bootstrap() {
  setStartupMask(true, "正在初始化界面...");
  setStatus("", false);
  await loadAppSettings();
  const browserMode = applyBrowserModeUi();
  setServiceHint(browserMode ? "浏览器模式：请先启动 codexmanager-service" : "请输入端口并点击启动", false);
  renderThemeButtons();
  restoreTheme(appSettingsSnapshot.theme);
  initLowTransparencySetting();
  initUpdateAutoCheckSetting();
  initCloseToTrayOnCloseSetting();
  void initLightweightModeOnCloseToTraySetting();
  initServiceListenModeSetting();
  initRouteStrategySetting();
  initCpaNoCookieHeaderModeSetting();
  initUpstreamProxySetting();
  initBackgroundTasksSetting();
  initEnvOverridesSetting();
  updateWebAccessPasswordState(appSettingsSnapshot.webAccessPasswordConfigured);
  void bootstrapUpdateStatus();
  serviceLifecycle.restoreServiceAddr();
  serviceLifecycle.updateServiceToggle();
  bindEvents();
  renderCurrentPageView();
  updateRequestLogFilterButtons();
  scheduleStartupUpdateCheck();
  void serviceLifecycle.autoStartService().finally(() => {
    void syncServiceListenModeOnStartup();
    void syncRouteStrategyOnStartup();
    void syncCpaNoCookieHeaderModeOnStartup();
    void syncUpstreamProxyOnStartup();
    void syncBackgroundTasksOnStartup();
    setStartupMask(false);
  });
}

window.addEventListener("DOMContentLoaded", () => {
  void bootstrap();
});








