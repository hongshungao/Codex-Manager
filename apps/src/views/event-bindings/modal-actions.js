let apiModelLoadSeq = 0;

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
  populateApiKeyModelSelect,
}) {
  dom.addAccountBtn.addEventListener("click", openAccountModal);
  dom.createApiKeyBtn.addEventListener("click", async () => {
    openApiKeyModal();
    // 中文注释：先用本地缓存秒开；仅在模型列表为空时再后台懒加载，避免弹窗开关被网络拖慢。
    if (state.apiModelOptions && state.apiModelOptions.length > 0) {
      return;
    }
    const currentSeq = ++apiModelLoadSeq;
    const ok = await ensureConnected();
    if (!ok || currentSeq !== apiModelLoadSeq) return;
    await refreshApiModels();
    if (currentSeq !== apiModelLoadSeq) return;
    if (!dom.modalApiKey || !dom.modalApiKey.classList.contains("active")) return;
    populateApiKeyModelSelect();
  });
  if (dom.closeAccountModal) {
    dom.closeAccountModal.addEventListener("click", closeAccountModal);
  }
  dom.cancelLogin.addEventListener("click", closeAccountModal);
  dom.submitLogin.addEventListener("click", handleLogin);
  dom.copyLoginUrl.addEventListener("click", () => {
    if (!dom.loginUrl.value) return;
    dom.loginUrl.select();
    dom.loginUrl.setSelectionRange(0, dom.loginUrl.value.length);
    try {
      document.execCommand("copy");
      showToast("授权链接已复制");
    } catch (_err) {
      showToast("复制失败，请手动复制链接", "error");
    }
  });
  dom.manualCallbackSubmit.addEventListener("click", handleManualCallback);
  dom.closeUsageModal.addEventListener("click", closeUsageModal);
  dom.refreshUsageSingle.addEventListener("click", refreshUsageForAccount);
  if (dom.closeApiKeyModal) {
    dom.closeApiKeyModal.addEventListener("click", closeApiKeyModal);
  }
  dom.cancelApiKey.addEventListener("click", closeApiKeyModal);
  dom.submitApiKey.addEventListener("click", createApiKey);
  dom.copyApiKey.addEventListener("click", () => {
    if (!dom.apiKeyValue.value) return;
    dom.apiKeyValue.select();
    dom.apiKeyValue.setSelectionRange(0, dom.apiKeyValue.value.length);
    try {
      document.execCommand("copy");
      showToast("平台 Key 已复制");
    } catch (_err) {
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
}
