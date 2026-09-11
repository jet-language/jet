#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { CdpDriver } from "../canvas-test/driver.mjs";

function arg(name, fallback = null) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : fallback;
}

function required(name) {
  const value = arg(name);
  if (!value) throw new Error(`missing ${name}`);
  return value;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

async function waitFor(check, label, timeoutMs = 30000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const value = await check();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 80));
  }
  throw new Error(`timed out waiting for ${label}`);
}

async function freePort() {
  const listener = createServer();
  await new Promise((resolve, reject) => {
    listener.once("error", reject);
    listener.listen(0, "127.0.0.1", resolve);
  });
  const port = listener.address().port;
  await new Promise((resolve) => listener.close(resolve));
  return port;
}

async function fetchText(port, path) {
  const response = await fetch(`http://127.0.0.1:${port}${path}`, { cache: "no-store" });
  return { status: response.status, text: await response.text() };
}

async function waitForHttp(port, path, predicate, label) {
  return await waitFor(async () => {
    try {
      const response = await fetchText(port, path);
      return response.status === 200 && predicate(response.text) ? response : null;
    } catch {
      return null;
    }
  }, label);
}

async function waitForVersion(port, baseline = null) {
  return await waitFor(async () => {
    try {
      const response = await fetchText(port, "/__jet_dev_version");
      if (response.status !== 200) return null;
      const value = response.text.trim();
      return value && value !== baseline ? value : null;
    } catch {
      return null;
    }
  }, baseline ? "dev rebuild" : "initial dev version");
}

async function runChild(program, args, cwd) {
  return await new Promise((resolve, reject) => {
    const child = spawn(program, args, { cwd, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, NO_COLOR: "1", CI: "1" } });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.once("error", reject);
    child.once("close", (code, signal) => resolve({ code, signal, stdout, stderr }));
  });
}

const metric = required("--metric");
const exercise = process.argv.includes("--exercise");
const manifestPath = required("--manifest");
const jetEnv = required("--jet-env");
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
const app = manifest.reference_apps?.find((candidate) => candidate.id === required("--app"));
assert(app, `reference app is missing from ${manifestPath}`);
const projectRoot = process.cwd();
const sourceRelative = app.edit_loop.file.startsWith(`${app.root}/`)
  ? app.edit_loop.file.slice(app.root.length + 1)
  : app.edit_loop.file;
const sourcePath = join(projectRoot, sourceRelative);
const entry = app.entry.startsWith(`${app.root}/`) ? app.entry.slice(app.root.length + 1) : app.entry;
const edit = app.edit_loop.edit;
const failure = app.edit_loop.failure;
const repair = app.edit_loop.repair;

let server = null;
let driver = null;
let original = await readFile(sourcePath, "utf8");
const port = await freePort();

async function dom(expression) {
  return await driver.evaluate(expression);
}

async function waitForReferencePage(marker) {
  return await waitFor(async () => {
    const state = await dom(`(() => {
      const root = document.querySelector('[data-jet-edit-loop]');
      const interactions = document.querySelector('[data-panel="interactions"]');
      const form = document.querySelector('form[action="/actions/ship-order-progressive"]');
      return {
        marker: root?.getAttribute('data-jet-edit-loop') || '',
        interactions: Boolean(interactions),
        form: Boolean(form),
        text: document.body?.innerText || ''
      };
    })()`);
    return (state.marker === marker || state.text.includes(marker)) && state.interactions && state.form ? state : null;
  }, `reference page ${marker}`);
}

async function startDev() {
  server = spawn(jetEnv, ["jet", "dev", entry, "--target=web", "--canvas", `--port=${port}`], {
    cwd: projectRoot,
    stdio: "ignore",
    env: { ...process.env, NO_COLOR: "1", CI: "1" },
  });
  server.once("error", (error) => { throw error; });
  await waitForHttp(port, "/", (text) => text.includes("data-jet-edit-loop"), "reference app preview");
  await waitForVersion(port);
}

