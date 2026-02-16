export function createStartupMaskController({ dom, state }) {
  const startupMaskState = {
    active: false,
    startedAt: 0,
    slowTimer: null,
    slowTicker: null,
  };

  function clearStartupMaskWatchers() {
    if (startupMaskState.slowTimer) {
      clearTimeout(startupMaskState.slowTimer);
      startupMaskState.slowTimer = null;
    }
    if (startupMaskState.slowTicker) {
      clearInterval(startupMaskState.slowTicker);
      startupMaskState.slowTicker = null;
    }
  }

  function hideStartupMaskDetail() {
    if (!dom.startupMaskDetail) return;
    dom.startupMaskDetail.hidden = true;
    dom.startupMaskDetail.textContent = "";
  }

  function updateStartupMaskDetail() {
    if (!startupMaskState.active || !dom.startupMaskDetail) return;
    const elapsedSec = Math.max(
      0,
      Math.floor((Date.now() - startupMaskState.startedAt) / 1000),
    );
    const addr = state.serviceAddr || "localhost:48760";
    const reason = state.serviceLastError
      ? `；最近错误：${state.serviceLastError}`
      : "";
    dom.startupMaskDetail.hidden = false;
    dom.startupMaskDetail.textContent = `启动耗时 ${elapsedSec}s，正在连接 ${addr}${reason}`;
  }

  function startStartupMaskWatchers() {
    clearStartupMaskWatchers();
    hideStartupMaskDetail();
    startupMaskState.slowTimer = setTimeout(() => {
      updateStartupMaskDetail();
      startupMaskState.slowTicker = setInterval(updateStartupMaskDetail, 1000);
    }, 5000);
  }

  function setStartupMask(active, message) {
    if (!dom.startupMask) return;
    if (active && !startupMaskState.active) {
      startupMaskState.active = true;
      startupMaskState.startedAt = Date.now();
      startStartupMaskWatchers();
    } else if (!active && startupMaskState.active) {
      startupMaskState.active = false;
      startupMaskState.startedAt = 0;
      clearStartupMaskWatchers();
      hideStartupMaskDetail();
    }
    dom.startupMask.classList.toggle("active", active);
    dom.startupMask.setAttribute("aria-hidden", active ? "false" : "true");
    if (dom.startupMaskText && message) {
      dom.startupMaskText.textContent = message;
    }
  }

  return {
    setStartupMask,
  };
}
