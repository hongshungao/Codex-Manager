"use client";

import { normalizeAccountProxySummaryFields } from "./account-proxy-normalize";

import {
  Account,
  AccountFetchedModel,
  AccountFetchModelsResult,
  AccountListResult,
  AccountUsage,
  AggregateApi,
  AggregateApiBalanceRefreshResult,
  AggregateApiBalanceSnapshot,
  AggregateApiAssociateModelsResult,
  AggregateApiCreateResult,
  AggregateApiFetchModelsResult,
  AggregateApiFetchedModel,
  AggregateApiSecretResult,
  AggregateApiTestResult,
  ApiKey,
  ApiKeyCreateResult,
  ApiKeyUsageStat,
  AppSettings,
  BackgroundTaskSettings,
  QuotaGuardSettings,
  RuntimeTimeZone,
  DeviceAuthInfo,
  EnvOverrideCatalogItem,
  InstalledPluginSummary,
  LoginStartResult,
  ModelCatalog,
  ModelInfo,
  ModelReasoningLevel,
  ModelTruncationPolicy,
  PluginCatalogEntry,
  PluginCatalogResult,
  PluginCatalogTask,
  PluginRunLogSummary,
  PluginTaskSummary,
  RequestLog,
  RequestLogFilterSummary,
  RequestLogListResult,
  RequestLogListWithSummaryResult,
  RequestLogTodaySummary,
  StartupSnapshot,
  UsageAggregateSummary,
} from "@/types";
import {
  DEFAULT_CODEX_ORIGINATOR,
  DEFAULT_CODEX_USER_AGENT,
  DEFAULT_CODEX_USER_AGENT_VERSION,
} from "@/lib/constants/codex";
import {
  DEFAULT_AUTHOR_SERVER_RECOMMENDATIONS,
  DEFAULT_AUTHOR_SPONSORS,
  normalizeSponsorLinkItems,
} from "@/lib/sponsor-links";
import {
  calcAvailability,
  getUsageDisplayBuckets,
  isLowQuotaUsage,
  toNullableNumber,
} from "@/lib/utils/usage";
import { readBillingModeLock } from "./billing-mode-lock";
import { normalizeGatewayTransportValues } from "@/lib/gateway/transport-settings";

const DEFAULT_BACKGROUND_TASKS: BackgroundTaskSettings = {
  usagePollingEnabled: true,
  usagePollIntervalSecs: 600,
  gatewayKeepaliveEnabled: true,
  gatewayKeepaliveIntervalSecs: 180,
  tokenRefreshPollingEnabled: true,
  tokenRefreshPollIntervalSecs: 60,
  usageRefreshWorkers: 4,
  httpWorkerFactor: 4,
  httpWorkerMin: 8,
  httpStreamWorkerFactor: 1,
  httpStreamWorkerMin: 2,
  warmupCronEnabled: false,
  warmupCronExpression: "",
};

const DEFAULT_QUOTA_GUARD: QuotaGuardSettings = {
  enabled: true,
  primaryMinRemainingPercent: 5,
  secondaryMinRemainingPercent: 10,
  allowAllLowQuotaFallback: true,
};

const DEFAULT_RUNTIME_TIME_ZONE: RuntimeTimeZone = {
  name: "Local",
  offset: "",
  source: "system",
};

/**
 * 函数 `asObject`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
function asObject(payload: unknown): Record<string, unknown> {
  return payload && typeof payload === "object" && !Array.isArray(payload)
    ? (payload as Record<string, unknown>)
    : {};
}

/**
 * 函数 `asArray`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
function asArray<T = unknown>(payload: unknown): T[] {
  return Array.isArray(payload) ? payload : [];
}

/**
 * 函数 `asString`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 * - fallback: 参数 fallback
 *
 * # 返回
 * 返回函数执行结果
 */
function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value.trim() : fallback;
}

function asJsonString(value: unknown): string | null {
  if (typeof value === "string") {
    const text = value.trim();
    return text || null;
  }
  if (!value || typeof value !== "object") return null;
  try {
    return JSON.stringify(value);
  } catch {
    return null;
  }
}

/**
 * 函数 `asBoolean`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 * - fallback: 参数 fallback
 *
 * # 返回
 * 返回函数执行结果
 */
function asBoolean(value: unknown, fallback = false): boolean {
  if (typeof value === "boolean") return value;
  if (typeof value === "number") return value !== 0;
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (["1", "true", "yes", "on"].includes(normalized)) return true;
    if (["0", "false", "no", "off"].includes(normalized)) return false;
  }
  return fallback;
}

function toNullableBoolean(value: unknown): boolean | null {
  if (typeof value === "boolean") return value;
  return null;
}

function toNullableObject(value: unknown): Record<string, unknown> | null {
  const object = asObject(value);
  return Object.keys(object).length > 0 ? object : null;
}

/**
 * 函数 `asInteger`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 * - fallback: 参数 fallback
 * - min: 参数 min
 *
 * # 返回
 * 返回函数执行结果
 */
function asInteger(value: unknown, fallback: number, min = 0): number {
  const parsed = toNullableNumber(value);
  if (parsed == null) return fallback;
  return Math.max(min, Math.trunc(parsed));
}

/**
 * 函数 `normalizeStringRecord`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
function normalizeStringRecord(payload: unknown): Record<string, string> {
  const source = asObject(payload);
  return Object.entries(source).reduce<Record<string, string>>((result, [key, value]) => {
    result[key] = asString(value);
    return result;
  }, {});
}

/**
 * 函数 `asStringArray`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 *
 * # 返回
 * 返回函数执行结果
 */
function asStringArray(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value
      .map((item) => asString(item))
      .filter((item) => item.length > 0);
  }
  if (typeof value === "string") {
    return value
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
  }
  return [];
}

/**
 * 函数 `normalizeUsageSnapshot`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeUsageSnapshot(payload: unknown): AccountUsage | null {
  const source = asObject(payload);
  const accountId = asString(source.accountId ?? source.account_id);
  if (!accountId) return null;

  return {
    accountId,
    availabilityStatus: asString(source.availabilityStatus ?? source.availability_status),
    usedPercent: toNullableNumber(source.usedPercent ?? source.used_percent),
    windowMinutes: toNullableNumber(source.windowMinutes ?? source.window_minutes),
    resetsAt: toNullableNumber(source.resetsAt ?? source.resets_at),
    secondaryUsedPercent: toNullableNumber(
      source.secondaryUsedPercent ?? source.secondary_used_percent
    ),
    secondaryWindowMinutes: toNullableNumber(
      source.secondaryWindowMinutes ?? source.secondary_window_minutes
    ),
    secondaryResetsAt: toNullableNumber(
      source.secondaryResetsAt ?? source.secondary_resets_at
    ),
    creditsJson: asJsonString(source.creditsJson ?? source.credits_json),
    capturedAt: toNullableNumber(source.capturedAt ?? source.captured_at),
  };
}

/**
 * 函数 `normalizeUsageList`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeUsageList(payload: unknown): AccountUsage[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizeUsageSnapshot(item))
    .filter((item): item is AccountUsage => Boolean(item));
}

/**
 * 函数 `buildUsageMap`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - usages: 参数 usages
 *
 * # 返回
 * 返回函数执行结果
 */
