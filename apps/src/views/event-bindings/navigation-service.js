export function bindNavigationAndServiceEvents({
  dom,
  switchPage,
  refreshAll,
  toggleThemePanel,
  closeThemePanel,
  setTheme,
  handleServiceToggle,
}) {
  dom.navDashboard.addEventListener("click", () => switchPage("dashboard"));
  dom.navAccounts.addEventListener("click", () => switchPage("accounts"));
  dom.navApiKeys.addEventListener("click", () => switchPage("apikeys"));
  dom.navRequestLogs.addEventListener("click", () => switchPage("requestlogs"));

  if (dom.refreshAll) {
    dom.refreshAll.addEventListener("click", refreshAll);
  }

  if (dom.themeToggle) {
    dom.themeToggle.addEventListener("click", (event) => {
      event.stopPropagation();
      toggleThemePanel();
    });
  }
  if (dom.themePanel) {
    dom.themePanel.addEventListener("click", (event) => {
      const target = event.target;
      if (target instanceof HTMLElement) {
        const themeButton = target.closest("button[data-theme]");
        if (themeButton && themeButton.dataset.theme) {
          setTheme(themeButton.dataset.theme);
          closeThemePanel();
        }
      }
      event.stopPropagation();
    });
  }
  document.addEventListener("click", () => closeThemePanel());
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") closeThemePanel();
  });

  dom.serviceToggleBtn.addEventListener("click", handleServiceToggle);
}
