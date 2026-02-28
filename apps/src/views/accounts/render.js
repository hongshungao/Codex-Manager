import { state } from "../../state.js";
import { dom } from "../../ui/dom.js";
import { formatLimitLabel, formatTs } from "../../utils/format.js";
import { getRefreshAllProgress } from "../../services/management/account-actions.js";
import {
  buildAccountDerivedMap,
  buildGroupFilterOptions,
  filterAccounts,
  normalizeGroupName,
} from "./state.js";

const ACCOUNT_ACTION_OPEN_USAGE = "open-usage";
const ACCOUNT_ACTION_SET_CURRENT = "set-current";
const ACCOUNT_ACTION_DELETE = "delete";

let accountRowsEventsBound = false;
let accountRowHandlers = null;
let accountLookupById = new Map();
let accountRowNodesById = new Map();
let groupOptionsAccountsRef = null;
let groupOptionsCache = [];
let groupSelectRenderedKey = null;
let refreshProgressNode = null;
let derivedCacheAccountsRef = null;
let derivedCacheUsageRef = null;
let derivedCacheMap = new Map();

function ensureRefreshProgressNode() {
  if (refreshProgressNode?.isConnected) {
    return refreshProgressNode;
  }
  if (!dom.accountsToolbar) {
    return null;
  }
  const existing = dom.accountsToolbar.querySelector(".accounts-refresh-progress");
  if (existing) {
    refreshProgressNode = existing;
    return refreshProgressNode;
  }
  const node = document.createElement("div");
  node.className = "accounts-refresh-progress";
  node.hidden = true;
  node.setAttribute("aria-live", "polite");
  dom.accountsToolbar.prepend(node);
  refreshProgressNode = node;
  return refreshProgressNode;
}

export function renderAccountsRefreshProgress(progress = getRefreshAllProgress()) {
  const node = ensureRefreshProgressNode();
  if (!node) return;
  const total = Math.max(0, Number(progress?.total || 0));
  const completed = Math.min(total, Math.max(0, Number(progress?.completed || 0)));
  const remaining = Math.max(0, Number(progress?.remaining ?? total - completed));
  const active = Boolean(progress?.active) && Boolean(progress?.manual) && total > 0;
  if (!active) {
    node.hidden = true;
    node.textContent = "";
    return;
  }
  const primaryText = `刷新进度 ${completed}/${total}，剩余 ${remaining} 项`;
  const lastTaskLabel = String(progress?.lastTaskLabel || "").trim();
  node.hidden = false;
  node.textContent = lastTaskLabel ? `${primaryText} · 最近完成：${lastTaskLabel}` : primaryText;
}

function getGroupOptions(accounts) {
  const list = Array.isArray(accounts) ? accounts : [];
  if (groupOptionsAccountsRef !== accounts) {
    groupOptionsAccountsRef = accounts;
    groupOptionsCache = buildGroupFilterOptions(list);
  } else if (Array.isArray(groupOptionsCache) && groupOptionsCache.length > 0) {
    // 保持“全部分组”计数与当前账号数量一致。
    groupOptionsCache[0] = {
      ...groupOptionsCache[0],
      count: list.length,
    };
  }
  return groupOptionsCache;
}

function syncGroupFilterSelect(options, optionsKey) {
  if (!dom.accountGroupFilter) return;
  const select = dom.accountGroupFilter;
  const safeOptions = Array.isArray(options) ? options : [];
  const nextValues = new Set(safeOptions.map((item) => item.value));

  // 中文注释：分组来自实时账号数据；若分组被删除/重命名，不自动回退会导致列表“看似空白”且用户难定位原因。
  if (!nextValues.has(state.accountGroupFilter)) {
    state.accountGroupFilter = "all";
  }

  if (groupSelectRenderedKey === optionsKey && select.children.length === safeOptions.length) {
    if (select.value !== state.accountGroupFilter) {
      select.value = state.accountGroupFilter;
    }
    return;
  }

  select.innerHTML = "";
  for (const option of safeOptions) {
    const node = document.createElement("option");
    node.value = option.value;
    node.textContent = `${option.label} (${option.count})`;
    if (option.value === state.accountGroupFilter) {
      node.selected = true;
    }
    select.appendChild(node);
  }
  groupSelectRenderedKey = optionsKey;
  if (!nextValues.has(state.accountGroupFilter)) {
    select.value = "all";
  }
}

