import assert from "node:assert/strict";
import test from "node:test";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const appsRoot = path.resolve(__dirname, "..");

test("request logs page wiring present", () => {
  const indexHtml = fs.readFileSync(path.join(appsRoot, "index.html"), "utf8");
  const domJs = fs.readFileSync(path.join(appsRoot, "src", "ui", "dom.js"), "utf8");
  const mainJs = fs.readFileSync(path.join(appsRoot, "src", "main.js"), "utf8");
  const apiJs = fs.readFileSync(path.join(appsRoot, "src", "api.js"), "utf8");

  assert(indexHtml.includes('id="navRequestLogs"'), "index.html missing navRequestLogs button");
  assert(indexHtml.includes('id="pageRequestLogs"'), "index.html missing pageRequestLogs section");
  assert(indexHtml.includes('id="requestLogRows"'), "index.html missing requestLogRows table body");
  assert(indexHtml.includes('id="filterLogAll"'), "index.html missing filterLogAll button");
  assert(indexHtml.includes('id="filterLog2xx"'), "index.html missing filterLog2xx button");
  assert(indexHtml.includes('id="filterLog4xx"'), "index.html missing filterLog4xx button");
  assert(indexHtml.includes('id="filterLog5xx"'), "index.html missing filterLog5xx button");
  assert(indexHtml.includes('id="clearRequestLogs"'), "index.html missing clearRequestLogs button");
  assert(domJs.includes("navRequestLogs"), "dom.js missing navRequestLogs mapping");
  assert(domJs.includes("requestLogRows"), "dom.js missing requestLogRows mapping");
  assert(domJs.includes("filterLogAll"), "dom.js missing filterLogAll mapping");
  assert(domJs.includes("clearRequestLogs"), "dom.js missing clearRequestLogs mapping");
  assert(mainJs.includes("refreshRequestLogs"), "main.js missing refreshRequestLogs integration");
  assert(mainJs.includes("refreshRequestLogTodaySummary"), "main.js missing today summary refresh integration");
  assert(apiJs.includes("service_requestlog_today_summary"), "api.js missing requestlog today summary rpc");
  assert(mainJs.includes("handleClearRequestLogs"), "main.js missing clear request logs handler");
});
