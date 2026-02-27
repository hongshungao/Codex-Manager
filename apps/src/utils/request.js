function createAbortError(message) {
  const msg = String(message || "The operation was aborted.");
  try {
    return new DOMException(msg, "AbortError");
  } catch {
    const err = new Error(msg);
    err.name = "AbortError";
    return err;
  }
}

function isAbortError(err) {
  if (!err) return false;
  if (err && typeof err === "object" && err.name === "AbortError") {
    return true;
  }
  return false;
}

function createTimeoutError(timeoutMs) {
  const safe = Math.max(0, Math.floor(Number(timeoutMs) || 0));
  const err = new Error(`请求超时（${safe}ms）`);
  err.name = "TimeoutError";
  return err;
}

function computeBackoffDelay(attempt, baseDelayMs, maxDelayMs) {
  const base = Math.max(0, Math.floor(Number(baseDelayMs) || 0));
  const max = Math.max(base, Math.floor(Number(maxDelayMs) || base));
  if (base <= 0) return 0;
  const exp = Math.min(10, Math.max(0, Math.floor(attempt)));
  return Math.min(max, base * (2 ** exp));
}

function createAbortPromise(signal) {
  if (!signal) {
    return { promise: null, cleanup: () => {} };
  }
  if (signal.aborted) {
    return {
      promise: Promise.reject(createAbortError()),
      cleanup: () => {},
    };
  }
  let handler = null;
  const promise = new Promise((_, reject) => {
    handler = () => reject(createAbortError());
    signal.addEventListener("abort", handler, { once: true });
  });
  return {
    promise,
    cleanup: () => {
      if (handler) {
        signal.removeEventListener("abort", handler);
      }
    },
  };
}

function createTimeoutPromise(timeoutMs) {
  const safe = Math.max(0, Math.floor(Number(timeoutMs) || 0));
  if (!safe) {
    return { promise: null, cleanup: () => {} };
  }
  let timer = null;
  const promise = new Promise((_, reject) => {
    timer = setTimeout(() => reject(createTimeoutError(safe)), safe);
  });
  return {
    promise,
    cleanup: () => {
      if (timer != null) {
        clearTimeout(timer);
      }
    },
  };
}

function createCombinedSignal(signal, timeoutMs) {
  const safeTimeout = Math.max(0, Math.floor(Number(timeoutMs) || 0));
  if (!signal && !safeTimeout) {
    return { signal: undefined, cleanup: () => {} };
  }
  const controller = new AbortController();
  const abort = () => controller.abort();
  let timeout = null;

  if (signal) {
    if (signal.aborted) {
      controller.abort();
    } else {
      signal.addEventListener("abort", abort, { once: true });
    }
  }
  if (safeTimeout) {
    timeout = setTimeout(() => controller.abort(), safeTimeout);
  }

  return {
    signal: controller.signal,
    cleanup: () => {
      if (timeout != null) {
        clearTimeout(timeout);
      }
      if (signal) {
        signal.removeEventListener("abort", abort);
      }
    },
  };
}

async function sleep(ms, signal) {
  const delay = Math.max(0, Math.floor(Number(ms) || 0));
  if (!signal) {
    return new Promise((resolve) => setTimeout(resolve, delay));
  }
  if (signal.aborted) {
    throw createAbortError();
  }
  let timer = null;
  let abortHandler = null;
  try {
    await new Promise((resolve, reject) => {
      timer = setTimeout(resolve, delay);
      abortHandler = () => reject(createAbortError());
      signal.addEventListener("abort", abortHandler, { once: true });
    });
  } finally {
    if (timer != null) {
      clearTimeout(timer);
    }
    if (abortHandler) {
      signal.removeEventListener("abort", abortHandler);
    }
  }
}

