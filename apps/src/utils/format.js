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

const COMPACT_NUMBER_UNITS = [
  { value: 1e18, suffix: "E" },
  { value: 1e15, suffix: "P" },
  { value: 1e12, suffix: "T" },
  { value: 1e9, suffix: "B" },
  { value: 1e6, suffix: "M" },
  { value: 1e3, suffix: "K" },
];

function formatDateTime(date) {
  return dateTimeFormatter.format(date);
}

function trimTrailingZeros(text) {
  return String(text)
    .replace(/\.0+$/, "")
    .replace(/(\.\d*[1-9])0+$/, "$1");
}

function parseFiniteNumber(value) {
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

export function resolveUsageWindows(usage) {
  const primaryUsed = parseFiniteNumber(usage?.usedPercent);
  const primaryWindow = parseFiniteNumber(usage?.windowMinutes);
  const secondaryUsed = parseFiniteNumber(usage?.secondaryUsedPercent);
  const secondaryWindow = parseFiniteNumber(usage?.secondaryWindowMinutes);
  const normalizedStatus = String(usage?.availabilityStatus || "").trim().toLowerCase();
  const accountId = String(usage?.accountId || "").toLowerCase();
  const forceSingleWindow = normalizedStatus === "primary_window_available_only"
    || accountId.includes("::free");

  const hasPrimaryWindow = !forceSingleWindow && primaryUsed != null && primaryWindow != null;
  const hasSecondaryWindow = secondaryUsed != null || secondaryWindow != null;

  return {
    normalizedStatus,
    forceSingleWindow,
    hasPrimaryWindow,
    hasSecondaryWindow,
    primaryUsed,
    primaryWindow,
    secondaryUsed,
    secondaryWindow,
  };
}

// 时间与用量展示相关工具函数
export function formatTs(ts, options = {}) {
  const emptyLabel = options.emptyLabel || "未知";
  if (!ts) return emptyLabel;
  const date = new Date(ts * 1000);
  if (Number.isNaN(date.getTime())) return emptyLabel;
  return formatDateTime(date);
}

export function formatCompactNumber(value, options = {}) {
  const fallback = options.fallback == null ? "-" : String(options.fallback);
  const parsed = parseFiniteNumber(value);
  if (parsed == null) return fallback;

  const normalized = Math.max(0, parsed);
  if (normalized < 1000) {
    return `${Math.round(normalized)}`;
  }

  const maxFractionDigits = Number.isFinite(options.maxFractionDigits)
    ? Math.max(0, Math.min(6, Math.floor(options.maxFractionDigits)))
    : 1;

  for (let i = 0; i < COMPACT_NUMBER_UNITS.length; i += 1) {
    const unit = COMPACT_NUMBER_UNITS[i];
    if (normalized < unit.value) continue;
    const scaled = normalized / unit.value;
    const fixed = trimTrailingZeros(scaled.toFixed(maxFractionDigits));
    return `${fixed}${unit.suffix}`;
  }

  return `${Math.round(normalized)}`;
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
  const windows = resolveUsageWindows(usage);
  const normalizedStatus = windows.normalizedStatus;
  if (normalizedStatus) {
    if (normalizedStatus === "available") {
      return { text: "可用", level: "ok" };
    }
    if (normalizedStatus === "primary_window_available_only") {
      return { text: "单窗口可用", level: "ok" };
    }
    if (normalizedStatus === "unavailable") {
      return { text: "不可用", level: "bad" };
    }
    if (normalizedStatus === "unknown") {
      return { text: "未知", level: "unknown" };
    }
  }
  const primary = windows.primaryUsed;
  const secondary = windows.secondaryUsed;
  const secondaryWindow = windows.secondaryWindow;
  const primaryMissing = !windows.hasPrimaryWindow;
  const secondaryPresent = windows.hasSecondaryWindow;
  const secondaryMissing = secondary == null || secondaryWindow == null;
  if (primaryMissing && windows.forceSingleWindow && !secondaryMissing) {
    if (secondary >= 100) {
      return { text: "7日已用尽", level: "bad" };
    }
    return { text: "单窗口可用", level: "ok" };
  }
  if (primaryMissing) {
    return { text: "用量缺失", level: "bad" };
  }
  if (primary != null && primary >= 100) {
    return { text: "5小时已用尽", level: "warn" };
  }
  if (!secondaryPresent) {
    return { text: "单窗口可用", level: "ok" };
  }
  if (secondaryMissing) {
    return { text: "用量缺失", level: "bad" };
  }
  if (secondary != null && secondary >= 100) {
    return { text: "7日已用尽", level: "bad" };
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

