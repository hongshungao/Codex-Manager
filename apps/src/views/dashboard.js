import { state } from "../state";
import { dom } from "../ui/dom";
import {
  calcAvailability,
  computeUsageStats,
  formatResetLabel,
  formatTs,
  remainingPercent,
} from "../utils/format";

// 渲染仪表盘视图
export function renderDashboard() {
  let okCount = 0;
  let warnCount = 0;
  let badCount = 0;

  const usageMap = new Map(
    state.usageList.map((item) => [item.accountId, item]),
  );

  state.accountList.forEach((account) => {
    const usage = usageMap.get(account.id);
    const status = calcAvailability(usage);
    if (status.level === "ok") okCount += 1;
    if (status.level === "warn") warnCount += 1;
    if (status.level === "bad") badCount += 1;
  });

  const stats = computeUsageStats(state.accountList, state.usageList);
  if (dom.metricTotal) dom.metricTotal.textContent = stats.total;
  if (dom.metricAvailable) dom.metricAvailable.textContent = okCount;
  if (dom.metricUnavailable) dom.metricUnavailable.textContent = warnCount + badCount;
  if (dom.metricLowQuota) dom.metricLowQuota.textContent = stats.lowCount;

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
  const workspaceLabel = account.workspaceName ? ` · ${account.workspaceName}` : "";
  const summaryTitle = document.createElement("strong");
  summaryTitle.textContent = account.label || "-";
  const summaryMeta = document.createElement("small");
  summaryMeta.textContent = `${account.id || "-"}${workspaceLabel}`;
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

function renderRecommendations(accounts, usageMap) {
  if (!dom.recommendations) return;
  dom.recommendations.innerHTML = "";
  const header = document.createElement("div");
  header.className = "panel-header";
  const title = document.createElement("h3");
  title.textContent = "最佳账号推荐";
  const hint = document.createElement("span");
  hint.className = "hint";
  hint.textContent = "按剩余额度";
  header.appendChild(title);
  header.appendChild(hint);
  dom.recommendations.appendChild(header);

  if (!accounts.length) {
    const empty = document.createElement("div");
    empty.className = "hint";
    empty.textContent = "暂无可推荐账号";
    dom.recommendations.appendChild(empty);
    return;
  }

  const list = document.createElement("div");
  list.className = "mini-usage";

  const primaryPick = pickBest(accounts, usageMap, false);
  const secondaryPick = pickBest(accounts, usageMap, true);
  list.appendChild(
    renderRecommendationItem("用于 5小时", primaryPick?.account, primaryPick?.remain),
  );
  list.appendChild(
    renderRecommendationItem("用于 7天", secondaryPick?.account, secondaryPick?.remain),
  );

  dom.recommendations.appendChild(list);
}

function pickBest(accounts, usageMap, secondary) {
  const ranked = accounts
    .map((account) => {
      const usage = usageMap.get(account.id);
      const remain = remainingPercent(
        usage ? (secondary ? usage.secondaryUsedPercent : usage.usedPercent) : null,
      );
      return { account, remain };
    })
    .filter((item) => item.remain != null)
    .sort((a, b) => (b.remain ?? 0) - (a.remain ?? 0));
  return ranked[0] || null;
}

function buildProgressLine(label, usedPercent, resetsAt, secondary) {
  const remain = remainingPercent(usedPercent);
  const line = document.createElement("div");
  line.className = "progress-line";
  if (secondary) line.classList.add("secondary");
  const lineLabel = document.createElement("span");
  lineLabel.textContent = `${label} ${remain == null ? "--" : `${remain}%`}`;
  const track = document.createElement("div");
  track.className = "track";
  const fill = document.createElement("div");
  fill.className = "fill";
  fill.style.width = remain == null ? "0%" : `${remain}%`;
  track.appendChild(fill);
  line.appendChild(lineLabel);
  line.appendChild(track);

  const wrap = document.createElement("div");
  wrap.appendChild(line);
  if (resetsAt) {
    const reset = document.createElement("div");
    reset.className = "hint";
    reset.textContent = formatResetLabel(resetsAt);
    wrap.appendChild(reset);
  }
  return wrap;
}

function renderRecommendationItem(label, account, remain) {
  const item = document.createElement("div");
  item.className = "cell";
  const itemLabel = document.createElement("small");
  itemLabel.textContent = label;
  item.appendChild(itemLabel);
  if (!account) {
    const empty = document.createElement("strong");
    empty.textContent = "暂无账号";
    item.appendChild(empty);
    return item;
  }
  const accountLabel = document.createElement("strong");
  accountLabel.textContent = account.label || "-";
  const accountId = document.createElement("small");
  accountId.textContent = account.id || "-";
  item.appendChild(accountLabel);
  item.appendChild(accountId);
  const badge = document.createElement("span");
  badge.className = "status-tag status-ok";
  badge.textContent = remain == null ? "--" : `${remain}%`;
  item.appendChild(badge);
  return item;
}
