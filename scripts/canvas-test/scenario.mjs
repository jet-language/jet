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
    const p = await this.pin(nodeTitle, pinName);
    await this.driver.rightClick(p.x, p.y);
    await sleep(120);
  }

  async menuOpen() {
    return await this.driver.evaluate(`(() => {
      const menu = document.getElementById("context-menu");
      return !!menu && menu.classList.contains("is-open");
    })()`);
  }

  async loadCoreCatalog() {
    await this.driver.evaluate(`(() => {
      const b = document.getElementById("core-catalog");
      if (!b) throw new Error("core catalog button missing");
      b.click();
      return true;
    })()`);
    await this.waitFor(async () => {
      return await this.driver.evaluate("Number(window.__jetCanvasCoreCatalogPalette || 0) > 0");
    }, "Core catalog palette");
    await this.driver.press("Escape");
    await sleep(120);
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
        return !!menu && menu.classList.contains("is-open") && menu.textContent.includes(${JSON.stringify(text)});
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
    if (!ok) throw new Error(`menu entry not found: ${text}`);
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
    if (!body.includes(text)) throw new Error(`source missing ${JSON.stringify(text)}\n${body}`);
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
    await ctx.openPinActionMenu("total", "output");
    await ctx.expectMenu("Search actions");
    await ctx.type("abs");
    await ctx.expectMenu("abs");
    await ctx.pickEntry("abs");
    await ctx.expectSourceContains("use core.math as math");
    await ctx.expectSourceContains("math.abs");
    await ctx.screenshot("core-abs-inserted");
  },

  "undo-restores-source": async (ctx) => {
    await ctx.openCanvas();
    const before = await ctx.driver.evaluate(`fetch("/canvas/source", { cache: "no-store" }).then((r) => r.text())`);
    await ctx.loadCoreCatalog();
    await ctx.openPinActionMenu("total", "output");
    await ctx.type("abs");
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
