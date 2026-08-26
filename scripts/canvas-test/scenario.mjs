import { mkdir, writeFile } from "node:fs/promises";
import { execFile, spawn } from "node:child_process";
import { join } from "node:path";
import { createDriver } from "./driver.mjs";

const execFileAsync = (file, args, options) => new Promise((resolve, reject) => {
  execFile(file, args, options, (error, stdout, stderr) => {
    if (error) reject(Object.assign(error, { stdout, stderr }));
    else resolve({ stdout, stderr });
  });
});

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function prepareReviewGitProject(ctx) {
  const project = await ctx.driver.evaluate(`fetch("/canvas/project", { cache: "no-store" }).then((r) => r.json())`);
  const root = project.project_root;
  const baselineMain = `fn helper() Int -> {
    return 1
}

fn run() {
    print("old")
    print("remove")
    print("keep-1")
    print("keep-2")
    print("keep-3")
    print("keep-4")
    print("keep-5")
}

fn removed() {
    print("remove-me")
}
`;
  const baselineHelper = `fn helper_file() Int -> {
    return 3
}
`;
  await writeFile(join(root, "package.jet"), "name: \"review_demo\"\nversion: \"0.1.0\"\n");
  await writeFile(join(root, "main.jet"), baselineMain);
  await writeFile(join(root, "helper.jet"), baselineHelper);
  await execFileAsync("git", ["init"], { cwd: root });
  await execFileAsync("git", ["config", "user.email", "canvas@example.invalid"], { cwd: root });
  await execFileAsync("git", ["config", "user.name", "Canvas Review"], { cwd: root });
  await execFileAsync("git", ["add", "package.jet", "main.jet", "helper.jet"], { cwd: root });
  await execFileAsync("git", ["commit", "-m", "baseline"], { cwd: root });
  const dirtyMain = baselineMain
    .replace('print("old")', 'print("new")')
    .replace('print("remove")', 'print("added")')
    .replace('\nfn removed() {\n    print("remove-me")\n}\n', '\n');
  const dirtyHelper = baselineHelper.replace("return 3", "return 4");
  await writeFile(join(root, "main.jet"), dirtyMain);
  await writeFile(join(root, "helper.jet"), dirtyHelper);
  return { root, dirtyMain, dirtyHelper };
}

const BIG_PROJECT = Object.freeze({
  functions: 300,
  files: 13,
  graphs: 301,
  openBudgetMs: 10000,
  frameP95BudgetMs: 50,
  frameMaxBudgetMs: 120,
  frameCaptureMs: 3000,
  minimumFrameSamples: 12,
});
let bigPerfSerial = 0;

function percentile(values, fraction) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * fraction) - 1))];
}

async function bigFrameMeasure(ctx, label, action) {
  const key = `big-project-perf:${label}:${bigPerfSerial++}`;
  await ctx.driver.evaluate(`(() => {
    const key = ${JSON.stringify(key)};
    window.__jetCanvasPerfRuns ||= {};
    const started = performance.now();
    const frames = [];
    const eventTypes = {};
    let last = null;
    let raf = 0;
    const types = ["pointerdown", "pointermove", "pointerup", "wheel", "click", "keydown", "keyup", "input", "change"];
    const onEvent = (event) => { eventTypes[event.type] = (eventTypes[event.type] || 0) + 1; };
    const finish = () => {
      for (const type of types) window.removeEventListener(type, onEvent, true);
      window.__jetCanvasPerfRuns[key] = {
        done: true,
        elapsed_ms: performance.now() - started,
        frames,
        events: Object.values(eventTypes).reduce((sum, count) => sum + count, 0),
        event_types: eventTypes,
      };
    };
    const sample = (now) => {
      if (last !== null) frames.push(now - last);
      last = now;
      if (now - started >= ${BIG_PROJECT.frameCaptureMs}) finish();
      else raf = requestAnimationFrame(sample);
    };
    for (const type of types) window.addEventListener(type, onEvent, true);
    window.__jetCanvasPerfRuns[key] = { done: false, frames, events: 0, event_types: {} };
    raf = requestAnimationFrame(sample);
  })()`);
  await action();
  await ctx.waitFor(async () => await ctx.driver.evaluate(`!!(window.__jetCanvasPerfRuns && window.__jetCanvasPerfRuns[${JSON.stringify(key)}] && window.__jetCanvasPerfRuns[${JSON.stringify(key)}].done)`), `${label} frame capture`, 5000);
  const result = await ctx.driver.evaluate(`window.__jetCanvasPerfRuns[${JSON.stringify(key)}]`);
  const frames = (result.frames || []).filter((value) => Number.isFinite(value) && value > 0);
  if (frames.length < BIG_PROJECT.minimumFrameSamples) {
    throw new Error(`${label} captured too few frame samples: ${frames.length}`);
  }
  if (!result.events) throw new Error(`${label} captured no real input events: ${JSON.stringify(result)}`);
  const metrics = {
    frames: frames.length,
    events: result.events,
    p50_ms: percentile(frames, 0.50),
    p95_ms: percentile(frames, 0.95),
    p99_ms: percentile(frames, 0.99),
    max_ms: Math.max(...frames),
    event_types: result.event_types,
  };
  console.log(`BIG_PROJECT_PERF ${label} ${JSON.stringify(metrics)}`);
  if (metrics.p95_ms > BIG_PROJECT.frameP95BudgetMs || metrics.max_ms > BIG_PROJECT.frameMaxBudgetMs) {
    throw new Error(`${label} exceeded frame budget: ${JSON.stringify({ metrics, budget: BIG_PROJECT })}`);
  }
  return metrics;
}

async function bigElementPoint(ctx, selector, label) {
  const point = await ctx.driver.evaluate(`(() => {
    const element = document.querySelector(${JSON.stringify(selector)});
    if (!element) return null;
    const rect = element.getBoundingClientRect();
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  })()`);
  if (!point) throw new Error(`${label} element missing: ${selector}`);
  return point;
}

async function bigClickSelector(ctx, selector, label) {
  const point = await bigElementPoint(ctx, selector, label);
  await ctx.driver.click(point.x, point.y);
}

async function bigClickGraphTab(ctx, title) {
  const point = await ctx.driver.evaluate(`(() => {
    const element = Array.from(document.querySelectorAll(".graph-tab")).find((candidate) => candidate.querySelector(".graph-tab-title")?.textContent === ${JSON.stringify(title)});
    if (!element) return null;
    element.scrollIntoView({ block: "nearest", inline: "center" });
    const rect = element.getBoundingClientRect();
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  })()`);
  if (!point) throw new Error(`graph tab missing: ${title}`);
  await ctx.driver.click(point.x, point.y);
  await ctx.waitFor(async () => (await ctx.state()).graphTitle === title, `graph tab ${title}`);
}

async function bigClickProjectFile(ctx, path) {
  const point = await ctx.driver.evaluate(`(() => {
    const element = document.querySelector('[data-project-file="${path}"]');
    if (!element) return null;
    element.scrollIntoView({ block: "center", inline: "nearest" });
    const drawer = document.getElementById("left-drawer");
    const drawerRect = drawer && drawer.getBoundingClientRect();
    let rect = element.getBoundingClientRect();
    if (drawer && drawerRect) {
      if (rect.bottom > drawerRect.bottom - 8) drawer.scrollTop += rect.bottom - (drawerRect.bottom - 8);
      if (rect.top < drawerRect.top + 8) drawer.scrollTop -= (drawerRect.top + 8) - rect.top;
      rect = element.getBoundingClientRect();
    }
    const point = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
    if (drawerRect) {
      const visibleLeft = Math.max(rect.left, drawerRect.left + 4);
      const visibleRight = Math.min(rect.right, drawerRect.right - 4);
      const visibleTop = Math.max(rect.top, drawerRect.top + 4);
      const visibleBottom = Math.min(rect.bottom, drawerRect.bottom - 4);
      if (visibleRight >= visibleLeft) point.x = (visibleLeft + visibleRight) / 2;
      if (visibleBottom >= visibleTop) point.y = (visibleTop + visibleBottom) / 2;
    }
    const top = document.elementFromPoint(point.x, point.y);
    return { ...point, card: { top: rect.top, bottom: rect.bottom, width: rect.width }, drawer: drawerRect && { top: drawerRect.top, bottom: drawerRect.bottom, width: drawerRect.width }, top: top && { tag: top.tagName, id: top.id, className: top.className, file: top.getAttribute("data-project-file") } };
  })()`);
  if (!point) throw new Error(`project file card missing: ${path}`);
  console.log(`BIG_PROJECT_FILE_CLICK ${JSON.stringify({ path, point })}`);
  await ctx.driver.click(point.x, point.y);
}

async function bigFit(ctx) {
  await bigClickSelector(ctx, "#fit", "fit button");
  await sleep(160);
}

async function bigMiddleDrag(ctx, dx, dy, steps = 24) {
  const rect = await ctx.canvasRect();
  const from = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  const to = { x: from.x + dx, y: from.y + dy };
  const session = ctx.driver.pageSession;
  await ctx.driver.send("Input.dispatchMouseEvent", {
    type: "mousePressed", x: from.x, y: from.y, button: "middle", buttons: 4, clickCount: 1,
  }, session);
  for (let step = 1; step <= steps; step++) {
    const fraction = step / steps;
    await ctx.driver.send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: from.x + (to.x - from.x) * fraction,
      y: from.y + (to.y - from.y) * fraction,
      button: "none",
      buttons: 4,
    }, session);
  }
  await ctx.driver.send("Input.dispatchMouseEvent", {
    type: "mouseReleased", x: to.x, y: to.y, button: "middle", buttons: 0, clickCount: 1,
  }, session);
}

async function bigClickNode(ctx, node) {
  const rect = await ctx.canvasRect();
  await ctx.driver.click(rect.left + node.x + node.w / 2, rect.top + node.y + node.h / 2);
}

async function bigClickPin(ctx, pin) {
  const point = await ctx.state();
  const rect = await ctx.canvasRect();
  const hit = point.pinPoints && point.pinPoints[pin.pin_id];
  await ctx.driver.click(
    hit && Number.isFinite(hit.client_x) ? hit.client_x : rect.left + pin.cx,
    hit && Number.isFinite(hit.client_y) ? hit.client_y : rect.top + pin.cy,
  );
}

async function bigMinimapInk(ctx) {
  return await ctx.driver.evaluate(`(() => {
    const canvas = document.getElementById("minimap");
    if (!canvas) return 0;
    const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    let ink = 0;
    for (let i = 0; i < data.length; i += 4) {
      if (data[i + 3] > 0 && (data[i] + data[i + 1] + data[i + 2]) > 90) ink++;
    }
    return ink;
  })()`);
}

export class CanvasScenario {
  constructor({ port, outDir, scenarioName, seed = 373, browser = "chromium", session = "", programTarget = "" }) {
    this.port = port;
    this.outDir = outDir;
    this.scenarioName = scenarioName;
    this.seed = Number(seed) || 373;
    this.browser = browser;
    this.session = session;
    this.programTarget = programTarget;
    this.driver = createDriver(browser);
    this.lastScreenshot = null;
  }

  async start() {
    await mkdir(this.outDir, { recursive: true });
    await this.driver.launch();
    return this;
  }

  async close() {
    await this.driver.close();
  }

  async openCanvas(port = this.port) {
    const session = this.session ? `?session=${encodeURIComponent(this.session)}` : "";
    await this.driver.navigate(`http://127.0.0.1:${port}/canvas${session}`);
    await this.waitForCanvas();
    return await this.driver.evaluate(`(() => {
      const navigation = performance.getEntriesByType("navigation")[0];
      return performance.now() - (navigation ? navigation.startTime : 0);
    })()`);
  }

  async waitForCanvas() {
    await this.waitFor(async () => {
      const state = await this.state();
      return state && state.nodeCount > 0 && Object.keys(state.nodeBounds || {}).length > 0;
    }, "Canvas hit map");
  }

  async state() {
    return await this.driver.evaluate("window.__jetCanvasTest || null");
  }

  sessionUrl(path) {
    if (!this.session) return path;
    return `${path}${path.includes("?") ? "&" : "?"}session=${encodeURIComponent(this.session)}`;
  }

  async pin(nodeTitle, pinName) {
    const state = await this.state();
    const node = Object.values(state.nodeBounds || {}).find((n) => n.title === nodeTitle || n.title.includes(nodeTitle));
    if (!node) throw new Error(`node not found: ${nodeTitle}`);
    const pins = (state.hitMap && state.hitMap.pins || []).filter((p) => p.node_id === node.node_id);
    const pin = pins.find((p) => p.name === pinName || p.name.includes(pinName))
      || pins.find((p) => p.direction === pinName)
      || pins[0];
    if (!pin) throw new Error(`pin not found: ${nodeTitle}.${pinName}`);
    const point = state.pinPoints && state.pinPoints[pin.pin_id];
    if (point && Number.isFinite(point.client_x) && Number.isFinite(point.client_y)) {
      return { x: point.client_x, y: point.client_y, pin };
    }
    const rect = await this.canvasRect();
    return { x: rect.left + pin.cx, y: rect.top + pin.cy, pin };
  }

  async node(nodeTitle) {
    const state = await this.state();
    const node = Object.values(state.nodeBounds || {}).find((n) => n.title === nodeTitle || n.title.includes(nodeTitle));
    if (!node) throw new Error(`node not found: ${nodeTitle}`);
    const rect = await this.canvasRect();
    return { x: rect.left + node.x + node.w / 2, y: rect.top + node.y + node.h / 2, node };
  }

  async canvasRect() {
    return await this.driver.evaluate(`(() => {
      const r = document.getElementById("jet-canvas-view").getBoundingClientRect();
      return { left: r.left, top: r.top, width: r.width, height: r.height };
    })()`);
  }

  async dragPin(nodeTitle, pinName, dx = 180, dy = 40) {
    const from = await this.pin(nodeTitle, pinName);
    await this.driver.drag({ x: from.x, y: from.y }, { x: from.x + dx, y: from.y + dy });
    await sleep(120);
  }

  async openPinActionMenu(nodeTitle, pinName) {
    await this.dragPin(nodeTitle, pinName, 190, 30);
    if (await this.menuOpen()) return;
    const opened = await this.driver.evaluate(`window.__jetCanvasTest.openPinMenu(${JSON.stringify(nodeTitle)}, ${JSON.stringify(pinName)})`);
    if (!opened) {
      const p = await this.pin(nodeTitle, pinName);
      await this.driver.rightClick(p.x, p.y);
    }
    await sleep(120);
  }

  async menuOpen() {
    return await this.driver.evaluate(`(() => {
      const menu = document.getElementById("context-menu");
      return !!menu && menu.classList.contains("is-open");
    })()`);
  }

  async loadCoreCatalog(query = "abs") {
    await this.driver.evaluate(`window.__jetCanvasTest.loadCoreCatalog(${JSON.stringify(query)})`, { awaitPromise: true });
    await this.waitFor(async () => {
      return await this.driver.evaluate("Number(window.__jetCanvasCoreCatalogPalette || 0) > 0");
    }, "Core catalog palette");
  }

  async openCoreCatalogPalette(query = "") {
    await this.driver.evaluate(`window.__jetCanvasTest.openCoreCatalogPalette(${JSON.stringify(query)})`);
    await sleep(120);
  }

  async switchGraph(title) {
    const ok = await this.driver.evaluate(`window.__jetCanvasTest.switchGraphByTitle(${JSON.stringify(title)})`);
    if (!ok) throw new Error(`graph not found: ${title}`);
    await this.waitForCanvas();
  }

  async click(x, y) {
    if (typeof x === "string") {
      const pos = await this.node(x);
      await this.driver.click(pos.x, pos.y);
    } else {
      await this.driver.click(x, y);
    }
    await sleep(120);
  }

  async type(text) {
    await this.driver.type(text);
    await sleep(80);
  }

  async expectMenu(text) {
    await this.waitFor(async () => {
      return await this.driver.evaluate(`(() => {
        const menu = document.getElementById("context-menu");
        return !!menu && menu.classList.contains("is-open") && (menu.textContent.includes(${JSON.stringify(text)}) || menu.innerHTML.includes(${JSON.stringify(text)}));
      })()`);
    }, `menu containing ${text}`);
  }

  async pickEntry(text) {
    const ok = await this.driver.evaluate(`(() => {
      const buttons = Array.from(document.querySelectorAll("#context-menu [data-menu-action]"));
      const button = buttons.find((b) => b.textContent.includes(${JSON.stringify(text)}));
      if (!button) return false;
      button.click();
      return true;
    })()`);
    if (!ok) {
      const menu = await this.driver.evaluate(`(() => {
        const el = document.getElementById("context-menu");
        return el ? el.textContent : "";
      })()`);
      throw new Error(`menu entry not found: ${text}\nmenu: ${menu}`);
    }
    await sleep(500);
    await this.waitForCanvas();
  }

  async expectNodeCount(min) {
    await this.waitFor(async () => {
      const state = await this.state();
      return state && state.nodeCount >= min;
    }, `node count >= ${min}`);
  }

  async expectSourceContains(text) {
    const url = this.sessionUrl("/canvas/source");
    const body = await this.driver.evaluate(`fetch(${JSON.stringify(url)}, { cache: "no-store" }).then((r) => r.text())`);
    if (!body.includes(text)) {
      const tx = await this.driver.evaluate(`JSON.stringify({ tx: window.__jetCanvasLastTx || null, result: window.__jetCanvasLastTxResult || null })`);
      throw new Error(`source missing ${JSON.stringify(text)}\n${body}\nlast: ${tx}`);
    }
  }

  async source() {
    return await this.driver.evaluate(`(() => {
      const base = ${JSON.stringify(this.sessionUrl("/canvas/source"))};
      const sourceId = window.__jetCanvasTest?.doc?.source_id;
      const suffix = sourceId ? (base.includes("?") ? "&" : "?") + "source_id=" + encodeURIComponent(sourceId) : "";
      return fetch(base + suffix, { cache: "no-store" }).then((r) => r.text());
    })()`);
  }

  async graph() {
    return await this.driver.evaluate(`(() => {
      const base = ${JSON.stringify(this.sessionUrl("/canvas/graph"))};
      const sourceId = window.__jetCanvasTest?.doc?.source_id;
      const suffix = sourceId ? (base.includes("?") ? "&" : "?") + "source_id=" + encodeURIComponent(sourceId) : "";
      return fetch(base + suffix, { cache: "no-store" }).then((r) => r.json());
    })()`);
  }

  async uiDoc() {
    return await this.driver.evaluate(`window.__jetCanvasTest && window.__jetCanvasTest.doc`);
  }

  async problems() {
    return await this.driver.evaluate(`(window.__jetCanvasTest && window.__jetCanvasTest.problems) || []`);
  }

  async diagnosticsByNode() {
    return await this.driver.evaluate(`(window.__jetCanvasTest && window.__jetCanvasTest.diagnosticsByNode) || []`);
  }

  async expectProblem(code) {
    await this.waitFor(async () => {
      const problems = await this.problems();
      return problems.some((p) => !code || p.code === code || String(p.rendered || "").includes(code));
    }, `problem ${code || ""}`);
    const problems = await this.problems();
    return problems.find((p) => !code || p.code === code || String(p.rendered || "").includes(code));
  }

  async setSourceEditor(source) {
    const ok = await this.driver.evaluate(`window.__jetCanvasTest.setSourceEditor(${JSON.stringify(source)})`);
    if (!ok) throw new Error("source editor helper missing");
  }

  async checkCurrentSource() {
    await this.driver.evaluate(`window.__jetCanvasTest.checkCurrentSource()`);
  }

  async jumpProblem(index = 0) {
    return await this.driver.evaluate(`window.__jetCanvasTest.jumpProblem(${Number(index) || 0})`);
  }

  async query(body) {
    const url = this.sessionUrl("/canvas/query");
    return await this.driver.evaluate(`fetch(${JSON.stringify(url)}, { method: "POST", headers: { "content-type": "application/json" }, body: ${JSON.stringify(JSON.stringify(body))} }).then((r) => r.json())`);
  }

  async transaction(body) {
    const url = this.sessionUrl("/canvas/transaction");
    return await this.driver.evaluate(`(() => {
      const request = Object.assign({}, ${JSON.stringify(body)});
      const sourceId = window.__jetCanvasTest?.doc?.source_id;
      if (sourceId && !request.source_id) request.source_id = sourceId;
      return fetch(${JSON.stringify(url)}, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(request) }).then((r) => r.json().then((json) => ({ ok: r.ok, json })));
    })()`);
  }

  async uiTransaction(body) {
    const ok = await this.driver.evaluate(`window.__jetCanvasTest.postTransaction(${JSON.stringify(body)})`);
    if (!ok) throw new Error("UI transaction helper missing");
    await this.waitFor(async () => {
      return await this.driver.evaluate(`window.__jetCanvasLastTxResult !== null && window.__jetCanvasLastTxResult !== undefined`);
    }, "UI transaction result");
    const json = await this.driver.evaluate(`window.__jetCanvasLastTxResult`);
    return { ok: !(json && json.ok === false), json };
  }

  async undo() {
    const before = await this.source();
    const state = await this.state();
    if (!state || state.undoDepth < 1) throw new Error("undo helper missing or stack empty");
    const asyncUndo = await this.driver.evaluate(`(() => {
      window.__jetCanvasHistoryPromise = window.__jetCanvasTest.undo();
      return typeof window.__jetCanvasHistoryPromise?.then === "function";
    })()`);
    if (!asyncUndo) throw new Error("undo helper did not return asynchronous completion");
    await this.driver.evaluate(`window.__jetCanvasHistoryPromise`);
    const fresh = await this.graph();
    const ui = await this.uiDoc();
    if (!fresh || !ui || fresh.source_text === before || ui.source_id !== fresh.source_id || ui.revision !== fresh.revision || ui.source_text !== fresh.source_text) {
      throw new Error(`undo UI did not reach restored revision/source: ${JSON.stringify({ freshSource: fresh && fresh.source_id, freshRevision: fresh && fresh.revision, uiSource: ui && ui.source_id, uiRevision: ui && ui.revision })}`);
    }
    return fresh.source_text;
  }

  async redo() {
    const before = await this.source();
    const state = await this.state();
    if (!state || state.redoDepth < 1) throw new Error("redo helper missing or stack empty");
    const asyncRedo = await this.driver.evaluate(`(() => {
      window.__jetCanvasHistoryPromise = window.__jetCanvasTest.redo();
      return window.__jetCanvasHistoryPromise instanceof Promise;
    })()`);
    if (!asyncRedo) throw new Error("redo helper did not return asynchronous completion");
    await this.driver.evaluate(`window.__jetCanvasHistoryPromise`);
    const fresh = await this.graph();
    const ui = await this.uiDoc();
    if (!fresh || !ui || fresh.source_text === before || ui.source_id !== fresh.source_id || ui.revision !== fresh.revision || ui.source_text !== fresh.source_text) {
      throw new Error(`redo UI did not reach restored revision/source: ${JSON.stringify({ freshSource: fresh && fresh.source_id, freshRevision: fresh && fresh.revision, uiSource: ui && ui.source_id, uiRevision: ui && ui.revision })}`);
    }
    return fresh.source_text;
  }

  async replaceSource(source) {
    const graph = await this.graph();
    const result = await this.transaction({ schema_version: 1, op: "replace_source", revision: graph.revision, source });
    if (!result.ok) throw new Error(`replace_source failed: ${JSON.stringify(result.json)}`);
    await this.waitForCanvas();
    return result.json;
  }

  async screenshot(name) {
    const path = join(this.outDir, `${this.scenarioName}-${name}.png`);
    this.lastScreenshot = path;
    return await this.driver.screenshot(path);
  }

  async nonblankPixels() {
    return await this.driver.evaluate(`(() => {
      const c = document.getElementById("jet-canvas-view");
      const ctx = c.getContext("2d");
      const w = Math.min(c.width, 160), h = Math.min(c.height, 120);
      const data = ctx.getImageData(0, 0, w, h).data;
      let count = 0;
      for (let i = 0; i < data.length; i += 4) {
        if (data[i] || data[i + 1] || data[i + 2]) count++;
      }
      return count;
    })()`);
  }

  async waitFor(fn, label, timeoutMs = 8000) {
    const start = Date.now();
    while (Date.now() - start < timeoutMs) {
      if (await fn()) return;
      await sleep(80);
    }
    throw new Error(`timed out waiting for ${label}`);
  }
}

function prng(seed) {
  let t = seed >>> 0;
  return () => {
    t += 0x6D2B79F5;
    let x = Math.imul(t ^ (t >>> 15), t | 1);
    x ^= x + Math.imul(x ^ (x >>> 7), x | 61);
    return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
  };
}

function pick(rng, xs) {
  return xs[Math.floor(rng() * xs.length)];
}

async function clickSelectDetails(ctx, options = {}) {
  if (options.noopClick) {
    await ctx.driver.evaluate("window.__jetCanvasNoopClick = true");
  }
  const before = await ctx.state();
  const current = before.selectedNodeId || "";
  const target = (before.hitMap.nodes || []).find((n) => n.node_id !== current && ["total", "square", "summarize", "helper"].includes(n.title))
    || (before.hitMap.nodes || []).find((n) => n.node_id !== current);
  if (!target) throw new Error("no alternate node to select");
  await ctx.click(target.title);
  await ctx.waitFor(async () => {
    const state = await ctx.state();
    return state.selectedNodeId === target.node_id;
  }, `${target.title} selected`, 1200);
  const details = await ctx.driver.evaluate("document.getElementById('details').textContent");
  if (!details.includes(target.title)) throw new Error(`details did not show selected ${target.title} node`);
  const pinsVisible = await ctx.driver.evaluate(`(() => {
    const heading = [...document.querySelectorAll("#details h2")].find((element) => element.textContent === "Pins");
    if (!heading) return false;
    const section = heading.parentElement;
    const style = getComputedStyle(section);
    const rect = section.getBoundingClientRect();
    return style.display !== "none" && rect.width > 0 && rect.height > 0;
  })()`);
  if (!pinsVisible) throw new Error(`details hid selected ${target.title} pins`);
}

function actionReturnType(action) {
  if (action.ret) return action.ret;
  const m = String(action.signature || "").match(/->\s*([A-Za-z0-9_\[\]?:.]+)/);
  return m ? m[1] : "Void";
}

function compatibleType(accepted, actual) {
  if (!accepted || !actual) return true;
  if (accepted === actual) return true;
  if (accepted === "Any" || accepted === "Value") return true;
  if (actual === "Any" || actual === "Value") return true;
  const numeric = new Set(["Int", "Float", "F32", "F64", "Decimal"]);
  return numeric.has(accepted) && numeric.has(actual);
}

function sourceForType(type) {
  if (compatibleType(type, "Int")) return { name: "limit", type: "Int" };
  if (compatibleType(type, "String")) return { name: "text", type: "String" };
  if (compatibleType(type, "Bool")) return { name: "flag", type: "Bool" };
  if (compatibleType(type, "Float")) return { name: "ratio", type: "Float" };
  return null;
}

function defaultArgForType(type) {
  if (type === "String" || type === "Path" || type === "Url") return "\"canvas\"";
  if (type === "Bool") return "true";
  if (type === "Float" || type === "F32" || type === "F64" || type === "Decimal") return "1.0";
  return "1";
}

function argsForEntry(entry) {
  const existing = entry.args || entry.default_args || [];
  if (existing.length) return existing.slice();
  const inputs = (entry.pins || []).filter((p) => p.direction === "input");
  return inputs.map((p) => defaultArgForType(p.type || "Int"));
}

async function scratchGraph(ctx) {
  const graph = await ctx.graph();
  const scratch = (graph.graphs || []).find((g) => g.title === "scratch") || (graph.graphs || [])[0];
  if (!scratch) throw new Error("scratch graph missing");
  return { doc: graph, graph: scratch };
}

async function runInsertAttempt(ctx, baseSource, entry, origin) {
  await ctx.replaceSource(baseSource);
  const { doc, graph } = await scratchGraph(ctx);
  if (entry.stageable) {
    return { state: "stageable", id: entry.action_id || entry.callee || entry.title, reason: entry.stage_reason_code };
  }
  if (entry.available === false) {
    if (!entry.unavailable_reason_code || !entry.denied_reason) {
      throw new Error(`excluded entry missing reason: ${entry.action_id || entry.callee || entry.title}`);
    }
    return { state: "excluded", id: entry.action_id || entry.callee || entry.title, reason: entry.unavailable_reason_code };
  }
  const callee = entry.insert_callee;
  if (!callee) throw new Error(`entry missing callee: ${JSON.stringify(entry)}`);
  const body = {
    schema_version: 1,
    op: "insert_call",
    revision: doc.revision,
    graph_id: graph.graph_id,
    callee,
    args: argsForEntry(entry),
  };
  if (origin === "exec") {
    body.wire_origin_pin_id = `${graph.graph_id}:entry:output:then`;
    body.wire_target_pin = "exec";
    const ret = actionReturnType(entry);
    if (ret && ret !== "Void") body.bind = "canvas_value";
  } else {
    const inputs = (entry.pins || []).filter((p) => p.direction === "input");
    const input = inputs.find((p) => sourceForType(p.type));
    if (!input) return { state: "no-compatible-data", id: entry.action_id || entry.callee || entry.title };
    const source = sourceForType(input.type);
    const pin = (graph.pins || []).find((p) => p.name === source.name && p.direction === "output");
    if (!pin) throw new Error(`scratch pin missing: ${source.name}`);
    body.wire_origin_pin_id = pin.pin_id;
    body.wire_target_pin = input.name || "arg1";
    body.wire_expr = source.name;
  }
  const result = await ctx.transaction(body);
  const id = entry.action_id || entry.callee || entry.title;
  if (!result.ok) {
    return { state: "failed", id, diagnostic: result.json && (result.json.message || JSON.stringify(result.json)) };
  }
  return { state: "inserted", id };
}

const STAGEABLE_CATALOG_REASONS = new Set(["needs_canvas_defaults", "method_only"]);
const EXCLUDED_CATALOG_REASONS = new Set([
  "needs_unsafe_region",
  "type_member",
  "type_only",
  "value_only",
  "needs_signature",
  "not_direct_call",
]);

function catalogEntryId(entry) {
  return entry.action_id || entry.callee || entry.title;
}

async function assertStageablePaletteEntry(ctx, baseSource, entry) {
  const title = String(entry.title || entry.callee);
  const name = title.split(" · ")[0];
  await ctx.replaceSource(baseSource);
  await ctx.loadCoreCatalog(name);
  await ctx.openCoreCatalogPalette(name);
  await ctx.expectMenu(name);
  const row = await ctx.driver.evaluate(`(() => {
    const buttons = Array.from(document.querySelectorAll("#context-menu [data-menu-action]"));
    const button = buttons.find((candidate) => candidate.textContent.includes(${JSON.stringify(title)}));
    if (!button) return null;
    return {
      available: button.dataset.available,
      code: button.dataset.unavailableReasonCode,
      disabled: button.disabled,
      className: button.className,
      text: button.textContent,
    };
  })()`);
  if (!row || row.available !== "true" || row.disabled || row.className.includes("is-disabled") || row.code !== entry.stage_reason_code) {
    throw new Error(`stageable palette row is not active: ${JSON.stringify({ entry: catalogEntryId(entry), row })}`);
  }
  const before = await ctx.source();
  await ctx.pickEntry(title);
  await ctx.waitFor(async () => {
    const state = await ctx.state();
    return (state.stagedRegistry || []).some((node) => String(node.title || "").includes(title));
  }, `staged ${title}`);
  const after = await ctx.source();
  if (after !== before) throw new Error(`staging ${name} changed source`);
  const state = await ctx.state();
  const staged = (state.stagedRegistry || []).find((node) => String(node.title || "").includes(title));
  const inputs = (staged && staged.pins || []).filter((pin) => pin.direction === "input");
  const typed = (pin) => pin.type && (pin.type !== "Value" || (entry.stage_reason_code === "method_only" && pin.name === "receiver"));
  if (!inputs.length || inputs.some((pin) => !typed(pin))) {
    throw new Error(`staged ${name} lacks typed inputs: ${JSON.stringify(staged)}`);
  }
  if (entry.stage_reason_code === "method_only") {
    const receiver = inputs.find((pin) => pin.name === "receiver");
    if (!entry.receiver_type || !receiver || receiver.type !== entry.receiver_type) {
      throw new Error(`staged method ${name} lacks typed receiver: ${JSON.stringify({ entry, staged })}`);
    }
  }
}

async function catalogSweep(ctx) {
  await ctx.openCanvas();
  const baseSource = await ctx.source();
  await ctx.switchGraph("scratch");
  await ctx.waitFor(async () => {
    const entries = await ctx.driver.evaluate("window.__jetCanvasTest.actionEntries()");
    return entries.some((entry) => entry.title === "square")
      || entries.some((entry) => String(entry.title || "").startsWith("abs ·"));
  }, "catalog smoke action");
  const smoke = await ctx.driver.evaluate(`(() => {
    const entries = window.__jetCanvasTest.actionEntries();
    return entries.find((entry) => entry.title === "square")
      || entries.find((entry) => String(entry.title || "").startsWith("abs ·"));
  })()`);
  const smokeName = smoke.title === "square" ? "square" : "abs";
  await ctx.openPinActionMenu("scratch", "limit");
  await ctx.type(smokeName);
  await ctx.expectMenu(smokeName);
  await ctx.pickEntry(smokeName);
  await ctx.waitFor(async () => (await ctx.source()).includes(`${smokeName}(limit)`), `${smokeName}(limit)`);
  await ctx.replaceSource(baseSource);
  const actionDocGraph = await ctx.graph();
  const actionDoc = await ctx.query({ schema_version: 1, op: "actions", revision: actionDocGraph.revision });
  const projectEntries = (actionDoc.project_functions || []).map((fn) => ({
    title: fn.name || fn.callee,
    kind: "project_function",
    action_id: `project:${fn.callee || fn.name}`,
    callee: fn.callee || fn.name,
    insert_callee: fn.insert_callee,
    module_path: fn.module_path || "project",
    signature: fn.signature || "",
    rank: Number(fn.rank || 0),
    rank_terms: fn.rank_terms || [],
    pure: !!fn.pure,
    pins: fn.pins || [],
    ret: fn.ret || actionReturnType(fn) || "Void",
    args: fn.default_args || [],
    available: fn.available !== false,
    denied_reason: fn.denied_reason || "",
    unavailable_reason_code: fn.unavailable_reason_code || "",
  }));
  const coreEntries = (actionDoc.actions || []).filter((entry) => entry.kind === "canvas.core_catalog");
  const targets = projectEntries.concat(coreEntries);
  const projectRank = projectEntries.find((entry) => entry.title === "square");
  const functionDescriptor = (await ctx.state()).nodeDescriptors.find((descriptor) => descriptor.id === "function_pure");
  if (!projectRank
    || projectRank.rank !== functionDescriptor?.palette?.rank
    || !projectRank.rank_terms.includes("function")) {
    throw new Error("project palette rank facts drifted from the node descriptor: " + JSON.stringify({ projectRank, functionDescriptor }));
  }
  const browserEntries = await ctx.driver.evaluate("window.__jetCanvasTest.actionEntries()");
  const browserProjectEntries = browserEntries.filter((entry) => entry.kind === "project_function" || entry.kind === "canvas.action");
  const browserSquareEntries = browserProjectEntries.filter((entry) => entry.title === "square");
  if (browserProjectEntries.some((entry) => entry.kind === "project_function") || browserSquareEntries.length !== 1) {
    throw new Error(`project palette contains metadata duplicates: ${JSON.stringify(browserSquareEntries)}`);
  }
  if (browserSquareEntries[0].rank !== functionDescriptor?.palette?.rank
    || !browserSquareEntries[0].rank_terms.includes("function")) {
    throw new Error(`browser palette rank facts drifted from the node descriptor: ${JSON.stringify({ entry: browserSquareEntries[0], functionDescriptor })}`);
  }
  const seen = new Set();
  const unique = targets.filter((entry) => {
    const id = entry.action_id || entry.callee || entry.title;
    if (seen.has(id)) return false;
    seen.add(id);
    return true;
  });
  const summary = {
    total: unique.length,
    inserted: 0,
    staged: 0,
    excluded: 0,
    dataInserted: 0,
    noDataOrigin: 0,
    failures: [],
  };
  const stageable = [];
  for (const entry of unique) {
    if (entry.kind === "canvas.core_catalog") {
      if (entry.stageable) {
        if (
          entry.available !== false
          || !STAGEABLE_CATALOG_REASONS.has(entry.stage_reason_code)
          || !entry.stage_reason
          || entry.unavailable_reason_code !== entry.stage_reason_code
          || !entry.denied_reason
          || entry.insert_op !== "insert_call"
          || !entry.insert_callee
          || !entry.node_descriptor_id
        ) {
          summary.failures.push({ id: catalogEntryId(entry), state: "invalid-stage-status", reason: entry.stage_reason_code });
          continue;
        }
        const inputs = (entry.pins || []).filter((pin) => pin.direction === "input");
        const typed = (pin) => pin.type && (pin.type !== "Value" || (entry.stage_reason_code === "method_only" && pin.name === "receiver"));
        if (!inputs.length || inputs.some((pin) => !typed(pin))) {
          summary.failures.push({ id: catalogEntryId(entry), state: "stageable-without-typed-inputs", pins: entry.pins });
          continue;
        }
        if (entry.stage_reason_code === "method_only" && (!entry.receiver_type || !inputs.some((pin) => pin.name === "receiver" && pin.type === entry.receiver_type))) {
          summary.failures.push({ id: catalogEntryId(entry), state: "method-without-receiver", receiver_type: entry.receiver_type, pins: entry.pins });
          continue;
        }
        stageable.push(entry);
        continue;
      } else if (entry.available === false) {
        if (
          !EXCLUDED_CATALOG_REASONS.has(entry.unavailable_reason_code)
          || !entry.denied_reason
          || entry.stage_reason_code
          || entry.stage_reason
        ) {
          summary.failures.push({ id: catalogEntryId(entry), state: "unstable-exclusion", reason: entry.unavailable_reason_code });
          continue;
        }
      } else if (entry.stage_reason_code || entry.stage_reason || entry.unavailable_reason_code || entry.denied_reason) {
        summary.failures.push({ id: catalogEntryId(entry), state: "contradictory-available-status" });
        continue;
      }
    }
    const exec = await runInsertAttempt(ctx, baseSource, entry, "exec");
    if (exec.state === "inserted") summary.inserted++;
    else if (exec.state === "excluded") summary.excluded++;
    else if (exec.state === "failed") summary.failures.push(exec);
    if (exec.state === "excluded") continue;
    const data = await runInsertAttempt(ctx, baseSource, entry, "data");
    if (data.state === "inserted") summary.dataInserted++;
    else if (data.state === "no-compatible-data") summary.noDataOrigin++;
    else if (data.state === "failed") summary.failures.push(data);
  }
  await ctx.replaceSource(baseSource);
  const needsDefaults = stageable.find((entry) => entry.stage_reason_code === "needs_canvas_defaults");
  const methodOnly = stageable.find((entry) => entry.stage_reason_code === "method_only");
  if (!needsDefaults || !methodOnly) {
    summary.failures.push({ state: "missing-stageable-representative", needsDefaults: !!needsDefaults, methodOnly: !!methodOnly });
  } else {
    await assertStageablePaletteEntry(ctx, baseSource, needsDefaults);
    await assertStageablePaletteEntry(ctx, baseSource, methodOnly);
  }
  summary.staged = stageable.length;
  if (summary.failures.length) {
    throw new Error(`catalog sweep failed ${JSON.stringify(summary.failures.slice(0, 12), null, 2)}\nsummary ${JSON.stringify(summary)}`);
  }
  console.log(`palette_insert_catalog_sweep total=${summary.total} inserted=${summary.inserted} staged=${summary.staged} data_inserted=${summary.dataInserted} excluded=${summary.excluded} no_data_origin=${summary.noDataOrigin}`);
  return summary;
}

async function scratchLimitInline(ctx) {
  const doc = await ctx.graph();
  const graph = (doc.graphs || []).find((g) => g.title === "scratch") || (doc.graphs || [])[0];
  if (!graph) throw new Error("scratch graph missing");
  const expr = (graph.inline_exprs || []).find((e) => e.source === "limit")
    || (graph.inline_exprs || []).find((e) => String(e.source || "").includes("limit"));
  if (!expr) throw new Error(`scratch limit inline expr missing: ${JSON.stringify(graph.inline_exprs || [])}`);
  return { doc, graph, expr };
}

function projectionSnapshot(doc) {
  const graphs = (doc && doc.graphs || []).map((g) => ({
    graph_id: g.graph_id,
    title: g.title,
    function: g.function,
    nodes: g.nodes,
    pins: g.pins,
    wires: g.wires,
    inline_exprs: g.inline_exprs,
    rails: g.rails,
  }));
  return JSON.stringify({
    protocol: doc && doc.protocol,
    source_id: doc && doc.source_id,
    revision: doc && doc.revision,
    source_text: doc && doc.source_text,
    graphs,
  });
}

async function assertSourceSync(ctx, opLog) {
  await ctx.waitFor(async () => {
    const ui = await ctx.uiDoc();
    const fresh = await ctx.graph();
    return !!(ui && fresh && ui.revision === fresh.revision && ui.source_text === fresh.source_text);
  }, "Canvas projection sync");
  await ctx.waitForCanvas();
  const ui = await ctx.uiDoc();
  const fresh = await ctx.graph();
  const problems = await ctx.problems();
  if (!(fresh && fresh.graphs && fresh.graphs.length) && !problems.length) {
    throw new Error(`fresh projection failed without visible diagnostic after ops:\n${opLog.join("\n")}`);
  }
  const uiSnap = projectionSnapshot(ui);
  const freshSnap = projectionSnapshot(fresh);
  if (uiSnap !== freshSnap) {
    throw new Error(`projection drift after ops:\n${opLog.join("\n")}\nui=${uiSnap.slice(0, 2000)}\nfresh=${freshSnap.slice(0, 2000)}`);
  }
}

async function assertCleanSourceSync(ctx, opLog) {
  await assertSourceSync(ctx, opLog);
  const problems = await ctx.problems();
  if (problems.length) {
    throw new Error(`source write left Jet diagnostics after ops:\n${opLog.join("\n")}\n${JSON.stringify(problems)}`);
  }
}

function graphByTitle(doc, title) {
  const graph = (doc.graphs || []).find((g) => g.title === title || String(g.title || "").includes(title));
  if (!graph) throw new Error(`graph missing: ${title}`);
  return graph;
}

function nodeByTitle(graph, title) {
  const nodes = graph.nodes || [];
  const node = nodes.find((n) => n.title === title)
    || nodes.find((n) => String(n.title || "").includes(title));
  if (!node) throw new Error(`node missing: ${title}`);
  return node;
}

function pinForNode(graph, title, direction, type = "exec") {
  const node = nodeByTitle(graph, title);
  const pin = (graph.pins || []).find((p) => p.node_id === node.node_id && p.direction === direction && p.type === type);
  if (!pin) throw new Error(`pin missing: ${title}.${direction}.${type}`);
  return pin;
}

function namedPinForNode(graph, title, direction, name) {
  const node = nodeByTitle(graph, title);
  const pin = (graph.pins || []).find((p) => p.node_id === node.node_id && p.direction === direction && p.name === name);
  if (!pin) throw new Error(`pin missing: ${title}.${direction}.${name}`);
  return pin;
}

function controlWireExists(graph, fromTitle, toTitle) {
  const from = nodeByTitle(graph, fromTitle);
  const to = nodeByTitle(graph, toTitle);
  const fromPins = new Set((graph.pins || []).filter((p) => p.node_id === from.node_id).map((p) => p.pin_id));
  const toPins = new Set((graph.pins || []).filter((p) => p.node_id === to.node_id).map((p) => p.pin_id));
  return (graph.wires || []).some((w) => w.wire_kind === "control" && fromPins.has(w.from_pin) && toPins.has(w.to_pin));
}

function controlIncomingWires(graph, title) {
  const node = nodeByTitle(graph, title);
  const inputPins = new Set((graph.pins || [])
    .filter((pin) => pin.node_id === node.node_id && pin.direction === "input" && pin.type === "exec")
    .map((pin) => pin.pin_id));
  return (graph.wires || []).filter((wire) => wire.wire_kind === "control" && inputPins.has(wire.to_pin));
}

function dataWireExists(graph, fromTitle, toTitle) {
  const from = nodeByTitle(graph, fromTitle);
  const to = nodeByTitle(graph, toTitle);
  const fromPins = new Set((graph.pins || []).filter((p) => p.node_id === from.node_id).map((p) => p.pin_id));
  const toPins = new Set((graph.pins || []).filter((p) => p.node_id === to.node_id).map((p) => p.pin_id));
  return (graph.wires || []).some((w) => w.wire_kind === "data" && fromPins.has(w.from_pin) && toPins.has(w.to_pin));
}

async function dragExecEndpoint(ctx, graphTitle, oldTargetTitle, newTargetTitle) {
  await ctx.switchGraph(graphTitle);
  await ctx.waitForCanvas();
  const state = await ctx.state();
  const targetPin = pinForNode(graphByTitle(await ctx.graph(), graphTitle), newTargetTitle, "input", "exec");
  const targetPoint = state.pinPoints[targetPin.pin_id];
  if (!targetPoint) throw new Error(`target pin point missing: ${targetPin.pin_id}`);
  const oldNode = Object.values(state.nodeBounds || {}).find((node) => node.title === oldTargetTitle || String(node.title || "").includes(oldTargetTitle));
  if (!oldNode) throw new Error(`old target node missing: ${oldTargetTitle}`);
  const oldPins = (state.hitMap.pins || []).filter((pin) => pin.node_id === oldNode.node_id).map((pin) => pin.pin_id);
  const endpoint = (state.wireEndpoints || []).find((e) => e.wire_kind === "control" && e.endpoint === "to" && oldPins.includes(e.pin_id));
  if (!endpoint) throw new Error(`exec wire endpoint missing for ${oldTargetTitle}: ${JSON.stringify(state.wireEndpoints || [])}`);
  await ctx.driver.drag(
    { x: endpoint.client_x, y: endpoint.client_y },
    { x: targetPoint.client_x, y: targetPoint.client_y },
    16
  );
  await sleep(500);
}

async function dragExecPin(ctx, graphTitle, fromTitle, fromPinName, toTitle) {
  const current = await ctx.state();
  if (!current || current.graphTitle !== graphTitle) await ctx.switchGraph(graphTitle);
  await ctx.waitForCanvas();
  const doc = await ctx.graph();
  const graph = graphByTitle(doc, graphTitle);
  const fromPin = namedPinForNode(graph, fromTitle, "output", fromPinName);
  const targetPin = namedPinForNode(graph, toTitle, "input", "exec");
  const state = await ctx.state();
  const fromPoint = state.pinPoints[fromPin.pin_id];
  const targetPoint = state.pinPoints[targetPin.pin_id];
  if (!fromPoint || !targetPoint) throw new Error(`exec pin points missing: ${fromPin.pin_id} -> ${targetPin.pin_id}`);
  await ctx.driver.drag(
    { x: fromPoint.client_x, y: fromPoint.client_y },
    { x: targetPoint.client_x, y: targetPoint.client_y },
    16
  );
  await sleep(500);
}

async function dataPinPoints(ctx, graphTitle, fromTitle, fromPinName, toTitle, toPinName) {
  await ctx.switchGraph(graphTitle);
  await ctx.waitForCanvas();
  const graph = graphByTitle(await ctx.graph(), graphTitle);
  const fromPin = namedPinForNode(graph, fromTitle, "output", fromPinName);
  const targetPin = namedPinForNode(graph, toTitle, "input", toPinName);
  const state = await ctx.state();
  const fromPoint = state.pinPoints[fromPin.pin_id];
  const targetPoint = state.pinPoints[targetPin.pin_id];
  if (!fromPoint || !targetPoint) throw new Error(`data pin points missing: ${fromPin.pin_id} -> ${targetPin.pin_id}`);
  return { graph, fromPin, targetPin, fromPoint, targetPoint };
}

async function dragDataPin(ctx, graphTitle, fromTitle, fromPinName, toTitle, toPinName, inspectPreview = false) {
  const points = await dataPinPoints(ctx, graphTitle, fromTitle, fromPinName, toTitle, toPinName);
  const { fromPoint, targetPoint } = points;
  const dispatch = async (stage, params) => {
    try {
      await ctx.driver.send("Input.dispatchMouseEvent", params, ctx.driver.pageSession);
    } catch (error) {
      throw new Error(`data drag ${stage}: ${error && error.message || error}`);
    }
  };
  await dispatch("press", { type: "mousePressed", x: fromPoint.client_x, y: fromPoint.client_y, button: "left", clickCount: 1 });
  for (let i = 1; i <= 16; i++) {
    const t = i / 16;
    await dispatch(`move-${i}`, {
      type: "mouseMoved",
      x: fromPoint.client_x + (targetPoint.client_x - fromPoint.client_x) * t,
      y: fromPoint.client_y + (targetPoint.client_y - fromPoint.client_y) * t,
      button: "left",
      buttons: 1,
    });
  }
  await dispatch("target", {
    type: "mouseMoved",
    x: targetPoint.client_x,
    y: targetPoint.client_y,
    button: "left",
    buttons: 1,
  });
  if (inspectPreview) {
    const preview = await ctx.driver.evaluate("window.__jetCanvasWirePreview || null");
    const expectedColor = { Int: "#2ec4b6", Float: "#9acd32", Bool: "#c0392b", String: "#c678dd" }[points.fromPin.type] || null;
    if (!preview || !preview.ok || preview.to_pin_id !== points.targetPin.pin_id || (expectedColor && preview.color !== expectedColor)) {
      throw new Error(`data wire preview missing type-colored compatible state: ${JSON.stringify(preview)}`);
    }
  }
  await dispatch("release", { type: "mouseReleased", x: targetPoint.client_x, y: targetPoint.client_y, button: "left", clickCount: 1 });
  await sleep(500);
  return points;
}

async function beginDataPinDrag(ctx, graphTitle, fromTitle, fromPinName) {
  const graph = graphByTitle(await ctx.graph(), graphTitle);
  const fromPin = namedPinForNode(graph, fromTitle, "output", fromPinName);
  const state = await ctx.state();
  const fromPoint = state.pinPoints[fromPin.pin_id];
  if (!fromPoint) throw new Error(`data source pin point missing: ${fromPin.pin_id}`);
  await ctx.driver.send("Input.dispatchMouseEvent", { type: "mousePressed", x: fromPoint.client_x, y: fromPoint.client_y, button: "left", clickCount: 1 }, ctx.driver.pageSession);
  return { graph, fromPin, fromPoint };
}

async function finishDataPinDrag(ctx, targetPin) {
  const state = await ctx.state();
  const targetPoint = state.pinPoints[targetPin.pin_id];
  if (!targetPoint) throw new Error(`data target pin point missing: ${targetPin.pin_id}`);
  for (let i = 1; i <= 16; i++) {
    const t = i / 16;
    await ctx.driver.send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: targetPoint.client_x,
      y: targetPoint.client_y,
      button: "left",
      buttons: 1,
    }, ctx.driver.pageSession);
    if (i < 16) await sleep(2);
  }
  await ctx.driver.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: targetPoint.client_x, y: targetPoint.client_y, button: "left", clickCount: 1 }, ctx.driver.pageSession);
  await sleep(500);
}

function sourceNameOrder(src, names) {
  return names.map((name) => src.indexOf(`${name} ::`));
}

function firstInline(graph, predicate, label) {
  const expr = (graph.inline_exprs || []).find(predicate);
  if (!expr) throw new Error(`inline expr missing: ${label}\n${JSON.stringify(graph.inline_exprs || [])}`);
  return expr;
}

async function expectSourceChange(ctx, before, label) {
  await ctx.waitFor(async () => (await ctx.source()) !== before, `${label} source change`);
  await ctx.waitForCanvas();
  return await ctx.source();
}

async function uiEdit(ctx, body, label) {
  const before = await ctx.source();
  const result = await ctx.uiTransaction(body);
  if (!result.ok) throw new Error(`${label} failed: ${JSON.stringify(result.json)}`);
  await expectSourceChange(ctx, before, label);
  return result.json;
}

async function currentGraphDoc(ctx, title) {
  const doc = await ctx.graph();
  return { doc, graph: graphByTitle(doc, title) };
}

async function expectConsumedDescriptor(ctx, id, expected = {}) {
  const state = await ctx.state();
  const descriptor = (state.nodeDescriptors || []).find((candidate) => candidate.id === id);
  if (!descriptor) throw new Error(`served descriptor missing: ${id}\n${JSON.stringify(state.nodeDescriptors || [])}`);
  const consumed = (state.descriptorConsumption || []).find((entry) => entry.node_descriptor_id === id);
  if (!consumed) throw new Error(`descriptor not consumed by rendered graph: ${id}\n${JSON.stringify(state.descriptorConsumption || [])}`);
  if (expected.transaction !== undefined && descriptor.transaction !== expected.transaction) {
    throw new Error(`descriptor ${id} transaction ${JSON.stringify(descriptor.transaction)} != ${JSON.stringify(expected.transaction)}`);
  }
  if (expected.glyph !== undefined && consumed.presentation_glyph !== expected.glyph) {
    throw new Error(`descriptor ${id} consumed glyph ${JSON.stringify(consumed.presentation_glyph)} != ${JSON.stringify(expected.glyph)}`);
  }
  if (expected.defaultEditor !== undefined && consumed.default_editor !== expected.defaultEditor) {
    throw new Error(`descriptor ${id} consumed editor ${JSON.stringify(consumed.default_editor)} != ${JSON.stringify(expected.defaultEditor)}`);
  }
  return { descriptor, consumed };
}

async function expectPaletteDescriptor(ctx, title, id, transaction) {
  const state = await ctx.state();
  const actions = await ctx.driver.evaluate("window.__jetCanvasTest.actionEntries()");
  const action = (actions || []).find((entry) => String(entry.title || "").includes(title) && entry.node_descriptor_id === id);
  if (!action) throw new Error(`palette action missing: ${title}\n${JSON.stringify(actions || [])}`);
  const descriptor = (state.nodeDescriptors || []).find((candidate) => candidate.id === id);
  if (!descriptor || !descriptor.palette || !descriptor.palette.insertable || descriptor.transaction !== transaction) {
    throw new Error(`palette action ${title} did not consume insertable served facts: ${JSON.stringify({ action, descriptor })}`);
  }
}

async function failScratchLimit(ctx) {
  await ctx.openCanvas();
  await ctx.switchGraph("scratch");
  const { doc, expr } = await scratchLimitInline(ctx);
  const result = await ctx.uiTransaction({
    schema_version: 1,
    op: "edit_inline_expr",
    revision: doc.revision,
    inline_expr_id: expr.inline_expr_id,
    new_expr: "missing_value"
  });
  if (result.ok) throw new Error(`diagnostic transaction unexpectedly passed: ${JSON.stringify(result.json)}`);
  await ctx.expectProblem("E0107");
  return result;
}

async function elementCenter(ctx, expression, label) {
  const result = await ctx.driver.evaluate(`(() => {
    const element = ${expression};
    if (!element) return { ok: false, reason: "missing" };
    element.scrollIntoView({ block: "center", inline: "center" });
    const drawer = element.closest(".side");
    const drawerRect = drawer && drawer.getBoundingClientRect();
    let rect = element.getBoundingClientRect();
    if (drawer && drawerRect) {
      if (rect.bottom > drawerRect.bottom - 8) drawer.scrollTop += rect.bottom - (drawerRect.bottom - 8);
      if (rect.top < drawerRect.top + 8) drawer.scrollTop -= (drawerRect.top + 8) - rect.top;
      rect = element.getBoundingClientRect();
    }
    const style = getComputedStyle(element);
    const centerX = rect.left + rect.width / 2;
    const centerY = rect.top + rect.height / 2;
    const hit = document.elementFromPoint(centerX, centerY);
    const receivesPointer = hit === element || element.contains(hit);
    const visible = rect.width > 0
      && rect.height > 0
      && rect.right > 0
      && rect.bottom > 0
      && rect.left < window.innerWidth
      && rect.top < window.innerHeight
      && style.display !== "none"
      && style.visibility !== "hidden"
      && Number(style.opacity || "1") > 0
      && style.pointerEvents !== "none"
      && receivesPointer
      && !element.disabled;
    return {
      ok: visible,
      reason: visible ? "" : "not-visible",
      x: centerX,
      y: centerY,
      rect: { x: rect.x, y: rect.y, w: rect.width, h: rect.height },
      display: style.display,
      visibility: style.visibility,
      opacity: style.opacity,
      pointerEvents: style.pointerEvents,
      hit: hit && (hit.id || hit.tagName),
      receivesPointer,
      disabled: !!element.disabled
    };
  })()`);
  if (!result.ok) throw new Error(`element cannot receive a real pointer gesture: ${label}: ${JSON.stringify(result)}`);
  return result;
}

async function replaceSearch(ctx, expression, value, label) {
  const point = await elementCenter(ctx, expression, label);
  await ctx.driver.click(point.x, point.y);
  const focused = await ctx.driver.evaluate(`(() => {
    const element = ${expression};
    if (!element) return false;
    element.focus();
    if (typeof element.select === "function") element.select();
    return document.activeElement === element;
  })()`);
  if (!focused) throw new Error(`element could not receive text input: ${label}`);
  await ctx.driver.send("Input.insertText", { text: String(value) }, ctx.driver.pageSession);
  await ctx.driver.evaluate(`(() => {
    const element = ${expression};
    element?.dispatchEvent(new Event("input", { bubbles: true }));
  })()`);
}

async function assertLiveDetailsControls(ctx, label) {
  const result = await ctx.driver.evaluate(`(() => {
    const controls = [...document.querySelectorAll("#details [data-details-input]")];
    const applyButtons = [...document.querySelectorAll("#details [data-field-apply]")];
    const dead = controls.filter((input) => !input.closest("[data-details-field]")
      || !input.dataset.detailsApplyOp
      || !applyButtons.some((button) => button.dataset.fieldApply === input.dataset.detailsApplyOp));
    return {
      controls: controls.length,
      applyButtons: applyButtons.length,
      dead: dead.map((input) => input.dataset.detailsPath || input.dataset.detailsInput || "unknown")
    };
  })()`);
  if (!result || result.dead.length || !result.applyButtons) {
    throw new Error(`${label} Details control has no live apply operation: ${JSON.stringify(result)}`);
  }
  return result;
}

async function clickElement(ctx, expression, label) {
  const point = await elementCenter(ctx, expression, label);
  if (ctx.driver.pageSession) {
    await ctx.driver.send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: point.x,
      y: point.y,
    }, ctx.driver.pageSession);
  }
  await ctx.driver.click(point.x, point.y);
  await sleep(160);
}

async function clickAttribute(ctx, attribute, value, label) {
  await clickElement(
    ctx,
    `Array.from(document.querySelectorAll("[${attribute}]")).find((element) => element.getAttribute(${JSON.stringify(attribute)}) === ${JSON.stringify(value)})`,
    label
  );
}

async function pressAttribute(ctx, attribute, value, label) {
  const focused = await ctx.driver.evaluate(`(() => {
    const element = Array.from(document.querySelectorAll("[${attribute}]")).find((candidate) => candidate.getAttribute(${JSON.stringify(attribute)}) === ${JSON.stringify(value)});
    if (!element) return { ok: false, reason: "missing" };
    element.focus();
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    const visible = rect.width > 0
      && rect.height > 0
      && rect.right > 0
      && rect.bottom > 0
      && rect.left < window.innerWidth
      && rect.top < window.innerHeight
      && style.display !== "none"
      && style.visibility !== "hidden"
      && Number(style.opacity || "1") > 0
      && !element.disabled;
    return {
      ok: visible && document.activeElement === element,
      tag: element.tagName,
      disabled: !!element.disabled,
      connected: element.isConnected,
      rect: { x: rect.x, y: rect.y, w: rect.width, h: rect.height },
      display: style.display,
      visibility: style.visibility,
      opacity: style.opacity,
      active: document.activeElement && document.activeElement.tagName
    };
  })()`);
  if (!focused.ok) throw new Error(`element could not receive keyboard focus: ${label}: ${JSON.stringify(focused)}`);
  await ctx.driver.send("Input.dispatchKeyEvent", {
    type: "keyDown",
    key: "Enter",
    code: "Enter",
    text: "\r",
    unmodifiedText: "\r",
    windowsVirtualKeyCode: 13,
    nativeVirtualKeyCode: 13,
  }, ctx.driver.pageSession);
  await ctx.driver.send("Input.dispatchKeyEvent", {
    type: "keyUp",
    key: "Enter",
    code: "Enter",
    windowsVirtualKeyCode: 13,
    nativeVirtualKeyCode: 13,
  }, ctx.driver.pageSession);
  await sleep(160);
}

async function selectNodeTitles(ctx, titles, label) {
  const count = await ctx.driver.evaluate(`window.__jetCanvasTest.selectNodeTitles(${JSON.stringify(titles)})`);
  if (count !== titles.length) throw new Error(`${label} selected ${count}/${titles.length} nodes`);
  await sleep(120);
}

async function canvasModifiedClick(ctx, point, modifiers) {
  await ctx.driver.send("Input.dispatchMouseEvent", {
    type: "mousePressed", x: point.x, y: point.y, button: "left", buttons: 1, clickCount: 1, modifiers,
  }, ctx.driver.pageSession);
  await ctx.driver.send("Input.dispatchMouseEvent", {
    type: "mouseReleased", x: point.x, y: point.y, button: "left", buttons: 0, clickCount: 1, modifiers,
  }, ctx.driver.pageSession);
  await sleep(120);
}

async function canvasModifiedDrag(ctx, from, to, modifiers = 0, steps = 16) {
  await ctx.driver.send("Input.dispatchMouseEvent", {
    type: "mousePressed", x: from.x, y: from.y, button: "left", buttons: 1, clickCount: 1, modifiers,
  }, ctx.driver.pageSession);
  for (let step = 1; step <= steps; step++) {
    const fraction = step / steps;
    await ctx.driver.send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: from.x + (to.x - from.x) * fraction,
      y: from.y + (to.y - from.y) * fraction,
      button: "left",
      buttons: 1,
      modifiers,
    }, ctx.driver.pageSession);
  }
  await ctx.driver.send("Input.dispatchMouseEvent", {
    type: "mouseReleased", x: to.x, y: to.y, button: "left", buttons: 0, clickCount: 1, modifiers,
  }, ctx.driver.pageSession);
  await sleep(150);
}

async function selectClipboardNode(ctx, title) {
  await ctx.openCanvas();
  if (await ctx.driver.evaluate("document.getElementById('first-run-tour')?.classList.contains('is-open')")) {
    await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
  }
  await ctx.switchGraph("summarize");
  const target = await ctx.node(title);
  await ctx.driver.click(target.x, target.y);
  await ctx.waitFor(async () => (await ctx.state()).selectedNodeId === target.node.node_id, `${title} selected`);
  return target.node;
}

async function doubleClickCanvasPoint(ctx, point) {
  await ctx.driver.send("Input.dispatchMouseEvent", {
    type: "mousePressed",
    x: point.x,
    y: point.y,
    button: "left",
    clickCount: 2,
  }, ctx.driver.pageSession);
  await ctx.driver.send("Input.dispatchMouseEvent", {
    type: "mouseReleased",
    x: point.x,
    y: point.y,
    button: "left",
    clickCount: 2,
  }, ctx.driver.pageSession);
  await sleep(160);
}

async function visibleSurface(ctx, expression, label) {
  const result = await ctx.driver.evaluate(`(() => {
    const element = ${expression};
    if (!element) return { ok: false, reason: "missing" };
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return {
      ok: rect.width > 0
        && rect.height > 0
        && rect.right > 0
        && rect.bottom > 0
        && rect.left < window.innerWidth
        && rect.top < window.innerHeight
        && style.display !== "none"
        && style.visibility !== "hidden"
        && Number(style.opacity || "1") > 0,
      text: element.textContent || "",
      rect: { x: rect.x, y: rect.y, w: rect.width, h: rect.height },
      display: style.display,
      visibility: style.visibility,
      opacity: style.opacity
    };
  })()`);
  if (!result.ok) throw new Error(`${label} is not visibly rendered: ${JSON.stringify(result)}`);
  return result;
}

async function expectRenderedComment(ctx, title, label) {
  await ctx.waitFor(async () => {
    const state = await ctx.state();
    return (state.renderedCommentRegions || []).some((region) =>
      region.source_backed && region.title === title && region.w > 0 && region.h > 0);
  }, label);
  const state = await ctx.state();
  const region = (state.renderedCommentRegions || []).find((candidate) =>
    candidate.source_backed && candidate.title === title);
  const canvas = await ctx.canvasRect();
  if (!region
    || region.w <= 0
    || region.h <= 0
    || region.x + region.w <= 0
    || region.y + region.h <= 0
    || region.x >= canvas.width
    || region.y >= canvas.height) {
    throw new Error(`${label} has no visible text geometry: ${JSON.stringify({ region, canvas })}`);
  }
  const pixels = await ctx.driver.evaluate(`(() => {
    const region = ${JSON.stringify(region)};
    const canvas = document.getElementById("jet-canvas-view");
    const rect = canvas.getBoundingClientRect();
    const scaleX = canvas.width / rect.width;
    const scaleY = canvas.height / rect.height;
    const context = canvas.getContext("2d");
    const sample = (x, y, w, h) => {
      const sx = Math.max(0, Math.floor(x * scaleX));
      const sy = Math.max(0, Math.floor(y * scaleY));
      const sw = Math.max(1, Math.min(canvas.width - sx, Math.floor(w * scaleX)));
      const sh = Math.max(1, Math.min(canvas.height - sy, Math.floor(h * scaleY)));
      const data = context.getImageData(sx, sy, sw, sh).data;
      let red = 0, green = 0, blue = 0, bright = 0;
      for (let i = 0; i < data.length; i += 4) {
        red += data[i];
        green += data[i + 1];
        blue += data[i + 2];
        if (data[i] >= 190 && data[i + 1] >= 210 && data[i + 2] >= 225 && data[i + 3] >= 128) bright += 1;
      }
      const count = data.length / 4;
      return { red: red / count, green: green / count, blue: blue / count, bright, count };
    };
    const width = Math.max(24, Math.min(region.w - 24, 180));
    const title = sample(region.x + 10, region.y + 8, width, Math.min(24, region.h - 12));
    const inside = sample(region.x + 12, region.y + 4, width, 4);
    const outside = sample(region.x + 12, Math.max(0, region.y - 8), width, 4);
    return {
      brightTitlePixels: title.bright,
      fillDelta: Math.abs(inside.red - outside.red)
        + Math.abs(inside.green - outside.green)
        + Math.abs(inside.blue - outside.blue),
      titleSamplePixels: title.count
    };
  })()`);
  if (pixels.brightTitlePixels < 8 || pixels.fillDelta < 5) {
    throw new Error(`${label} has no observed Canvas title/fill pixels: ${JSON.stringify({ region, pixels })}`);
  }
  return region;
}

async function selectInlineExpression(ctx, graphTitle, predicate, label) {
  await ctx.driver.send("Emulation.setDeviceMetricsOverride", {
    width: 1440,
    height: 900,
    deviceScaleFactor: 1,
    mobile: false,
  }, ctx.driver.pageSession);
  await sleep(120);
  await ctx.switchGraph(graphTitle);
  const doc = await ctx.graph();
  const graph = graphByTitle(doc, graphTitle);
  const expr = firstInline(graph, predicate, label);
  const node = (graph.nodes || []).find((candidate) => candidate.node_id === expr.node_id);
  if (!node) throw new Error(`inline expression node missing: ${label}`);
  await ctx.click(node.title);
  await ctx.waitFor(async () => {
    return await ctx.driver.evaluate(`Array.from(document.querySelectorAll("[data-inline-id]")).some((element) => element.getAttribute("data-inline-id") === ${JSON.stringify(expr.inline_expr_id)})`);
  }, `${label} details`);
  await ctx.driver.evaluate(`(() => {
    const types = document.querySelector('[data-detail-toggle="types"]');
    if (types && !types.checked) {
      types.checked = true;
      types.dispatchEvent(new Event("change", { bubbles: true }));
    }
    const drawer = document.getElementById("right-drawer");
    drawer.classList.add("is-drawer-open");
    drawer.style.display = "block";
    drawer.style.position = "fixed";
    drawer.style.right = "0";
    drawer.style.top = "0";
    drawer.style.bottom = "0";
    drawer.style.width = "326px";
    drawer.style.zIndex = "40";
    document.getElementById("dock-details").classList.add("is-active");
  })()`);
  const detailsVisible = await ctx.driver.evaluate(`(() => {
    const element = Array.from(document.querySelectorAll("[data-inline-id]")).find((candidate) => candidate.getAttribute("data-inline-id") === ${JSON.stringify(expr.inline_expr_id)});
    if (!element) return false;
    const rect = element.getBoundingClientRect();
    return rect.right > 0 && rect.left < window.innerWidth && rect.bottom > 0 && rect.top < window.innerHeight;
  })()`);
  if (!detailsVisible) {
    await clickElement(ctx, `document.getElementById("dock-details")`, "details drawer");
  }
  return { doc, graph, expr, node };
}

async function expectVisibleRefusal(ctx, text, label) {
  await ctx.waitFor(async () => {
    const toast = await ctx.driver.evaluate(`document.getElementById("toast").textContent`);
    const wireStatus = await ctx.driver.evaluate(`document.getElementById("wire-status") ? document.getElementById("wire-status").textContent : ""`);
    const problems = await ctx.problems();
    return String(toast || "").toLowerCase().includes(text.toLowerCase())
      || String(wireStatus || "").toLowerCase().includes(text.toLowerCase())
      || problems.some((problem) => String(problem.rendered || problem.what || "").toLowerCase().includes(text.toLowerCase()));
  }, label);
}

async function assertSourceUnchangedAfterReload(ctx, before, label) {
  if (await ctx.source() !== before) throw new Error(`${label} changed Jet source bytes`);
  await ctx.openCanvas();
  if (await ctx.source() !== before) throw new Error(`${label} reload changed Jet source bytes`);
}

async function createCallbackThroughRail(ctx, name) {
  const before = await ctx.source();
  await ctx.driver.evaluate(`window.prompt = () => ${JSON.stringify(name)}`);
  await clickElement(ctx, `document.getElementById("canvas-new-callback")`, `create ${name} callback`);
  await ctx.waitFor(async () => {
    const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
    const source = await ctx.source();
    return result && result.changed === true && source.includes(`fn ${name}()`);
  }, `${name} callback transaction`);
  await ctx.waitForCanvas();
  return { before, after: await ctx.source(), result: await ctx.driver.evaluate("window.__jetCanvasLastTxResult") };
}

export const scenarios = {
  "canvas-onboarding-tour": async (ctx) => {
    await ctx.openCanvas();
    const original = await ctx.source();
    const tour = await visibleSurface(ctx, `document.getElementById("first-run-tour")`, "first-run tour");
    if (!tour.ok || !tour.text.includes("Read the graph")) {
      throw new Error(`first-run tour did not open with useful guidance: ${JSON.stringify(tour)}`);
    }
    const initialTour = await ctx.driver.evaluate("window.__jetCanvasTourState || null");
    if (!initialTour || initialTour.step !== 0 || initialTour.total < 4) {
      throw new Error(`tour state missing or too short: ${JSON.stringify(initialTour)}`);
    }

    await clickElement(ctx, `document.getElementById("tour-next")`, "tour edit step");
    await clickElement(ctx, `document.getElementById("tour-action")`, "tour edit action");
    await ctx.waitFor(async () => {
      const state = await ctx.driver.evaluate("window.__jetCanvasTourState || null");
      const field = await ctx.driver.evaluate(`Array.from(document.querySelectorAll("[data-inline-id]"))
        .find((element) => element.value === "4")?.getAttribute("data-inline-id") || ""`);
      return state && state.target === "details" && !!field;
    }, "tour example editor");
    const tourField = await ctx.driver.evaluate(`Array.from(document.querySelectorAll("[data-inline-id]"))
      .find((element) => element.value === "4")?.getAttribute("data-inline-id") || ""`);
    if (!tourField) throw new Error("tour did not select source-backed example value");
    await clickElement(ctx, `Array.from(document.querySelectorAll("[data-inline-id]"))
      .find((element) => element.getAttribute("data-inline-id") === ${JSON.stringify(tourField)})`, "tour value editor");
    await ctx.driver.evaluate(`(() => {
      const input = Array.from(document.querySelectorAll("[data-inline-id]")).find((element) => element.getAttribute("data-inline-id") === ${JSON.stringify(tourField)});
      input.focus();
    })()`);
    await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Control", code: "ControlLeft", modifiers: 2, windowsVirtualKeyCode: 17, nativeVirtualKeyCode: 17 }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyDown", key: "a", code: "KeyA", modifiers: 2, windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65 }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyUp", key: "a", code: "KeyA", modifiers: 2, windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65 }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Control", code: "ControlLeft", modifiers: 0, windowsVirtualKeyCode: 17, nativeVirtualKeyCode: 17 }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Backspace", code: "Backspace", windowsVirtualKeyCode: 8, nativeVirtualKeyCode: 8 }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Backspace", code: "Backspace", windowsVirtualKeyCode: 8, nativeVirtualKeyCode: 8 }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchKeyEvent", {
      type: "keyDown",
      key: "5",
      code: "Digit5",
      text: "5",
      unmodifiedText: "5",
      windowsVirtualKeyCode: 53,
      nativeVirtualKeyCode: 53,
    }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchKeyEvent", {
      type: "keyUp",
      key: "5",
      code: "Digit5",
      windowsVirtualKeyCode: 53,
      nativeVirtualKeyCode: 53,
    }, ctx.driver.pageSession);
    const typedEditor = await ctx.driver.evaluate(`(() => {
      const input = Array.from(document.querySelectorAll("[data-inline-id]")).find((element) => element.getAttribute("data-inline-id") === ${JSON.stringify(tourField)});
      return { value: input && input.value, active: document.activeElement === input, activeTag: document.activeElement && document.activeElement.tagName };
    })()`);
    if (typedEditor.value !== "5") throw new Error(`tour typed value missing: ${JSON.stringify(typedEditor)}`);
    await pressAttribute(ctx, "data-inline-apply", tourField, "tour save edit");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const source = document.getElementById("source-editor").value;
      const tx = window.__jetCanvasLastTxResult || null;
      return source.includes("summarize(5)") && !!tx && tx.changed === true;
    })()`), "tour saved source edit");
    const editState = await ctx.driver.evaluate(`({
      source: document.getElementById("source-editor").value,
      save: document.getElementById("save-state").textContent,
      toast: document.getElementById("toast").textContent,
      tx: window.__jetCanvasLastTxResult || null
    })`);
    if (!editState.source.includes("summarize(5)")) throw new Error(`tour saved source edit missing: ${JSON.stringify(editState)}`);
    const saveState = await ctx.driver.evaluate("document.getElementById('save-state').textContent");
    if (!saveState.includes("saved")) throw new Error(`accepted edit did not show saved state: ${saveState}`);

    await clickElement(ctx, `document.getElementById("tour-next")`, "tour check step");
    await ctx.driver.evaluate("window.__tourClickSeen = false; document.getElementById('tour-action').addEventListener('click', () => { window.__tourClickSeen = true; }, { once: true, capture: true });");
    await clickElement(ctx, `document.getElementById("tour-action")`, "tour check action");
    const actionClicked = await ctx.driver.evaluate("!!document.getElementById('execute-command-authority')");
    if (!actionClicked) await ctx.driver.evaluate("document.getElementById('tour-action').click()");
    const actionDebug = await ctx.driver.evaluate("({ seen: window.__tourClickSeen, state: window.__jetCanvasTourState, details: document.getElementById('details').textContent.slice(0, 80) })");
    if (!actionClicked) throw new Error(`tour check action did not prepare authority: ${JSON.stringify(actionDebug)}`);
    await ctx.waitFor(async () => await ctx.driver.evaluate("!!document.getElementById('execute-command-authority')"), "tour check authority");
    await clickElement(ctx, `document.getElementById("execute-command-authority")`, "tour check command");
    await ctx.waitFor(async () => await ctx.driver.evaluate("document.getElementById('run-hud').textContent.includes('passed')"), "tour check receipt", 15000);

    await clickElement(ctx, `document.getElementById("tour-next")`, "tour run step");
    await clickElement(ctx, `document.getElementById("tour-action")`, "tour run action");
    await ctx.waitFor(async () => await ctx.driver.evaluate("!!document.getElementById('execute-command-authority')"), "tour run authority");
    await clickElement(ctx, `document.getElementById("execute-command-authority")`, "tour run command");
    await ctx.waitFor(async () => await ctx.driver.evaluate("document.getElementById('run-hud').textContent.includes('passed')"), "tour run receipt", 15000);
    const receipt = await ctx.driver.evaluate("document.getElementById('details').textContent");
    if (!receipt.includes("stdout") || !receipt.includes("25")) throw new Error(`tour run output missing: ${receipt}`);

    await clickElement(ctx, `document.getElementById("tour-next")`, "tour undo step");
    await clickElement(ctx, `document.getElementById("tour-action")`, "tour undo action");
    await ctx.waitFor(async () => (await ctx.source()) === original, "tour undo source");
    await clickElement(ctx, `document.getElementById("tour-dismiss")`, "finish onboarding tour");
    await ctx.waitFor(async () => !(await ctx.driver.evaluate("document.getElementById('first-run-tour').classList.contains('is-open')")), "tour dismissal");
    await ctx.openCanvas();
    if (await ctx.driver.evaluate("document.getElementById('first-run-tour').classList.contains('is-open')")) {
      throw new Error("tour dismissal did not persist in local editor state");
    }

    await ctx.setSourceEditor(original.replace("square(limit)", "square(missing_value)"));
    await clickElement(ctx, `document.getElementById("check-current")`, "invalid source check");
    const problem = await ctx.expectProblem("E0107");
    if (!String(problem.rendered || "").includes("Why:") || !String(problem.rendered || "").includes("Fix:")) {
      throw new Error(`onboarding diagnostic missing guidance: ${JSON.stringify(problem)}`);
    }
    const invalidState = await ctx.driver.evaluate("window.__jetCanvasCanvasState || null");
    if (!invalidState || invalidState.kind !== "invalid") throw new Error(`invalid source state missing: ${JSON.stringify(invalidState)}`);
    await ctx.setSourceEditor(original);
    await clickElement(ctx, `document.getElementById("check-current")`, "diagnostic recovery check");
    await ctx.waitFor(async () => !(await ctx.driver.evaluate("window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === 'invalid'")), "diagnostic recovery");

    await ctx.driver.evaluate("window.dispatchEvent(new Event('offline'))");
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === 'offline'"), "offline state");
    const offlineSource = await ctx.driver.evaluate("document.getElementById('source-editor').value");
    if (!offlineSource.includes("fn run")) throw new Error("offline state hid editable source");
    await ctx.driver.evaluate("window.dispatchEvent(new Event('online'))");
    await ctx.waitFor(async () => !(await ctx.driver.evaluate("window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === 'offline'")), "offline recovery");

    await ctx.driver.shortcut(["Control", "p"]);
    await ctx.waitFor(async () => await ctx.driver.evaluate("!!document.getElementById('action-palette-search')"), "permission palette");
    await ctx.driver.type("service");
    await ctx.waitFor(async () => await ctx.driver.evaluate("!!document.querySelector('#context-menu [data-available=\"false\"]')"), "permission-denied action");
    await clickElement(ctx, `document.querySelector('#context-menu [data-available="false"]')`, "permission-denied action");
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === 'permission'"), "permission state");
    const permissionState = await ctx.driver.evaluate("window.__jetCanvasCanvasState");
    if (!permissionState.detail || !permissionState.detail.toLowerCase().includes("service")) {
      throw new Error(`permission state lacked next action: ${JSON.stringify(permissionState)}`);
    }
  },

  "open-and-render": async (ctx) => {
    await ctx.openCanvas();
    await ctx.expectNodeCount(3);
    await expectConsumedDescriptor(ctx, "entry", { glyph: "ƒ", defaultEditor: "function_signature" });
    const state = await ctx.state();
    if (!(state.descriptorConsumption || []).every((entry) => entry.node_descriptor_id && entry.presentation_label && entry.presentation_glyph && entry.hover && entry.default_editor)) {
      throw new Error(`rendered nodes did not consume complete served descriptor facts: ${JSON.stringify(state.descriptorConsumption || [])}`);
    }
    if (!state.defaultEditorFactsConsumed) throw new Error("pin editor selection did not consume descriptor default_editor facts");
    const pixels = await ctx.nonblankPixels();
    if (pixels < 100) throw new Error(`canvas looked blank: ${pixels} colored pixels`);
    await ctx.screenshot("rendered");
  },

  "devserver-real-client-survival": async (ctx) => {
    await ctx.openCanvas();
    const state = await ctx.state();
    if (!state || state.nodeCount < 1 || !state.graphTitle) {
      throw new Error(`Canvas projection did not arrive: ${JSON.stringify(state)}`);
    }
  },

  "resident-session-ide-state-matrix": async (ctx) => {
    await ctx.openCanvas();
    await ctx.waitFor(async () => await ctx.driver.evaluate("!!window.__jetCanvasSession"), "resident Canvas session");
    const state = await ctx.driver.evaluate(`(() => {
      const session = window.__jetCanvasSession || {};
      const rows = Array.from(document.querySelectorAll("[data-session-view]"));
      const app = session.listeners && session.listeners.application;
      return {
        id: session.id,
        revision: session.sourceRevision,
        lastGood: session.lastGoodProgram,
        outputCount: document.querySelectorAll("[data-canvas-output]").length,
        views: rows.map((row) => ({ name: row.getAttribute("data-session-view"), id: row.dataset.sessionId, revision: row.dataset.sourceRevision })),
        preview: document.getElementById("preview-link")?.href || "",
        appPort: app && app.port
      };
    })()`);
    if (!state.id || !state.revision || !state.lastGood) throw new Error(`session state incomplete: ${JSON.stringify(state)}`);
    if (state.views.length < 8 || state.views.some((view) => view.id !== state.id || !view.revision)) {
      throw new Error(`IDE views did not report one session/revision: ${JSON.stringify(state.views)}`);
    }
    if (!state.appPort || state.preview.indexOf(`:${state.appPort}/`) < 0) {
      throw new Error(`preview did not use the application listener: ${JSON.stringify(state)}`);
    }
    const second = await ctx.driver.evaluate(`fetch("/canvas/session?client_id=browser-two", { cache: "no-store" }).then((r) => r.json())`);
    const secondSession = second.session || second.canvas?.session;
    if (!secondSession || secondSession.clients < 2) throw new Error(`second client did not join resident session: ${JSON.stringify(second)}`);
    const project = await ctx.driver.evaluate(`fetch("/canvas/project", { cache: "no-store" }).then((r) => r.json())`);
    const projectPayload = project.canvas || project;
    if (!Array.isArray(projectPayload.outputs)) throw new Error("project output launcher field missing");
    await ctx.screenshot("resident-session-workbench");
  },

  "canvas-workbench-e2e": async (ctx) => {
    await ctx.openCanvas();
    const payload = (value) => value?.canvas && typeof value.canvas === "object" ? value.canvas : value;
    const snapshot = await ctx.driver.evaluate(`Promise.all([
      fetch(${JSON.stringify(ctx.sessionUrl("/canvas/session"))}, { cache: "no-store" }).then((r) => r.json()),
      fetch(${JSON.stringify(ctx.sessionUrl("/canvas/project"))}, { cache: "no-store" }).then((r) => r.json()),
      fetch(${JSON.stringify(ctx.sessionUrl("/canvas/graph"))}, { cache: "no-store" }).then((r) => r.json())
    ]).then(([session, project, graph]) => ({
      session: session.session || session.canvas?.session || session,
      project: project.canvas || project,
      graph: graph.canvas || graph
    }))`);
    const session = snapshot.session;
    const project = snapshot.project;
    const browserClient = await ctx.driver.evaluate("window.__jetCanvasSessionApi.clientId()");
    const outputs = project.outputs || [];
    const expectedOutputs = new Map([
      ["cli", "executable"], ["service", "service"], ["web", "executable"],
      ["ui", "executable"], ["game", "executable"], ["library", "library"],
      ["build", "check"],
    ]);
    const outputMap = new Map(outputs.map((output) => [output.target || output.name, output]));
    const missingOutputs = [...expectedOutputs].filter(([name, kind]) => outputMap.get(name)?.kind !== kind);
    if (missingOutputs.length) throw new Error(`workbench output launcher is incomplete: ${JSON.stringify({ missingOutputs, outputs })}`);

    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const names = ["project", "output", "graphs", "status", "problems", "details", "proof", "preview"];
      const views = ["text", "graph", "designer", "preview", "terminal", "debugger", "tests", "custom servers"];
      return names.every((name) => document.querySelector("[data-canvas-panel=\"" + name + "\"]"))
        && views.every((name) => document.querySelector("[data-session-view=\"" + name + "\"]"))
        && document.querySelectorAll("[data-canvas-output]").length >= 7;
    })()`), "complete workbench chrome");
    const chrome = await ctx.driver.evaluate(`(() => ({
      heading: document.querySelector("#workbench-header")?.textContent || "",
      outputNames: Array.from(document.querySelectorAll("[data-canvas-output]")).map((node) => node.dataset.canvasOutput),
      views: Array.from(document.querySelectorAll("[data-session-view]")).map((node) => node.dataset.sessionView),
      previewVisible: !document.getElementById("preview-panel")?.hidden,
      sessionId: document.getElementById("session-identity")?.dataset.sessionId || "",
      revision: document.getElementById("session-identity")?.dataset.sourceRevision || ""
    }))()`);
    if (!chrome.heading.includes("Canvas Workbench") || !chrome.previewVisible
      || chrome.sessionId !== session.id || !chrome.revision) {
      throw new Error(`workbench chrome lost shared session facts: ${JSON.stringify({ chrome, session })}`);
    }

    const actionsValue = await ctx.query({ schema_version: 1, op: "actions", revision: snapshot.graph.revision });
    const actions = payload(actionsValue).actions || [];
    const commandMap = new Map(actions.filter((action) => action.kind === "canvas.command").map((action) => [action.action_id, action]));
    for (const [id, command] of [
      ["canvas.command:run", ["jet", "run", "run.jet"]],
      ["canvas.command:check", ["jet", "check", "run.jet"]],
      ["canvas.command:test", ["jet", "test", "run.jet"]],
      ["canvas.command:dev", ["jet", "dev", "run.jet", "--target=web"]],
      ["canvas.command:service.start", ["jetpack", "services", "up"]],
    ]) {
      if (JSON.stringify(commandMap.get(id)?.command) !== JSON.stringify(command)
        || commandMap.get(id)?.available !== true) {
        throw new Error(`workbench command surface missing ${id}: ${JSON.stringify(commandMap.get(id))}`);
      }
    }

    const postCommand = async (actionId) => {
      const graph = await ctx.graph();
      return await ctx.driver.evaluate(`(async () => {
        const response = await fetch(${JSON.stringify(ctx.sessionUrl("/canvas/command"))}, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ schema_version: 1, revision: ${JSON.stringify(graph.revision)}, action_id: ${JSON.stringify(actionId)}, confirmed: true, client_id: "workbench-primary" })
        });
        return { status: response.status, value: await response.json() };
      })()`);
    };
    const checkReceipt = await postCommand("canvas.command:check");
    const runReceipt = await postCommand("canvas.command:run");
    const checkPayload = payload(checkReceipt.value);
    const runPayload = payload(runReceipt.value);
    if (checkReceipt.status !== 200 || !checkPayload.success || checkPayload.command?.join(" ") !== "jet check run.jet") {
      throw new Error(`Canvas did not execute the real CLI check: ${JSON.stringify(checkReceipt)}`);
    }
    if (runReceipt.status !== 200 || !runPayload.success || !runPayload.stdout.includes("cli")) {
      throw new Error(`Canvas did not execute the real CLI run: ${JSON.stringify(runReceipt)}`);
    }
    await ctx.waitFor(async () => await ctx.driver.evaluate(`document.getElementById("run-hud")?.textContent.includes("passed")`), "CLI receipt in run state", 15000);

    const projectRoot = project.project_root;
    const jet = process.env.JET_BIN || join(process.cwd(), "target/debug/jet");
    const jetpack = process.env.JETPACK_BIN || join(process.cwd(), "target/debug/jetpack");
    const runJetpack = async (args) => {
      try {
        const result = await execFileAsync(jetpack, args, {
          cwd: projectRoot,
          env: {
            ...process.env,
            JET_ROOT: projectRoot,
            TMPDIR: process.env.TMPDIR || "/home/nate/.cache/jet-test-scratch",
          },
          maxBuffer: 2 * 1024 * 1024,
        });
        return { ok: true, ...result };
      } catch (error) {
        return { ok: false, code: error.code, stdout: error.stdout || "", stderr: error.stderr || "" };
      }
    };
    const runCli = async (args) => {
      return await runJetpack(["env", "-y", "--", jet, ...args]);
    };
    for (const target of ["web", "x86_64-unknown-linux-gnu"]) {
      const result = await runCli(["check", "run.jet", `--target=${target}`]);
      if (!result.ok) throw new Error(`real ${target} check failed: ${JSON.stringify(result)}`);
    }
    const testResult = await runCli(["test", "run.jet"]);
    if (!testResult.ok) throw new Error(`real CLI test failed: ${JSON.stringify(testResult)}`);

    for (const [output, marker] of [["cli", "cli"], ["service", "service"], ["ui", "ui"], ["game", "game"]]) {
      const result = await runCli(["run", "run.jet", `--output=${output}`]);
      const outputText = `${result.stdout || ""}${result.stderr || ""}`;
      if (!result.ok || !outputText.includes(marker)) {
        throw new Error(`named ${output} output did not execute through the CLI: ${JSON.stringify(result)}`);
      }
    }

    let serviceAttempted = false;
    try {
      serviceAttempted = true;
      const up = await runJetpack(["services", "up", "canvas_service", "--trust", "--no-color"]);
      if (!up.ok) throw new Error(`real service start failed: ${JSON.stringify(up)}`);
      const health = await runJetpack(["services", "health", "canvas_service", "--trust", "--json", "--no-color"]);
      if (!health.ok || !health.stdout.includes('"health":"healthy"')) {
        throw new Error(`real service health failed: ${JSON.stringify(health)}`);
      }
      const wait = await runJetpack(["services", "wait", "canvas_service", "--trust", "--no-color"]);
      if (!wait.ok) throw new Error(`real service wait failed: ${JSON.stringify(wait)}`);
      const logs = await runJetpack(["services", "logs", "canvas_service", "--no-color"]);
      const logText = `${logs.stdout || ""}${logs.stderr || ""}`;
      if (!logs.ok || !logText.includes("canvas-service-ready")) {
        throw new Error(`real service logs lost readiness evidence: ${JSON.stringify(logs)}`);
      }
    } finally {
      if (serviceAttempted) {
        const down = await runJetpack(["services", "down", "canvas_service", "--no-color"]);
        if (!down.ok) throw new Error(`real service shutdown failed: ${JSON.stringify(down)}`);
      }
    }

    const appPort = session.listeners?.application?.port;
    if (!appPort || appPort === session.listeners?.canvas?.port) {
      throw new Error(`web app listener did not stay separate from Canvas: ${JSON.stringify(session.listeners)}`);
    }
    const appResponse = await fetch(`http://127.0.0.1:${appPort}/`);
    const appBody = await appResponse.text();
    if (appResponse.status !== 200 || !appBody.includes("<html")) {
      throw new Error(`web output did not load from application listener: ${appResponse.status} ${appBody.slice(0, 120)}`);
    }

    const custom = spawn(process.env.NODE || "node", ["custom-server.mjs"], {
      cwd: projectRoot,
      env: { ...process.env, TMPDIR: process.env.TMPDIR || "/home/nate/.cache/jet-test-scratch" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    try {
      await ctx.waitFor(async () => {
        try {
          const response = await fetch("http://127.0.0.1:43817/health");
          return response.status === 200 && (await response.text()) === "custom-server-ready";
        } catch (_) {
          return false;
        }
      }, "custom server readiness", 5000);
    } finally {
      if (custom.exitCode === null) {
        custom.kill("SIGTERM");
        await new Promise((resolve) => custom.once("close", resolve));
      }
    }
    if (session.custom_servers?.owner !== "application" || session.custom_servers?.transport !== "application") {
      throw new Error(`custom server crossed Canvas transport boundary: ${JSON.stringify(session.custom_servers)}`);
    }

    const second = await ctx.driver.evaluate(`fetch(${JSON.stringify(ctx.sessionUrl("/canvas/session?client_id=workbench-second"))}, { cache: "no-store" }).then((r) => r.json())`);
    const secondSession = second.session || second.canvas?.session || second;
    if (secondSession.id !== session.id || secondSession.source_revision !== session.source_revision || secondSession.clients < 2) {
      throw new Error(`second workbench client diverged: ${JSON.stringify(second)}`);
    }
    const beforeError = await ctx.source();
    const invalid = await ctx.transaction({ schema_version: 1, op: "replace_source", revision: snapshot.graph.revision, source: "fn broken(" });
    const invalidPayload = payload(invalid.json);
    if (invalid.ok || invalidPayload?.kind !== "diagnostic" || await ctx.source() !== beforeError) {
      throw new Error(`invalid source changed the last-good workbench: ${JSON.stringify(invalid)}`);
    }
    const afterError = await ctx.driver.evaluate(`fetch(${JSON.stringify(ctx.sessionUrl("/canvas/session"))}, { cache: "no-store" }).then((r) => r.json()).then((value) => value.session || value.canvas?.session || value)`);
    if (afterError.id !== session.id || !afterError.last_good_program
      || !(afterError.history?.receipts || []).some((receipt) => receipt.status === "refused")) {
      throw new Error(`last-good/error state was not shared: ${JSON.stringify(afterError)}`);
    }
    await ctx.driver.navigate("about:blank");
    await ctx.openCanvas();
    const reconnected = await ctx.driver.evaluate(`fetch(${JSON.stringify(ctx.sessionUrl("/canvas/session"))}, { cache: "no-store" }).then((r) => r.json()).then((value) => value.session || value.canvas?.session || value)`);
    if (reconnected.id !== session.id || reconnected.source_revision !== session.source_revision || reconnected.history.count !== afterError.history.count) {
      throw new Error(`reconnect lost workbench history: ${JSON.stringify({ afterError, reconnected })}`);
    }
    await ctx.screenshot("workbench");

    const endpoint = `http://127.0.0.1:${ctx.port}`;
    const disconnect = async (client) => fetch(`${endpoint}${ctx.sessionUrl(`/__jet_dev_disconnect?client=${encodeURIComponent(client)}`)}`, {
      method: "POST",
      headers: { authorization: `Bearer ${ctx.session}`, origin: `http://127.0.0.1:${ctx.port}`, host: `127.0.0.1:${ctx.port}` },
    });
    await disconnect("workbench-second");
    await disconnect(browserClient);
    await ctx.driver.navigate("about:blank");
    let shutdown = null;
    for (let attempt = 0; attempt < 20; attempt++) {
      await disconnect("workbench-primary");
      await disconnect(browserClient);
      await disconnect("workbench-second");
      const response = await fetch(`${endpoint}${ctx.sessionUrl("/canvas/session")}`, { cache: "no-store" });
      const value = await response.json();
      shutdown = value.session || value.canvas?.session || value;
      if (shutdown.clients === 0) break;
      await sleep(100);
    }
    if (!shutdown || shutdown.id !== session.id || shutdown.clients !== 0) {
      throw new Error(`workbench shutdown did not release client leases: ${JSON.stringify(shutdown)}`);
    }
  },

  "session-surface-matrix": async (ctx) => {
    await ctx.openCanvas();
    await ctx.waitFor(async () => await ctx.driver.evaluate("!!window.__jetCanvasSession"), "surface matrix session");
    const primaryClientAtStart = await ctx.driver.evaluate("window.__jetCanvasSessionApi.clientId()");

    const canvasPayload = (value) => value?.canvas && typeof value.canvas === "object" ? value.canvas : value;
    const initial = await ctx.driver.evaluate(`Promise.all([
      fetch(${JSON.stringify(ctx.sessionUrl("/canvas/session"))}, { cache: "no-store" }).then((r) => r.json()),
      fetch(${JSON.stringify(ctx.sessionUrl("/canvas/project"))}, { cache: "no-store" }).then((r) => r.json()),
      fetch(${JSON.stringify(ctx.sessionUrl("/canvas/graph"))}, { cache: "no-store" }).then((r) => r.json())
    ]).then(([session, project, graph]) => ({
      session: session.session || session.canvas?.session || session,
      project: project.canvas || project,
      graph: graph.canvas || graph
    }))`);
    const session = initial.session;
    const project = initial.project;
    const outputs = project.outputs || [];
    if (ctx.programTarget && session.run?.target !== ctx.programTarget) {
      throw new Error(`Canvas session lost selected program target: ${JSON.stringify({ expected: ctx.programTarget, session })}`);
    }
    const outputByTarget = new Map(outputs.map((output) => [output.target || output.name, output]));
    const requiredOutputs = [
      ["cli", "executable"],
      ["service", "service"],
      ["web", "executable"],
      ["ui", "executable"],
      ["game", "executable"],
      ["library", "library"],
      ["build", "check"],
    ];
    const missingOutputs = requiredOutputs.filter(([target, kind]) => {
      const output = outputByTarget.get(target);
      return !output || output.kind !== kind || !output.entry;
    });
    if (missingOutputs.length || outputs.length < requiredOutputs.length) {
      throw new Error(`surface output matrix incomplete: ${JSON.stringify({ missingOutputs, outputs })}`);
    }

    const capabilities = project.capabilities || {};
    if (!capabilities.preview || !capabilities.designer || !capabilities.service) {
      throw new Error(`surface capabilities incomplete: ${JSON.stringify(capabilities)}`);
    }
    const customServer = (project.services || []).find((service) => service.name === "custom_server");
    if (!customServer || !customServer.run?.includes("custom-server") || customServer.ports?.[0] !== 43817) {
      throw new Error(`custom server projection missing: ${JSON.stringify(project.services)}`);
    }

    const canvasListener = session.listeners?.canvas;
    const applicationListener = session.listeners?.application;
    const applicationListenerReady = ctx.programTarget === "web"
      ? applicationListener?.port > 0
      : applicationListener?.port === 0;
    if (!canvasListener || canvasListener.transport !== "canvas" || !canvasListener.port
      || !applicationListenerReady
      || session.custom_servers?.owner !== "application"
      || session.custom_servers?.transport !== "application") {
      throw new Error(`session listener/custom-server boundary missing: ${JSON.stringify(session)}`);
    }

    const actions = canvasPayload(await ctx.query({ schema_version: 1, op: "actions", revision: initial.graph.revision }));
    const commands = Object.fromEntries((actions.actions || [])
      .filter((action) => action.kind === "canvas.command")
      .map((action) => [action.action_id, action]));
    for (const actionId of ["canvas.command:run", "canvas.command:check", "canvas.command:dev"]) {
      if (!commands[actionId] || commands[actionId].command?.[0] !== "jet") {
        throw new Error(`CLI command surface missing ${actionId}: ${JSON.stringify(commands[actionId])}`);
      }
    }
    if (!commands["canvas.command:service.start"]?.available
      || JSON.stringify(commands["canvas.command:service.start"].command) !== JSON.stringify(["jetpack", "services", "up"])) {
      throw new Error(`service command surface missing: ${JSON.stringify(commands["canvas.command:service.start"])}`);
    }

    for (const [target] of requiredOutputs) {
      const attempted = await ctx.driver.evaluate(`fetch(${JSON.stringify(ctx.sessionUrl("/canvas/session"))}, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          op: "select_output",
          output: ${JSON.stringify(target)},
          target: ${JSON.stringify(target)},
          tier: "native-lldb",
          entry: "other.jet",
          preview_adapter: "canvas",
          client_id: "surface-primary"
        })
      }).then(async (r) => ({ status: r.status, body: await r.text() }))`);
      const selected = await ctx.driver.evaluate(`fetch(${JSON.stringify(ctx.sessionUrl("/canvas/session"))}, { cache: "no-store" }).then((r) => r.json())`);
      const selectedSession = canvasPayload(selected).session || canvasPayload(selected);
      if (attempted.status !== 405
        || !selectedSession
        || selectedSession.id !== session.id
        || selectedSession.source_revision !== session.source_revision
        || selectedSession.entry !== session.entry
        || selectedSession.run?.output !== session.run?.output
        || selectedSession.run?.target !== session.run?.target
        || selectedSession.debugger?.tier !== session.debugger?.tier
        || JSON.stringify(selectedSession.listeners?.application) !== JSON.stringify(session.listeners?.application)
        || JSON.stringify(selectedSession.custom_servers) !== JSON.stringify(session.custom_servers)) {
        throw new Error(`Canvas endpoint changed program selection: ${JSON.stringify({ target, attempted, before: session, after: selectedSession })}`);
      }
    }

    const secondary = new CanvasScenario({
      port: ctx.port,
      outDir: join(ctx.outDir, "secondary"),
      scenarioName: `${ctx.scenarioName}-secondary`,
      seed: ctx.seed + 1,
      browser: ctx.browser,
      session: ctx.session,
      programTarget: ctx.programTarget,
    });
    await secondary.start();
    try {
      await secondary.openCanvas();
      await secondary.waitFor(
        async () => await secondary.driver.evaluate("!!window.__jetCanvasSession"),
        "second browser session",
      );
      const secondaryClientAtStart = await secondary.driver.evaluate("window.__jetCanvasSessionApi.clientId()");
      const secondSession = await secondary.driver.evaluate("window.__jetCanvasSession");
      if (!secondaryClientAtStart || secondaryClientAtStart === primaryClientAtStart
        || !secondSession || secondSession.id !== session.id || secondSession.clients < 2
        || secondSession.sourceRevision !== session.source_revision) {
        throw new Error(`second browser did not share resident session: ${JSON.stringify({ primaryClientAtStart, secondaryClientAtStart, secondSession })}`);
      }

      const endpoint = `http://127.0.0.1:${ctx.port}`;
      const validHeaders = {
        authorization: `Bearer ${ctx.session}`,
        host: `127.0.0.1:${ctx.port}`,
        origin: `http://127.0.0.1:${ctx.port}`,
      };
      const hostileRequests = [
      {
        label: "missing session",
        path: "/canvas/graph",
        expected: 401,
        init: { headers: { host: validHeaders.host } },
      },
      {
        label: "wrong session",
        path: "/canvas/graph",
        expected: 401,
        init: { headers: { ...validHeaders, authorization: "Bearer wrong" } },
      },
      {
        label: "foreign origin",
        path: "/canvas/graph",
        expected: 401,
        init: { headers: { ...validHeaders, origin: `http://evil.invalid:${ctx.port}` } },
      },
      {
        label: "foreign host",
        path: "/canvas/graph",
        expected: 401,
        init: { headers: { ...validHeaders, host: `evil.invalid:${ctx.port}`, origin: `http://evil.invalid:${ctx.port}` } },
      },
      {
        label: "wrong method",
        path: "/canvas/graph",
        expected: 405,
        init: { method: "POST", headers: validHeaders, body: "{}" },
      },
      {
        label: "unknown path",
        path: "/canvas/not-a-route",
        expected: 401,
        init: { headers: validHeaders },
      },
      {
        label: "duplicate session query",
        path: `/canvas/graph?session=${encodeURIComponent(ctx.session)}&session=wrong`,
        expected: 401,
        init: { headers: validHeaders },
      },
      ];
      for (const hostile of hostileRequests) {
        const response = await fetch(`${endpoint}${hostile.path}`, hostile.init);
        if (response.status !== hostile.expected) {
          throw new Error(`${hostile.label} request returned ${response.status}, expected ${hostile.expected}`);
        }
      }

      const projectRoot = project.project_root;
      if (!projectRoot) throw new Error(`project root missing from Canvas projection: ${JSON.stringify(project)}`);
      const sourceBeforeReconnect = await ctx.source();
      await writeFile(join(projectRoot, "package.jet"), `name: "canvas_session_matrix"
version: "0.1.0"
    outputs: .{
    cli: .Executable{ name: "cli", entry: run }
    library: .Library{ name: "library", entry: run }
}
`);
      await ctx.driver.navigate("about:blank");
      await ctx.openCanvas();
      const reconnected = await ctx.driver.evaluate(`fetch(${JSON.stringify(ctx.sessionUrl("/canvas/session"))}, { cache: "no-store" }).then((r) => r.json()).then((value) => value.session || value.canvas?.session || value)`);
      if (!reconnected || reconnected.id !== session.id || reconnected.source_revision !== session.source_revision) {
        throw new Error(`Canvas reconnect did not preserve resident session: ${JSON.stringify({ session, reconnected })}`);
      }
      let observedProject = null;
      try {
        await ctx.waitFor(async () => {
          observedProject = await ctx.driver.evaluate(`fetch(${JSON.stringify(ctx.sessionUrl("/canvas/project"))}, { cache: "no-store" }).then(async (r) => {
          const value = await r.json();
          const project = value.canvas || value;
          return { status: r.status, outputs: project.outputs || [], capabilities: project.capabilities || {}, project_root: project.project_root || null };
        })`);
          return observedProject.status === 200
            && observedProject.outputs.length === 2
            && !observedProject.capabilities.preview
            && !observedProject.capabilities.designer
            && JSON.stringify(observedProject.capabilities) !== JSON.stringify(project.capabilities || {});
        }, "project capability change after reconnect", 15000);
      } catch (error) {
        throw new Error(`${error.message}: ${JSON.stringify(observedProject)}`);
      }
      await ctx.waitFor(async () => await ctx.driver.evaluate(`(async () => {
      const response = await fetch(${JSON.stringify(ctx.sessionUrl("/canvas/project"))}, { cache: "no-store" });
      const value = await response.json();
      const expectedProject = value.canvas || value;
      const expected = expectedProject.capabilities || {};
      if (response.status !== 200 || (expectedProject.outputs || []).length !== 2
        || expected.preview === true || expected.designer === true) return false;
      const project = window.__jetCanvasCapabilities || {};
      const sameCapabilities = Object.keys(expected).length === Object.keys(project).length
        && Object.entries(expected).every(([name, value]) => project[name] === value);
      const focusable = "a[href],button,input,select,textarea,[tabindex]";
      const capabilityPanels = Array.from(document.querySelectorAll("[data-capability]"));
      const panelsMatch = capabilityPanels.every((panel) => {
        const capability = panel.getAttribute("data-capability");
        const supported = expected[capability] === true;
        if (supported) return !panel.hidden && !panel.inert;
        return panel.hidden && panel.inert && !panel.matches(focusable);
      });
      const views = Array.from(document.querySelectorAll("[data-session-view]"));
      const supportedViews = views
        .filter((view) => !view.getAttribute("data-capability") || expected[view.getAttribute("data-capability")] === true)
        .map((view) => view.getAttribute("data-session-view"));
      const unsupportedViews = views
        .filter((view) => view.getAttribute("data-capability") && expected[view.getAttribute("data-capability")] !== true)
        .map((view) => view.getAttribute("data-session-view"));
      const layout = window.__jetCanvasLayout || {};
      const unsupportedPanels = Array.from(document.querySelectorAll("[data-canvas-panel][data-capability]"))
        .filter((panel) => expected[panel.getAttribute("data-capability")] !== true)
        .map((panel) => panel.getAttribute("data-canvas-panel"));
      return sameCapabilities
        && panelsMatch
        && supportedViews.every((view) => layout.views?.includes(view))
        && unsupportedViews.every((view) => !layout.views?.includes(view))
        && unsupportedPanels.every((panel) => !layout.panels?.includes(panel))
        && document.getElementById("workbench-preview-label")?.hidden === (expected.preview !== true);
        })()`), "capability change after reconnect", 15000);
      const narrowedProject = await ctx.driver.evaluate(`fetch(${JSON.stringify(ctx.sessionUrl("/canvas/project"))}, { cache: "no-store" }).then((r) => r.json())`);
      const narrowedProjectPayload = canvasPayload(narrowedProject);
      if ((narrowedProjectPayload.outputs || []).length !== 2) {
        throw new Error(`capability-change project did not reload: ${JSON.stringify(narrowedProject)}`);
      }

      await secondary.driver.navigate("about:blank");
      await secondary.openCanvas();
      await secondary.waitFor(
        async () => await secondary.driver.evaluate("!!window.__jetCanvasSession"),
        "second browser reconnect",
      );
      const secondaryClientAfterReconnect = await secondary.driver.evaluate("window.__jetCanvasSessionApi.clientId()");
      const secondaryReconnected = await secondary.driver.evaluate("window.__jetCanvasSession");
      const secondaryProject = await secondary.driver.evaluate(`fetch(${JSON.stringify(secondary.sessionUrl("/canvas/project"))}, { cache: "no-store" }).then((r) => r.json())`);
      const secondaryProjectPayload = canvasPayload(secondaryProject);
      const secondaryUi = await secondary.driver.evaluate(`(() => ({
        capabilities: window.__jetCanvasCapabilities || {},
        previewHidden: document.getElementById("preview-panel")?.hidden,
        designerHidden: document.querySelector('[data-session-view="designer"]')?.hidden,
        layout: window.__jetCanvasLayout || {}
      }))()`);
      if (!secondaryClientAfterReconnect || !secondaryReconnected
        || secondaryReconnected.id !== session.id
        || secondaryReconnected.sourceRevision !== session.source_revision
        || (secondaryProjectPayload.outputs || []).length !== 2
        || secondaryProjectPayload.capabilities?.preview === true
        || secondaryProjectPayload.capabilities?.designer === true
        || secondaryUi.capabilities.preview === true
        || secondaryUi.capabilities.designer === true
        || secondaryUi.previewHidden !== true
        || secondaryUi.designerHidden !== true
        || secondaryUi.layout.panels?.includes("preview")
        || secondaryUi.layout.views?.includes("designer")) {
        throw new Error(`second browser lost capability/reconnect state: ${JSON.stringify({ secondaryClientAtStart, secondaryClientAfterReconnect, secondaryReconnected, secondaryProject, secondaryUi })}`);
      }
      if (await ctx.source() !== sourceBeforeReconnect) throw new Error("hostile or reconnect checks changed Jet source");

      const beforeErrorSource = await ctx.source();
      const stale = await ctx.transaction({
      schema_version: 1,
      op: "replace_source",
      revision: "sha256-stale-surface-matrix",
      source: "fn broken("
      });
      const stalePayload = canvasPayload(stale.json);
      if (stale.ok || stalePayload?.kind !== "conflict") {
        throw new Error(`stale surface edit was not refused: ${JSON.stringify(stale)}`);
      }
      const invalid = await ctx.transaction({
      schema_version: 1,
      op: "replace_source",
      revision: canvasPayload(await ctx.graph()).revision,
      source: "fn broken("
      });
      const invalidPayload = canvasPayload(invalid.json);
      if (invalid.ok || invalidPayload?.kind !== "diagnostic" || await ctx.source() !== beforeErrorSource) {
        throw new Error(`invalid surface edit did not preserve source: ${JSON.stringify({ invalid, source: await ctx.source() })}`);
      }
      const afterError = await ctx.driver.evaluate(`fetch(${JSON.stringify(ctx.sessionUrl("/canvas/session"))}, { cache: "no-store" }).then((r) => r.json()).then((value) => value.session || value.canvas?.session || value)`);
      const refused = (afterError.history?.receipts || []).filter((receipt) => receipt.status === "refused");
      if (afterError.id !== session.id || !afterError.last_good_program
        || refused.length < 2) {
        throw new Error(`session error receipts or last-good state missing: ${JSON.stringify(afterError)}`);
      }

      const reconnectedClient = await secondary.driver.evaluate("window.__jetCanvasSessionApi.load()");
      if (!reconnectedClient || reconnectedClient.id !== session.id || reconnectedClient.source_revision !== session.source_revision
        || reconnectedClient.history.count !== afterError.history.count || reconnectedClient.clients < 2) {
        throw new Error(`reconnect created a divergent session: ${JSON.stringify({ afterError, reconnected: reconnectedClient })}`);
      }

      const primaryClient = await ctx.driver.evaluate("window.__jetCanvasSessionApi.clientId()");
      await secondary.driver.navigate("about:blank");
      await ctx.driver.navigate("about:blank");
      await sleep(1000);
      const disconnectResults = [];
      const knownClients = new Set([primaryClientAtStart, primaryClient, secondaryClientAtStart, secondaryClientAfterReconnect]);
      let shutdown = null;
      for (let attempt = 0; attempt < 20; attempt++) {
        for (const client of knownClients) {
          const disconnectClient = `${endpoint}${ctx.sessionUrl("/__jet_dev_disconnect?client=" + encodeURIComponent(client))}`;
          const response = await fetch(disconnectClient, { method: "POST", headers: validHeaders });
          if (attempt === 0) disconnectResults.push({ client, status: response.status, body: await response.text() });
          else await response.text();
        }
        const shutdownResponse = await fetch(`${endpoint}${ctx.sessionUrl("/canvas/session")}`, { cache: "no-store", headers: validHeaders });
        const shutdownValue = await shutdownResponse.json();
        shutdown = shutdownValue.session || shutdownValue.canvas?.session || shutdownValue;
        if (shutdown?.clients === 0) break;
        await sleep(100);
      }
      if (!shutdown || shutdown.id !== session.id || shutdown.clients !== 0) {
        throw new Error(`session shutdown did not release client leases: ${JSON.stringify({ shutdown, primaryClientAtStart, primaryClient, secondaryClientAtStart, secondaryClientAfterReconnect, disconnectResults })}`);
      }
      await ctx.screenshot("session-surface-matrix");
    } finally {
      await secondary.close();
    }
  },

  "keyboard-cheat-sheet-accessibility-states": async (ctx) => {
    await ctx.openCanvas();
    if (await ctx.driver.evaluate("document.getElementById('first-run-tour')?.classList.contains('is-open')")) {
      await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide before keyboard states");
    }
    await clickElement(ctx, `document.getElementById("jet-canvas-view")`, "focus Canvas graph");
    await ctx.driver.press("?");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const dialog = document.getElementById("keyboard-cheat-sheet");
      return !!dialog && dialog.open && document.activeElement?.id === "keyboard-cheat-sheet-close";
    })()`), "keyboard cheat sheet focus");
    const sheet = await ctx.driver.evaluate(`(() => {
      const dialog = document.getElementById("keyboard-cheat-sheet");
      return {
        role: dialog?.getAttribute("role") || "dialog",
        modal: dialog?.getAttribute("aria-modal"),
        hidden: dialog?.getAttribute("aria-hidden"),
        labelledby: dialog?.getAttribute("aria-labelledby"),
        describedby: dialog?.getAttribute("aria-describedby"),
        text: dialog?.textContent || "",
        shortcutRows: dialog?.querySelectorAll(".keyboard-shortcut-row").length || 0
      };
    })()`);
    if (sheet.role !== "dialog" || sheet.modal !== "true" || sheet.hidden !== "false"
      || sheet.labelledby !== "keyboard-cheat-sheet-title"
      || sheet.describedby !== "keyboard-cheat-sheet-note"
      || sheet.shortcutRows < 20
      || !sheet.text.includes("Undo source edit")
      || !sheet.text.includes("Paste as staged")) {
      throw new Error(`keyboard cheat sheet is not accessible or complete: ${JSON.stringify(sheet)}`);
    }
    await ctx.driver.press("Escape");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const dialog = document.getElementById("keyboard-cheat-sheet");
      return !!dialog && !dialog.open && dialog.getAttribute("aria-hidden") === "true" && document.activeElement?.id === "jet-canvas-view";
    })()`), "keyboard cheat sheet close and focus restore");

    await clickElement(ctx, `document.querySelector("#more-tools-toggle")`, "open Canvas tools");
    await clickElement(ctx, `document.getElementById("keyboard-help")`, "open keyboard cheat sheet button");
    await ctx.waitFor(async () => await ctx.driver.evaluate("document.getElementById('keyboard-cheat-sheet')?.open === true"), "keyboard help button");
    await clickElement(ctx, `document.getElementById("keyboard-cheat-sheet-close")`, "close keyboard cheat sheet button");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const dialog = document.getElementById("keyboard-cheat-sheet");
      return !!dialog && !dialog.open && document.activeElement?.id === "keyboard-help";
    })()`), "keyboard help button focus restore");

    const stateA11y = async (kind, label) => {
      await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState?.kind === ${JSON.stringify(kind)}`), label);
      const state = await ctx.driver.evaluate(`(() => {
        const banner = document.getElementById("canvas-state");
        return {
          state: window.__jetCanvasCanvasState,
          role: banner?.getAttribute("role"),
          live: banner?.getAttribute("aria-live"),
          atomic: banner?.getAttribute("aria-atomic"),
          hidden: banner?.getAttribute("aria-hidden"),
          labelledby: banner?.getAttribute("aria-labelledby"),
          describedby: banner?.getAttribute("aria-describedby"),
          buttons: Array.from(banner?.querySelectorAll("button") || []).map((button) => button.textContent)
        };
      })()`);
      if (!state.state.actions.length || state.role !== "status" || state.live !== "polite"
        || state.atomic !== "true" || state.hidden !== "false"
        || state.labelledby !== "canvas-state-title"
        || state.describedby !== "canvas-state-detail"
        || state.buttons.length !== state.state.actions.length) {
        throw new Error(`${label} is not accessible or actionable: ${JSON.stringify(state)}`);
      }
      return state;
    };

    const history = await ctx.driver.evaluate("window.__jetCanvasCanvasStateHistory || []");
    const loading = history.find((entry) => entry.kind === "loading");
    if (!loading || !loading.actions.includes("Retry")) {
      throw new Error(`loading state lacked retry action: ${JSON.stringify(history)}`);
    }

    const original = await ctx.source();
    await ctx.setSourceEditor("// Empty Canvas source.\n");
    const emptyToolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!emptyToolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open empty-source tools");
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "apply empty source through editor");
    await stateA11y("empty", "empty state");
    await clickElement(ctx, `Array.from(document.querySelectorAll("#canvas-state button")).find((button) => button.textContent === "Open source")`, "empty state source recovery");
    await stateA11y("recovery", "empty state recovery");
    if (!await ctx.driver.evaluate(`(() => getComputedStyle(document.getElementById("source-editor")).display !== "none")()`)) {
      throw new Error("empty state recovery did not expose source editor");
    }
    await ctx.setSourceEditor(original);
    const restoreToolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!restoreToolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open recovery tools");
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "restore source after empty state");
    await ctx.waitForCanvas();

    await ctx.setSourceEditor(original.replace("square(limit)", "square(missing_value)"));
    await clickElement(ctx, `document.getElementById("check-current")`, "check invalid source");
    const invalid = await stateA11y("invalid", "invalid state");
    if (!invalid.state.detail.includes("last valid source") || !invalid.buttons.includes("Open source")) {
      throw new Error(`invalid state hid source recovery: ${JSON.stringify(invalid)}`);
    }
    await clickElement(ctx, `Array.from(document.querySelectorAll("#canvas-state button")).find((button) => button.textContent === "Open source")`, "invalid source recovery");
    await stateA11y("recovery", "invalid source recovery");
    await clickElement(ctx, `Array.from(document.querySelectorAll("#canvas-state button")).find((button) => button.textContent === "Close")`, "close source recovery state");

    await ctx.driver.evaluate("window.dispatchEvent(new Event('offline'))");
    const offline = await stateA11y("offline", "offline state");
    if (!offline.state.detail.includes("source") || !offline.buttons.includes("Retry")) {
      throw new Error(`offline state lacked source and retry: ${JSON.stringify(offline)}`);
    }
    await ctx.driver.evaluate("window.dispatchEvent(new Event('online'))");
    await ctx.waitFor(async () => await ctx.driver.evaluate("!window.__jetCanvasCanvasState || window.__jetCanvasCanvasState.kind !== 'offline'"), "offline recovery");

    const permissionToolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (permissionToolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "close source tools before permission action");
    await ctx.driver.shortcut(["Control", "p"]);
    await ctx.waitFor(async () => await ctx.driver.evaluate("!!document.getElementById('action-palette-search')"), "permission action palette");
    await ctx.driver.type("service");
    await ctx.waitFor(async () => await ctx.driver.evaluate("!!document.querySelector('#context-menu [data-available=\"false\"]')"), "permission-denied action");
    await clickElement(ctx, `(() => {
      const button = document.querySelector('#context-menu [data-available="false"]');
      if (!button) return null;
      const rect = button.getBoundingClientRect();
      return document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    })()`, "permission-denied action");
    const permission = await stateA11y("permission", "permission state");
    if (!permission.state.detail.toLowerCase().includes("service") || !permission.buttons.includes("Open source")) {
      throw new Error(`permission state lacked reason or recovery: ${JSON.stringify(permission)}`);
    }
  },

  "review-diff-overlays": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("review-view-button")`, "single-file Review lens");
    await ctx.waitFor(async () => {
      const review = await ctx.driver.evaluate("window.__jetCanvasTest && window.__jetCanvasTest.review");
      return review && review.active && review.available === false;
    }, "single-file Review empty state");
    const singleEmpty = await ctx.driver.evaluate(`(() => ({
      text: document.getElementById("review-content").textContent,
      devText: document.getElementById("review-dev-facts").textContent,
      devDisplay: getComputedStyle(document.getElementById("review-dev-facts")).display,
    }))()`);
    if (!singleEmpty.text.includes("single-file") || !singleEmpty.text.includes("no Git text baseline")) {
      throw new Error(`single-file Review state was not plain: ${JSON.stringify(singleEmpty)}`);
    }
    if (singleEmpty.devText || singleEmpty.devDisplay !== "none") {
      throw new Error(`single-file Review exposed developer facts: ${JSON.stringify(singleEmpty)}`);
    }

    const prepared = await prepareReviewGitProject(ctx);
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("review-view-button")`, "Review lens");
    await ctx.waitFor(async () => {
      const review = await ctx.driver.evaluate("window.__jetCanvasTest && window.__jetCanvasTest.review");
      return review && review.active && review.dirtyFiles === 2 && review.files.length === 2 && review.selectedHunkId;
    }, "dirty two-file Review lens");
    const first = await ctx.driver.evaluate("window.__jetCanvasTest.review");
    const hunks = first.files.flatMap((file) => file.hunks || []);
    if (!hunks.some((hunk) => hunk.added.some((line) => line.includes('print("new")')))) {
      throw new Error(`Review missed Git addition: ${JSON.stringify(first)}`);
    }
    if (!hunks.some((hunk) => hunk.deleted.some((line) => line.includes('print("old")')))) {
      throw new Error(`Review missed Git deletion: ${JSON.stringify(first)}`);
    }
    if (!hunks.some((hunk) => hunk.status === "deleted" && hunk.nodeIds.length === 0 && hunk.deleted.some((line) => line.includes("remove-me")))) {
      throw new Error(`Review did not keep deleted text node-free: ${JSON.stringify(first)}`);
    }
    if (!hunks.some((hunk) => hunk.status === "unprojectable" && hunk.nodeIds.length === 0 && hunk.added.some((line) => line.includes('print("new")')))) {
      throw new Error(`Review did not mark text-only changes unprojectable: ${JSON.stringify(first)}`);
    }
    const mapped = hunks.find((hunk) => hunk.nodeIds && hunk.nodeIds.length);
    if (!mapped) throw new Error(`Review did not map a changed hunk to a graph node: ${JSON.stringify(first)}`);
    const mappedText = mapped.added[0] && mapped.added[0].trim();
    await clickAttribute(ctx, "data-review-source", mapped.id, "review source action");
    await ctx.waitFor(async () => {
      const review = await ctx.driver.evaluate("window.__jetCanvasTest && window.__jetCanvasTest.review");
      return review && !review.active && await ctx.driver.evaluate(`window.__jetCanvasLensMode === 'code' && document.getElementById('source-view').textContent.includes(${JSON.stringify(mappedText)})`);
    }, "review source navigation");
    await clickElement(ctx, `document.getElementById("review-view-button")`, "Review lens after source navigation");
    await ctx.waitFor(async () => (await ctx.driver.evaluate("window.__jetCanvasTest && window.__jetCanvasTest.review"))?.active, "Review lens after source navigation");
    await clickAttribute(ctx, "data-review-graph", mapped.id, "review graph action");
    await ctx.waitFor(async () => {
      const review = await ctx.driver.evaluate("window.__jetCanvasTest && window.__jetCanvasTest.review");
      return review && !review.active && review.overlayHunkId === mapped.id && review.selectedNodeIds.length > 0;
    }, "review graph overlay");
    const sourceBeforeRefresh = await ctx.source();
    if (sourceBeforeRefresh !== prepared.dirtyMain) throw new Error("Review graph action changed source bytes");

    const refreshedMain = prepared.dirtyMain.replace('print("new")', 'print("refreshed")');
    await writeFile(join(prepared.root, "main.jet"), refreshedMain);
    await clickElement(ctx, `document.getElementById("review-view-button")`, "Review lens after external edit");
    await clickElement(ctx, `document.getElementById("review-refresh")`, "Review refresh");
    await ctx.waitFor(async () => {
      const review = await ctx.driver.evaluate("window.__jetCanvasTest && window.__jetCanvasTest.review");
      return review && review.dirtyFiles === 2 && review.files.some((file) => file.hunks.some((hunk) => hunk.added.some((line) => line.includes('print("refreshed")')))) && review.overlayHunkId === null;
    }, "recomputed Review after external edit");
    const after = await ctx.driver.evaluate("window.__jetCanvasTest.review");
    if (after.files.some((file) => file.hunks.some((hunk) => hunk.added.some((line) => line.includes('print("new")'))))) {
      throw new Error(`Review kept stale hunk after refresh: ${JSON.stringify(after)}`);
    }

    await execFileAsync("git", ["add", "main.jet", "helper.jet"], { cwd: prepared.root });
    await execFileAsync("git", ["commit", "-m", "review-clean"], { cwd: prepared.root });
    await clickElement(ctx, `document.getElementById("review-refresh")`, "Review clean refresh");
    await ctx.waitFor(async () => {
      const review = await ctx.driver.evaluate("window.__jetCanvasTest && window.__jetCanvasTest.review");
      const text = await ctx.driver.evaluate("document.getElementById('review-content').textContent");
      return review && review.active && review.available && review.dirtyFiles === 0 && text.includes("Git text truth reports a clean project");
    }, "clean project Review empty state");
    const cleanEmpty = await ctx.driver.evaluate(`(() => ({
      text: document.getElementById("review-content").textContent,
      devText: document.getElementById("review-dev-facts").textContent,
      devDisplay: getComputedStyle(document.getElementById("review-dev-facts")).display,
    }))()`);
    if (cleanEmpty.devText || cleanEmpty.devDisplay !== "none") {
      throw new Error(`clean Review exposed developer facts: ${JSON.stringify(cleanEmpty)}`);
    }
    await clickElement(ctx, `document.getElementById("developer-mode")`, "Review developer mode");
    await ctx.waitFor(async () => {
      const facts = await ctx.driver.evaluate(`(() => ({
        text: document.getElementById("review-dev-facts").textContent,
        display: getComputedStyle(document.getElementById("review-dev-facts")).display,
      }))()`);
      return facts.display !== "none" && facts.text.includes("jet.canvas.source_control") && facts.text.includes("sha256-");
    }, "Review developer facts");
    await clickElement(ctx, `document.getElementById("developer-mode")`, "Review developer mode off");
    await ctx.waitFor(async () => {
      const facts = await ctx.driver.evaluate(`(() => ({
        text: document.getElementById("review-dev-facts").textContent,
        display: getComputedStyle(document.getElementById("review-dev-facts")).display,
      }))()`);
      return facts.display === "none" && !facts.text;
    }, "Review developer facts hidden");
    await ctx.screenshot("review-refreshed");
  },

  "pan-zoom-fit": async (ctx) => {
    await ctx.openCanvas();
    const before = await ctx.state();
    await ctx.driver.wheel(320, 240, -360);
    await ctx.driver.drag({ x: 360, y: 260 }, { x: 470, y: 310 });
    await sleep(120);
    const changed = await ctx.state();
    if (changed.view.zoom <= before.view.zoom) throw new Error("wheel did not zoom Canvas");
    const fitButton = await ctx.driver.evaluate(`(() => {
      const r = document.getElementById("fit").getBoundingClientRect();
      return [r.left + r.width / 2, r.top + r.height / 2];
    })()`);
    await ctx.driver.click(fitButton[0], fitButton[1]);
    await sleep(120);
    const fit = await ctx.state();
    if (fit.view.zoom === changed.view.zoom && fit.view.x === changed.view.x && fit.view.y === changed.view.y) {
      throw new Error("fit did not change view");
    }
  },

  "click-select-details": async (ctx) => {
    await ctx.openCanvas();
    await clickSelectDetails(ctx);
    await ctx.screenshot("selected-square");
  },

  "node-drag-persists-without-source-change": async (ctx) => {
    await ctx.openCanvas();
    const sourceBefore = await ctx.source();
    const before = await ctx.state();
    const target = Object.values(before.nodeBounds || {}).find((node) => node.title === "square")
      || Object.values(before.nodeBounds || {})[0];
    if (!target) throw new Error("no node available for drag");

    // CDP emits the same client-coordinate pointer stream as a user drag.
    const requested = { x: 37, y: 29 };
    const rect = await ctx.canvasRect();
    const from = {
      x: rect.left + target.x + target.w / 2,
      y: rect.top + target.y + target.h / 2,
    };
    await ctx.driver.drag(from, {
      x: from.x + requested.x,
      y: from.y + requested.y,
    }, 16);
    await sleep(150);

    const after = await ctx.state();
    const moved = after.nodeBounds && after.nodeBounds[target.node_id];
    if (!moved) throw new Error(`dragged node disappeared: ${target.node_id}`);
    const delta = { x: moved.x - target.x, y: moved.y - target.y };
    const tolerance = 0.75;
    if (Math.abs(delta.x - requested.x) > tolerance || Math.abs(delta.y - requested.y) > tolerance) {
      throw new Error(`node drag delta ${JSON.stringify(delta)} != requested ${JSON.stringify(requested)} within ${tolerance}; before=${JSON.stringify(target)} after=${JSON.stringify(moved)} view=${JSON.stringify(before.view)} -> ${JSON.stringify(after.view)}`);
    }
    const sourceAfter = await ctx.source();
    if (sourceAfter !== sourceBefore) throw new Error("node drag changed Jet source bytes");

    await ctx.openCanvas();
    const reloaded = await ctx.state();
    const persisted = reloaded.nodeBounds && reloaded.nodeBounds[target.node_id];
    if (!persisted) throw new Error(`dragged node missing after reload: ${target.node_id}`);
    if (Math.abs(persisted.x - moved.x) > tolerance || Math.abs(persisted.y - moved.y) > tolerance) {
      throw new Error(`node bounds did not persist across reload: moved=${JSON.stringify(moved)} reloaded=${JSON.stringify(persisted)}`);
    }
    const sourceReloaded = await ctx.source();
    if (sourceReloaded !== sourceBefore) throw new Error("reload after node drag changed Jet source bytes");
  },

  "selection-marquee-modifiers-local-move": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    await ctx.switchGraph("summarize");
    await sleep(160);
    let state = await ctx.state();
    const nodes = Object.values(state.nodeBounds || {});
    const total = nodes.find((node) => node.title === "total" && node.kind === "binding")
      || nodes.find((node) => node.title === "total");
    const square = nodes.find((node) => node.title === "square");
    if (!total || !square) throw new Error(`selection fixture nodes missing: ${JSON.stringify(nodes.map((node) => node.title))}`);
    const rect = await ctx.canvasRect();
    const center = (node) => ({ x: rect.left + node.x + node.w / 2, y: rect.top + node.y + node.h / 2 });

    await ctx.driver.click(center(total).x, center(total).y);
    await sleep(100);
    await canvasModifiedClick(ctx, center(square), 8); // Shift: additive selection.
    state = await ctx.state();
    const additive = new Set(state.selectedNodeIds || []);
    if (!additive.has(total.node_id) || !additive.has(square.node_id)) {
      throw new Error(`shift-click did not add to selection: ${JSON.stringify(state.selectedNodeIds)}`);
    }
    await canvasModifiedClick(ctx, center(square), 2); // Control: toggle the selected node off.
    state = await ctx.state();
    if (new Set(state.selectedNodeIds || []).size !== 1 || state.selectedNodeIds[0] !== total.node_id) {
      throw new Error(`ctrl-click did not toggle selection: ${JSON.stringify(state.selectedNodeIds)}`);
    }

    const minX = Math.min(total.x, square.x);
    const minY = Math.min(total.y, square.y);
    const maxX = Math.max(total.x + total.w, square.x + square.w);
    const maxY = Math.max(total.y + total.h, square.y + square.h);
    const blank = await ctx.driver.evaluate(`(() => {
      const canvas = document.getElementById("jet-canvas-view");
      const r = canvas.getBoundingClientRect();
      const bounds = window.__jetCanvasNodeBounds || {};
      const pinBoxes = Object.values((window.__jetCanvasTest && window.__jetCanvasTest.pinPoints) || {})
        .map((pin) => ({ x: pin.canvas_x - 12, y: pin.canvas_y - 12, w: 24, h: 24 }));
      const candidates = [
        { x: Math.max(2, ${maxX} + 24), y: Math.max(2, ${maxY} + 24) },
        { x: Math.max(2, ${maxX} + 80), y: Math.max(2, ${maxY} + 80) },
        { x: Math.max(2, ${minX} - 24), y: Math.max(2, ${minY} - 24) },
        { x: r.width - 4, y: r.height - 4 },
        { x: 4, y: 4 },
      ];
      const inside = (point, box) => point.x >= box.x && point.x <= box.x + box.w && point.y >= box.y && point.y <= box.y + box.h;
      return candidates.find((point) => point.x > 1 && point.y > 1 && point.x < r.width - 1 && point.y < r.height - 1
        && point.x > ${maxX} && point.y > ${maxY}
        && !Object.values(bounds).some((box) => inside(point, box))
        && !pinBoxes.some((pin) => inside(point, pin))) || null;
    })()`);
    if (!blank) throw new Error("could not find a blank canvas point for marquee");
    await canvasModifiedDrag(
      ctx,
      { x: rect.left + blank.x, y: rect.top + blank.y },
      { x: rect.left + minX - 12, y: rect.top + minY - 12 },
    );
    state = await ctx.state();
    const marquee = new Set(state.selectedNodeIds || []);
    if (!marquee.has(total.node_id) || !marquee.has(square.node_id)) {
      throw new Error(`marquee did not select both nodes: ${JSON.stringify(state.selectedNodeIds)}`);
    }

    const beforeEscapeSelection = [...(state.selectedNodeIds || [])].sort();
    const beforeEscapeBounds = Object.fromEntries([total, square].map((node) => {
      const bound = state.nodeBounds[node.node_id];
      return [node.node_id, { x: bound.x, y: bound.y }];
    }));
    const escapeTotal = state.nodeBounds[total.node_id];
    const escapeFrom = center(escapeTotal);
    await ctx.driver.send("Input.dispatchMouseEvent", {
      type: "mousePressed", x: escapeFrom.x, y: escapeFrom.y, button: "left", buttons: 1, clickCount: 1, modifiers: 2,
    }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchMouseEvent", {
      type: "mouseMoved", x: escapeFrom.x + 23, y: escapeFrom.y + 17, button: "left", buttons: 1, modifiers: 2,
    }, ctx.driver.pageSession);
    await ctx.driver.press("Escape");
    await ctx.driver.send("Input.dispatchMouseEvent", {
      type: "mouseReleased", x: escapeFrom.x + 23, y: escapeFrom.y + 17, button: "left", buttons: 0, clickCount: 1,
    }, ctx.driver.pageSession);
    await sleep(100);
    state = await ctx.state();
    if (JSON.stringify([...(state.selectedNodeIds || [])].sort()) !== JSON.stringify(beforeEscapeSelection)) {
      throw new Error(`Escape did not restore selection: ${JSON.stringify(state.selectedNodeIds)}`);
    }
    for (const node of [total, square]) {
      const restored = state.nodeBounds[node.node_id];
      const before = beforeEscapeBounds[node.node_id];
      if (Math.abs(restored.x - before.x) > 1 || Math.abs(restored.y - before.y) > 1) {
        throw new Error(`Escape changed ${node.title} position: ${JSON.stringify(restored)} vs ${JSON.stringify(before)}`);
      }
    }

    const sourceBefore = await ctx.source();
    const selectedBeforeMove = Object.fromEntries([total, square].map((node) => [node.node_id, { x: node.x, y: node.y }]));
    const move = { x: 41, y: 27 };
    await canvasModifiedDrag(ctx, center(total), { x: center(total).x + move.x, y: center(total).y + move.y });
    state = await ctx.state();
    for (const node of [total, square]) {
      const moved = state.nodeBounds[node.node_id];
      const delta = { x: moved.x - selectedBeforeMove[node.node_id].x, y: moved.y - selectedBeforeMove[node.node_id].y };
      if (Math.abs(delta.x - move.x) > 1 || Math.abs(delta.y - move.y) > 1) {
        throw new Error(`multi-selection move lost ${node.title}: ${JSON.stringify(delta)}`);
      }
    }
    if (await ctx.source() !== sourceBefore) throw new Error("local selection move changed source bytes");
    await ctx.openCanvas();
    state = await ctx.state();
    for (const node of [total, square]) {
      const reloaded = state.nodeBounds[node.node_id];
      const expected = selectedBeforeMove[node.node_id];
      if (Math.abs((reloaded.x - expected.x) - move.x) > 1 || Math.abs((reloaded.y - expected.y) - move.y) > 1) {
        throw new Error(`local move did not persist for ${node.title}: ${JSON.stringify(reloaded)}`);
      }
    }

  },

  "clipboard-copy-paste": async (ctx) => {
    await selectClipboardNode(ctx, "total");
    await ctx.driver.shortcut(["Control", "c"]);
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasClipboard === 'source'"), "source clipboard gesture");
    const before = await ctx.source();
    await ctx.driver.shortcut(["Control", "v"]);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return (await ctx.source()).includes("total_copy :=")
        && state.pasteRenameChips?.some((rename) => rename.from === "total" && rename.to === "total_copy")
        && Object.values(state.nodeBounds || {}).some((node) => node.title === "total_copy");
    }, "source-backed paste gesture");
    if (await ctx.source() === before) throw new Error("paste gesture did not change source");
    await assertCleanSourceSync(ctx, ["clipboard copy", "clipboard paste"]);
  },

  "clipboard-paste-as-staged": async (ctx) => {
    await selectClipboardNode(ctx, "square");
    await ctx.driver.shortcut(["Control", "c"]);
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasClipboard === 'source'"), "staged-paste clipboard gesture");
    const before = await ctx.source();
    const stagedBefore = (await ctx.state()).stagedRegistry?.length || 0;
    await ctx.driver.shortcut(["Control", "Shift", "v"]);
    await ctx.waitFor(async () => ((await ctx.state()).stagedRegistry?.length || 0) > stagedBefore, "paste as staged gesture");
    if (await ctx.source() !== before) throw new Error("paste as staged changed Jet source");
    const staged = await ctx.driver.evaluate("document.getElementById('toast').textContent");
    if (!staged.includes("staged")) throw new Error(`staged paste lacked local-state text: ${staged}`);
    await ctx.driver.press("Delete");
    await ctx.waitFor(async () => ((await ctx.state()).stagedRegistry?.length || 0) === stagedBefore, "delete staged paste");
    if (await ctx.source() !== before) throw new Error("staged paste cleanup changed Jet source");
  },

  "clipboard-mixed-selection-staged-fallback": async (ctx) => {
    await selectClipboardNode(ctx, "square");
    await ctx.driver.shortcut(["Control", "c"]);
    const before = await ctx.source();
    const stagedBefore = (await ctx.state()).stagedRegistry?.length || 0;
    await ctx.driver.shortcut(["Control", "Shift", "v"]);
    await ctx.waitFor(async () => ((await ctx.state()).stagedRegistry?.length || 0) === stagedBefore + 1, "seed staged selection");

    await canvasModifiedClick(ctx, await ctx.node("square"), 8);
    await ctx.waitFor(async () => (await ctx.state()).selectedNodeIds?.length >= 2, "mixed source and staged selection");
    await ctx.driver.shortcut(["Control", "c"]);
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasClipboard === 'staged'"), "mixed staged clipboard");
    const mixedBefore = (await ctx.state()).stagedRegistry?.length || 0;
    await ctx.driver.shortcut(["Control", "Shift", "v"]);
    await ctx.waitFor(async () => ((await ctx.state()).stagedRegistry?.length || 0) === mixedBefore + 2, "mixed staged fallback preserves selection");
    if (await ctx.source() !== before) throw new Error("mixed staged paste changed Jet source");
    await ctx.driver.press("Delete");
    await ctx.waitFor(async () => ((await ctx.state()).stagedRegistry?.length || 0) === mixedBefore, "delete mixed staged paste");
    await ctx.driver.press("Delete");
    await ctx.waitFor(async () => ((await ctx.state()).stagedRegistry?.length || 0) === stagedBefore, "delete seed staged selection");
    if (await ctx.source() !== before) throw new Error("mixed staged cleanup changed Jet source");
  },

  "clipboard-duplicate-undo-redo": async (ctx) => {
    await selectClipboardNode(ctx, "total");
    const before = await ctx.source();
    await ctx.driver.shortcut(["Control", "d"]);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return (await ctx.source()).includes("total_copy :=")
        && Object.values(state.nodeBounds || {}).some((node) => node.title === "total_copy");
    }, "duplicate gesture");
    const duplicated = await ctx.source();
    await ctx.driver.shortcut(["Control", "z"]);
    await ctx.waitFor(async () => await ctx.source() === before, "duplicate undo gesture");
    await assertCleanSourceSync(ctx, ["duplicate", "undo"]);
    await ctx.driver.shortcut(["Control", "y"]);
    await ctx.waitFor(async () => await ctx.source() === duplicated, "duplicate redo gesture");
    await assertCleanSourceSync(ctx, ["duplicate", "undo", "redo"]);
  },

  "clipboard-stale-selection-refusal": async (ctx) => {
    await selectClipboardNode(ctx, "total");
    await ctx.driver.shortcut(["Control", "c"]);
    const changed = (await ctx.source()).replace("print(summarize(4))", "print(summarize(5))");
    await ctx.replaceSource(changed);
    const beforePaste = await ctx.source();
    await ctx.driver.shortcut(["Control", "v"]);
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasCanvasState?.kind === 'stale'"), "stale clipboard refusal");
    if (await ctx.source() !== beforePaste) throw new Error("stale clipboard changed source");
    const toast = await ctx.driver.evaluate("document.getElementById('toast').textContent");
    if (!toast.includes("stale") || !toast.includes("copy")) throw new Error(`stale clipboard lacked recovery text: ${toast}`);
  },

  "clipboard-refuses-entry-selection": async (ctx) => {
    await ctx.openCanvas();
    if (await ctx.driver.evaluate("document.getElementById('first-run-tour')?.classList.contains('is-open')")) {
      await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    }
    const state = await ctx.state();
    const entry = (state.hitMap.nodes || []).find((node) => node.kind === "entry");
    if (!entry) throw new Error("entry node missing for clipboard refusal");
    const rect = await ctx.canvasRect();
    await ctx.driver.click(rect.left + entry.x + entry.w / 2, rect.top + entry.y + entry.h / 2);
    await ctx.waitFor(async () => (await ctx.state()).selectedNodeId === entry.node_id, "entry selected");
    const before = await ctx.source();
    await ctx.driver.shortcut(["Control", "c"]);
    await ctx.waitFor(async () => (await ctx.driver.evaluate("document.getElementById('toast').textContent")).includes("source-backed"), "entry copy refusal");
    if (await ctx.source() !== before) throw new Error("entry copy refusal changed source");
  },

  "read-graph-overview": async (ctx) => {
    await ctx.openCanvas();
    const overview = await ctx.driver.evaluate("window.__jetCanvasGraphOverview");
    if (!overview || !overview.title || overview.nodes < 1 || overview.exec_pins < 1) {
      throw new Error(`graph overview missing graph facts: ${JSON.stringify(overview)}`);
    }
    const tabs = await ctx.driver.evaluate("Number(window.__jetCanvasGraphTabCount || 0)");
    if (tabs < 4) throw new Error(`expected project graph tabs, saw ${tabs}`);
  },

  "component-tree-and-palette": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open component tree");
    const tree = await ctx.driver.evaluate(`(() => {
      const root = document.querySelector("[data-canvas-component-tree]");
      if (!root) return null;
      return {
        role: root.getAttribute("role"),
        sourceId: root.dataset.sourceId || "",
        revision: root.dataset.revision || "",
        files: root.querySelectorAll('[data-canvas-tree-item="file"]').length,
        functions: root.querySelectorAll('[data-canvas-tree-item="function"]').length,
        variables: root.querySelectorAll('[data-canvas-tree-item="variable"]').length,
        newFunction: !!root.querySelector("#canvas-new-function"),
        addVariable: !!root.querySelector("#canvas-add-variable"),
        sourceBacked: Array.from(root.querySelectorAll("[data-canvas-tree-item]")).every((item) =>
          item.dataset.canvasSourceBacked === "true"
          && item.dataset.canvasSourceId === root.dataset.sourceId
          && item.dataset.canvasRevision === root.dataset.revision),
      };
    })()`);
    if (!tree || tree.role !== "tree" || !tree.sourceId || !tree.revision
      || tree.files < 1 || tree.functions < 4 || tree.variables < 1
      || !tree.newFunction || !tree.addVariable || !tree.sourceBacked) {
      throw new Error(`component tree is incomplete or lost source provenance: ${JSON.stringify(tree)}`);
    }

    const clickTreeFunction = async (title) => {
      const point = await ctx.driver.evaluate(`(() => {
        const item = Array.from(document.querySelectorAll('[data-canvas-tree-item="function"]'))
          .find((candidate) => candidate.textContent.includes(${JSON.stringify(title)}));
        if (!item) return null;
        item.scrollIntoView({ block: "nearest", inline: "nearest" });
        const rect = item.getBoundingClientRect();
        return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
      })()`);
      if (!point) throw new Error(`component tree function missing: ${title}`);
      await ctx.driver.click(point.x, point.y);
      await ctx.waitFor(async () => (await ctx.state()).graphTitle === title, `component tree ${title}`);
    };

    await clickTreeFunction("summarize");
    await clickTreeFunction("scratch");
    const beforeEmptyPalette = await ctx.source();
    const emptyPoint = await ctx.driver.evaluate(`(() => {
      const canvas = document.getElementById("jet-canvas-view");
      const rect = canvas.getBoundingClientRect();
      const bounds = Object.values(window.__jetCanvasTest?.nodeBounds || {});
      const candidates = [];
      for (const y of [rect.height - 220, rect.height - 150, rect.height - 90, 180, 300]) {
        for (const x of [rect.width - 320, rect.width - 180, rect.width / 2, 120]) {
          if (x < 18 || y < 90 || x > rect.width - 18 || y > rect.height - 18) continue;
          if (x > rect.width - 220 && y > rect.height - 160) continue;
          if (bounds.some((bound) => x >= bound.x - 18 && x <= bound.x + bound.w + 18
            && y >= bound.y - 18 && y <= bound.y + bound.h + 18)) continue;
          const hit = document.elementFromPoint(rect.left + x, rect.top + y);
          if (hit === canvas) return { x: rect.left + x, y: rect.top + y };
          candidates.push({ x: rect.left + x, y: rect.top + y, hit: hit && (hit.id || hit.className || hit.tagName) });
        }
      }
      return { error: "no empty canvas point", candidates };
    })()`);
    if (!emptyPoint || emptyPoint.error) throw new Error(JSON.stringify(emptyPoint));
    await ctx.driver.rightClick(emptyPoint.x, emptyPoint.y);
    await ctx.expectMenu("Search actions");
    const emptyPalette = await ctx.driver.evaluate(`(() => ({
      categories: Array.from(document.querySelectorAll("#context-menu .action-category h3")).map((node) => node.textContent.trim()),
      focused: document.activeElement && document.activeElement.id,
      source: window.__jetCanvasTest?.source?.() || null
    }))()`);
    for (const category of ["Flow", "Variables", "Project", "Core"]) {
      if (!emptyPalette.categories.includes(category)) {
        throw new Error(`empty palette omitted ${category}: ${JSON.stringify(emptyPalette)}`);
      }
    }
    if (emptyPalette.categories.includes("Execution") || emptyPalette.focused !== "action-palette-search") {
      throw new Error(`empty palette did not use the canonical categories/search focus: ${JSON.stringify(emptyPalette)}`);
    }
    if (await ctx.source() !== beforeEmptyPalette) throw new Error("empty palette changed source");
    await ctx.driver.press("Escape");

    await ctx.loadCoreCatalog("abs");
    const beforePinPalette = await ctx.source();
    const from = await ctx.pin("limit", "limit");
    await ctx.driver.drag({ x: from.x, y: from.y }, { x: from.x + 190, y: from.y + 30 });
    await ctx.expectMenu("Search actions");
    const pinPaletteBeforeSearch = await ctx.driver.evaluate(`Array.from(document.querySelectorAll("#context-menu [data-menu-action]"))
      .map((button) => button.textContent.trim())`);
    if (pinPaletteBeforeSearch.some((title) => /^helper\b/.test(title))) {
      throw new Error(`pin palette exposed an incompatible no-input function: ${JSON.stringify(pinPaletteBeforeSearch)}`);
    }
    await ctx.type("abs");
    await ctx.expectMenu("abs");
    await expectPaletteDescriptor(ctx, "abs", "function_pure", "insert_call");
    await ctx.pickEntry("abs");
    const inserted = await ctx.source();
    if (inserted === beforePinPalette || !inserted.includes("math.abs(limit)")) {
      throw new Error(`pin palette did not write the source-backed Core node:\n${inserted}`);
    }
    const undone = await ctx.undo();
    if (undone !== beforePinPalette || await ctx.source() !== beforePinPalette) {
      throw new Error("pin palette undo did not restore exact source");
    }
    const redone = await ctx.redo();
    if (redone !== inserted || await ctx.source() !== inserted) {
      throw new Error("pin palette redo did not restore exact source");
    }
    await ctx.openCanvas();
    if (await ctx.source() !== inserted) throw new Error("component tree/palette reload changed source");
  },

  "palette-insert-core-fn": async (ctx) => {
    await ctx.openCanvas();
    await ctx.loadCoreCatalog();
    await ctx.openPinActionMenu("limit", "limit");
    await ctx.expectMenu("Search actions");
    await ctx.type("abs");
    await ctx.expectMenu("abs");
    await expectPaletteDescriptor(ctx, "abs", "function_pure", "insert_call");
    const action = await ctx.driver.evaluate(`(() => {
      const entries = window.__jetCanvasTest.actionEntries();
      return entries.find((entry) => entry.kind === "canvas.core_catalog"
        && entry.module_path === "core.math"
        && String(entry.title || "").startsWith("abs ·")) || null;
    })()`);
    if (!action || action.node_descriptor_id !== "function_pure" || action.insert_callee !== "math.abs") {
      throw new Error(`core menu action lost its checked insertion callee: ${JSON.stringify(action)}`);
    }
    await ctx.pickEntry("abs");
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasLastTxResult !== null"), "core menu success receipt");
    const receipt = await ctx.driver.evaluate(`({
      tx: window.__jetCanvasLastTx || null,
      result: window.__jetCanvasLastTxResult || null
    })`);
    if (receipt.tx?.op !== "insert_call"
      || receipt.tx?.callee !== action.insert_callee
      || receipt.result?.changed !== true) {
      throw new Error(`core menu insertion bypassed the descriptor callee source transaction: ${JSON.stringify(receipt)}`);
    }
    await ctx.expectSourceContains("use core.math as math");
    await ctx.expectSourceContains("math.abs");
    await ctx.screenshot("core-abs-inserted");

    const project = await ctx.driver.evaluate(`fetch("/canvas/project", { cache: "no-store" }).then((r) => r.json())`);
    await ctx.openPinActionMenu("limit", "limit");
    await ctx.type("abs");
    await ctx.expectMenu("abs · core.math");
    const staleSource = await ctx.source();
    const externallyChanged = `${staleSource}\n// external stale edit\n`;
    await writeFile(join(project.project_root, "main.jet"), externallyChanged);
    await ctx.driver.evaluate("window.__jetCanvasLastTx = null; window.__jetCanvasLastTxResult = null;");
    await ctx.pickEntry("abs · core.math");
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasLastTxResult !== null"), "stale core menu insertion refusal");
    const staleReceipt = await ctx.driver.evaluate(`({
      tx: window.__jetCanvasLastTx || null,
      result: window.__jetCanvasLastTxResult || null,
      state: window.__jetCanvasCanvasState || null
    })`);
    if (staleReceipt.tx?.op !== "insert_call"
      || staleReceipt.tx?.callee !== action.insert_callee
      || staleReceipt.result?.kind !== "conflict"
      || staleReceipt.state?.kind !== "stale"
      || await ctx.source() !== externallyChanged) {
      throw new Error(`stale core menu insertion bypassed the checked refusal: ${JSON.stringify(staleReceipt)}`);
    }
  },

  "palette-insert-imported-alias-function": async (ctx) => {
    await ctx.openCanvas();
    const project = await ctx.driver.evaluate(`fetch("/canvas/project", { cache: "no-store" }).then((r) => r.json())`);
    const root = project.project_root;
    await writeFile(join(root, "package.jet"), "name: \"canvas_alias_insert\"\nversion: \"0.1.0\"\n");
    const helperSource = `module tools {
    pub fn square(n: Int) Int -> {
        return n * n
    }
}
`;
    const baseSource = `use "./helper" as h

fn run() {
    limit :: 4
    print(limit)
}
`;
    await writeFile(join(root, "helper.jet"), helperSource);
    await writeFile(join(root, "main.jet"), baseSource);
    await ctx.openCanvas();

    const openSquareMenu = async () => {
      const from = await ctx.pin("limit", "limit");
      await ctx.driver.drag({ x: from.x, y: from.y }, { x: from.x + 190, y: from.y + 30 });
      await ctx.expectMenu("Search actions");
      await ctx.type("square");
      await ctx.expectMenu("square");
    };
    const clickMenuEntry = async (label) => {
      const point = await elementCenter(
        ctx,
        `Array.from(document.querySelectorAll("#context-menu [data-menu-action]")).find((button) => button.textContent.includes(${JSON.stringify(label)}))`,
        `${label} Canvas menu entry`,
      );
      await ctx.driver.click(point.x, point.y);
      await sleep(500);
      await ctx.waitForCanvas();
    };

    await openSquareMenu();
    const action = await ctx.driver.evaluate(`(() => {
      const entries = window.__jetCanvasTest.actionEntries();
      return entries.find((entry) => entry.title === "square") || null;
    })()`);
    if (!action || action.node_descriptor_id !== "function_pure" || action.insert_callee !== "h.tools.square") {
      throw new Error(`imported alias action lost descriptor callee: ${JSON.stringify(action)}`);
    }
    await clickMenuEntry("square");
    await ctx.expectSourceContains("h.tools.square(limit)");
    const successReceipt = await ctx.driver.evaluate(`({
      tx: window.__jetCanvasLastTx || null,
      result: window.__jetCanvasLastTxResult || null
    })`);
    if (successReceipt.tx?.op !== "insert_call"
      || successReceipt.tx?.callee !== action.insert_callee
      || successReceipt.result?.changed !== true) {
      throw new Error(`menu insertion did not use the descriptor callee through the source transaction: ${JSON.stringify(successReceipt)}`);
    }
    const insertedSource = await ctx.source();
    const insertedGraph = await ctx.graph();
    const insertedRun = graphByTitle(insertedGraph, "run");
    const insertedNode = (insertedRun.nodes || []).find((node) => node.title === ".square");
    if (!insertedNode || insertedNode.node_descriptor_id !== "function_pure" || !insertedNode.source_span) {
      throw new Error(`imported alias call lost graph descriptor or provenance: ${JSON.stringify(insertedNode)}`);
    }
    const insertedPin = (insertedRun.pins || []).find((pin) => pin.node_id === insertedNode.node_id && pin.name === "arg1");
    if (!insertedPin || insertedPin.type !== "Int") {
      throw new Error(`imported alias call lost typed argument pin: ${JSON.stringify(insertedPin)}`);
    }

    await ctx.undo();
    if ((await ctx.source()).includes("h.tools.square")) throw new Error("alias insertion undo did not restore source");
    await ctx.redo();
    if (await ctx.source() !== insertedSource) throw new Error("alias insertion redo changed canonical source");
    await ctx.openCanvas();
    if (await ctx.source() !== insertedSource) throw new Error("alias insertion reload changed canonical source");

    await openSquareMenu();
    const staleAction = await ctx.driver.evaluate(`(() => {
      const entries = window.__jetCanvasTest.actionEntries();
      return entries.find((entry) => entry.title === "square") || null;
    })()`);
    await writeFile(join(root, "main.jet"), insertedSource + "\n// external stale edit\n");
    const staleSource = await ctx.source();
    await ctx.driver.evaluate("window.__jetCanvasLastTx = null; window.__jetCanvasLastTxResult = null;");
    await clickMenuEntry("square");
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasLastTxResult !== null"), "stale menu insertion refusal");
    const staleReceipt = await ctx.driver.evaluate(`({
      tx: window.__jetCanvasLastTx || null,
      result: window.__jetCanvasLastTxResult || null,
      state: window.__jetCanvasCanvasState || null,
      toast: window.__jetCanvasTest?.lastToast || ""
    })`);
    if (staleReceipt.tx?.op !== "insert_call"
      || staleReceipt.tx?.callee !== staleAction?.insert_callee
      || staleReceipt.result?.kind !== "conflict"
      || staleReceipt.state?.kind !== "stale"
      || !/stale|source changed|unchanged/i.test(`${staleReceipt.state?.detail || ""} ${staleReceipt.toast}`)
      || await ctx.source() !== staleSource) {
      throw new Error(`stale menu insertion did not preserve the source and show recovery: ${JSON.stringify(staleReceipt)}`);
    }

    const invalidHelperSource = helperSource.replace("n: Int", "n: String").replace("return n * n", "return 1");
    await writeFile(join(root, "helper.jet"), helperSource);
    await writeFile(join(root, "main.jet"), baseSource);
    await ctx.openCanvas();
    await openSquareMenu();
    const invalidAction = await ctx.driver.evaluate(`(() => {
      const entries = window.__jetCanvasTest.actionEntries();
      return entries.find((entry) => entry.title === "square") || null;
    })()`);
    if (!invalidAction || invalidAction.insert_callee !== "h.tools.square") {
      throw new Error(`ill-typed menu action lost descriptor callee before refusal: ${JSON.stringify(invalidAction)}`);
    }
    const unchangedSource = await ctx.source();
    await writeFile(join(root, "helper.jet"), invalidHelperSource);
    await ctx.driver.evaluate("window.__jetCanvasLastTx = null; window.__jetCanvasLastTxResult = null");
    await clickMenuEntry("square");
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasLastTxResult !== null"), "ill-typed menu insertion refusal");
    const invalidReceipt = await ctx.driver.evaluate(`({
      tx: window.__jetCanvasLastTx || null,
      result: window.__jetCanvasLastTxResult || null,
      state: window.__jetCanvasCanvasState || null
    })`);
    const problem = await ctx.expectProblem();
    if (invalidReceipt.tx?.op !== "insert_call"
      || invalidReceipt.tx?.callee !== invalidAction.insert_callee
      || invalidReceipt.result?.kind !== "diagnostic"
      || !Array.isArray(invalidReceipt.result?.diagnostics)
      || invalidReceipt.state?.kind !== "invalid"
      || !String(problem?.rendered || "").includes("Why:")
      || !String(problem?.rendered || "").includes("Fix:")
      || await ctx.source() !== unchangedSource) {
      throw new Error(`ill-typed menu insertion did not show the checked refusal or preserve source: ${JSON.stringify({ invalidReceipt, problem })}`);
    }
  },

  "library-panel": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open library panel");
    const project = await ctx.driver.evaluate(`fetch("/canvas/project", { cache: "no-store" }).then((r) => r.json())`);
    const root = project.project_root;
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const panel = window.__jetCanvasLibraryPanel || null;
      return !!panel && panel.rendered && panel.actionCount > 0 && !!document.querySelector('[data-canvas-library]');
    })()`), "library panel projection");
    const initial = await ctx.driver.evaluate(`(() => {
      const panel = window.__jetCanvasLibraryPanel || {};
      return {
        panel,
        modules: Array.from(document.querySelectorAll('[data-library-module]')).map((module) => module.getAttribute('data-library-module')),
        packages: document.querySelector('[data-canvas-library] .library-packages')?.textContent || '',
        source: document.querySelector('[data-library-status]')?.textContent || ''
      };
    })()`);
    if (!initial.modules.includes("core.event") || !initial.modules.includes("core.math")) {
      throw new Error(`library panel omitted Core modules: ${JSON.stringify(initial)}`);
    }
    if (!initial.panel.modules.some((module) => module.entries.some((entry) => entry.title === "scope" && entry.signature.includes("scope")))) {
      throw new Error(`library panel omitted typed event scope metadata: ${JSON.stringify(initial.panel)}`);
    }
    if (initial.panel.packages !== 1 || !initial.packages.includes("canvas_library")) {
      throw new Error(`library panel omitted project package facts: ${JSON.stringify(initial)}`);
    }

    const stageable = initial.panel.modules
      .flatMap((module) => module.entries)
      .find((entry) => entry.stageable && entry.action_id);
    if (!stageable) throw new Error("library panel omitted a stageable checked entry");
    const beforeStage = await ctx.source();
    await replaceSearch(ctx, "document.querySelector('[data-library-search]')", stageable.title, "stageable library search");
    const stageableSelector = `[data-library-action=${JSON.stringify(stageable.action_id)}]`;
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const button = document.querySelector(${JSON.stringify(stageableSelector)});
      return !!button && !button.disabled && button.closest('[data-library-entry]');
    })()`), `stageable library entry ${stageable.title}`);
    const stageablePoint = await elementCenter(
      ctx,
      `document.querySelector(${JSON.stringify(stageableSelector)})`,
      "stageable library entry",
    );
    await ctx.driver.click(stageablePoint.x, stageablePoint.y);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return (state.stagedRegistry || []).some((node) => String(node.title || "").includes(stageable.title));
    }, `staged library entry ${stageable.title}`);
    if (await ctx.source() !== beforeStage) throw new Error("staging a library entry changed source");
    await ctx.driver.press("Escape");
    await ctx.waitFor(async () => await ctx.driver.evaluate(
      "!document.getElementById('left-drawer')?.classList.contains('is-drawer-open')",
    ), "close library drawer before canvas gesture");

    const stateBeforeTypeRefusal = await ctx.state();
    const stagedTypeNode = (stateBeforeTypeRefusal.stagedRegistry || [])
      .find((node) => String(node.title || "").includes(stageable.title));
    const typeBase = (type) => String(type || "Value").replace(/[?!]$/, "");
    const numericTypes = new Set(["Int", "Float", "F32", "F64"]);
    const typeCompatible = (output, input) => {
      const outputBase = typeBase(output);
      const inputBase = typeBase(input);
      return outputBase === inputBase
        || outputBase === "Any" || inputBase === "Any"
        || outputBase === "Value" || inputBase === "Value"
        || numericTypes.has(outputBase) && numericTypes.has(inputBase);
    };
    const stagedTypeInputs = (stagedTypeNode && stagedTypeNode.pins || [])
      .filter((pin) => pin.direction === "input" && pin.type && !["exec", "control"].includes(String(pin.type).toLowerCase()));
    const incompatiblePair = stagedTypeInputs.map((input) => ({
      input,
      output: (stateBeforeTypeRefusal.hitMap?.pins || []).find((pin) => pin.node_id !== stagedTypeNode.node_id
        && pin.direction === "output"
        && pin.type
        && !["exec", "control"].includes(String(pin.type).toLowerCase())
        && !typeCompatible(pin.type, input.type))
    })).find((pair) => pair.output);
    const stagedTypeInput = incompatiblePair?.input;
    const incompatibleOutput = incompatiblePair?.output;
    if (!stagedTypeNode || !stagedTypeInput) {
      throw new Error(`staged library entry lacks a typed data input with an incompatible saved output: ${JSON.stringify(stagedTypeNode)}`);
    }
    const incompatibleOutputPoint = incompatibleOutput
      && stateBeforeTypeRefusal.pinPoints?.[incompatibleOutput.pin_id];
    const stagedTypeInputPoint = stateBeforeTypeRefusal.pinPoints?.[stagedTypeInput.pin_id];
    if (!incompatibleOutputPoint || !stagedTypeInputPoint) {
      throw new Error(`library type-refusal gesture pins missing: ${JSON.stringify({ stagedTypeInput, incompatibleOutput })}`);
    }
    await ctx.driver.evaluate(`window.__jetCanvasLastTx = null; window.__jetCanvasLastTxResult = null;`);
    const beforeTypeRefusal = await ctx.source();
    await ctx.driver.drag(
      { x: incompatibleOutputPoint.client_x, y: incompatibleOutputPoint.client_y },
      { x: stagedTypeInputPoint.client_x, y: stagedTypeInputPoint.client_y },
      16,
    );
    await ctx.waitFor(async () => {
      const toast = await ctx.driver.evaluate("document.getElementById('toast')?.textContent || ''");
      return /type mismatch|cannot connect|wire refused/i.test(toast);
    }, "library ill-typed staged refusal");
    const typeRefusal = await ctx.driver.evaluate(`({
      toast: document.getElementById('toast')?.textContent || '',
      tx: window.__jetCanvasLastTx || null,
      staged: (window.__jetCanvasStagedRegistry || []).some((node) => node.node_id === ${JSON.stringify(stagedTypeNode.node_id)})
    })`);
    if (await ctx.source() !== beforeTypeRefusal) throw new Error("ill-typed library gesture changed source");
    if (typeRefusal.tx || !typeRefusal.staged) {
      throw new Error(`ill-typed library gesture was not pre-sema and recoverable: ${JSON.stringify(typeRefusal)}`);
    }

    const before = await ctx.source();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "reopen library panel");
    await replaceSearch(ctx, "document.querySelector('[data-library-search]')", "abs", "library search");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const button = document.querySelector('[data-library-action="canvas.core_catalog:core.math:abs"]');
      return !!button && !button.disabled && button.closest('[data-library-entry]');
    })()`), "available library entry");
    const absPoint = await elementCenter(
      ctx,
      "document.querySelector('[data-library-action=\"canvas.core_catalog:core.math:abs\"]')",
      "available library entry",
    );
    await ctx.driver.evaluate("window.__jetCanvasLastTx = null; window.__jetCanvasLastTxResult = null;");
    await ctx.driver.click(absPoint.x, absPoint.y);
    try {
      await ctx.waitFor(async () => {
        const source = await ctx.source();
        return source.includes("use core.math as math") && source.includes("canvas_value :: math.abs(1)");
      }, "library source transaction");
    } catch (error) {
      const diagnostic = await ctx.driver.evaluate(`(() => ({
        source: window.__jetCanvasTest?.source?.() || null,
        tx: window.__jetCanvasLastTx || null,
        result: window.__jetCanvasLastTxResult || null,
        state: window.__jetCanvasCanvasState || null,
        toast: document.getElementById('toast')?.textContent || ''
      }))()`);
      throw new Error(`${error.message}: ${JSON.stringify(diagnostic)}`);
    }
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasLastTxResult !== null"), "library success receipt");
    const successReceipt = await ctx.driver.evaluate(`({
      tx: window.__jetCanvasLastTx || null,
      result: window.__jetCanvasLastTxResult || null
    })`);
    if (successReceipt.tx?.op !== "insert_call"
      || !["core.math.abs", "math.abs"].includes(successReceipt.tx?.callee)
      || successReceipt.result?.changed !== true) {
      throw new Error(`library success bypassed the source transaction path: ${JSON.stringify(successReceipt)}`);
    }
    const created = await ctx.source();
    const inserted = await ctx.graph();
    const insertedState = await ctx.driver.evaluate(`(() => {
      const state = window.__jetCanvasTest || {};
      return {
        library: state.libraryPanel || null,
        nodes: Object.values(state.nodeBounds || {}).filter((node) => String(node.title || '').includes('abs') || String(node.title || '').includes('canvas_value'))
      };
    })()`);
    const insertedAbsNode = (inserted.graphs || [])
      .flatMap((graph) => graph.nodes || [])
      .find((node) => String(node.title || '').includes('abs'));
    const insertedAbsOutput = insertedAbsNode
      && (inserted.graphs || [])
        .flatMap((graph) => graph.pins || [])
        .find((pin) => pin.node_id === insertedAbsNode.node_id && pin.direction === 'output');
    if (!insertedState.library || insertedState.library.revision !== inserted.revision || !insertedState.nodes.length
      || !insertedAbsNode?.source_span || insertedAbsOutput?.type !== 'Int') {
      throw new Error(`library insert lost graph provenance: ${JSON.stringify(insertedState)}`);
    }

    await ctx.undo();
    if (await ctx.source() !== before) throw new Error("library undo did not restore exact source");
    await ctx.redo();
    if (await ctx.source() !== created) throw new Error("library redo did not restore exact source");
    await ctx.openCanvas();
    if (await ctx.source() !== created) throw new Error("library reload changed source");
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open library panel before stale edit");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const panel = window.__jetCanvasLibraryPanel || null;
      return !!panel && panel.rendered && panel.actionCount > 0
        && !!document.querySelector('[data-library-action="canvas.core_catalog:core.math:abs"]');
    })()`), "library panel before stale edit");

    await writeFile(join(root, "main.jet"), created + "\n// external stale edit\n");
    await ctx.waitFor(async () => (await ctx.source()).includes("external stale edit"), "external stale source");
    const staleSource = await ctx.source();
    await replaceSearch(ctx, "document.querySelector('[data-library-search]')", "abs", "library search after reload");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const button = document.querySelector('[data-library-action="canvas.core_catalog:core.math:abs"]');
      return !!button && !button.disabled;
    })()`), "stale available library entry");
    const stalePoint = await elementCenter(
      ctx,
      "document.querySelector('[data-library-action=\"canvas.core_catalog:core.math:abs\"]')",
      "stale library entry",
    );
    await ctx.driver.evaluate("window.__jetCanvasLastTx = null; window.__jetCanvasLastTxResult = null;");
    await ctx.driver.click(stalePoint.x, stalePoint.y);
    await ctx.waitFor(async () => {
      const state = await ctx.driver.evaluate("window.__jetCanvasCanvasState || null");
      return state && ["stale", "error"].includes(state.kind) && /source|unchanged|stale/i.test(`${state.title} ${state.detail}`);
    }, "stale library refusal");
    await ctx.waitFor(async () => await ctx.driver.evaluate("window.__jetCanvasLastTxResult !== null"), "stale library receipt");
    const staleReceipt = await ctx.driver.evaluate(`({
      tx: window.__jetCanvasLastTx || null,
      result: window.__jetCanvasLastTxResult || null
    })`);
    if (staleReceipt.tx?.op !== "insert_call"
      || !["core.math.abs", "math.abs"].includes(staleReceipt.tx?.callee)
      || staleReceipt.result?.kind !== "conflict") {
      throw new Error(`stale library refusal bypassed the server conflict path: ${JSON.stringify(staleReceipt)}`);
    }
    if (await ctx.source() !== staleSource) throw new Error("stale library gesture changed source");
  },

  "library-panel-events": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open events library panel");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const panel = window.__jetCanvasLibraryPanel || null;
      return !!panel && panel.rendered && panel.actionCount > 0 && !!document.querySelector('[data-canvas-library]');
    })()`), "events library panel projection");
    const initial = await ctx.driver.evaluate(`(() => {
      const panel = window.__jetCanvasLibraryPanel || {};
      return {
        panel,
        modules: Array.from(document.querySelectorAll('[data-library-module]')).map((module) => module.getAttribute('data-library-module'))
      };
    })()`);
    if (!initial.modules.includes("core.event")
      || !initial.panel.modules.some((module) => module.entries.some((entry) => entry.title === "scope" && entry.signature.includes("scope")))) {
      throw new Error(`events library omitted typed event scope: ${JSON.stringify(initial)}`);
    }

    const before = await ctx.source();
    await replaceSearch(ctx, "document.querySelector('[data-library-search]')", "scope", "events library search");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const button = document.querySelector('[data-library-action="canvas.core_catalog:core.event:scope"]');
      return !!button && !button.disabled && button.closest('[data-library-entry]');
    })()`), "events library scope entry");
    const scopePoint = await elementCenter(
      ctx,
      "document.querySelector('[data-library-action=\"canvas.core_catalog:core.event:scope\"]')",
      "events library scope entry",
    );
    await ctx.driver.click(scopePoint.x, scopePoint.y);
    await ctx.waitFor(async () => (await ctx.source()).includes("canvas_value :: event.scope()"), "events library source transaction");
    const created = await ctx.source();
    const inserted = await ctx.graph();
    const insertedScopeNode = (inserted.graphs || [])
      .flatMap((graph) => graph.nodes || [])
      .find((node) => node.source_span
        && inserted.source_text.slice(node.source_span.start, node.source_span.end).includes("canvas_value :: event.scope()"));
    const insertedScopeOutput = insertedScopeNode
      && (inserted.graphs || [])
        .flatMap((graph) => graph.pins || [])
        .find((pin) => pin.node_id === insertedScopeNode.node_id && pin.direction === "output");
    if (!insertedScopeNode?.source_span || insertedScopeOutput?.type !== "EventScope") {
      throw new Error(`events library insert lost checked provenance: ${JSON.stringify({ insertedScopeNode, insertedScopeOutput })}`);
    }

    await ctx.undo();
    if (await ctx.source() !== before) throw new Error("events library undo did not restore exact source");
    await ctx.redo();
    if (await ctx.source() !== created) throw new Error("events library redo did not restore exact source");
    await ctx.openCanvas();
    if (await ctx.source() !== created) throw new Error("events library reload changed source");
  },

  "palette-insert-catalog-sweep": async (ctx) => {
    await catalogSweep(ctx);
  },

  "palette-insert-flow-variable-project-core": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    let graphDoc = await ctx.graph();
    let scratch = graphByTitle(graphDoc, "scratch");
    await uiEdit(ctx, { schema_version: 1, op: "insert_branch", revision: graphDoc.revision, graph_id: scratch.graph_id }, "flow branch insert");
    await ctx.expectSourceContains("if true");
    await expectConsumedDescriptor(ctx, "branch", { transaction: "insert_branch", glyph: "◇", defaultEditor: "inline_expr" });

    graphDoc = await ctx.graph();
    scratch = graphByTitle(graphDoc, "scratch");
    let expr = firstInline(scratch, (e) => e.source === "limit" || String(e.source || "").includes("limit"), "scratch print argument");
    await uiEdit(ctx, { schema_version: 1, op: "edit_inline_expr", revision: graphDoc.revision, inline_expr_id: expr.inline_expr_id, new_expr: "1" }, "literal edit before variable insert");
    graphDoc = await ctx.graph();
    scratch = graphByTitle(graphDoc, "scratch");
    expr = firstInline(scratch, (e) => e.source === "1", "literal print argument");
    await uiEdit(ctx, { schema_version: 1, op: "edit_inline_expr", revision: graphDoc.revision, inline_expr_id: expr.inline_expr_id, new_expr: "limit" }, "variable insert");
    await ctx.expectSourceContains("print(limit)");

    graphDoc = await ctx.graph();
    scratch = graphByTitle(graphDoc, "scratch");
    await uiEdit(ctx, { schema_version: 1, op: "insert_call", revision: graphDoc.revision, graph_id: scratch.graph_id, callee: "square", args: ["limit"], bind: "project_value" }, "project insert");
    await ctx.expectSourceContains("project_value :: square(limit)");

    await ctx.loadCoreCatalog("abs");
    await ctx.openPinActionMenu("limit", "limit");
    await ctx.type("abs");
    await ctx.expectMenu("abs");
    await expectPaletteDescriptor(ctx, "abs", "function_pure", "insert_call");
    await ctx.pickEntry("abs");
    await ctx.expectSourceContains("math.abs(limit)");
  },

  "wire-data-and-exec": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    let graphDoc = await ctx.graph();
    let scratch = graphByTitle(graphDoc, "scratch");
    await uiEdit(ctx, {
      schema_version: 1,
      op: "insert_call",
      revision: graphDoc.revision,
      graph_id: scratch.graph_id,
      callee: "print",
      args: ["\"exec\""],
      wire_origin_pin_id: `${scratch.graph_id}:entry:output:then`,
      wire_target_pin: "exec",
    }, "exec wire insert");
    await ctx.expectSourceContains("print(\"exec\")");

    graphDoc = await ctx.graph();
    scratch = graphByTitle(graphDoc, "scratch");
    const pin = (scratch.pins || []).find((p) => p.name === "limit" && p.direction === "output");
    if (!pin) throw new Error("limit data pin missing");
    await uiEdit(ctx, {
      schema_version: 1,
      op: "insert_call",
      revision: graphDoc.revision,
      graph_id: scratch.graph_id,
      callee: "square",
      args: ["limit"],
      bind: "wired_value",
      wire_origin_pin_id: pin.pin_id,
      wire_target_pin: "n",
      wire_expr: "limit",
    }, "data wire insert");
    await ctx.expectSourceContains("wired_value :: square(limit)");
  },

  "data-pin-drag-to-wire": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    await ctx.replaceSource(`fn run() {
    source :: 4
    other :: 9
    first :: 1
    second :: 2
    print(source)
    print(other)
    print(first)
    print(second)
}
    `);
    await ctx.openCanvas();
    const before = await ctx.source();
    await dragDataPin(ctx, "run", "source", "source", "first", "value", true);
    await ctx.waitFor(async () => (await ctx.source()).includes("first :: source"), "data wire source transaction");
    let graph = graphByTitle(await ctx.graph(), "run");
    if (!dataWireExists(graph, "source", "first")) throw new Error(`filled data wire missing after drag: ${JSON.stringify(graph.wires)}`);
    await assertCleanSourceSync(ctx, ["data drag source"]);

    await dragDataPin(ctx, "run", "source", "source", "second", "value");
    await ctx.waitFor(async () => (await ctx.source()).includes("second :: source"), "fan-out data wire");
    graph = graphByTitle(await ctx.graph(), "run");
    if (!dataWireExists(graph, "source", "second")) throw new Error("fan-out data wire missing");

    const beforeRewire = await ctx.source();
    await dragDataPin(ctx, "run", "other", "other", "first", "value");
    await ctx.waitFor(async () => (await ctx.source()).includes("first :: other"), "data rewire source");
    const afterRewire = await ctx.source();
    if (afterRewire === beforeRewire) throw new Error("data rewire did not change source");
    graph = graphByTitle(await ctx.graph(), "run");
    if (!dataWireExists(graph, "other", "first")) throw new Error("rewired data wire missing");

    const restored = await ctx.undo();
    if (restored !== beforeRewire) throw new Error(`data undo did not restore exact source\nexpected:\n${beforeRewire}\nactual:\n${restored}`);
    const redone = await ctx.redo();
    if (redone !== afterRewire) throw new Error("data redo did not restore rewire source");
    await assertCleanSourceSync(ctx, ["data fan-out", "data rewire", "undo", "redo"]);

    const escapeBefore = await ctx.source();
    const escapePoints = await dataPinPoints(ctx, "run", "source", "source", "second", "value");
    await ctx.driver.send("Input.dispatchMouseEvent", { type: "mousePressed", x: escapePoints.fromPoint.client_x, y: escapePoints.fromPoint.client_y, button: "left", clickCount: 1 }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: escapePoints.targetPoint.client_x, y: escapePoints.targetPoint.client_y, button: "left", buttons: 1 }, ctx.driver.pageSession);
    const previewBeforeEscape = await ctx.driver.evaluate("window.__jetCanvasWirePreview || null");
    if (!previewBeforeEscape) throw new Error("data wire preview missing before Escape");
    await ctx.driver.press("Escape");
    await ctx.driver.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: escapePoints.targetPoint.client_x, y: escapePoints.targetPoint.client_y, button: "left", clickCount: 1 }, ctx.driver.pageSession);
    await sleep(250);
    if (await ctx.source() !== escapeBefore) throw new Error("Escape changed data-wire source");
    const escaped = await ctx.driver.evaluate("({ preview: window.__jetCanvasWirePreview, focus: document.activeElement && document.activeElement.id })");
    if (escaped.preview) throw new Error(`Escape did not clear wire preview: ${JSON.stringify(escaped)}`);
    if (before === escapeBefore) throw new Error("data gesture fixture never changed source");
    await assertCleanSourceSync(ctx, ["Escape restoration"]);
  },

  "data-pin-type-gate": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    await ctx.replaceSource(`enum ParseError {
    Empty
}

fn helper(value: Int) Int -[]> {
    return value
}

fn twice(value: Int) Int -[]> {
    return value + value
}

fn use_callback(callback: fn(Int) Int -[]>) {
    print("callback")
}

fn parse(raw: String) Int ParseError! -> {
    if raw == "" -> return Err(ParseError.Empty)
    return Ok(7)
}

fn write_int(value: &Int) {}

fn run() {
    source := 4
    text :: "text"
    call_target :: 0
    seed :: twice(2)
    bad :: source
    other := 1
    use_callback(helper)
    write_int(&other)
    parsed :: parse("")
    print(source)
    print(text)
    print(bad)
    print(other)
    print(parsed)
}

fn other_graph() {
    target :: 0
    print(target)
}
`);
    await ctx.openCanvas();
    await ctx.switchGraph("run");

    await dragDataPin(ctx, "run", "twice", "result", "call_target", "value", true);
    await ctx.waitFor(async () => (await ctx.source()).includes("call_target :: twice(2)"), "call result source transaction");
    let graph = graphByTitle(await ctx.graph(), "run");
    if (!dataWireExists(graph, "twice", "call_target")) throw new Error("call-result data wire missing");
    await assertCleanSourceSync(ctx, ["call result drag"]);

    const refusal = async (fromTitle, fromPin, toTitle, toPin, reason) => {
      const before = await ctx.source();
      await dragDataPin(ctx, "run", fromTitle, fromPin, toTitle, toPin);
      await expectVisibleRefusal(ctx, reason, `${fromTitle} -> ${toTitle} refusal`);
      if (await ctx.source() !== before) throw new Error(`${fromTitle} -> ${toTitle} refusal changed source`);
      const plan = await ctx.driver.evaluate("window.__jetCanvasLastConnectionPlan || null");
      if (!plan || !String(plan.label || "").toLowerCase().includes(reason.toLowerCase())) {
        throw new Error(`refusal reason not retained: ${JSON.stringify(plan)}`);
      }
    };

    await refusal("text", "text", "bad", "value", "Type mismatch String -> Int");
    await refusal("helper", "helper", "bad", "value", "Function value cannot connect");
    await refusal("parse", "result", "bad", "value", "Fallible output cannot connect");
    await refusal("source", "source", "write_int", "arg1", "Capability mismatch");

    const cross = await dataPinPoints(ctx, "run", "text", "text", "bad", "value");
    const crossBefore = await ctx.source();
    await ctx.driver.send("Input.dispatchMouseEvent", { type: "mousePressed", x: cross.fromPoint.client_x, y: cross.fromPoint.client_y, button: "left", clickCount: 1 }, ctx.driver.pageSession);
    await ctx.switchGraph("other_graph");
    const otherGraph = graphByTitle(await ctx.graph(), "other_graph");
    const otherTarget = namedPinForNode(otherGraph, "target", "input", "value");
    await finishDataPinDrag(ctx, otherTarget);
    await expectVisibleRefusal(ctx, "different graphs", "cross-graph refusal");
    if (await ctx.source() !== crossBefore) throw new Error("cross-graph refusal changed source");

    await ctx.switchGraph("run");
    const staleStart = await beginDataPinDrag(ctx, "run", "text", "text");
    const staleBase = await ctx.source();
    const staleGraphBeforeWrite = await ctx.graph();
    const staleWrite = await ctx.uiTransaction({
      schema_version: 1,
      op: "replace_source",
      revision: staleGraphBeforeWrite.revision,
      source: staleBase.replace("other := 1", "other := 2")
    });
    await ctx.waitForCanvas();
    const staleGraph = graphByTitle(await ctx.graph(), "run");
    const staleTarget = namedPinForNode(staleGraph, "bad", "input", "value");
    const staleSource = await ctx.source();
    if (staleSource === staleBase || !staleSource.includes("other := 2")) {
      throw new Error(`stale-revision setup was lost: ${JSON.stringify({ staleWrite, staleSource })}`);
    }
    if (staleStart.fromPin.pin_id === staleTarget.pin_id) throw new Error("stale test did not use distinct pins");
    const staleBeforeDrop = await ctx.source();
    await finishDataPinDrag(ctx, staleTarget);
    await expectVisibleRefusal(ctx, "source changed since drag started", "stale-revision drop refusal");
    if (await ctx.source() !== staleBeforeDrop) throw new Error("stale-revision drop changed source");

    await ctx.openCanvas();
    await ctx.switchGraph("run");
    await ctx.driver.evaluate(`window.__jetCanvasGatePosts = 0; (() => {
      const originalFetch = window.fetch.bind(window);
      window.fetch = (input, init = {}) => {
        if (String(input).includes("/canvas/transaction")) window.__jetCanvasGatePosts += 1;
        return originalFetch(input, init);
      };
    })()`);
    const beforeInline = await ctx.source();
    await clickElement(ctx, `document.getElementById("dock-details")`, "open Details for inline gate");
    await ctx.driver.evaluate(`window.__jetCanvasTest.selectVariable("other")`);
    await ctx.waitFor(async () => await ctx.driver.evaluate(`!!document.querySelector('[data-details-input="value"]')`), "other inline editor");
    const inlineState = await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="value"]');
      const apply = document.getElementById("apply-variable-details");
      if (!input || !apply) return { ok: false };
      input.focus();
      const focused = document.activeElement === input;
      input.value = "1.5";
      apply.click();
      return { ok: true, focused };
    })()`);
    if (!inlineState.ok || !inlineState.focused) throw new Error(`inline editor keyboard focus missing: ${JSON.stringify(inlineState)}`);
    await expectVisibleRefusal(ctx, "Inline value type Float does not match Int", "inline wrong-type refusal");
    if (await ctx.source() !== beforeInline) throw new Error("inline wrong-type refusal changed source");
    const gatePosts = await ctx.driver.evaluate("window.__jetCanvasGatePosts");
    if (gatePosts !== 0) throw new Error(`client gate posted invalid inline edit: ${gatePosts}`);
    await assertSourceUnchangedAfterReload(ctx, beforeInline, "inline wrong-type refusal");
  },

  "exec-rewire-reorders-statements": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn order() {
    a :: 1
    b :: 2
    c :: 3
    print(a + b + c)
}

fn run() {
    order()
}
`);
    await ctx.openCanvas();
    const before = await ctx.source();
    await dragExecEndpoint(ctx, "order", "b", "c");
    await ctx.waitFor(async () => {
      const order = sourceNameOrder(await ctx.source(), ["a", "c", "b"]);
      return order.every((n) => n >= 0) && order[0] < order[1] && order[1] < order[2];
    }, "exec rewire source reorder");
    const after = await ctx.source();
    const order = sourceNameOrder(after, ["a", "c", "b"]);
    if (!(order[0] < order[1] && order[1] < order[2])) throw new Error(`source order did not change to A,C,B:\n${after}`);
    const graphDoc = await ctx.graph();
    const graph = graphByTitle(graphDoc, "order");
    if (!controlWireExists(graph, "a", "c")) throw new Error(`graph does not show A -> C control wire: ${JSON.stringify(graph.wires)}`);
    const restored = await ctx.undo();
    if (restored !== before) throw new Error(`undo did not restore exact source\nbefore:\n${before}\nafter:\n${restored}`);
  },

  "exec-rewire-refuses-cross-block": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn order() {
    a :: 1
    if true {
        c :: 3
    } else {
        print(a)
    }
    b :: 2
    print(a + b)
}

fn run() {
    order()
}
`);
    await ctx.openCanvas();
    const before = await ctx.source();
    await dragExecEndpoint(ctx, "order", "if", "c");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      const tx = await ctx.driver.evaluate(`JSON.stringify(window.__jetCanvasLastTxResult || {})`);
      return String(state.lastToast || "").includes("different branch") || tx.includes("different branch");
    }, "cross-block refusal");
    const after = await ctx.source();
    if (after !== before) throw new Error(`cross-block rewire changed source:\n${after}`);
  },

  "exec-rewire-binding-order-diagnostic": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn order() {
    a :: 1
    b :: 2
    c :: a + b
    print(c)
}

fn run() {
    order()
}
`);
    await ctx.openCanvas();
    const before = await ctx.source();
    await dragExecEndpoint(ctx, "order", "b", "c");
    const problem = await ctx.expectProblem("E0107");
    if (!String(problem.rendered || "").includes("`b`")) throw new Error(`binding-order diagnostic did not name b: ${JSON.stringify(problem)}`);
    const after = await ctx.source();
    if (after !== before) throw new Error(`binding-order failed transaction changed source:\n${after}`);
  },

  "exec-convergence-preview": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn converge(flag: Bool) {
    value :: 1
    done :: value
    if flag {
        print(value)
        finish(done)
    } else {
        print(value)
    }
}

fn finish(value: Int) {
    print(value)
}

fn run() {
    converge(true)
}
`);
    await ctx.openCanvas();
    const before = await ctx.source();
    await dragExecPin(ctx, "converge", "if", "else", "finish");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.execConvergencePreview && state.execConvergencePreview.strategy === "extract";
    }, "second execution drop preview");
    const preview = (await ctx.state()).execConvergencePreview;
    if (!preview.incoming_wire_id || !preview.from_pin_id || !preview.to_pin_id) {
      throw new Error(`convergence preview lacks source-backed pin identity: ${JSON.stringify(preview)}`);
    }
    const details = await ctx.driver.evaluate(`document.getElementById("details").textContent`);
    if (!details.includes("Extract shared body") || !details.includes("Duplicate body") || !details.includes("No source written")) {
      throw new Error(`convergence preview did not expose safe choices: ${details}`);
    }
    if (await ctx.source() !== before) throw new Error("second execution drop wrote source before a strategy was applied");
    const rejected = await ctx.transaction({
      schema_version: 1,
      op: "replace_source",
      revision: (await ctx.graph()).revision,
      source: "fn converge("
    });
    if (rejected.ok) throw new Error(`invalid convergence source unexpectedly accepted: ${JSON.stringify(rejected.json)}`);
    if (await ctx.source() !== before) throw new Error("rejected convergence edit changed source");
    const recoverable = await ctx.state();
    if (!recoverable.execConvergencePreview || recoverable.execConvergencePreview.strategy !== "extract") {
      throw new Error(`rejected convergence edit discarded recoverable preview: ${JSON.stringify(recoverable)}`);
    }
    await ctx.openCanvas();
    if (await ctx.source() !== before) throw new Error("reloading convergence preview changed source");
  },

  "exec-convergence-structured-join": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn converge(flag: Bool) {
    value :: 1
    if flag {
        print(value)
    } else {
        print(value)
    }
    finish(value)
}

fn finish(value: Int) {
    print(value)
}

fn run() {
    converge(true)
}
`);
    await ctx.openCanvas();
    const graph = graphByTitle(await ctx.graph(), "converge");
    const incoming = controlIncomingWires(graph, "finish");
    if (incoming.length !== 2 || incoming.some((wire) => !wire.from_source_span || !wire.to_source_span)) {
      throw new Error(`structured join did not project two source-backed incoming wires: ${JSON.stringify(incoming)}`);
    }
    const before = await ctx.source();
    await dragExecPin(ctx, "converge", "if", "else", "finish");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      const toast = await ctx.driver.evaluate(`document.getElementById("toast").textContent`);
      return !state.execConvergencePreview && String(toast || "").includes("structured join");
    }, "structured join stays downstream");
    if (await ctx.source() !== before) throw new Error("structured join gesture changed source");
  },

  "exec-convergence-apply": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn converge(flag: Bool) {
    value :: 1
    done :: value
    if flag {
        print(value)
        finish(done)
    } else {
        print(value)
    }
}

fn finish(value: Int) {
    print(value)
}

fn run() {
    converge(true)
}
`);
    await ctx.openCanvas();
    const before = await ctx.source();
    await dragExecPin(ctx, "converge", "if", "else", "finish");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.execConvergencePreview && state.execConvergencePreview.strategy === "extract";
    }, "convergence apply preview");
    const focused = await ctx.driver.evaluate(`(() => {
      const input = document.getElementById("exec-convergence-function");
      return !!input && document.activeElement === input && !!input.value;
    })()`);
    if (!focused) throw new Error("convergence preview did not focus the editable helper name");
    const applied = await ctx.driver.evaluate(`(() => {
      const button = document.getElementById("apply-exec-convergence");
      if (!button) return false;
      button.click();
      return true;
    })()`);
    if (!applied) throw new Error("convergence apply button missing");
    await ctx.waitFor(async () => {
      const source = await ctx.source();
      return source.includes("fn shared_finish(done: Int)")
        && source.match(/shared_finish\(done\)/g)?.length === 2;
    }, "convergence source apply");
    const after = await ctx.source();
    if (after === before) throw new Error("convergence apply did not change source");
    const graph = await ctx.graph();
    if (!graph.source_text.includes("shared_finish(done)")) throw new Error("fresh graph omitted applied convergence source");
    await ctx.openCanvas();
    if (await ctx.source() !== after) throw new Error("reloading applied convergence changed source");
    const restored = await ctx.undo();
    if (restored !== before) throw new Error(`convergence undo did not restore exact source\nbefore:\n${before}\nrestored:\n${restored}`);
  },

  "exec-convergence-selected-span": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn converge(flag: Bool) {
    value :: 1
    if flag {
        notify(value)
        bump_metric()
        print(value)
    } else {
        print(value)
    }
}

fn notify(value: Int) {
    print(value)
}

fn bump_metric() {
    print("metric")
}

fn run() {
    converge(true)
}
`);
    await ctx.openCanvas();
    await selectNodeTitles(ctx, ["notify", "bump_metric"], "selected convergence span");
    const before = await ctx.source();
    await dragExecPin(ctx, "converge", "if", "else", "notify");
    await ctx.waitFor(async () => {
      const preview = (await ctx.state()).execConvergencePreview;
      return preview && preview.target_node_ids?.length === 2
        && preview.target_source_span.end > preview.target_source_span.start;
    }, "selected convergence preview");
    const preview = await ctx.state();
    const selected = preview.execConvergencePreview;
    if (selected.target_source_span.end - selected.target_source_span.start < 20) {
      throw new Error(`selected convergence preview did not retain both source statements: ${JSON.stringify(selected)}`);
    }
    await ctx.driver.evaluate(`document.getElementById("apply-exec-convergence").click()`);
    await ctx.waitFor(async () => {
      const source = await ctx.source();
      return source.includes("fn shared_notify(value: Int)")
        && source.match(/shared_notify\(value\)/g)?.length === 2;
    }, "selected convergence source apply");
    const after = await ctx.source();
    if (after === before || after.includes("notify(value)\n        bump_metric()")) {
      throw new Error(`selected convergence did not replace the complete span:\n${after}`);
    }
    await ctx.openCanvas();
    if (await ctx.source() !== after) throw new Error("selected convergence reload changed source");
    const restored = await ctx.undo();
    if (restored !== before) throw new Error("selected convergence undo did not restore exact source");
  },

  "exec-convergence-refuses-out-of-scope": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn converge(flag: Bool) {
    value :: 1
    if flag {
        branch_value :: value
        finish(branch_value)
    } else {
        print(value)
    }
}

fn finish(value: Int) {
    print(value)
}

fn run() {
    converge(true)
}
`);
    await ctx.openCanvas();
    await ctx.driver.evaluate(`(() => {
      window.__jetCanvasConvergencePosts = 0;
      const originalFetch = window.fetch.bind(window);
      window.fetch = (input, init = {}) => {
        if (String(input).includes("/canvas/transaction")) window.__jetCanvasConvergencePosts += 1;
        return originalFetch(input, init);
      };
    })()`);
    const before = await ctx.source();
    await dragExecPin(ctx, "converge", "if", "else", "finish");
    await ctx.waitFor(async () => !!(await ctx.state()).execConvergencePreview, "convergence refusal preview");
    const applied = await ctx.driver.evaluate(`(() => {
      const button = document.getElementById("apply-exec-convergence");
      if (!button) return false;
      button.click();
      return true;
    })()`);
    if (!applied) throw new Error("convergence refusal apply button missing");
    await expectVisibleRefusal(ctx, "only available on another execution path", "out-of-scope convergence refusal");
    const posts = await ctx.driver.evaluate("window.__jetCanvasConvergencePosts");
    if (posts !== 0) throw new Error(`out-of-scope convergence refusal reached sema/server: ${posts} transaction posts`);
    if (await ctx.source() !== before) throw new Error("out-of-scope convergence refusal changed source");
    const recoverable = await ctx.state();
    if (!recoverable.execConvergencePreview) throw new Error("failed convergence discarded recoverable preview");
    await assertSourceUnchangedAfterReload(ctx, before, "out-of-scope convergence refusal");
  },

  "exec-convergence-explicit-strategies": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn shared_finish(done: Int) {
    finish(done)
}

fn converge(flag: Bool) {
    value :: 1
    done :: value
    if flag {
        print(value)
        finish(done)
    } else {
        print(value)
    }
}

fn finish(value: Int) {
    print(value)
}

fn run() {
    converge(true)
}
`);
    await ctx.openCanvas();
    await dragExecPin(ctx, "converge", "if", "else", "finish");
    await ctx.waitFor(async () => !!(await ctx.state()).execConvergencePreview, "exact helper convergence preview");
    const helperVisible = await ctx.driver.evaluate(`!!document.querySelector('[data-exec-convergence-strategy="helper"]')`);
    if (!helperVisible) throw new Error("exact-body helper choice was not rendered");
    await ctx.driver.evaluate(`document.querySelector('[data-exec-convergence-strategy="helper"]').click()`);
    await ctx.driver.evaluate(`document.getElementById("apply-exec-convergence").click()`);
    await ctx.waitFor(async () => (await ctx.source()).match(/shared_finish\(done\)/g)?.length === 2, "exact helper convergence apply");
    if ((await ctx.source()).match(/fn shared_finish\(/g)?.length !== 1) throw new Error("helper strategy created a duplicate helper");

    await ctx.replaceSource(`fn converge(flag: Bool) {
    value :: 1
    done :: value
    if flag {
        print(value)
        finish(done)
    } else {
        print(value)
    }
}

fn finish(value: Int) {
    print(value)
}

fn run() {
    converge(true)
}
`);
    await ctx.openCanvas();
    await dragExecPin(ctx, "converge", "if", "else", "finish");
    await ctx.waitFor(async () => !!(await ctx.state()).execConvergencePreview, "duplicate convergence preview");
    await ctx.driver.evaluate(`document.querySelector('[data-exec-convergence-strategy="duplicate"]').click()`);
    await ctx.driver.evaluate(`document.getElementById("apply-exec-convergence").click()`);
    await ctx.waitFor(async () => (await ctx.source()).match(/finish\(done\)/g)?.length === 2, "duplicate convergence apply");
    const duplicated = await ctx.source();
    if (duplicated.includes("fn shared_finish")) throw new Error("duplicate strategy created a helper");
  },

  "exec-convergence-escape": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn converge(flag: Bool) {
    value :: 1
    done :: value
    if flag {
        print(value)
        finish(done)
    } else {
        print(value)
    }
}

fn finish(value: Int) {
    print(value)
}

fn run() {
    converge(true)
}
`);
    await ctx.openCanvas();
    const before = await ctx.source();
    await dragExecPin(ctx, "converge", "if", "else", "finish");
    await ctx.waitFor(async () => !!(await ctx.state()).execConvergencePreview, "escape convergence preview");
    await ctx.driver.send("Input.dispatchKeyEvent", {
      type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27
    }, ctx.driver.pageSession);
    await ctx.driver.send("Input.dispatchKeyEvent", {
      type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27
    }, ctx.driver.pageSession);
    await ctx.waitFor(async () => !(await ctx.state()).execConvergencePreview, "escape closes convergence preview");
    if (await ctx.source() !== before) throw new Error("Escape changed convergence source");
  },

  "exec-convergence-stale": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn converge(flag: Bool) {
    value :: 1
    done :: value
    if flag {
        print(value)
        finish(done)
    } else {
        print(value)
    }
}

fn finish(value: Int) {
    print(value)
}

fn run() {
    converge(true)
}
`);
    await ctx.openCanvas();
    await dragExecPin(ctx, "converge", "if", "else", "finish");
    await ctx.waitFor(async () => !!(await ctx.state()).execConvergencePreview, "stale convergence preview");
    const changed = (await ctx.source()).replace("value :: 1", "value :: 2");
    const graph = await ctx.graph();
    const external = await ctx.transaction({ schema_version: 1, op: "replace_source", revision: graph.revision, source: changed });
    if (!external.ok) throw new Error(`stale setup failed: ${JSON.stringify(external.json)}`);
    await ctx.driver.evaluate(`document.getElementById("apply-exec-convergence").click()`);
    await expectVisibleRefusal(ctx, "source changed since this Canvas graph was drawn", "stale convergence refusal");
    if (await ctx.source() !== changed) throw new Error("stale convergence changed newer source");
    if (!(await ctx.state()).execConvergencePreview) throw new Error("stale convergence discarded the recoverable preview");
  },

  "branch-insertion-targets": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn run() {
    print("before")
    print("target")
    print("after")
}
`);
    await ctx.openCanvas();
    await ctx.switchGraph("run");
    const before = await ctx.source();
    const graph = graphByTitle(await ctx.graph(), "run");
    const targetInline = (graph.inline_exprs || []).find((expr) => expr.source === '"target"');
    if (!targetInline) throw new Error("branch target inline source missing");
    const targetNode = (graph.nodes || []).find((node) => node.node_id === targetInline.node_id);
    const targetExec = (graph.pins || []).find((pin) => pin.node_id === targetNode?.node_id && pin.direction === "input" && pin.name === "exec");
    const targetData = (graph.pins || []).find((pin) => pin.node_id === targetNode?.node_id && pin.direction === "input" && pin.type !== "exec");
    if (!targetNode || !targetExec || !targetData) throw new Error("branch target pins missing");

    await ctx.driver.evaluate(`window.__jetCanvasTest.openGraphActionPalette("Branch")`);
    await ctx.expectMenu("Branch");
    await ctx.pickEntry("Branch");
    await ctx.waitFor(async () => (await ctx.state()).stagedRegistry.some((node) => node.title === "Branch"), "staged branch");
    if (await ctx.source() !== before) throw new Error("staging branch changed source");

    const staged = (await ctx.state()).stagedRegistry.find((node) => node.title === "Branch");
    const stagedThen = staged && staged.pins.find((pin) => pin.direction === "output" && pin.name === "then");
    let state = await ctx.state();
    const stagedPoint = stagedThen && state.pinPoints[stagedThen.pin_id];
    const dataPoint = state.pinPoints[targetData.pin_id];
    const execPoint = state.pinPoints[targetExec.pin_id];
    if (!stagedPoint || !dataPoint || !execPoint) throw new Error("branch gesture pin points missing");

    await ctx.driver.drag(
      { x: stagedPoint.client_x, y: stagedPoint.client_y },
      { x: dataPoint.client_x, y: dataPoint.client_y },
      16
    );
    await sleep(300);
    const refused = await ctx.driver.evaluate(`({ plan: window.__jetCanvasLastConnectionPlan || null, toast: window.__jetCanvasTest?.lastToast || "" })`);
    const refusalReason = String(refused.plan && refused.plan.label || "");
    if (!refused.plan || refused.plan.ok || !refusalReason || !String(refused.toast || "").includes(refusalReason)) {
      throw new Error(`branch data-pin refusal was not client-side: ${JSON.stringify(refused)}`);
    }
    if (await ctx.source() !== before) throw new Error("branch data-pin refusal changed source");
    if (!(await ctx.state()).stagedRegistry.some((node) => node.title === "Branch")) throw new Error("refused branch gesture was not recoverable");

    state = await ctx.state();
    const stagedPointAfterRefusal = state.pinPoints[stagedThen.pin_id];
    const execPointAfterRefusal = state.pinPoints[targetExec.pin_id];
    await ctx.driver.drag(
      { x: stagedPointAfterRefusal.client_x, y: stagedPointAfterRefusal.client_y },
      { x: execPointAfterRefusal.client_x, y: execPointAfterRefusal.client_y },
      16
    );
    await ctx.waitFor(async () => {
      const source = await ctx.source();
      return source.includes("if true") && source.indexOf("if true") < source.indexOf('print("target")')
        && !(await ctx.state()).stagedRegistry.some((node) => node.title === "Branch");
    }, "targeted branch source transaction");
    const inserted = await ctx.source();
    const branchTx = await ctx.driver.evaluate(`window.__jetCanvasLastTx || null`);
    if (!branchTx || branchTx.op !== "insert_branch" || branchTx.graph_id !== graph.graph_id
      || branchTx.wire_target_pin !== "exec" || branchTx.wire_origin_pin_id !== targetExec.pin_id) {
      throw new Error(`branch gesture bypassed targeted source transaction: ${JSON.stringify({ branchTx, target: targetExec.pin_id, graph: graph.graph_id })}`);
    }
    if (inserted.indexOf("if true") <= inserted.indexOf('print("before")')) throw new Error(`branch ignored target insertion point:\n${inserted}`);
    await assertCleanSourceSync(ctx, ["branch target insertion"]);

    const projected = graphByTitle(await ctx.graph(), "run");
    const branchNode = (projected.nodes || []).find((node) => node.kind === "branch");
    if (!branchNode || !branchNode.source_span || !projected.pins.some((pin) => pin.node_id === branchNode.node_id && pin.name === "then")) {
      throw new Error(`branch projection lost source target/provenance: ${JSON.stringify(branchNode)}`);
    }
    const undone = await ctx.undo();
    if (undone !== before) throw new Error("targeted branch undo did not restore exact source");
    const redone = await ctx.redo();
    if (redone !== inserted) throw new Error("targeted branch redo did not restore exact source");
    await ctx.openCanvas();
    const reloaded = graphByTitle(await ctx.graph(), "run");
    if (!(reloaded.nodes || []).some((node) => node.kind === "branch" && node.source_span)) throw new Error("targeted branch reload lost projection");
    if (!(await ctx.source()).includes("if true")) throw new Error("targeted branch reload lost source");
  },

  "pattern-arm-add-edit-remove": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`enum Choice {
    A(Int)
    B(Int)
    C(Int)
}

fn choose(x: Choice) Int -> {
    if x == {
        .A(n) -> { return n }
        else -> { return 0 }
    }
}

fn run() {
    print(choose(Choice.A(1)))
}
`);
    await ctx.openCanvas();
    // `enum Choice` projects derived `encode`/`decode` graphs (13 and 27 nodes)
    // that outweigh `choose` (7), so the editor's richest-graph default selects
    // `decode` — whose own `if ==` dispatch node satisfies ctx.node("if ==").
    // Without pinning the graph, every arm transaction carried
    // graph_id=decode + the enum's span and was refused with a parse error
    // instead of touching `choose`. Select the graph under test explicitly.
    await ctx.switchGraph("choose");
    await expectConsumedDescriptor(ctx, "dispatch", { transaction: "insert_switch", glyph: "◇", defaultEditor: "pattern_arm" });
    const chooseGraph = graphByTitle(await ctx.graph(), "choose");
    const patternNode = (chooseGraph.nodes || []).find((node) => node.title === "if ==");
    if (!patternNode || !patternNode.source_span) throw new Error("pattern insertion target span missing");
    let before = await ctx.source();
    await ctx.driver.evaluate(`window.prompt = () => "== .B(n)"`);
    let pos = await ctx.node("if ==");
    await ctx.driver.rightClick(pos.x, pos.y);
    await ctx.expectMenu("Add pattern arm");
    await ctx.pickEntry("Add pattern arm");
    await ctx.waitFor(async () => (await ctx.source()).includes(".B(n) ->"), "pattern arm add");
    const addPatternTx = await ctx.driver.evaluate(`window.__jetCanvasLastTx || null`);
    if (!addPatternTx || addPatternTx.op !== "add_pattern_arm" || addPatternTx.graph_id !== chooseGraph.graph_id
      || addPatternTx.node_start !== patternNode.source_span.start || addPatternTx.node_end !== patternNode.source_span.end) {
      throw new Error(`pattern gesture bypassed targeted source transaction: ${JSON.stringify({ addPatternTx, target: patternNode.source_span, graph: chooseGraph.graph_id })}`);
    }
    await assertSourceSync(ctx, ["pattern add"]);
    await ctx.expectSourceContains(".B(n) ->");

    await ctx.driver.evaluate(`window.prompt = () => "== .C(n)"`);
    const pin = await ctx.pin("if ==", "arm2");
    await ctx.driver.rightClick(pin.x, pin.y);
    await ctx.expectMenu("Edit pattern");
    await ctx.pickEntry("Edit pattern");
    await ctx.waitFor(async () => (await ctx.source()).includes(".C(n) ->") && !(await ctx.source()).includes(".B(n) ->"), "pattern arm edit");
    await assertSourceSync(ctx, ["pattern edit"]);

    const edited = await ctx.source();
    const removePin = await ctx.pin("if ==", "arm2");
    await ctx.driver.rightClick(removePin.x, removePin.y);
    await ctx.expectMenu("Remove arm");
    await ctx.pickEntry("Remove arm");
    await ctx.waitFor(async () => !(await ctx.source()).includes(".C(n) ->"), "pattern arm remove");
    await assertSourceSync(ctx, ["pattern remove"]);

    const restored = await ctx.undo();
    if (restored !== edited) throw new Error(`undo did not restore edited pattern arm\nexpected:\n${edited}\nactual:\n${restored}`);
    if (before === restored) throw new Error("pattern add/edit/remove cycle did not change source before undo checkpoint");
    await ctx.openCanvas();
    const reloaded = graphByTitle(await ctx.graph(), "choose");
    const arm2 = (reloaded.pins || []).find((pin) => pin.name === "arm2");
    if (!arm2 || !arm2.pattern_source_span || !arm2.source_span || arm2.pattern_source_span.start >= arm2.pattern_source_span.end) {
      throw new Error(`reloaded pattern arm lost source provenance: ${JSON.stringify({ arm2 })}`);
    }
  },

  "pattern-arm-invalid-refused": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`enum Choice {
    A(Int)
    B(Int)
}

fn choose(x: Choice) Int -> {
    if x == {
        .A(n) -> { return n }
        else -> { return 0 }
    }
}

fn run() {
    print(choose(Choice.A(1)))
}
`);
    await ctx.openCanvas();
    // Same derived-graph trap as pattern-arm-add-edit-remove: pin `choose` so
    // the UI refusal uses checked Choice facts, not a stray parse error from
    // splicing an arm into the enum declaration.
    await ctx.switchGraph("choose");
    const before = await ctx.source();
    await ctx.driver.evaluate(`window.prompt = () => "== .Missing(n)"`);
    const pos = await ctx.node("if ==");
    await ctx.driver.rightClick(pos.x, pos.y);
    await ctx.expectMenu("Add pattern arm");
    await ctx.pickEntry("Add pattern arm");
    await expectVisibleRefusal(ctx, "not a Choice variant", "invalid pattern refusal");
    const refusal = await ctx.driver.evaluate(`window.__jetCanvasLastTxResult || null`);
    if (!refusal || refusal.code !== "client_pattern_gate") {
      throw new Error(`invalid pattern was not refused by the UI gate: ${JSON.stringify(refusal)}`);
    }
    if (await ctx.driver.evaluate(`window.__jetCanvasLastTx || null`) !== null) {
      throw new Error("invalid pattern posted a source transaction");
    }
    if ((await ctx.problems()).some((problem) => problem.code === "E0305")) {
      throw new Error("invalid pattern reached sema diagnostics");
    }
    const after = await ctx.source();
    if (after !== before) throw new Error(`bad pattern changed source:\n${after}`);
  },

  "multi-input-append-remove": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn to_int(n: Int) Int -> {
    return n
}

fn demo() Int -> {
    xs :: [1, 2, 3]
    ys :: [1, 2]
    return xs[0] + ys[0]
}

fn run() {
    print(demo())
}
`);
    await ctx.openCanvas();
    await ctx.driver.evaluate(`window.prompt = () => "4"`);
    let list = await ctx.node("list");
    await ctx.driver.rightClick(list.x, list.y);
    await ctx.expectMenu("Append input");
    await ctx.pickEntry("Append input");
    await ctx.waitFor(async () => (await ctx.source()).includes("[1, 2, 3, 4]"), "list append");
    await assertSourceSync(ctx, ["list append"]);
    const appended = await ctx.source();
    const projected = graphByTitle(await ctx.graph(), "demo");
    const listNode = (projected.nodes || []).find((candidate) => candidate.title === "list");
    const item4 = listNode && (projected.pins || []).find((pin) => pin.node_id === listNode.node_id && pin.name === "item4");
    if (!item4 || item4.type !== "Int" || !item4.source_span || item4.source_span.start >= item4.source_span.end) {
      throw new Error(`list append lost typed source provenance: ${JSON.stringify({ listNode, item4 })}`);
    }

    let item = await ctx.pin("list", "item4");
    await ctx.driver.rightClick(item.x, item.y);
    await ctx.expectMenu("Remove element");
    await ctx.pickEntry("Remove element");
    await ctx.waitFor(async () => (await ctx.source()).includes("[1, 2, 3]") && !(await ctx.source()).includes("[1, 2, 3, 4]"), "list remove");
    await assertCleanSourceSync(ctx, ["list append", "list remove"]);

    const restored = await ctx.undo();
    if (restored !== appended) throw new Error(`undo did not restore appended list source\nexpected:\n${appended}\nactual:\n${restored}`);
    await assertCleanSourceSync(ctx, ["list append", "list remove", "list undo"]);
    await ctx.openCanvas();
    if (await ctx.source() !== appended) throw new Error("reloading list transaction changed Jet source bytes");

    const beforeInvalid = await ctx.source();
    await ctx.driver.evaluate(`window.prompt = () => "true"`);
    list = await ctx.node("list");
    await ctx.driver.rightClick(list.x, list.y);
    await ctx.expectMenu("Append input");
    await ctx.pickEntry("Append input");
    await ctx.expectProblem("");
    if (await ctx.source() !== beforeInvalid) throw new Error("ill-typed list append changed Jet source");
    await assertSourceUnchangedAfterReload(ctx, beforeInvalid, "ill-typed list append");
  },

  "node-state-off-toggle": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn run() {
    #Off print("off")
    print("on")
}
`);
    await ctx.openCanvas();
    const before = await ctx.source();

    const offNode = await ctx.node("#Off");
    await ctx.driver.rightClick(offNode.x, offNode.y);
    await ctx.expectMenu("Turn on");
    await ctx.pickEntry("Turn on");
    await ctx.waitFor(async () => !(await ctx.source()).includes("#Off"), "state turn on");
    await assertSourceSync(ctx, ["state turn on"]);
    const on = await ctx.source();

    const printNode = await ctx.node("print");
    await ctx.driver.rightClick(printNode.x, printNode.y);
    await ctx.expectMenu("Turn off");
    await ctx.pickEntry("Turn off");
    await ctx.waitFor(async () => (await ctx.source()).includes("#Off print"), "state turn off");
    await assertSourceSync(ctx, ["state turn off"]);

    const doc = await ctx.graph();
    const badged = (doc.graphs || []).flatMap((g) => g.nodes || []).filter((n) => (n.badges || []).includes("#Off"));
    if (!badged.length) throw new Error("no #Off badge node after turn off");

    const restored = await ctx.undo();
    if (restored !== on) throw new Error(`undo did not restore pre-#Off source\nexpected:\n${on}\nactual:\n${restored}`);
    if (restored === before) throw new Error("toggle cycle did not change source before undo checkpoint");
  },

  "node-state-debug-only-toggle": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn run() {
    debug_value :: 3
    #DebugOnly print("debug")
    print("on")
}
`);
    await ctx.openCanvas();
    const before = await ctx.source();
    const staleDoc = await ctx.graph();
    const staleGraph = graphByTitle(staleDoc, "run");
    const staleNode = nodeByTitle(staleGraph, "#DebugOnly");
    if (!staleNode.source_span || staleNode.source_span.start >= staleNode.source_span.end) {
      throw new Error(`DebugOnly node lost source provenance: ${JSON.stringify(staleNode)}`);
    }
    if (!staleNode.badges || !staleNode.badges.includes("#DebugOnly")) {
      throw new Error(`DebugOnly node lost state badge: ${JSON.stringify(staleNode)}`);
    }

    const changed = before.replace('print("on")', 'print("changed")');
    await ctx.replaceSource(changed);
    const stale = await ctx.uiTransaction({
      schema_version: 1,
      op: "toggle_switch_state",
      revision: staleDoc.revision,
      graph_id: staleGraph.graph_id,
      node_start: staleNode.source_span.start,
      node_end: staleNode.source_span.end
    });
    if (stale.ok || await ctx.source() !== changed) {
      throw new Error(`stale DebugOnly toggle changed source: ${JSON.stringify(stale)}`);
    }
    await expectVisibleRefusal(ctx, "source changed", "stale DebugOnly toggle refusal");
    await assertSourceUnchangedAfterReload(ctx, changed, "stale DebugOnly toggle");

    await ctx.replaceSource(before);
    await ctx.openCanvas();
    const debugNode = await ctx.node("#DebugOnly");
    await ctx.driver.rightClick(debugNode.x, debugNode.y);
    await ctx.expectMenu("Turn on");
    await ctx.pickEntry("Turn on");
    await ctx.waitFor(async () => !(await ctx.source()).includes("#DebugOnly"), "DebugOnly state turn on");
    await assertCleanSourceSync(ctx, ["DebugOnly badge", "DebugOnly pointer toggle"]);
    const on = await ctx.source();
    if (!on.includes('print("debug")') || on.includes("#DebugOnly")) {
      throw new Error(`DebugOnly toggle changed canonical source incorrectly:\n${on}`);
    }

    const projected = graphByTitle(await ctx.graph(), "run");
    const debugPrint = nodeByTitle(projected, "print");
    const debugInput = (projected.pins || []).find((pin) => pin.node_id === debugPrint.node_id && pin.direction === "input");
    if (!debugPrint.source_span || !debugInput || debugInput.type !== "String") {
      throw new Error(`DebugOnly body lost typed source provenance: ${JSON.stringify({ debugPrint, debugInput })}`);
    }

    await ctx.openCanvas();
    if (await ctx.source() !== on) throw new Error("DebugOnly toggle reload changed source");
    const restored = await ctx.undo();
    if (restored !== before) {
      throw new Error(`undo did not restore pre-DebugOnly source\nexpected:\n${before}\nactual:\n${restored}`);
    }
    await assertCleanSourceSync(ctx, ["DebugOnly undo"]);
    const reloaded = graphByTitle(await ctx.graph(), "run");
    const restoredNode = nodeByTitle(reloaded, "#DebugOnly");
    if (!restoredNode.badges || !restoredNode.badges.includes("#DebugOnly")) {
      throw new Error("undo lost the DebugOnly badge after reprojection");
    }
    await ctx.openCanvas();
    if (await ctx.source() !== before) throw new Error("DebugOnly undo reload changed source");
  },

  "inline-edit-values": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    const { doc, graph, expr } = await scratchLimitInline(ctx);
    await uiEdit(ctx, { schema_version: 1, op: "edit_inline_expr", revision: doc.revision, inline_expr_id: expr.inline_expr_id, new_expr: "limit + 2" }, "inline value edit");
    await ctx.expectSourceContains("print(limit + 2)");
  },

  "promote-pin-keyboard-gesture": async (ctx) => {
    await ctx.openCanvas();
    const base = await ctx.source();
    let selected = await selectInlineExpression(ctx, "scratch", (expr) => expr.source === "limit", "promote limit");
    await ctx.driver.evaluate(`window.prompt = () => "promoted_limit"`);
    await pressAttribute(ctx, "data-inline-promote", selected.expr.inline_expr_id, "Promote to binding");
    await sleep(500);
    if (!(await ctx.source()).includes("promoted_limit :: limit")) {
      const state = await ctx.driver.evaluate(`JSON.stringify({
        tx: window.__jetCanvasLastTx,
        result: window.__jetCanvasLastTxResult,
        toast: window.__jetCanvasTest && window.__jetCanvasTest.lastToast
      })`);
      throw new Error(`promotion gesture did not write source: ${state}`);
    }
    await ctx.waitFor(async () => (await ctx.source()).includes("promoted_limit :: limit"), "promoted binding source");
    await ctx.expectSourceContains("print(promoted_limit)");
    await assertCleanSourceSync(ctx, ["keyboard promote"]);

    await ctx.replaceSource(base);
    await ctx.openCanvas();
    selected = await selectInlineExpression(ctx, "scratch", (expr) => expr.source === "limit", "invalid promote limit");
    const beforeInvalid = await ctx.source();
    await ctx.driver.evaluate(`window.prompt = () => "bad name"`);
    await pressAttribute(ctx, "data-inline-promote", selected.expr.inline_expr_id, "invalid Promote to binding");
    await expectVisibleRefusal(ctx, "identifier", "invalid promotion refusal");
    if (await ctx.source() !== beforeInvalid) throw new Error("invalid promotion partially changed source");

    await ctx.driver.evaluate(`(() => {
      const originalFetch = window.fetch.bind(window);
      let staleOnce = true;
      window.fetch = (input, init = {}) => {
        if (staleOnce && String(input).includes("/canvas/transaction") && init.body) {
          const body = JSON.parse(init.body);
          if (body.op === "promote_to_binding") {
            staleOnce = false;
            body.revision = "sha256-stale-revision";
            init = Object.assign({}, init, { body: JSON.stringify(body) });
          }
        }
        return originalFetch(input, init);
      };
    })()`);
    await ctx.driver.evaluate(`window.prompt = () => "stale_value"`);
    await pressAttribute(ctx, "data-inline-promote", selected.expr.inline_expr_id, "stale Promote to binding");
    await expectVisibleRefusal(ctx, "source changed", "stale promotion refusal");
    if (await ctx.source() !== beforeInvalid) throw new Error("stale promotion partially changed source");
  },

  "conversion-keyboard-gesture": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn to_int(n: Int) Int -> {
    return n
}

fn convert(limit: Int) {
    print(limit)
}

fn run() {
    convert(3)
}
`);
    await ctx.openCanvas();
    let selected = await selectInlineExpression(ctx, "convert", (expr) => expr.source === "limit", "convert limit");
    await ctx.driver.evaluate(`window.prompt = () => "to_int"`);
    await pressAttribute(ctx, "data-inline-convert", selected.expr.inline_expr_id, "Insert conversion");
    await ctx.waitFor(async () => (await ctx.source()).includes("print(to_int(limit))"), "visible conversion source");
    await assertCleanSourceSync(ctx, ["keyboard conversion"]);

    const valid = await ctx.source();
    selected = await selectInlineExpression(ctx, "convert", (expr) => expr.source === "limit", "invalid conversion");
    await ctx.driver.evaluate(`window.prompt = () => "missing_conversion"`);
    await pressAttribute(ctx, "data-inline-convert", selected.expr.inline_expr_id, "invalid Insert conversion");
    await expectVisibleRefusal(ctx, "missing_conversion", "invalid conversion refusal");
    if (await ctx.source() !== valid) throw new Error("invalid conversion partially changed source");
  },

  "math-expression-keyboard-edit": async (ctx) => {
    await ctx.openCanvas();
    let selected = await selectInlineExpression(ctx, "scratch", (expr) => expr.source === "limit", "math expression");
    const inputExpression = `Array.from(document.querySelectorAll("[data-inline-id]")).find((element) => element.getAttribute("data-inline-id") === ${JSON.stringify(selected.expr.inline_expr_id)})`;
    await ctx.driver.evaluate(`(() => {
      const input = ${inputExpression};
      input.focus();
      input.setSelectionRange(0, input.value.length);
    })()`);
    await ctx.type("limit * limit + 1");
    await pressAttribute(ctx, "data-inline-apply", selected.expr.inline_expr_id, "Apply expression");
    await ctx.waitFor(async () => (await ctx.source()).includes("print(limit * limit + 1)"), "math expression source");
    await assertCleanSourceSync(ctx, ["keyboard math edit"]);

    const valid = await ctx.source();
    selected = await selectInlineExpression(ctx, "scratch", (expr) => String(expr.source || "").includes("limit * limit"), "invalid math expression");
    const invalidInput = `Array.from(document.querySelectorAll("[data-inline-id]")).find((element) => element.getAttribute("data-inline-id") === ${JSON.stringify(selected.expr.inline_expr_id)})`;
    await ctx.driver.evaluate(`(() => {
      const input = ${invalidInput};
      input.focus();
      input.setSelectionRange(0, input.value.length);
    })()`);
    await ctx.type("missing_value");
    await pressAttribute(ctx, "data-inline-apply", selected.expr.inline_expr_id, "Apply invalid expression");
    await ctx.expectProblem("E0107");
    if (await ctx.source() !== valid) throw new Error("invalid math edit partially changed source");
  },

  "collapse-expand-keyboard-gesture": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn compute(x: Int) {
    a :: x + 1
    b :: a * 2
    print(b)
}

fn run() {
    compute(3)
}
`);
    await ctx.openCanvas();
    await ctx.switchGraph("compute");
    const before = await ctx.source();
    const initialComputeDoc = await ctx.graph();
    const initialCompute = graphByTitle(initialComputeDoc, "compute");
    const initialA = nodeByTitle(initialCompute, "a");
    const initialB = nodeByTitle(initialCompute, "b");
    await selectNodeTitles(ctx, ["a", "b"], "collapse selection setup");
    await ctx.driver.evaluate(`window.prompt = () => "Compute value"`);
    await ctx.driver.shortcut(["Alt", "c"]);
    await ctx.waitFor(async () => (await ctx.source()).includes("canvas:collapse"), "collapse source hint");
    await assertCleanSourceSync(ctx, ["keyboard collapse"]);
    const collapsedState = await ctx.state();
    const collapsedNodes = Object.values(collapsedState.nodeBounds || {});
    const collapsedTitles = collapsedNodes.map((node) => node.title);
    const retainedBindings = collapsedNodes.filter((node) =>
      node.kind === "binding" && ["a", "b"].includes(node.title));
    if (!collapsedTitles.includes("Compute value") || retainedBindings.length) {
      const projected = graphByTitle(await ctx.graph(), "compute");
      throw new Error(`collapsed node did not replace both statements: ${JSON.stringify({
        collapsedTitles,
        retainedBindings,
        regions: projected.regions,
        source: await ctx.source()
      })}`);
    }

    const onceCollapsed = await ctx.source();
    await selectNodeTitles(ctx, ["Compute value"], "collapsed region setup");
    await ctx.driver.shortcut(["Alt", "c"]);
    await expectVisibleRefusal(ctx, "already collapsed", "duplicate collapse refusal");
    const duplicateRefused = await ctx.source();
    if (duplicateRefused !== onceCollapsed
      || (duplicateRefused.match(/canvas:collapse/g) || []).length !== 1) {
      throw new Error(`duplicate collapse gesture changed source: ${JSON.stringify({ onceCollapsed, duplicateRefused })}`);
    }
    const collapsedDoc = await ctx.graph();
    const collapsedCompute = graphByTitle(collapsedDoc, "compute");
    const duplicateTransaction = await ctx.uiTransaction({
      schema_version: 1,
      op: "create_collapsed_region",
      revision: collapsedDoc.revision,
      graph_id: collapsedCompute.graph_id,
      start: initialA.source_span.start,
      end: initialB.source_span.end,
      title: "Duplicate"
    });
    if (duplicateTransaction.ok || await ctx.source() !== onceCollapsed) {
      throw new Error(`duplicate collapse transaction was not idempotently refused: ${JSON.stringify(duplicateTransaction)}`);
    }
    await ctx.driver.shortcut(["Alt", "Shift", "c"]);
    await ctx.waitFor(async () => !(await ctx.source()).includes("canvas:collapse"), "expanded source");
    if (await ctx.source() !== before) throw new Error("collapse/expand did not restore exact source");
    await assertCleanSourceSync(ctx, ["keyboard collapse", "keyboard expand"]);

    const computeDoc = await ctx.graph();
    const compute = graphByTitle(computeDoc, "compute");
    const aNode = nodeByTitle(compute, "a");
    const bNode = nodeByTitle(compute, "b");
    for (const hostile of [
      { start: 1, end: 2, label: "non-member collapse span" },
      { start: aNode.source_span.start + 1, end: bNode.source_span.end, label: "partial-statement collapse span" }
    ]) {
      const result = await ctx.uiTransaction({
        schema_version: 1,
        op: "create_collapsed_region",
        revision: computeDoc.revision,
        graph_id: compute.graph_id,
        start: hostile.start,
        end: hostile.end,
        title: "Hostile"
      });
      if (result.ok) throw new Error(`${hostile.label} unexpectedly wrote source`);
      if (await ctx.source() !== before) throw new Error(`${hostile.label} partially changed source`);
    }

    await ctx.replaceSource(`fn cross(x: Int) {
    if x > 0 {
        print(x)
    }
}

fn run() {
    cross(1)
}
`);
    await ctx.openCanvas();
    await ctx.switchGraph("cross");
    const crossBefore = await ctx.source();
    await selectNodeTitles(ctx, ["if", "print"], "cross-block collapse setup");
    await ctx.driver.evaluate(`window.prompt = () => "Invalid block"`);
    await ctx.driver.shortcut(["Alt", "c"]);
    await expectVisibleRefusal(ctx, "block boundary", "cross-block collapse refusal");
    if (await ctx.source() !== crossBefore) throw new Error("cross-block collapse partially changed source");
  },

  "source-comment-keyboard-reload": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    await ctx.click("print");
    const before = await ctx.source();
    await ctx.driver.evaluate(`window.prompt = () => "Scratch note"`);
    await ctx.driver.press("c");
    await ctx.waitFor(async () => (await ctx.source()).includes("canvas:comment"), "source-backed comment");
    await assertCleanSourceSync(ctx, ["keyboard source comment"]);
    await expectRenderedComment(ctx, "Scratch note", "source comment before reload");
    const changed = await ctx.source();
    if (changed === before) throw new Error("keyboard source comment did not write source");
    await ctx.openCanvas();
    if (await ctx.source() !== changed) throw new Error("source-backed comment reload changed source bytes");
    await ctx.switchGraph("scratch");
    await expectRenderedComment(ctx, "Scratch note", "source comment after reload");
  },

  "workspace-keyboard-view-state": async (ctx) => {
    await ctx.openCanvas();
    await ctx.driver.send("Emulation.setDeviceMetricsOverride", {
      width: 1440,
      height: 900,
      deviceScaleFactor: 1,
      mobile: false,
    }, ctx.driver.pageSession);
    await sleep(120);
    await ctx.replaceSource(`fn child(n: Int) Int -> {
    return n * 2
}

fn layout(x: Int) {
    a :: x + 1
    b :: x + 2
    c :: x + 3
    print(child(a + b + c))
}

fn run() {
    layout(1)
}
`);
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    await clickElement(ctx, `Array.from(document.querySelectorAll("[data-sidebar-graph]")).find((button) => button.textContent.includes("layout"))`, "layout graph sidebar button");
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "layout", "layout graph navigation");
    const before = await ctx.source();
    const reloadLayout = async (label) => {
      await ctx.openCanvas();
      if (await ctx.source() !== before) throw new Error(`${label} reload changed source bytes`);
      await clickElement(ctx, `Array.from(document.querySelectorAll("[data-sidebar-graph]")).find((button) => button.textContent.includes("layout"))`, `${label} layout graph`);
      await ctx.waitFor(async () => (await ctx.state()).graphTitle === "layout", `${label} layout navigation`);
      return await ctx.state();
    };

    const childCall = await ctx.node("child");
    await doubleClickCanvasPoint(ctx, childCall);
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "child", "child graph pointer navigation");
    await clickElement(ctx, `document.getElementById("graph-back")`, "parent graph back");
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "layout", "parent graph pointer navigation");

    await ctx.driver.shortcut(["Alt", "b"]);
    await expectVisibleRefusal(ctx, "bookmark saved", "bookmark keyboard status");
    await doubleClickCanvasPoint(ctx, await ctx.node("child"));
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "child", "bookmarked child pointer navigation");
    await ctx.driver.shortcut(["Alt", "g"]);
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "layout", "bookmark keyboard return");
    await ctx.openCanvas();
    if (await ctx.source() !== before) throw new Error("bookmark reload changed source bytes");
    const reloadedChild = graphByTitle(await ctx.graph(), "child");
    await pressAttribute(ctx, "data-sidebar-graph", reloadedChild.graph_id, "reloaded child graph");
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "child", "reloaded child navigation");
    await ctx.driver.shortcut(["Alt", "g"]);
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "layout", "persisted bookmark return");

    await selectNodeTitles(ctx, ["a"], "nudge a setup");
    await ctx.driver.press("ArrowLeft");
    await ctx.driver.press("ArrowLeft");
    await ctx.driver.press("ArrowLeft");
    await selectNodeTitles(ctx, ["c"], "nudge c setup");
    await ctx.driver.press("ArrowRight");
    await ctx.driver.press("ArrowRight");
    await ctx.driver.press("ArrowRight");
    await ctx.driver.press("ArrowRight");
    await ctx.driver.press("ArrowRight");
    await selectNodeTitles(ctx, ["a", "b", "c"], "layout selection setup");
    await ctx.driver.shortcut(["Alt", "a"]);
    await expectVisibleRefusal(ctx, "aligned top", "successful align status");
    const alignedState = await ctx.state();
    const aligned = Object.fromEntries(Object.values(alignedState.nodeBounds || {})
      .filter((node) => ["a", "b", "c"].includes(node.title))
      .map((node) => [node.title, node]));
    if (Object.keys(aligned).length !== 3 || Math.max(aligned.a.y, aligned.b.y, aligned.c.y) - Math.min(aligned.a.y, aligned.b.y, aligned.c.y) > 1) {
      throw new Error(`align did not produce one visible row: ${JSON.stringify(aligned)}`);
    }
    const alignedPositions = JSON.stringify(alignedState.savedNodePositions);
    const reloadedAlignedState = await reloadLayout("aligned positions");
    const reloadedAligned = Object.fromEntries(Object.values(reloadedAlignedState.nodeBounds || {})
      .filter((node) => ["a", "b", "c"].includes(node.title))
      .map((node) => [node.title, node]));
    if (JSON.stringify(reloadedAlignedState.savedNodePositions) !== alignedPositions
      || Object.keys(reloadedAligned).length !== 3
      || Math.max(reloadedAligned.a.y, reloadedAligned.b.y, reloadedAligned.c.y)
        - Math.min(reloadedAligned.a.y, reloadedAligned.b.y, reloadedAligned.c.y) > 1) {
      throw new Error(`aligned positions did not survive reload: ${JSON.stringify({
        alignedPositions,
        reloadedPositions: reloadedAlignedState.savedNodePositions,
        reloadedAligned
      })}`);
    }

    await selectNodeTitles(ctx, ["a", "b", "c"], "distribution selection setup");
    await ctx.driver.shortcut(["Alt", "d"]);
    await expectVisibleRefusal(ctx, "distributed horizontally", "successful distribute status");
    const distributedState = await ctx.state();
    const distributed = Object.values(distributedState.nodeBounds || {})
      .filter((node) => ["a", "b", "c"].includes(node.title))
      .sort((a, b) => a.x - b.x);
    if (distributed.length !== 3) throw new Error(`distribute lost selected nodes: ${JSON.stringify(distributed)}`);
    const leftGap = distributed[1].x - distributed[0].x;
    const rightGap = distributed[2].x - distributed[1].x;
    if (leftGap <= 0 || Math.abs(leftGap - rightGap) > 1) {
      throw new Error(`distribute did not create equal visible spacing: ${JSON.stringify(distributed)}`);
    }
    const distributedPositions = JSON.stringify(distributedState.savedNodePositions);
    const reloadedDistributedState = await reloadLayout("distributed positions");
    const reloadedDistributed = Object.values(reloadedDistributedState.nodeBounds || {})
      .filter((node) => ["a", "b", "c"].includes(node.title))
      .sort((a, b) => a.x - b.x);
    const reloadedLeftGap = reloadedDistributed.length === 3
      ? reloadedDistributed[1].x - reloadedDistributed[0].x
      : 0;
    const reloadedRightGap = reloadedDistributed.length === 3
      ? reloadedDistributed[2].x - reloadedDistributed[1].x
      : 0;
    if (JSON.stringify(reloadedDistributedState.savedNodePositions) !== distributedPositions
      || reloadedDistributed.length !== 3
      || reloadedLeftGap <= 0
      || Math.abs(reloadedLeftGap - reloadedRightGap) > 1) {
      throw new Error(`distributed positions did not survive reload: ${JSON.stringify({
        distributedPositions,
        reloadedPositions: reloadedDistributedState.savedNodePositions,
        reloadedDistributed
      })}`);
    }

    const beforeTidy = JSON.stringify((await ctx.state()).nodeBounds);
    await clickElement(ctx, `document.getElementById("org-tidy")`, "tidy graph");
    await expectVisibleRefusal(ctx, "graph tidied", "successful tidy status");
    const tidyState = await ctx.state();
    if (JSON.stringify(tidyState.nodeBounds) === beforeTidy) throw new Error("tidy did not change visible positions");
    if (!Object.keys(tidyState.savedNodePositions || {}).length) throw new Error("tidy did not persist node positions");

    await ctx.waitFor(async () => (await ctx.state()).favoriteCandidate, "favorite candidate");
    const favoriteBefore = await ctx.state();
    await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "more tools");
    await clickElement(ctx, `document.getElementById("favorite-action")`, "favorite action");
    await expectVisibleRefusal(ctx, "favorite pinned", "favorite status");
    const savedPositions = JSON.stringify(favoriteBefore.savedNodePositions);
    const favoriteId = favoriteBefore.favoriteCandidate;
    const favoriteTitle = favoriteBefore.favoriteCandidateTitle;

    await reloadLayout("tidy and favorite");
    await ctx.waitFor(async () => (await ctx.state()).favoriteCandidate === favoriteId, "reloaded favorite candidate");
    const reloaded = await ctx.state();
    if (JSON.stringify(reloaded.savedNodePositions) !== savedPositions) {
      throw new Error(`node positions did not persist across reload: ${JSON.stringify({ savedPositions, reloaded: reloaded.savedNodePositions })}`);
    }
    if (!reloaded.favorites.includes(favoriteId)
      || reloaded.favoriteCandidateRank < favoriteBefore.favoriteCandidateRank + 100000) {
      throw new Error(`favorite did not persist with durable ranking across reload: ${JSON.stringify({ favoriteBefore, reloaded })}`);
    }
    await ctx.driver.shortcut(["Control", "k"]);
    await ctx.type(favoriteTitle);
    const favoriteRow = await visibleSurface(ctx, `document.querySelector("#context-menu .action-result.is-favorite")`, "ranked favorite action");
    if (!favoriteRow.text.includes(favoriteTitle) || !favoriteRow.text.includes("★")) {
      throw new Error(`favorite action is not visibly ranked/pinned after reload: ${JSON.stringify({ favoriteRow, favoriteTitle })}`);
    }
  },

  "node-docs-pointer-hover": async (ctx) => {
    await ctx.openCanvas();
    const before = await ctx.source();
    const hoverAndAssert = async (nodeTitle, label) => {
      const defaultProfile = await ctx.driver.evaluate(`!document.body.classList.contains("is-dev-mode")`);
      if (!defaultProfile) throw new Error(`${label} unexpectedly enabled developer mode`);
      const target = await ctx.node(nodeTitle);
      await ctx.driver.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: target.x, y: target.y }, ctx.driver.pageSession);
      await ctx.waitFor(async () => {
        const state = await ctx.state();
        return state.hoveredNodeTitle === nodeTitle && String(state.hoveredNodeDescription || "").length > 0;
      }, `${label} node hover documentation`);
      const state = await ctx.state();
      const visible = await visibleSurface(ctx, `document.getElementById("wire-status")`, `${label} node hover details`);
      if (!visible.text.includes(nodeTitle) || !visible.text.includes(state.hoveredNodeDescription)) {
        throw new Error(`${label} node hover text mismatch: ${JSON.stringify({ visible, description: state.hoveredNodeDescription })}`);
      }
      return visible.text;
    };
    const squareDocs = await hoverAndAssert("square", "initial graph");
    if (!squareDocs.includes("Squares an integer input for this Canvas example.")) {
      throw new Error(`square hover did not use checked source docs: ${squareDocs}`);
    }
    await ctx.switchGraph("summarize");
    const resetState = await ctx.state();
    const resetSurface = await visibleSurface(ctx, `document.getElementById("wire-status")`, "navigation hover reset");
    if (resetState.hoveredNodeTitle
      || resetSurface.text.includes("square")
      || !resetSurface.text.includes("Hover a node or pin for details")) {
      throw new Error(`graph navigation retained stale hover docs: ${JSON.stringify({ resetState, resetSurface })}`);
    }
    const totalDocs = await hoverAndAssert("total", "navigated graph");
    if (totalDocs === squareDocs || totalDocs.includes("square")) {
      throw new Error(`navigated graph did not visibly replace hover docs: ${JSON.stringify({ squareDocs, totalDocs })}`);
    }
    if (await ctx.source() !== before) throw new Error("node docs hover changed source");
    const renamed = before.replace(/\btotal\b/g, "score");
    if (renamed === before) throw new Error("hover reprojection fixture has no total binding");
    const hoverDoc = await ctx.graph();
    const renameResult = await ctx.uiTransaction({
      schema_version: 1,
      op: "replace_source",
      revision: hoverDoc.revision,
      source: renamed
    });
    if (!renameResult.ok) throw new Error(`hover reprojection source write failed: ${JSON.stringify(renameResult)}`);
    await ctx.waitFor(async () => {
      const source = await ctx.source();
      return source.includes("score") && !source.includes("total");
    }, "hover reprojection source");
    const changed = await ctx.source();
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state.sourceText === changed && state.hoveredNodeTitle !== "total";
    }, "hover docs refreshed after reprojection");
    await assertCleanSourceSync(ctx, ["hover reprojection"]);
    const refreshed = await visibleSurface(ctx, `document.getElementById("wire-status")`, "reprojected node hover details");
    if (refreshed.text.includes("total")
      || (!refreshed.text.includes("score") && !refreshed.text.includes("Hover a node or pin for details"))) {
      throw new Error(`same-graph reprojection retained stale hover docs: ${JSON.stringify({ totalDocs, refreshed })}`);
    }
    await ctx.openCanvas();
    if (await ctx.source() !== changed) throw new Error("node docs hover reload changed source");
    await hoverAndAssert("square", "after reload");
  },

  "canvas-teaching-empty-states": async (ctx) => {
    await ctx.openCanvas();
    const original = await ctx.source();

    const loadingScript = await ctx.driver.send("Page.addScriptToEvaluateOnNewDocument", {
      source: `(() => {
        window.__canvasRealFetch = window.fetch;
        window.__canvasGraphRelease = null;
        window.fetch = (input, init) => {
          const url = typeof input === "string" ? input : input.url;
          if (!window.__canvasGraphRelease && String(url).includes("/canvas/graph")) {
            return new Promise((resolve, reject) => {
              window.__canvasGraphRelease = () => window.__canvasRealFetch(input, init).then(resolve, reject);
            });
          }
          return window.__canvasRealFetch(input, init);
        };
      })()`
    }, ctx.driver.pageSession);
    await ctx.driver.navigate(`http://127.0.0.1:${ctx.port}/canvas`);
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState?.kind === "loading"`), "initial loading state");
    await clickElement(ctx, `document.querySelector('[data-canvas-state-action="Show source"]')`, "recover source during initial loading");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const editor = document.getElementById("source-editor");
      return window.__jetCanvasCanvasState?.kind === "recovery"
        && editor?.value.includes("fn run")
        && getComputedStyle(editor).display !== "none";
    })()`), "initial loading source recovery");
    await ctx.driver.evaluate(`(() => {
      const release = window.__canvasGraphRelease;
      window.fetch = window.__canvasRealFetch;
      window.__canvasGraphRelease = null;
      if (release) release();
    })()`);
    await ctx.driver.send("Page.removeScriptToEvaluateOnNewDocument", { identifier: loadingScript.identifier }, ctx.driver.pageSession);
    await ctx.waitForCanvas();

    await ctx.setSourceEditor("// Empty Canvas source.\n");
    const toolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!toolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open source tools");
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "apply empty source");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.nodeCount === 0
        && Object.keys(state.nodeBounds || {}).length === 0
        && await ctx.driver.evaluate(`window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === "empty"`);
    }, "teaching empty state");
    const emptyState = await ctx.driver.evaluate(`({
      state: window.__jetCanvasCanvasState || null,
      source: document.getElementById("source-editor")?.value || "",
      overview: document.getElementById("graph-overview")?.textContent || "",
      graphList: document.getElementById("graph-list")?.textContent || ""
    })`);
    if (!emptyState.state.actions.includes("Open source")
      || !emptyState.state.actions.includes("Reload")
      || emptyState.overview.includes("summarize")
      || emptyState.graphList.trim()) {
      throw new Error(`empty Canvas state was not teaching or fresh: ${JSON.stringify(emptyState)}`);
    }

    await ctx.driver.navigate(`http://127.0.0.1:${ctx.port}/canvas`);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.nodeCount === 0
        && Object.keys(state.nodeBounds || {}).length === 0
        && await ctx.driver.evaluate(`window.__jetCanvasCanvasState?.kind === "empty"`);
    }, "initial empty Canvas state");

    await clickElement(ctx, `document.querySelector('[data-canvas-state-action="Open source"]')`, "recover empty source");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const editor = document.getElementById("source-editor");
      return !!editor && editor.value.includes("Empty Canvas source.") && getComputedStyle(editor).display !== "none";
    })()`), "empty source recovery editor");
    await ctx.setSourceEditor(original);
    await ctx.driver.evaluate(`document.getElementById("source-editor")?.dispatchEvent(new Event("input", { bubbles: true }))`);
    const recoveryToolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!recoveryToolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open recovery tools");
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "restore source after empty state");
    await ctx.waitForCanvas();
    await sleep(300);

    const invalidSource = original.replace("print(summarize(4))", "print(missing_recovery_value)");
    await ctx.setSourceEditor(invalidSource);
    await clickElement(ctx, `document.getElementById("check-current")`, "check invalid source recovery draft");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === "invalid"`), "invalid source recovery state");
    await clickElement(ctx, `document.querySelector('[data-canvas-state-action="Open source"]')`, "recover invalid source draft");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const editor = document.getElementById("source-editor");
      return window.__jetCanvasCanvasState?.kind === "recovery"
        && getComputedStyle(editor).display !== "none"
        && editor.value.includes("missing_recovery_value");
    })()`), "invalid source draft recovery");
    await ctx.setSourceEditor(original);
    await ctx.driver.evaluate(`document.getElementById("source-editor")?.dispatchEvent(new Event("input", { bubbles: true }))`);
    const invalidRecoveryToolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!invalidRecoveryToolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open invalid recovery tools");
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "restore source after invalid recovery");
    await ctx.waitForCanvas();
    await sleep(300);

    await ctx.driver.evaluate(`(() => {
      window.__canvasSourceFetch = window.fetch;
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input.url;
        if (String(url).includes("/canvas/source")) return Promise.reject(new Error("offline source request"));
        return window.__canvasSourceFetch(input, init);
      };
      window.dispatchEvent(new Event("offline"));
    })()`);
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState?.kind === "offline"`), "offline source recovery state");
    await clickElement(ctx, `document.querySelector('[data-canvas-state-action="Show source"]')`, "recover source from offline state");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`(() => {
      const editor = document.getElementById("source-editor");
      return window.__jetCanvasCanvasState?.kind === "recovery"
        && getComputedStyle(editor).display !== "none"
        && editor.value === ${JSON.stringify(original)};
    })()`), "offline source recovery editor");
    await clickElement(ctx, `document.querySelector('[data-canvas-state-action="Close"]')`, "close offline source recovery");
    await ctx.driver.evaluate(`(() => {
      window.fetch = window.__canvasSourceFetch;
      window.__canvasSourceFetch = null;
      window.dispatchEvent(new Event("online"));
    })()`);
    await sleep(300);

    await ctx.driver.evaluate(`(() => {
      window.__canvasRealFetch = window.fetch;
      window.__canvasGraphRelease = null;
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input.url;
        if (!window.__canvasGraphRelease && String(url).includes("/canvas/graph")) {
          return new Promise((resolve, reject) => {
            window.__canvasGraphRelease = () => window.__canvasRealFetch(input, init).then(resolve, reject);
          });
        }
        return window.__canvasRealFetch(input, init);
      };
    })()`);
    const reloadToolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!reloadToolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open reload tools");
    await clickElement(ctx, `document.getElementById("reload")`, "show loading state");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === "loading"`), "loading state");
    const loadingState = await ctx.driver.evaluate(`window.__jetCanvasCanvasState`);
    if (!loadingState.actions.includes("Show source") || !loadingState.actions.includes("Retry")) {
      throw new Error(`loading state lacked source recovery: ${JSON.stringify(loadingState)}`);
    }
    await ctx.driver.evaluate(`(() => {
      const release = window.__canvasGraphRelease;
      window.fetch = window.__canvasRealFetch;
      window.__canvasGraphRelease = null;
      if (release) release();
    })()`);
    await ctx.waitForCanvas();
    const reloadToolsStillOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (reloadToolsStillOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "close reload tools");

    await ctx.driver.shortcut(["Control", "p"]);
    await ctx.waitFor(async () => await ctx.driver.evaluate(`!!document.getElementById("action-palette-search")`), "empty action palette");
    await ctx.type("no_checked_canvas_action");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`!!document.querySelector("#context-menu [data-action-empty]")`), "empty action guidance");
    const palette = await ctx.driver.evaluate(`(() => {
      const empty = document.querySelector("#context-menu [data-action-empty]");
      return {
        text: empty?.textContent || "",
        source: !!empty?.querySelector("[data-menu-empty-source]"),
        close: !!empty?.querySelector("[data-menu-empty-close]")
      };
    })()`);
    if (!palette.source || !palette.close || !palette.text.includes("checked")) {
      throw new Error(`empty action palette lacked a next action: ${JSON.stringify(palette)}`);
    }
    await clickElement(ctx, `document.querySelector("[data-menu-empty-source]")`, "open source from empty palette");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === "recovery"`), "palette source recovery");
  },

  "harness-checked-doc-empty-noop-selftest": async (ctx) => {
    await ctx.openCanvas();
    const original = await ctx.source();
    const target = await ctx.node("square");
    await ctx.driver.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: target.x, y: target.y }, ctx.driver.pageSession);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state.hoveredNodeTitle === "square"
        && String(state.hoveredNodeDescription || "").includes("Squares an integer input");
    }, "hover production baseline");
    const blank = await ctx.canvasRect();
    await ctx.driver.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: blank.left + 1, y: blank.top + 1 }, ctx.driver.pageSession);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return !state.hoveredNodeTitle && !state.hoveredNodeDescription;
    }, "clear hover production baseline");
    await ctx.driver.evaluate("window.__jetCanvasNoopHover = true");
    try {
      await ctx.driver.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: target.x, y: target.y }, ctx.driver.pageSession);
      let hoverFailed = false;
      try {
        await ctx.waitFor(async () => {
          const state = await ctx.state();
          return state.hoveredNodeTitle === "square" && String(state.hoveredNodeDescription || "").includes("Squares an integer input");
        }, "hover no-op self-test", 1200);
      } catch (_) {
        hoverFailed = true;
      }
      if (!hoverFailed) throw new Error("checked-doc hover scenario passed with pointer hover bypassed");
    } finally {
      await ctx.driver.evaluate("window.__jetCanvasNoopHover = false");
    }

    const toolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!toolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open empty-state tools");
    await ctx.setSourceEditor("// Empty Canvas source.\n");
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "apply empty source baseline");
    await ctx.waitFor(async () => (await ctx.source()).includes("// Empty Canvas source."), "empty source baseline transaction");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === "empty"`), "empty state production baseline");
    const baselineEmpty = await ctx.driver.evaluate(`window.__jetCanvasCanvasState`);
    if (!baselineEmpty.actions.includes("Open source") || !baselineEmpty.actions.includes("Reload")) {
      throw new Error(`empty production baseline lacked recovery actions: ${JSON.stringify(baselineEmpty)}`);
    }
    await clickElement(ctx, `document.querySelector('[data-canvas-state-action="Open source"]')`, "restore source after empty baseline");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState?.kind === "recovery"`), "empty baseline recovery");
    await ctx.setSourceEditor(original);
    const baselineRecoveryToolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!baselineRecoveryToolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open baseline recovery tools");
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "restore source after empty baseline");
    await ctx.waitForCanvas();
    await ctx.setSourceEditor("// Empty Canvas source.\n");
    await ctx.driver.evaluate("window.__jetCanvasNoopCanvasState = true");
    try {
      await clickElement(ctx, `document.getElementById("apply-source-edit")`, "apply empty source with state bypass");
      await ctx.waitFor(async () => (await ctx.source()).includes("// Empty Canvas source."), "empty source transaction");
      let emptyFailed = false;
      try {
        await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === "empty"`), "empty-state no-op self-test", 1200);
      } catch (_) {
        emptyFailed = true;
      }
      if (!emptyFailed) throw new Error("teaching empty-state scenario passed with Canvas state bypassed");
    } finally {
      await ctx.driver.evaluate("window.__jetCanvasNoopCanvasState = false");
    }
  },

  "rename-variable-sidebar": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("summarize");
    await ctx.driver.evaluate(`window.__jetCanvasTest.selectVariable("total")`);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.selectedVariableName === "total";
    }, "sidebar variable selected");
    await ctx.waitFor(async () => {
      return await ctx.driver.evaluate(`!!document.getElementById("variable-name-input")`);
    }, "variable sidebar editor");
    const ok = await ctx.driver.evaluate(`(() => {
      const name = document.getElementById("variable-name-input");
      const apply = document.getElementById("apply-variable-details");
      if (!name) return false;
      if (!apply) return false;
      name.value = "score";
      apply.click();
      return true;
    })()`);
    if (!ok) {
      const doc = await ctx.graph();
      await uiEdit(ctx, {
        schema_version: 1,
        op: "rename_binding",
        revision: doc.revision,
        from: "total",
        to: "score"
      }, "sidebar-selected variable rename");
    }
    await ctx.waitFor(async () => (await ctx.source()).includes("score := square(limit)") || (await ctx.source()).includes("score :: square(limit)"), "sidebar rename source");
    await ctx.waitForCanvas();
    await ctx.expectSourceContains("if score > 10");
  },

  "project-rename-preview-commit": async (ctx) => {
    const waitForProjectModel = async (project, label) => {
      await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasProjectRail && window.__jetCanvasProjectRail.files === ${project.files.length}`), label);
    };
    const ensureFunctionRenameControls = async (label) => {
      const visible = async () => await ctx.driver.evaluate(`(() => {
        const input = document.getElementById("function-rename-to");
        if (!input) return false;
        const rect = input.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0 && rect.right > 0 && rect.bottom > 0;
      })()`);
      if (!await visible()) await clickElement(ctx, `document.getElementById("dock-details")`, "open function details");
      await ctx.waitFor(visible, label);
    };
    await ctx.openCanvas();
    if (await ctx.driver.evaluate("document.getElementById('first-run-tour')?.classList.contains('is-open')")) {
      await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide before project rename");
    }
    const project = await ctx.driver.evaluate(`fetch("/canvas/project", { cache: "no-store" }).then((r) => r.json())`);
    const root = project.project_root;
    await writeFile(join(root, "package.jet"), "name: \"canvas_project_rename\"\nversion: \"0.1.0\"\n");
    await writeFile(join(root, "main.jet"), `pub fn helper() Int -> {
    return 1
}

fn run() {
    print(helper())
}
`);
    await writeFile(join(root, "other.jet"), `use "./main" as main

fn use_helper() Int -> {
    return main.helper()
}
`);
    await ctx.openCanvas();
    const doc = await ctx.graph();
    const source = await ctx.source();
    const projectNow = await ctx.driver.evaluate(`fetch("/canvas/project", { cache: "no-store" }).then((r) => r.json())`);
    await waitForProjectModel(projectNow, "project model for rename");
    const preview = await ctx.query({
      schema_version: 1,
      op: "preview_rename",
      source_id: "main.jet",
      revision: doc.revision,
      project_revision: projectNow.project_revision,
      symbol: "helper",
      to: "compute"
    });
    if (!preview.ok || preview.op !== "preview_rename" || !preview.diff || preview.diff.files.length !== 2
      || !preview.results.some((result) => result.source_id === "other.jet")
      || !preview.diff.text.includes("+pub fn compute()")) {
      throw new Error(`project rename preview incomplete: ${JSON.stringify(preview)}`);
    }
    if (await ctx.source() !== source) throw new Error("project rename preview wrote source");

    await ctx.switchGraph("helper");
    await ensureFunctionRenameControls("function rename controls");
    await ctx.driver.evaluate(`(() => {
      const realFetch = window.fetch.bind(window);
      window.__jetCanvasProjectTxRequests = [];
      window.__jetCanvasSourceTxRequests = [];
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input && input.url || "";
        if (url.endsWith("/canvas/project/transaction")) {
          window.__jetCanvasProjectTxRequests.push(typeof init?.body === "string" ? JSON.parse(init.body) : { method: init?.method || null });
        } else if (url.endsWith("/canvas/transaction")) {
          window.__jetCanvasSourceTxRequests.push(typeof init?.body === "string" ? JSON.parse(init.body) : { method: init?.method || null });
        }
        return realFetch(input, init);
      };
    })()`);
    await replaceSearch(ctx, `document.getElementById("function-rename-to")`, "compute", "project rename input");
    const renameValue = await ctx.driver.evaluate(`document.getElementById("function-rename-to")?.value || ""`);
    if (renameValue !== "compute") throw new Error(`project rename input gesture did not update the field: ${renameValue}`);
    const renamePhase = await ctx.driver.evaluate(`window.__jetCanvasDetailsState?.phase || ""`);
    if (renamePhase !== "dirty") throw new Error(`project rename input gesture did not mark the editor dirty: ${renamePhase}`);
    await pressAttribute(ctx, "id", "rename-function", "project rename apply");
    await ctx.waitFor(async () => {
      return await ctx.driver.evaluate(`window.__jetCanvasLastTxResult !== null && window.__jetCanvasLastTxResult !== undefined`);
    }, "project rename UI result");
    const successResult = await ctx.driver.evaluate(`window.__jetCanvasLastTxResult`);
    if (successResult.protocol !== "jet.canvas.project.edit" || successResult.writes !== "source_transaction" || successResult.changed !== true) {
      throw new Error(`project rename UI transaction failed: ${JSON.stringify(successResult)}`);
    }
    const projectTx = await ctx.driver.evaluate(`window.__jetCanvasProjectTxRequests`);
    const sourceTx = await ctx.driver.evaluate(`window.__jetCanvasSourceTxRequests`);
    const sourceRevisions = new Map((projectNow.files || [])
      .filter((file) => file.kind === "source")
      .map((file) => [file.path, file.revision]));
    const request = projectTx && projectTx[0];
    const touched = new Map((request && request.files || []).map((file) => [file.path, file.revision]));
    if (!request || projectTx.length !== 1 || request.op !== "rename_function"
      || request.source_id !== "main.jet" || request.project_revision !== projectNow.project_revision
      || touched.get("main.jet") !== sourceRevisions.get("main.jet")
      || touched.get("other.jet") !== sourceRevisions.get("other.jet")
      || !sourceTx || sourceTx.length !== 0) {
      throw new Error(`project rename bypassed checked cross-file transaction: ${JSON.stringify({ projectTx, sourceTx, projectRevision: projectNow.project_revision, sourceRevisions: Object.fromEntries(sourceRevisions) })}`);
    }
    const successAudit = await ctx.driver.evaluate(`window.__jetCanvasLastTxResult.audit && window.__jetCanvasLastTxResult.audit.touched_files`);
    if (!successAudit || !successAudit.some((file) => file.path === "main.jet") || !successAudit.some((file) => file.path === "other.jet")) {
      throw new Error(`project rename response omitted cross-file audit: ${JSON.stringify(successAudit)}`);
    }
    await ctx.openCanvas();
    if (!(await ctx.source()).includes("compute()")) throw new Error("project rename did not update entry source");
    let other = null;
    await ctx.waitFor(async () => {
      other = await ctx.driver.evaluate(`fetch("/canvas/graph?source_id=other.jet", { cache: "no-store" }).then((r) => r.json())`);
      return other.protocol === "jet.canvas.graph" && other.source_text.includes("compute()");
    }, "project rename reference source");
    if (other.protocol !== "jet.canvas.graph" || !other.source_text.includes("compute()")) throw new Error("project rename did not update reference source");

    await writeFile(join(root, "main.jet"), `pub fn helper() Int -> {
    return 1
}

fn run() {
    print(helper())
}
`);
    await writeFile(join(root, "other.jet"), `use "./main" as main

fn use_helper() Int -> {
    return main.helper()
}
    `);
    await ctx.openCanvas();
    const staleProject = await ctx.driver.evaluate(`fetch("/canvas/project", { cache: "no-store" }).then((r) => r.json())`);
    await waitForProjectModel(staleProject, "stale project model for rename");
    await ctx.switchGraph("helper");
    const staleSource = await ctx.source();
    const staleOtherBefore = await ctx.driver.evaluate(`fetch("/canvas/graph?source_id=other.jet", { cache: "no-store" }).then((r) => r.json())`);
    await ensureFunctionRenameControls("stale function rename controls");
    await ctx.driver.evaluate(`(() => {
      const originalFetch = window.fetch.bind(window);
      window.__jetCanvasStaleProjectOnce = true;
      window.__jetCanvasProjectTxRequests = [];
      window.__jetCanvasSourceTxRequests = [];
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input && input.url || "";
        if (window.__jetCanvasStaleProjectOnce && url.endsWith("/canvas/project/transaction")) {
          window.__jetCanvasStaleProjectOnce = false;
          const body = JSON.parse(init.body);
          const sent = Object.assign({}, body, {
            files: body.files.map((file) => file.path === "main.jet" ? Object.assign({}, file, { revision: "sha256-stale" }) : file)
          });
          window.__jetCanvasProjectTxRequests.push({ body, sent });
          init = Object.assign({}, init, { body: JSON.stringify(sent) });
        } else if (url.endsWith("/canvas/transaction")) {
          window.__jetCanvasSourceTxRequests.push(typeof init?.body === "string" ? JSON.parse(init.body) : { method: init?.method || null });
        }
        return originalFetch(input, init);
      };
    })()`);
    await replaceSearch(ctx, `document.getElementById("function-rename-to")`, "compute", "stale project rename input");
    const staleRenameValue = await ctx.driver.evaluate(`document.getElementById("function-rename-to")?.value || ""`);
    if (staleRenameValue !== "compute") throw new Error(`stale project rename input gesture did not update the field: ${staleRenameValue}`);
    await pressAttribute(ctx, "id", "rename-function", "stale project rename apply");
    await ctx.waitFor(async () => {
      const result = await ctx.driver.evaluate(`window.__jetCanvasLastTxResult`);
      return result && result.kind === "conflict";
    }, "stale project rename refusal");
    const staleProjectTx = await ctx.driver.evaluate(`window.__jetCanvasProjectTxRequests`);
    const staleSourceTx = await ctx.driver.evaluate(`window.__jetCanvasSourceTxRequests`);
    const staleRequest = staleProjectTx && staleProjectTx[0];
    if (!staleRequest || staleProjectTx.length !== 1
      || staleRequest.body.op !== "rename_function"
      || !staleRequest.body.files.some((file) => file.path === "main.jet" && file.revision !== "sha256-stale")
      || !staleRequest.sent.files.some((file) => file.path === "main.jet" && file.revision === "sha256-stale")
      || !staleRequest.sent.files.some((file) => file.path === "other.jet")
      || !staleSourceTx || staleSourceTx.length !== 0) {
      throw new Error(`stale project rename bypassed checked refusal path: ${JSON.stringify({ staleProjectTx, staleSourceTx })}`);
    }
    await expectVisibleRefusal(ctx, "source file changed since", "stale project rename message");
    if (await ctx.source() !== staleSource) throw new Error("stale project rename changed entry source");
    const staleOther = await ctx.driver.evaluate(`fetch("/canvas/graph?source_id=other.jet", { cache: "no-store" }).then((r) => r.json())`);
    if (staleOther.source_text !== staleOtherBefore.source_text || !staleOther.source_text.includes("main.helper()")) throw new Error("stale project rename changed reference source");
  },

  "details-scalar-enum-reference-editors": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    await ctx.replaceSource(`enum Mode {
    Fast
    Slow
}

/// Edits a checked choice.
pub fn edit(choice: Mode) -[]> {
    #Meta(category: "Tuning", tunable)
    amount :: 3
    other :: 9
    flag :: true
    label :: "start"
    mode :: Mode.Fast
    alias :: amount
    needs_int(amount)
    print(alias)
}

fn needs_int(value: Int) {}

fn run() {
    edit(Mode.Fast)
}
    `);
    await ctx.openCanvas();
    await ctx.switchGraph("edit");
    await clickElement(ctx, `document.getElementById("dock-details")`, "open Inspector");

    const selectVariable = async (name) => {
      const selected = await ctx.driver.evaluate(`window.__jetCanvasTest.selectVariable(${JSON.stringify(name)})`);
      if (selected === false) throw new Error(`variable selection helper refused ${name}`);
      await ctx.waitFor(async () => {
        const state = await ctx.state();
        return state && state.selectedVariableName === name;
      }, `${name} selected`);
      await ctx.waitFor(async () => await ctx.driver.evaluate(`!!document.querySelector('[data-details-input="value"]')`), `${name} Details field`);
    };

    const applyValue = async (name, value, expectedKind, label) => {
      const before = await ctx.source();
      await selectVariable(name);
      const changed = await ctx.driver.evaluate(`(() => {
        const input = document.querySelector('[data-details-input="value"]');
        const apply = document.getElementById("apply-variable-details");
        if (!input || !apply) return { ok: false };
        if (${JSON.stringify(expectedKind)} && input.dataset.detailKind !== ${JSON.stringify(expectedKind)}) return { ok: false, kind: input.dataset.detailKind };
        if (input.type === "checkbox") input.checked = ${JSON.stringify(value)} === "true";
        else input.value = ${JSON.stringify(value)};
        apply.click();
        return { ok: true, kind: input.dataset.detailKind, tag: input.tagName };
      })()`);
      if (!changed.ok) throw new Error(`${label} control missing or wrong kind: ${JSON.stringify(changed)}`);
      await ctx.waitFor(async () => (await ctx.source()) !== before, `${label} source change`);
      await ctx.waitForCanvas();
    };

    await selectVariable("choice");
    const beforeIncompleteEnum = await ctx.source();
    const incompleteEnum = await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="value"]');
      const apply = document.getElementById("apply-variable-details");
      if (!input || !apply) return { ok: false };
      const before = input.value;
      apply.click();
      return { ok: true, tag: input.tagName, kind: input.dataset.detailKind, value: before };
    })()`);
    if (!incompleteEnum.ok || incompleteEnum.tag !== "SELECT" || incompleteEnum.value !== "") {
      throw new Error(`incomplete enum editor changed its default: ${JSON.stringify(incompleteEnum)}`);
    }
    await ctx.waitFor(async () => !!(await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null")), "incomplete enum result");
    if (await ctx.source() !== beforeIncompleteEnum) throw new Error("incomplete enum edit changed source");

    await selectVariable("amount");
    const metadata = await ctx.driver.evaluate(`(() => {
      const meta = document.querySelector("[data-details-meta]");
      const input = document.querySelector('[data-details-input="value"]');
      return { text: meta && meta.textContent, type: input && input.type, kind: input && input.dataset.detailKind };
    })()`);
    if (!metadata || !String(metadata.text || "").includes("Tuning") || !String(metadata.text || "").includes("tunable")) {
      throw new Error(`binding metadata missing from Details: ${JSON.stringify(metadata)}`);
    }
    if (metadata.kind !== "scalar" || metadata.type !== "number") throw new Error(`scalar editor missing: ${JSON.stringify(metadata)}`);
    const descriptorSurface = await ctx.driver.evaluate(`(() => {
      const root = document.getElementById("details");
      return {
        fields: root && root.querySelectorAll("[data-details-field]").length,
        applyButtons: root && root.querySelectorAll("[data-field-apply]").length,
        unsafeHandlers: root && root.querySelectorAll("[onclick], [onchange], [onerror], [onload]").length,
        state: window.__jetCanvasDetailsState && window.__jetCanvasDetailsState.phase
      };
    })()`);
    if (!descriptorSurface || descriptorSurface.unsafeHandlers !== 0 || descriptorSurface.fields < 1 || descriptorSurface.applyButtons !== 1) {
      throw new Error(`Details did not use safe descriptor rows: ${JSON.stringify(descriptorSurface)}`);
    }

    const beforeEscape = await ctx.source();
    await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="value"]');
      input.focus();
      input.value = "99";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    })()`);
    await ctx.driver.press("Escape");
    await ctx.waitFor(async () => (await ctx.driver.evaluate(`window.__jetCanvasDetailsState && window.__jetCanvasDetailsState.phase`)) === "clean", "Details Escape reset");
    if (await ctx.source() !== beforeEscape) throw new Error("Escape wrote a Details edit");
    const escapedValue = await ctx.driver.evaluate(`document.querySelector('[data-details-input="value"]').value`);
    if (escapedValue !== "3") throw new Error(`Escape did not restore Details value: ${escapedValue}`);

    await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="value"]');
      input.focus();
      input.value = "8";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    })()`);
    await ctx.driver.press("Enter");
    await ctx.waitFor(async () => (await ctx.source()).includes("amount :: 8"), "Details Enter apply");
    await ctx.waitForCanvas();

    await selectVariable("amount");
    await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="value"]');
      input.focus();
      input.value = "6";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      document.getElementById("toolbar-search").focus();
    })()`);
    await ctx.waitFor(async () => (await ctx.source()).includes("amount :: 6"), "Details blur apply");
    await ctx.waitForCanvas();

    await selectVariable("amount");
    const beforeSelectionChange = await ctx.source();
    await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="value"]');
      input.focus();
      input.value = "77";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    })()`);
    await ctx.driver.evaluate(`window.__jetCanvasTest.selectVariable("other")`);
    await ctx.waitFor(async () => (await ctx.state()).selectedVariableName === "other", "Details selection change");
    if (await ctx.source() !== beforeSelectionChange) throw new Error("selection change wrote a dirty Details edit");

    await applyValue("flag", "false", "scalar", "boolean Details edit");
    await applyValue("label", "line\n<b>☃</b>", "scalar", "string Details edit");
    await applyValue("amount", "7", "scalar", "scalar Details edit");
    await applyValue("mode", "Mode.Slow", "enum", "enum Details edit");
    await applyValue("alias", "other", "reference", "reference Details edit");
    const edited = await ctx.source();
    for (const text of ["amount :: 7", "flag :: false", "label :: \"line\\n<b>☃</b>\"", "mode :: Mode.Slow", "alias :: other", "print(alias)"]) {
      if (!edited.includes(text)) throw new Error(`Details edit missing ${text}:\n${edited}`);
    }

    await ctx.openCanvas();
    if (await ctx.source() !== edited) throw new Error("Details edits changed source on reload");
    await selectVariable("label");
    const unicodeMultiline = await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="value"]');
      const row = input && input.closest('[data-details-field]');
      return {
        value: input && input.value,
        expected: "line\\n<b>☃</b>",
        rowText: row && row.textContent,
        handlers: row && row.querySelectorAll("[onclick], [onchange], [onerror], [onload]").length,
        markupNodes: row && row.querySelectorAll("b, img, script").length
      };
    })()`);
    if (!unicodeMultiline || unicodeMultiline.value !== unicodeMultiline.expected || unicodeMultiline.handlers !== 0 || unicodeMultiline.markupNodes !== 0 || String(unicodeMultiline.rowText || "").includes("<b>")) {
      throw new Error(`Details text lost safe multiline/Unicode rendering: ${JSON.stringify(unicodeMultiline)}`);
    }
    await selectVariable("alias");
    const reloaded = await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="value"]');
      return { kind: input && input.dataset.detailKind, value: input && input.value };
    })()`);
    if (!reloaded || reloaded.kind !== "reference" || reloaded.value !== "other") throw new Error(`reference editor did not reload its source value: ${JSON.stringify(reloaded)}`);

    const undone = await ctx.undo();
    if (!undone.includes("alias :: amount")) throw new Error(`undo did not restore reference source:\n${undone}`);
    const redone = await ctx.redo();
    if (!redone.includes("alias :: other")) throw new Error(`redo did not restore reference edit:\n${redone}`);

    const beforeInvalid = await ctx.source();
    await selectVariable("amount");
    const invalidStarted = await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="value"]');
      const apply = document.getElementById("apply-variable-details");
      if (!input || !apply) return false;
      input.value = "";
      apply.click();
      return true;
    })()`);
    if (!invalidStarted) throw new Error("scalar refusal control missing");
    await ctx.waitFor(async () => {
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      return result && result.ok === false;
    }, "incomplete scalar refusal");
    const refusalState = await ctx.driver.evaluate("window.__jetCanvasDetailsState || null");
    if (!refusalState || refusalState.phase !== "refused" || !(refusalState.event === "validation-error" || refusalState.event === "transaction-refused")) {
      throw new Error(`Details refusal state was not preserved: ${JSON.stringify(refusalState)}`);
    }
    await assertSourceUnchangedAfterReload(ctx, beforeInvalid, "incomplete scalar edit");

    await ctx.openCanvas();
    await ctx.switchGraph("edit");
    const functionDetailsVisible = await ctx.driver.evaluate(`(() => {
      const element = document.getElementById("function-signature");
      if (!element) return false;
      const rect = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return rect.width > 0 && rect.height > 0 && style.display !== "none" && style.visibility !== "hidden";
    })()`);
    if (!functionDetailsVisible) await clickElement(ctx, `document.getElementById("dock-details")`, "open function Inspector");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`!!document.getElementById("function-signature") && !!document.getElementById("function-visibility") && !!document.getElementById("function-pure")`), "function Details fields");
    const functionSurface = await ctx.driver.evaluate(`(() => {
      const root = document.getElementById("details");
      const docs = root && root.querySelector('[data-details-value="docs"]');
      const signature = document.getElementById("function-signature");
      const visibility = document.getElementById("function-visibility");
      const pure = document.getElementById("function-pure");
      return {
        docs: docs && docs.textContent,
        signature: signature && signature.value,
        visibility: visibility && visibility.value,
        pure: pure && pure.checked,
        apply: root && root.querySelector('[data-field-apply="function-signature"]') && root.querySelector('[data-field-apply="function-signature"]').id
      };
    })()`);
    if (!functionSurface || functionSurface.docs !== "Edits a checked choice." || !String(functionSurface.signature || "").includes(" -[]>")
      || functionSurface.visibility !== "public" || functionSurface.pure !== true || functionSurface.apply !== "edit-function-signature") {
      throw new Error(`function Details projection is incomplete: ${JSON.stringify(functionSurface)}`);
    }

    const beforeFunction = await ctx.source();
    await clickElement(ctx, `document.getElementById("function-signature")`, "function signature editor");
    await ctx.driver.shortcut(["Control", "A"]);
    await ctx.driver.type("pub(package) fn edit(choice: Mode) -[]>");
    await clickElement(ctx, `document.getElementById("edit-function-signature")`, "function signature apply");
    await ctx.waitFor(async () => {
      const source = await ctx.source();
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      return source.includes("pub(package) fn edit(choice: Mode) -[]>") && result && result.changed === true;
    }, "function signature source transaction");
    await ctx.waitForCanvas();
    const afterFunction = await ctx.source();
    const undoneFunction = await ctx.undo();
    if (undoneFunction !== beforeFunction) throw new Error("function signature undo did not restore exact source");
    const redoneFunction = await ctx.redo();
    if (redoneFunction !== afterFunction) throw new Error("function signature redo did not restore exact source");

    await ctx.openCanvas();
    await ctx.switchGraph("edit");
    await ctx.driver.evaluate(`(() => {
      const signature = document.getElementById("function-signature");
      const apply = document.getElementById("edit-function-signature");
      if (!signature || !apply) return false;
      signature.focus();
      signature.value = "pub(package) fn edit(";
      signature.dispatchEvent(new Event("input", { bubbles: true }));
      apply.click();
      return true;
    })()`);
    await ctx.waitFor(async () => {
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      return result && result.ok === false;
    }, "invalid function signature refusal");
    const functionRefusal = await ctx.driver.evaluate("window.__jetCanvasDetailsState || null");
    if (!functionRefusal || functionRefusal.phase !== "refused" || !String(functionRefusal.reason || "").length) {
      throw new Error(`function signature refusal state was not preserved: ${JSON.stringify(functionRefusal)}`);
    }
    if (await ctx.source() !== afterFunction) throw new Error("invalid function signature changed source");
  },

  "details-collection-nested-editors": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn edit() {
    matrix :: [[1, 2], [3, 4]]
    settings :: Point{x: 4, y: [5, 6]}
    lookup :: ["first": 7, "second": 8]
    print(1)
}

struct Point {
    x: Int
    y: [Int]
}

fn run() {
    edit()
}
`);
    await ctx.openCanvas();
    await ctx.switchGraph("edit");

    const exposeDetails = async () => {
      await ctx.driver.evaluate(`(() => {
        const drawer = document.getElementById("right-drawer");
        if (!drawer) return false;
        drawer.classList.add("is-drawer-open");
        drawer.style.display = "block";
        drawer.style.position = "fixed";
        drawer.style.right = "0";
        drawer.style.top = "0";
        drawer.style.bottom = "0";
        drawer.style.width = "326px";
        drawer.style.zIndex = "40";
        document.getElementById("dock-details")?.classList.add("is-active");
        return true;
      })()`);
    };

    const selectVariable = async (name) => {
      const selected = await ctx.driver.evaluate(`window.__jetCanvasTest.selectVariable(${JSON.stringify(name)})`);
      if (selected === false) throw new Error(`collection variable selection helper refused ${name}`);
      await ctx.waitFor(async () => (await ctx.state()).selectedVariableName === name, `${name} collection selected`);
      await exposeDetails();
      await ctx.waitFor(async () => await ctx.driver.evaluate(`document.querySelectorAll('[data-details-input][data-details-nested="true"]').length > 0`), `${name} nested Details controls`);
      await assertLiveDetailsControls(ctx, `${name} collection`);
    };

    const setNested = async (path, value) => {
      const result = await ctx.driver.evaluate(`(() => {
        const input = document.querySelector('[data-details-input][data-details-path="${path}"]');
        if (!input) return { ok: false };
        input.focus();
        input.value = ${JSON.stringify(value)};
        input.dispatchEvent(new Event("input", { bubbles: true }));
        return { ok: true, kind: input.dataset.detailKind, type: input.dataset.detailType };
      })()`);
      if (!result.ok) throw new Error(`nested Details control missing: ${path}`);
      return result;
    };

    await selectVariable("lookup");
    const mapSurface = await ctx.driver.evaluate(`(() => {
      const controls = [...document.querySelectorAll('#details [data-details-input]')];
      const applyButtons = [...document.querySelectorAll('#details [data-field-apply]')];
      const apply = document.querySelector('#details [data-field-apply]');
      return {
        paths: controls.map((input) => input.dataset.detailsPath || ""),
        nested: controls.filter((input) => input.dataset.detailsNested === "true").length,
        apply: !!apply,
        dead: controls.some((input) => !input.closest('[data-details-field]')
          || !input.dataset.detailsApplyOp
          || !applyButtons.some((button) => button.dataset.fieldApply === input.dataset.detailsApplyOp))
      };
    })()`);
    if (!mapSurface || mapSurface.nested < 4 || !mapSurface.apply || mapSurface.dead
      || !mapSurface.paths.includes("value[0].value") || !mapSurface.paths.includes("value[1].value")) {
      throw new Error(`map Details surface is incomplete or dead: ${JSON.stringify(mapSurface)}`);
    }
    const beforeMap = await ctx.source();
    await setNested("value[0].value", "10");
    await clickElement(ctx, `document.getElementById("apply-variable-details")`, "map nested apply");
    await ctx.waitFor(async () => {
      const source = await ctx.source();
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      const state = await ctx.state();
      if (result && result.ok === false) throw new Error(`map nested apply refused: ${JSON.stringify(result)}`);
      return source.includes('lookup :: ["first": 10, "second": 8]')
        && result && result.changed === true && result.source_text === source && state && state.undoDepth >= 1;
    }, "map nested apply");
    const afterMap = await ctx.source();
    const undoneMap = await ctx.undo();
    if (undoneMap !== beforeMap) throw new Error("collection map undo did not restore exact source");
    const redoneMap = await ctx.redo();
    if (redoneMap !== afterMap) throw new Error("collection map redo did not restore exact source");

    await selectVariable("settings");
    const structSurface = await ctx.driver.evaluate(`({
      paths: [...document.querySelectorAll('#details [data-details-input]')].map((input) => input.dataset.detailsPath || ""),
      nested: document.querySelectorAll('#details [data-details-input][data-details-nested="true"]').length
    })`);
    if (!structSurface.paths.includes("value.x") || !structSurface.paths.includes("value.y[0]") || structSurface.nested < 3) {
      throw new Error(`struct Details surface is incomplete: ${JSON.stringify(structSurface)}`);
    }
    const beforeStruct = await ctx.source();
    await setNested("value.x", "9");
    await setNested("value.y[1]", "7");
    await clickElement(ctx, `document.getElementById("apply-variable-details")`, "struct nested apply");
    await ctx.waitFor(async () => {
      const source = await ctx.source();
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      const state = await ctx.state();
      return source.includes("settings :: Point{x: 9, y: [5, 7]}")
        && result && result.changed === true && result.source_text === source && state && state.undoDepth >= 1;
    }, "struct nested apply");
    const afterStruct = await ctx.source();
    const undoneStruct = await ctx.undo();
    if (undoneStruct !== beforeStruct) throw new Error("struct nested undo did not restore exact source");
    const redoneStruct = await ctx.redo();
    if (redoneStruct !== afterStruct) throw new Error("struct nested redo did not restore exact source");

    await selectVariable("matrix");
    const beforeEscape = await ctx.source();
    await setNested("value[0][0]", "9");
    await setNested("value[1][1]", "8");
    await ctx.driver.press("Escape");
    await ctx.waitFor(async () => {
      const state = await ctx.driver.evaluate("window.__jetCanvasDetailsState || null");
      return state && state.phase === "clean";
    }, "nested Escape refusal");
    if (await ctx.source() !== beforeEscape) throw new Error("nested Escape wrote source");
    const escaped = await ctx.driver.evaluate(`({
      first: document.querySelector('[data-details-path="value[0][0]"]').value,
      last: document.querySelector('[data-details-path="value[1][1]"]').value
    })`);
    if (escaped.first !== "1" || escaped.last !== "4") throw new Error(`nested Escape did not restore all fields: ${JSON.stringify(escaped)}`);

    await setNested("value[0][1]", "11");
    await ctx.driver.evaluate(`document.getElementById("toolbar-search").focus()`);
    await ctx.waitFor(async () => {
      const source = await ctx.source();
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      const state = await ctx.state();
      return source.includes("matrix :: [[1, 11], [3, 4]]")
        && result && result.changed === true && result.source_text === source && state && state.undoDepth >= 1;
    }, "nested blur apply");
    const beforeMatrixApply = await ctx.source();
    await selectVariable("matrix");
    await setNested("value[1][0]", "12");
    await clickElement(ctx, `document.getElementById("apply-variable-details")`, "matrix nested apply");
    await ctx.waitFor(async () => {
      const source = await ctx.source();
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      const state = await ctx.state();
      return source.includes("matrix :: [[1, 11], [12, 4]]")
        && result && result.changed === true && result.source_text === source && state && state.undoDepth >= 1;
    }, "nested matrix apply");
    const afterMatrixApply = await ctx.source();
    const undoneMatrix = await ctx.undo();
    if (undoneMatrix !== beforeMatrixApply) throw new Error("nested matrix undo did not restore exact source");
    const redoneMatrix = await ctx.redo();
    if (redoneMatrix !== afterMatrixApply) throw new Error("nested matrix redo did not restore exact source");

    await ctx.openCanvas();
    await ctx.switchGraph("edit");
    await selectVariable("matrix");
    const reloaded = await ctx.driver.evaluate(`document.querySelector('[data-details-path="value[1][0]"]').value`);
    if (reloaded !== "12") throw new Error(`nested collection value did not reload: ${reloaded}`);

    const beforeInvalid = await ctx.source();
    await setNested("value[0][0]", "not-an-int");
    await ctx.driver.evaluate(`document.getElementById("toolbar-search").focus()`);
    await ctx.waitFor(async () => {
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      return result && result.ok === false;
    }, "nested validation refusal on blur");
    const invalidState = await ctx.driver.evaluate("window.__jetCanvasDetailsState || null");
    if (!invalidState || invalidState.phase !== "refused" || !invalidState.reason) {
      throw new Error(`nested validation refusal was not visible: ${JSON.stringify(invalidState)}`);
    }
    if (await ctx.source() !== beforeInvalid) throw new Error("invalid nested edit changed source bytes");

    const doc = await ctx.graph();
    const graph = graphByTitle(doc, "edit");
    const lookup = (graph.inline_exprs || []).find((expr) => String(expr.source || "").includes('"first": 10'));
    if (!lookup) throw new Error("lookup source anchor missing for stale nested refusal");
    await setNested("value[0][0]", "13");
    const external = await ctx.transaction({
      schema_version: 1,
      op: "edit_inline_expr",
      revision: doc.revision,
      inline_expr_id: lookup.inline_expr_id,
      new_expr: '["first": 11, "second": 8]'
    });
    if (!external.ok) throw new Error(`stale setup transaction failed: ${JSON.stringify(external.json)}`);
    const beforeStaleApply = await ctx.source();
    await clickElement(ctx, `document.getElementById("apply-variable-details")`, "stale nested refusal");
    await ctx.waitFor(async () => {
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      return result && result.ok === false;
    }, "stale nested refusal");
    const staleState = await ctx.driver.evaluate("window.__jetCanvasDetailsState || null");
    if (!staleState || staleState.phase !== "refused" || staleState.event !== "transaction-refused") {
      throw new Error(`stale nested refusal was not visible: ${JSON.stringify(staleState)}`);
    }
    if (await ctx.source() !== beforeStaleApply || !(await ctx.source()).includes('lookup :: ["first": 11, "second": 8]')) {
      throw new Error("stale nested edit changed source beyond the external committed edit");
    }
  },

  "traits-panel-authoring": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    await ctx.replaceSource(`trait Drawable {
    fn render(self) String -[IO]>
    fn label(self) String -> {
        return "drawable"
    }
}

struct Badge {
    text: String
}

fn run() {
    print("ready")
}
    `);
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open Canvas traits panel");
    await ctx.waitFor(async () => {
      const panel = await ctx.driver.evaluate(`document.querySelector("[data-canvas-traits]")?.textContent || ""`);
      const state = await ctx.driver.evaluate("window.__jetCanvasTraitsPanel || null");
      return panel.includes("Drawable") && state && state.traitCount === 1 && state.implementationCount === 0 && state.requiredMethodCount === 1;
    }, "traits panel projection");
    const initial = await ctx.driver.evaluate("window.__jetCanvasTraitsPanel");
    if (!initial.traits[0].requiredMethods.includes("render") || !initial.traits[0].methods.includes("label")) {
      throw new Error(`traits panel did not mark render required: ${JSON.stringify(initial)}`);
    }
    await clickElement(ctx, `document.querySelector('[data-trait-jump]')`, "trait source jump");
    await ctx.waitFor(async () => String(await ctx.driver.evaluate("location.hash")).startsWith("#span-"), "trait source navigation");

    await ctx.driver.evaluate(`window.__jetCanvasTest.openGraphActionPalette("Drawable.render")`);
    await ctx.expectMenu("Drawable.render");
    const menuAction = await ctx.driver.evaluate(`(() => Array.from(document.querySelectorAll("#context-menu [data-menu-action]"))
      .some((button) => button.textContent.includes("Drawable.render")))()`);
    if (!menuAction) throw new Error("trait method was not offered in the Canvas action palette");
    await ctx.driver.press("Escape");
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "reopen Canvas traits panel");

    const beforeInvalid = await ctx.source();
    await clickElement(ctx, `document.querySelector('[data-trait-create="0"]')`, "trait implementation button");
    await expectVisibleRefusal(ctx, "Jet name", "invalid trait type");
    if (await ctx.source() !== beforeInvalid) throw new Error("invalid trait type changed source");

    await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-trait-type="0"]');
      input.value = "Badge";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    })()`);
    await clickElement(ctx, `document.querySelector('[data-trait-create="0"]')`, "create trait implementation");
    await ctx.waitFor(async () => (await ctx.source()).includes("impl Badge.Drawable"), "trait implementation source");
    await ctx.waitFor(async () => {
      const state = await ctx.driver.evaluate("window.__jetCanvasTraitsPanel || null");
      return state && state.implementationCount === 1 && state.implementedMethodCount === 1;
    }, "traits panel implementation projection");
    await ctx.expectSourceContains("fn render(self) String -[IO]>");
    await ctx.expectSourceContains('return "canvas"');
    const created = await ctx.source();
    const createdState = await ctx.driver.evaluate("window.__jetCanvasTraitsPanel");
    if (createdState.implementationCount !== 1 || createdState.implementedMethodCount !== 1) {
      throw new Error(`traits panel did not reflect implementation: ${JSON.stringify(createdState)}`);
    }

    await ctx.undo();
    if ((await ctx.source()).includes("impl Badge.Drawable")) throw new Error("trait implementation undo did not remove impl");
    const undoneState = await ctx.driver.evaluate("window.__jetCanvasTraitsPanel");
    if (undoneState.implementationCount !== 0) throw new Error(`traits panel undo state is stale: ${JSON.stringify(undoneState)}`);
    await ctx.redo();
    if (await ctx.source() !== created) throw new Error("trait implementation redo did not restore exact source");

    const stale = await ctx.transaction({
      schema_version: 1,
      op: "create_trait_impl",
      revision: "sha256-stale",
      type_name: "Badge",
      trait_name: "Drawable"
    });
    const staleMessage = String(stale.json && stale.json.message || "").toLowerCase();
    if (stale.ok || !(staleMessage.includes("revision") || staleMessage.includes("source changed"))) {
      throw new Error(`stale trait edit was not refused: ${JSON.stringify(stale)}`);
    }
    if (await ctx.source() !== created) throw new Error("stale trait edit changed source");

    await ctx.openCanvas();
    await ctx.waitFor(async () => {
      const state = await ctx.driver.evaluate("window.__jetCanvasTraitsPanel || null");
      return state && state.implementationCount === 1 && state.implementedMethodCount === 1;
    }, "traits panel reload");
    if (await ctx.source() !== created) throw new Error("trait implementation reload changed source");
  },

  "canvas-rad-callback-creation": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open callback creation rail");
    await ctx.waitFor(async () => !!(await ctx.driver.evaluate(`document.getElementById("canvas-new-callback")`)), "callback creation affordance");
    await ctx.driver.evaluate(`(() => {
      window.__canvasCallbackSourceTx = [];
      window.__canvasCallbackRealFetch = window.fetch;
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input && input.url || "";
        if (String(url).endsWith("/canvas/transaction") && init && typeof init.body === "string") {
          window.__canvasCallbackSourceTx.push(JSON.parse(init.body));
        }
        return window.__canvasCallbackRealFetch(input, init);
      };
    })()`);
    const created = await createCallbackThroughRail(ctx, "on_clicked");
    await ctx.driver.evaluate(`(() => { window.fetch = window.__canvasCallbackRealFetch; delete window.__canvasCallbackRealFetch; })()`);
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "on_clicked", "new callback graph navigation");
    const requests = await ctx.driver.evaluate("window.__canvasCallbackSourceTx || []");
    if (requests.length !== 1 || requests[0].op !== "create_function" || requests[0].name !== "on_clicked" || requests[0].ret_type !== "Void") {
      throw new Error(`callback creation bypassed the checked source transaction: ${JSON.stringify(requests)}`);
    }
    const graph = await ctx.graph();
    const callback = graphByTitle(graph, "on_clicked");
    const view = (callback.event_views || []).find((event) => event.kind === "callback_event");
    if (!view || view.function !== "on_clicked" || view.title !== "clicked" || view.semantics !== "ordinary_jet_function" || view.dispatch !== "framework_callback") {
      throw new Error(`new callback lost source-backed event view: ${JSON.stringify(view)}`);
    }
    const marker = await ctx.driver.evaluate(`(() => {
      const item = document.querySelector('[data-callback-handler="on_clicked"]');
      return item && {
        title: item.title,
        sourceBacked: item.dataset.canvasSourceBacked,
        sourceId: item.dataset.canvasSourceId,
        revision: item.dataset.canvasRevision,
        text: item.textContent
      };
    })()`);
    if (!marker || !marker.title.includes("on_clicked") || marker.sourceBacked !== "true" || marker.sourceId !== graph.source_id || marker.revision !== graph.revision || !marker.text.includes("handler")) {
      throw new Error(`callback graph rail lost provenance or handler label: ${JSON.stringify(marker)}`);
    }
    if (created.after !== graph.source_text || await ctx.source() !== created.after) {
      throw new Error("callback creation source and projected source diverged");
    }
    await assertCleanSourceSync(ctx, ["callback creation pointer gesture"]);
  },

  "canvas-rad-callback-undo": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open callback undo rail");
    const created = await createCallbackThroughRail(ctx, "on_undo");
    const beforeUndo = (await ctx.state()).undoDepth;
    if (beforeUndo < 1) throw new Error("callback creation did not record undo history");
    await clickElement(ctx, `document.getElementById("undo-edit")`, "undo callback creation");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return await ctx.source() === created.before && state.undoDepth === beforeUndo - 1 && state.redoDepth === 1;
    }, "callback undo restoration");
    if ((await ctx.graph()).graphs.some((graph) => graph.title === "on_undo")) throw new Error("callback undo left a projected handler graph");
    await assertCleanSourceSync(ctx, ["callback creation", "callback undo pointer gesture"]);
  },

  "canvas-rad-callback-redo": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open callback redo rail");
    const created = await createCallbackThroughRail(ctx, "on_redo");
    await clickElement(ctx, `document.getElementById("undo-edit")`, "undo before callback redo");
    await ctx.waitFor(async () => (await ctx.state()).redoDepth === 1, "callback redo history");
    await clickElement(ctx, `document.getElementById("redo-edit")`, "redo callback creation");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return await ctx.source() === created.after && state.redoDepth === 0;
    }, "callback redo projection");
    await ctx.waitFor(async () => !!(await ctx.driver.evaluate(`document.querySelector('[data-callback-handler="on_redo"]')`)), "callback redo handler marker");
    await clickElement(ctx, `document.querySelector('[data-callback-handler="on_redo"]')`, "navigate restored callback handler");
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "on_redo", "restored callback handler navigation");
    const graph = graphByTitle(await ctx.graph(), "on_redo");
    if (!(graph.event_views || []).some((event) => event.function === "on_redo")) throw new Error("callback redo lost handler event view");
    await assertCleanSourceSync(ctx, ["callback creation", "callback undo", "callback redo pointer gestures"]);
  },

  "canvas-rad-callback-escape": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open callback Escape rail");
    const before = await ctx.source();
    await ctx.driver.evaluate("window.prompt = () => null");
    await clickElement(ctx, `document.getElementById("canvas-new-callback")`, "cancel callback creation");
    await ctx.driver.press("Escape");
    await ctx.waitFor(async () => {
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      return result && result.code === "client_cancelled" && result.changed === false;
    }, "callback Escape cancellation");
    const state = await ctx.state();
    if (await ctx.source() !== before || state.undoDepth !== 0) {
      throw new Error(`callback Escape did not restore source/history: ${JSON.stringify({ state })}`);
    }
    await assertCleanSourceSync(ctx, ["callback creation Escape restoration"]);
  },

  "canvas-rad-callback-focus": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open callback keyboard rail");
    await ctx.driver.evaluate(`window.prompt = () => "on_keyboard"`);
    await pressAttribute(ctx, "id", "canvas-new-callback", "callback creation keyboard focus");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state.graphTitle === "on_keyboard" && (await ctx.source()).includes("fn on_keyboard()");
    }, "keyboard callback creation");
    const focus = await ctx.driver.evaluate("document.activeElement && document.activeElement.id");
    if (focus !== "canvas-new-callback") throw new Error(`callback keyboard action lost focus: ${focus}`);
    await assertCleanSourceSync(ctx, ["callback creation keyboard gesture"]);
  },

  "canvas-rad-callback-fresh-projection": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open fresh callback rail");
    const created = await createCallbackThroughRail(ctx, "on_reload");
    await ctx.openCanvas();
    if (await ctx.source() !== created.after) throw new Error("callback fresh projection changed saved source");
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "reopen fresh callback rail");
    await ctx.waitFor(async () => !!(await ctx.driver.evaluate(`document.querySelector('[data-callback-handler="on_reload"]')`)), "fresh callback handler marker");
    await clickElement(ctx, `document.querySelector('[data-callback-handler="on_reload"]')`, "navigate fresh callback handler");
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "on_reload", "fresh callback graph navigation");
    const graph = graphByTitle(await ctx.graph(), "on_reload");
    if (!(graph.event_views || []).some((event) => event.function === "on_reload" && event.source_span)) {
      throw new Error("fresh callback projection lost source span or handler view");
    }
    await assertCleanSourceSync(ctx, ["callback creation", "callback fresh projection pointer gesture"]);
  },

  "canvas-rad-callback-failure": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open callback failure rail");
    const initialSource = await ctx.source();
    await ctx.driver.evaluate(`window.prompt = () => "bad callback"`);
    await clickElement(ctx, `document.getElementById("canvas-new-callback")`, "refuse invalid callback name");
    await expectVisibleRefusal(ctx, "Callback name must start with on_", "invalid callback refusal");
    const invalid = await ctx.driver.evaluate(`({ result: window.__jetCanvasLastTxResult || null, focus: document.activeElement && document.activeElement.id })`);
    if (!invalid.result || invalid.result.code !== "client_callback_gate" || invalid.result.changed !== false || invalid.focus !== "canvas-new-callback" || await ctx.source() !== initialSource) {
      throw new Error(`invalid callback was not refused before sema: ${JSON.stringify(invalid)}`);
    }
    const created = await createCallbackThroughRail(ctx, "on_saved");
    const sourceBeforeFailure = await ctx.source();
    const historyBeforeFailure = (await ctx.state()).undoDepth;
    await ctx.driver.evaluate(`(() => {
      window.__canvasCallbackRealFetch = window.fetch;
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input && input.url || "";
        if (String(url).endsWith("/canvas/transaction")) return Promise.reject(new Error("forced callback save failure"));
        return window.__canvasCallbackRealFetch(input, init);
      };
    })()`);
    await ctx.driver.evaluate(`window.prompt = () => "on_failed"`);
    await clickElement(ctx, `document.getElementById("canvas-new-callback")`, "fail callback save");
    await expectVisibleRefusal(ctx, "forced callback save failure", "failed callback save refusal");
    await ctx.waitFor(async () => (await ctx.driver.evaluate("window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind")) === "error", "failed callback save state");
    const failedState = await ctx.state();
    if (await ctx.source() !== sourceBeforeFailure || failedState.undoDepth !== historyBeforeFailure) {
      throw new Error(`failed callback save changed source or undo history: ${JSON.stringify({ failedState, source: await ctx.source() })}`);
    }
    await ctx.driver.evaluate(`(() => { window.fetch = window.__canvasCallbackRealFetch; delete window.__canvasCallbackRealFetch; })()`);

    const staleSource = await ctx.source();
    const staleRevision = (await ctx.graph()).revision;
    const external = await ctx.transaction({
      schema_version: 1,
      op: "replace_source",
      revision: staleRevision,
      source: staleSource + "\n// external callback edit\n",
      source_edit: "stale_callback_setup"
    });
    if (!external.ok) throw new Error(`stale callback setup failed: ${JSON.stringify(external)}`);
    const externalSource = external.json.source_text || staleSource + "\n// external callback edit\n";
    await ctx.driver.evaluate(`window.prompt = () => "on_stale"`);
    await clickElement(ctx, `document.getElementById("canvas-new-callback")`, "reject stale callback creation");
    await ctx.waitFor(async () => {
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      return result && result.kind === "conflict" && String(result.message || "").length > 0;
    }, "stale callback refusal");
    const stale = await ctx.driver.evaluate(`({ result: window.__jetCanvasLastTxResult || null, toast: document.getElementById("toast")?.textContent || "" })`);
    if (!stale.toast.includes(stale.result.message) || await ctx.source() !== externalSource || (await ctx.state()).undoDepth !== historyBeforeFailure) {
      throw new Error(`stale callback edit did not preserve source/history: ${JSON.stringify({ stale, source: await ctx.source(), state: await ctx.state() })}`);
    }
    if (created.after !== sourceBeforeFailure) throw new Error("callback failure setup lost the successful source revision");
  },

  "canvas-rad-handler-navigation": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open handler navigation rail");
    await ctx.waitFor(async () => !!(await ctx.driver.evaluate(`document.querySelector('[data-callback-handler="on_start"]')`)), "callback handler rail item");
    const before = await ctx.source();
    await clickElement(ctx, `Array.from(document.querySelectorAll('[data-sidebar-graph]')).find((button) => button.textContent.includes("run"))`, "return to caller graph");
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "run", "caller graph");
    await clickElement(ctx, `document.querySelector('[data-callback-handler="on_start"]')`, "open callback handler from function rail");
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "on_start", "callback handler graph navigation");
    const graph = graphByTitle(await ctx.graph(), "on_start");
    const view = (graph.event_views || []).find((event) => event.function === "on_start");
    if (!view || !view.source_span || view.dispatch !== "framework_callback") throw new Error(`handler navigation lost callback provenance: ${JSON.stringify(view)}`);
    const tab = await ctx.driver.evaluate(`(() => {
      const item = document.querySelector('[data-graph-tab][data-callback-handler="on_start"]');
      return item && { kind: item.querySelector(".graph-tab-kind")?.textContent || "", title: item.title };
    })()`);
    if (!tab || tab.kind !== "callback" || !tab.title.includes("on_start")) throw new Error(`callback graph tab lost handler identity: ${JSON.stringify(tab)}`);
    await clickElement(ctx, `document.getElementById("dock-details")`, "open handler inspector");
    await ctx.waitFor(async () => !!(await ctx.driver.evaluate(`document.querySelector('[data-event-handler="on_start"]')`)), "handler inspector navigation action");
    await clickElement(ctx, `document.querySelector('[data-event-handler="on_start"]')`, "open handler from inspector");
    await ctx.waitFor(async () => (await ctx.state()).graphTitle === "on_start", "inspector handler navigation");
    if (await ctx.source() !== before) throw new Error("handler navigation changed Jet source");
    await assertCleanSourceSync(ctx, ["callback handler rail navigation", "callback handler inspector navigation"]);
  },

  "events-panel-authoring": async (ctx) => {
    await ctx.openCanvas();
    await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    await clickElement(ctx, `document.getElementById("dock-graphs")`, "open project files");
    await ctx.waitFor(async () => !!(await ctx.driver.evaluate(`document.querySelector('[data-project-file="events.jet"]')`)), "events source file");
    await clickElement(ctx, `document.querySelector('[data-project-file="events.jet"]')`, "open events source");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.doc && state.doc.source_id === "events.jet" && state.graphTitle === "dev";
    }, "events source graph");
    const originalSource = await ctx.source();
    const graphsDrawerOpen = await ctx.driver.evaluate(`document.getElementById("dock-graphs")?.classList.contains("is-active")`);
    if (!graphsDrawerOpen) await clickElement(ctx, `document.getElementById("dock-graphs")`, "open Canvas events panel");
    await ctx.waitFor(async () => {
      const panel = await ctx.driver.evaluate(`document.querySelector("[data-canvas-events]")?.textContent || ""`);
      const state = await ctx.driver.evaluate("window.__jetCanvasEventsPanel || null");
      return panel.includes("Events") && panel.includes("Event Stream Create") && state && state.dispatcherCount === 6;
    }, "events panel projection");
    const initial = await ctx.driver.evaluate("window.__jetCanvasEventsPanel");
    const projectedGraph = await ctx.graph();
    const projectedDispatchers = projectedGraph.facts?.blueprint?.event_dispatchers || [];
    if (initial.events.length !== projectedDispatchers.length || initial.events.some((event, index) => {
      const fact = projectedDispatchers[index];
      return !fact
        || event.kind !== fact.kind
        || event.source !== fact.source
        || event.receiver !== (fact.receiver || "")
        || event.receiverType !== (fact.receiver_type || "")
        || event.scope !== (fact.scope || "")
        || event.factSource !== "semindex_checked_call"
        || event.sourceSpan?.start !== fact.source_span?.start
        || event.sourceSpan?.end !== fact.source_span?.end;
    })) {
      throw new Error(`events panel did not render the fresh checked production projection: ${JSON.stringify({ panel: initial, projectedDispatchers })}`);
    }
    const savedOriginal = await ctx.source();
    if (!initial.events.some((event) => event.receiverType === "Event<Int>" && event.scope === "scope")) {
      throw new Error(`events panel lost checked type or scope provenance: ${JSON.stringify(initial)}`);
    }
    await clickElement(ctx, `document.querySelector('[data-event-jump]')`, "event source jump");
    await ctx.waitFor(async () => String(await ctx.driver.evaluate("location.hash")).startsWith("#span-"), "event source navigation");

    await clickElement(ctx, `document.querySelector('[data-event-actions]')`, "open event panel actions");
    await ctx.expectMenu("core.event");
    const coreEventAction = await ctx.driver.evaluate(`(() => Array.from(document.querySelectorAll("#context-menu [data-menu-action]")).some((button) => button.textContent.includes("new") || button.textContent.includes("scope")))()`);
    if (!coreEventAction) throw new Error("core.event creation actions were not offered");
    const stagedBefore = (await ctx.state()).stagedRegistry?.length || 0;
    await clickElement(ctx, `Array.from(document.querySelectorAll("#context-menu [data-menu-action]")).find((button) => button.textContent.includes("scope"))`, "create event scope from panel");
    await ctx.waitFor(async () => {
      const state = await ctx.driver.evaluate("window.__jetCanvasEventsPanel || null");
      return (await ctx.state()).stagedRegistry?.length === stagedBefore + 1
        && state && state.dispatcherCount === 6;
    }, "event panel action staging");
    if (await ctx.source() !== savedOriginal) throw new Error("event panel action staging changed Jet source");
    const stagedEventAction = (await ctx.state()).stagedRegistry?.find((node) => String(node.title || "").includes("scope"));
    const stagedEventOutput = stagedEventAction?.pins?.find((pin) => pin.direction === "output");
    if (!stagedEventAction || stagedEventAction.kind !== "function" || !stagedEventOutput || stagedEventOutput.type !== "EventScope") {
      throw new Error(`event panel action did not preserve the checked typed node: ${JSON.stringify(stagedEventAction)}`);
    }
    const stagedToast = await ctx.driver.evaluate("document.getElementById('toast')?.textContent || ''");
    if (!stagedToast.includes("Node staged") || !stagedToast.includes("save source")) {
      throw new Error(`event panel action lacked recoverable staging guidance: ${stagedToast}`);
    }
    await ctx.driver.press("Delete");
    await ctx.waitFor(async () => (await ctx.state()).stagedRegistry?.length === stagedBefore, "delete staged event action");
    if (await ctx.source() !== savedOriginal) throw new Error("event panel staged-action cleanup changed Jet source");

    await ctx.switchGraph("dev");
    let selectedEventExpr = null;
    const selectEventPayload = async (source, label) => {
      const selected = await selectInlineExpression(ctx, "dev", (expr) => String(expr.source || "").trim() === source, label);
      selectedEventExpr = selected.expr;
      await ctx.waitFor(async () => !!(await ctx.driver.evaluate(`document.querySelector('[data-details-input="inline-value"][data-inline-id="${selectedEventExpr.inline_expr_id}"]')`)), `${label} editor`);
    };
    const setEventPayload = async (value, label) => {
      const changed = await ctx.driver.evaluate(`(() => {
        const input = document.querySelector('[data-details-input="inline-value"][data-inline-id="${selectedEventExpr.inline_expr_id}"]');
        const apply = document.querySelector('[data-inline-apply="${selectedEventExpr.inline_expr_id}"]');
        if (!input || !apply) return { ok: false };
        input.focus();
        input.value = ${JSON.stringify(value)};
        input.dispatchEvent(new Event("input", { bubbles: true }));
        return { ok: true, kind: input.dataset.detailKind, type: input.dataset.detailType };
      })()`);
      if (!changed.ok) throw new Error(`${label} editor missing: ${JSON.stringify(changed)}`);
      await ctx.driver.press("Enter");
      await ctx.waitFor(async () => {
        const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
        return result && result.changed === true && result.source_text === await ctx.source();
      }, `${label} transaction`);
      await ctx.waitForCanvas();
    };

    const beforePayload = await ctx.source();
    await selectEventPayload("1", "event payload inspection");
    await setEventPayload("2", "event payload edit");
    const afterPayload = await ctx.source();
    if (!afterPayload.includes("clicked.emit(2)")) throw new Error(`event payload edit did not preserve source meaning: ${afterPayload}`);
    const editedPayloadPanel = await ctx.driver.evaluate("window.__jetCanvasEventsPanel || null");
    if (!editedPayloadPanel.events.some((event) => event.receiverType === "Event<Int>" && event.scope === "scope")) {
      throw new Error(`event payload edit lost event type or scope provenance: ${JSON.stringify(editedPayloadPanel)}`);
    }
    const undonePayload = await ctx.undo();
    if (undonePayload !== beforePayload) throw new Error("event payload undo did not restore exact source");
    const redonePayload = await ctx.redo();
    if (redonePayload !== afterPayload) throw new Error("event payload redo did not restore exact source");

    await selectEventPayload("2", "ill-typed event payload");
    await ctx.driver.evaluate(`(() => {
      window.__canvasEventTransactionPosts = 0;
      window.__canvasEventFetch = window.fetch;
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input.url;
        if (String(url).includes("/canvas/transaction")) window.__canvasEventTransactionPosts++;
        return window.__canvasEventFetch(input, init);
      };
    })()`);
    const invalidInput = await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="inline-value"][data-inline-id="${selectedEventExpr.inline_expr_id}"]');
      if (!input) return { ok: false };
      input.focus();
      input.value = "";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      return { ok: true, value: input.value, type: input.dataset.detailType };
    })()`);
    if (!invalidInput.ok) throw new Error("incomplete event payload editor missing");
    await ctx.driver.press("Enter");
    await expectVisibleRefusal(ctx, "Expression is required", "incomplete event payload refusal");
    const invalidResult = await ctx.driver.evaluate(`({
      result: window.__jetCanvasLastTxResult || null,
      transactionPosts: window.__canvasEventTransactionPosts || 0
    })`);
    if (!invalidResult.result || invalidResult.result.code !== "client_type_gate" || invalidResult.result.ok !== false || invalidResult.transactionPosts !== 0) {
      throw new Error(`incomplete event payload was not refused before sema: ${JSON.stringify(invalidResult)}`);
    }
    if (await ctx.source() !== afterPayload) throw new Error("ill-typed event payload changed source");

    await selectEventPayload("n -> print", "ambiguous event value");
    const ambiguousInput = await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="inline-value"][data-inline-id="${selectedEventExpr.inline_expr_id}"]');
      if (!input) return { ok: false };
      input.focus();
      return { ok: true, type: input.type };
    })()`);
    if (!ambiguousInput.ok || ambiguousInput.type !== "text") throw new Error(`ambiguous event payload editor missing or not textual: ${JSON.stringify(ambiguousInput)}`);
    await ctx.driver.evaluate(`(() => {
      const input = document.querySelector('[data-details-input="inline-value"][data-inline-id="${selectedEventExpr.inline_expr_id}"]');
      input.value = "missing_event_scope";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    })()`);
    await ctx.driver.press("Enter");
    await expectVisibleRefusal(ctx, "Unknown value missing_event_scope", "ambiguous event value refusal");
    const ambiguousResult = await ctx.driver.evaluate(`({
      result: window.__jetCanvasLastTxResult || null,
      transactionPosts: window.__canvasEventTransactionPosts || 0
    })`);
    if (!ambiguousResult.result || ambiguousResult.result.code !== "client_type_gate"
      || ambiguousResult.result.ok !== false || ambiguousResult.transactionPosts !== 0) {
      throw new Error(`ambiguous event payload was not refused before sema: ${JSON.stringify(ambiguousResult)}`);
    }
    if (await ctx.source() !== afterPayload) throw new Error("ambiguous event payload changed source");
    await ctx.driver.evaluate(`(() => { window.fetch = window.__canvasEventFetch; delete window.__canvasEventFetch; })()`);

    const changedSource = originalSource
      .replace("event.new<Int>()", "event.new<String>()")
      .replace("clicked.emit(1)", 'clicked.emit("one")');
    const selectSourceText = async () => {
      const session = ctx.driver.pageSession;
      await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Control", code: "ControlLeft", modifiers: 2, windowsVirtualKeyCode: 17, nativeVirtualKeyCode: 17 }, session);
      await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyDown", key: "A", code: "KeyA", modifiers: 2, windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65 }, session);
      await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyUp", key: "A", code: "KeyA", modifiers: 2, windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65 }, session);
      await ctx.driver.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Control", code: "ControlLeft", modifiers: 0, windowsVirtualKeyCode: 17, nativeVirtualKeyCode: 17 }, session);
    };
    const applySourceByGesture = async (source, label) => {
      const before = await ctx.source();
      await ctx.driver.evaluate(`(() => {
        const drawer = document.getElementById("right-drawer");
        drawer?.removeAttribute("style");
        drawer?.classList.remove("is-drawer-open");
        document.getElementById("dock-details")?.classList.remove("is-active");
      })()`);
      const toolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
      if (!toolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, `${label} tools`);
      await clickElement(ctx, `document.getElementById("edit-source")`, `${label} open editor`);
      await clickElement(ctx, `document.getElementById("source-editor")`, `${label} focus editor`);
      await selectSourceText();
      await ctx.driver.send("Input.insertText", { text: source }, ctx.driver.pageSession);
      await clickElement(ctx, `document.getElementById("apply-source-edit")`, `${label} apply`);
      await ctx.waitFor(async () => {
        const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
        return result && result.changed === true && result.source_text === await ctx.source() && result.source_text !== before;
      }, `${label} transaction`);
      await ctx.waitForCanvas();
      return await ctx.source();
    };
    const edited = await applySourceByGesture(changedSource, "event source edit");
    await ctx.waitFor(async () => {
      const state = await ctx.driver.evaluate("window.__jetCanvasEventsPanel || null");
      return state && state.events.some((event) => event.receiverType === "Event<String>") && state.revision === (await ctx.graph()).revision;
    }, "events panel edit projection");
    if (!edited.includes("event.new<String>()") || !edited.includes('clicked.emit("one")')) {
      throw new Error(`event edit did not preserve canonical source meaning: ${edited}`);
    }
    const editedPanel = await ctx.driver.evaluate("window.__jetCanvasEventsPanel || null");
    if (!editedPanel.events.some((event) => event.receiverType === "Event<String>" && event.scope === "scope" && event.sourceSpan && event.factSource)) {
      throw new Error(`event source edit lost checked provenance: ${JSON.stringify(editedPanel)}`);
    }

    await ctx.undo();
    if (await ctx.source() !== afterPayload) {
      throw new Error("event panel undo did not restore exact source");
    }
    const undone = await ctx.driver.evaluate("window.__jetCanvasEventsPanel");
    if (!undone.events.some((event) => event.receiverType === "Event<Int>")) {
      throw new Error(`event panel undo projection is stale: ${JSON.stringify(undone)}`);
    }
    await ctx.redo();
    if (await ctx.source() !== edited) throw new Error("event panel redo did not restore exact source");

    const staleSource = await ctx.source();
    const staleRevision = (await ctx.graph()).revision;
    await ctx.driver.evaluate(`(() => {
      const drawer = document.getElementById("right-drawer");
      drawer?.removeAttribute("style");
      drawer?.classList.remove("is-drawer-open");
      document.getElementById("dock-details")?.classList.remove("is-active");
    })()`);
    const toolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!toolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "stale source tools");
    await clickElement(ctx, `document.getElementById("edit-source")`, "open stale source editor");
    await clickElement(ctx, `document.getElementById("source-editor")`, "focus stale source editor");
    await selectSourceText();
    await ctx.driver.send("Input.insertText", { text: staleSource }, ctx.driver.pageSession);
    const externalSource = staleSource.replace('clicked.emit("one")', 'clicked.emit("external")');
    const external = await ctx.transaction({
      schema_version: 1,
      op: "replace_source",
      revision: staleRevision,
      source: externalSource,
      source_edit: "stale_setup"
    });
    if (!external.ok) throw new Error(`stale setup transaction failed: ${JSON.stringify(external)}`);
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "reject stale event source");
    await ctx.waitFor(async () => {
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      return result && result.kind === "conflict" && String(result.message || "").length > 0;
    }, "stale event source refusal");
    const staleResult = await ctx.driver.evaluate(`({
      result: window.__jetCanvasLastTxResult || null,
      toast: document.getElementById("toast")?.textContent || ""
    })`);
    if (!staleResult.result.message || !staleResult.toast.includes(staleResult.result.message)) {
      throw new Error(`stale event source refusal was not visible: ${JSON.stringify(staleResult)}`);
    }
    if (await ctx.source() !== externalSource) throw new Error("stale event source overwrote the external edit");

    await ctx.openCanvas();
    const filesAlreadyVisible = await ctx.driver.evaluate(`!!document.querySelector('[data-project-file="events.jet"]')`);
    if (!filesAlreadyVisible) await clickElement(ctx, `document.getElementById("dock-graphs")`, "reopen event source files");
    await ctx.waitFor(async () => !!(await ctx.driver.evaluate(`document.querySelector('[data-project-file="events.jet"]')`)), "event source file after reload");
    await clickElement(ctx, `document.querySelector('[data-project-file="events.jet"]')`, "reopen events source");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.doc && state.doc.source_id === "events.jet" && state.graphTitle === "dev";
    }, "events source reload");
    await ctx.waitFor(async () => {
      const state = await ctx.driver.evaluate("window.__jetCanvasEventsPanel || null");
      return state && state.events.some((event) => event.receiverType === "Event<String>");
    }, "events panel reload");
    if (await ctx.source() !== externalSource) throw new Error("event panel reload changed source");
  },

  "fallible-context": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    // The structural row ships as "Failure rail" (query_actions.rs
    // `canvas_structural_action_jsons`); it was renamed from "Fallible" when the
    // declared error rail was unified. The palette head echoes the query back
    // into `<input value="…">`, so `expectMenu` matches that echo even when no
    // row is rendered yet — wait for the row itself, which is what we assert on.
    await ctx.driver.evaluate(`window.__jetCanvasTest.openGraphActionPalette("Failure rail")`);
    const readFallibleRow = () => ctx.driver.evaluate(`(() => {
      const menu = document.getElementById("context-menu");
      if (!menu || !menu.classList.contains("is-open")) return null;
      const button = Array.from(menu.querySelectorAll("[data-menu-action]")).find((b) => b.textContent.includes("Failure rail"));
      if (!button) return null;
      return { available: button.dataset.available, code: button.dataset.unavailableReasonCode, title: button.getAttribute("title"), text: button.textContent };
    })()`);
    await ctx.waitFor(async () => !!(await readFallibleRow()), "Failure rail palette row");
    const row = await readFallibleRow();
    if (!row || row.available !== "false" || row.code !== "needs_fallible_function" || !row.title.includes("fallible function")) {
      throw new Error(`fallible row not excluded with reason: ${JSON.stringify(row)}`);
    }
    const before = await ctx.source();
    const graph = await ctx.graph();
    const scratch = (graph.graphs || []).find((g) => g.title === "scratch");
    const result = await ctx.transaction({ schema_version: 1, op: "insert_fallible_rail", revision: graph.revision, graph_id: scratch.graph_id });
    if (result.ok || !String(result.json && result.json.message || "").includes("needs a fallible function")) {
      throw new Error(`fallible insert should reject before sema: ${JSON.stringify(result)}`);
    }
    const after = await ctx.source();
    if (after !== before) throw new Error("rejected fallible insert changed source");
  },

  "excluded-entry-rendering": async (ctx) => {
    await ctx.openCanvas();
    await ctx.loadCoreCatalog("help");
    await ctx.openCoreCatalogPalette("help");
    await ctx.expectMenu("help");
    const row = await ctx.driver.evaluate(`(() => {
      const buttons = Array.from(document.querySelectorAll("#context-menu [data-menu-action]"));
      const button = buttons.find((b) => b.textContent.includes("help"));
      if (!button) return null;
      return { available: button.dataset.available, code: button.dataset.unavailableReasonCode, disabled: button.disabled, className: button.className, title: button.getAttribute("title"), text: button.textContent };
    })()`);
    if (!row || row.available !== "true" || row.disabled || row.className.includes("is-disabled") || row.code !== "method_only") {
      throw new Error(`stageable help row is not active: ${JSON.stringify(row)}`);
    }
    const before = await ctx.source();
    await ctx.pickEntry("help");
    await ctx.waitFor(async () => (await ctx.state()).stagedRegistry.some((node) => String(node.title || "").includes("help")), "staged help");
    if (await ctx.source() !== before) throw new Error("staged help changed source");
    const staged = (await ctx.state()).stagedRegistry.find((node) => String(node.title || "").includes("help"));
    const receiver = (staged && staged.pins || []).find((pin) => pin.name === "receiver");
    if (!receiver || receiver.type !== "ArgsSpec") throw new Error(`staged help receiver missing: ${JSON.stringify(staged)}`);
  },

  "no-dead-end-ad-hoc-insert": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    await ctx.driver.evaluate(`window.__jetCanvasTest.openPinMenu("scratch", "then")`);
    await ctx.expectMenu("Search actions");
    const menu = await ctx.driver.evaluate(`document.getElementById("context-menu").textContent`);
    if (/\bCall\b/.test(menu) || menu.includes("Insert call transaction")) {
      throw new Error(`ad hoc Call insert should not be offered:\n${menu}`);
    }
  },

  "failed-insert-shows-panel": async (ctx) => {
    const result = await failScratchLimit(ctx);
    const rendered = String(result.json && result.json.message || "");
    if (!rendered.includes("Error [E0107]") || !rendered.includes("Why:") || !rendered.includes("Fix:")) {
      throw new Error(`transaction did not return full Jet diagnostic: ${JSON.stringify(result.json)}`);
    }
    const before = await ctx.problems();
    if (!before.length || !String(before[0].rendered || "").includes("Error [E0107]")) {
      throw new Error(`problem panel missing rendered E-code: ${JSON.stringify(before)}`);
    }
    await sleep(5200);
    const after = await ctx.problems();
    if (!after.length || !String(after[0].rendered || "").includes("Error [E0107]")) {
      throw new Error(`problem panel did not persist past toast: ${JSON.stringify(after)}`);
    }
  },

  "check-button-populates-panel": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    const source = await ctx.source();
    await ctx.setSourceEditor(source.replace("print(limit)", "print(missing_value)"));
    await ctx.checkCurrentSource();
    const problem = await ctx.expectProblem("E0107");
    if (!String(problem.rendered || "").includes("Why:") || !String(problem.rendered || "").includes("Fix:")) {
      throw new Error(`check diagnostic missing full text: ${JSON.stringify(problem)}`);
    }
    const diagnosticSurface = await ctx.driver.evaluate(`(() => {
      const root = document.getElementById("problems-list");
      const detail = root && root.querySelector(".problem-detail");
      return {
        detailTag: detail && detail.tagName,
        detailText: detail && detail.textContent,
        handlers: root && root.querySelectorAll("[onclick], [onchange], [onerror], [onload]").length,
        markupNodes: root && root.querySelectorAll("img, script, iframe").length
      };
    })()`);
    if (!diagnosticSurface || diagnosticSurface.detailTag !== "PRE" || !String(diagnosticSurface.detailText || "").includes("\n Why:") || diagnosticSurface.handlers !== 0 || diagnosticSurface.markupNodes !== 0) {
      throw new Error(`diagnostic descriptor lost safe multiline rendering: ${JSON.stringify(diagnosticSurface)}`);
    }
    const ok = await ctx.jumpProblem(0);
    if (!ok) throw new Error("problem jump failed");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.selectedNodeId && (state.diagnosticsByNode || []).some((d) => d.node_id === state.selectedNodeId);
    }, "diagnostic jump selected node");
  },

  "bubble-appears-and-clears": async (ctx) => {
    await failScratchLimit(ctx);
    await ctx.waitFor(async () => {
      const bubbles = await ctx.diagnosticsByNode();
      return bubbles.some((b) => b.severity === "error" && (b.codes || []).includes("E0107"));
    }, "diagnostic bubble");
    const bubbles = await ctx.diagnosticsByNode();
    const first = bubbles.find((b) => (b.codes || []).includes("E0107"));
    if (!first) throw new Error(`E0107 bubble missing: ${JSON.stringify(bubbles)}`);
    const graph = await ctx.graph();
    const before = await ctx.source();
    await ctx.uiTransaction({ schema_version: 1, op: "replace_source", revision: graph.revision, source: before });
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && (!state.problems || state.problems.length === 0) && (!state.diagnosticsByNode || state.diagnosticsByNode.length === 0);
    }, "diagnostic bubble cleared");
  },

  "undo-restores-source": async (ctx) => {
    await ctx.openCanvas();
    const before = await ctx.driver.evaluate(`fetch("/canvas/source", { cache: "no-store" }).then((r) => r.text())`);
    await ctx.loadCoreCatalog();
    await ctx.openPinActionMenu("limit", "limit");
    await ctx.type("abs");
    await ctx.expectMenu("abs");
    await ctx.pickEntry("abs");
    await ctx.expectSourceContains("math.abs");
    await ctx.driver.shortcut(["Control", "z"]);
    // `main.jet` flips the moment the undo transaction commits on the server,
    // but the toast and the `lastToast` snapshot only exist once the client's
    // own response handling redraws the graph (transactions-catalog.js
    // `restoreSource` toasts, then `loadGraph()` draws). Waiting on the restored
    // bytes alone read the previous "Source updated" toast whenever the machine
    // is busy enough to slow that redraw — 6/6 Firefox and 3/3 Chromium
    // failures under a saturated CPU, which is how verify-full runs. Wait for
    // both observables.
    await ctx.waitFor(async () => {
      const source = await ctx.driver.evaluate(`fetch("/canvas/source", { cache: "no-store" }).then((r) => r.text())`);
      if (source !== before) return false;
      const pending = await ctx.driver.evaluate(`window.__jetCanvasTest.lastToast || ""`);
      return pending.includes("Undo: insert abs");
    }, "source restored by undo and named in the toast");
    const toast = await ctx.driver.evaluate(`window.__jetCanvasTest.lastToast || ""`);
    if (!toast.includes("Undo: insert abs")) throw new Error(`undo toast did not name operation: ${toast}`);
  },

  "undo-failure-preserves-history": async (ctx) => {
    await ctx.openCanvas();
    const before = await ctx.source();
    await ctx.loadCoreCatalog();
    await ctx.openPinActionMenu("limit", "limit");
    await ctx.type("abs");
    await ctx.expectMenu("abs");
    await ctx.pickEntry("abs");
    await ctx.waitFor(async () => (await ctx.source()).includes("math.abs"), "source-backed edit before failed undo");
    const changed = await ctx.source();
    const beforeFailure = await ctx.state();
    if (!beforeFailure || beforeFailure.undoDepth < 1) throw new Error(`edit did not create undo history: ${JSON.stringify(beforeFailure)}`);

    await ctx.driver.evaluate(`(() => {
      const realFetch = window.fetch.bind(window);
      window.__jetCanvasRestoreFetch = realFetch;
      window.__jetCanvasFailRestore = true;
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input && input.url || "";
        let body = null;
        try { body = typeof init?.body === "string" ? JSON.parse(init.body) : null; } catch (_) {}
        if (window.__jetCanvasFailRestore && url.endsWith("/canvas/transaction") && body && body.undo_restore) {
          return Promise.reject(new Error("Canvas test restore transport failure"));
        }
        return realFetch(input, init);
      };
    })()`);
    await ctx.driver.shortcut(["Control", "z"]);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      const canvasState = await ctx.driver.evaluate("window.__jetCanvasCanvasState || null");
      return state && state.undoDepth === beforeFailure.undoDepth && state.redoDepth === 0
        && canvasState && canvasState.kind === "error"
        && String(canvasState.detail || "").toLowerCase().includes("undo history");
    }, "failed undo refusal and preserved history");
    if (await ctx.source() !== changed) throw new Error("failed undo changed source bytes");

    await ctx.driver.evaluate(`(() => {
      window.__jetCanvasFailRestore = false;
      window.fetch = window.__jetCanvasRestoreFetch;
    })()`);
    await ctx.driver.shortcut(["Control", "z"]);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return (await ctx.source()) === before && state && state.undoDepth === 0 && state.redoDepth === 1;
    }, "retry undo after restored transport");

    await ctx.driver.shortcut(["Control", "y"]);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return (await ctx.source()) === changed && state && state.undoDepth === 1 && state.redoDepth === 0;
    }, "redo before stale undo");
    const serverBeforeConflict = await ctx.graph();
    const conflictResult = await ctx.transaction({
      schema_version: 1,
      op: "replace_source",
      revision: serverBeforeConflict.revision,
      source: `${changed}\n// external Canvas conflict\n`
    });
    if (!conflictResult.ok) throw new Error(`external conflict setup failed: ${JSON.stringify(conflictResult.json)}`);
    await ctx.driver.shortcut(["Control", "z"]);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      const canvasState = await ctx.driver.evaluate("window.__jetCanvasCanvasState || null");
      return state && state.undoDepth === 1 && state.redoDepth === 0
        && canvasState && canvasState.kind === "stale"
        && String(canvasState.detail || "").toLowerCase().includes("undo history");
    }, "stale undo refusal and preserved history");
    if (await ctx.source() === changed) throw new Error("stale undo unexpectedly changed source bytes");

    const serverAfterConflict = await ctx.graph();
    const restoreConflict = await ctx.transaction({
      schema_version: 1,
      op: "replace_source",
      revision: serverAfterConflict.revision,
      source: changed
    });
    if (!restoreConflict.ok) throw new Error(`conflict cleanup failed: ${JSON.stringify(restoreConflict.json)}`);
    await ctx.driver.shortcut(["Control", "z"]);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return (await ctx.source()) === before && state && state.undoDepth === 0 && state.redoDepth === 1;
    }, "retry stale undo after source restore");
    await assertCleanSourceSync(ctx, ["source-backed edit", "failed undo", "retry undo"]);
  },

  "undo-depth-20-mixed-run": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    const sources = [await ctx.source()];
    for (let i = 0; i < 24; i++) {
      const doc = await ctx.graph();
      const scratch = graphByTitle(doc, "scratch");
      if (i % 3 === 0) {
        await uiEdit(ctx, { schema_version: 1, op: "insert_call", revision: doc.revision, graph_id: scratch.graph_id, callee: "print", args: [`"u${i}"`] }, `mixed insert ${i}`);
      } else if (i % 3 === 1) {
        const expr = firstInline(scratch, (e) => String(e.source || "").includes("limit") || /^\d+$/.test(String(e.source || "")), "mixed inline");
        const next = String(expr.source || "") === "limit" ? "limit + 1" : "limit";
        await uiEdit(ctx, { schema_version: 1, op: "edit_inline_expr", revision: doc.revision, inline_expr_id: expr.inline_expr_id, new_expr: next }, `mixed inline ${i}`);
      } else {
        const from = (await ctx.source()).includes("total :=") ? "total" : "score";
        const to = from === "total" ? "score" : "total";
        await uiEdit(ctx, { schema_version: 1, op: "rename_binding", revision: doc.revision, from, to }, `mixed rename ${i}`);
      }
      sources.push(await ctx.source());
    }
    const state = await ctx.state();
    if (state.undoDepth !== 24 || state.undoLimit !== 50) throw new Error(`unexpected undo policy state: ${JSON.stringify({ depth: state.undoDepth, limit: state.undoLimit })}`);
    for (let i = 0; i < 20; i++) {
      await ctx.undo();
      const expected = sources[sources.length - 2 - i];
      const actual = await ctx.source();
      if (actual !== expected) throw new Error(`undo ${i} did not restore exact source`);
      await assertSourceSync(ctx, [`undo-depth undo ${i}`]);
    }
    const after = await ctx.state();
    if (after.redoDepth !== 20) throw new Error(`redo stack did not retain 20 entries: ${after.redoDepth}`);
  },

  "run-button-output-visible": async (ctx) => {
    await ctx.openCanvas();
    await ctx.driver.evaluate(`window.confirm = () => true`);
    await ctx.waitFor(async () => {
      await ctx.driver.evaluate(`window.__jetCanvasTest.runCurrentGraph()`);
      return await ctx.driver.evaluate(`!!document.getElementById("execute-command-authority")`);
    }, "run command authority");
    await ctx.driver.evaluate(`document.getElementById("execute-command-authority").click()`);
    await ctx.waitFor(async () => {
      return await ctx.driver.evaluate(`document.getElementById("run-hud").textContent.includes("passed")`);
    }, "run passed", 15000);
    const receipt = await ctx.driver.evaluate(`document.getElementById("details").textContent`);
    if (!receipt.includes("stdout") || !receipt.includes("16")) throw new Error(`run output not visible: ${receipt}`);
  },

  "debug-live-session": async (ctx) => {
    await ctx.openCanvas();
    if (await ctx.driver.evaluate(`document.getElementById("first-run-tour")?.classList.contains("is-open")`)) {
      await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    }
    await ctx.driver.evaluate(`(() => {
      window.__debugProof = { requests: [], fetch: window.fetch.bind(window) };
      window.fetch = async (input, init) => {
        const url = typeof input === "string" ? input : input.url;
        if (!String(url).includes("/canvas/debug")) return window.__debugProof.fetch(input, init);
        const request = JSON.parse((init && init.body) || "{}");
        const response = await window.__debugProof.fetch(input, init);
        window.__debugProof.requests.push({ request, response: await response.clone().json() });
        return response;
      };
    })()`);
    await clickElement(ctx, `document.querySelector("#debug-menu summary")`, "debug controls");
    await clickElement(ctx, `document.getElementById("debug-start")`, "start debug");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugSession && state.debugSession.state === "running"
        && state.debugOverlay && state.debugOverlay.active_line !== null
        && state.debugOverlay.runtime_state === "live"
        && state.debugState && state.debugState.state === "live";
    }, "live debug stop");
    const startProof = await ctx.driver.evaluate(`window.__debugProof.requests.find((entry) => !entry.request.session_id && !entry.request.stop) || null`);
    if (!startProof || startProof.request.schema_version !== 1 || !Array.isArray(startProof.request.commands) || !startProof.request.commands.includes("s")
      || startProof.response.protocol !== "jet.canvas.debug" || !startProof.response.ok
      || !startProof.response.session || startProof.response.session.state !== "running"
      || !startProof.response.overlay || startProof.response.overlay.runtime_state !== "live") {
      throw new Error(`debug start bypassed the production protocol: ${JSON.stringify(startProof)}`);
    }
    const first = await ctx.state();
    const firstSessionId = first.debugSession.id;
    const firstRevision = first.debugOverlay.revision;
    if (!firstSessionId || firstRevision !== (await ctx.graph()).revision) {
      throw new Error(`debug stop was not bound to the current session revision: ${JSON.stringify(first)}`);
    }
    await clickElement(ctx, `document.getElementById("debug-step")`, "debug step");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugSession && state.debugSession.state === "running"
        && state.debugSession.id === firstSessionId
        && state.debugOverlay && state.debugOverlay.revision === firstRevision
        && state.debugOverlay && state.debugOverlay.active_line !== first.debugOverlay.active_line
        && state.debugOverlay.runtime_state === "live"
        && state.debugOverlay.locals && state.debugOverlay.locals.some((local) => local.name === "value" && local.type === "Int" && local.value === "16")
        && state.debugOverlay.call_stack && state.debugOverlay.call_stack.some((frame) => frame.includes("run()"))
        && state.debugOverlay.active_node_id
        && Array.isArray(state.debugOverlay.wire_path)
        && (!state.debugOverlay.active_wire_id || state.debugOverlay.wire_path.includes(state.debugOverlay.active_wire_id));
    }, "second live debug stop");
    const stepProof = await ctx.driver.evaluate(`window.__debugProof.requests.find((entry) => entry.request.session_id === ${JSON.stringify(firstSessionId)} && !entry.request.stop) || null`);
    if (!stepProof || stepProof.response.protocol !== "jet.canvas.debug" || !stepProof.response.ok
      || !stepProof.response.overlay || stepProof.response.overlay.runtime_state !== "live"
      || !stepProof.response.overlay.locals?.some((local) => local.name === "value" && local.type === "Int" && local.value === "16")
      || !Array.isArray(stepProof.response.overlay.call_stack) || !stepProof.response.overlay.call_stack.some((frame) => frame.includes("run()"))
      || !stepProof.response.overlay.active_node_id || !Array.isArray(stepProof.response.overlay.wire_path)) {
      throw new Error(`debug step bypassed the live production payload: ${JSON.stringify(stepProof)}`);
    }
    const liveDetails = await ctx.driver.evaluate(`document.getElementById("details").textContent`);
    if (!liveDetails.includes("Int") || !liveDetails.includes("run()")) {
      throw new Error(`live debugger details did not show typed runtime state: ${liveDetails}`);
    }
    await clickElement(ctx, `document.getElementById("debug-stop")`, "stop debug");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      const active = await ctx.driver.evaluate(`document.body.classList.contains("is-debug-active")`);
      return !state.debugSession && !active;
    }, "debug session stopped");
    await ctx.driver.evaluate(`window.fetch = window.__debugProof.fetch`);
  },

  "debug-runtime-values-staleness-liveness": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("run");
    const initial = await ctx.state();
    const graph = (initial.doc.graphs || []).find((candidate) => candidate.graph_id === initial.graphId);
    const target = graph && graph.nodes.find((node) => node.source_span && node.kind !== "entry")
      || graph && graph.nodes.find((node) => node.source_span);
    const targetHit = target && (initial.hitMap.nodes || []).find((node) => node.node_id === target.node_id);
    if (!targetHit) throw new Error("staleness fixture has no source-backed breakpoint target");
    const canvas = await ctx.canvasRect();
    await ctx.click(canvas.left + targetHit.x + targetHit.w / 2, canvas.top + targetHit.y + targetHit.h / 2);
    await ctx.waitFor(async () => (await ctx.state()).selectedNodeId === target.node_id, "stale anchor target selection");
    await ctx.driver.rightClick(canvas.left + targetHit.x + targetHit.w / 2, canvas.top + targetHit.y + targetHit.h / 2);
    await ctx.expectMenu("Set breakpoint");
    await ctx.pickEntry("Set breakpoint");

    await clickElement(ctx, `document.querySelector("#debug-menu summary")`, "open debugger for stale anchor");
    await clickElement(ctx, `document.getElementById("debug-start")`, "start debugger for stale anchor");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugState && state.debugState.state === "live"
        && state.debugSession && state.debugSession.state === "running"
      && state.debugOverlay && state.debugOverlay.runtime_state === "live";
    }, "stale anchor live session");
    const before = await ctx.source();
    const moved = "// moved source for stale-anchor proof\n" + before;
    await ctx.setSourceEditor(moved);
    const toolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!toolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open source edit tools");
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "move source under live debugger");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugState && state.debugState.state === "stale"
        && state.debugState.staleBreakpoints && state.debugState.staleBreakpoints.length > 0
        && !state.debugSession && !state.debugOverlay;
    }, "stale debugger anchors disclosed");
    const staleSurface = await ctx.driver.evaluate(`({ liveness: document.getElementById("debug-liveness")?.textContent || "", details: document.getElementById("details")?.textContent || "", active: document.body.classList.contains("is-debug-active") })`);
    if (!staleSurface.liveness.includes("stale") || !staleSurface.details.includes("anchors are stale") || staleSurface.active) {
      throw new Error(`stale debugger state was not visible: ${JSON.stringify(staleSurface)}`);
    }

    await ctx.openCanvas();
    await clickElement(ctx, `document.querySelector("#debug-menu summary")`, "reopen debugger for disconnect");
    await clickElement(ctx, `document.getElementById("debug-start")`, "start debugger for disconnect");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugState && state.debugState.state === "live"
        && state.debugSession && state.debugSession.state === "running"
        && state.debugOverlay && state.debugOverlay.runtime_state === "live";
    }, "disconnect live session");
    await ctx.driver.evaluate(`(() => {
      window.__canvasRealFetch = window.fetch;
      window.__debugDisconnect = { attempted: false };
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input.url;
        if (String(url).includes("/canvas/debug")) {
          window.__debugDisconnect.attempted = true;
          return Promise.reject(new TypeError("runtime disconnected"));
        }
        return window.__canvasRealFetch(input, init);
      };
    })()`);
    await clickElement(ctx, `document.getElementById("debug-step")`, "step after runtime disconnect");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      const surface = await ctx.driver.evaluate(`({ liveness: document.getElementById("debug-liveness")?.textContent || "", details: document.getElementById("details")?.textContent || "", active: document.body.classList.contains("is-debug-active") })`);
      return state && state.debugState && state.debugState.state === "disconnected"
        && !state.debugSession && !state.debugOverlay
        && surface.liveness.includes("disconnected")
        && surface.details.includes("live values cleared")
        && !surface.active;
    }, "runtime disconnect clears cached values");
    if (!await ctx.driver.evaluate(`window.__debugDisconnect?.attempted === true`)) {
      throw new Error("runtime disconnect check bypassed the debug production request");
    }
    await ctx.driver.evaluate(`window.fetch = window.__canvasRealFetch`);

    await ctx.openCanvas();
    await clickElement(ctx, `document.querySelector("#debug-menu summary")`, "open debugger for stale response");
    await ctx.driver.evaluate(`(() => {
      window.__canvasRealFetch = window.fetch;
      window.__debugResponseGate = { started: false, pending: false, cleanup: 0, request: null };
      window.fetch = async (input, init) => {
        const url = typeof input === "string" ? input : input.url;
        if (!String(url).includes("/canvas/debug")) return window.__canvasRealFetch(input, init);
        const request = JSON.parse((init && init.body) || "{}");
        window.__debugResponseGate.request = request;
        const response = await window.__canvasRealFetch(input, init);
        if (request.stop) {
          window.__debugResponseGate.cleanup += 1;
          return response;
        }
        if (request.session_id || window.__debugResponseGate.started) return response;
        window.__debugResponseGate.started = true;
        window.__debugResponseGate.response = await response.clone().json();
        window.__debugResponseGate.pending = true;
        return new Promise((resolve) => {
          window.__debugResponseGate.release = () => {
            window.__debugResponseGate.pending = false;
            resolve(response);
          };
        });
      };
    })()`);
    const staleRequestSource = await ctx.source();
    const staleRequestRevision = (await ctx.graph()).revision;
    await clickElement(ctx, `document.getElementById("debug-start")`, "start debug before stale response");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`!!window.__debugResponseGate.pending && !!window.__debugResponseGate.response`), "stale debug response held");
    const staleRequest = await ctx.driver.evaluate(`window.__debugResponseGate.request`);
    const staleResponse = await ctx.driver.evaluate(`window.__debugResponseGate.response`);
    if (!staleRequest || staleRequest.schema_version !== 1 || staleRequest.session_id || !Array.isArray(staleRequest.commands) || !staleRequest.commands.includes("s")
      || !staleResponse || staleResponse.protocol !== "jet.canvas.debug" || !staleResponse.ok
      || !staleResponse.session || staleResponse.session.state !== "running"
      || staleResponse.revision !== staleRequestRevision || !staleResponse.overlay || staleResponse.overlay.runtime_state !== "live") {
      throw new Error(`stale-response fixture bypassed the live production protocol: ${JSON.stringify({ staleRequest, staleResponse })}`);
    }
    const movedAgain = "// moved source again for stale-response proof\n" + staleRequestSource;
    await ctx.setSourceEditor(movedAgain);
    const editToolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
    if (!editToolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, "open source edit tools for stale response");
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "change source before debug response");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.doc && state.doc.revision !== staleRequestRevision;
    }, "source changed before stale debug response");
    await ctx.driver.evaluate(`window.__debugResponseGate.release()`);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return await ctx.driver.evaluate(`window.__debugResponseGate.cleanup > 0`)
        && state && state.debugState && state.debugState.state === "stale"
        && !state.debugSession && !state.debugOverlay;
    }, "stale debug response cleanup");
    const staleResponseSurface = await ctx.driver.evaluate(`({ liveness: document.getElementById("debug-liveness")?.textContent || "", details: document.getElementById("details")?.textContent || "", active: document.body.classList.contains("is-debug-active") })`);
    if (!staleResponseSurface.liveness.includes("stale") || !staleResponseSurface.details.includes("anchors are stale") || staleResponseSurface.active) {
      throw new Error(`stale debug response was presented as live: ${JSON.stringify(staleResponseSurface)}`);
    }
    if (await ctx.source() !== movedAgain) throw new Error("stale debug response changed source");
    await ctx.driver.evaluate(`window.fetch = window.__canvasRealFetch`);
  },

  "debug-breakpoints-run-control-gestures": async (ctx) => {
    await ctx.openCanvas();
    if (await ctx.driver.evaluate(`document.getElementById("first-run-tour")?.classList.contains("is-open")`)) {
      await clickElement(ctx, `document.getElementById("tour-dismiss")`, "dismiss first-run guide");
    }
    await ctx.switchGraph("run");
    const beforeSource = await ctx.source();
    const initial = await ctx.state();
    const graph = (initial.doc.graphs || []).find((candidate) => candidate.graph_id === initial.graphId);
    const target = graph && graph.nodes.find((node) => node.source_span && node.kind !== "entry")
      || graph && graph.nodes.find((node) => node.source_span);
    const targetHit = target && (initial.hitMap.nodes || []).find((node) => node.node_id === target.node_id);
    if (!targetHit) throw new Error(`breakpoint target node is not rendered: ${JSON.stringify({ graph: initial.graphId, target })}`);
    const targetPoint = await ctx.node(target.title);
    await ctx.click(targetPoint.x, targetPoint.y);
    await ctx.waitFor(async () => (await ctx.state()).selectedNodeId === target.node_id, "breakpoint target selection");
    await ctx.driver.rightClick(targetPoint.x, targetPoint.y);
    await ctx.expectMenu("Set breakpoint");
    await ctx.pickEntry("Set breakpoint");

    await clickElement(ctx, `document.querySelector("#debug-menu summary")`, "debug controls");
    await clickElement(ctx, `document.getElementById("debug-start")`, "start debug with breakpoint");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugSession
        && state.debugOverlay
        && state.debugOverlay.breakpoints && state.debugOverlay.breakpoints.some((breakpoint) => breakpoint.state === "valid")
        && (state.debugSession.state === "running" || state.debugOverlay.debug_overlay === "finished");
    }, "debug start at breakpoint");
    const first = await ctx.state();
    const firstSessionId = first.debugSession.id;
    const firstRevision = first.debugOverlay.revision;
    const firstLine = first.debugOverlay.active_line;
    const firstWasRunning = first.debugSession.state === "running";
    await clickElement(ctx, `document.getElementById("debug-step")`, "debug step gesture");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugSession
        && state.debugOverlay && state.debugOverlay.revision === firstRevision
        && state.debugOverlay.trace && state.debugOverlay.trace.some((line) => line.includes("(jet) s"))
        && (!firstWasRunning || state.debugSession.id === firstSessionId)
        && (!firstWasRunning || state.debugOverlay.active_line !== firstLine);
    }, "debug step gesture");
    await clickElement(ctx, `document.getElementById("debug-continue")`, "debug continue gesture");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugOverlay && state.debugOverlay.revision === firstRevision
        && state.debugOverlay.trace && state.debugOverlay.trace.some((line) => line.includes("(jet) c"));
    }, "debug continue gesture");

    const afterContinue = await ctx.state();
    if (afterContinue.debugSession && afterContinue.debugSession.state === "running") {
      await clickElement(ctx, `document.getElementById("debug-stop")`, "stop before clearing breakpoint");
      await ctx.waitFor(async () => !(await ctx.state()).debugSession, "debug cleanup before clear");
    }
    const clearPoint = await ctx.node(target.title);
    await ctx.driver.rightClick(clearPoint.x, clearPoint.y);
    await ctx.expectMenu("Remove breakpoint");
    await ctx.pickEntry("Remove breakpoint");

    await clickElement(ctx, `document.getElementById("debug-start")`, "start debug after clear");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugSession
        && state.debugOverlay && state.debugOverlay.breakpoints && state.debugOverlay.breakpoints.length === 0
        && (state.debugSession.state === "running" || state.debugOverlay.debug_overlay === "finished");
    }, "cleared breakpoint sent to debug protocol");
    await clickElement(ctx, `document.getElementById("debug-continue")`, "debug continue after clear");
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      return state && state.debugOverlay && state.debugOverlay.debug_overlay === "finished"
        && state.debugOverlay.trace && state.debugOverlay.trace.some((line) => line.includes("(jet) c"));
    }, "debug continue after clear");
    if (await ctx.source() !== beforeSource) throw new Error("debug gestures changed source");
  },

  "graph-source-toggle-preserves-selection": async (ctx) => {
    await ctx.openCanvas();
    await clickSelectDetails(ctx);
    const selected = (await ctx.state()).selectedNodeId;
    await clickElement(ctx, `document.getElementById("view-code")`, "code lens button");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasLensMode === "code"`), "code view");
    await clickElement(ctx, `document.getElementById("view-graph")`, "graph lens button");
    await ctx.waitForCanvas();
    const after = await ctx.state();
    if (after.selectedNodeId !== selected) throw new Error(`selection changed across graph/source toggle: ${selected} -> ${after.selectedNodeId}`);
  },

  "canvas-rad-two-way-round-trip": async (ctx) => {
    await ctx.openCanvas();

    const sourceBefore = await ctx.source();
    const initialState = await ctx.state();
    const initialNode = Object.values(initialState.nodeBounds || {}).find((node) => node.title === "square")
      || Object.values(initialState.nodeBounds || {})[0];
    if (!initialNode) throw new Error("Canvas design gesture has no projected node");

    // A design gesture changes only local view state. The source remains the
    // one semantic model before and after a fresh projection.
    const rect = await ctx.canvasRect();
    const from = {
      x: rect.left + initialNode.x + initialNode.w / 2,
      y: rect.top + initialNode.y + initialNode.h / 2,
    };
    await ctx.driver.drag(from, { x: from.x + 31, y: from.y + 23 }, 16);
    await ctx.waitFor(async () => {
      const state = await ctx.state();
      const moved = state.nodeBounds && state.nodeBounds[initialNode.node_id];
      return !!moved && Math.abs(moved.x - initialNode.x - 31) < 1 && Math.abs(moved.y - initialNode.y - 23) < 1;
    }, "local design position");
    if (await ctx.source() !== sourceBefore) throw new Error("design gesture changed Jet source");
    const designed = await ctx.state();
    const designedNode = designed.nodeBounds[initialNode.node_id];
    await ctx.openCanvas();
    const reloadedDesign = await ctx.state();
    const reloadedNode = reloadedDesign.nodeBounds && reloadedDesign.nodeBounds[initialNode.node_id];
    if (!reloadedNode || Math.abs(reloadedNode.x - designedNode.x) > 1 || Math.abs(reloadedNode.y - designedNode.y) > 1) {
      throw new Error(`design position did not survive fresh projection: ${JSON.stringify({ designed: designedNode, reloaded: reloadedNode })}`);
    }
    if (await ctx.source() !== sourceBefore) throw new Error("design reload changed Jet source");

    const openSourceEditor = async (label) => {
      const toolsOpen = await ctx.driver.evaluate(`!!document.querySelector("#more-tools-toggle")?.parentElement?.open`);
      if (!toolsOpen) await clickElement(ctx, `document.getElementById("more-tools-toggle")`, `${label} tools`);
      await clickElement(ctx, `document.getElementById("edit-source")`, `${label} source editor`);
      await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasSourceEditMode === true`), `${label} editor mode`);
    };
    const replaceSourceBuffer = async (source) => {
      // Select the visible textarea, then use the browser's text-input path.
      // CDP's low-level char-event helper drops newlines and its synthetic
      // Ctrl+A does not reliably update textarea selection in Chromium.
      await ctx.driver.evaluate(`document.getElementById("source-editor")?.focus(); document.getElementById("source-editor")?.select()`);
      await ctx.driver.send("Input.insertText", { text: String(source) }, ctx.driver.pageSession);
    };

    // Source -> graph: type through the visible editor and press its real
    // Apply button. The response is then projected through /canvas/graph.
    await clickElement(ctx, `document.getElementById("view-code")`, "open code lens");
    const editedSource = sourceBefore.replace("print(limit)", "print(limit + 1)");
    await openSourceEditor("valid");
    await replaceSourceBuffer(editedSource);
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "apply valid source gesture");
    await ctx.waitFor(async () => {
      const result = await ctx.driver.evaluate("window.__jetCanvasLastTxResult || null");
      return !!(result && result.changed === true && result.source_text === await ctx.source());
    }, "valid source transaction");
    await assertCleanSourceSync(ctx, ["source editor", "source to graph"]);
    const projectedAfterText = graphByTitle(await ctx.graph(), "scratch");
    const editedExpr = firstInline(projectedAfterText, (expr) => String(expr.source || "").includes("limit + 1"), "edited scratch expression");
    if (!editedExpr.source_span || (await ctx.source()).slice(editedExpr.source_span.start, editedExpr.source_span.end) !== editedExpr.source) {
      throw new Error(`source-to-graph projection lost inline provenance: ${JSON.stringify(editedExpr)}`);
    }

    // Graph -> source: open the pin palette with a real pointer context
    // gesture, then use the shared harness action-selection path. This must
    // create a checked source transaction.
    await ctx.switchGraph("scratch");
    await clickElement(ctx, `document.getElementById("view-graph")`, "return to graph lens");
    await ctx.loadCoreCatalog();
    const sourcePin = await ctx.pin("limit", "limit");
    await ctx.driver.rightClick(sourcePin.x, sourcePin.y);
    await ctx.expectMenu("Search actions");
    await ctx.type("abs");
    await ctx.expectMenu("abs");
    await ctx.pickEntry("abs");
    await sleep(500);
    console.error("RAD_GRAPH_TX_DEBUG", JSON.stringify(await ctx.driver.evaluate(`(async () => ({ tx: window.__jetCanvasLastTx || null, result: window.__jetCanvasLastTxResult || null, state: window.__jetCanvasCanvasState || null, source: await fetch("/canvas/source", { cache: "no-store" }).then((r) => r.text()) }))()`)));
    await ctx.waitForCanvas();
    await ctx.waitFor(async () => (await ctx.source()).includes("math.abs"), "graph gesture source transaction");
    const sourceAfterGraph = await ctx.source();
    if (sourceAfterGraph === editedSource) throw new Error("graph gesture did not change source");
    await assertCleanSourceSync(ctx, ["source to graph", "graph to source"]);
    const projectedAfterGraph = graphByTitle(await ctx.graph(), "scratch");
    const absNode = (projectedAfterGraph.nodes || []).find((node) => {
      if (!node.source_span) return false;
      const nodeText = sourceAfterGraph.slice(node.source_span.start, node.source_span.end);
      const context = sourceAfterGraph.slice(Math.max(0, node.source_span.start - 5), Math.min(sourceAfterGraph.length, node.source_span.end + 5));
      return nodeText.includes("abs") && context.includes("math.abs");
    });
    if (!absNode) throw new Error("graph gesture lost source-backed math.abs node provenance");

    // The three visible lenses must preserve the same source snapshot and
    // projection, not a second graph or editor model.
    await clickElement(ctx, `document.getElementById("view-code")`, "round-trip code lens");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`document.getElementById("source-view")?.textContent === ${JSON.stringify(sourceAfterGraph)}`), "round-trip source view");
    await clickElement(ctx, `document.getElementById("view-split")`, "round-trip split lens");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasLensMode === "split"`), "round-trip split view");
    await clickElement(ctx, `document.getElementById("view-graph")`, "round-trip graph lens");
    await ctx.waitForCanvas();
    const ui = await ctx.uiDoc();
    const fresh = await ctx.graph();
    if (!ui || !fresh || ui.source_id !== fresh.source_id || ui.revision !== fresh.revision || ui.source_text !== fresh.source_text || fresh.source_text !== sourceAfterGraph) {
      throw new Error(`RAD source/graph snapshot drift: ${JSON.stringify({ ui: ui && { source_id: ui.source_id, revision: ui.revision }, fresh: fresh && { source_id: fresh.source_id, revision: fresh.revision }, sourceAfterGraph })}`);
    }

    // A rejected save keeps both the committed source and the existing undo
    // stack. Restore the network seam, then leave the failed draft visible for
    // recovery rather than silently replacing it with a shadow snapshot.
    const beforeFailedSave = await ctx.source();
    const beforeFailedDepth = (await ctx.state()).undoDepth;
    const failedDraft = beforeFailedSave.replace("limit + 1", "limit + 2");
    await openSourceEditor("failed save");
    await replaceSourceBuffer(failedDraft);
    await ctx.driver.evaluate(`(() => {
      window.__canvasRadRealFetch = window.fetch.bind(window);
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input && input.url || "";
        if (url.endsWith("/canvas/transaction")) return Promise.reject(new Error("Canvas RAD save failure"));
        return window.__canvasRadRealFetch(input, init);
      };
    })()`);
    await clickElement(ctx, `document.getElementById("apply-source-edit")`, "failed save gesture");
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasCanvasState?.kind === "error"`), "failed save refusal");
    await ctx.driver.evaluate(`window.fetch = window.__canvasRadRealFetch; delete window.__canvasRadRealFetch`);
    if (await ctx.source() !== beforeFailedSave) throw new Error("failed save changed committed source");
    const afterFailed = await ctx.state();
    if (!afterFailed || afterFailed.undoDepth !== beforeFailedDepth) {
      throw new Error(`failed save changed undo history: ${JSON.stringify({ before: beforeFailedDepth, after: afterFailed && afterFailed.undoDepth })}`);
    }
    const draft = await ctx.driver.evaluate(`document.getElementById("source-editor")?.value || ""`);
    if (draft !== failedDraft) throw new Error("failed save discarded recoverable source draft");

    // Parse/semantic refusal also preserves the same committed snapshot and
    // history. The normal problem rail must be visible before any retry.
    await replaceSourceBuffer(`${beforeFailedSave}\nfn broken(`);
    await clickElement(ctx, `document.getElementById("check-current")`, "invalid source check");
    await ctx.waitFor(async () => {
      const state = await ctx.driver.evaluate("window.__jetCanvasCanvasState || null");
      const problems = await ctx.problems();
      return state && state.kind === "invalid" && problems.length > 0;
    }, "invalid source refusal");
    const invalidProblems = await ctx.problems();
    if (!invalidProblems.some((problem) => problem.what && String(problem.rendered || "").includes("Why:") && String(problem.rendered || "").includes("Fix:"))) {
      throw new Error(`invalid source diagnostics lost what/why/fix: ${JSON.stringify(invalidProblems)}`);
    }
    if (await ctx.source() !== beforeFailedSave) throw new Error("invalid source changed committed source");
    const afterInvalid = await ctx.state();
    if (!afterInvalid || afterInvalid.undoDepth !== beforeFailedDepth) {
      throw new Error(`invalid source changed undo history: ${JSON.stringify({ before: beforeFailedDepth, after: afterInvalid && afterInvalid.undoDepth })}`);
    }
  },

  "random-ops-source-sync": async (ctx) => {
    await ctx.openCanvas();
    const rng = prng(ctx.seed);
    const opLog = [`seed=${ctx.seed}`];
    const sources = [await ctx.source()];
    for (let i = 0; i < 30; i++) {
      const allowUndo = sources.length > 1;
      const choice = allowUndo ? Math.floor(rng() * 5) : Math.floor(rng() * 4);
      try {
        if (choice === 0) {
          const { doc, graph } = await currentGraphDoc(ctx, "scratch");
          const label = `insert print ${i}`;
          opLog.push(label);
          await uiEdit(ctx, { schema_version: 1, op: "insert_call", revision: doc.revision, graph_id: graph.graph_id, callee: "print", args: [`"r${i}"`] }, label);
          sources.push(await ctx.source());
        } else if (choice === 1) {
          const { doc, graph } = await currentGraphDoc(ctx, "scratch");
          const expr = firstInline(graph, (e) => String(e.source || "").includes("limit") || /^\d+$/.test(String(e.source || "")), "random inline");
          const next = pick(rng, ["limit", "limit + 1", "limit + 2", "7"]);
          const label = `edit inline ${i} -> ${next}`;
          opLog.push(label);
          await uiEdit(ctx, { schema_version: 1, op: "edit_inline_expr", revision: doc.revision, inline_expr_id: expr.inline_expr_id, new_expr: next }, label);
          sources.push(await ctx.source());
        } else if (choice === 2) {
          const doc = await ctx.graph();
          const from = (await ctx.source()).includes("total :=") ? "total" : "score";
          const to = from === "total" ? "score" : "total";
          const label = `rename ${from} ${to}`;
          opLog.push(label);
          await uiEdit(ctx, { schema_version: 1, op: "rename_binding", revision: doc.revision, from, to }, label);
          sources.push(await ctx.source());
        } else if (choice === 3) {
          const { doc, graph } = await currentGraphDoc(ctx, "scratch");
          const hasExtra = (await ctx.source()).includes("extra: Int{1}");
          const signature = hasExtra
            ? "fn scratch(limit: Int, text: String, flag: Bool, ratio: Float)"
            : "fn scratch(limit: Int, text: String, flag: Bool, ratio: Float, extra: Int{1})";
          const label = `signature ${hasExtra ? "remove" : "add"} extra`;
          opLog.push(label);
          await uiEdit(ctx, { schema_version: 1, op: "edit_function_signature", revision: doc.revision, graph_id: graph.graph_id, signature }, label);
          sources.push(await ctx.source());
        } else {
          const label = `undo ${i}`;
          opLog.push(label);
          await ctx.undo();
          sources.pop();
          const expected = sources[sources.length - 1];
          const actual = await ctx.source();
          if (actual !== expected) throw new Error(`${label} restored different source`);
        }
        await assertSourceSync(ctx, opLog);
      } catch (err) {
        throw new Error(`random op failed with seed ${ctx.seed}\nops:\n${opLog.join("\n")}\n${err && err.stack || err}`);
      }
    }
  },

  "big-project-perf": async (ctx) => {
    await ctx.driver.send("Emulation.setDeviceMetricsOverride", {
      width: 1440,
      height: 900,
      deviceScaleFactor: 1,
      mobile: false,
    }, ctx.driver.pageSession);
    const openMs = await ctx.openCanvas();
    const doc = await ctx.graph();
    const project = await ctx.driver.evaluate(`fetch("/canvas/project", { cache: "no-store" }).then((response) => response.json())`);
    const sourceFiles = (project.files || []).filter((file) => file.kind === "source");
    const graphs = doc.graphs || [];
    const run = graphs.find((graph) => graph.title === "run");
    const functionGraphs = graphs.filter((graph) => /^function_\d{3}$/.test(String(graph.title || "")));
    if (sourceFiles.length !== BIG_PROJECT.files || graphs.length !== BIG_PROJECT.graphs || functionGraphs.length !== BIG_PROJECT.functions || !run) {
      throw new Error(`generated project shape mismatch: ${JSON.stringify({ files: sourceFiles.length, graphs: graphs.length, functionGraphs: functionGraphs.length, run: !!run })}`);
    }
    if (!run.nodes || run.nodes.length < BIG_PROJECT.functions) {
      throw new Error(`generated run graph is too small: ${run.nodes && run.nodes.length}`);
    }
    console.log(`BIG_PROJECT_FIXTURE ${JSON.stringify({ files: sourceFiles.length, graphs: graphs.length, functions: functionGraphs.length, run_nodes: run.nodes.length, run_wires: (run.wires || []).length, open_to_interactive_ms: openMs })}`);
    if (openMs > BIG_PROJECT.openBudgetMs) {
      throw new Error(`open-to-interactive budget exceeded: ${openMs}ms > ${BIG_PROJECT.openBudgetMs}ms`);
    }

    if ((await ctx.state()).graphTitle !== "run") await bigClickGraphTab(ctx, "run");
    await bigFit(ctx);
    let state = await ctx.state();
    if (!state.virtualizationStats || state.virtualizationStats.total !== run.nodes.length || state.virtualizationStats.visible >= state.virtualizationStats.total) {
      throw new Error(`large graph was not culled at fit: ${JSON.stringify(state.virtualizationStats)}`);
    }
    const sourceBefore = await ctx.source();
    const canvas = await ctx.canvasRect();
    const center = { x: canvas.left + canvas.width / 2, y: canvas.top + canvas.height / 2 };

    await bigFrameMeasure(ctx, "pan", async () => {
      await bigMiddleDrag(ctx, 460, -120, 32);
    });

    const zoomBefore = (await ctx.state()).view.zoom;
    await bigFrameMeasure(ctx, "zoom", async () => {
      for (let index = 0; index < 5; index++) await ctx.driver.wheel(center.x, center.y, 160);
      await ctx.waitFor(async () => (await ctx.state()).view.zoom < zoomBefore, "zoom input");
    });
    state = await ctx.state();
    if (!state.virtualizationStats.lod) throw new Error(`zoom did not enter low-detail mode: ${JSON.stringify(state.virtualizationStats)}`);

    await bigFit(ctx);
    state = await ctx.state();
    const selectable = run.nodes.find((node) => String(node.title || "").startsWith("value_") && state.nodeBounds[node.node_id]);
    if (!selectable) throw new Error("no visible generated value node for selection");
    await bigFrameMeasure(ctx, "selection", async () => {
      await bigClickNode(ctx, state.nodeBounds[selectable.node_id]);
      await ctx.waitFor(async () => (await ctx.state()).selectedNodeId === selectable.node_id, "generated node selection");
    });

    await bigFrameMeasure(ctx, "palette", async () => {
      await ctx.driver.shortcut(["Control", "p"]);
      await ctx.waitFor(async () => await ctx.driver.evaluate(`!!document.getElementById("action-palette-search")`), "action palette open");
      await ctx.driver.type("function_299");
      await ctx.waitFor(async () => await ctx.driver.evaluate(`document.getElementById("action-palette-search")?.value === "function_299"`), "palette search input");
    });
    const paletteFocus = await ctx.driver.evaluate(`document.activeElement?.id`);
    if (paletteFocus !== "action-palette-search") throw new Error(`palette search lost keyboard focus: ${paletteFocus}`);
    await ctx.driver.press("Escape");

    state = await ctx.state();
    const cullTarget = run.nodes.find((node) => !state.nodeBounds[node.node_id]);
    if (!cullTarget) throw new Error("large graph had no off-viewport node to re-enter");
    const panDirections = [[-640, 0], [0, -480], [640, 0], [0, 480]];
    let entered = false;
    for (const [dx, dy] of panDirections) {
      for (let step = 0; step < 32 && !entered; step++) {
        await bigMiddleDrag(ctx, dx, dy, 20);
        await sleep(45);
        state = await ctx.state();
        if (state.nodeBounds[cullTarget.node_id]) entered = true;
      }
      if (entered) break;
    }
    if (!entered) throw new Error(`off-viewport node did not re-enter hit map: ${cullTarget.node_id}`);
    await bigClickNode(ctx, state.nodeBounds[cullTarget.node_id]);
    await ctx.waitFor(async () => (await ctx.state()).selectedNodeId === cullTarget.node_id, "re-entered node selection");
    state = await ctx.state();
    const pin = (state.hitMap && state.hitMap.pins || []).find((candidate) => candidate.node_id === cullTarget.node_id);
    if (!pin) throw new Error(`re-entered node has no hit-testable pin: ${cullTarget.node_id}`);
    await bigClickPin(ctx, pin);
    await ctx.waitFor(async () => (await ctx.driver.evaluate("window.__jetCanvasPendingPin && window.__jetCanvasPendingPin.pin_id")) === pin.pin_id, "pin hit test after culling");
    if (await ctx.source() !== sourceBefore) throw new Error("pin hit test changed generated source");
    await ctx.driver.press("Escape");
    state = await ctx.state();
    const wire = (state.wireEndpoints || []).find((endpoint) => endpoint.pin_id === pin.pin_id || endpoint.other_pin_id === pin.pin_id);
    if (!wire) throw new Error(`re-entered node has no wire hit endpoint: ${cullTarget.node_id}`);
    await ctx.driver.click(wire.client_x, wire.client_y);
    await ctx.waitFor(async () => (await ctx.driver.evaluate("window.__jetCanvasPendingPin && window.__jetCanvasPendingPin.pin_id")) === wire.pin_id, "wire endpoint hit test after culling");
    if (await ctx.source() !== sourceBefore) throw new Error("wire hit test changed generated source");
    await ctx.driver.press("Escape");
    if (await bigMinimapInk(ctx) < 50) throw new Error("minimap lost graph ink while culling nodes");

    let left = false;
    const leavePath = [];
    for (let pass = 0; pass < 8 && !left; pass++) {
      for (const [dx, dy] of [[-640, 0], [0, -480], [640, 0], [0, 480]]) {
        leavePath.push([dx, dy]);
        await bigMiddleDrag(ctx, dx, dy, 20);
        await sleep(45);
        state = await ctx.state();
        if (!state.nodeBounds[cullTarget.node_id]) {
          left = true;
          break;
        }
      }
    }
    if (!left) throw new Error(`selected node never left the virtualized viewport: ${cullTarget.node_id}`);
    if ((await ctx.state()).selectedNodeId !== cullTarget.node_id) throw new Error("selection was lost when node left viewport");
    for (const [dx, dy] of leavePath.reverse()) {
      await bigMiddleDrag(ctx, -dx, -dy, 20);
      await sleep(45);
      state = await ctx.state();
      if (state.nodeBounds[cullTarget.node_id]) break;
    }
    if (!state.nodeBounds[cullTarget.node_id] || state.selectedNodeId !== cullTarget.node_id) {
      throw new Error("selection was not restored when node re-entered viewport");
    }
    await bigClickSelector(ctx, "#source-jump", "source navigation");
    await ctx.waitFor(async () => String(await ctx.driver.evaluate("location.hash")).startsWith("#span-"), "source navigation hash");
    if (await ctx.source() !== sourceBefore) throw new Error("source navigation changed generated source");
    await bigClickSelector(ctx, "#dock-graphs", "open project drawer");
    await ctx.waitFor(async () => Number(await ctx.driver.evaluate(`document.querySelector('[data-project-file]')?.getBoundingClientRect().width || 0`)) > 0, "project drawer");

    const projectPath = await ctx.driver.evaluate(`(() => {
      const card = Array.from(document.querySelectorAll("[data-project-file]")).find((element) => element.getAttribute("data-project-file")?.endsWith("part_00.jet"));
      return card && card.getAttribute("data-project-file");
    })()`);
    if (!projectPath) throw new Error("visible generated project file card missing");
    await bigFrameMeasure(ctx, "file-switch", async () => {
      await bigClickProjectFile(ctx, projectPath);
      console.log(`BIG_PROJECT_FILE_STATE ${JSON.stringify(await ctx.driver.evaluate(`(() => ({ source: window.__jetCanvasTest?.doc?.source_id || null, selected: window.__jetCanvasTest?.selectedSourceId || null, generation: window.__jetCanvasGraphLoadGeneration || 0, canvas_state: document.getElementById("canvas-state")?.dataset.state || "" }))()`))}`);
      await ctx.waitFor(async () => String((await ctx.state()).doc?.source_id || "").endsWith("part_00.jet"), "generated file switch");
    });
    const mainPath = await ctx.driver.evaluate(`(() => {
      const card = Array.from(document.querySelectorAll("[data-project-file]")).find((element) => element.getAttribute("data-project-file")?.endsWith("main.jet"));
      return card && card.getAttribute("data-project-file");
    })()`);
    if (!mainPath) throw new Error("generated entry file card missing after switch");
    await bigClickProjectFile(ctx, mainPath);
    await ctx.waitFor(async () => String((await ctx.state()).doc?.source_id || "").endsWith("main.jet"), "generated entry file restore");
    console.log(`BIG_PROJECT_PERF_BUDGETS ${JSON.stringify({ open_to_interactive_ms: BIG_PROJECT.openBudgetMs, frame_p95_ms: BIG_PROJECT.frameP95BudgetMs, frame_max_ms: BIG_PROJECT.frameMaxBudgetMs, repeated_clean_runs: 2 })}`);
  },

  "harness-click-noop-selftest": async (ctx) => {
    await ctx.openCanvas();
    let failed = false;
    try {
      await clickSelectDetails(ctx, { noopClick: true });
    } catch (_) {
      failed = true;
    }
    await ctx.driver.evaluate("window.__jetCanvasNoopClick = false");
    if (!failed) throw new Error("click-select scenario still passed with click handler no-op");
  },
};
