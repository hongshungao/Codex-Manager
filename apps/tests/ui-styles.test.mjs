import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const stylesRoot = path.resolve(__dirname, "..", "src", "styles");
const baseCss = fs.readFileSync(path.join(stylesRoot, "base.css"), "utf8");
const compCss = fs.readFileSync(path.join(stylesRoot, "components.css"), "utf8");

const mustHave = [
  "--surface",
  "--ink-strong",
  ".top-nav",
  ".pill-nav",
  ".stat-card",
  ".accounts-toolbar",
];

test("ui styles include new tokens and components", () => {
  for (const token of mustHave) {
    assert.ok(baseCss.includes(token) || compCss.includes(token), `missing ${token}`);
  }
});
