const DEFAULT_AUTO_REFRESH_INTERVAL_MS = 60000;
const DEFAULT_REFRESH_TASK_CONCURRENCY = 3;

function normalizeConcurrency(value, fallback) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  const int = Math.floor(parsed);
  if (int <= 0) return fallback;
  return Math.min(8, int);
}

export async function runRefreshTasks(tasks, onTaskError, options = {}) {
  const taskList = Array.isArray(tasks) ? tasks : [];
  const concurrency = Math.min(
    taskList.length,
    normalizeConcurrency(options.concurrency, DEFAULT_REFRESH_TASK_CONCURRENCY),
  );

  const results = new Array(taskList.length);
  if (taskList.length === 0) {
    return results;
  }

  let nextIndex = 0;

  async function runOne(index) {
    const item = taskList[index];
    try {
      const value = await Promise.resolve().then(() => item.run());
      results[index] = { status: "fulfilled", value };
    } catch (reason) {
      results[index] = { status: "rejected", reason };
    }
  }

  const workers = [];
  const workerCount = Math.max(1, concurrency);
  for (let i = 0; i < workerCount; i += 1) {
    workers.push((async () => {
      while (nextIndex < taskList.length) {
        const current = nextIndex;
        nextIndex += 1;
        await runOne(current);
      }
    })());
  }
  await Promise.all(workers);

  return results.map((result, index) => {
    const taskName =
      taskList[index] && taskList[index].name
        ? taskList[index].name
        : `task-${index}`;
    if (result && result.status === "rejected" && typeof onTaskError === "function") {
      onTaskError(taskName, result.reason);
    }
    return {
      name: taskName,
      ...(result || { status: "rejected", reason: new Error("unknown task result") }),
    };
  });
}

export function ensureAutoRefreshTimer(
  stateRef,
  onTick,
  intervalMs = DEFAULT_AUTO_REFRESH_INTERVAL_MS,
) {
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