function getAccountDerivedMapCached(accounts, usageSource) {
  if (derivedCacheAccountsRef === accounts && derivedCacheUsageRef === usageSource) {
    return derivedCacheMap;
  }
  derivedCacheAccountsRef = accounts;
  derivedCacheUsageRef = usageSource;
  derivedCacheMap = buildAccountDerivedMap(accounts, usageSource);
  return derivedCacheMap;
}

function renderMiniUsageLine(label, remain, secondary) {
  const line = document.createElement("div");
  line.className = "progress-line";
  if (secondary) line.classList.add("secondary");
  const text = document.createElement("span");
  text.textContent = `${label} ${remain == null ? "--" : `${remain}%`}`;
  const track = document.createElement("div");
  track.className = "track";
  const fill = document.createElement("div");
  fill.className = "fill";
  fill.style.width = remain == null ? "0%" : `${remain}%`;
  track.appendChild(fill);
  line.appendChild(text);
  line.appendChild(track);
  return line;
}

function createStatusTag(status) {
  const statusTag = document.createElement("span");
  statusTag.className = "status-tag";
  statusTag.textContent = status.text;
  if (status.level === "ok") statusTag.classList.add("status-ok");
  if (status.level === "warn") statusTag.classList.add("status-warn");
  if (status.level === "bad") statusTag.classList.add("status-bad");
  if (status.level === "unknown") statusTag.classList.add("status-unknown");
  return statusTag;
}

function createAccountCell(account, accountDerived) {
  const cellAccount = document.createElement("td");
  const accountWrap = document.createElement("div");
  accountWrap.className = "cell-stack";
  const primaryRemain = accountDerived?.primaryRemain ?? null;
  const secondaryRemain = accountDerived?.secondaryRemain ?? null;
  const accountTitle = document.createElement("strong");
  accountTitle.textContent = account.label || "-";
  const accountMeta = document.createElement("small");
  accountMeta.textContent = `${account.id || "-"}`;
  accountWrap.appendChild(accountTitle);
  accountWrap.appendChild(accountMeta);
  const mini = document.createElement("div");
  mini.className = "mini-usage";
  const usage = accountDerived?.usage || null;
  const hasPrimaryWindow = usage?.usedPercent != null && usage?.windowMinutes != null;
  const hasSecondaryWindow =
    usage?.secondaryUsedPercent != null
    || usage?.secondaryWindowMinutes != null;

  if (hasPrimaryWindow) {
    const primaryLabel = formatLimitLabel(usage?.windowMinutes, "5小时");
    mini.appendChild(
      renderMiniUsageLine(primaryLabel, primaryRemain, false),
    );
  }

  if (hasSecondaryWindow) {
    mini.appendChild(
      renderMiniUsageLine("7天", secondaryRemain, true),
    );
  }
  accountWrap.appendChild(mini);
  cellAccount.appendChild(accountWrap);
  return cellAccount;
}

function createGroupCell(account) {
  const cellGroup = document.createElement("td");
  cellGroup.textContent = normalizeGroupName(account.groupName) || "-";
  return cellGroup;
}

function createSortCell(account) {
  const cellSort = document.createElement("td");
  const sortInput = document.createElement("input");
  sortInput.className = "sort-input";
  sortInput.type = "number";
  sortInput.setAttribute("data-field", "sort");
  sortInput.value = account.sort != null ? String(account.sort) : "0";
  sortInput.dataset.originSort = sortInput.value;
  cellSort.appendChild(sortInput);
  return cellSort;
}

