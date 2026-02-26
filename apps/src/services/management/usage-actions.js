import * as api from "../../api";

export function createUsageActions({
  dom,
  state,
  ensureConnected,
  openUsageModal,
  renderUsageSnapshot,
  renderAccountsView,
}) {
  async function handleOpenUsageModal(account) {
    openUsageModal(account);
    await refreshUsageForAccount();
  }

  async function refreshUsageForAccount() {
    if (!state.currentUsageAccount) return;
    const ok = await ensureConnected();
    if (!ok) return;
    dom.refreshUsageSingle.disabled = true;
    try {
      await api.serviceUsageRefresh(state.currentUsageAccount.id);
      const res = await api.serviceUsageRead(state.currentUsageAccount.id);
      const snap = res ? res.snapshot : null;
      if (snap) {
        const accountId = state.currentUsageAccount.id;
        const next = Array.isArray(state.usageList) ? [...state.usageList] : [];
        const idx = next.findIndex((item) => item && item.accountId === accountId);
        const normalized = { ...snap, accountId };
        if (idx >= 0) {
          next[idx] = normalized;
        } else {
          next.push(normalized);
        }
        state.usageList = next;
        renderAccountsView?.();
      }
      renderUsageSnapshot(snap);
    } catch (err) {
      dom.usageDetail.textContent = String(err);
    }
    dom.refreshUsageSingle.disabled = false;
  }

  return { handleOpenUsageModal, refreshUsageForAccount };
}