async function exerciseReferenceApp() {
  const initial = await dom(`(() => ({
    routes: document.querySelector('[data-panel="routes"]')?.getAttribute("data-jet-routes") || "",
    query: Boolean(document.querySelector('[data-panel="query"]')),
    forms: Boolean(document.querySelector('[data-panel="forms"]')),
    table: Boolean(document.querySelector('[data-panel="table"]')),
    virtual: Boolean(document.querySelector('[data-panel="virtual"]')),
    store: Boolean(document.querySelector('[data-panel="store"]')),
    refresh: document.querySelector('[data-action="refresh-orders"]')?.getAttribute("href") || "",
    next: document.querySelector('[data-action="next-page"]')?.getAttribute("href") || "",
    cart: document.querySelector('[data-action="add-to-cart"]')?.getAttribute("href") || ""
  }))()`);
  assert(initial.routes.includes("/orders") && initial.routes.includes("/settings"), "reference routes were not rendered");
  for (const panel of ["query", "forms", "table", "virtual", "store"]) {
    assert(initial[panel], `reference ${panel} panel was not rendered`);
  }
  for (const link of ["refresh", "next", "cart"]) assert(initial[link], `reference ${link} link was not rendered`);

  const navigateByAction = async (action, label) => {
    const loaded = driver.waitForEvent("Page.loadEventFired", driver.pageSession);
    await driver.evaluate(`document.querySelector('[data-action="${action}"]').click()`);
    await loaded;
    await waitForReferencePage("state-preserved");
    return await dom(`document.querySelector('[data-panel="table"]')?.getAttribute("data-table-page") || ""`);
  };
  assert(await navigateByAction("next-page", "next page") === "1", "next-page did not update the table page");
  await navigateByAction("refresh-orders", "refresh");
  const refresh = await dom(`document.querySelector('[data-panel="query"]')?.getAttribute("data-query-refresh") || ""`);
  assert(Number(refresh) > 0, "refresh link did not update query state");
  await navigateByAction("add-to-cart", "cart");
  const cartCount = await dom(`document.querySelector('[data-panel="store"]')?.getAttribute("data-cart-count") || ""`);
  assert(cartCount === "1", `cart link did not update store state: ${cartCount}`);

  const formResult = await dom(`(async () => {
    const form = document.querySelector('form[action="/actions/ship-order-progressive"]');
    if (!form) return null;
    const body = new URLSearchParams(Array.from(new FormData(form), ([key, value]) => [key, String(value)]));
    const response = await fetch(form.action, {
      method: form.method || "POST",
      body,
      credentials: "same-origin"
    });
    return { action: form.action, method: form.method, status: response.status, text: await response.text() };
  })()`);
  assert(formResult?.action.endsWith("/actions/ship-order-progressive"), "progressive form action was not rendered");
  assert(String(formResult.method).toUpperCase() === "POST", "progressive form method was not POST");
  assert(formResult.status === 200 && formResult.text.includes("ship-order submitted"), `progressive form failed: ${JSON.stringify(formResult)}`);
}

async function startBrowser() {
  driver = await new CdpDriver().launch();
  await driver.navigate(`http://127.0.0.1:${port}/`);
  await waitForReferencePage("state-preserved");
  if (exercise) await exerciseReferenceApp();
}

async function editAndReload(from, to, marker, label) {
  const baseline = (await fetchText(port, "/__jet_dev_version")).text.trim();
  const current = await readFile(sourcePath, "utf8");
  assert(current.includes(from), `${label} source recipe is not applicable`);
  await writeFile(sourcePath, current.replace(from, to));
  await waitForVersion(port, baseline);
  await driver.navigate(`http://127.0.0.1:${port}/`);
  await waitForReferencePage(marker);
}

async function runFirst() {
  const started = Date.now();
  await startDev();
  await startBrowser();
  await waitForReferencePage("state-preserved");
  return Date.now() - started;
}

async function runEdit() {
  await startDev();
  await startBrowser();
  const started = Date.now();
  await editAndReload(edit.needle, edit.replacement, edit.visible_marker, "edit");
  return Date.now() - started;
}

async function runError() {
  await startDev();
  await startBrowser();
  const baseline = (await fetchText(port, "/__jet_dev_version")).text.trim();
  const diagnosisStarted = Date.now();
  const current = await readFile(sourcePath, "utf8");
  assert(current.includes(failure.needle), "failure source recipe is not applicable");
  await writeFile(sourcePath, current.replace(failure.needle, failure.replacement));
  await waitFor(async () => {
    const visible = await dom(`document.querySelector('#jet-dev-overlay')?.style.display === 'flex'`);
    return visible ? true : null;
  }, "reference compile diagnostic overlay");
  const diagnosisMs = Date.now() - diagnosisStarted;
  const repairStarted = Date.now();
  const broken = await readFile(sourcePath, "utf8");
  assert(broken.includes(repair.needle), "repair source recipe is not applicable");
  await writeFile(sourcePath, broken.replace(repair.needle, repair.replacement));
  await waitForVersion(port, baseline);
  await driver.navigate(`http://127.0.0.1:${port}/`);
  await waitFor(async () => {
    const hidden = await dom(`(() => {
      const overlay = document.querySelector('#jet-dev-overlay');
      return !overlay || overlay.style.display === 'none';
    })()`);
    return hidden ? true : null;
  }, "reference overlay recovery");
  await waitForReferencePage("state-preserved");
  const repairMs = Date.now() - repairStarted;
  return { value: diagnosisMs + repairMs, diagnosisMs, repairMs };
}

async function runTest() {
  const started = Date.now();
  const result = await runChild(jetEnv, ["jet", "test"], projectRoot);
  assert(result.code === 0, `reference test loop failed: ${result.stderr || result.stdout}`);
  return Date.now() - started;
}

try {
  let value;
  let detail = null;
  if (metric === "first_run") value = await runFirst();
  else if (metric === "edit_to_see") value = await runEdit();
  else if (metric === "error_to_fix") {
    detail = await runError();
    value = detail.value;
  } else if (metric === "test_loop") value = await runTest();
  else throw new Error(`unsupported reference metric: ${metric}`);
  if (driver) await driver.close();
  if (server && server.exitCode === null && server.signalCode === null) server.kill("SIGTERM");
  console.log(`REFERENCE_METRIC:${metric} value_ms=${value}${detail ? ` diagnosis_ms=${detail.diagnosisMs} repair_ms=${detail.repairMs}` : ""}`);
} finally {
  await writeFile(sourcePath, original);
  if (driver) await driver.close().catch(() => {});
  if (server && server.exitCode === null && server.signalCode === null) server.kill("SIGKILL");
}
