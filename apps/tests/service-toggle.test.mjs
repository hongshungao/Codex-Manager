import { readFileSync } from "node:fs";
import { strict as assert } from "node:assert";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const projectRoot = join(here, "..");

const indexHtml = readFileSync(join(projectRoot, "index.html"), "utf8");
const domJs = readFileSync(join(projectRoot, "src", "ui", "dom.js"), "utf8");
const mainJs = readFileSync(join(projectRoot, "src", "main.js"), "utf8");
const serviceLifecycleJs = readFileSync(
  join(projectRoot, "src", "services", "service-lifecycle.js"),
  "utf8",
);
const stateJs = readFileSync(join(projectRoot, "src", "state.js"), "utf8");

assert(indexHtml.includes('id="serviceToggle"'), "index.html missing serviceToggle button");
assert(!indexHtml.includes("serviceStart"), "index.html should not contain serviceStart button");
assert(!indexHtml.includes("serviceStop"), "index.html should not contain serviceStop button");
assert(domJs.includes("serviceToggle"), "dom.js missing serviceToggle mapping");
assert(
  mainJs.includes("serviceToggle") || serviceLifecycleJs.includes("serviceToggle"),
  "service toggle usage missing in main/service lifecycle",
);
assert(stateJs.includes("serviceBusy"), "state.js missing serviceBusy state");
