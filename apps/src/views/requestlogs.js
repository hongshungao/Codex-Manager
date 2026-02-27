import { dom } from "../ui/dom.js";
import { state } from "../state.js";
import { copyText } from "../utils/clipboard.js";
import { formatTs } from "../utils/format.js";

const REQUEST_LOG_BATCH_SIZE = 80;
const REQUEST_LOG_DOM_LIMIT = 240;
const REQUEST_LOG_DOM_RECYCLE_TO = 180;
const REQUEST_LOG_SCROLL_BUFFER = 180;
const REQUEST_LOG_FALLBACK_ROW_HEIGHT = 54;
const REQUEST_LOG_COLUMN_COUNT = 9;
const REQUEST_LOG_NEAR_BOTTOM_MAX_BATCHES = 1;

const requestLogWindowState = {
  filter: "all",
  filtered: [],
  filteredKeys: [],
  nextIndex: 0,
  topSpacerHeight: 0,
  recycledRowHeight: REQUEST_LOG_FALLBACK_ROW_HEIGHT,
  accountListRef: null,
  accountLabelById: new Map(),
  topSpacerRow: null,
  topSpacerCell: null,
  boundRowsEl: null,
  boundScrollerEl: null,
  scrollTickHandle: null,
  scrollTickMode: "",
  hasRendered: false,
};

function fallbackAccountNameFromId(accountId) {
  const raw = String(accountId || "").trim();
  if (!raw) return "";
  const sep = raw.indexOf("::");
  if (sep < 0) return "";
  return raw.slice(sep + 2).trim();
}

function fallbackAccountDisplayFromKey(keyId) {
  const raw = String(keyId || "").trim();
  if (!raw) return "";
  const compact = raw.slice(0, 10);
  return `Key ${compact}`;
}

function ensureAccountLabelMap() {
  const list = Array.isArray(state.accountList) ? state.accountList : [];
  if (requestLogWindowState.accountListRef === list) {
    return requestLogWindowState.accountLabelById;
  }
  const map = new Map();
  for (let i = 0; i < list.length; i += 1) {
    const account = list[i];
    const id = account?.id;
    const label = account?.label;
    if (id && label) {
      map.set(id, label);
    }
  }
  requestLogWindowState.accountListRef = list;
  requestLogWindowState.accountLabelById = map;
  return map;
}

function resolveAccountDisplayName(item) {
  const accountId = item?.accountId || item?.account?.id || "";
  const directLabel = item?.accountLabel || item?.account?.label || "";
  if (directLabel) return directLabel;
  if (accountId) {
    const label = requestLogWindowState.accountLabelById.get(accountId);
    if (label) {
      return label;
    }
  }
  return fallbackAccountNameFromId(accountId);
}

function matchesStatusFilter(item, filter) {
  if (filter === "all") return true;
  const code = Number(item.statusCode);
  if (!Number.isFinite(code)) return false;
  if (filter === "2xx") return code >= 200 && code < 300;
  if (filter === "4xx") return code >= 400 && code < 500;
  if (filter === "5xx") return code >= 500 && code < 600;
  return true;
}

function buildRequestLogIdentity(item, fallbackIndex) {
  const precomputed = item && typeof item === "object" ? item.__identity : null;
  if (precomputed != null && String(precomputed).trim()) {
    return String(precomputed);
  }
  if (item && typeof item === "object" && item.id != null && String(item.id).trim()) {
    return String(item.id);
  }
  // 中文注释：identity 用于“增量追加”判断，避免把 error/path 等长字段拼进 key 导致大量分配与 GC。
  return [
    item?.createdAt ?? "",
    item?.method ?? "",
    item?.statusCode ?? "",
    item?.accountId ?? "",
    item?.keyId ?? "",
    fallbackIndex,
  ].join("|");
}