function createUpdatedCell(usage) {
  const cellUpdated = document.createElement("td");
  const updatedText = document.createElement("strong");
  updatedText.textContent = usage && usage.capturedAt ? formatTs(usage.capturedAt) : "未知";
  cellUpdated.appendChild(updatedText);
  return cellUpdated;
}

function createActionsCell(isDeletable) {
  const cellActions = document.createElement("td");
  const actionsWrap = document.createElement("div");
  actionsWrap.className = "cell-actions";
  const btn = document.createElement("button");
  btn.className = "secondary";
  btn.type = "button";
  btn.setAttribute("data-action", ACCOUNT_ACTION_OPEN_USAGE);
  btn.textContent = "用量查询";
  actionsWrap.appendChild(btn);
  const setCurrent = document.createElement("button");
  setCurrent.className = "secondary";
  setCurrent.type = "button";
  setCurrent.setAttribute("data-action", ACCOUNT_ACTION_SET_CURRENT);
  setCurrent.textContent = "切到当前";
  actionsWrap.appendChild(setCurrent);

  if (isDeletable) {
    const del = document.createElement("button");
    del.className = "danger";
    del.type = "button";
    del.setAttribute("data-action", ACCOUNT_ACTION_DELETE);
    del.textContent = "删除";
    actionsWrap.appendChild(del);
  }
  cellActions.appendChild(actionsWrap);
  return cellActions;
}

function syncSetCurrentButton(actionsWrap, status) {
  if (!actionsWrap) return;
  const btn = actionsWrap.querySelector(`button[data-action="${ACCOUNT_ACTION_SET_CURRENT}"]`);
  if (!btn) return;
  const level = status?.level;
  const disabled = level === "warn" || level === "bad";
  btn.disabled = disabled;
  btn.title = disabled ? `账号当前不可用（${status?.text || "不可用"}），不参与网关选路` : "锁定为当前账号（异常前持续优先使用）";
}

function renderEmptyRow(message) {
  const emptyRow = document.createElement("tr");
  const emptyCell = document.createElement("td");
  emptyCell.colSpan = 6;
  emptyCell.textContent = message;
  emptyRow.appendChild(emptyCell);
  dom.accountRows.appendChild(emptyRow);
}

function renderAccountRow(account, accountDerivedMap, { onDelete }) {
  const row = document.createElement("tr");
  row.setAttribute("data-account-id", account.id || "");
  const accountDerived = accountDerivedMap.get(account.id) || {
    usage: null,
    primaryRemain: null,
    secondaryRemain: null,
    status: { text: "未知", level: "unknown" },
  };

  row.appendChild(createAccountCell(account, accountDerived));
  row.appendChild(createGroupCell(account));
  row.appendChild(createSortCell(account));

  const cellStatus = document.createElement("td");
  cellStatus.appendChild(createStatusTag(accountDerived.status));
  row.appendChild(cellStatus);

  row.appendChild(createUpdatedCell(accountDerived.usage));
  const actionsCell = createActionsCell(Boolean(onDelete));
  row.appendChild(actionsCell);
  syncSetCurrentButton(actionsCell.querySelector(".cell-actions"), accountDerived.status);
  return row;
}

function removeAllAccountRows() {
  if (!dom.accountRows) return;
  while (dom.accountRows.firstElementChild) {
    dom.accountRows.firstElementChild.remove();
  }
  accountRowNodesById = new Map();
}

function updateStatusTag(node, status) {
  if (!node) return;
  const next = status || { text: "未知", level: "unknown" };
  node.textContent = next.text;
  node.className = "status-tag";
  if (next.level === "ok") node.classList.add("status-ok");
  if (next.level === "warn") node.classList.add("status-warn");
  if (next.level === "bad") node.classList.add("status-bad");
  if (next.level === "unknown") node.classList.add("status-unknown");
}

