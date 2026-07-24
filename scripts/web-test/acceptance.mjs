#!/usr/bin/env node
import { CdpDriver } from "../canvas-test/driver.mjs";

function arg(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : null;
}

async function waitFor(check, label, timeoutMs = 20000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const value = await check();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 80));
  }
  throw new Error(`timed out waiting for ${label}`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

const port = Number(arg("--port"));
if (!port) {
  throw new Error("usage: acceptance.mjs --port <port>");
}

function hexToUtf8(hex) {
  const clean = hex.replace(/^0x/i, "");
  let out = "";
  for (let i = 0; i < clean.length; i += 2) {
    out += String.fromCharCode(Number.parseInt(clean.slice(i, i + 2), 16));
  }
  return out;
}

async function fetchText(path) {
  const response = await fetch(`http://127.0.0.1:${port}${path}`, { cache: "no-store" });
  assert(response.ok, `${path} returned ${response.status}`);
  return await response.text();
}

async function assertBundle(prefix) {
  const manifest = JSON.parse(await fetchText(`${prefix}/web.manifest.json`));
  assert(manifest.status === "m2", `${prefix} manifest status`);
  assert(typeof manifest.sourceMap === "string" && manifest.sourceMap.length > 0, `${prefix} sourceMap missing`);
  const map = JSON.parse(hexToUtf8(manifest.sourceMap));
  assert(map.version === 3, `${prefix} source map version`);
  assert(typeof map.mappings === "string" && map.mappings.length > 0, `${prefix} source map mappings`);
  assert(Array.isArray(map.sources) && map.sources.length > 0, `${prefix} source map sources`);
  for (const file of ["app.js", "jet_dom_runtime.js", "app.wasm"]) {
    const body = await fetchText(`${prefix}/${file}`);
    assert(body.length > 0, `${prefix}/${file} empty`);
  }
}

async function domBox(driver) {
  return await driver.evaluate(`(() => {
    const app = document.getElementById("jet-app");
    const box = app?.querySelector("[data-jet-node]") || app?.firstElementChild;
    if (!box) return null;
    return {
      children: app.children.length,
      text: box.textContent || "",
      background: box.style.background || box.style.backgroundColor || "",
    };
  })()`);
}

async function readLogs(driver) {
  return await driver.evaluate("window.__jetAcceptanceLogs || []");
}

const driver = await new CdpDriver().launch();
try {
  await driver.send(
    "Page.addScriptToEvaluateOnNewDocument",
    {
      source: `window.__jetAcceptanceLogs = [];
console.log = (...args) => {
  window.__jetAcceptanceLogs.push(args.map((v) => String(v)).join(" "));
};`,
    },
    driver.pageSession,
  );

  await assertBundle("/click");
  await assertBundle("/reactive");
  await assertBundle("/compute");
  await assertBundle("/callback");
  await assertBundle("/lifecycle");

  await driver.navigate(`http://127.0.0.1:${port}/click/index.html`);
  let box = await waitFor(() => domBox(driver), "click initial paint");
  assert(box.children === 1, `click create: ${JSON.stringify(box)}`);
  assert(box.text.includes("Clicks: 0"), `click initial text: ${box.text}`);
  assert(
    box.background.includes("rgb(51, 102, 255)") || box.background.includes("#3366ff"),
    `click initial color: ${box.background}`,
  );
  await driver.evaluate(`document.getElementById("btn").click()`);
  box = await waitFor(async () => {
    const next = await domBox(driver);
    return next && next.text.includes("Clicks: 1") ? next : null;
  }, "click update");
  assert(box.children === 1, `click reuse: ${JSON.stringify(box)}`);
  assert(
    box.background.includes("rgb(232, 121, 12)") || box.background.includes("#e8790c"),
    `click updated color: ${box.background}`,
  );
  await driver.evaluate(`document.getElementById("btn").click()`);
  box = await waitFor(async () => {
    const next = await domBox(driver);
    return next && next.text.includes("Clicks: 2") ? next : null;
  }, "click second update");
  assert(box.children === 1, `click still one node: ${JSON.stringify(box)}`);

  await driver.navigate(`http://127.0.0.1:${port}/reactive/index.html`);
  box = await waitFor(async () => {
    const next = await domBox(driver);
    return next && next.text.includes("world") ? next : null;
  }, "reactive final paint");
  assert(box.text.includes("world"), `reactive text: ${box.text}`);

  await driver.navigate(`http://127.0.0.1:${port}/compute/index.html`);
  await waitFor(async () => {
    const printed = await readLogs(driver);
    return printed.some((line) => line.trim() === "42") ? printed : null;
  }, "wasm compute console");
  const computeLogs = await readLogs(driver);
  assert(computeLogs.some((line) => line.trim() === "42"), `compute output: ${computeLogs.join("|")}`);

  await driver.navigate(`http://127.0.0.1:${port}/callback/index.html`);
  await driver.evaluate(`document.getElementById("go").click()`);
  await waitFor(async () => {
    const printed = await readLogs(driver);
    return printed.some((line) => line.trim() === "42") ? printed : null;
  }, "wasm callback click");
  const callbackLogs = await readLogs(driver);
  assert(callbackLogs.some((line) => line.trim() === "42"), `callback output: ${callbackLogs.join("|")}`);

  await driver.navigate(`http://127.0.0.1:${port}/lifecycle/index.html`);
  let labels = await waitFor(async () => {
    const next = await driver.evaluate(`Array.from(document.querySelectorAll("#jet-app [data-jet-node]")).map((el) => el.textContent)`);
    return next.length === 2 ? next : null;
  }, "lifecycle initial nodes");
  assert(labels.includes("keep") && labels.includes("drop"), `lifecycle initial: ${labels.join("|")}`);
  await driver.evaluate(`document.getElementById("hide").click()`);
  labels = await waitFor(async () => {
    const next = await driver.evaluate(`Array.from(document.querySelectorAll("#jet-app [data-jet-node]")).map((el) => el.textContent)`);
    return next.length === 1 && next[0] === "keep" ? next : null;
  }, "lifecycle remove child");
  assert(labels[0] === "keep", `lifecycle remove left: ${labels.join("|")}`);
  await driver.evaluate(`document.getElementById("show").click()`);
  labels = await waitFor(async () => {
    const next = await driver.evaluate(`Array.from(document.querySelectorAll("#jet-app [data-jet-node]")).map((el) => el.textContent)`);
    return next.length === 2 ? next : null;
  }, "lifecycle restore child");
  assert(labels.includes("drop"), `lifecycle restore: ${labels.join("|")}`);

  console.log("PASS web backend browser acceptance matrix");
} finally {
  await driver.close();
}
