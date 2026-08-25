#!/usr/bin/env node
import assert from "node:assert/strict";
import { CdpDriver } from "./driver.mjs";
import { setTimeout as delay } from "node:timers/promises";
import { spawn } from "node:child_process";

const repo = process.cwd();
const jet = process.env.JET_BIN;
const source = process.env.JET_SOURCE || "examples/features/tooling/canvas_blueprint_demo.jet";
const tmpdir = process.env.TMPDIR || "/home/nate/.cache/jet-test-scratch";
const chromium = process.env.CHROMIUM || "chromium";

assert(jet, "JET_BIN is required");

function startJet(args, env) {
  const child = spawn(jet, args, {
    cwd: repo,
    env: { ...process.env, TMPDIR: tmpdir, ...env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const state = { child, stdout: "", stderr: "", result: null, error: null };
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => { state.stdout += chunk; });
  child.stderr.on("data", (chunk) => { state.stderr += chunk; });
  child.on("error", (error) => { state.error = error; });
  state.closed = new Promise((resolve) => child.on("close", (code, signal) => {
    state.result = { code, signal };
    resolve(state.result);
  }));
  return state;
}

async function waitForUrl(state, timeoutMs = 45_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const line = state.stdout.match(/^Canvas: (https?:\/\/[^\r\n]+)$/m);
    if (line) return line[1].trim();
    if (state.result) {
      throw new Error(`jet dev exited before Canvas URL: ${state.result.code ?? state.result.signal}\n${state.stdout}\n${state.stderr}`);
    }
    await delay(50);
  }
  throw new Error(`timed out waiting for Canvas URL\n${state.stdout}\n${state.stderr}`);
}

async function waitForExit(state, timeoutMs = 15_000) {
  if (state.result) return state.result;
  return await Promise.race([
    state.closed,
    delay(timeoutMs).then(() => {
      throw new Error(`jet dev did not exit\n${state.stdout}\n${state.stderr}`);
    }),
  ]);
}

async function stopJet(state, signal = "SIGKILL") {
  if (!state.result) state.child.kill(signal);
  return await waitForExit(state);
}

function statusUrl(canvasUrl) {
  const canvas = new URL(canvasUrl);
  const status = new URL("/__jet_dev_status", canvas);
  status.searchParams.set("session", canvas.searchParams.get("session"));
  return status;
}

function canvasSessionUrl(canvasUrl) {
  const canvas = new URL(canvasUrl);
  const session = new URL("/__jet_canvas/session", canvas);
  session.searchParams.set("session", canvas.searchParams.get("session"));
  return session;
}

function authorizedFetch(url) {
  const target = new URL(url);
  const token = target.searchParams.get("session");
  return fetch(target, {
    headers: {
      authorization: `Bearer ${token}`,
      origin: target.origin,
    },
  });
}

async function waitForStatus(url, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError = "";
  while (Date.now() < deadline) {
    try {
      const response = await authorizedFetch(url);
      if (response.ok) return await response.json();
      lastError = `HTTP ${response.status}`;
    } catch (error) {
      lastError = String(error);
    }
    await delay(50);
  }
  throw new Error(`Canvas status never became ready: ${lastError}`);
}

async function waitForClosed(url, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      await authorizedFetch(url);
    } catch (_) {
      return;
    }
    await delay(50);
  }
  throw new Error("Canvas port remained reachable after Ctrl-C");
}

async function waitForCanvas(driver) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const page = await driver.evaluate("({ readyState: document.readyState, title: document.title })");
    if (page && page.readyState === "complete" && page.title === "Jet Canvas") return page;
    await delay(50);
  }
  throw new Error("Canvas page did not finish loading");
}

async function readCanvasSession(url) {
  const response = await authorizedFetch(url);
  assert(response.ok, `Canvas session request failed: HTTP ${response.status}`);
  const body = await response.json();
  const session = body.session || body.canvas?.session;
  assert(session?.id, "Canvas session response must identify the resident session");
  return session;
}

async function closeDriver(driver) {
  if (!driver) return;
  await driver.close().catch(() => {});
}

const missing = startJet(
  ["dev", "missing-canvas-lifecycle.jet", "--canvas"],
  { JET_CANVAS_BROWSER: "/definitely/missing/canvas-browser" },
);
const missingResult = await waitForExit(missing);
assert.notEqual(missingResult.code, 0, "source-not-found must exit with an error");
assert.match(`${missing.stdout}\n${missing.stderr}`, /can't find the file `missing-canvas-lifecycle\.jet`/);
assert(!missing.stdout.includes("Canvas: http"), "source-not-found must not start Canvas");

const server = startJet(
  ["dev", source, "--canvas"],
  { JET_CANVAS_BROWSER: "/definitely/missing/canvas-browser" },
);
let canvasUrl;
let first;
let second;
try {
  canvasUrl = await waitForUrl(server);
  assert.match(server.stderr, /Canvas browser launch failed/);
  const status = statusUrl(canvasUrl);
  const initial = await waitForStatus(status);
  assert.equal(initial.state, "ready", "browser-launch failure must leave resident dev ready");

  first = new CdpDriver({ chrome: chromium });
  await first.launch();
  await first.navigate(canvasUrl);
  await waitForCanvas(first);
  const firstSession = await readCanvasSession(canvasSessionUrl(canvasUrl));
  await first.close();
  first = null;

  const afterBrowserClose = await waitForStatus(status);
  assert(afterBrowserClose, "browser close must leave the resident Canvas host reusable");

  second = new CdpDriver({ chrome: chromium });
  await second.launch();
  await second.navigate(canvasUrl);
  await waitForCanvas(second);
  const secondSession = await readCanvasSession(canvasSessionUrl(canvasUrl));
  assert.equal(secondSession.id, firstSession.id, "second Canvas request must reuse resident session");
  await second.close();
  second = null;

  server.child.kill("SIGINT");
  const interrupt = await waitForExit(server);
  assert(interrupt.code !== null || interrupt.signal === "SIGINT", "Ctrl-C must exit jet dev");
  await waitForClosed(status);
} finally {
  await closeDriver(first);
  await closeDriver(second);
  if (!server.result) await stopJet(server);
}

console.log("PASS native jet dev --canvas lifecycle: source-not-found, launch failure, browser close, second request, Ctrl-C cleanup");