function collectFilteredRequestLogs() {
  const filter = state.requestLogStatusFilter || "all";
  const list = Array.isArray(state.requestLogList) ? state.requestLogList : [];
  const filtered = [];
  const filteredKeys = [];
  for (let i = 0; i < list.length; i += 1) {
    const item = list[i];
    if (!matchesStatusFilter(item, filter)) {
      continue;
    }
    filtered.push(item);
    filteredKeys.push(buildRequestLogIdentity(item, i));
  }
  return { filter, filtered, filteredKeys };
}

function isAppendOnlyResult(prevKeys, nextKeys) {
  if (!Array.isArray(prevKeys) || !Array.isArray(nextKeys)) return false;
  if (prevKeys.length > nextKeys.length) return false;
  for (let i = 0; i < prevKeys.length; i += 1) {
    if (prevKeys[i] !== nextKeys[i]) {
      return false;
    }
  }
  return true;
}

function getRowHeight(row) {
  if (!row) return REQUEST_LOG_FALLBACK_ROW_HEIGHT;
  if (typeof row.getBoundingClientRect === "function") {
    const rectHeight = Number(row.getBoundingClientRect().height);
    if (Number.isFinite(rectHeight) && rectHeight > 0) {
      return rectHeight;
    }
  }
  const offsetHeight = Number(row.offsetHeight);
  if (Number.isFinite(offsetHeight) && offsetHeight > 0) {
    return offsetHeight;
  }
  return REQUEST_LOG_FALLBACK_ROW_HEIGHT;
}

function updateTopSpacer() {
  const spacerRow = requestLogWindowState.topSpacerRow;
  const spacerCell = requestLogWindowState.topSpacerCell;
  if (!spacerRow || !spacerCell) return;
  const height = Math.max(0, Math.round(requestLogWindowState.topSpacerHeight));
  spacerRow.hidden = height <= 0;
  spacerCell.style.height = `${height}px`;
}

function createTopSpacerRow() {
  const row = document.createElement("tr");
  row.dataset.spacerTop = "1";
  const cell = document.createElement("td");
  cell.colSpan = REQUEST_LOG_COLUMN_COUNT;
  cell.style.height = "0px";
  cell.style.padding = "0";
  cell.style.border = "0";
  cell.style.background = "transparent";
  row.appendChild(cell);
  requestLogWindowState.topSpacerRow = row;
  requestLogWindowState.topSpacerCell = cell;
  return row;
}

