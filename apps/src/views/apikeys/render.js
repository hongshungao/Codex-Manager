import { state } from "../../state.js";
import { dom } from "../../ui/dom.js";
import {
  REASONING_OPTIONS,
  getProtocolProfileLabel,
  getStatusViewModel,
  mapReasoningEffortToSelectValue,
} from "./state.js";

const APIKEY_ACTION_TOGGLE = "toggle";
const APIKEY_ACTION_DELETE = "delete";
const APIKEY_ACTION_COPY = "copy";
const APIKEY_FIELD_MODEL = "model";
const APIKEY_FIELD_REASONING = "reasoning";

let apiKeyRowsEventsBound = false;
let apiKeyRowHandlers = null;
let apiKeyLookupById = new Map();
let modelOptionSignature = "";
let modelOptionTemplate = null;
let reasoningOptionTemplate = null;

function getModelSignature() {
  return (state.apiModelOptions || [])
    .map((model) => `${model.slug || ""}:${model.displayName || ""}`)
    .join("|");
}

function buildModelOptionTemplate() {
  const fragment = document.createDocumentFragment();
  const followOption = document.createElement("option");
  followOption.value = "";
  followOption.textContent = "跟随请求模型";
  fragment.appendChild(followOption);
  (state.apiModelOptions || []).forEach((model) => {
    const option = document.createElement("option");
    option.value = model.slug;
    option.textContent = model.displayName || model.slug;
    fragment.appendChild(option);
  });
  return fragment;
}

function getModelOptionTemplate() {
  const signature = getModelSignature();
  if (!modelOptionTemplate || signature !== modelOptionSignature) {
    modelOptionTemplate = buildModelOptionTemplate();
    modelOptionSignature = signature;
  }
  return modelOptionTemplate;
}

function getReasoningOptionTemplate() {
  if (!reasoningOptionTemplate) {
    const fragment = document.createDocumentFragment();
    REASONING_OPTIONS.forEach((optionItem) => {
      const option = document.createElement("option");
      option.value = optionItem.value;
      option.textContent = optionItem.label;
      fragment.appendChild(option);
    });
    reasoningOptionTemplate = fragment;
  }
  return reasoningOptionTemplate;
}

function appendModelOptions(select) {
  select.appendChild(getModelOptionTemplate().cloneNode(true));
}

function appendReasoningOptions(select) {
  select.appendChild(getReasoningOptionTemplate().cloneNode(true));
}

function syncEffortState(modelSelect, effortSelect) {
  const hasModelOverride = Boolean((modelSelect.value || "").trim());
  effortSelect.disabled = !hasModelOverride;
  if (!hasModelOverride) {
    effortSelect.value = "";
  }
}

function createModelCell(item) {
  const cellModel = document.createElement("td");
  const modelWrap = document.createElement("div");
  modelWrap.className = "cell-stack";
  const modelSelect = document.createElement("select");
  modelSelect.className = "inline-select";
  modelSelect.setAttribute("data-field", APIKEY_FIELD_MODEL);
  appendModelOptions(modelSelect);

  const effortSelect = document.createElement("select");
  effortSelect.className = "inline-select";
  effortSelect.setAttribute("data-field", APIKEY_FIELD_REASONING);
  appendReasoningOptions(effortSelect);

  modelSelect.value = item.modelSlug || "";
  effortSelect.value = mapReasoningEffortToSelectValue(item.reasoningEffort);
  syncEffortState(modelSelect, effortSelect);
  modelWrap.appendChild(modelSelect);
  modelWrap.appendChild(effortSelect);
  cellModel.appendChild(modelWrap);
  return cellModel;
}

function createStatusCell(item) {
  const cellStatus = document.createElement("td");
  const statusViewModel = getStatusViewModel(item.status);
  const statusTag = document.createElement("span");
  statusTag.className = "status-tag";
  statusTag.classList.add(statusViewModel.className);
  statusTag.textContent = statusViewModel.label;
  cellStatus.appendChild(statusTag);
  return { cellStatus, isDisabled: statusViewModel.isDisabled };
}

function createActionsCell(isDisabled) {
  const cellActions = document.createElement("td");
  const actionsWrap = document.createElement("div");
  actionsWrap.className = "cell-actions";
  const btnCopy = document.createElement("button");
  btnCopy.className = "ghost";
  btnCopy.type = "button";
  btnCopy.setAttribute("data-action", APIKEY_ACTION_COPY);
  btnCopy.textContent = "复制Key";

  const btnDisable = document.createElement("button");
  btnDisable.className = "secondary";
  btnDisable.type = "button";
  btnDisable.setAttribute("data-action", APIKEY_ACTION_TOGGLE);
  btnDisable.textContent = isDisabled ? "启用" : "禁用";

  const btnDelete = document.createElement("button");
  btnDelete.className = "danger";
  btnDelete.type = "button";
  btnDelete.setAttribute("data-action", APIKEY_ACTION_DELETE);
  btnDelete.textContent = "删除";
  actionsWrap.appendChild(btnCopy);
  actionsWrap.appendChild(btnDisable);
  actionsWrap.appendChild(btnDelete);
  cellActions.appendChild(actionsWrap);
  return cellActions;
}

