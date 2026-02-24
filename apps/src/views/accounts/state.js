import { calcAvailability, remainingPercent } from "../../utils/format.js";

export function normalizeGroupName(value) {
  return String(value || "").trim();
}

export function buildGroupFilterOptions(accounts) {
  const list = Array.isArray(accounts) ? accounts : [];
  const counter = new Map();

  for (const account of list) {
    const group = normalizeGroupName(account && account.groupName);
    if (!group) continue;
    counter.set(group, (counter.get(group) || 0) + 1);
  }

  const dynamicGroups = Array.from(counter.entries())
    .sort((left, right) => left[0].localeCompare(right[0], "zh-Hans-CN"))
    .map(([value, count]) => ({
      value,
      label: value,
      count,
    }));

  return [
    {
      value: "all",
      label: "全部分组",
      count: list.length,
    },
    ...dynamicGroups,
  ];
}

function isDerivedAccountValue(value) {
  return Boolean(
    value &&
      typeof value === "object" &&
      Object.prototype.hasOwnProperty.call(value, "status") &&
      Object.prototype.hasOwnProperty.call(value, "primaryRemain") &&
      Object.prototype.hasOwnProperty.call(value, "secondaryRemain"),
  );
}

function toUsageMap(usageSource) {
  if (usageSource instanceof Map) {
    return usageSource;
  }
  return new Map((usageSource || []).map((item) => [item.accountId, item]));
}

export function buildAccountDerivedMap(accounts, usageSource) {
  const usageMap = toUsageMap(usageSource);
  const derived = new Map();
  for (const account of accounts || []) {
    const usage = usageMap.get(account.id);
    derived.set(account.id, {
      usage,
      primaryRemain: remainingPercent(usage ? usage.usedPercent : null),
      secondaryRemain: remainingPercent(
        usage ? usage.secondaryUsedPercent : null,
      ),
      status: calcAvailability(usage),
    });
  }
  return derived;
}

function toDerivedMap(accounts, usageSource) {
  if (usageSource instanceof Map) {
    const iterator = usageSource.values().next();
    if (!iterator.done && isDerivedAccountValue(iterator.value)) {
      return usageSource;
    }
  }
  return buildAccountDerivedMap(accounts, usageSource);
}

export function filterAccounts(accounts, usageSource, query, filter, groupFilter = "all") {
  const keyword = String(query || "").trim().toLowerCase();
  const normalizedGroupFilter = normalizeGroupName(groupFilter) || "all";
  const derivedMap = toDerivedMap(accounts, usageSource);

  return (accounts || []).filter((account) => {
    if (keyword) {
      const label = String(account.label || "").toLowerCase();
      const id = String(account.id || "").toLowerCase();
      if (!label.includes(keyword) && !id.includes(keyword)) return false;
    }

    if (normalizedGroupFilter !== "all") {
      const accountGroup = normalizeGroupName(account.groupName);
      if (accountGroup !== normalizedGroupFilter) return false;
    }

    if (filter === "active" || filter === "low") {
      const derived = derivedMap.get(account.id);
      if (filter === "active" && derived?.status?.level !== "ok") return false;
      if (
        filter === "low" &&
        !(
          (derived?.primaryRemain != null && derived.primaryRemain <= 20) ||
          (derived?.secondaryRemain != null && derived.secondaryRemain <= 20)
        )
      ) {
        return false;
      }
    }
    return true;
  });
}
