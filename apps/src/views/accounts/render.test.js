import test from "node:test";
import assert from "node:assert/strict";

import { handleAccountRowsChange, handleAccountRowsClick } from "./render.js";

function createAccountActionTarget(action, accountId) {
  const row = {
    dataset: { accountId },
  };
  const button = {
    dataset: { action },
    closest(selector) {
      if (selector === "tr[data-account-id]") return row;
      return null;
    },
  };
  return {
    closest(selector) {
      if (selector === "button[data-action]") return button;
      return null;
    },
  };
}

function createSortChangeTarget(accountId, value) {
  const row = {
    dataset: { accountId },
  };
  const input = {
    value,
    dataset: { originSort: String(value) },
    closest(selector) {
      if (selector === "tr[data-account-id]") return row;
      return null;
    },
  };
  return {
    closest(selector) {
      if (selector === "input[data-field='sort']") return input;
      return null;
    },
  };
}

test("handleAccountRowsClick delegates open usage action by account id", () => {
  const account = { id: "acc-1", label: "main" };
  const lookup = new Map([[account.id, account]]);
  let opened = null;
  const handled = handleAccountRowsClick(
    createAccountActionTarget("open-usage", "acc-1"),
    {
      onOpenUsage: (item) => {
        opened = item;
      },
    },
    lookup,
  );
  assert.equal(handled, true);
  assert.deepEqual(opened, account);
});

test("handleAccountRowsClick delegates delete action by account id", () => {
  const account = { id: "acc-2", label: "backup" };
  const lookup = new Map([[account.id, account]]);
  let deleted = null;
  const handled = handleAccountRowsClick(
    createAccountActionTarget("delete", "acc-2"),
    {
      onDelete: (item) => {
        deleted = item;
      },
    },
    lookup,
  );
  assert.equal(handled, true);
  assert.deepEqual(deleted, account);
});

test("handleAccountRowsChange delegates sort change with numeric value", () => {
  let payload = null;
  const target = createSortChangeTarget("acc-3", "42");
  target.closest("input[data-field='sort']").dataset.originSort = "7";
  const handled = handleAccountRowsChange(target, {
    onUpdateSort: (accountId, sort) => {
      payload = { accountId, sort };
    },
  });
  assert.equal(handled, true);
  assert.deepEqual(payload, { accountId: "acc-3", sort: 42 });
});

test("handleAccountRowsChange skips unchanged sort value", () => {
  let called = false;
  const handled = handleAccountRowsChange(createSortChangeTarget("acc-4", "5"), {
    onUpdateSort: () => {
      called = true;
    },
  });
  assert.equal(handled, false);
  assert.equal(called, false);
});
