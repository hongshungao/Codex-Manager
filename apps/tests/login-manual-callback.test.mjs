import { readFileSync } from "node:fs";
import { strict as assert } from "node:assert";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const projectRoot = join(here, "..");

const indexHtml = readFileSync(join(projectRoot, "index.html"), "utf8");
const domJs = readFileSync(join(projectRoot, "src", "ui", "dom.js"), "utf8");
const mainJs = readFileSync(join(projectRoot, "src", "main.js"), "utf8");
const loginFlowJs = readFileSync(join(projectRoot, "src", "services", "login-flow.js"), "utf8");
const apiJs = readFileSync(join(projectRoot, "src", "api.js"), "utf8");

assert(
  indexHtml.includes('id="manualCallbackUrl"'),
  "index.html missing manualCallbackUrl input",
);
assert(
  indexHtml.includes('id="manualCallbackSubmit"'),
  "index.html missing manualCallbackSubmit button",
);
assert(domJs.includes("manualCallbackUrl"), "dom.js missing manualCallbackUrl");
assert(
  domJs.includes("manualCallbackSubmit"),
  "dom.js missing manualCallbackSubmit",
);
assert(
  mainJs.includes("handleManualCallback") || loginFlowJs.includes("handleManualCallback"),
  "manual callback handler missing in main.js/login-flow.js",
);
assert(
  apiJs.includes("serviceLoginComplete"),
  "api.js missing serviceLoginComplete",
);
