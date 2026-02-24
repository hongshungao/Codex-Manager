export function createNavigationHandlers({ state, dom, closeThemePanel, onPageActivated }) {
  function switchPage(page) {
    if (state.currentPage === page) {
      closeThemePanel();
      return;
    }
    state.currentPage = page;
    closeThemePanel();
    dom.navDashboard?.classList.toggle("active", page === "dashboard");
    dom.navAccounts?.classList.toggle("active", page === "accounts");
    dom.navApiKeys?.classList.toggle("active", page === "apikeys");
    dom.navRequestLogs?.classList.toggle("active", page === "requestlogs");
    dom.navSettings?.classList.toggle("active", page === "settings");
    dom.pageDashboard?.classList.toggle("active", page === "dashboard");
    dom.pageAccounts?.classList.toggle("active", page === "accounts");
    dom.pageApiKeys?.classList.toggle("active", page === "apikeys");
    dom.pageRequestLogs?.classList.toggle("active", page === "requestlogs");
    dom.pageSettings?.classList.toggle("active", page === "settings");
    dom.pageTitle.textContent =
      page === "dashboard"
        ? "仪表盘"
        : page === "accounts"
          ? "账号管理"
          : page === "apikeys"
            ? "平台 Key"
            : page === "requestlogs"
              ? "请求日志"
              : "设置";
    onPageActivated?.(page);
  }

  function updateRequestLogFilterButtons() {
    const current = state.requestLogStatusFilter || "all";
    if (dom.filterLogAll) dom.filterLogAll.classList.toggle("active", current === "all");
    if (dom.filterLog2xx) dom.filterLog2xx.classList.toggle("active", current === "2xx");
    if (dom.filterLog4xx) dom.filterLog4xx.classList.toggle("active", current === "4xx");
    if (dom.filterLog5xx) dom.filterLog5xx.classList.toggle("active", current === "5xx");
  }

  return {
    switchPage,
    updateRequestLogFilterButtons,
  };
}
