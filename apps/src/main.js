import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/responsive.css";

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
let refreshAllInFlight = null;

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
} = managementActions;

function buildMainRenderActions() {
  return buildRenderActions({
    updateAccountSort,
    handleOpenUsageModal,
    deleteAccount,
    toggleApiKeyStatus,
    deleteApiKey,
    updateApiKeyModel,
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
    refreshAll,
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
}

function bootstrap() {
  setStartupMask(true, "正在初始化界面...");
  setStatus("", false);
  setServiceHint("请输入端口并点击启动", false);
  renderThemeButtons();
  restoreTheme();
  serviceLifecycle.restoreServiceAddr();
  serviceLifecycle.updateServiceToggle();
  bindEvents();
  renderAllViews(buildMainRenderActions());
  updateRequestLogFilterButtons();
  void serviceLifecycle.autoStartService().finally(() => {
    setStartupMask(false);
  });
}

window.addEventListener("DOMContentLoaded", bootstrap);






