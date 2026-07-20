import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { CdpDriver } from "./driver.mjs";

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export class CanvasScenario {
  constructor({ port, outDir, scenarioName, seed = 373 }) {
    this.port = port;
    this.outDir = outDir;
    this.scenarioName = scenarioName;
    this.seed = Number(seed) || 373;
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
    return await this.driver.evaluate(`fetch("/canvas/query", { method: "POST", headers: { "content-type": "application/json" }, body: ${JSON.stringify(JSON.stringify(body))} }).then((r) => r.json())`);
  }

  async transaction(body) {
    return await this.driver.evaluate(`fetch("/canvas/transaction", { method: "POST", headers: { "content-type": "application/json" }, body: ${JSON.stringify(JSON.stringify(body))} }).then((r) => r.json().then((json) => ({ ok: r.ok, json })))`);
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
      return window.__jetCanvasHistoryPromise instanceof Promise;
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

function graphByTitle(doc, title) {
  const graph = (doc.graphs || []).find((g) => g.title === title || String(g.title || "").includes(title));
  if (!graph) throw new Error(`graph missing: ${title}`);
  return graph;
}

function nodeByTitle(graph, title) {
  const node = (graph.nodes || []).find((n) => n.title === title || String(n.title || "").includes(title));
  if (!node) throw new Error(`node missing: ${title}`);
  return node;
}

function pinForNode(graph, title, direction, type = "exec") {
  const node = nodeByTitle(graph, title);
  const pin = (graph.pins || []).find((p) => p.node_id === node.node_id && p.direction === direction && p.type === type);
  if (!pin) throw new Error(`pin missing: ${title}.${direction}.${type}`);
  return pin;
}

function controlWireExists(graph, fromTitle, toTitle) {
  const from = nodeByTitle(graph, fromTitle);
  const to = nodeByTitle(graph, toTitle);
  const fromPins = new Set((graph.pins || []).filter((p) => p.node_id === from.node_id).map((p) => p.pin_id));
  const toPins = new Set((graph.pins || []).filter((p) => p.node_id === to.node_id).map((p) => p.pin_id));
  return (graph.wires || []).some((w) => w.wire_kind === "control" && fromPins.has(w.from_pin) && toPins.has(w.to_pin));
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

  "read-graph-overview": async (ctx) => {
    await ctx.openCanvas();
    const overview = await ctx.driver.evaluate("window.__jetCanvasGraphOverview");
    if (!overview || !overview.title || overview.nodes < 1 || overview.exec_pins < 1) {
      throw new Error(`graph overview missing graph facts: ${JSON.stringify(overview)}`);
    }
    const tabs = await ctx.driver.evaluate("Number(window.__jetCanvasGraphTabCount || 0)");
    if (tabs < 4) throw new Error(`expected project graph tabs, saw ${tabs}`);
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

  "palette-insert-flow-variable-project-core": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    let graphDoc = await ctx.graph();
    let scratch = graphByTitle(graphDoc, "scratch");
    await uiEdit(ctx, { schema_version: 1, op: "insert_branch", revision: graphDoc.revision, graph_id: scratch.graph_id }, "flow branch insert");
    await ctx.expectSourceContains("if true");

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

  "pattern-arm-add-edit-remove": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`enum Choice {
    A(Int)
    B(Int)
    C(Int)
}

fn choose(x: Choice) -> Int {
    if x == {
        A(n) -> { return n }
        else -> { return 0 }
    }
}

fn run() {
    print(choose(Choice.A(1)))
}
`);
    await ctx.openCanvas();
    let before = await ctx.source();
    await ctx.driver.evaluate(`window.prompt = () => "== B(n)"`);
    let pos = await ctx.node("if ==");
    await ctx.driver.rightClick(pos.x, pos.y);
    await ctx.expectMenu("Add pattern arm");
    await ctx.pickEntry("Add pattern arm");
    await ctx.waitFor(async () => (await ctx.source()).includes("B(n) ->"), "pattern arm add");
    await assertSourceSync(ctx, ["pattern add"]);
    await ctx.expectSourceContains("B(n) ->");

    await ctx.driver.evaluate(`window.prompt = () => "== C(n)"`);
    const pin = await ctx.pin("if ==", "arm2");
    await ctx.driver.rightClick(pin.x, pin.y);
    await ctx.expectMenu("Edit pattern");
    await ctx.pickEntry("Edit pattern");
    await ctx.waitFor(async () => (await ctx.source()).includes("C(n) ->") && !(await ctx.source()).includes("B(n) ->"), "pattern arm edit");
    await assertSourceSync(ctx, ["pattern edit"]);

    const edited = await ctx.source();
    const removePin = await ctx.pin("if ==", "arm2");
    await ctx.driver.rightClick(removePin.x, removePin.y);
    await ctx.expectMenu("Remove arm");
    await ctx.pickEntry("Remove arm");
    await ctx.waitFor(async () => !(await ctx.source()).includes("C(n) ->"), "pattern arm remove");
    await assertSourceSync(ctx, ["pattern remove"]);

    const restored = await ctx.undo();
    if (restored !== edited) throw new Error(`undo did not restore edited pattern arm\nexpected:\n${edited}\nactual:\n${restored}`);
    if (before === restored) throw new Error("pattern add/edit/remove cycle did not change source before undo checkpoint");
  },

  "pattern-arm-invalid-refused": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`enum Choice {
    A(Int)
    B(Int)
}

fn choose(x: Choice) -> Int {
    if x == {
        A(n) -> { return n }
        else -> { return 0 }
    }
}

fn run() {
    print(choose(Choice.A(1)))
}
`);
    await ctx.openCanvas();
    const before = await ctx.source();
    await ctx.driver.evaluate(`window.prompt = () => "== Missing(n)"`);
    const pos = await ctx.node("if ==");
    await ctx.driver.rightClick(pos.x, pos.y);
    await ctx.expectMenu("Add pattern arm");
    await ctx.pickEntry("Add pattern arm");
    await ctx.expectProblem("E0305");
    const after = await ctx.source();
    if (after !== before) throw new Error(`bad pattern changed source:\n${after}`);
  },

  "multi-input-append-remove": async (ctx) => {
    await ctx.openCanvas();
    await ctx.replaceSource(`fn to_int(n: Int) -> Int {
    return n
}

fn demo() -> Int {
    xs :: [1, 2, 3]
    ys :: to_int.[1, 2]
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

    let item = await ctx.pin("list", "item4");
    await ctx.driver.rightClick(item.x, item.y);
    await ctx.expectMenu("Remove element");
    await ctx.pickEntry("Remove element");
    await ctx.waitFor(async () => (await ctx.source()).includes("[1, 2, 3]") && !(await ctx.source()).includes("[1, 2, 3, 4]"), "list remove");

    await ctx.driver.evaluate(`window.prompt = () => "3"`);
    let fanout = await ctx.node("fanout");
    await ctx.driver.rightClick(fanout.x, fanout.y);
    await ctx.expectMenu("Append input");
    await ctx.pickEntry("Append input");
    await ctx.waitFor(async () => (await ctx.source()).includes("to_int.[1, 2, 3]"), "fanout append");
    await assertSourceSync(ctx, ["fanout append"]);

    item = await ctx.pin("fanout", "item3");
    await ctx.driver.rightClick(item.x, item.y);
    await ctx.expectMenu("Remove element");
    await ctx.pickEntry("Remove element");
    await ctx.waitFor(async () => (await ctx.source()).includes("to_int.[1, 2]") && !(await ctx.source()).includes("to_int.[1, 2, 3]"), "fanout remove");
    await assertSourceSync(ctx, ["fanout remove"]);
  },

  "inline-edit-values": async (ctx) => {
    await ctx.openCanvas();
    await ctx.switchGraph("scratch");
    const { doc, graph, expr } = await scratchLimitInline(ctx);
    await uiEdit(ctx, { schema_version: 1, op: "edit_inline_expr", revision: doc.revision, inline_expr_id: expr.inline_expr_id, new_expr: "limit + 2" }, "inline value edit");
    await ctx.expectSourceContains("print(limit + 2)");
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
    const toast = await ctx.driver.evaluate(`window.__jetCanvasTest.lastToast || ""`);
    if (!toast.includes("Undo: insert abs")) throw new Error(`undo toast did not name operation: ${toast}`);
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

  "graph-source-toggle-preserves-selection": async (ctx) => {
    await ctx.openCanvas();
    await clickSelectDetails(ctx);
    const selected = (await ctx.state()).selectedNodeId;
    await ctx.driver.evaluate(`window.__jetCanvasTest.setViewMode("code")`);
    await ctx.waitFor(async () => await ctx.driver.evaluate(`window.__jetCanvasLensMode === "code"`), "code view");
    await ctx.driver.evaluate(`window.__jetCanvasTest.setViewMode("graph")`);
    await ctx.waitForCanvas();
    const after = await ctx.state();
    if (after.selectedNodeId !== selected) throw new Error(`selection changed across graph/source toggle: ${selected} -> ${after.selectedNodeId}`);
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
          const hasExtra = (await ctx.source()).includes("extra: Int = 1");
          const signature = hasExtra
            ? "fn scratch(limit: Int, text: String, flag: Bool, ratio: Float)"
            : "fn scratch(limit: Int, text: String, flag: Bool, ratio: Float, extra: Int = 1)";
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
