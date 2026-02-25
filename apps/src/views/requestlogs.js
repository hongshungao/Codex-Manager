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

const requestLogWindowState = {
  filter: "all",
  filtered: [],
  filteredKeys: [],
  nextIndex: 0,
  topSpacerHeight: 0,
  topSpacerRow: null,
  topSpacerCell: null,
  boundRowsEl: null,
  boundScrollerEl: null,
  hasRendered: false,
};

function fallbackAccountNameFromId(accountId) {
  const raw = String(accountId || "").trim();
  if (!raw) return "";
  const sep = raw.indexOf("::");
  if (sep < 0) return "";
  return raw.slice(sep + 2).trim();
}

function resolveAccountDisplayName(item) {
  const accountId = item?.accountId || item?.account?.id || "";
  const directLabel = item?.accountLabel || item?.account?.label || "";
  if (directLabel) return directLabel;
  if (accountId) {
    const found = state.accountList.find((account) => account?.id === accountId);
    if (found?.label) {
      return found.label;
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
  return [
    item?.id ?? "",
    item?.createdAt ?? "",
    item?.accountLabel ?? "",
    item?.accountId ?? "",
    item?.keyId ?? "",
    item?.method ?? "",
    item?.requestPath ?? "",
    item?.model ?? "",
    item?.reasoningEffort ?? "",
    item?.statusCode ?? "",
    item?.error ?? "",
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
  const cellTime = document.createElement("td");
  cellTime.textContent = formatTs(item.createdAt, { emptyLabel: "-" });
  row.appendChild(cellTime);

  const cellAccount = document.createElement("td");
  const accountLabel = resolveAccountDisplayName(item);
  const accountId = item?.accountId || item?.account?.id || "";
  if (accountLabel) {
    cellAccount.textContent = accountLabel;
    cellAccount.title = accountId || accountLabel;
  } else if (accountId) {
    cellAccount.textContent = accountId;
  } else {
    cellAccount.textContent = "-";
  }
  row.appendChild(cellAccount);

  const cellKey = document.createElement("td");
  cellKey.textContent = item.keyId || "-";
  row.appendChild(cellKey);

  const cellMethod = document.createElement("td");
  cellMethod.textContent = item.method || "-";
  row.appendChild(cellMethod);

  const cellPath = document.createElement("td");
  const pathWrap = document.createElement("div");
  pathWrap.className = "request-path-wrap";
  const pathText = document.createElement("span");
  pathText.className = "request-path";
  pathText.textContent = item.requestPath || "-";
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
  cellModel.textContent = item.model || "-";
  row.appendChild(cellModel);

  const cellEffort = document.createElement("td");
  cellEffort.textContent = item.reasoningEffort || "-";
  row.appendChild(cellEffort);

  const cellStatus = document.createElement("td");
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
  cellError.textContent = item.error || "-";
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
  let removedHeight = 0;
  for (let i = 0; i < removeCount; i += 1) {
    const row = rows[i];
    removedHeight += getRowHeight(row);
    row.remove();
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
  if (!isNearBottom(requestLogWindowState.boundScrollerEl)) {
    return;
  }
  appendRequestLogBatch();
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
  }
  if (scroller && requestLogWindowState.boundScrollerEl !== scroller) {
    scroller.addEventListener("scroll", onRequestLogScroll);
    requestLogWindowState.boundScrollerEl = scroller;
  } else if (!scroller) {
    requestLogWindowState.boundScrollerEl = null;
  }
}

export function renderRequestLogs() {
  if (!dom.requestLogRows) {
    return;
  }
  ensureRequestLogBindings();
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
      appendRequestLogBatch();
    }
    return;
  }

  dom.requestLogRows.innerHTML = "";
  requestLogWindowState.filtered = filtered;
  requestLogWindowState.filteredKeys = filteredKeys;
  requestLogWindowState.filter = filter;
  requestLogWindowState.nextIndex = 0;
  requestLogWindowState.topSpacerHeight = 0;
  requestLogWindowState.topSpacerRow = null;
  requestLogWindowState.topSpacerCell = null;
  requestLogWindowState.hasRendered = true;
  if (!filtered.length) {
    renderEmptyRequestLogs();
    return;
  }
  dom.requestLogRows.appendChild(createTopSpacerRow());
  appendRequestLogBatch();
}