export function buildUsageMap(usages: AccountUsage[]): Map<string, AccountUsage> {
  const result = new Map<string, AccountUsage>();
  for (const item of usages) {
    const previous = result.get(item.accountId);
    if (
      previous &&
      previous.capturedAt != null &&
      (item.capturedAt == null || item.capturedAt < previous.capturedAt)
    ) {
      continue;
    }
    result.set(item.accountId, item);
  }
  return result;
}

/**
 * 函数 `normalizeUsageAggregateSummary`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeUsageAggregateSummary(payload: unknown): UsageAggregateSummary {
  const source = asObject(payload);
  return {
    primaryBucketCount: asInteger(source.primaryBucketCount, 0, 0),
    primaryKnownCount: asInteger(source.primaryKnownCount, 0, 0),
    primaryUnknownCount: asInteger(source.primaryUnknownCount, 0, 0),
    primaryRemainPercent: toNullableNumber(source.primaryRemainPercent),
    secondaryBucketCount: asInteger(source.secondaryBucketCount, 0, 0),
    secondaryKnownCount: asInteger(source.secondaryKnownCount, 0, 0),
    secondaryUnknownCount: asInteger(source.secondaryUnknownCount, 0, 0),
    secondaryRemainPercent: toNullableNumber(source.secondaryRemainPercent),
  };
}

function normalizeStartupAccountSummary(payload: unknown) {
  const source = asObject(payload);
  return {
    accountCount: asInteger(source.accountCount ?? source.account_count, 0, 0),
    availableCount: asInteger(source.availableCount ?? source.available_count, 0, 0),
    lowQuotaCount: asInteger(source.lowQuotaCount ?? source.low_quota_count, 0, 0),
    primaryRemainPercent: toNullableNumber(
      source.primaryRemainPercent ?? source.primary_remain_percent
    ),
    secondaryRemainPercent: toNullableNumber(
      source.secondaryRemainPercent ?? source.secondary_remain_percent
    ),
    lastRefreshedAt: toNullableNumber(source.lastRefreshedAt ?? source.last_refreshed_at),
  };
}

/**
 * 函数 `normalizeTodaySummary`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeTodaySummary(payload: unknown): RequestLogTodaySummary {
  const source = asObject(payload);
  const inputTokens = asInteger(source.inputTokens, 0, 0);
  const cachedInputTokens = asInteger(source.cachedInputTokens, 0, 0);
  const outputTokens = asInteger(source.outputTokens, 0, 0);
  const reasoningOutputTokens = asInteger(source.reasoningOutputTokens, 0, 0);
  return {
    inputTokens,
    cachedInputTokens,
    outputTokens,
    reasoningOutputTokens,
    todayTokens: asInteger(
      source.todayTokens,
      Math.max(0, inputTokens - cachedInputTokens) + outputTokens,
      0
    ),
    estimatedCost: Math.max(0, toNullableNumber(source.estimatedCost) ?? 0),
  };
}

/**
 * 函数 `normalizeAccount`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - item: 参数 item
 * - usage?: 参数 usage?
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeAccount(item: unknown, usage?: AccountUsage | null): Account | null {
  const source = asObject(item);
  const id = asString(source.id);
  if (!id) return null;

  const name = asString(source.label || source.name) || id;
  const groupName = asString(source.groupName ?? source.group_name);
  const status = asString(source.status);
  const statusReason = asString(source.statusReason ?? source.status_reason);
  const rawHasToken = source.hasToken ?? source.has_token;
  const hasToken = typeof rawHasToken === "boolean" ? Boolean(rawHasToken) : true;
  const availability = calcAvailability(usage, { status, statusReason, hasToken });
  const usageBuckets = getUsageDisplayBuckets(usage);

  return {
    id,
    name,
    group: groupName,
    priority: asInteger(source.sort ?? source.priority, 0, 0),
    preferred: Boolean(source.preferred),
    resetWarmupEnabled:
      (source.resetWarmupEnabled ?? source.reset_warmup_enabled) !== false,
    label: name,
    groupName,
    sort: asInteger(source.sort ?? source.priority, 0, 0),
    status,
    statusReason,
    hasToken,
    planType:
      asString(source.planType ?? source.plan_type ?? source.subscriptionPlan ?? source.subscription_plan) ||
      null,
    planTypeRaw: asString(source.planTypeRaw ?? source.plan_type_raw) || null,
    hasSubscription:
      typeof (source.hasSubscription ?? source.has_subscription) === "boolean"
        ? Boolean(source.hasSubscription ?? source.has_subscription)
        : null,
    subscriptionPlan:
      asString(source.subscriptionPlan ?? source.subscription_plan) || null,
    subscriptionExpiresAt: toNullableNumber(
      source.subscriptionExpiresAt ?? source.subscription_expires_at
    ),
    subscriptionRenewsAt: toNullableNumber(
      source.subscriptionRenewsAt ?? source.subscription_renews_at
    ),
    note: asString(source.note) || null,
    tags: asStringArray(source.tags),
    quotaCapacityPrimaryWindowTokens: toNullableNumber(
      source.quotaCapacityPrimaryWindowTokens ??
        source.quota_capacity_primary_window_tokens
    ),
    quotaCapacitySecondaryWindowTokens: toNullableNumber(
      source.quotaCapacitySecondaryWindowTokens ??
        source.quota_capacity_secondary_window_tokens
    ),
    isAvailable: availability.level === "ok",
    isLowQuota: isLowQuotaUsage(usage),
    lastRefreshAt: usage?.capturedAt ?? null,
    availabilityText: availability.text,
    availabilityLevel: availability.level,
    primaryRemainPercent: usageBuckets.primaryRemainPercent,
    secondaryRemainPercent: usageBuckets.secondaryRemainPercent,
    usage: usage ?? null,
    ...normalizeAccountProxySummaryFields(source),
  };
}

/**
 * 函数 `normalizeAccountList`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 * - usages: 参数 usages
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeAccountList(
  payload: unknown,
  usages: AccountUsage[] = []
): AccountListResult {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  const usageMap = buildUsageMap(usages);
  const normalizedItems = items
    .map((item) => normalizeAccount(item, usageMap.get(asString(asObject(item).id))))
    .filter((item): item is Account => Boolean(item));

  return {
    items: normalizedItems,
    total: asInteger(source.total, normalizedItems.length, 0),
    page: asInteger(source.page, 1, 1),
    pageSize: asInteger(source.pageSize, normalizedItems.length || 20, 1),
  };
}

/**
 * 函数 `attachUsagesToAccounts`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - accounts: 参数 accounts
 * - usages: 参数 usages
 *
 * # 返回
 * 返回函数执行结果
 */
export function attachUsagesToAccounts(
  accounts: Account[],
  usages: AccountUsage[]
): Account[] {
  const usageMap = buildUsageMap(usages);
  return accounts.map((account) => normalizeAccount(account, usageMap.get(account.id)) || account);
}

