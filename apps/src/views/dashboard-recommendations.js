import { dom } from "../ui/dom.js";
import { remainingPercent } from "../utils/format.js";

export function renderRecommendations(accounts, usageMap) {
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

  const { primaryPick, secondaryPick } = pickBestRecommendations(accounts, usageMap);
  list.appendChild(
    renderRecommendationItem("用于 5小时", primaryPick?.account, primaryPick?.remain),
  );
  list.appendChild(
    renderRecommendationItem("用于 7天", secondaryPick?.account, secondaryPick?.remain),
  );

  dom.recommendations.appendChild(list);
}

export function pickBestRecommendations(accounts, usageMap) {
  let primaryPick = null;
  let secondaryPick = null;

  (accounts || []).forEach((account) => {
    const usage = usageMap.get(account.id);
    const primaryRemain = remainingPercent(usage ? usage.usedPercent : null);
    const secondaryRemain = remainingPercent(
      usage ? usage.secondaryUsedPercent : null,
    );

    if (primaryRemain != null && (!primaryPick || primaryRemain > primaryPick.remain)) {
      primaryPick = { account, remain: primaryRemain };
    }
    if (secondaryRemain != null && (!secondaryPick || secondaryRemain > secondaryPick.remain)) {
      secondaryPick = { account, remain: secondaryRemain };
    }
  });

  return { primaryPick, secondaryPick };
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
