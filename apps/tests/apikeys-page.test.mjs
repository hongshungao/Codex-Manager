import { readFileSync } from "node:fs";
import { strict as assert } from "node:assert";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const projectRoot = join(here, "..");

const indexHtml = readFileSync(join(projectRoot, "index.html"), "utf8");
const domJs = readFileSync(join(projectRoot, "src", "ui", "dom.js"), "utf8");
const mainJs = readFileSync(join(projectRoot, "src", "main.js"), "utf8");

assert(indexHtml.includes('id="navApiKeys"'), "index.html missing navApiKeys button");
assert(indexHtml.includes('id="pageApiKeys"'), "index.html missing pageApiKeys section");
assert(domJs.includes("navApiKeys"), "dom.js missing navApiKeys mapping");
assert(domJs.includes("pageApiKeys"), "dom.js missing pageApiKeys mapping");
assert(mainJs.includes("apikeys"), "main.js missing apikeys page switch");
