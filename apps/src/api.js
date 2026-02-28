import { state } from "./state.js";
import { fetchWithRetry, isAbortError, runWithControl } from "./utils/request.js";

function isTauriRuntime() {
  const tauri = globalThis.window && window.__TAURI__;
  return Boolean(tauri && tauri.core && tauri.core.invoke);
}

// 统一 Tauri 调用入口
export async function invoke(method, params, options = {}) {
  const tauri = globalThis.window && window.__TAURI__;
  if (!tauri || !tauri.core || !tauri.core.invoke) {
    throw new Error("Tauri API 不可用（请在桌面端运行）");
  }
  const invokeOptions = options && typeof options === "object" ? options : {};
  const res = await runWithControl(
    () => tauri.core.invoke(method, params || {}),
    {
      signal: invokeOptions.signal,
      timeoutMs: invokeOptions.timeoutMs,
      retries: invokeOptions.retries,
      retryDelayMs: invokeOptions.retryDelayMs,
      maxRetryDelayMs: invokeOptions.maxRetryDelayMs,
      shouldRetry: invokeOptions.shouldRetry,
    },
  );
  // 中文注释：统一把 JSON-RPC error 转成异常，避免调用方把失败误判成成功。
  if (res && typeof res === "object" && Object.prototype.hasOwnProperty.call(res, "error")) {
    const err = res.error;
    if (typeof err === "string" && err.trim()) {
      throw new Error(err);
    }
    if (err && typeof err === "object" && typeof err.message === "string" && err.message.trim()) {
      throw new Error(err.message);
    }
    try {
      throw new Error(JSON.stringify(err));
    } catch {
      throw new Error("RPC 调用失败");
    }
  }

  const throwIfBusinessError = (payload) => {
    if (!payload || typeof payload !== "object") return;
    // 业务约定：ok=false + error 代表本次动作失败（如 usage refresh）。
    if (payload.ok === false) {
      const msg = typeof payload.error === "string" && payload.error.trim()
        ? payload.error
        : "操作失败";
      throw new Error(msg);
    }
    // 兼容 value_or_error: 仅包含 error 字段时视为失败。
    if (
      typeof payload.error === "string"
      && payload.error.trim()
      && Object.keys(payload).length === 1
    ) {
      throw new Error(payload.error);
    }
  };

  if (res && Object.prototype.hasOwnProperty.call(res, "result")) {
    const payload = res.result;
    throwIfBusinessError(payload);
    return payload;
  }
  throwIfBusinessError(res);
  return res;
}

function isCommandMissingError(err) {
  const msg = String(err && err.message ? err.message : err).toLowerCase();
  if (
    msg.includes("not found")
    || msg.includes("unknown command")
    || msg.includes("no such command")
    || msg.includes("not managed")
    || msg.includes("does not exist")
  ) {
    return true;
  }
  return msg.includes("invalid args") && msg.includes("for command");
}

let rpcRequestId = 1;
let rpcTokenCache = "";

async function rpcInvoke(method, params, options = {}) {
  const opts = options && typeof options === "object" ? options : {};
  const signal = opts.signal;
  const timeoutMs = opts.timeoutMs == null ? 8000 : opts.timeoutMs;
  const retries = opts.retries == null ? 0 : opts.retries;
  const retryDelayMs = opts.retryDelayMs == null ? 180 : opts.retryDelayMs;
  const maxRetryDelayMs = opts.maxRetryDelayMs == null ? 1200 : opts.maxRetryDelayMs;
  const body = JSON.stringify({
    jsonrpc: "2.0",
    id: rpcRequestId++,
    method,
    params: params == null ? undefined : params,
  });
  const response = await fetchWithRetry(
    "/api/rpc",
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body,
    },
    {
      signal,
      timeoutMs,
      retries,
      retryDelayMs,
      maxRetryDelayMs,
      shouldRetry: () => true,
      shouldRetryStatus: (status) => status === 429 || (status >= 500 && status < 600),
    },
  );
  if (!response.ok) {
    throw new Error(`RPC HTTP ${response.status}`);
  }
  const payload = await response.json();
  const rpcError = unwrapRpcError(payload);
  if (rpcError) {
    throw new Error(rpcError);
  }
  if (payload && Object.prototype.hasOwnProperty.call(payload, "result")) {
    const result = payload.result;
    if (result && typeof result === "object" && result.ok === false) {
      const msg = typeof result.error === "string" && result.error.trim()
        ? result.error
        : "操作失败";
      throw new Error(msg);
    }
    return result;
  }
  return payload;
}

