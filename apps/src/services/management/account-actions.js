import * as api from "../../api.js";
import { calcAvailability } from "../../utils/format.js";

const EMPTY_REFRESH_PROGRESS = Object.freeze({
  active: false,
  manual: false,
  completed: 0,
  total: 0,
  remaining: 0,
  lastTaskLabel: "",
});

let refreshAllProgress = { ...EMPTY_REFRESH_PROGRESS };

function nextPaintTick() {
  return new Promise((resolve) => {
    const raf = typeof globalThis !== "undefined" ? globalThis.requestAnimationFrame : null;
    if (typeof raf === "function") {
      raf(() => resolve());
      return;
    }
    setTimeout(resolve, 0);
  });
}

function normalizeProgress(next) {
  const total = Math.max(0, Number(next?.total || 0));
  const completed = Math.min(total, Math.max(0, Number(next?.completed || 0)));
  return {
    active: Boolean(next?.active) && total > 0,
    manual: Boolean(next?.manual),
    total,
    completed,
    remaining: Math.max(0, total - completed),
    lastTaskLabel: String(next?.lastTaskLabel || "").trim(),
  };
}

export function setRefreshAllProgress(progress) {
  refreshAllProgress = normalizeProgress(progress);
  return { ...refreshAllProgress };
}

export function clearRefreshAllProgress() {
  refreshAllProgress = { ...EMPTY_REFRESH_PROGRESS };
  return { ...refreshAllProgress };
}

export function getRefreshAllProgress() {
  return { ...refreshAllProgress };
}

export function createAccountActions({
  state,
  ensureConnected,
  refreshAccountsAndUsage,
  renderAccountsView,
  renderCurrentPageView,
  showToast,
  showConfirmDialog,
}) {
  let accountOpsQueue = Promise.resolve();
  let refreshSectionInFlight = null;

  function enqueueAccountOp(task) {
    const run = accountOpsQueue.then(task, task);
    accountOpsQueue = run.catch(() => {});
    return run;
  }

  const refreshAccountsSection = async () => {
    if (refreshSectionInFlight) {
      return refreshSectionInFlight;
    }
    refreshSectionInFlight = (async () => {
      const ok = await refreshAccountsAndUsage();
      if (!ok) {
        showToast("账号数据刷新失败，请稍后重试", "error");
        return false;
      }
      renderAccountsView();
      return true;
    })();
    try {
      return await refreshSectionInFlight;
    } finally {
      refreshSectionInFlight = null;
    }
  };

  async function updateAccountSort(accountId, sort, previousSort) {
    if (Number.isFinite(previousSort) && previousSort === sort) {
      return;
    }
    const ok = await ensureConnected();
    if (!ok) return;
    const res = await api.serviceAccountUpdate(accountId, sort);
    if (res && res.ok === false) {
      showToast(res.error || "排序更新失败", "error");
      return;
    }
    const refreshed = await refreshAccountsAndUsage({ includeUsage: false });
    if (!refreshed) {
      showToast("账号排序已更新，但列表刷新失败，请稍后重试", "error");
      return;
    }
    renderAccountsView();
  }

  async function deleteAccount(account) {
    if (!account || !account.id) return;
    const confirmed = await showConfirmDialog({
      title: "删除账号",
      message: `确定删除账号 ${account.label} 吗？删除后不可恢复。`,
      confirmText: "删除",
      cancelText: "取消",
    });
    if (!confirmed) return;
    await enqueueAccountOp(async () => {
      const ok = await ensureConnected();
      if (!ok) return;
      const res = await api.serviceAccountDelete(account.id);
      if (res && res.error === "unknown_method") {
        const fallback = await api.localAccountDelete(account.id);
        if (fallback && fallback.ok) {
          await refreshAccountsSection();
          return;
        }
        const msg = fallback && fallback.error ? fallback.error : "删除失败";
        showToast(msg, "error");
        return;
      }
      if (res && res.ok) {
        await refreshAccountsSection();
        showToast("账号已删除");
      } else {
        const msg = res && res.error ? res.error : "删除失败";
        showToast(msg, "error");
      }
    });
  }

  async function setManualPreferredAccount(account) {
    if (!account || !account.id) return;
    const ok = await ensureConnected();
    if (!ok) return;
    await enqueueAccountOp(async () => {
      const usageList = Array.isArray(state?.usageList) ? state.usageList : [];
      const usage = usageList.find((item) => item && item.accountId === account.id) || null;
      const status = calcAvailability(usage);
      if (status.level === "warn" || status.level === "bad") {
        showToast(`账号当前不可用（${status.text}），无法锁定`, "error");
        return;
      }
      const res = await api.serviceGatewayManualAccountSet(account.id);
      if (res && res.ok === false) {
        showToast(res.error || "锁定当前账号失败", "error");
        return;
      }
      if (state && typeof state === "object") {
        state.manualPreferredAccountId = account.id;
      }
      showToast(`已锁定 ${account.label || account.id}，异常前将持续优先使用`);
      renderAccountsView?.();
      renderCurrentPageView?.();
    });
  }

  async function importAccountsFromFiles(fileList) {
    const files = Array.from(fileList || []);
    if (!files.length) return;
    const ok = await ensureConnected();
    if (!ok) return;

    // 中文注释：多文件/大文件读取时，避免 Promise.all 同时触发所有 file.text() 导致 UI 抖动或卡顿。
    // 这里改为顺序读取，并在关键阶段让出一次绘制机会。
    const totalBytes = files.reduce((sum, file) => sum + Math.max(0, Number(file?.size || 0)), 0);
    const shouldShowProgressToast = files.length > 1 || totalBytes >= 2 * 1024 * 1024;
    if (shouldShowProgressToast) {
      showToast(`正在读取并导入账号（${files.length} 个文件）...`);
    }
    await nextPaintTick();

    const contents = [];
    const yieldEvery = files.length > 6 || totalBytes >= 8 * 1024 * 1024 ? 1 : 2;
    for (let index = 0; index < files.length; index += 1) {
      const file = files[index];
      let text = "";
      try {
        if (file && typeof file.text === "function") {
          text = await file.text();
        }
      } catch {
        text = "";
      }
      const trimmed = String(text || "").trim();
      if (trimmed) {
        contents.push(trimmed);
      }
      if ((index + 1) % yieldEvery === 0) {
        await nextPaintTick();
      }
    }

    await nextPaintTick();

    if (!contents.length) {
      showToast("未读取到可导入内容", "error");
      return;
    }

    await enqueueAccountOp(async () => {
      await nextPaintTick();
      const res = await api.serviceAccountImport(contents);
      if (res && res.error) {
        showToast(res.error || "导入失败", "error");
        return;
      }
      const total = Number(res?.total || 0);
      const created = Number(res?.created || 0);
      const updated = Number(res?.updated || 0);
      const failed = Number(res?.failed || 0);

      await refreshAccountsSection();
      showToast(`导入完成：共${total}，新增${created}，更新${updated}，失败${failed}`);
      await nextPaintTick();
      if (failed > 0 && Array.isArray(res?.errors) && res.errors.length > 0) {
        const first = res.errors[0];
        const index = Number(first?.index || 0);
        const message = String(first?.message || "unknown error");
        showToast(`首个失败项 #${index}: ${message}`, "error");
      }
    });
  }

  return { updateAccountSort, deleteAccount, importAccountsFromFiles, setManualPreferredAccount };
}
