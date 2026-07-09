#!/usr/bin/env node
import { CanvasScenario, scenarios } from "./scenario.mjs";

function arg(name) {
  const idx = process.argv.indexOf(name);
  return idx >= 0 ? process.argv[idx + 1] : null;
}

const scenarioName = arg("--scenario");
const port = Number(arg("--port") || process.env.JET_CANVAS_PORT || "0");
const outDir = arg("--out-dir") || process.env.JET_CANVAS_OUT_DIR || "target/canvas-screenshots";

if (!scenarioName || !scenarios[scenarioName]) {
  console.error(`unknown scenario: ${scenarioName || "(missing)"}`);
  console.error(`known: ${Object.keys(scenarios).join(", ")}`);
  process.exit(2);
}
if (!port) {
  console.error("missing --port");
  process.exit(2);
}

const ctx = await new CanvasScenario({ port, outDir, scenarioName }).start();
try {
  await scenarios[scenarioName](ctx);
  console.log(`PASS ${scenarioName}`);
} catch (err) {
  const path = await ctx.screenshot("failure").catch(() => null);
  console.error(`FAIL ${scenarioName}: ${err && err.stack || err}`);
  if (path || ctx.lastScreenshot) console.error(`screenshot: ${path || ctx.lastScreenshot}`);
  process.exitCode = 1;
} finally {
  await ctx.close();
}

