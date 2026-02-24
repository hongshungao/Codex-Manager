const DEFAULT_AUTO_REFRESH_INTERVAL_MS = 30000;

export async function runRefreshTasks(tasks, onTaskError) {
  const taskList = Array.isArray(tasks) ? tasks : [];
  const settled = await Promise.allSettled(
    taskList.map((item) => Promise.resolve().then(() => item.run())),
  );

  return settled.map((result, index) => {
    const taskName = taskList[index] && taskList[index].name ? taskList[index].name : `task-${index}`;
    if (result.status === "rejected" && typeof onTaskError === "function") {
      onTaskError(taskName, result.reason);
    }
    return {
      name: taskName,
      ...result,
    };
  });
}

export function ensureAutoRefreshTimer(stateRef, onTick, intervalMs = DEFAULT_AUTO_REFRESH_INTERVAL_MS) {
  if (!stateRef || typeof onTick !== "function") {
    return false;
  }
  if (stateRef.autoRefreshTimer) {
    return false;
  }
  let tickInFlight = null;
  // 中文注释：统一从这里创建定时器，避免启动链路多个入口重复 setInterval 导致刷新风暴。
  stateRef.autoRefreshTimer = setInterval(() => {
    if (tickInFlight) return;
    tickInFlight = Promise.resolve()
      .then(() => onTick())
      .finally(() => {
        tickInFlight = null;
      });
  }, intervalMs);
  return true;
}

export function stopAutoRefreshTimer(stateRef) {
  if (!stateRef || !stateRef.autoRefreshTimer) {
    return false;
  }
  clearInterval(stateRef.autoRefreshTimer);
  stateRef.autoRefreshTimer = null;
  return true;
}

