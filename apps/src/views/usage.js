import { dom } from "../ui/dom.js";
import { state } from "../state.js";
import {
  formatLimitLabel,
  formatResetLabel,
  remainingPercent,
  parseCredits,
} from "../utils/format.js";

// 查找指定账号的用量快照
export function findUsage(accountId) {
  return state.usageList.find((item) => item.accountId === accountId);
}

// 打开用量弹窗
export function openUsageModal(account) {
  dom.modalUsage.classList.add("active");
  dom.usageTitle.textContent = `用量查询 · ${account.label}`;
  dom.usageDetail.textContent = "刷新中...";
  dom.usageProgress.innerHTML = "";
  state.currentUsageAccount = account;
}

// 关闭用量弹窗
export function closeUsageModal() {
  dom.modalUsage.classList.remove("active");
  state.currentUsageAccount = null;
}

// 更新用量详情展示
export function renderUsageSnapshot(snapshot) {
  dom.usageProgress.innerHTML = "";
  if (!snapshot) {
    dom.usageDetail.textContent = "暂无用量数据。";
    return;
  }

  const primaryRemain = remainingPercent(snapshot.usedPercent);
  const secondaryRemain = remainingPercent(snapshot.secondaryUsedPercent);
  const primaryLevel =
    primaryRemain != null && primaryRemain <= 5
      ? "danger"
      : primaryRemain != null && primaryRemain <= 15
        ? "warn"
        : "";
  const secondaryLevel =
    secondaryRemain != null && secondaryRemain <= 5
      ? "danger"
      : secondaryRemain != null && secondaryRemain <= 15
        ? "warn"
        : "";

  const primaryLabel = formatLimitLabel(snapshot.windowMinutes, "5小时用量");
  const secondaryLabel = formatLimitLabel(
    snapshot.secondaryWindowMinutes,
    "7天用量",
  );

  dom.usageProgress.appendChild(
    renderProgressRow(primaryLabel, primaryRemain, snapshot.resetsAt, primaryLevel),
  );
  dom.usageProgress.appendChild(
    renderProgressRow(
      secondaryLabel,
      secondaryRemain,
      snapshot.secondaryResetsAt,
      secondaryLevel,
    ),
  );

  const credits = parseCredits(snapshot.creditsJson);
  if (credits && credits.balance != null) {
    dom.usageDetail.textContent = `Credits: ${credits.balance} (${credits.unlimited ? "unlimited" : "limited"})`;
  } else {
    dom.usageDetail.textContent = "已刷新";
  }
}

function renderProgressRow(label, percent, resetsAt, level) {
  const row = document.createElement("div");
  row.className = "progress-row";
  const rowLabel = document.createElement("div");
  rowLabel.className = "progress-label";
  const left = document.createElement("span");
  left.textContent = label;
  const right = document.createElement("span");
  right.textContent = percent == null ? "n/a" : `${percent}% left`;
  rowLabel.appendChild(left);
  rowLabel.appendChild(right);
  const track = document.createElement("div");
  track.className = "progress-track";
  const fill = document.createElement("div");
  fill.className = "progress-fill";
  if (level === "warn") fill.classList.add("warn");
  if (level === "danger") fill.classList.add("danger");
  fill.style.width = percent == null ? "0%" : `${percent}%`;
  track.appendChild(fill);
  row.appendChild(rowLabel);
  row.appendChild(track);
  if (resetsAt) {
    const reset = document.createElement("div");
    reset.className = "hint";
    reset.textContent = formatResetLabel(resetsAt);
    row.appendChild(reset);
  }
  return row;
}
