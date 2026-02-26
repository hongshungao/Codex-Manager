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
assert(indexHtml.includes('value="azure_openai"'), "index.html missing azure_openai protocol option");
assert(indexHtml.includes('id="inputApiKeyEndpoint"'), "index.html missing inputApiKeyEndpoint field");
assert(indexHtml.includes('id="inputApiKeyAzureApiKey"'), "index.html missing inputApiKeyAzureApiKey field");
assert(domJs.includes("navApiKeys"), "dom.js missing navApiKeys mapping");
assert(domJs.includes("pageApiKeys"), "dom.js missing pageApiKeys mapping");
assert(domJs.includes("inputApiKeyEndpoint"), "dom.js missing inputApiKeyEndpoint mapping");
assert(domJs.includes("inputApiKeyAzureApiKey"), "dom.js missing inputApiKeyAzureApiKey mapping");
assert(mainJs.includes("apikeys"), "main.js missing apikeys page switch");
