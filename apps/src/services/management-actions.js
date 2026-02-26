import { createRequestLogActions } from "./management/requestlog-actions";
import { createAccountActions } from "./management/account-actions";
import { createUsageActions } from "./management/usage-actions";
import { createApiKeyActions } from "./management/apikey-actions";

export function createManagementActions({
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
}) {
  const requestlogActions = createRequestLogActions({
    dom,
    state,
    ensureConnected,
    withButtonBusy,
    showToast,
    showConfirmDialog,
    clearRequestLogs,
    refreshRequestLogs,
    renderRequestLogs,
  });

  const accountActions = createAccountActions({
    ensureConnected,
    refreshAccountsAndUsage,
    renderAccountsView,
    showToast,
    showConfirmDialog,
  });

  const usageActions = createUsageActions({
    dom,
    state,
    ensureConnected,
    openUsageModal,
    renderUsageSnapshot,
    renderAccountsView,
  });

  const apiKeyActions = createApiKeyActions({
    dom,
    ensureConnected,
    withButtonBusy,
    showToast,
    showConfirmDialog,
    refreshApiModels,
    refreshApiKeys,
    populateApiKeyModelSelect,
    renderApiKeys,
  });

  return {
    ...requestlogActions,
    ...accountActions,
    ...usageActions,
    ...apiKeyActions,
  };
}
