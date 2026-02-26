import * as api from "../../api.js";

const EMPTY_REFRESH_PROGRESS = Object.freeze({
  active: false,
  completed: 0,
  total: 0,
  remaining: 0,
  lastTaskLabel: "",
});

let refreshAllProgress = { ...EMPTY_REFRESH_PROGRESS };

function normalizeProgress(next) {
  const total = Math.max(0, Number(next?.total || 0));
  const completed = Math.min(total, Math.max(0, Number(next?.completed || 0)));
  return {
    active: Boolean(next?.active) && total > 0,
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
  ensureConnected,
  refreshAccountsAndUsage,
  renderAccountsView,
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

  async function importAccountsFromFiles(fileList) {
    const files = Array.from(fileList || []);
    if (!files.length) return;
    const ok = await ensureConnected();
    if (!ok) return;

    const contents = (await Promise.all(
      files.map(async (file) => {
        try {
          return await file.text();
        } catch {
          return "";
        }
      }),
    ))
      .map((item) => String(item || "").trim())
      .filter((item) => item.length > 0);

    if (!contents.length) {
      showToast("未读取到可导入内容", "error");
      return;
    }

    await enqueueAccountOp(async () => {
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
      if (failed > 0 && Array.isArray(res?.errors) && res.errors.length > 0) {
        const first = res.errors[0];
        const index = Number(first?.index || 0);
        const message = String(first?.message || "unknown error");
        showToast(`首个失败项 #${index}: ${message}`, "error");
      }
    });
  }

  return { updateAccountSort, deleteAccount, importAccountsFromFiles };
}