function resolveRpcAddr() {
  const raw = String(state.serviceAddr || "").trim();
  if (raw) {
    return raw;
  }
  return "localhost:48760";
}

function unwrapRpcError(payload) {
  const err = payload && typeof payload === "object" ? payload.error : null;
  if (!err) return "";
  if (typeof err === "string") return err;
  if (typeof err.message === "string" && err.message.trim()) {
    return err.message;
  }
  return JSON.stringify(err);
}

async function getRpcToken(options = {}) {
  if (rpcTokenCache) {
    return rpcTokenCache;
  }
  const opts = options && typeof options === "object" ? options : {};
  const token = await invoke("service_rpc_token", {}, {
    signal: opts.signal,
    timeoutMs: opts.timeoutMs == null ? 2500 : opts.timeoutMs,
    retries: opts.retries,
    retryDelayMs: opts.retryDelayMs,
    maxRetryDelayMs: opts.maxRetryDelayMs,
    shouldRetry: opts.shouldRetry,
  });
  const normalized = String(token || "").trim();
  if (!normalized) {
    throw new Error("RPC token unavailable");
  }
  rpcTokenCache = normalized;
  return rpcTokenCache;
}

async function requestlogListViaHttpRpc(query, limit, options = {}) {
  const signal = options && options.signal ? options.signal : undefined;
  const timeoutMs = options && Number.isFinite(options.timeoutMs) ? options.timeoutMs : 8000;
  const retries = options && Number.isFinite(options.retries) ? options.retries : 1;
  const retryDelayMs = options && Number.isFinite(options.retryDelayMs) ? options.retryDelayMs : 160;
  const addr = resolveRpcAddr();
  const token = await getRpcToken({
    signal,
    timeoutMs: Math.min(2500, timeoutMs),
  });
  const body = JSON.stringify({
    jsonrpc: "2.0",
    id: rpcRequestId++,
    method: "requestlog/list",
    params: { query, limit },
  });
  const response = await fetchWithRetry(
    `http://${addr}/rpc`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-CodexManager-Rpc-Token": token,
      },
      body,
    },
    {
      signal,
      timeoutMs,
      retries,
      retryDelayMs,
      maxRetryDelayMs: 1200,
      shouldRetry: () => true,
      shouldRetryStatus: (status) => status === 429 || (status >= 500 && status < 600),
    },
  );
  if (!response.ok) {
    throw new Error(`RPC HTTP ${response.status}`);
  }
  const payload = await response.json();
  const rpcError = unwrapRpcError(payload);
  if (rpcError) {
    throw new Error(rpcError);
  }
  if (payload && Object.prototype.hasOwnProperty.call(payload, "result")) {
    return payload.result;
  }
  return payload;
}

async function invokeFirst(methods, params) {
  let lastErr = null;
  for (const method of methods) {
    try {
      return await invoke(method, params);
    } catch (err) {
      lastErr = err;
      if (!isCommandMissingError(err)) {
        throw err;
      }
    }
  }
  if (lastErr) {
    throw lastErr;
  }
  throw new Error("未配置可用命令");
}

function withAddr(extra) {
  return {
    addr: state.serviceAddr || null,
    ...(extra || {}),
  };
}

// service 生命周期
export async function serviceStart(addr) {
  if (!isTauriRuntime()) {
    throw new Error("浏览器模式不支持启动/停止 service，请手动启动 codexmanager-service");
  }
  return invoke("service_start", { addr });
}

export async function serviceStop() {
  if (!isTauriRuntime()) {
    throw new Error("浏览器模式不支持启动/停止 service，请手动停止 codexmanager-service");
  }
  return invoke("service_stop", {});
}

export async function serviceInitialize() {
  if (!isTauriRuntime()) {
    return rpcInvoke("initialize");
  }
  return invoke("service_initialize", withAddr());
}

// 账号
export async function serviceAccountList() {
  if (!isTauriRuntime()) {
    return rpcInvoke("account/list");
  }
  return invoke("service_account_list", withAddr());
}

export async function serviceAccountDelete(accountId) {
  if (!isTauriRuntime()) {
    return rpcInvoke("account/delete", { accountId });
  }
  return invoke("service_account_delete", withAddr({ accountId }));
}

export async function serviceAccountUpdate(accountId, sort) {
  if (!isTauriRuntime()) {
    return rpcInvoke("account/update", { accountId, sort });
  }
  return invoke("service_account_update", withAddr({ accountId, sort }));
}

