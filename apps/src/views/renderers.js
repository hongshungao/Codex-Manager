import { renderDashboard } from "./dashboard";
import { renderAccounts } from "./accounts";
import { renderApiKeys } from "./apikeys";
import { renderRequestLogs } from "./requestlogs";

let renderTaskScheduled = false;
let dirtyDashboard = false;
let dirtyAccounts = false;
let dirtyApiKeys = false;
let dirtyRequestLogs = false;
let lastHandlers = null;

function scheduleMicrotask(task) {
  if (typeof queueMicrotask === "function") {
    queueMicrotask(task);
    return;
  }
  Promise.resolve().then(task);
}

export function scheduleRender() {
  if (renderTaskScheduled) {
    return;
  }
  renderTaskScheduled = true;
  // Coalesce multiple render triggers into a single frame.
  if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
    window.requestAnimationFrame(flushRender);
    return;
  }
  scheduleMicrotask(flushRender);
}

function renderApiKeysOnlyNow(handlers) {
  renderApiKeys({
    onToggleStatus: handlers.onToggleApiKeyStatus,
    onDelete: handlers.onDeleteApiKey,
    onUpdateModel: handlers.onUpdateApiKeyModel,
    onCopy: handlers.onCopyApiKey,
  });
}

function renderAccountsOnlyNow(handlers) {
  renderAccounts({
    onUpdateSort: handlers.onUpdateSort,
    onOpenUsage: handlers.onOpenUsage,
    onSetCurrentAccount: handlers.onSetCurrentAccount,
    onDelete: handlers.onDeleteAccount,
  });
}

function flushRender() {
  renderTaskScheduled = false;

  const handlers = lastHandlers;
  const nextDashboard = dirtyDashboard;
  const nextAccounts = dirtyAccounts;
  const nextApiKeys = dirtyApiKeys;
  const nextRequestLogs = dirtyRequestLogs;

  dirtyDashboard = false;
  dirtyAccounts = false;
  dirtyApiKeys = false;
  dirtyRequestLogs = false;

  if (nextDashboard) {
    renderDashboard();
  }
  if (nextAccounts && handlers) {
    renderAccountsOnlyNow(handlers);
  }
  if (nextApiKeys && handlers) {
    renderApiKeysOnlyNow(handlers);
  }
  if (nextRequestLogs) {
    renderRequestLogs();
  }

  if (dirtyDashboard || dirtyAccounts || dirtyApiKeys || dirtyRequestLogs) {
    scheduleRender();
  }
}

function markDirty(partial, handlers) {
  if (handlers) {
    lastHandlers = handlers;
  }
  if (partial.dashboard) dirtyDashboard = true;
  if (partial.accounts) dirtyAccounts = true;
  if (partial.apikeys) dirtyApiKeys = true;
  if (partial.requestlogs) dirtyRequestLogs = true;
  scheduleRender();
}

export function renderAccountsOnly(handlers) {
  markDirty({ accounts: true }, handlers);
}

export function renderCurrentView(page, handlers) {
  if (page === "accounts") {
    markDirty({ accounts: true }, handlers);
    return;
  }
  if (page === "apikeys") {
    markDirty({ apikeys: true }, handlers);
    return;
  }
  if (page === "requestlogs") {
    markDirty({ requestlogs: true }, handlers);
    return;
  }
  if (page === "settings") {
    return;
  }
  markDirty({ dashboard: true }, handlers);
}

export function renderAllViews(handlers) {
  markDirty({
    dashboard: true,
    accounts: true,
    apikeys: true,
    requestlogs: true,
  }, handlers);
}
