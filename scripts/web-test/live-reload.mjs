#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";
import { CdpDriver } from "../canvas-test/driver.mjs";

function arg(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : null;
}

async function waitFor(check, label, timeoutMs = 15000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const value = await check();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 40));
  }
  throw new Error(`timed out waiting for ${label}`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function requiredArg(name) {
  const value = arg(name);
  if (!value) throw new Error(`missing ${name}`);
  return value;
}

function replaceOnce(source, from, to, label) {
  assert(source.includes(from), `${label} marker missing: ${from}`);
  return source.replace(from, to);
}

const port = Number(requiredArg("--port"));
const sourcePath = requiredArg("--source");
const shellPath = requiredArg("--shell");
const assetPath = requiredArg("--asset");
const sourceBefore = await readFile(sourcePath, "utf8");
const shellBefore = await readFile(shellPath, "utf8");
const assetBefore = await readFile(assetPath, "utf8");
const sourceAfter = replaceOnce(
  sourceBefore,
  requiredArg("--source-from"),
  requiredArg("--source-to"),
  "source",
);
const shellAfter = replaceOnce(
  shellBefore,
  requiredArg("--shell-from"),
  requiredArg("--shell-to"),
  "shell",
);
const assetAfter = replaceOnce(
  assetBefore,
  requiredArg("--asset-from"),
  requiredArg("--asset-to"),
  "asset",
);
const brokenSource = replaceOnce(
  sourceBefore,
  requiredArg("--error-from"),
  requiredArg("--error-to"),
  "error",
);
const sourceVisibleBefore = requiredArg("--source-visible-before");
const shellVisibleBefore = requiredArg("--shell-visible-before");
const sourceVisibleAfter = requiredArg("--source-visible-after");
const shellVisibleAfter = requiredArg("--shell-visible-after");
const assetColorAfter = requiredArg("--asset-color-after");

async function pageText(driver) {
  return await driver.evaluate("document.body?.innerText || \"\"");
}

async function waitForReload(driver, path, contents, label) {
  const loaded = driver.waitForEvent("Page.loadEventFired", driver.pageSession, 10000);
  const started = Date.now();
  await writeFile(path, contents);
  await loaded;
  const elapsed = Date.now() - started;
  assert(elapsed < 1000, `${label} warm reload took ${elapsed}ms`);
  return elapsed;
}

const driver = await new CdpDriver().launch();
const reloads = {};
try {
  await driver.navigate(`http://127.0.0.1:${port}/`);
  await waitFor(
    async () => (await pageText(driver)).includes(sourceVisibleBefore),
    "initial source marker",
  );
  await waitFor(
    async () => (await pageText(driver)).includes(shellVisibleBefore),
    "initial shell marker",
  );

  reloads.source = await waitForReload(driver, sourcePath, sourceAfter, "source");
  await waitFor(
    async () => (await pageText(driver)).includes(sourceVisibleAfter),
    "source update",
  );

  reloads.shell = await waitForReload(driver, shellPath, shellAfter, "shell");
  await waitFor(
    async () => (await pageText(driver)).includes(shellVisibleAfter),
    "shell update",
  );

  reloads.asset = await waitForReload(driver, assetPath, assetAfter, "asset");
  await waitFor(
    () => driver.evaluate(`getComputedStyle(document.getElementById("live-asset")).color === ${JSON.stringify(assetColorAfter)}`),
    "asset update",
  );

  await writeFile(sourcePath, brokenSource);
  await waitFor(
    () => driver.evaluate('document.getElementById("jet-dev-overlay")?.style.display === "flex"'),
    "diagnostic overlay",
  );
  const overlay = await driver.evaluate(`(() => ({
    body: document.querySelector("#jet-dev-overlay pre")?.textContent || "",
    title: document.querySelector("#jet-dev-overlay h3")?.textContent || ""
  }))()`);
  const status = await fetch(`http://127.0.0.1:${port}/__jet_dev_status`, { cache: "no-store" }).then((response) => response.json());
  assert(overlay.body === status.diagnostic, "browser overlay changed the registered diagnostic");
  assert(overlay.body.includes("Error [E0102]"), `diagnostic code missing: ${overlay.body}`);
  assert(overlay.body.includes("Why:"), `diagnostic why missing: ${overlay.body}`);
  assert(overlay.body.includes("Fix:"), `diagnostic fix missing: ${overlay.body}`);
  assert(overlay.title.includes("Build failed"), `diagnostic overlay title missing: ${overlay.title}`);

  reloads.recovery = await waitForReload(driver, sourcePath, sourceAfter, "recovery");
  await waitFor(
    async () => (await pageText(driver)).includes(sourceVisibleAfter)
      && await driver.evaluate('document.getElementById("jet-dev-overlay")?.style.display === "none"'),
    "good-save recovery",
  );

  const maxReload = Math.max(...Object.values(reloads));
  console.log(
    `PASS live reload browser matrix: source=${reloads.source}ms shell=${reloads.shell}ms asset=${reloads.asset}ms recovery=${reloads.recovery}ms max=${maxReload}ms`,
  );
} finally {
  await writeFile(sourcePath, sourceBefore);
  await writeFile(shellPath, shellBefore);
  await writeFile(assetPath, assetBefore);
  await driver.close();
}
