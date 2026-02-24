const THEME_OPTIONS = [
  { id: "tech", label: "科技蓝" },
  { id: "business", label: "商务金" },
  { id: "mint", label: "薄荷绿" },
  { id: "sunset", label: "晚霞橙" },
  { id: "grape", label: "葡萄紫" },
  { id: "ocean", label: "海湾青" },
  { id: "forest", label: "松林绿" },
  { id: "rose", label: "玫瑰粉" },
  { id: "slate", label: "石板灰" },
  { id: "aurora", label: "极光青" },
];

export function createThemeController({ dom }) {
  function renderThemeButtons() {
    if (!dom.themePanel) return;
    dom.themePanel.innerHTML = "";
    THEME_OPTIONS.forEach((theme) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "secondary";
      button.dataset.theme = theme.id;
      button.textContent = theme.label;
      dom.themePanel.appendChild(button);
    });
  }

  function setTheme(theme) {
    const validThemes = new Set(THEME_OPTIONS.map((item) => item.id));
    const nextTheme = validThemes.has(theme) ? theme : "tech";
    document.body.dataset.theme = nextTheme;
    localStorage.setItem("codexmanager.ui.theme", nextTheme);
    if (dom.themePanel) {
      dom.themePanel.querySelectorAll("button[data-theme]").forEach((button) => {
        button.classList.toggle("is-active", button.dataset.theme === nextTheme);
      });
    }
    if (dom.themeToggle) {
      const activeTheme = THEME_OPTIONS.find((item) => item.id === nextTheme);
      dom.themeToggle.textContent = activeTheme ? `主题 · ${activeTheme.label}` : "主题";
    }
  }

  function restoreTheme() {
    const savedTheme = localStorage.getItem("codexmanager.ui.theme");
    setTheme(savedTheme || "tech");
  }

  function closeThemePanel() {
    if (!dom.themePanel || !dom.themeToggle) return;
    dom.themePanel.hidden = true;
    dom.themeToggle.setAttribute("aria-expanded", "false");
  }

  function openThemePanel() {
    if (!dom.themePanel || !dom.themeToggle) return;
    dom.themePanel.hidden = false;
    dom.themeToggle.setAttribute("aria-expanded", "true");
  }

  function toggleThemePanel() {
    if (!dom.themePanel) return;
    if (dom.themePanel.hidden) {
      openThemePanel();
    } else {
      closeThemePanel();
    }
  }

  return {
    renderThemeButtons,
    setTheme,
    restoreTheme,
    closeThemePanel,
    toggleThemePanel,
  };
}
