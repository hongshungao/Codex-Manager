let navigationEventsBound = false;

export function bindNavigationAndServiceEvents({
  dom,
  switchPage,
  refreshAll,
  toggleThemePanel,
  closeThemePanel,
  setTheme,
  handleServiceToggle,
}) {
  if (navigationEventsBound) {
    return;
  }
  navigationEventsBound = true;

  let themeGlobalListenersBound = false;
  const bindThemeGlobalListeners = () => {
    if (themeGlobalListenersBound) return;
    document.addEventListener("click", handleOutsideThemeClick);
    document.addEventListener("keydown", handleThemeEscape);
    themeGlobalListenersBound = true;
  };
  const unbindThemeGlobalListeners = () => {
    if (!themeGlobalListenersBound) return;
    document.removeEventListener("click", handleOutsideThemeClick);
    document.removeEventListener("keydown", handleThemeEscape);
    themeGlobalListenersBound = false;
  };
  const handleOutsideThemeClick = () => {
    if (!dom.themePanel || dom.themePanel.hidden) {
      unbindThemeGlobalListeners();
      return;
    }
    closeThemePanel();
    unbindThemeGlobalListeners();
  };
  const handleThemeEscape = (event) => {
    if (event.key !== "Escape") return;
    closeThemePanel();
    unbindThemeGlobalListeners();
  };
  const handleSwitchPage = (page) => {
    switchPage(page);
    unbindThemeGlobalListeners();
  };

  if (dom.navDashboard) dom.navDashboard.addEventListener("click", () => handleSwitchPage("dashboard"));
  if (dom.navAccounts) dom.navAccounts.addEventListener("click", () => handleSwitchPage("accounts"));
  if (dom.navApiKeys) dom.navApiKeys.addEventListener("click", () => handleSwitchPage("apikeys"));
  if (dom.navRequestLogs) dom.navRequestLogs.addEventListener("click", () => handleSwitchPage("requestlogs"));

  if (dom.refreshAll) {
    dom.refreshAll.addEventListener("click", refreshAll);
  }

  if (dom.themeToggle) {
    dom.themeToggle.addEventListener("click", (event) => {
      event.stopPropagation();
      const wasHidden = !dom.themePanel || dom.themePanel.hidden;
      toggleThemePanel();
      const isHidden = !dom.themePanel || dom.themePanel.hidden;
      if (wasHidden && !isHidden) {
        bindThemeGlobalListeners();
      } else if (!wasHidden && isHidden) {
        unbindThemeGlobalListeners();
      }
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
          unbindThemeGlobalListeners();
        }
      }
      event.stopPropagation();
    });
  }

  if (dom.serviceToggleBtn) {
    dom.serviceToggleBtn.addEventListener("click", handleServiceToggle);
  }
}
