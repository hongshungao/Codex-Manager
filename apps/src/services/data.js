import { state } from "../state";
import * as api from "../api";

let requestLogRefreshSeq = 0;
const DEFAULT_REQUEST_LOG_TODAY_SUMMARY = {
  todayTokens: 0,
  cachedInputTokens: 0,
  reasoningOutputTokens: 0,
  estimatedCost: 0,
};

function ensureRpcSuccess(result, fallbackMessage) {
  if (result && typeof result === "object" && typeof result.error === "string" && result.error) {
    throw new Error(result.error);
  }
  if (result == null) {
    throw new Error(fallbackMessage);
  }
  return result;
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

function readPath(source, path) {
  const steps = String(path).split(".");
  let cursor = source;
  for (const step of steps) {
    if (!cursor || typeof cursor !== "object" || !(step in cursor)) {
      return undefined;
    }
    cursor = cursor[step];
  }
  return cursor;
}

function toFiniteNumber(value) {
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : null;
  }
  if (typeof value === "string") {
    const normalized = value.trim();
    if (!normalized) return null;
    const parsed = Number(normalized);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function pickNumber(source, paths, fallback = 0) {
  for (const path of paths) {
    const parsed = toFiniteNumber(readPath(source, path));
    if (parsed != null) {
      return parsed;
    }
  }
  return fallback;
}

// 刷新账号列表
export async function refreshAccounts() {
  const res = ensureRpcSuccess(await api.serviceAccountList(), "读取账号列表失败");
  state.accountList = Array.isArray(res.items) ? res.items : [];
}

// 刷新用量列表
export async function refreshUsageList() {
  const res = ensureRpcSuccess(await api.serviceUsageList(), "读取用量列表失败");
  state.usageList = Array.isArray(res.items) ? res.items : [];
}

// 刷新 API Key 列表
export async function refreshApiKeys() {
  const res = ensureRpcSuccess(await api.serviceApiKeyList(), "读取平台 Key 列表失败");
  state.apiKeyList = Array.isArray(res.items) ? res.items : [];
}

// 刷新模型下拉选项（来自平台上游 /v1/models）
export async function refreshApiModels() {
  const res = ensureRpcSuccess(await api.serviceApiKeyModels(), "读取模型列表失败");
  state.apiModelOptions = Array.isArray(res.items) ? res.items : [];
}

// 刷新请求日志（按关键字过滤）
export async function refreshRequestLogs(query, options = {}) {
  const latestOnly = options.latestOnly !== false;
  const seq = ++requestLogRefreshSeq;
  const res = ensureRpcSuccess(
    await api.serviceRequestLogList(query || null, 300),
    "读取请求日志失败",
  );
  if (latestOnly && seq !== requestLogRefreshSeq) {
    return false;
  }
  state.requestLogList = Array.isArray(res.items) ? res.items : [];
  return true;
}

export async function clearRequestLogs() {
  return ensureRpcSuccess(await api.serviceRequestLogClear(), "清空请求日志失败");
}

export async function refreshRequestLogTodaySummary() {
  try {
    const res = ensureRpcSuccess(
      await api.serviceRequestLogTodaySummary(),
      "读取今日请求汇总失败",
    );
    const inputTokens = pickNumber(res, [
      "inputTokens",
      "promptTokens",
      "tokens.input",
      "result.inputTokens",
      "result.promptTokens",
      "result.tokens.input",
    ], 0);
    const outputTokens = pickNumber(res, [
      "outputTokens",
      "completionTokens",
      "tokens.output",
      "result.outputTokens",
      "result.completionTokens",
      "result.tokens.output",
    ], 0);
    const cachedInputTokens = pickNumber(res, [
      "cachedInputTokens",
      "cachedTokens",
      "tokens.cachedInput",
      "usage.cachedInputTokens",
      "usage.cachedTokens",
      "result.cachedInputTokens",
      "result.cachedTokens",
      "result.tokens.cachedInput",
      "result.usage.cachedInputTokens",
      "result.usage.cachedTokens",
    ], 0);
    const reasoningOutputTokens = pickNumber(res, [
      "reasoningOutputTokens",
      "reasoningTokens",
      "tokens.reasoningOutput",
      "usage.reasoningOutputTokens",
      "usage.reasoningTokens",
      "result.reasoningOutputTokens",
      "result.reasoningTokens",
      "result.tokens.reasoningOutput",
      "result.usage.reasoningOutputTokens",
      "result.usage.reasoningTokens",
    ], 0);
    const todayTokens = pickNumber(res, [
      "todayTokens",
      "totalTokens",
      "tokenTotal",
      "tokens.total",
      "result.todayTokens",
      "result.totalTokens",
      "result.tokenTotal",
      "result.tokens.total",
    ], Math.max(0, inputTokens - cachedInputTokens) + outputTokens);
    const estimatedCost = pickNumber(res, [
      "estimatedCost",
      "cost",
      "costEstimate",
      "todayCost",
      "result.estimatedCost",
      "result.cost",
      "result.costEstimate",
      "result.todayCost",
    ], 0);
    state.requestLogTodaySummary = {
      todayTokens: Math.max(0, todayTokens),
      cachedInputTokens: Math.max(0, cachedInputTokens),
      reasoningOutputTokens: Math.max(0, reasoningOutputTokens),
      estimatedCost: Math.max(0, estimatedCost),
    };
  } catch (err) {
    if (!isCommandMissingError(err)) {
      throw err;
    }
    state.requestLogTodaySummary = { ...DEFAULT_REQUEST_LOG_TODAY_SUMMARY };
  }
}