/**
 * 函数 `normalizeModelReasoningLevels`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-12
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
function normalizeModelReasoningLevels(payload: unknown): ModelReasoningLevel[] {
  const items = asArray(payload);
  return items
    .map((item) => {
      const current = asObject(item);
      const effort = asString(current.effort);
      if (!effort) return null;
      return {
        ...current,
        effort,
        description: asString(current.description),
      };
    })
    .filter((item): item is ModelReasoningLevel => Boolean(item));
}

function normalizeModelTruncationPolicy(payload: unknown): ModelTruncationPolicy | null {
  const source = asObject(payload);
  const mode = asString(source.mode);
  if (!mode) return null;
  return {
    ...source,
    mode,
    limit: toNullableNumber(source.limit) ?? 0,
  };
}

function normalizeModelServiceTiers(payload: unknown): ModelInfo["serviceTiers"] {
  const seen = new Set<string>();
  return asArray(payload)
    .map((item) => {
      const current = asObject(item);
      const id = asString(current.id);
      if (!id || seen.has(id)) return null;
      seen.add(id);
      return {
        ...current,
        id,
        name: asString(current.name) || id,
        description: asString(current.description),
      };
    })
    .filter((item): item is ModelInfo["serviceTiers"][number] => Boolean(item));
}

function normalizeModelVisibility(value: unknown): string | null {
  const normalized = asString(value).trim().toLowerCase();
  if (!normalized) return null;
  if (normalized === "hidden") {
    return "hide";
  }
  return normalized;
}

function normalizeModelInfo(payload: unknown): ModelInfo | null {
  const source = asObject(payload);
  const slug = asString(source.slug);
  if (!slug) return null;
  const rawInputModalities =
    source.input_modalities ?? source.inputModalities ?? ["text", "image"];

  return {
    ...source,
    slug,
    displayName: asString(source.display_name ?? source.displayName) || slug,
    description: asString(source.description) || null,
    defaultReasoningLevel:
      asString(source.default_reasoning_level ?? source.defaultReasoningLevel) || null,
    supportedReasoningLevels: normalizeModelReasoningLevels(
      source.supported_reasoning_levels ?? source.supportedReasoningLevels,
    ),
    shellType: asString(source.shell_type ?? source.shellType) || null,
    visibility: normalizeModelVisibility(source.visibility),
    supportedInApi: asBoolean(source.supported_in_api ?? source.supportedInApi, true),
    priority: toNullableNumber(source.priority) ?? 0,
    additionalSpeedTiers: asArray(
      source.additional_speed_tiers ?? source.additionalSpeedTiers,
    ).map((item) => asString(item)),
    serviceTiers: normalizeModelServiceTiers(source.service_tiers ?? source.serviceTiers),
    defaultServiceTier:
      asString(source.default_service_tier ?? source.defaultServiceTier) || null,
    availabilityNux: toNullableObject(source.availability_nux ?? source.availabilityNux),
    upgrade: toNullableObject(source.upgrade),
    upgradeInfo: toNullableObject(source.upgrade_info ?? source.upgradeInfo),
    baseInstructions:
      asString(source.base_instructions ?? source.baseInstructions) || null,
    modelMessages: toNullableObject(source.model_messages ?? source.modelMessages),
    supportsReasoningSummaries: toNullableBoolean(
      source.supports_reasoning_summaries ?? source.supportsReasoningSummaries,
    ),
    defaultReasoningSummary:
      asString(source.default_reasoning_summary ?? source.defaultReasoningSummary) || null,
    supportVerbosity: toNullableBoolean(
      source.support_verbosity ?? source.supportVerbosity,
    ),
    defaultVerbosity: source.default_verbosity ?? source.defaultVerbosity ?? null,
    applyPatchToolType:
      asString(source.apply_patch_tool_type ?? source.applyPatchToolType) || null,
    webSearchToolType:
      asString(source.web_search_tool_type ?? source.webSearchToolType) || null,
    truncationPolicy: normalizeModelTruncationPolicy(
      source.truncation_policy ?? source.truncationPolicy,
    ),
    supportsParallelToolCalls: toNullableBoolean(
      source.supports_parallel_tool_calls ?? source.supportsParallelToolCalls,
    ),
    supportsImageDetailOriginal: toNullableBoolean(
      source.supports_image_detail_original ?? source.supportsImageDetailOriginal,
    ),
    contextWindow: toNullableNumber(source.context_window ?? source.contextWindow),
    autoCompactTokenLimit: toNullableNumber(
      source.auto_compact_token_limit ?? source.autoCompactTokenLimit,
    ),
    effectiveContextWindowPercent: toNullableNumber(
      source.effective_context_window_percent ?? source.effectiveContextWindowPercent,
    ),
    experimentalSupportedTools: asArray(
      source.experimental_supported_tools ?? source.experimentalSupportedTools,
    ).map((item) => asString(item)),
    inputModalities: asArray(rawInputModalities).map((item) => asString(item)),
    outputModalities: asArray(
      source.output_modalities ?? source.outputModalities,
    ).map((item) => asString(item)),
    supportedEndpoints: asArray(
      source.supported_endpoints ?? source.supportedEndpoints,
    ).map((item) => asString(item)),
    supportsTextGeneration: asBoolean(
      source.supports_text_generation ?? source.supportsTextGeneration,
      true,
    ),
    minimalClientVersion:
      source.minimal_client_version ?? source.minimalClientVersion ?? null,
    supportsSearchTool: toNullableBoolean(
      source.supports_search_tool ?? source.supportsSearchTool,
    ),
    availableInPlans: asArray(source.available_in_plans ?? source.availableInPlans).map((item) =>
      asString(item),
    ),
  };
}

/**
 * 函数 `normalizeModelCatalog`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-12
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeModelCatalog(payload: unknown): ModelCatalog {
  const source = asObject(payload);
  const items = asArray(source.models ?? payload);
  return {
    ...source,
    models: items
      .map((item) => normalizeModelInfo(item))
      .filter((item): item is ModelInfo => Boolean(item)),
  };
}

/**
 * 函数 `normalizeApiKey`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - item: 参数 item
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeApiKey(item: unknown): ApiKey | null {
  const source = asObject(item);
  const id = asString(source.id);
  if (!id) return null;

  return {
    id,
    name: asString(source.name),
    model: asString(source.modelSlug ?? source.model_slug),
    modelSlug: asString(source.modelSlug ?? source.model_slug),
    reasoningEffort: asString(source.reasoningEffort ?? source.reasoning_effort),
    serviceTier: asString(source.serviceTier ?? source.service_tier),
    rotationStrategy: asString(source.rotationStrategy ?? source.rotation_strategy) || "account_rotation",
    aggregateApiId: asString(source.aggregateApiId ?? source.aggregate_api_id) || null,
    accountPlanFilter: asString(source.accountPlanFilter ?? source.account_plan_filter) || null,
    accountGroupFilter: asString(source.accountGroupFilter ?? source.account_group_filter) || null,
    aggregateApiUrl: asString(source.aggregateApiUrl ?? source.aggregate_api_url) || null,
    quotaLimitTokens: toNullableNumber(source.quotaLimitTokens ?? source.quota_limit_tokens),
    protocol: asString(source.protocolType ?? source.protocol_type) || "openai_compat",
    clientType: asString(source.clientType ?? source.client_type),
    authScheme: asString(source.authScheme ?? source.auth_scheme),
    upstreamBaseUrl: asString(source.upstreamBaseUrl ?? source.upstream_base_url),
    staticHeadersJson: asString(source.staticHeadersJson ?? source.static_headers_json),
    status: asString(source.status) || "enabled",
    createdAt: toNullableNumber(source.createdAt ?? source.created_at),
    lastUsedAt: toNullableNumber(source.lastUsedAt ?? source.last_used_at),
  };
}

/**
 * 函数 `normalizeApiKeyList`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeApiKeyList(payload: unknown): ApiKey[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizeApiKey(item))
    .filter((item): item is ApiKey => Boolean(item));
}

/**
 * 函数 `normalizeApiKeyCreateResult`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeApiKeyCreateResult(payload: unknown): ApiKeyCreateResult {
  const source = asObject(payload);
  return {
    id: asString(source.id),
    key: asString(source.key),
  };
}

/**
 * 函数 `normalizeAggregateApi`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - item: 参数 item
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeAggregateApi(item: unknown): AggregateApi | null {
  const source = asObject(item);
  const id = asString(source.id);
  if (!id) return null;

  return {
    id,
    providerType: asString(source.providerType ?? source.provider_type) || "codex",
    supplierName: asString(source.supplierName ?? source.supplier_name) || null,
    sort: asInteger(source.sort ?? source.priority, 0, 0),
    url: asString(source.url),
    userAgent: asString(source.userAgent ?? source.user_agent) || null,
    authType: asString(source.authType ?? source.auth_type) || "apikey",
    authParams:
      source.authParams && typeof source.authParams === "object"
        ? asObject(source.authParams)
        : source.auth_params && typeof source.auth_params === "object"
          ? asObject(source.auth_params)
          : null,
    action:
      typeof source.action === "string"
        ? source.action
        : asString(source.action) || null,
    status: asString(source.status) || "active",
    createdAt: toNullableNumber(source.createdAt ?? source.created_at),
    updatedAt: toNullableNumber(source.updatedAt ?? source.updated_at),
    lastTestAt: toNullableNumber(source.lastTestAt ?? source.last_test_at),
    lastTestStatus: asString(source.lastTestStatus ?? source.last_test_status) || null,
    lastTestError: asString(source.lastTestError ?? source.last_test_error) || null,
    balanceQueryEnabled: asBoolean(
      source.balanceQueryEnabled ?? source.balance_query_enabled,
      false
    ),
    balanceQueryTemplate:
      asString(source.balanceQueryTemplate ?? source.balance_query_template) || null,
    balanceQueryBaseUrl:
      asString(source.balanceQueryBaseUrl ?? source.balance_query_base_url) || null,
    balanceQueryUserId:
      asString(source.balanceQueryUserId ?? source.balance_query_user_id) || null,
    balanceQueryConfigJson:
      asString(
        source.balanceQueryConfigJson ?? source.balance_query_config_json
      ) || null,
    lastBalanceAt: toNullableNumber(source.lastBalanceAt ?? source.last_balance_at),
    lastBalanceStatus:
      asString(source.lastBalanceStatus ?? source.last_balance_status) || null,
    lastBalanceError:
      asString(source.lastBalanceError ?? source.last_balance_error) || null,
    lastBalanceJson:
      asString(source.lastBalanceJson ?? source.last_balance_json) || null,
    modelSlugs: asStringArray(source.modelSlugs ?? source.model_slugs),
  };
}

/**
 * 函数 `normalizeAggregateApiList`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeAggregateApiList(payload: unknown): AggregateApi[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizeAggregateApi(item))
    .filter((item): item is AggregateApi => Boolean(item));
}

/**
 * 函数 `normalizeAggregateApiCreateResult`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeAggregateApiCreateResult(payload: unknown): AggregateApiCreateResult {
  const source = asObject(payload);
  return {
    id: asString(source.id),
    key: asString(source.key),
  };
}

export function normalizeAggregateApiSecretResult(payload: unknown): AggregateApiSecretResult {
  const source = asObject(payload);
  return {
    id: asString(source.id),
    key: asString(source.key),
    authType: asString(source.authType ?? source.auth_type) || "apikey",
    username: asString(source.username) || null,
    password: asString(source.password) || null,
  };
}

/**
 * 函数 `normalizeAggregateApiTestResult`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeAggregateApiTestResult(payload: unknown): AggregateApiTestResult {
  const source = asObject(payload);
  return {
    id: asString(source.id),
    ok: asBoolean(source.ok),
    statusCode: toNullableNumber(source.statusCode ?? source.status_code),
    message: asString(source.message) || null,
    testedAt: asInteger(source.testedAt ?? source.tested_at, 0, 0),
    latencyMs: asInteger(source.latencyMs ?? source.latency_ms, 0, 0),
  };
}

export function normalizeAggregateApiBalanceSnapshot(
  payload: unknown
): AggregateApiBalanceSnapshot | null {
  const source = asObject(payload);
  const hasBalanceFields =
    "remaining" in source ||
    "used" in source ||
    "total" in source ||
    "isValid" in source ||
    "is_valid" in source;
  if (!hasBalanceFields) return null;
  return {
    isValid: asBoolean(source.isValid ?? source.is_valid, true),
    invalidMessage:
      asString(source.invalidMessage ?? source.invalid_message) || null,
    remaining: toNullableNumber(source.remaining),
    unit: asString(source.unit) || null,
    planName: asString(source.planName ?? source.plan_name) || null,
    total: toNullableNumber(source.total),
    used: toNullableNumber(source.used),
    extra: toNullableObject(source.extra),
  };
}

export function normalizeAggregateApiBalanceRefreshResult(
  payload: unknown
): AggregateApiBalanceRefreshResult {
  const source = asObject(payload);
  return {
    id: asString(source.id),
    ok: asBoolean(source.ok),
    balance: normalizeAggregateApiBalanceSnapshot(source.balance),
    message: asString(source.message) || null,
    queriedAt: asInteger(source.queriedAt ?? source.queried_at, 0, 0),
    latencyMs: asInteger(source.latencyMs ?? source.latency_ms, 0, 0),
  };
}

export function normalizeAggregateApiFetchModelsResult(
  payload: unknown,
): AggregateApiFetchModelsResult {
  const source = asObject(payload);
  const items = asArray(source.items).map((item): AggregateApiFetchedModel => {
    const value = asObject(item);
    return {
      upstreamModel: asString(value.upstreamModel ?? value.upstream_model),
      displayName: asString(value.displayName ?? value.display_name) || null,
      existingModelSlug:
        asString(value.existingModelSlug ?? value.existing_model_slug) || null,
      alreadyLinked: asBoolean(value.alreadyLinked ?? value.already_linked, false),
    };
  });
  return {
    apiId: asString(source.apiId ?? source.api_id),
    providerType: asString(source.providerType ?? source.provider_type),
    fetchedAt: asInteger(source.fetchedAt ?? source.fetched_at, 0, 0),
    items,
  };
}

export function normalizeAccountFetchModelsResult(
  payload: unknown,
): AccountFetchModelsResult {
  const source = asObject(payload);
  const items = asArray(source.items).map((item): AccountFetchedModel => {
    const value = asObject(item);
    return {
      upstreamModel: asString(value.upstreamModel ?? value.upstream_model),
      displayName: asString(value.displayName ?? value.display_name) || null,
      existingModelSlug:
        asString(value.existingModelSlug ?? value.existing_model_slug) || null,
      alreadyLinked: asBoolean(value.alreadyLinked ?? value.already_linked, false),
    };
  });
  return {
    accountId: asString(source.accountId ?? source.account_id),
    fetchedAt: asInteger(source.fetchedAt ?? source.fetched_at, 0, 0),
    items,
  };
}

export function normalizeAggregateApiAssociateModelsResult(
  payload: unknown,
): AggregateApiAssociateModelsResult {
  const source = asObject(payload);
  return {
    createdModels: asStringArray(source.createdModels ?? source.created_models),
    addedRoutes: asStringArray(source.addedRoutes ?? source.added_routes),
    unchangedRoutes: asStringArray(source.unchangedRoutes ?? source.unchanged_routes),
  };
}

/**
 * 函数 `normalizeApiKeyUsageStats`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeApiKeyUsageStats(payload: unknown): ApiKeyUsageStat[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => {
      const current = asObject(item);
      const keyId = asString(current.keyId ?? current.key_id);
      if (!keyId) return null;
      return {
        keyId,
        todayTokens: asInteger(
          current.todayTokens ?? current.today_tokens,
          0,
          0
        ),
        todayEstimatedCostUsd: Math.max(
          0,
          toNullableNumber(
            current.todayEstimatedCostUsd ?? current.today_estimated_cost_usd
          ) ?? 0
        ),
        totalTokens: asInteger(current.totalTokens ?? current.total_tokens, 0, 0),
        estimatedCostUsd: Math.max(
          0,
          toNullableNumber(current.estimatedCostUsd ?? current.estimated_cost_usd) ?? 0
        ),
      };
    })
    .filter((item): item is ApiKeyUsageStat => Boolean(item));
}

/**
 * 函数 `normalizePluginCatalogTask`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizePluginCatalogTask(payload: unknown): PluginCatalogTask | null {
  const source = asObject(payload);
  const id = asString(source.id);
  if (!id) return null;

  return {
    id,
    name: asString(source.name) || id,
    description: asString(source.description) || null,
    entrypoint: asString(source.entrypoint) || "run",
    scheduleKind: asString(source.scheduleKind ?? source.schedule_kind) || "manual",
    intervalSeconds: toNullableNumber(source.intervalSeconds ?? source.interval_seconds),
    enabled: asBoolean(source.enabled, true),
  };
}

/**
 * 函数 `normalizePluginCatalogEntry`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizePluginCatalogEntry(payload: unknown): PluginCatalogEntry | null {
  const source = asObject(payload);
  const id = asString(source.id);
  if (!id) return null;
  return {
    id,
    name: asString(source.name) || id,
    version: asString(source.version) || "0.0.0",
    description: asString(source.description) || null,
    author: asString(source.author) || null,
    homepageUrl: asString(source.homepageUrl ?? source.homepage_url) || null,
    scriptUrl: asString(source.scriptUrl ?? source.script_url) || null,
    scriptBody: asString(source.scriptBody ?? source.script_body) || null,
    permissions: asArray(source.permissions).map((item) => asString(item)).filter(Boolean),
    tasks: asArray(source.tasks)
      .map((item) => normalizePluginCatalogTask(item))
      .filter((item): item is PluginCatalogTask => Boolean(item)),
    manifestVersion: asString(source.manifestVersion ?? source.manifest_version) || "1",
    category: asString(source.category) || null,
    runtimeKind: asString(source.runtimeKind ?? source.runtime_kind) || "rhai",
    tags: asArray(source.tags).map((item) => asString(item)).filter(Boolean),
    sourceUrl: asString(source.sourceUrl ?? source.source_url) || null,
  };
}

/**
 * 函数 `normalizePluginCatalogResult`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizePluginCatalogResult(payload: unknown): PluginCatalogResult {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload)
    .map((item) => normalizePluginCatalogEntry(item))
    .filter((item): item is PluginCatalogEntry => Boolean(item));
  return {
    sourceUrl: asString(source.sourceUrl ?? source.source_url),
    items,
  };
}

/**
 * 函数 `normalizeInstalledPlugin`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeInstalledPlugin(payload: unknown): InstalledPluginSummary | null {
  const source = asObject(payload);
  const pluginId = asString(source.pluginId ?? source.plugin_id);
  if (!pluginId) return null;

  return {
    pluginId,
    sourceUrl: asString(source.sourceUrl ?? source.source_url) || null,
    name: asString(source.name) || pluginId,
    version: asString(source.version) || "0.0.0",
    description: asString(source.description) || null,
    author: asString(source.author) || null,
    homepageUrl: asString(source.homepageUrl ?? source.homepage_url) || null,
    scriptUrl: asString(source.scriptUrl ?? source.script_url) || null,
    permissions: asArray(source.permissions).map((item) => asString(item)).filter(Boolean),
    status: asString(source.status) || "disabled",
    installedAt: asInteger(source.installedAt ?? source.installed_at, 0, 0),
    updatedAt: asInteger(source.updatedAt ?? source.updated_at, 0, 0),
    lastRunAt: toNullableNumber(source.lastRunAt ?? source.last_run_at),
    lastError: asString(source.lastError ?? source.last_error) || null,
    taskCount: asInteger(source.taskCount ?? source.task_count, 0, 0),
    enabledTaskCount: asInteger(source.enabledTaskCount ?? source.enabled_task_count, 0, 0),
    manifestVersion: asString(source.manifestVersion ?? source.manifest_version) || "1",
    category: asString(source.category) || null,
    runtimeKind: asString(source.runtimeKind ?? source.runtime_kind) || "rhai",
    tags: asArray(source.tags).map((item) => asString(item)).filter(Boolean),
  };
}

/**
 * 函数 `normalizePluginInstalledList`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizePluginInstalledList(payload: unknown): InstalledPluginSummary[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizeInstalledPlugin(item))
    .filter((item): item is InstalledPluginSummary => Boolean(item));
}

/**
 * 函数 `normalizePluginTask`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizePluginTask(payload: unknown): PluginTaskSummary | null {
  const source = asObject(payload);
  const id = asString(source.id);
  const pluginId = asString(source.pluginId ?? source.plugin_id);
  if (!id || !pluginId) return null;
  return {
    id,
    pluginId,
    pluginName: asString(source.pluginName ?? source.plugin_name) || pluginId,
    name: asString(source.name) || id,
    description: asString(source.description) || null,
    entrypoint: asString(source.entrypoint) || "run",
    scheduleKind: asString(source.scheduleKind ?? source.schedule_kind) || "manual",
    intervalSeconds: toNullableNumber(source.intervalSeconds ?? source.interval_seconds),
    enabled: asBoolean(source.enabled, true),
    nextRunAt: toNullableNumber(source.nextRunAt ?? source.next_run_at),
    lastRunAt: toNullableNumber(source.lastRunAt ?? source.last_run_at),
    lastStatus: asString(source.lastStatus ?? source.last_status) || null,
    lastError: asString(source.lastError ?? source.last_error) || null,
  };
}

/**
 * 函数 `normalizePluginTaskList`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizePluginTaskList(payload: unknown): PluginTaskSummary[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizePluginTask(item))
    .filter((item): item is PluginTaskSummary => Boolean(item));
}

/**
 * 函数 `normalizePluginRunLog`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizePluginRunLog(payload: unknown): PluginRunLogSummary | null {
  const source = asObject(payload);
  const id = asInteger(source.id, 0, 0);
  if (!id) return null;
  return {
    id,
    pluginId: asString(source.pluginId ?? source.plugin_id),
    pluginName: asString(source.pluginName ?? source.plugin_name) || null,
    taskId: asString(source.taskId ?? source.task_id) || null,
    taskName: asString(source.taskName ?? source.task_name) || null,
    runType: asString(source.runType ?? source.run_type) || "manual",
    status: asString(source.status) || "ok",
    startedAt: asInteger(source.startedAt ?? source.started_at, 0, 0),
    finishedAt: toNullableNumber(source.finishedAt ?? source.finished_at),
    durationMs: toNullableNumber(source.durationMs ?? source.duration_ms),
    output: source.output ?? null,
    error: asString(source.error) || null,
  };
}

/**
 * 函数 `normalizePluginRunLogList`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizePluginRunLogList(payload: unknown): PluginRunLogSummary[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizePluginRunLog(item))
    .filter((item): item is PluginRunLogSummary => Boolean(item));
}

/**
 * 函数 `normalizeDeviceAuthInfo`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeDeviceAuthInfo(payload: unknown): DeviceAuthInfo | null {
  const source = asObject(payload);
  const verificationUrl = asString(source.verificationUrl ?? source.verification_url);
  if (!verificationUrl) return null;

  return {
    userCodeUrl: asString(source.userCodeUrl ?? source.user_code_url),
    tokenUrl: asString(source.tokenUrl ?? source.token_url),
    verificationUrl,
    redirectUri: asString(source.redirectUri ?? source.redirect_uri),
  };
}

/**
 * 函数 `normalizeLoginStartResult`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeLoginStartResult(payload: unknown): LoginStartResult {
  const source = asObject(payload);
  const type = asString(source.type ?? source.loginType ?? source.login_type);
  const verificationUrl = asString(source.verificationUrl ?? source.verification_url);
  const loginId = asString(source.loginId ?? source.login_id);

  if (type === "chatgptDeviceCode") {
    return {
      type,
      loginId,
      verificationUrl,
      userCode: asString(source.userCode ?? source.user_code),
    };
  }
  if (type === "chatgpt") {
    return {
      type,
      loginId,
      authUrl: asString(source.authUrl ?? source.auth_url),
    };
  }

  throw new Error(`unsupported login start result type: ${type || "unknown"}`);
}

/**
 * 函数 `normalizeRequestLog`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - item: 参数 item
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeRequestLog(item: unknown): RequestLog | null {
  const source = asObject(item);
  const createdAt = toNullableNumber(source.createdAt ?? source.created_at);
  const traceId = asString(source.traceId ?? source.trace_id);
  const keyId = asString(source.keyId ?? source.key_id);
  const accountId = asString(source.accountId ?? source.account_id);
  const requestPath = asString(source.requestPath ?? source.request_path);
  const method = asString(source.method);
  const id = traceId || [createdAt ?? "", method, requestPath, accountId, keyId].join("|");
  if (!id) return null;
  const durationMs = toNullableNumber(
    source.durationMs ??
      source.duration_ms ??
      source.latencyMs ??
      source.latency_ms ??
      source.elapsedMs ??
      source.elapsed_ms ??
      source.responseTimeMs ??
      source.response_time_ms
  );
  const firstResponseMs = toNullableNumber(
    source.firstResponseMs ??
      source.first_response_ms ??
      source.firstTokenMs ??
      source.first_token_ms ??
      source.ttftMs ??
      source.ttft_ms
  );

  return {
    id,
    traceId,
    keyId,
    accountId,
    initialAccountId: asString(source.initialAccountId ?? source.initial_account_id),
    attemptedAccountIds: asArray(source.attemptedAccountIds ?? source.attempted_account_ids)
      .map((value) => asString(value))
      .filter((value) => value.length > 0),
    initialAggregateApiId: asString(
      source.initialAggregateApiId ?? source.initial_aggregate_api_id
    ),
    attemptedAggregateApiIds: asArray(
      source.attemptedAggregateApiIds ?? source.attempted_aggregate_api_ids
    )
      .map((value) => asString(value))
      .filter((value) => value.length > 0),
    requestPath,
    originalPath: asString(source.originalPath ?? source.original_path),
    adaptedPath: asString(source.adaptedPath ?? source.adapted_path),
    method,
    requestType: asString(source.requestType ?? source.request_type) || "http",
    gatewayMode: asString(source.gatewayMode ?? source.gateway_mode),
    routeStrategy: asString(source.routeStrategy ?? source.route_strategy),
    routeSource: asString(source.routeSource ?? source.route_source),
    path: requestPath,
    clientModel: asString(source.clientModel ?? source.client_model),
    model: asString(source.model),
    modelSource: asString(source.modelSource ?? source.model_source),
    upstreamModel: asString(source.upstreamModel ?? source.upstream_model),
    actualSourceKind: asString(
      source.actualSourceKind ?? source.actual_source_kind
    ),
    actualSourceId: asString(source.actualSourceId ?? source.actual_source_id),
    clientReasoningEffort: asString(
      source.clientReasoningEffort ?? source.client_reasoning_effort
    ),
    reasoningEffort: asString(source.reasoningEffort ?? source.reasoning_effort),
    reasoningSource: asString(source.reasoningSource ?? source.reasoning_source),
    serviceTier: asString(source.serviceTier ?? source.service_tier),
    effectiveServiceTier: asString(
      source.effectiveServiceTier ?? source.effective_service_tier
    ),
    serviceTierSource: asString(
      source.serviceTierSource ?? source.service_tier_source
    ),
    responseAdapter: asString(source.responseAdapter ?? source.response_adapter),
    canonicalSource:
      asString(source.canonicalSource ?? source.canonical_source) || "native_codex",
    sizeRejectStage:
      asString(source.sizeRejectStage ?? source.size_reject_stage) || "-",
    upstreamUrl: asString(source.upstreamUrl ?? source.upstream_url),
    aggregateApiSupplierName:
      asString(
        source.aggregateApiSupplierName ?? source.aggregate_api_supplier_name
      ) || null,
    aggregateApiUrl:
      asString(source.aggregateApiUrl ?? source.aggregate_api_url) || null,
    statusCode: toNullableNumber(source.statusCode ?? source.status_code),
    inputTokens: toNullableNumber(source.inputTokens ?? source.input_tokens),
    cachedInputTokens: toNullableNumber(
      source.cachedInputTokens ?? source.cached_input_tokens
    ),
    outputTokens: toNullableNumber(source.outputTokens ?? source.output_tokens),
    totalTokens: toNullableNumber(source.totalTokens ?? source.total_tokens),
    reasoningOutputTokens: toNullableNumber(
      source.reasoningOutputTokens ?? source.reasoning_output_tokens
    ),
    estimatedCostUsd: toNullableNumber(
      source.estimatedCostUsd ?? source.estimated_cost_usd
    ),
    durationMs,
    firstResponseMs,
    error: asString(source.error),
    createdAt,
  };
}

/**
 * 函数 `normalizeRequestLogs`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeRequestLogs(payload: unknown): RequestLog[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizeRequestLog(item))
    .filter((item): item is RequestLog => Boolean(item));
}

/**
 * 函数 `normalizeRequestLogListResult`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeRequestLogListResult(payload: unknown): RequestLogListResult {
  const source = asObject(payload);
  const items = normalizeRequestLogs(source.items ?? payload);
  return {
    items,
    total: asInteger(source.total, items.length, 0),
    page: asInteger(source.page, 1, 1),
    pageSize: asInteger(source.pageSize, items.length || 20, 1),
  };
}

export function normalizeRequestLogListWithSummaryResult(
  payload: unknown
): RequestLogListWithSummaryResult {
  const source = asObject(payload);
  return {
    ...normalizeRequestLogListResult(payload),
    summary: normalizeRequestLogFilterSummary(source.summary),
  };
}

/**
 * 函数 `normalizeRequestLogFilterSummary`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeRequestLogFilterSummary(
  payload: unknown
): RequestLogFilterSummary {
  const source = asObject(payload);
  return {
    totalCount: asInteger(source.totalCount, 0, 0),
    filteredCount: asInteger(source.filteredCount, 0, 0),
    successCount: asInteger(source.successCount, 0, 0),
    errorCount: asInteger(source.errorCount, 0, 0),
    totalTokens: asInteger(source.totalTokens, 0, 0),
    totalCostUsd: Math.max(0, toNullableNumber(source.totalCostUsd) ?? 0),
  };
}

/**
 * 函数 `normalizeBackgroundTasks`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeBackgroundTasks(payload: unknown): BackgroundTaskSettings {
  const source = asObject(payload);
  return {
    usagePollingEnabled: asBoolean(
      source.usagePollingEnabled,
      DEFAULT_BACKGROUND_TASKS.usagePollingEnabled
    ),
    usagePollIntervalSecs: asInteger(
      source.usagePollIntervalSecs,
      DEFAULT_BACKGROUND_TASKS.usagePollIntervalSecs,
      1
    ),
    gatewayKeepaliveEnabled: asBoolean(
      source.gatewayKeepaliveEnabled,
      DEFAULT_BACKGROUND_TASKS.gatewayKeepaliveEnabled
    ),
    gatewayKeepaliveIntervalSecs: asInteger(
      source.gatewayKeepaliveIntervalSecs,
      DEFAULT_BACKGROUND_TASKS.gatewayKeepaliveIntervalSecs,
      1
    ),
    tokenRefreshPollingEnabled: asBoolean(
      source.tokenRefreshPollingEnabled,
      DEFAULT_BACKGROUND_TASKS.tokenRefreshPollingEnabled
    ),
    tokenRefreshPollIntervalSecs: asInteger(
      source.tokenRefreshPollIntervalSecs,
      DEFAULT_BACKGROUND_TASKS.tokenRefreshPollIntervalSecs,
      1
    ),
    warmupCronEnabled: asBoolean(
      source.warmupCronEnabled,
      DEFAULT_BACKGROUND_TASKS.warmupCronEnabled
    ),
    warmupCronExpression: asString(
      source.warmupCronExpression,
      DEFAULT_BACKGROUND_TASKS.warmupCronExpression
    ),
    usageRefreshWorkers: asInteger(
      source.usageRefreshWorkers,
      DEFAULT_BACKGROUND_TASKS.usageRefreshWorkers,
      1
    ),
    httpWorkerFactor: asInteger(
      source.httpWorkerFactor,
      DEFAULT_BACKGROUND_TASKS.httpWorkerFactor,
      1
    ),
    httpWorkerMin: asInteger(
      source.httpWorkerMin,
      DEFAULT_BACKGROUND_TASKS.httpWorkerMin,
      1
    ),
    httpStreamWorkerFactor: asInteger(
      source.httpStreamWorkerFactor,
      DEFAULT_BACKGROUND_TASKS.httpStreamWorkerFactor,
      1
    ),
    httpStreamWorkerMin: asInteger(
      source.httpStreamWorkerMin,
      DEFAULT_BACKGROUND_TASKS.httpStreamWorkerMin,
      1
    ),
  };
}

/**
 * 函数 `normalizeEnvOverrideCatalog`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
function clampPercent(value: number | null | undefined, fallback: number): number {
  const parsed = toNullableNumber(value);
  if (parsed == null) return fallback;
  return Math.max(0, Math.min(100, parsed));
}

export function normalizeQuotaGuard(payload: unknown): QuotaGuardSettings {
  const source = asObject(payload);
  return {
    enabled: asBoolean(source.enabled, DEFAULT_QUOTA_GUARD.enabled),
    primaryMinRemainingPercent: clampPercent(
      toNullableNumber(
        source.primaryMinRemainingPercent ??
          source.primary_min_remaining_percent
      ),
      DEFAULT_QUOTA_GUARD.primaryMinRemainingPercent
    ),
    secondaryMinRemainingPercent: clampPercent(
      toNullableNumber(
        source.secondaryMinRemainingPercent ??
          source.secondary_min_remaining_percent
      ),
      DEFAULT_QUOTA_GUARD.secondaryMinRemainingPercent
    ),
    allowAllLowQuotaFallback: asBoolean(
      source.allowAllLowQuotaFallback ?? source.allow_all_low_quota_fallback,
      DEFAULT_QUOTA_GUARD.allowAllLowQuotaFallback
    ),
  };
}

export function normalizeRuntimeTimeZone(payload: unknown): RuntimeTimeZone {
  const source = asObject(payload);
  return {
    name: asString(source.name) || DEFAULT_RUNTIME_TIME_ZONE.name,
    offset: asString(source.offset),
    source: asString(source.source) || DEFAULT_RUNTIME_TIME_ZONE.source,
  };
}

export function normalizeEnvOverrideCatalog(payload: unknown): EnvOverrideCatalogItem[] {
  return asArray(payload).reduce<EnvOverrideCatalogItem[]>((result, item) => {
    const source = asObject(item);
    const key = asString(source.key);
    if (!key) return result;
    result.push({
      key,
      label: asString(source.label) || key,
      defaultValue: asString(source.defaultValue ?? source.default_value),
      scope: asString(source.scope),
      applyMode: asString(source.applyMode ?? source.apply_mode),
      riskLevel: asString(source.riskLevel ?? source.risk_level) || "medium",
      effectScope:
        asString(source.effectScope ?? source.effect_scope) || "runtime-global",
      safetyNote: asString(source.safetyNote ?? source.safety_note),
    });
    return result;
  }, []);
}

/**
 * 函数 `normalizeAppSettings`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeAppSettings(payload: unknown): AppSettings {
  const source = asObject(payload);
  const legacyLightweightMode = asBoolean(
    source.lightweightModeOnCloseToTray,
    false
  );
  const keepWindowUiMounted = asBoolean(
    source.keepWindowUiMounted,
    !legacyLightweightMode
  );
  return {
    updateAutoCheck: asBoolean(source.updateAutoCheck, true),
    removeRequiresOpenaiAuth: asBoolean(source.removeRequiresOpenaiAuth, false),
    autoStartEnabled: asBoolean(source.autoStartEnabled, false),
    autoStartSupported: asBoolean(source.autoStartSupported, false),
    showMainWindowOnStartup: asBoolean(source.showMainWindowOnStartup, true),
    closeToTrayOnClose: asBoolean(source.closeToTrayOnClose, false),
    closeToTraySupported: asBoolean(source.closeToTraySupported, false),
    keepWindowUiMounted,
    lowTransparency: asBoolean(source.lowTransparency, false),
    zoomFactor: Math.min(
      1.25,
      Math.max(0.75, toNullableNumber(source.zoomFactor) ?? 1),
    ),
    lightweightModeOnCloseToTray: !keepWindowUiMounted,
    codexCliGuideDismissed: asBoolean(source.codexCliGuideDismissed, false),
    webAccessPasswordConfigured: asBoolean(
      source.webAccessPasswordConfigured,
      false
    ),
    webAuthMode: asString(source.webAuthMode) || "none",
    webAuthModeOptions: asArray(source.webAuthModeOptions)
      .map((item) => asString(item))
      .filter(Boolean),
    distributionEnabled: asBoolean(source.distributionEnabled, false),
    billingModeLock: readBillingModeLock(source.billingModeLock),
    appUsersConfigured: asBoolean(source.appUsersConfigured, false),
    appUserCount: asInteger(source.appUserCount, 0, 0),
    locale: asString(source.locale) || "zh-CN",
    localeOptions: asArray(source.localeOptions).map((item) => asString(item)).filter(Boolean),
    serviceAddr: asString(source.serviceAddr) || "localhost:48760",
    serviceListenMode: asString(source.serviceListenMode) || "loopback",
    serviceListenModeOptions: asArray(source.serviceListenModeOptions).map((item) =>
      asString(item)
    ),
    routeStrategy: asString(source.routeStrategy) || "ordered",
    routeStrategyOptions: asArray(source.routeStrategyOptions).map((item) =>
      asString(item)
    ),
    freeAccountMaxModel: asString(source.freeAccountMaxModel) || "auto",
    freeAccountMaxModelOptions: asArray(source.freeAccountMaxModelOptions).map((item) =>
      asString(item)
    ),
    modelForwardRules: asString(source.modelForwardRules ?? source.model_forward_rules),
    compactModelForwardRules: asString(
      source.compactModelForwardRules ?? source.compact_model_forward_rules
    ),
    accountMaxInflight: asInteger(source.accountMaxInflight, 1, 0),
    threadAwareAccountDistributionEnabled: asBoolean(
      source.threadAwareAccountDistributionEnabled,
      true
    ),
    quotaGuard: normalizeQuotaGuard(source.quotaGuard ?? source.quota_guard),
    gatewayOriginator:
      asString(source.gatewayOriginator) || DEFAULT_CODEX_ORIGINATOR,
    gatewayOriginatorDefault:
      asString(source.gatewayOriginatorDefault) || DEFAULT_CODEX_ORIGINATOR,
    gatewayUserAgent: asString(
      source.gatewayUserAgent ?? source.gateway_user_agent
    ),
    gatewayUserAgentDefault:
      asString(
        source.gatewayUserAgentDefault ?? source.gateway_user_agent_default
      ) || DEFAULT_CODEX_USER_AGENT,
    gatewayUserAgentVersion:
      asString(source.gatewayUserAgentVersion) || DEFAULT_CODEX_USER_AGENT_VERSION,
    gatewayUserAgentVersionDefault:
      asString(source.gatewayUserAgentVersionDefault) ||
      DEFAULT_CODEX_USER_AGENT_VERSION,
    gatewayResidencyRequirement: asString(source.gatewayResidencyRequirement),
    gatewayResidencyRequirementOptions: asArray(
      source.gatewayResidencyRequirementOptions
    ).map((item) => asString(item)),
    pluginMarketMode: asString(source.pluginMarketMode ?? source.plugin_market_mode) || "builtin",
    pluginMarketSourceUrl: asString(source.pluginMarketSourceUrl ?? source.plugin_market_source_url),
    authorSponsors: normalizeSponsorLinkItems(
      source.authorSponsors,
      DEFAULT_AUTHOR_SPONSORS
    ),
    authorServerRecommendations: normalizeSponsorLinkItems(
      source.authorServerRecommendations,
      DEFAULT_AUTHOR_SERVER_RECOMMENDATIONS
    ),
    upstreamProxyUrl: asString(source.upstreamProxyUrl),
    upstreamProxyBypassHosts: asString(source.upstreamProxyBypassHosts),
    ...normalizeGatewayTransportValues(source),
    backgroundTasks: normalizeBackgroundTasks(source.backgroundTasks),
    runtimeTimeZone: normalizeRuntimeTimeZone(source.runtimeTimeZone),
    envOverrides: normalizeStringRecord(source.envOverrides),
    envOverrideCatalog: normalizeEnvOverrideCatalog(source.envOverrideCatalog),
    envOverrideReservedKeys: asArray(source.envOverrideReservedKeys).map((item) =>
      asString(item)
    ),
    envOverrideUnsupportedKeys: asArray(source.envOverrideUnsupportedKeys).map((item) =>
      asString(item)
    ),
    theme: asString(source.theme) || "tech",
    appearancePreset: asString(source.appearancePreset) || "classic",
  };
}

/**
 * 函数 `normalizeStartupSnapshot`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeStartupSnapshot(payload: unknown): StartupSnapshot {
  const source = asObject(payload);
  const usageSnapshots = normalizeUsageList(source.usageSnapshots);
  const usageMap = buildUsageMap(usageSnapshots);
  const accounts = asArray(source.accounts)
    .map((item) => normalizeAccount(item, usageMap.get(asString(asObject(item).id))))
    .filter((item): item is Account => Boolean(item));

  return {
    accounts,
    accountSummary: normalizeStartupAccountSummary(source.accountSummary ?? source.account_summary),
    usageSnapshots,
    usageAggregateSummary: normalizeUsageAggregateSummary(source.usageAggregateSummary),
    apiKeys: normalizeApiKeyList(source.apiKeys),
    apiModels: normalizeModelCatalog(source.apiModels ?? { models: source.apiModelOptions }),
    manualPreferredAccountId: asString(source.manualPreferredAccountId),
    requestLogTodaySummary: normalizeTodaySummary(source.requestLogTodaySummary),
    requestLogs: normalizeRequestLogs(source.requestLogs),
  };
}
