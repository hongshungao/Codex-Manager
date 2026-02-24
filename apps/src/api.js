import { state } from "./state.js";

// 统一 Tauri 调用入口
export async function invoke(method, params) {
  const tauri = window.__TAURI__;
  if (!tauri || !tauri.core || !tauri.core.invoke) {
    throw new Error("Tauri API 不可用（请在桌面端运行）");
  }
  const res = await tauri.core.invoke(method, params || {});
  if (res && Object.prototype.hasOwnProperty.call(res, "result")) {
    return res.result;
  }
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
  return invoke("service_start", { addr });
}

export async function serviceStop() {
  return invoke("service_stop", {});
}

export async function serviceInitialize() {
  return invoke("service_initialize", withAddr());
}

// 账号
export async function serviceAccountList() {
  return invoke("service_account_list", withAddr());
}

export async function serviceAccountDelete(accountId) {
  return invoke("service_account_delete", withAddr({ accountId }));
}

export async function serviceAccountUpdate(accountId, sort) {
  return invoke("service_account_update", withAddr({ accountId, sort }));
}

export async function localAccountDelete(accountId) {
  return invoke("local_account_delete", { accountId });
}

// 用量
export async function serviceUsageRead(accountId) {
  return invoke("service_usage_read", withAddr({ accountId }));
}

export async function serviceUsageList() {
  return invoke("service_usage_list", withAddr());
}

export async function serviceUsageRefresh(accountId) {
  return invoke("service_usage_refresh", withAddr({ accountId }));
}

export async function serviceRequestLogList(query, limit) {
  return invoke("service_requestlog_list", withAddr({ query, limit }));
}

export async function serviceRequestLogClear() {
  return invoke("service_requestlog_clear", withAddr());
}

// 登录
export async function serviceLoginStart(payload) {
  return invoke("service_login_start", withAddr(payload));
}

export async function serviceLoginStatus(loginId) {
  return invoke("service_login_status", withAddr({ loginId }));
}

export async function serviceLoginComplete(state, code, redirectUri) {
  return invoke("service_login_complete", withAddr({ state, code, redirectUri }));
}

// API Key
export async function serviceApiKeyList() {
  return invoke("service_apikey_list", withAddr());
}

export async function serviceApiKeyReadSecret(keyId) {
  return invoke("service_apikey_read_secret", withAddr({ keyId }));
}

export async function serviceApiKeyCreate(name, modelSlug, reasoningEffort, profile = {}) {
  return invoke("service_apikey_create", withAddr({
    name,
    modelSlug,
    reasoningEffort,
    protocolType: profile.protocolType || null,
  }));
}

export async function serviceApiKeyModels() {
  return invoke("service_apikey_models", withAddr());
}

export async function serviceApiKeyUpdateModel(keyId, modelSlug, reasoningEffort, profile = {}) {
  return invoke("service_apikey_update_model", withAddr({
    keyId,
    modelSlug,
    reasoningEffort,
    protocolType: profile.protocolType || null,
  }));
}

export async function serviceApiKeyDelete(keyId) {
  return invoke("service_apikey_delete", withAddr({ keyId }));
}

export async function serviceApiKeyDisable(keyId) {
  return invoke("service_apikey_disable", withAddr({ keyId }));
}

export async function serviceApiKeyEnable(keyId) {
  return invoke("service_apikey_enable", withAddr({ keyId }));
}

// 打开浏览器
export async function openInBrowser(url) {
  return invoke("open_in_browser", { url });
}

// 应用更新
export async function updateCheck() {
  return invokeFirst(["app_update_check", "update_check", "check_update"], {});
}

export async function updateDownload(payload = {}) {
  return invokeFirst(["app_update_prepare", "update_download", "download_update"], payload);
}

export async function updateInstall(payload = {}) {
  return invokeFirst(["app_update_launch_installer", "update_install", "install_update"], payload);
}

export async function updateRestart(payload = {}) {
  return invokeFirst(["app_update_apply_portable", "update_restart", "restart_update"], payload);
}

export async function updateStatus() {
  return invokeFirst(["app_update_status", "update_status"], {});
}
