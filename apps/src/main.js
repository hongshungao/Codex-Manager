import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/responsive.css";
import "./styles/performance.css";

import {
  serviceGatewayBackgroundTasksGet,
  serviceGatewayBackgroundTasksSet,
  serviceGatewayHeaderPolicyGet,
  serviceGatewayHeaderPolicySet,
  serviceGatewayRouteStrategyGet,
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
const UI_LOW_TRANSPARENCY_STORAGE_KEY = "codexmanager.ui.low_transparency";
const UI_LOW_TRANSPARENCY_BODY_CLASS = "cm-low-transparency";
const UI_LOW_TRANSPARENCY_TOGGLE_ID = "lowTransparencyMode";
const UI_LOW_TRANSPARENCY_CARD_ID = "settingsLowTransparencyCard";
const ROUTE_STRATEGY_STORAGE_KEY = "codexmanager.gateway.route_strategy";
const ROUTE_STRATEGY_ORDERED = "ordered";
const ROUTE_STRATEGY_BALANCED = "balanced";
const CPA_NO_COOKIE_HEADER_MODE_STORAGE_KEY = "codexmanager.gateway.cpa_no_cookie_header_mode";
const BACKGROUND_TASKS_SETTINGS_STORAGE_KEY = "codexmanager.gateway.background_tasks";
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
let refreshAllInFlight = null;
let refreshAllProgressClearTimer = null;
let updateCheckInFlight = null;
let pendingUpdateCandidate = null;
let routeStrategySyncInFlight = null;
let routeStrategySyncedProbeId = -1;
let cpaNoCookieHeaderModeSyncInFlight = null;
let cpaNoCookieHeaderModeSyncedProbeId = -1;
let backgroundTasksSyncInFlight = null;
let backgroundTasksSyncedProbeId = -1;
let apiModelsRemoteRefreshInFlight = null;
function buildRefreshAllTasks(options = {}) {
  const refreshRemoteUsage = options.refreshRemoteUsage === true;
  const refreshRemoteModels = options.refreshRemoteModels === true;
  return [
    { name: "accounts", label: "账号列表", run: refreshAccounts },
    { name: "usage", label: "账号用量", run: () => refreshUsageList({ refreshRemote: refreshRemoteUsage }) },
    { name: "api-models", label: "模型列表", run: () => refreshApiModels({ refreshRemote: refreshRemoteModels }) },
    { name: "api-keys", label: "平台 Key", run: refreshApiKeys },
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
  const updateCard = dom.checkUpdate ? dom.checkUpdate.closest(".settings-card") : null;
  if (updateCard) {
    updateCard.style.display = "none";
  }

  return true;
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

function readLowTransparencySetting() {
  if (typeof localStorage === "undefined") {
    return false;
  }
  const raw = localStorage.getItem(UI_LOW_TRANSPARENCY_STORAGE_KEY);
  if (raw == null) {
    return false;
  }
  const normalized = String(raw).trim().toLowerCase();
  return ["1", "true", "yes", "on"].includes(normalized);
}

function saveLowTransparencySetting(enabled) {
  if (typeof localStorage === "undefined") {
    return;
  }
  localStorage.setItem(UI_LOW_TRANSPARENCY_STORAGE_KEY, enabled ? "1" : "0");
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
  if (typeof localStorage !== "undefined" && localStorage.getItem(UI_LOW_TRANSPARENCY_STORAGE_KEY) == null) {
    saveLowTransparencySetting(enabled);
  }
  applyLowTransparencySetting(enabled);
  const toggle = ensureLowTransparencySettingCard();
  if (toggle) {
    toggle.checked = enabled;
  }
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
    hintText = "按 Key + 模型 均衡轮询起点，优先使用可用账号（不可用账号不会参与选路）。";
  }
  dom.routeStrategyHint.title = hintText;
  dom.routeStrategyHint.setAttribute("aria-label", `网关选路策略说明：${hintText}`);
}

function readRouteStrategySetting() {
  if (typeof localStorage === "undefined") {
    return ROUTE_STRATEGY_ORDERED;
  }
  return normalizeRouteStrategy(localStorage.getItem(ROUTE_STRATEGY_STORAGE_KEY));
}

function saveRouteStrategySetting(strategy) {
  if (typeof localStorage === "undefined") {
    return;
  }
  localStorage.setItem(ROUTE_STRATEGY_STORAGE_KEY, normalizeRouteStrategy(strategy));
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
  if (typeof localStorage !== "undefined" && localStorage.getItem(ROUTE_STRATEGY_STORAGE_KEY) == null) {
    saveRouteStrategySetting(mode);
  }
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
  const connected = await ensureConnected();
  serviceLifecycle.updateServiceToggle();
  if (!connected) {
    return;
  }

  const hasLocalSetting = typeof localStorage !== "undefined"
    && localStorage.getItem(ROUTE_STRATEGY_STORAGE_KEY) != null;
  if (hasLocalSetting) {
    await applyRouteStrategyToService(readRouteStrategySetting(), { silent: true });
    return;
  }

  try {
    const response = await serviceGatewayRouteStrategyGet();
    const strategy = resolveRouteStrategyFromPayload(response);
    saveRouteStrategySetting(strategy);
    setRouteStrategySelect(strategy);
    routeStrategySyncedProbeId = state.serviceProbeId;
  } catch {
    setRouteStrategySelect(readRouteStrategySetting());
  }
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
  if (typeof localStorage === "undefined") {
    return false;
  }
  return normalizeCpaNoCookieHeaderMode(localStorage.getItem(CPA_NO_COOKIE_HEADER_MODE_STORAGE_KEY));
}

function saveCpaNoCookieHeaderModeSetting(enabled) {
  if (typeof localStorage === "undefined") {
    return;
  }
  localStorage.setItem(
    CPA_NO_COOKIE_HEADER_MODE_STORAGE_KEY,
    normalizeCpaNoCookieHeaderMode(enabled) ? "1" : "0",
  );
}

function setCpaNoCookieHeaderModeToggle(enabled) {
  if (dom.cpaNoCookieHeaderMode) {
    dom.cpaNoCookieHeaderMode.checked = Boolean(enabled);
  }
}

function initCpaNoCookieHeaderModeSetting() {
  const enabled = readCpaNoCookieHeaderModeSetting();
  if (typeof localStorage !== "undefined" && localStorage.getItem(CPA_NO_COOKIE_HEADER_MODE_STORAGE_KEY) == null) {
    saveCpaNoCookieHeaderModeSetting(enabled);
  }
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
  const connected = await ensureConnected();
  serviceLifecycle.updateServiceToggle();
  if (!connected) {
    return;
  }

  const hasLocalSetting = typeof localStorage !== "undefined"
    && localStorage.getItem(CPA_NO_COOKIE_HEADER_MODE_STORAGE_KEY) != null;
  if (hasLocalSetting) {
    await applyCpaNoCookieHeaderModeToService(readCpaNoCookieHeaderModeSetting(), { silent: true });
    return;
  }

  try {
    const response = await serviceGatewayHeaderPolicyGet();
    const enabled = resolveCpaNoCookieHeaderModeFromPayload(response);
    saveCpaNoCookieHeaderModeSetting(enabled);
    setCpaNoCookieHeaderModeToggle(enabled);
    cpaNoCookieHeaderModeSyncedProbeId = state.serviceProbeId;
  } catch {
    setCpaNoCookieHeaderModeToggle(readCpaNoCookieHeaderModeSetting());
  }
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
  if (typeof localStorage === "undefined") {
    return normalizeBackgroundTasksSettings(DEFAULT_BACKGROUND_TASKS_SETTINGS);
  }
  const raw = localStorage.getItem(BACKGROUND_TASKS_SETTINGS_STORAGE_KEY);
  if (!raw) {
    return normalizeBackgroundTasksSettings(DEFAULT_BACKGROUND_TASKS_SETTINGS);
  }
  try {
    const parsed = JSON.parse(raw);
    return normalizeBackgroundTasksSettings(parsed);
  } catch {
    return normalizeBackgroundTasksSettings(DEFAULT_BACKGROUND_TASKS_SETTINGS);
  }
}

function saveBackgroundTasksSetting(settings) {
  if (typeof localStorage === "undefined") {
    return;
  }
  const normalized = normalizeBackgroundTasksSettings(settings);
  localStorage.setItem(BACKGROUND_TASKS_SETTINGS_STORAGE_KEY, JSON.stringify(normalized));
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
    ["usageRefreshWorkers", dom.backgroundUsageRefreshWorkers, "用量刷新 worker 数"],
    ["httpWorkerFactor", dom.backgroundHttpWorkerFactor, "HTTP worker 因子"],
    ["httpWorkerMin", dom.backgroundHttpWorkerMin, "HTTP worker 最小值"],
    ["httpStreamWorkerFactor", dom.backgroundHttpStreamWorkerFactor, "流式 worker 因子"],
    ["httpStreamWorkerMin", dom.backgroundHttpStreamWorkerMin, "流式 worker 最小值"],
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
  if (typeof localStorage !== "undefined" && localStorage.getItem(BACKGROUND_TASKS_SETTINGS_STORAGE_KEY) == null) {
    saveBackgroundTasksSetting(settings);
  }
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
  const connected = await ensureConnected();
  serviceLifecycle.updateServiceToggle();
  if (!connected) {
    return;
  }
  const hasLocalSetting = typeof localStorage !== "undefined"
    && localStorage.getItem(BACKGROUND_TASKS_SETTINGS_STORAGE_KEY) != null;
  if (hasLocalSetting) {
    await applyBackgroundTasksToService(readBackgroundTasksSetting(), { silent: true });
    return;
  }
  try {
    const response = await serviceGatewayBackgroundTasksGet();
    const settings = resolveBackgroundTasksSettingsFromPayload(response);
    const restartKeys = resolveBackgroundTasksRestartKeys(response);
    saveBackgroundTasksSetting(settings);
    setBackgroundTasksForm(settings);
    updateBackgroundTasksHint(restartKeys);
    backgroundTasksSyncedProbeId = state.serviceProbeId;
  } catch {
    setBackgroundTasksForm(readBackgroundTasksSetting());
    updateBackgroundTasksHint(BACKGROUND_TASKS_RESTART_KEYS_DEFAULT);
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
    if (routeStrategySyncedProbeId !== state.serviceProbeId) {
      await applyRouteStrategyToService(readRouteStrategySetting(), { silent: true });
    }
    if (cpaNoCookieHeaderModeSyncedProbeId !== state.serviceProbeId) {
      await applyCpaNoCookieHeaderModeToService(readCpaNoCookieHeaderModeSetting(), { silent: true });
    }
    if (backgroundTasksSyncedProbeId !== state.serviceProbeId) {
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
          `[refreshAll] partial failure: ${failedLabelText}; first error: ${firstFailedMessage || "unknown"}`,
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
    refreshApiModelsNow,
    populateApiKeyModelSelect,
    importAccountsFromFiles,
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
  if (dom.routeStrategySelect && dom.routeStrategySelect.dataset.bound !== "1") {
    dom.routeStrategySelect.dataset.bound = "1";
    dom.routeStrategySelect.addEventListener("change", () => {
      const selected = normalizeRouteStrategy(dom.routeStrategySelect.value);
      saveRouteStrategySetting(selected);
      setRouteStrategySelect(selected);
      void applyRouteStrategyToService(selected, { silent: false });
    });
  }
  if (dom.cpaNoCookieHeaderMode && dom.cpaNoCookieHeaderMode.dataset.bound !== "1") {
    dom.cpaNoCookieHeaderMode.dataset.bound = "1";
    dom.cpaNoCookieHeaderMode.addEventListener("change", () => {
      const enabled = Boolean(dom.cpaNoCookieHeaderMode.checked);
      saveCpaNoCookieHeaderModeSetting(enabled);
      setCpaNoCookieHeaderModeToggle(enabled);
      void applyCpaNoCookieHeaderModeToService(enabled, { silent: false });
    });
  }
  if (dom.backgroundTasksSave && dom.backgroundTasksSave.dataset.bound !== "1") {
    dom.backgroundTasksSave.dataset.bound = "1";
    dom.backgroundTasksSave.addEventListener("click", () => {
      void withButtonBusy(dom.backgroundTasksSave, "保存中...", async () => {
        const parsed = readBackgroundTasksForm();
        if (!parsed.ok) {
          showToast(parsed.error, "error");
          return;
        }
        const nextSettings = parsed.settings;
        saveBackgroundTasksSetting(nextSettings);
        await applyBackgroundTasksToService(nextSettings, { silent: false });
      });
    });
  }
  const lowTransparencyToggle = typeof document === "undefined"
    ? null
    : document.getElementById(UI_LOW_TRANSPARENCY_TOGGLE_ID);
  if (lowTransparencyToggle && lowTransparencyToggle.dataset.bound !== "1") {
    lowTransparencyToggle.dataset.bound = "1";
    lowTransparencyToggle.addEventListener("change", () => {
      const enabled = Boolean(lowTransparencyToggle.checked);
      saveLowTransparencySetting(enabled);
      applyLowTransparencySetting(enabled);
    });
  }
}

function bootstrap() {
  setStartupMask(true, "正在初始化界面...");
  setStatus("", false);
  const browserMode = applyBrowserModeUi();
  setServiceHint(browserMode ? "浏览器模式：请先启动 codexmanager-service" : "请输入端口并点击启动", false);
  renderThemeButtons();
  restoreTheme();
  initLowTransparencySetting();
  initUpdateAutoCheckSetting();
  initRouteStrategySetting();
  initCpaNoCookieHeaderModeSetting();
  initBackgroundTasksSetting();
  void bootstrapUpdateStatus();
  serviceLifecycle.restoreServiceAddr();
  serviceLifecycle.updateServiceToggle();
  bindEvents();
  renderCurrentPageView();
  updateRequestLogFilterButtons();
  scheduleStartupUpdateCheck();
  void serviceLifecycle.autoStartService().finally(() => {
    void syncRouteStrategyOnStartup();
    void syncCpaNoCookieHeaderModeOnStartup();
    void syncBackgroundTasksOnStartup();
    setStartupMask(false);
  });
}

window.addEventListener("DOMContentLoaded", bootstrap);






