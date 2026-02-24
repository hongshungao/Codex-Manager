import test from "node:test";
import assert from "node:assert/strict";

import { state } from "../../state.js";
import { dom } from "../../ui/dom.js";
import { populateApiKeyModelSelect } from "./actions.js";

class FakeSelect {
  constructor() {
    this._innerHTML = "";
    this.children = [];
    this.appendCount = 0;
    this.clearCount = 0;
  }

  set innerHTML(value) {
    this._innerHTML = value;
    this.children = [];
    this.clearCount += 1;
  }

  get innerHTML() {
    return this._innerHTML;
  }

  appendChild(node) {
    this.children.push(node);
    this.appendCount += 1;
    return node;
  }
}

test("populateApiKeyModelSelect only rebuilds model options when signature changes", () => {
  const previousDocument = globalThis.document;
  const previousModelSelect = dom.inputApiKeyModel;
  const previousReasoningSelect = dom.inputApiKeyReasoning;
  const previousModelOptions = state.apiModelOptions;

  globalThis.document = {
    createElement() {
      return { value: "", textContent: "" };
    },
  };

  const modelSelect = new FakeSelect();
  const reasoningSelect = new FakeSelect();
  dom.inputApiKeyModel = modelSelect;
  dom.inputApiKeyReasoning = reasoningSelect;
  state.apiModelOptions = [{ slug: "model-a", displayName: "Model A" }];

  try {
    populateApiKeyModelSelect({ force: true });
    const modelAppendAfterFirst = modelSelect.appendCount;
    const reasoningAppendAfterFirst = reasoningSelect.appendCount;
    assert.ok(modelAppendAfterFirst > 0);
    assert.ok(reasoningAppendAfterFirst > 0);

    populateApiKeyModelSelect();
    assert.equal(modelSelect.appendCount, modelAppendAfterFirst);
    assert.equal(reasoningSelect.appendCount, reasoningAppendAfterFirst);

    state.apiModelOptions = [
      { slug: "model-a", displayName: "Model A" },
      { slug: "model-b", displayName: "Model B" },
    ];
    populateApiKeyModelSelect();
    assert.ok(modelSelect.appendCount > modelAppendAfterFirst);
  } finally {
    globalThis.document = previousDocument;
    dom.inputApiKeyModel = previousModelSelect;
    dom.inputApiKeyReasoning = previousReasoningSelect;
    state.apiModelOptions = previousModelOptions;
  }
});