function createRequestLogRow(item, index) {
  const row = document.createElement("tr");
  row.dataset.logRow = "1";
  row.dataset.logIndex = String(index);
  row.className = "requestlog-row";
  const cellTime = document.createElement("td");
  cellTime.className = "requestlog-col requestlog-col-time";
  cellTime.textContent = formatTs(item.createdAt, { emptyLabel: "-" });
  row.appendChild(cellTime);

  const cellAccount = document.createElement("td");
  cellAccount.className = "requestlog-col requestlog-col-account";
  const accountLabel = resolveAccountDisplayName(item);
  const accountId = item?.accountId || item?.account?.id || "";
  const keyId = item?.keyId || "";
  const accountWrap = document.createElement("div");
  accountWrap.className = "cell-stack";
  if (accountLabel) {
    const title = document.createElement("strong");
    title.textContent = accountLabel;
    accountWrap.appendChild(title);
    if (accountId) {
      const meta = document.createElement("small");
      meta.textContent = accountId;
      accountWrap.appendChild(meta);
    }
    cellAccount.title = accountId || accountLabel;
  } else if (accountId) {
    const meta = document.createElement("small");
    meta.textContent = accountId;
    accountWrap.appendChild(meta);
    cellAccount.title = accountId;
  } else {
    const keyFallback = fallbackAccountDisplayFromKey(keyId);
    accountWrap.textContent = keyFallback || "-";
    cellAccount.title = keyFallback || "-";
  }
  cellAccount.appendChild(accountWrap);
  row.appendChild(cellAccount);

  const cellKey = document.createElement("td");
  cellKey.className = "requestlog-col requestlog-col-key";
  cellKey.textContent = item.keyId || "-";
  row.appendChild(cellKey);

  const cellMethod = document.createElement("td");
  cellMethod.className = "requestlog-col requestlog-col-method";
  cellMethod.textContent = item.method || "-";
  row.appendChild(cellMethod);

  const cellPath = document.createElement("td");
  cellPath.className = "requestlog-col requestlog-col-path";
  const pathWrap = document.createElement("div");
  pathWrap.className = "request-path-wrap";
  const pathText = document.createElement("span");
  pathText.className = "request-path";
  pathText.textContent = item.requestPath || "-";
  pathText.title = item.requestPath || "-";
  const copyBtn = document.createElement("button");
  copyBtn.className = "ghost path-copy";
  copyBtn.type = "button";
  copyBtn.textContent = "复制";
  copyBtn.title = "复制请求路径";
  copyBtn.dataset.logIndex = String(index);
  pathWrap.appendChild(pathText);
  pathWrap.appendChild(copyBtn);
  cellPath.appendChild(pathWrap);
  row.appendChild(cellPath);

  const cellModel = document.createElement("td");
  cellModel.className = "requestlog-col requestlog-col-model";
  cellModel.textContent = item.model || "-";
  row.appendChild(cellModel);

  const cellEffort = document.createElement("td");
  cellEffort.className = "requestlog-col requestlog-col-effort";
  cellEffort.textContent = item.reasoningEffort || "-";
  row.appendChild(cellEffort);

  const cellStatus = document.createElement("td");
  cellStatus.className = "requestlog-col requestlog-col-status";
  const statusTag = document.createElement("span");
  statusTag.className = "status-tag";
  const code = item.statusCode == null ? null : Number(item.statusCode);
  statusTag.textContent = code == null ? "-" : String(code);
  if (code != null) {
    if (code >= 200 && code < 300) {
      statusTag.classList.add("status-ok");
    } else if (code >= 400 && code < 500) {
      statusTag.classList.add("status-warn");
    } else if (code >= 500) {
      statusTag.classList.add("status-bad");
    } else {
      statusTag.classList.add("status-unknown");
    }
  } else {
    statusTag.classList.add("status-unknown");
  }
  cellStatus.appendChild(statusTag);
  row.appendChild(cellStatus);

  const cellError = document.createElement("td");
  cellError.className = "requestlog-col requestlog-col-error";
  const errorText = item.error ? String(item.error) : "-";
  const errorSpan = document.createElement("span");
  errorSpan.className = "request-error";
  errorSpan.textContent = errorText;
  if (item.error) {
    errorSpan.title = String(item.error);
  }
  cellError.appendChild(errorSpan);
  row.appendChild(cellError);
  return row;
}

function renderEmptyRequestLogs() {
  const row = document.createElement("tr");
  const cell = document.createElement("td");
  cell.colSpan = REQUEST_LOG_COLUMN_COUNT;
  cell.textContent = "暂无请求日志";
  row.appendChild(cell);
  dom.requestLogRows.appendChild(row);
}

function appendRequestLogBatch() {
  if (!dom.requestLogRows) return false;
  const start = requestLogWindowState.nextIndex;
  if (start >= requestLogWindowState.filtered.length) return false;
  const end = Math.min(
    start + REQUEST_LOG_BATCH_SIZE,
    requestLogWindowState.filtered.length,
  );
  const fragment = document.createDocumentFragment();
  for (let i = start; i < end; i += 1) {
    fragment.appendChild(createRequestLogRow(requestLogWindowState.filtered[i], i));
  }
  dom.requestLogRows.appendChild(fragment);
  requestLogWindowState.nextIndex = end;
  recycleLogRowsIfNeeded();
  return true;
}

function appendNearBottomBatches(scroller, maxBatches = REQUEST_LOG_NEAR_BOTTOM_MAX_BATCHES) {
  let appended = false;
  let rounds = 0;
  while (
    rounds < maxBatches &&
    isNearBottom(scroller) &&
    appendRequestLogBatch()
  ) {
    appended = true;
    rounds += 1;
  }
  return appended;
}

