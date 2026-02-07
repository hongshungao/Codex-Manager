import { state } from "../state";
import { dom } from "../ui/dom";

const REASONING_OPTIONS = [
  { value: "", label: "跟随请求等级" },
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "xhigh", label: "XHigh" },
];

// 渲染 API Key 列表
export function renderApiKeys({ onToggleStatus, onDelete, onUpdateModel }) {
  dom.apiKeyRows.innerHTML = "";
  if (state.apiKeyList.length === 0) {
    const emptyRow = document.createElement("tr");
    const emptyCell = document.createElement("td");
    emptyCell.colSpan = 6;
    emptyCell.textContent = "暂无平台 Key";
    emptyRow.appendChild(emptyCell);
    dom.apiKeyRows.appendChild(emptyRow);
    return;
  }

  state.apiKeyList.forEach((item) => {
    const row = document.createElement("tr");
    const cellId = document.createElement("td");
    cellId.className = "mono";
    cellId.textContent = item.id;

    const cellName = document.createElement("td");
    cellName.textContent = item.name || "-";

    const cellModel = document.createElement("td");
    const modelWrap = document.createElement("div");
    modelWrap.className = "cell-stack";
    const modelSelect = document.createElement("select");
    modelSelect.className = "inline-select";
    const followOption = document.createElement("option");
    followOption.value = "";
    followOption.textContent = "跟随请求模型";
    modelSelect.appendChild(followOption);
    state.apiModelOptions.forEach((model) => {
      const option = document.createElement("option");
      option.value = model.slug;
      option.textContent = model.displayName || model.slug;
      modelSelect.appendChild(option);
    });
    const effortSelect = document.createElement("select");
    effortSelect.className = "inline-select";
    REASONING_OPTIONS.forEach((optionItem) => {
      const option = document.createElement("option");
      option.value = optionItem.value;
      option.textContent = optionItem.label;
      effortSelect.appendChild(option);
    });
    modelSelect.value = item.modelSlug || "";
    effortSelect.value = item.reasoningEffort === "extra_high"
      ? "xhigh"
      : (item.reasoningEffort || "");
    const syncEffortState = () => {
      const hasModelOverride = Boolean((modelSelect.value || "").trim());
      effortSelect.disabled = !hasModelOverride;
      if (!hasModelOverride) {
        effortSelect.value = "";
      }
    };
    modelSelect.addEventListener("change", () => {
      syncEffortState();
      onUpdateModel?.(item, modelSelect.value, effortSelect.value);
    });
    effortSelect.addEventListener("change", () => {
      onUpdateModel?.(item, modelSelect.value, effortSelect.value);
    });
    syncEffortState();
    modelWrap.appendChild(modelSelect);
    modelWrap.appendChild(effortSelect);
    cellModel.appendChild(modelWrap);

    const cellStatus = document.createElement("td");
    const statusTag = document.createElement("span");
    statusTag.className = "status-tag";
    const normalizedStatus = String(item.status || "").toLowerCase();
    if (normalizedStatus === "active") {
      statusTag.classList.add("status-ok");
      statusTag.textContent = "启用";
    } else if (normalizedStatus === "disabled") {
      statusTag.classList.add("status-bad");
      statusTag.textContent = "禁用";
    } else {
      statusTag.classList.add("status-unknown");
      statusTag.textContent = item.status || "未知";
    }
    cellStatus.appendChild(statusTag);

    const cellUsed = document.createElement("td");
    cellUsed.textContent = item.lastUsedAt
      ? new Date(item.lastUsedAt * 1000).toLocaleString()
      : "-";

    const cellActions = document.createElement("td");
    const actionsWrap = document.createElement("div");
    actionsWrap.className = "cell-actions";
    const btnDisable = document.createElement("button");
    btnDisable.className = "secondary";
    const isDisabled = String(item.status || "").toLowerCase() === "disabled";
    btnDisable.textContent = isDisabled ? "启用" : "禁用";
    btnDisable.addEventListener("click", () => onToggleStatus?.(item));
    const btnDelete = document.createElement("button");
    btnDelete.className = "danger";
    btnDelete.textContent = "删除";
    btnDelete.addEventListener("click", () => onDelete?.(item));
    actionsWrap.appendChild(btnDisable);
    actionsWrap.appendChild(btnDelete);
    cellActions.appendChild(actionsWrap);

    row.appendChild(cellId);
    row.appendChild(cellName);
    row.appendChild(cellModel);
    row.appendChild(cellStatus);
    row.appendChild(cellUsed);
    row.appendChild(cellActions);
    dom.apiKeyRows.appendChild(row);
  });
}

// 打开 API Key 弹窗
export function openApiKeyModal() {
  dom.modalApiKey.classList.add("active");
  dom.inputApiKeyName.value = "";
  populateApiKeyModelSelect();
  if (dom.inputApiKeyModel) {
    dom.inputApiKeyModel.value = "";
  }
  if (dom.inputApiKeyReasoning) {
    dom.inputApiKeyReasoning.value = "";
    dom.inputApiKeyReasoning.disabled = true;
  }
  dom.apiKeyValue.value = "";
}

// 关闭 API Key 弹窗
export function closeApiKeyModal() {
  dom.modalApiKey.classList.remove("active");
  if (dom.inputApiKeyModel) {
    dom.inputApiKeyModel.disabled = false;
  }
}

export function populateApiKeyModelSelect() {
  if (!dom.inputApiKeyModel) return;
  dom.inputApiKeyModel.innerHTML = "";

  const followOption = document.createElement("option");
  followOption.value = "";
  followOption.textContent = "跟随请求模型（不覆盖）";
  dom.inputApiKeyModel.appendChild(followOption);

  state.apiModelOptions.forEach((model) => {
    const option = document.createElement("option");
    option.value = model.slug;
    option.textContent = model.displayName || model.slug;
    dom.inputApiKeyModel.appendChild(option);
  });

  if (dom.inputApiKeyReasoning) {
    dom.inputApiKeyReasoning.innerHTML = "";
    REASONING_OPTIONS.forEach((item) => {
      const option = document.createElement("option");
      option.value = item.value;
      option.textContent = item.label;
      dom.inputApiKeyReasoning.appendChild(option);
    });
  }
}
