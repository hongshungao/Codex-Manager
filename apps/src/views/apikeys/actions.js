import { state } from "../../state.js";
import { dom } from "../../ui/dom.js";
import { REASONING_OPTIONS } from "./state.js";

let modelOptionSignature = "";
let reasoningOptionsReady = false;

function getModelOptionSignature() {
  return (state.apiModelOptions || [])
    .map((model) => `${model.slug || ""}:${model.displayName || ""}`)
    .join("|");
}

function populateReasoningSelect(force = false) {
  if (!dom.inputApiKeyReasoning) return;
  if (reasoningOptionsReady && !force) return;
  dom.inputApiKeyReasoning.innerHTML = "";
  REASONING_OPTIONS.forEach((item) => {
    const option = document.createElement("option");
    option.value = item.value;
    option.textContent = item.label;
    dom.inputApiKeyReasoning.appendChild(option);
  });
  reasoningOptionsReady = true;
}

function syncApiKeyProtocolFields() {
  const protocolType = dom.inputApiKeyProtocol?.value || "openai_compat";
  const isAzureProtocol = protocolType === "azure_openai";
  if (dom.apiKeyAzureFields) {
    dom.apiKeyAzureFields.hidden = !isAzureProtocol;
  }
  if (!isAzureProtocol) {
    if (dom.inputApiKeyEndpoint) {
      dom.inputApiKeyEndpoint.value = "";
    }
    if (dom.inputApiKeyAzureApiKey) {
      dom.inputApiKeyAzureApiKey.value = "";
    }
  }
}

export function populateApiKeyModelSelect(options = {}) {
  const force = Boolean(options.force);
  if (!dom.inputApiKeyModel) return;
  const nextSignature = getModelOptionSignature();
  const shouldRebuildModels = force || nextSignature !== modelOptionSignature;
  if (shouldRebuildModels) {
    dom.inputApiKeyModel.innerHTML = "";

    const followOption = document.createElement("option");
    followOption.value = "";
    followOption.textContent = "跟随请求模型（不覆盖）";
    dom.inputApiKeyModel.appendChild(followOption);

    (state.apiModelOptions || []).forEach((model) => {
      const option = document.createElement("option");
      option.value = model.slug;
      option.textContent = model.displayName || model.slug;
      dom.inputApiKeyModel.appendChild(option);
    });
    modelOptionSignature = nextSignature;
  }

  populateReasoningSelect(force);
}

// 打开 API Key 弹窗
export function openApiKeyModal() {
  dom.modalApiKey.classList.add("active");
  dom.inputApiKeyName.value = "";
  if (dom.inputApiKeyProtocol) {
    dom.inputApiKeyProtocol.value = "openai_compat";
  }
  if (dom.inputApiKeyEndpoint) {
    dom.inputApiKeyEndpoint.value = "";
  }
  if (dom.inputApiKeyAzureApiKey) {
    dom.inputApiKeyAzureApiKey.value = "";
  }
  syncApiKeyProtocolFields();
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