function updateMiniUsage(mini, usage, primaryRemain, secondaryRemain) {
  if (!mini) return;
  const safeUsage = usage || null;
  const hasPrimaryWindow = safeUsage?.usedPercent != null && safeUsage?.windowMinutes != null;
  const hasSecondaryWindow =
    safeUsage?.secondaryUsedPercent != null
    || safeUsage?.secondaryWindowMinutes != null;

  mini.textContent = "";
  if (hasPrimaryWindow) {
    const primaryLabel = formatLimitLabel(safeUsage?.windowMinutes, "5小时");
    mini.appendChild(renderMiniUsageLine(primaryLabel, primaryRemain ?? null, false));
  }
  if (hasSecondaryWindow) {
    mini.appendChild(renderMiniUsageLine("7天", secondaryRemain ?? null, true));
  }
}

function ensureDeleteButton(actionsWrap) {
  if (!actionsWrap) return null;
  const existing = actionsWrap.querySelector(`button[data-action="${ACCOUNT_ACTION_DELETE}"]`);
  if (existing) return existing;
  const del = document.createElement("button");
  del.className = "danger";
  del.type = "button";
  del.setAttribute("data-action", ACCOUNT_ACTION_DELETE);
  del.textContent = "删除";
  actionsWrap.appendChild(del);
  return del;
}

function syncDeleteButton(actionsWrap, enabled) {
  if (!actionsWrap) return;
  const existing = actionsWrap.querySelector(`button[data-action="${ACCOUNT_ACTION_DELETE}"]`);
  if (enabled) {
    ensureDeleteButton(actionsWrap);
    return;
  }
  existing?.remove();
}

function updateAccountRow(row, account, accountDerivedMap, { onDelete }) {
  if (!row || !account || !account.id) {
    return row;
  }
  row.setAttribute("data-account-id", account.id);
  const accountDerived = accountDerivedMap.get(account.id) || {
    usage: null,
    primaryRemain: null,
    secondaryRemain: null,
    status: { text: "未知", level: "unknown" },
  };

  // Column 0: account cell
  const cellAccount = row.children[0];
  const title = cellAccount?.querySelector?.("strong");
  const meta = cellAccount?.querySelector?.("small");
  if (title) title.textContent = account.label || "-";
  if (meta) meta.textContent = `${account.id || "-"}`;
  const mini = cellAccount?.querySelector?.(".mini-usage");
  updateMiniUsage(mini, accountDerived.usage, accountDerived.primaryRemain, accountDerived.secondaryRemain);

  // Column 1: group
  const cellGroup = row.children[1];
  if (cellGroup) cellGroup.textContent = normalizeGroupName(account.groupName) || "-";

  // Column 2: sort input (avoid clobbering user input while focused)
  const sortInput = row.querySelector?.("input[data-field='sort']");
  if (sortInput) {
    const next = account.sort != null ? String(account.sort) : "0";
    if (document.activeElement !== sortInput) {
      sortInput.value = next;
      sortInput.dataset.originSort = next;
    }
  }

  // Column 3: status
  const statusTag = row.querySelector?.(".status-tag");
  updateStatusTag(statusTag, accountDerived.status);

  // Column 4: updated
  const updatedStrong = row.children[4]?.querySelector?.("strong");
  if (updatedStrong) {
    updatedStrong.textContent = accountDerived.usage && accountDerived.usage.capturedAt
      ? formatTs(accountDerived.usage.capturedAt)
      : "未知";
  }

  // Column 5: actions
  const actionsWrap = row.children[5]?.querySelector?.(".cell-actions");
  syncDeleteButton(actionsWrap, Boolean(onDelete));
  syncSetCurrentButton(actionsWrap, accountDerived.status);
  return row;
}

