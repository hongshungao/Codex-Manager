const dateTimeFormatter = new Intl.DateTimeFormat(undefined, {
  year: "numeric",
  month: "numeric",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
  second: "2-digit",
});

const monthFormatterZh = new Intl.DateTimeFormat("zh-CN", {
  month: "numeric",
});

function formatDateTime(date) {
  return dateTimeFormatter.format(date);
}

// 时间与用量展示相关工具函数
export function formatTs(ts, options = {}) {
  const emptyLabel = options.emptyLabel || "未知";
  if (!ts) return emptyLabel;
  const date = new Date(ts * 1000);
  if (Number.isNaN(date.getTime())) return emptyLabel;
  return formatDateTime(date);
}

export function formatLimitLabel(windowMinutes, fallback) {
  if (windowMinutes == null) return fallback;
  const minutes = Math.max(0, windowMinutes);
  const MINUTES_PER_HOUR = 60;
  const MINUTES_PER_DAY = 24 * MINUTES_PER_HOUR;
  const MINUTES_PER_WEEK = 7 * MINUTES_PER_DAY;
  const MINUTES_PER_MONTH = 30 * MINUTES_PER_DAY;
  const ROUNDING_BIAS = 3;
  if (minutes <= MINUTES_PER_DAY + ROUNDING_BIAS) {
    const hours = Math.max(
      1,
      Math.floor((minutes + ROUNDING_BIAS) / MINUTES_PER_HOUR),
    );
    return `${hours}小时用量`;
  }
  if (minutes <= MINUTES_PER_WEEK + ROUNDING_BIAS) return "7天用量";
  if (minutes <= MINUTES_PER_MONTH + ROUNDING_BIAS) return "30天用量";
  return "年度用量";
}

export function formatResetLabel(ts) {
  if (!ts) return "重置：--";
  const date = new Date(ts * 1000);
  if (Number.isNaN(date.getTime())) return "重置：--";
  const now = new Date();
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  if (sameDay) {
    return `重置：${hh}:${mm}`;
  }
  const day = date.getDate();
  const month = monthFormatterZh.format(date);
  return `重置：${month}月${day}日 ${hh}:${mm}`;
}

export function calcAvailability(usage) {
  if (!usage) return { text: "未知", level: "unknown" };
  const secondary = usage.secondaryUsedPercent;
  const primary = usage.usedPercent;
  const primaryMissing = primary == null || usage.windowMinutes == null;
  const secondaryMissing =
    secondary == null || usage.secondaryWindowMinutes == null;
  if (primaryMissing || secondaryMissing) {
    return { text: "用量缺失", level: "bad" };
  }
  if (secondary != null && secondary >= 100) {
    return { text: "7日已用尽", level: "bad" };
  }
  if (primary != null && primary >= 100) {
    return { text: "5小时已用尽", level: "warn" };
  }
  return { text: "可用", level: "ok" };
}

function normalizePercent(value) {
  if (value == null) return null;
  return Math.max(0, Math.min(100, value));
}

export function remainingPercent(value) {
  const used = normalizePercent(value);
  if (used == null) return null;
  return Math.max(0, 100 - used);
}

export function computeUsageStats(accounts, usageSource) {
  const usageMap = usageSource instanceof Map
    ? usageSource
    : new Map((usageSource || []).map((u) => [u.accountId, u]));
  let total = 0;
  let okCount = 0;
  let unavailableCount = 0;
  let lowCount = 0;

  (accounts || []).forEach((acc) => {
    total += 1;
    const usage = usageMap.get(acc.id);
    const status = calcAvailability(usage);
    if (status.level === "ok") okCount += 1;
    if (status.level === "warn" || status.level === "bad") unavailableCount += 1;
    const primaryRemain = remainingPercent(usage ? usage.usedPercent : null);
    const secondaryRemain = remainingPercent(
      usage ? usage.secondaryUsedPercent : null,
    );
    if (
      (primaryRemain != null && primaryRemain <= 20) ||
      (secondaryRemain != null && secondaryRemain <= 20)
    ) {
      lowCount += 1;
    }
  });

  return {
    total,
    okCount,
    unavailableCount,
    lowCount,
  };
}

export function parseCredits(raw) {
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch (err) {
    return null;
  }
}