async function runWithControl(task, options = {}) {
  if (typeof task !== "function") {
    throw new Error("runWithControl requires task function");
  }
  const signal = options.signal || undefined;
  const timeoutMs = Math.max(0, Math.floor(Number(options.timeoutMs) || 0));
  const retries = Math.max(0, Math.floor(Number(options.retries) || 0));
  const retryDelayMs = Math.max(0, Math.floor(Number(options.retryDelayMs) || 0));
  const maxRetryDelayMs = Math.max(retryDelayMs, Math.floor(Number(options.maxRetryDelayMs) || retryDelayMs));
  const shouldRetry = typeof options.shouldRetry === "function" ? options.shouldRetry : null;

  if (signal && signal.aborted) {
    throw createAbortError();
  }

  for (let attempt = 0; attempt <= retries; attempt += 1) {
    if (signal && signal.aborted) {
      throw createAbortError();
    }
    try {
      const requestPromise = Promise.resolve().then(task);
      // 若被 signal/timeout 提前打断，确保原 promise 的 rejection 不会成为 unhandled。
      requestPromise.catch(() => {});
      const abort = createAbortPromise(signal);
      const timeout = createTimeoutPromise(timeoutMs);
      try {
        const contenders = [requestPromise];
        if (abort.promise) contenders.push(abort.promise);
        if (timeout.promise) contenders.push(timeout.promise);
        return await Promise.race(contenders);
      } finally {
        abort.cleanup();
        timeout.cleanup();
      }
    } catch (err) {
      if (isAbortError(err)) {
        throw err;
      }
      const nextAttempt = attempt + 1;
      const allowRetry = nextAttempt <= retries
        && (!shouldRetry || shouldRetry(err, { attempt, retries }));
      if (!allowRetry) {
        throw err;
      }
      const delay = computeBackoffDelay(attempt, retryDelayMs, maxRetryDelayMs);
      if (delay > 0) {
        await sleep(delay, signal);
      }
    }
  }

  throw new Error("runWithControl exhausted retries unexpectedly");
}

async function fetchWithRetry(url, fetchOptions = {}, options = {}) {
  const fetchFn = options.fetch || globalThis.fetch;
  if (typeof fetchFn !== "function") {
    throw new Error("fetch is not available");
  }
  const signal = options.signal || undefined;
  const timeoutMs = Math.max(0, Math.floor(Number(options.timeoutMs) || 0));
  const retries = Math.max(0, Math.floor(Number(options.retries) || 0));
  const retryDelayMs = Math.max(0, Math.floor(Number(options.retryDelayMs) || 0));
  const maxRetryDelayMs = Math.max(retryDelayMs, Math.floor(Number(options.maxRetryDelayMs) || retryDelayMs));
  const shouldRetry = typeof options.shouldRetry === "function" ? options.shouldRetry : null;
  const shouldRetryStatus =
    typeof options.shouldRetryStatus === "function" ? options.shouldRetryStatus : null;

  if (signal && signal.aborted) {
    throw createAbortError();
  }

  for (let attempt = 0; attempt <= retries; attempt += 1) {
    if (signal && signal.aborted) {
      throw createAbortError();
    }
    const combo = createCombinedSignal(signal, timeoutMs);
    try {
      const response = await fetchFn(url, { ...fetchOptions, signal: combo.signal });
      if (response && typeof response === "object" && typeof response.ok === "boolean") {
        if (
          !response.ok
          && shouldRetryStatus
          && attempt < retries
          && shouldRetryStatus(response.status, { attempt, retries })
        ) {
          const delay = computeBackoffDelay(attempt, retryDelayMs, maxRetryDelayMs);
          if (delay > 0) {
            await sleep(delay, signal);
          }
          continue;
        }
      }
      return response;
    } catch (err) {
      if (isAbortError(err)) {
        throw err;
      }
      const nextAttempt = attempt + 1;
      const allowRetry = nextAttempt <= retries
        && (!shouldRetry || shouldRetry(err, { attempt, retries }));
      if (!allowRetry) {
        throw err;
      }
      const delay = computeBackoffDelay(attempt, retryDelayMs, maxRetryDelayMs);
      if (delay > 0) {
        await sleep(delay, signal);
      }
    } finally {
      combo.cleanup();
    }
  }

  throw new Error("fetchWithRetry exhausted retries unexpectedly");
}

export {
  createAbortError,
  isAbortError,
  runWithControl,
  fetchWithRetry,
  sleep,
};

