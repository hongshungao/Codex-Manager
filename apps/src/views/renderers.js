import { renderDashboard } from "./dashboard";
import { renderAccounts } from "./accounts";
import { renderApiKeys } from "./apikeys";
import { renderRequestLogs } from "./requestlogs";

function renderApiKeysOnly(handlers) {
  renderApiKeys({
    onToggleStatus: handlers.onToggleApiKeyStatus,
    onDelete: handlers.onDeleteApiKey,
    onUpdateModel: handlers.onUpdateApiKeyModel,
    onCopy: handlers.onCopyApiKey,
  });
}

export function renderAccountsOnly(handlers) {
  renderAccounts({
    onUpdateSort: handlers.onUpdateSort,
    onOpenUsage: handlers.onOpenUsage,
    onDelete: handlers.onDeleteAccount,
  });
}

export function renderCurrentView(page, handlers) {
  if (page === "accounts") {
    renderAccountsOnly(handlers);
    return;
  }
  if (page === "apikeys") {
    renderApiKeysOnly(handlers);
    return;
  }
  if (page === "requestlogs") {
    renderRequestLogs();
    return;
  }
  renderDashboard();
}

export function renderAllViews(handlers) {
  renderDashboard();
  renderAccountsOnly(handlers);
  renderApiKeysOnly(handlers);
  renderRequestLogs();
}
