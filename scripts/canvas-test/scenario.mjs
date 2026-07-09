import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { CdpDriver } from "./driver.mjs";

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export class CanvasScenario {
  constructor({ port, outDir, scenarioName }) {
    this.port = port;
    this.outDir = outDir;
    this.scenarioName = scenarioName;
    this.driver = new CdpDriver();
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
    await this.driver.navigate(`http://127.0.0.1:${port}/canvas`);
    await this.waitForCanvas();
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
    const body = await this.driver.evaluate(`fetch("/canvas/source", { cache: "no-store" }).then((r) => r.text())`);
    if (!body.includes(text)) {
      const tx = await this.driver.evaluate(`JSON.stringify({ tx: window.__jetCanvasLastTx || null, result: window.__jetCanvasLastTxResult || null })`);
      throw new Error(`source missing ${JSON.stringify(text)}\n${body}\nlast: ${tx}`);
    }
  }

  async source() {
    return await this.driver.evaluate(`fetch("/canvas/source", { cache: "no-store" }).then((r) => r.text())`);
  }

  async graph() {
    return await this.driver.evaluate(`fetch("/canvas/graph", { cache: "no-store" }).then((r) => r.json())`);
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
    return await this.driver.evaluate(`fetch("/canvas/query", { method: "POST", headers: { "content-type": "application/json" }, body: ${JSON.stringify(JSON.stringify(body))} }).then((r) => r.json())`);
  }

  async transaction(body) {
    return await this.driver.evaluate(`fetch("/canvas/transaction", { method: "POST", headers: { "content-type": "application/json" }, body: ${JSON.stringify(JSON.stringify(body))} }).then((r) => r.json().then((json) => ({ ok: r.ok, json })))`);
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
  if (entry.available === false) {
    if (!entry.unavailable_reason_code || !entry.denied_reason) {
      throw new Error(`excluded entry missing reason: ${entry.action_id || entry.callee || entry.title}`);
    }
    return { state: "excluded", id: entry.action_id || entry.callee || entry.title, reason: entry.unavailable_reason_code };
  }
  const callee = entry.insert_callee || entry.callee;
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

async function catalogSweep(ctx) {
  await ctx.openCanvas();
  const baseSource = await ctx.source();
  await ctx.switchGraph("scratch");
  await ctx.openPinActionMenu("scratch", "limit");
  await ctx.type("square");
  await ctx.expectMenu("square");
  await ctx.pickEntry("square");
  await ctx.expectSourceContains("square(limit)");
  await ctx.replaceSource(baseSource);
  const actionDocGraph = await ctx.graph();
  const actionDoc = await ctx.query({ schema_version: 1, op: "actions", revision: actionDocGraph.revision });
  const projectEntries = (actionDoc.project_functions || []).map((fn) => ({
    title: fn.name || fn.callee,
    kind: "project_function",
    action_id: `project:${fn.callee || fn.name}`,
    callee: fn.callee || fn.name,
    insert_callee: fn.insert_callee || fn.callee || fn.name,
    module_path: fn.module_path || "project",
    signature: fn.signature || "",
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
  const seen = new Set();
  const unique = targets.filter((entry) => {
    const id = entry.action_id || entry.callee || entry.title;
    if (seen.has(id)) return false;
    seen.add(id);
    return true;
  });
  const summary = { total: unique.length, inserted: 0, excluded: 0, dataInserted: 0, noDataOrigin: 0, failures: [] };
  for (const entry of unique) {
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
  if (summary.failures.length) {
    throw new Error(`catalog sweep failed ${JSON.stringify(summary.failures.slice(0, 12), null, 2)}\nsummary ${JSON.stringify(summary)}`);
  }
  console.log(`palette_insert_catalog_sweep total=${summary.total} inserted=${summary.inserted} data_inserted=${summary.dataInserted} excluded=${summary.excluded} no_data_origin=${summary.noDataOrigin}`);
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

async function failScratchLimit(ctx) {
  await ctx.openCanvas();
  await ctx.switchGraph("scratch");
  const { doc, expr } = await scratchLimitInline(ctx);
  const result = await ctx.transaction({
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

export const scenarios = {
  "open-and-render": async (ctx) => {
    await ctx.openCanvas();
    await ctx.expectNodeCount(3);
    const pixels = await ctx.nonblankPixels();
    if (pixels < 100) throw new Error(`canvas looked blank: ${pixels} colored pixels`);
    await ctx.screenshot("rendered");
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

  "palette-insert-core-fn": async (ctx) => {
    await ctx.openCanvas();
    await ctx.loadCoreCatalog();
    await ctx.openPinActionMenu("limit", "limit");
    await ctx.expectMenu("Search actions");
    await ctx.type("abs");
    await ctx.expectMenu("abs");
    await ctx.pickEntry("abs");
    await ctx.expectSourceContains("use core.math as math");
    await ctx.expectSourceContains("math.abs");
    await ctx.screenshot("core-abs-inserted");
  },

  "palette-insert-catalog-sweep": async (ctx) => {
    await catalogSweep(ctx);
  },

  "fallible-context": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    await ctx.driver.evaluate(`window.__jetCanvasTest.openGraphActionPalette("Fallible")`);
    await ctx.expectMenu("Fallible");
    const row = await ctx.driver.evaluate(`(() => {
      const buttons = Array.from(document.querySelectorAll("#context-menu [data-menu-action]"));
      const button = buttons.find((b) => b.textContent.includes("Fallible"));
      if (!button) return null;
      return { available: button.dataset.available, code: button.dataset.unavailableReasonCode, title: button.getAttribute("title"), text: button.textContent };
    })()`);
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
      return { available: button.dataset.available, code: button.dataset.unavailableReasonCode, title: button.getAttribute("title"), text: button.textContent };
    })()`);
    if (!row || row.available !== "false" || row.code !== "method_only" || !row.title.includes("ArgsSpec")) {
      throw new Error(`excluded help row missing hover reason: ${JSON.stringify(row)}`);
    }
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
    const before = await ctx.source();
    await ctx.replaceSource(before);
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
    const undo = await ctx.driver.evaluate(`(() => {
      const b = document.getElementById("undo-edit");
      if (!b) return false;
      b.click();
      return true;
    })()`);
    if (!undo) throw new Error("undo button missing");
    await ctx.waitFor(async () => {
      const source = await ctx.driver.evaluate(`fetch("/canvas/source", { cache: "no-store" }).then((r) => r.text())`);
      return source === before;
    }, "source restored by undo");
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
