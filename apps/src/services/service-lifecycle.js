export function createServiceLifecycle({
  state,
  dom,
  setServiceHint,
  normalizeAddr,
  startService,
  stopService,
  waitForConnection,
  refreshAll,
  ensureAutoRefreshTimer,
  stopAutoRefreshTimer,
  onStartupState,
}) {
  function notifyStartupState(loading, message) {
    if (typeof onStartupState !== "function") return;
    onStartupState(loading, message);
  }

  function updateServiceToggle() {
    if (!dom.serviceToggleBtn) return;
    if (state.serviceBusy) return;
    dom.serviceToggleBtn.checked = state.serviceConnected;
    dom.serviceToggleBtn.setAttribute("aria-checked", state.serviceConnected ? "true" : "false");
    if (dom.serviceToggleText) {
      dom.serviceToggleText.textContent = state.serviceConnected ? "已连接" : "未连接";
    }
  }

  function setServiceBusy(busy, mode) {
    state.serviceBusy = busy;
    if (!dom.serviceToggleBtn) return;
    dom.serviceToggleBtn.disabled = busy;
    dom.serviceToggleBtn.classList.toggle("is-busy", busy);
    if (busy) {
      if (dom.serviceToggleText) {
        dom.serviceToggleText.textContent = mode === "stop" ? "断开中..." : "连接中...";
      }
    } else {
      updateServiceToggle();
    }
  }

  function syncServiceAddrFromInput() {
    if (!dom.serviceAddrInput) return;
    const raw = dom.serviceAddrInput.value;
    if (!raw) return;
    try {
      state.serviceAddr = normalizeAddr(raw);
    } catch (err) {
      // ignore invalid input during bootstrap
    }
  }

  function restoreServiceAddr() {
    const savedAddr = localStorage.getItem("codexmanager.service.addr");
    if (savedAddr) {
      state.serviceAddr = savedAddr;
      dom.serviceAddrInput.value = savedAddr;
      syncServiceAddrFromInput();
      return;
    }
    dom.serviceAddrInput.value = "48760";
    syncServiceAddrFromInput();
  }

  async function handleStartService(options = {}) {
    const { fromBootstrap = false } = options;
    if (fromBootstrap) {
      notifyStartupState(true, "正在启动本地服务...");
    }
    setServiceBusy(true, "start");
    const started = await startService(dom.serviceAddrInput.value, {
      skipInitialize: true,
    });
    dom.serviceAddrInput.value = state.serviceAddr;
    localStorage.setItem("codexmanager.service.addr", state.serviceAddr);
    if (!started) {
      setServiceBusy(false);
      updateServiceToggle();
      if (fromBootstrap) notifyStartupState(false);
      return false;
    }
    const probeId = state.serviceProbeId + 1;
    state.serviceProbeId = probeId;
    if (fromBootstrap) {
      notifyStartupState(true, "正在等待服务响应...");
    }
    const ok = await waitForConnection({ retries: 12, delayMs: 400, silent: true });
    if (state.serviceProbeId !== probeId) {
      if (fromBootstrap) notifyStartupState(false);
      return false;
    }
    setServiceBusy(false);
    updateServiceToggle();
    if (!ok) {
      const reason = state.serviceLastError ? `：${state.serviceLastError}` : "";
      setServiceHint(`连接失败${reason}，请检查端口或 service 状态`, true);
      if (fromBootstrap) notifyStartupState(false);
      return false;
    }
    if (fromBootstrap) {
      notifyStartupState(true, "正在加载账号与用量数据...");
    }
    await refreshAll();
    ensureAutoRefreshTimer(state, refreshAll);
    if (fromBootstrap) notifyStartupState(false);
    return true;
  }

  async function handleStopService() {
    setServiceBusy(true, "stop");
    state.serviceProbeId += 1;
    await stopService();
    setServiceBusy(false);
    updateServiceToggle();
    stopAutoRefreshTimer(state);
  }

  async function handleServiceToggle() {
    if (state.serviceBusy) return;
    if (state.serviceConnected) {
      await handleStopService();
    } else {
      await handleStartService();
    }
  }

  async function autoStartService() {
    if (!dom.serviceAddrInput) return;
    notifyStartupState(true, "正在检查服务连接...");
    syncServiceAddrFromInput();
    const probeId = state.serviceProbeId + 1;
    state.serviceProbeId = probeId;
    const ok = await waitForConnection({
      retries: 1,
      delayMs: 200,
      silent: true,
    });
    if (state.serviceProbeId !== probeId) {
      notifyStartupState(false);
      return false;
    }
    if (ok) {
      updateServiceToggle();
      notifyStartupState(true, "正在加载账号与用量数据...");
      await refreshAll();
      // 中文注释：探活成功后立即复用统一定时器入口，避免“已连通但未启动自动刷新”的状态分叉。
      ensureAutoRefreshTimer(state, refreshAll);
      notifyStartupState(false);
      return true;
    }
    return handleStartService({ fromBootstrap: true });
  }

  return {
    updateServiceToggle,
    restoreServiceAddr,
    autoStartService,
    handleServiceToggle,
  };
}