export async function serviceAccountImport(contents) {
  if (!isTauriRuntime()) {
    return rpcInvoke("account/import", { contents });
  }
  return invoke("service_account_import", withAddr({ contents }));
}

export async function localAccountDelete(accountId) {
  if (!isTauriRuntime()) {
    return { ok: false, error: "浏览器模式不支持本地删除（请升级 service 或使用桌面端）" };
  }
  return invoke("local_account_delete", { accountId });
}

// 用量
export async function serviceUsageRead(accountId) {
  if (!isTauriRuntime()) {
    return rpcInvoke("account/usage/read", accountId ? { accountId } : undefined);
  }
  return invoke("service_usage_read", withAddr({ accountId }));
}

export async function serviceUsageList() {
  if (!isTauriRuntime()) {
    return rpcInvoke("account/usage/list");
  }
  return invoke("service_usage_list", withAddr());
}

export async function serviceUsageRefresh(accountId) {
  if (!isTauriRuntime()) {
    return rpcInvoke("account/usage/refresh", accountId ? { accountId } : undefined);
  }
  return invoke("service_usage_refresh", withAddr({ accountId }));
}

export async function serviceRequestLogList(query, limit, options = {}) {
  const signal = options && options.signal ? options.signal : undefined;
  if (signal && isTauriRuntime()) {
    try {
      return await requestlogListViaHttpRpc(query, limit, {
        signal,
        timeoutMs: options.timeoutMs,
        retries: options.retries,
        retryDelayMs: options.retryDelayMs,
      });
    } catch (err) {
      if (isAbortError(err)) {
        throw err;
      }
      rpcTokenCache = "";
    }
  }
  if (!isTauriRuntime()) {
    return rpcInvoke("requestlog/list", { query, limit }, options);
  }
  return invoke("service_requestlog_list", withAddr({ query, limit }));
}

export async function serviceRequestLogClear() {
  if (!isTauriRuntime()) {
    return rpcInvoke("requestlog/clear");
  }
  return invoke("service_requestlog_clear", withAddr());
}

export async function serviceRequestLogTodaySummary() {
  if (!isTauriRuntime()) {
    return rpcInvoke("requestlog/today_summary");
  }
  return invoke("service_requestlog_today_summary", withAddr());
}

export async function serviceGatewayRouteStrategyGet() {
  if (!isTauriRuntime()) {
    return rpcInvoke("gateway/routeStrategy/get");
  }
  return invoke("service_gateway_route_strategy_get", withAddr());
}

export async function serviceGatewayRouteStrategySet(strategy) {
  if (!isTauriRuntime()) {
    return rpcInvoke("gateway/routeStrategy/set", { strategy });
  }
  return invoke("service_gateway_route_strategy_set", withAddr({ strategy }));
}

export async function serviceGatewayManualAccountGet() {
  if (!isTauriRuntime()) {
    return rpcInvoke("gateway/manualAccount/get");
  }
  return invoke("service_gateway_manual_account_get", withAddr());
}

export async function serviceGatewayManualAccountSet(accountId) {
  if (!isTauriRuntime()) {
    return rpcInvoke("gateway/manualAccount/set", { accountId });
  }
  return invoke("service_gateway_manual_account_set", withAddr({ accountId }));
}

export async function serviceGatewayManualAccountClear() {
  if (!isTauriRuntime()) {
    return rpcInvoke("gateway/manualAccount/clear");
  }
  return invoke("service_gateway_manual_account_clear", withAddr());
}

// 登录
export async function serviceLoginStart(payload) {
  if (!isTauriRuntime()) {
    const safe = payload && typeof payload === "object" ? payload : {};
    return rpcInvoke("account/login/start", {
      type: safe.loginType || safe.type || "chatgpt",
      openBrowser: safe.openBrowser !== false,
      note: safe.note || null,
      tags: safe.tags || null,
      groupName: safe.groupName || null,
      workspaceId: safe.workspaceId || null,
    });
  }
  return invoke("service_login_start", withAddr(payload));
}

export async function serviceLoginStatus(loginId) {
  if (!isTauriRuntime()) {
    return rpcInvoke("account/login/status", { loginId });
  }
  return invoke("service_login_status", withAddr({ loginId }));
}

