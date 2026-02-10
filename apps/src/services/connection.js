import { state } from "../state.js";
import * as api from "../api.js";
import { setStatus, setServiceHint } from "../ui/status.js";

// 规范化端口/地址输入
export function normalizeAddr(raw) {
  const trimmed = String(raw || "").trim();
  if (!trimmed) {
    throw new Error("请输入端口或地址");
  }
  let value = trimmed;
  if (value.startsWith("http://")) {
    value = value.slice("http://".length);
  }
  if (value.startsWith("https://")) {
    value = value.slice("https://".length);
  }
  value = value.split("/")[0];
  if (!value.includes(":")) {
    value = `localhost:${value}`;
  }
  const [host, port] = value.split(":");
  if (!port) return value;
  if (host === "127.0.0.1" || host === "0.0.0.0") {
    return `localhost:${port}`;
  }
  return value;
}

function formatConnectError(err) {
  const raw = err && typeof err === "object" && "message" in err ? err.message : String(err);
  const text = String(raw || "").trim();
  if (!text) return "unknown";
  const firstLine = text.split("\n")[0].trim();
  const lower = firstLine.toLowerCase();
  if (lower.includes("timed out")) return "连接超时";
  if (lower.includes("connection refused") || lower.includes("actively refused")) return "连接被拒绝";
  if (lower.includes("empty response")) return "空响应";
  if (lower.includes("invalid service address")) return "地址不合法";
  return firstLine.length > 120 ? `${firstLine.slice(0, 120)}...` : firstLine;
}

function readServerNameFromInitialize(res) {
  if (!res || typeof res !== "object") return "";
  if (typeof res.server_name === "string") return res.server_name;
  // 中文注释：Tauri 命令返回的是 JSON-RPC 包装结构，server_name 在 result 内层。
  const nested = res.result;
  if (nested && typeof nested === "object" && typeof nested.server_name === "string") {
    return nested.server_name;
  }
  return "";
}

function isExpectedInitializeResult(res) {
  return readServerNameFromInitialize(res) === "gpttools-service";
}

// 初始化连接（不负责启动 service）
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export function createConnectionService(deps) {
  const {
    api: apiClient = api,
    state: stateRef = state,
    setStatus: setStatusFn = setStatus,
    setServiceHint: setServiceHintFn = setServiceHint,
    wait = sleep,
  } = deps || {};

  async function initializeService(options = {}) {
    const {
      retries = 0,
      delayMs = 300,
      silent = false,
      wait: waitFn = wait,
    } = options;
    setStatusFn("连接中...", false);
    setServiceHintFn("", false);

    let lastError = null;
    for (let attempt = 0; attempt <= retries; attempt += 1) {
      try {
        const res = await apiClient.serviceInitialize();
        if (!isExpectedInitializeResult(res)) {
          const serverName = readServerNameFromInitialize(res);
          const hint = serverName ? `server_name=${serverName}` : "响应不匹配";
          throw new Error(`端口可能被其他服务占用（${hint}）`);
        }
        stateRef.serviceConnected = true;
        stateRef.serviceLastError = "";
        stateRef.serviceLastErrorAt = 0;
        setStatusFn("", true);
        setServiceHintFn("", false);
        return true;
      } catch (err) {
        lastError = err;
        stateRef.serviceConnected = false;
        stateRef.serviceLastError = formatConnectError(err);
        stateRef.serviceLastErrorAt = Date.now();
        if (silent && attempt < retries) {
          setStatusFn(`连接中...（重试 ${attempt + 1}/${retries + 1}）`, false);
          setServiceHintFn(`正在重试：${stateRef.serviceLastError}`, false);
        }
        if (attempt < retries) {
          await waitFn(delayMs);
        }
      }
    }

    setStatusFn("", false);
    if (!silent) {
      const reason = stateRef.serviceLastError ? `：${stateRef.serviceLastError}` : "";
      setServiceHintFn(`连接失败${reason}，请检查端口或 service 状态`, true);
    }
    if (lastError) {
      return false;
    }
    return false;
  }

  async function ensureConnected() {
    if (stateRef.serviceConnected) return true;
    return initializeService({ retries: 1, delayMs: 200 });
  }

  async function startService(rawAddr, options = {}) {
    const addr = normalizeAddr(rawAddr);
    stateRef.serviceAddr = addr;
    setServiceHintFn("", false);
    setStatusFn("启动中...", false);
    try {
      await apiClient.serviceStart(addr);
    } catch (err) {
      setStatusFn("", false);
      setServiceHintFn(`启动失败：${String(err)}`, true);
      return false;
    }
    const {
      retries = 8,
      delayMs = 400,
      silent = false,
      wait: waitFn,
      skipInitialize = false,
    } = options;
    if (skipInitialize) {
      return true;
    }
    return initializeService({ retries, delayMs, wait: waitFn, silent });
  }

  async function stopService() {
    setStatusFn("停止中...", false);
    try {
      await apiClient.serviceStop();
    } catch (err) {
      setServiceHintFn(`停止失败：${String(err)}`, true);
    }
    stateRef.serviceConnected = false;
    setStatusFn("", false);
  }

  return {
    initializeService,
    ensureConnected,
    startService,
    stopService,
    waitForConnection: (options = {}) => {
      const {
        retries = 8,
        delayMs = 400,
        silent = true,
        wait: waitFn,
      } = options;
      return initializeService({ retries, delayMs, silent, wait: waitFn });
    },
  };
}

// 确保已连接
const defaultService = createConnectionService();

export const initializeService = defaultService.initializeService;
export const ensureConnected = defaultService.ensureConnected;
export const startService = defaultService.startService;
export const stopService = defaultService.stopService;
export const waitForConnection = defaultService.waitForConnection;
