export async function withButtonBusy(button, busyText, task) {
  if (!button) {
    return task();
  }
  if (button.dataset.busy === "1") {
    return;
  }
  const originalText = button.textContent;
  const originalMinWidth = button.style.minWidth;
  const currentWidth = Math.ceil(button.getBoundingClientRect().width || 0);
  if (currentWidth > 0) {
    // 锁定按钮最小宽度，避免 loading 文案变化导致布局抖动。
    button.style.minWidth = `${currentWidth}px`;
  }
  button.dataset.busy = "1";
  button.disabled = true;
  button.classList.add("is-loading");
  if (busyText) {
    button.textContent = busyText;
  }
  try {
    return await task();
  } finally {
    button.dataset.busy = "0";
    button.disabled = false;
    button.classList.remove("is-loading");
    button.textContent = originalText;
    button.style.minWidth = originalMinWidth;
  }
}