export async function serviceLoginComplete(state, code, redirectUri) {
  if (!isTauriRuntime()) {
    return rpcInvoke("account/login/complete", { state, code, redirectUri });
  }
  return invoke("service_login_complete", withAddr({ state, code, redirectUri }));
}

// API Key
export async function serviceApiKeyList() {
  if (!isTauriRuntime()) {
    return rpcInvoke("apikey/list");
  }
  return invoke("service_apikey_list", withAddr());
}

export async function serviceApiKeyReadSecret(keyId) {
  if (!isTauriRuntime()) {
    return rpcInvoke("apikey/readSecret", { id: keyId });
  }
  return invoke("service_apikey_read_secret", withAddr({ keyId }));
}

export async function serviceApiKeyCreate(name, modelSlug, reasoningEffort, profile = {}) {
  const params = {
    name,
    modelSlug,
    reasoningEffort,
    protocolType: profile.protocolType || null,
    upstreamBaseUrl: profile.upstreamBaseUrl || null,
    staticHeadersJson: profile.staticHeadersJson || null,
  };
  if (!isTauriRuntime()) {
    return rpcInvoke("apikey/create", params);
  }
  return invoke("service_apikey_create", withAddr(params));
}

export async function serviceApiKeyModels(options = {}) {
  const refreshRemote = options && options.refreshRemote === true;
  if (!isTauriRuntime()) {
    return rpcInvoke("apikey/models", refreshRemote ? { refreshRemote } : undefined);
  }
  return invoke("service_apikey_models", withAddr({ refreshRemote }));
}

export async function serviceApiKeyUpdateModel(keyId, modelSlug, reasoningEffort, profile = {}) {
  const params = {
    id: keyId,
    modelSlug,
    reasoningEffort,
    protocolType: profile.protocolType || null,
    upstreamBaseUrl: profile.upstreamBaseUrl || null,
    staticHeadersJson: profile.staticHeadersJson || null,
  };
  if (!isTauriRuntime()) {
    return rpcInvoke("apikey/updateModel", params);
  }
  // 兼容桌面端的 tauri command 参数名
  return invoke("service_apikey_update_model", withAddr({
    keyId,
    modelSlug,
    reasoningEffort,
    protocolType: profile.protocolType || null,
    upstreamBaseUrl: profile.upstreamBaseUrl || null,
    staticHeadersJson: profile.staticHeadersJson || null,
  }));
}

export async function serviceApiKeyDelete(keyId) {
  if (!isTauriRuntime()) {
    return rpcInvoke("apikey/delete", { id: keyId });
  }
  return invoke("service_apikey_delete", withAddr({ keyId }));
}

export async function serviceApiKeyDisable(keyId) {
  if (!isTauriRuntime()) {
    return rpcInvoke("apikey/disable", { id: keyId });
  }
  return invoke("service_apikey_disable", withAddr({ keyId }));
}

export async function serviceApiKeyEnable(keyId) {
  if (!isTauriRuntime()) {
    return rpcInvoke("apikey/enable", { id: keyId });
  }
  return invoke("service_apikey_enable", withAddr({ keyId }));
}

// 打开浏览器
export async function openInBrowser(url) {
  if (!isTauriRuntime()) {
    try {
      window.open(url, "_blank", "noopener,noreferrer");
      return { ok: true };
    } catch {
      return { ok: false };
    }
  }
  return invoke("open_in_browser", { url });
}

// 应用更新
export async function updateCheck() {
  if (!isTauriRuntime()) {
    throw new Error("浏览器模式不支持桌面端更新");
  }
  return invokeFirst(["app_update_check", "update_check", "check_update"], {});
}

export async function updateDownload(payload = {}) {
  if (!isTauriRuntime()) {
    throw new Error("浏览器模式不支持桌面端更新");
  }
  return invokeFirst(["app_update_prepare", "update_download", "download_update"], payload);
}

export async function updateInstall(payload = {}) {
  if (!isTauriRuntime()) {
    throw new Error("浏览器模式不支持桌面端更新");
  }
  return invokeFirst(["app_update_launch_installer", "update_install", "install_update"], payload);
}

export async function updateRestart(payload = {}) {
  if (!isTauriRuntime()) {
    throw new Error("浏览器模式不支持桌面端更新");
  }
  return invokeFirst(["app_update_apply_portable", "update_restart", "restart_update"], payload);
}

export async function updateStatus() {
  if (!isTauriRuntime()) {
    throw new Error("浏览器模式不支持桌面端更新");
  }
  return invokeFirst(["app_update_status", "update_status"], {});
}