function appendAtLeastOneBatch(scroller, extraMaxBatches = REQUEST_LOG_NEAR_BOTTOM_MAX_BATCHES - 1) {
  const appended = appendRequestLogBatch();
  if (!appended) return false;
  if (extraMaxBatches > 0) {
    appendNearBottomBatches(scroller, extraMaxBatches);
  }
  return true;
}

function recycleLogRowsIfNeeded() {
  if (!dom.requestLogRows) return;
  const rows = [];
  for (const child of dom.requestLogRows.children) {
    if (child?.dataset?.logRow === "1") {
      rows.push(child);
    }
  }
  if (rows.length <= REQUEST_LOG_DOM_LIMIT) {
    return;
  }
  const removeCount = rows.length - REQUEST_LOG_DOM_RECYCLE_TO;
  // 中文注释：避免对每一行调用 getBoundingClientRect/offsetHeight（强制同步布局，滚动时很容易卡顿）。
  // 这里抽样一行高度来估算回收高度即可；配合 error/path 的摘要展示，行高波动很小。
  const sampleHeight = getRowHeight(rows[0]);
  if (Number.isFinite(sampleHeight) && sampleHeight > 0) {
    requestLogWindowState.recycledRowHeight = sampleHeight;
  }
  const removedHeight = requestLogWindowState.recycledRowHeight * removeCount;
  for (let i = 0; i < removeCount; i += 1) {
    rows[i].remove();
  }
  requestLogWindowState.topSpacerHeight += removedHeight;
  updateTopSpacer();
}

function isNearBottom(scroller) {
  if (!scroller) return false;
  const scrollTop = Number(scroller.scrollTop);
  const clientHeight = Number(scroller.clientHeight);
  const scrollHeight = Number(scroller.scrollHeight);
  if (!Number.isFinite(scrollTop) || !Number.isFinite(clientHeight) || !Number.isFinite(scrollHeight)) {
    return false;
  }
  return scrollTop + clientHeight >= scrollHeight - REQUEST_LOG_SCROLL_BUFFER;
}

function resolveRequestLogScroller(rowsEl) {
  if (!rowsEl || typeof rowsEl.closest !== "function") {
    return null;
  }
  return rowsEl.closest(".requestlog-wrap");
}

async function onRequestLogRowsClick(event) {
  const target = event?.target;
  if (!target || typeof target.closest !== "function") {
    return;
  }
  const copyBtn = target.closest("button.path-copy");
  if (!copyBtn || !dom.requestLogRows || !dom.requestLogRows.contains(copyBtn)) {
    return;
  }
  const index = Number(copyBtn.dataset.logIndex);
  if (!Number.isInteger(index)) {
    return;
  }
  const rowItem = requestLogWindowState.filtered[index];
  if (!rowItem?.requestPath) {
    return;
  }
  const ok = await copyText(rowItem.requestPath);
  copyBtn.textContent = ok ? "已复制" : "失败";
  const token = String(Date.now());
  copyBtn.dataset.copyToken = token;
  setTimeout(() => {
    if (copyBtn.dataset.copyToken !== token) return;
    copyBtn.textContent = "复制";
  }, 900);
}

function onRequestLogScroll() {
  if (requestLogWindowState.scrollTickHandle != null) {
    return;
  }
  const flush = () => {
    requestLogWindowState.scrollTickHandle = null;
    requestLogWindowState.scrollTickMode = "";
    if (!isNearBottom(requestLogWindowState.boundScrollerEl)) {
      return;
    }
    appendNearBottomBatches(requestLogWindowState.boundScrollerEl);
  };
  if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
    requestLogWindowState.scrollTickMode = "raf";
    requestLogWindowState.scrollTickHandle = window.requestAnimationFrame(flush);
    return;
  }
  flush();
}

function cancelPendingScrollTick() {
  if (requestLogWindowState.scrollTickHandle == null) {
    return;
  }
  if (
    requestLogWindowState.scrollTickMode === "raf"
    && typeof window !== "undefined"
    && typeof window.cancelAnimationFrame === "function"
  ) {
    window.cancelAnimationFrame(requestLogWindowState.scrollTickHandle);
  } else {
    clearTimeout(requestLogWindowState.scrollTickHandle);
  }
  requestLogWindowState.scrollTickHandle = null;
  requestLogWindowState.scrollTickMode = "";
}

