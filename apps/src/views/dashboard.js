import { state } from "../state";
import { dom } from "../ui/dom";
import {
  calcAvailability,
  computeUsageStats,
  formatCompactNumber,
  formatTs,
} from "../utils/format";
import { buildProgressLine } from "./dashboard-progress";
import { renderRecommendations } from "./dashboard-recommendations";

function toSafeNumber(value, fallback = 0) {
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : fallback;
  }
  if (typeof value === "string") {
    const parsed = Number(value.trim());
    return Number.isFinite(parsed) ? parsed : fallback;
  }
  return fallback;
}

function formatTokenCount(value) {
  const num = Math.max(0, Math.round(toSafeNumber(value, 0)));
  return formatCompactNumber(num, { fallback: "0", maxFractionDigits: 1 });
}

function formatEstimatedCost(value) {
  const num = Math.max(0, toSafeNumber(value, 0));
  return `$${num.toFixed(2)}`;
}

// 渲染仪表盘视图
export function renderDashboard() {
  const usageMap = new Map(
    state.usageList.map((item) => [item.accountId, item]),
  );

  const stats = computeUsageStats(state.accountList, usageMap);
  if (dom.metricTotal) dom.metricTotal.textContent = stats.total;
  if (dom.metricAvailable) dom.metricAvailable.textContent = stats.okCount;
  if (dom.metricUnavailable) dom.metricUnavailable.textContent = stats.unavailableCount;
  if (dom.metricLowQuota) dom.metricLowQuota.textContent = stats.lowCount;
  if (dom.metricTodayTokens) {
    dom.metricTodayTokens.textContent = formatTokenCount(state.requestLogTodaySummary?.todayTokens);
  }
  if (dom.metricCachedInputTokens) {
    dom.metricCachedInputTokens.textContent = formatTokenCount(
      state.requestLogTodaySummary?.cachedInputTokens,
    );
  }
  if (dom.metricReasoningOutputTokens) {
    dom.metricReasoningOutputTokens.textContent = formatTokenCount(
      state.requestLogTodaySummary?.reasoningOutputTokens,
    );
  }
  if (dom.metricTodayCost) {
    dom.metricTodayCost.textContent = formatEstimatedCost(state.requestLogTodaySummary?.estimatedCost);
  }

  renderCurrentAccount(state.accountList, usageMap);
  renderRecommendations(state.accountList, usageMap);
}

function renderCurrentAccount(accounts, usageMap) {
  if (!dom.currentAccountCard) return;
  dom.currentAccountCard.innerHTML = "";
  if (!accounts.length) {
    const empty = document.createElement("div");
    empty.className = "hint";
    empty.textContent = "暂无账号";
    dom.currentAccountCard.appendChild(empty);
    return;
  }
  const account = accounts[0];
  const usage = usageMap.get(account.id);
  const status = calcAvailability(usage);

  const header = document.createElement("div");
  header.className = "panel-header";
  const title = document.createElement("h3");
  title.textContent = "当前账号";
  header.appendChild(title);
  const statusTag = document.createElement("span");
  statusTag.className = "status-tag";
  statusTag.textContent = status.text;
  if (status.level === "ok") statusTag.classList.add("status-ok");
  if (status.level === "warn") statusTag.classList.add("status-warn");
  if (status.level === "bad") statusTag.classList.add("status-bad");
  if (status.level === "unknown") statusTag.classList.add("status-unknown");
  header.appendChild(statusTag);
  dom.currentAccountCard.appendChild(header);

  const summary = document.createElement("div");
  summary.className = "cell";
  const summaryTitle = document.createElement("strong");
  summaryTitle.textContent = account.label || "-";
  const summaryMeta = document.createElement("small");
  summaryMeta.textContent = `${account.id || "-"}`;
  summary.appendChild(summaryTitle);
  summary.appendChild(summaryMeta);
  dom.currentAccountCard.appendChild(summary);

  const usageWrap = document.createElement("div");
  usageWrap.className = "mini-usage";
  usageWrap.appendChild(
    buildProgressLine("5小时", usage ? usage.usedPercent : null, usage?.resetsAt, false),
  );
  usageWrap.appendChild(
    buildProgressLine(
      "7天",
      usage ? usage.secondaryUsedPercent : null,
      usage?.secondaryResetsAt,
      true,
    ),
  );
  dom.currentAccountCard.appendChild(usageWrap);

  const updated = document.createElement("div");
  updated.className = "hint";
  updated.textContent = usage?.capturedAt
    ? `最近刷新 ${formatTs(usage.capturedAt)}`
    : "暂无刷新记录";
  dom.currentAccountCard.appendChild(updated);
}

