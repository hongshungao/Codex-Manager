let requestLogSearchTimer = null;

export function bindFilterEvents({
  dom,
  state,
  handleClearRequestLogs,
  refreshRequestLogs,
  renderRequestLogs,
  renderAccountsView,
  updateRequestLogFilterButtons,
}) {
  if (dom.refreshRequestLogs) {
    dom.refreshRequestLogs.addEventListener("click", async () => {
      await refreshRequestLogs(state.requestLogQuery);
      renderRequestLogs();
    });
  }
  if (dom.clearRequestLogs) {
    dom.clearRequestLogs.addEventListener("click", handleClearRequestLogs);
  }
  if (dom.requestLogSearch) {
    dom.requestLogSearch.addEventListener("input", (event) => {
      state.requestLogQuery = event.target.value || "";
      if (requestLogSearchTimer) {
        clearTimeout(requestLogSearchTimer);
      }
      requestLogSearchTimer = setTimeout(async () => {
        await refreshRequestLogs(state.requestLogQuery);
        renderRequestLogs();
      }, 220);
    });
  }
  const setLogFilter = (value) => {
    state.requestLogStatusFilter = value;
    updateRequestLogFilterButtons();
    renderRequestLogs();
  };
  if (dom.filterLogAll) dom.filterLogAll.addEventListener("click", () => setLogFilter("all"));
  if (dom.filterLog2xx) dom.filterLog2xx.addEventListener("click", () => setLogFilter("2xx"));
  if (dom.filterLog4xx) dom.filterLog4xx.addEventListener("click", () => setLogFilter("4xx"));
  if (dom.filterLog5xx) dom.filterLog5xx.addEventListener("click", () => setLogFilter("5xx"));

  if (dom.accountSearch) {
    dom.accountSearch.addEventListener("input", (event) => {
      state.accountSearch = event.target.value || "";
      renderAccountsView();
    });
  }

  if (dom.accountGroupFilter) {
    dom.accountGroupFilter.addEventListener("change", (event) => {
      state.accountGroupFilter = event.target.value || "all";
      renderAccountsView();
    });
  }

  const updateFilterButtons = () => {
    if (dom.filterAll) dom.filterAll.classList.toggle("active", state.accountFilter === "all");
    if (dom.filterActive) dom.filterActive.classList.toggle("active", state.accountFilter === "active");
    if (dom.filterLow) dom.filterLow.classList.toggle("active", state.accountFilter === "low");
  };

  const setFilter = (filter) => {
    state.accountFilter = filter;
    updateFilterButtons();
    renderAccountsView();
  };

  if (dom.filterAll) dom.filterAll.addEventListener("click", () => setFilter("all"));
  if (dom.filterActive) dom.filterActive.addEventListener("click", () => setFilter("active"));
  if (dom.filterLow) dom.filterLow.addEventListener("click", () => setFilter("low"));
}