function ensureRequestLogBindings() {
  const rowsEl = dom.requestLogRows;
  if (!rowsEl || typeof rowsEl.addEventListener !== "function") {
    return;
  }
  if (requestLogWindowState.boundRowsEl && requestLogWindowState.boundRowsEl !== rowsEl) {
    requestLogWindowState.boundRowsEl.removeEventListener("click", onRequestLogRowsClick);
  }
  if (requestLogWindowState.boundRowsEl !== rowsEl) {
    rowsEl.addEventListener("click", onRequestLogRowsClick);
    requestLogWindowState.boundRowsEl = rowsEl;
  }
  const scroller = resolveRequestLogScroller(rowsEl);
  if (
    requestLogWindowState.boundScrollerEl &&
    requestLogWindowState.boundScrollerEl !== scroller
  ) {
    requestLogWindowState.boundScrollerEl.removeEventListener("scroll", onRequestLogScroll);
    cancelPendingScrollTick();
  }
  if (scroller && requestLogWindowState.boundScrollerEl !== scroller) {
    scroller.addEventListener("scroll", onRequestLogScroll, { passive: true });
    requestLogWindowState.boundScrollerEl = scroller;
  } else if (!scroller) {
    cancelPendingScrollTick();
    requestLogWindowState.boundScrollerEl = null;
  }
}

export function renderRequestLogs() {
  if (!dom.requestLogRows) {
    return;
  }
  ensureRequestLogBindings();
  ensureAccountLabelMap();
  const { filter, filtered, filteredKeys } = collectFilteredRequestLogs();
  const sameFilter = filter === requestLogWindowState.filter;
  const appendOnly = sameFilter && isAppendOnlyResult(
    requestLogWindowState.filteredKeys,
    filteredKeys,
  );
  const unchanged = appendOnly && filteredKeys.length === requestLogWindowState.filteredKeys.length;
  const canReuseRenderedDom = filtered.length > 0
    ? Boolean(
      requestLogWindowState.topSpacerRow &&
      dom.requestLogRows.contains(requestLogWindowState.topSpacerRow),
    )
    : dom.requestLogRows.children.length > 0;

  if (requestLogWindowState.hasRendered && canReuseRenderedDom && unchanged) {
    requestLogWindowState.filtered = filtered;
    requestLogWindowState.filteredKeys = filteredKeys;
    return;
  }

  if (
    requestLogWindowState.hasRendered &&
    appendOnly &&
    requestLogWindowState.topSpacerRow &&
    dom.requestLogRows.contains(requestLogWindowState.topSpacerRow)
  ) {
    const previousLength = requestLogWindowState.filtered.length;
    requestLogWindowState.filtered = filtered;
    requestLogWindowState.filteredKeys = filteredKeys;
    requestLogWindowState.filter = filter;
    if (
      requestLogWindowState.nextIndex >= previousLength ||
      isNearBottom(requestLogWindowState.boundScrollerEl)
    ) {
      appendAtLeastOneBatch(requestLogWindowState.boundScrollerEl);
    }
    return;
  }

  dom.requestLogRows.innerHTML = "";
  requestLogWindowState.filtered = filtered;
  requestLogWindowState.filteredKeys = filteredKeys;
  requestLogWindowState.filter = filter;
  requestLogWindowState.nextIndex = 0;
  requestLogWindowState.topSpacerHeight = 0;
  requestLogWindowState.recycledRowHeight = REQUEST_LOG_FALLBACK_ROW_HEIGHT;
  requestLogWindowState.topSpacerRow = null;
  requestLogWindowState.topSpacerCell = null;
  requestLogWindowState.hasRendered = true;
  if (!filtered.length) {
    renderEmptyRequestLogs();
    return;
  }
  dom.requestLogRows.appendChild(createTopSpacerRow());
  appendAtLeastOneBatch(requestLogWindowState.boundScrollerEl, 1);
}
