import { copyText } from "../../utils/clipboard.js";

let apiModelLoadSeq = 0;
let modalActionEventsBound = false;

export function bindModalActionEvents({
  dom,
  state,
  openAccountModal,
  openApiKeyModal,
  closeAccountModal,
  handleLogin,
  showToast,
  handleManualCallback,
  closeUsageModal,
  refreshUsageForAccount,
  closeApiKeyModal,
  createApiKey,
  ensureConnected,
  refreshApiModels,
  refreshApiModelsNow,
  populateApiKeyModelSelect,
  importAccountsFromFiles,
}) {
  if (modalActionEventsBound) {
    return;
  }
  modalActionEventsBound = true;

  if (dom.addAccountBtn) dom.addAccountBtn.addEventListener("click", openAccountModal);
  if (dom.importAccountsBtn && dom.importAccountsInput) {
    dom.importAccountsBtn.addEventListener("click", () => {
      dom.importAccountsInput.click();
    });
    dom.importAccountsInput.addEventListener("change", (event) => {
      const files = event?.target?.files;
      void importAccountsFromFiles?.(files);
      event.target.value = "";
    });
  }
  if (dom.refreshApiModelsBtn) {
    dom.refreshApiModelsBtn.addEventListener("click", () => {
      void refreshApiModelsNow?.();
    });
  }

  if (dom.createApiKeyBtn) dom.createApiKeyBtn.addEventListener("click", async () => {
    openApiKeyModal();
    // 中文注释：先用本地缓存秒开；仅在模型列表为空时再后台懒加载，避免弹窗开关被网络拖慢。
    if (state.apiModelOptions && state.apiModelOptions.length > 0) {
      return;
    }
    const currentSeq = ++apiModelLoadSeq;
    const ok = await ensureConnected();
    if (!ok || currentSeq !== apiModelLoadSeq) return;
    try {
      if (typeof refreshApiModelsNow === "function") {
        await refreshApiModelsNow({ silent: true, button: null });
      } else {
        await refreshApiModels({ refreshRemote: true });
      }
    } catch (err) {
      showToast(`模型列表刷新失败：${err instanceof Error ? err.message : String(err)}`, "error");
      return;
    }
    if (currentSeq !== apiModelLoadSeq) return;
    if (!dom.modalApiKey || !dom.modalApiKey.classList.contains("active")) return;
    populateApiKeyModelSelect();
  });
  if (dom.closeAccountModal) {
    dom.closeAccountModal.addEventListener("click", closeAccountModal);
  }
  if (dom.cancelLogin) dom.cancelLogin.addEventListener("click", closeAccountModal);
  if (dom.submitLogin) dom.submitLogin.addEventListener("click", handleLogin);
  if (dom.copyLoginUrl) dom.copyLoginUrl.addEventListener("click", async () => {
    if (!dom.loginUrl.value) return;
    const ok = await copyText(dom.loginUrl.value);
    if (ok) {
      showToast("授权链接已复制");
    } else {
      showToast("复制失败，请手动复制链接", "error");
    }
  });
  if (dom.manualCallbackSubmit) dom.manualCallbackSubmit.addEventListener("click", handleManualCallback);
  if (dom.closeUsageModal) dom.closeUsageModal.addEventListener("click", closeUsageModal);
  if (dom.refreshUsageSingle) dom.refreshUsageSingle.addEventListener("click", refreshUsageForAccount);
  if (dom.closeApiKeyModal) {
    dom.closeApiKeyModal.addEventListener("click", closeApiKeyModal);
  }
  if (dom.cancelApiKey) dom.cancelApiKey.addEventListener("click", closeApiKeyModal);
  if (dom.submitApiKey) dom.submitApiKey.addEventListener("click", createApiKey);
  if (dom.copyApiKey) dom.copyApiKey.addEventListener("click", async () => {
    if (!dom.apiKeyValue.value) return;
    const ok = await copyText(dom.apiKeyValue.value);
    if (ok) {
      showToast("平台 Key 已复制");
    } else {
      showToast("复制失败，请手动复制", "error");
    }
  });
  if (dom.inputApiKeyModel && dom.inputApiKeyReasoning) {
    const syncReasoningSelect = () => {
      const enabled = Boolean((dom.inputApiKeyModel.value || "").trim());
      dom.inputApiKeyReasoning.disabled = !enabled;
      if (!enabled) {
        dom.inputApiKeyReasoning.value = "";
      }
    };
    dom.inputApiKeyModel.addEventListener("change", syncReasoningSelect);
    syncReasoningSelect();
  }
  if (dom.inputApiKeyProtocol) {
    const syncApiKeyProtocolFields = () => {
      const protocolType = dom.inputApiKeyProtocol.value || "openai_compat";
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
    };
    dom.inputApiKeyProtocol.addEventListener("change", syncApiKeyProtocolFields);
    syncApiKeyProtocolFields();
  }
}