function syncAccountRows(filtered, accountDerivedMap, { onDelete }) {
  if (!dom.accountRows) return;
  const nextIds = new Set(filtered.map((account) => account.id));

  // Remove stale cache entries (and DOM nodes if still present)
  for (const [accountId, cachedRow] of accountRowNodesById.entries()) {
    if (!nextIds.has(accountId)) {
      cachedRow?.remove?.();
      accountRowNodesById.delete(accountId);
    }
  }

  let cursor = dom.accountRows.firstElementChild;
  for (const account of filtered) {
    if (!account || !account.id) continue;
    const accountId = account.id;
    let row = accountRowNodesById.get(accountId);
    if (!row || !row.isConnected) {
      row = renderAccountRow(account, accountDerivedMap, { onDelete });
      accountRowNodesById.set(accountId, row);
    } else {
      updateAccountRow(row, account, accountDerivedMap, { onDelete });
    }

    if (row === cursor) {
      cursor = cursor?.nextElementSibling || null;
      continue;
    }
    dom.accountRows.insertBefore(row, cursor);
  }

  // Remove any leftover nodes (including previous empty row) after the cursor.
  while (cursor) {
    const next = cursor.nextElementSibling;
    const accountId = cursor?.dataset?.accountId || "";
    if (!accountId || !nextIds.has(accountId)) {
      cursor.remove();
    }
    cursor = next;
  }
}

function getAccountFromRow(row, lookup) {
  const accountId = row?.dataset?.accountId;
  if (!accountId) return null;
  return lookup.get(accountId) || null;
}

export function handleAccountRowsClick(target, handlers = accountRowHandlers, lookup = accountLookupById) {
  const actionButton = target?.closest?.("button[data-action]");
  if (!actionButton) return false;
  const row = actionButton.closest("tr[data-account-id]");
  if (!row) return false;
  const account = getAccountFromRow(row, lookup);
  if (!account) return false;
  const action = actionButton.dataset.action;
  if (action === ACCOUNT_ACTION_OPEN_USAGE) {
    handlers?.onOpenUsage?.(account);
    return true;
  }
  if (action === ACCOUNT_ACTION_SET_CURRENT) {
    handlers?.onSetCurrentAccount?.(account);
    return true;
  }
  if (action === ACCOUNT_ACTION_DELETE) {
    handlers?.onDelete?.(account);
    return true;
  }
  return false;
}

export function handleAccountRowsChange(target, handlers = accountRowHandlers) {
  const sortInput = target?.closest?.("input[data-field='sort']");
  if (!sortInput) return false;
  const row = sortInput.closest("tr[data-account-id]");
  if (!row) return false;
  const accountId = row.dataset.accountId;
  if (!accountId) return false;
  const sortValue = Number(sortInput.value || 0);
  const originSort = Number(sortInput.dataset.originSort);
  if (Number.isFinite(originSort) && originSort === sortValue) {
    return false;
  }
  sortInput.dataset.originSort = String(sortValue);
  handlers?.onUpdateSort?.(accountId, sortValue, originSort);
  return true;
}

function ensureAccountRowsEventsBound() {
  if (accountRowsEventsBound || !dom.accountRows) {
    return;
  }
  accountRowsEventsBound = true;
  dom.accountRows.addEventListener("click", (event) => {
    handleAccountRowsClick(event.target);
  });
  dom.accountRows.addEventListener("change", (event) => {
    handleAccountRowsChange(event.target);
  });
}

// 渲染账号列表
export function renderAccounts({ onUpdateSort, onOpenUsage, onSetCurrentAccount, onDelete }) {
  ensureAccountRowsEventsBound();
  renderAccountsRefreshProgress();
  accountRowHandlers = { onUpdateSort, onOpenUsage, onSetCurrentAccount, onDelete };
  syncGroupFilterSelect(getGroupOptions(state.accountList), state.accountList);
  const accountDerivedMap = getAccountDerivedMapCached(state.accountList, state.usageList);

  const filtered = filterAccounts(
    state.accountList,
    accountDerivedMap,
    state.accountSearch,
    state.accountFilter,
    state.accountGroupFilter,
  );

  if (filtered.length === 0) {
    accountLookupById = new Map();
    const message = state.accountList.length === 0 ? "暂无账号" : "当前筛选条件下无结果";
    removeAllAccountRows();
    renderEmptyRow(message);
    return;
  }

  accountLookupById = new Map(filtered.map((account) => [account.id, account]));
  syncAccountRows(filtered, accountDerivedMap, { onDelete });
}
