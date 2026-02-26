import { state } from "../../state.js";
import { dom } from "../../ui/dom.js";
import { formatLimitLabel, formatTs } from "../../utils/format.js";
import {
  buildAccountDerivedMap,
  buildGroupFilterOptions,
  filterAccounts,
  normalizeGroupName,
} from "./state.js";

const ACCOUNT_ACTION_OPEN_USAGE = "open-usage";
const ACCOUNT_ACTION_DELETE = "delete";

let accountRowsEventsBound = false;
let accountRowHandlers = null;
let accountLookupById = new Map();
let groupOptionsCacheKey = "";
let groupOptionsCache = [];
let groupSelectRenderedKey = "";

function getGroupOptionsCacheKey(accounts) {
  const list = Array.isArray(accounts) ? accounts : [];
  return list
    .map((account) => `${account.id || ""}:${normalizeGroupName(account.groupName)}`)
    .join("|");
}

function getGroupOptions(accounts) {
  const nextKey = getGroupOptionsCacheKey(accounts);
  if (nextKey !== groupOptionsCacheKey) {
    groupOptionsCacheKey = nextKey;
    groupOptionsCache = buildGroupFilterOptions(accounts);
  } else if (Array.isArray(groupOptionsCache) && groupOptionsCache.length > 0) {
    // 保持“全部分组”计数与当前账号数量一致。
    groupOptionsCache[0] = {
      ...groupOptionsCache[0],
      count: Array.isArray(accounts) ? accounts.length : 0,
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
  const primaryLabel = formatLimitLabel(accountDerived?.usage?.windowMinutes, "5小时");
  mini.appendChild(
    renderMiniUsageLine(primaryLabel, primaryRemain, false),
  );
  const hasSecondaryWindow =
    accountDerived?.usage?.secondaryUsedPercent != null
    || accountDerived?.usage?.secondaryWindowMinutes != null;
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
  row.appendChild(createActionsCell(Boolean(onDelete)));
  dom.accountRows.appendChild(row);
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
  handlers?.onUpdateSort?.(accountId, sortValue);
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
export function renderAccounts({ onUpdateSort, onOpenUsage, onDelete }) {
  ensureAccountRowsEventsBound();
  accountRowHandlers = { onUpdateSort, onOpenUsage, onDelete };
  dom.accountRows.innerHTML = "";
  syncGroupFilterSelect(getGroupOptions(state.accountList), groupOptionsCacheKey);
  const accountDerivedMap = buildAccountDerivedMap(state.accountList, state.usageList);

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
    renderEmptyRow(message);
    return;
  }

  accountLookupById = new Map(filtered.map((account) => [account.id, account]));
  filtered.forEach((account) => {
    renderAccountRow(account, accountDerivedMap, { onDelete });
  });
}
