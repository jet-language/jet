#!/usr/bin/env node
import { CanvasScenario, scenarios } from "./scenario.mjs";
import os from "node:os";
import path from "node:path";

function arg(name) {
  const idx = process.argv.indexOf(name);
  return idx >= 0 ? process.argv[idx + 1] : null;
}

const scenarioName = arg("--scenario");
const port = Number(arg("--port") || process.env.JET_CANVAS_PORT || "0");
// card 1640: nothing writes inside target/ — default screenshots to temp.
const outDir = arg("--out-dir") || process.env.JET_CANVAS_OUT_DIR
  || path.join(os.tmpdir(), "jet-canvas-screenshots");
const seed = Number(arg("--seed") || process.env.JET_CANVAS_SEED || "373");
const browser = arg("--browser") || process.env.JET_CANVAS_BROWSER || "chromium";

if (!scenarioName || !scenarios[scenarioName]) {
  console.error(`unknown scenario: ${scenarioName || "(missing)"}`);
  console.error(`known: ${Object.keys(scenarios).join(", ")}`);
  process.exit(2);
}
if (!port) {
  console.error("missing --port");
  process.exit(2);
}

const ctx = new CanvasScenario({ port, outDir, scenarioName, seed, browser });
try {
  await ctx.start();
  await scenarios[scenarioName](ctx);
  console.log(`PASS ${ctx.driver.metadata.browser}@${ctx.driver.metadata.version} ${scenarioName}`);
} catch (err) {
  const path = await ctx.screenshot("failure").catch(() => null);
  const metadata = ctx.driver.metadata || { browser, version: "unknown" };
  console.error(`FAIL ${metadata.browser}@${metadata.version} ${scenarioName}: ${err && err.stack || err}`);
  if (path || ctx.lastScreenshot) console.error(`screenshot: ${path || ctx.lastScreenshot}`);
  process.exitCode = 1;
} finally {
  await ctx.close().catch((error) => {
    const metadata = ctx.driver.metadata || { browser, version: "unknown" };
    console.error(`FAIL ${metadata.browser}@${metadata.version} ${scenarioName} cleanup: ${error && error.stack || error}`);
    process.exitCode = 1;
  });
}
