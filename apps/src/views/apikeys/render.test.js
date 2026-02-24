import test from "node:test";
import assert from "node:assert/strict";

import { handleApiKeyRowsChange, handleApiKeyRowsClick } from "./render.js";

function createApiKeyActionTarget(action, keyId) {
  const row = {
    dataset: { keyId },
  };
  const button = {
    dataset: { action },
    closest(selector) {
      if (selector === "tr[data-key-id]") return row;
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

function createApiKeyChangeTarget({ keyId, field, modelValue, effortValue }) {
  const row = {
    dataset: { keyId },
    querySelector(selector) {
      if (selector === 'select[data-field="model"]') return modelSelect;
      if (selector === 'select[data-field="reasoning"]') return effortSelect;
      return null;
    },
  };

  const modelSelect = {
    dataset: { field: "model" },
    value: modelValue,
    closest(selector) {
      if (selector === "tr[data-key-id]") return row;
      return null;
    },
  };
  const effortSelect = {
    dataset: { field: "reasoning" },
    value: effortValue,
    disabled: false,
    closest(selector) {
      if (selector === "tr[data-key-id]") return row;
      return null;
    },
  };
  const changedSelect = field === "model" ? modelSelect : effortSelect;
  const target = {
    closest(selector) {
      if (selector === "select[data-field]") return changedSelect;
      return null;
    },
  };
  return { target, modelSelect, effortSelect };
}

test("handleApiKeyRowsClick delegates toggle action", () => {
  const item = { id: "k-1", status: "active" };
  const lookup = new Map([[item.id, item]]);
  let toggled = null;
  const handled = handleApiKeyRowsClick(
    createApiKeyActionTarget("toggle", "k-1"),
    {
      onToggleStatus: (value) => {
        toggled = value;
      },
    },
    lookup,
  );
  assert.equal(handled, true);
  assert.deepEqual(toggled, item);
});

test("handleApiKeyRowsChange syncs reasoning when model override cleared", () => {
  const item = { id: "k-2" };
  const lookup = new Map([[item.id, item]]);
  const { target, effortSelect } = createApiKeyChangeTarget({
    keyId: "k-2",
    field: "model",
    modelValue: "",
    effortValue: "high",
  });
  let payload = null;
  const handled = handleApiKeyRowsChange(
    target,
    {
      onUpdateModel: (it, modelSlug, reasoningEffort) => {
        payload = { it, modelSlug, reasoningEffort };
      },
    },
    lookup,
  );
  assert.equal(handled, true);
  assert.equal(effortSelect.disabled, true);
  assert.equal(effortSelect.value, "");
  assert.deepEqual(payload, { it: item, modelSlug: "", reasoningEffort: "" });
});

test("handleApiKeyRowsChange delegates reasoning change", () => {
  const item = { id: "k-3" };
  const lookup = new Map([[item.id, item]]);
  const { target, effortSelect } = createApiKeyChangeTarget({
    keyId: "k-3",
    field: "reasoning",
    modelValue: "gpt-4o",
    effortValue: "medium",
  });
  let payload = null;
  const handled = handleApiKeyRowsChange(
    target,
    {
      onUpdateModel: (it, modelSlug, reasoningEffort) => {
        payload = { it, modelSlug, reasoningEffort };
      },
    },
    lookup,
  );
  assert.equal(handled, true);
  assert.equal(effortSelect.disabled, false);
  assert.deepEqual(payload, {
    it: item,
    modelSlug: "gpt-4o",
    reasoningEffort: "medium",
  });
});