function renderApiKeyRow(item, parent = dom.apiKeyRows) {
  const row = document.createElement("tr");
  row.setAttribute("data-key-id", item.id || "");
  const cellId = document.createElement("td");
  cellId.className = "mono";
  cellId.textContent = item.id;

  const cellName = document.createElement("td");
  cellName.textContent = item.name || "-";

  const cellProfile = document.createElement("td");
  const protocolType = item.protocolType || "openai_compat";
  cellProfile.textContent = getProtocolProfileLabel(protocolType);

  const cellModel = createModelCell(item);
  const { cellStatus, isDisabled } = createStatusCell(item);
  const cellActions = createActionsCell(isDisabled);

  row.appendChild(cellId);
  row.appendChild(cellName);
  row.appendChild(cellProfile);
  row.appendChild(cellModel);
  row.appendChild(cellStatus);
  row.appendChild(cellActions);
  parent?.appendChild(row);
  return row;
}

function getApiKeyFromRow(row, lookup) {
  const keyId = row?.dataset?.keyId;
  if (!keyId) return null;
  return lookup.get(keyId) || null;
}

export function handleApiKeyRowsClick(target, handlers = apiKeyRowHandlers, lookup = apiKeyLookupById) {
  const actionButton = target?.closest?.("button[data-action]");
  if (!actionButton) return false;
  const row = actionButton.closest("tr[data-key-id]");
  if (!row) return false;
  const item = getApiKeyFromRow(row, lookup);
  if (!item) return false;
  const action = actionButton.dataset.action;
  if (action === APIKEY_ACTION_TOGGLE) {
    handlers?.onToggleStatus?.(item);
    return true;
  }
  if (action === APIKEY_ACTION_DELETE) {
    handlers?.onDelete?.(item);
    return true;
  }
  if (action === APIKEY_ACTION_COPY) {
    handlers?.onCopy?.(item, actionButton);
    return true;
  }
  return false;
}

export function handleApiKeyRowsChange(target, handlers = apiKeyRowHandlers, lookup = apiKeyLookupById) {
  const changedSelect = target?.closest?.("select[data-field]");
  if (!changedSelect) return false;
  const row = changedSelect.closest("tr[data-key-id]");
  if (!row) return false;
  const item = getApiKeyFromRow(row, lookup);
  if (!item) return false;
  const modelSelect = row.querySelector(`select[data-field="${APIKEY_FIELD_MODEL}"]`);
  const effortSelect = row.querySelector(`select[data-field="${APIKEY_FIELD_REASONING}"]`);
  if (!modelSelect || !effortSelect) return false;
  if (changedSelect.dataset.field === APIKEY_FIELD_MODEL) {
    syncEffortState(modelSelect, effortSelect);
  }
  handlers?.onUpdateModel?.(item, modelSelect.value, effortSelect.value);
  return true;
}

function ensureApiKeyRowsEventsBound() {
  if (apiKeyRowsEventsBound || !dom.apiKeyRows) {
    return;
  }
  apiKeyRowsEventsBound = true;
  dom.apiKeyRows.addEventListener("click", (event) => {
    handleApiKeyRowsClick(event.target);
  });
  dom.apiKeyRows.addEventListener("change", (event) => {
    handleApiKeyRowsChange(event.target);
  });
}

function renderEmptyRow() {
  const emptyRow = document.createElement("tr");
  const emptyCell = document.createElement("td");
  emptyCell.colSpan = 6;
  emptyCell.textContent = "暂无平台 Key";
  emptyRow.appendChild(emptyCell);
  dom.apiKeyRows.appendChild(emptyRow);
}

// 渲染 API Key 列表
export function renderApiKeys({ onToggleStatus, onDelete, onUpdateModel, onCopy }) {
  ensureApiKeyRowsEventsBound();
  apiKeyRowHandlers = { onToggleStatus, onDelete, onUpdateModel, onCopy };
  dom.apiKeyRows.innerHTML = "";
  if (state.apiKeyList.length === 0) {
    apiKeyLookupById = new Map();
    renderEmptyRow();
    return;
  }

  apiKeyLookupById = new Map(state.apiKeyList.map((item) => [item.id, item]));
  const fragment = document.createDocumentFragment();
  state.apiKeyList.forEach((item) => renderApiKeyRow(item, fragment));
  dom.apiKeyRows.appendChild(fragment);
}
