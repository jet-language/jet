// D-DOMGEN1=A (c123 M2): first-party DOM/runtime shim for generated web JS.
// Generated `app.js` imports these primitives; the loader wires WASM exports.

export function print(value) {
  const text = String(value);
  if (typeof console !== "undefined" && console.log) {
    console.log(text);
  }
  return text;
}

// D-DOMGEN1=A (Phase 7 extension): when a real `document` is available (the
// generated app is running in a browser, not under `node`'s golden/roundtrip
// tests), the DOM backend also mounts real elements under a fixed-id
// container so `jet build --target web` output is an actually-viewable page,
// not just an in-memory command log. Under `node` (no `document`), this is a
// no-op and behavior is byte-identical to before — the existing snapshot
// tests never touch a browser.
function jetDomContainer() {
  if (typeof document === "undefined") return null;
  let el = document.getElementById("jet-app");
  if (!el) {
    el = document.createElement("div");
    el.id = "jet-app";
    el.style.position = "relative";
    document.body.appendChild(el);
  }
  return el;
}

// D-UISHOWCASE1 (c134 Phase 8): stable per-node DOM identity without adding
// any argument to the Jet-level `ui.null_backend()` call (I7/I8 — no new
// language surface for what's purely a codegen bookkeeping concern). Every
// exported top-level `#Js` function's generated body is wrapped in
// `enterRenderScope(name)` / `exitRenderScope()` (see Web.rs's `emit_js_fn`).
// `enterRenderScope` only resets the scope name + counter when call depth is
// 0 — i.e. only for the OUTERMOST exported call, not for a shared helper
// (also exported, since every `#Js` function is) invoked *from* that outer
// call. `createBackend()` then stamps each new backend with
// `"{scope}#{ordinal}"`, and `paint()` looks its box up in a scope-keyed
// registry instead of caching one element per backend OBJECT. That's what
// makes both real-world shapes work with the same mechanism:
//   - 196_ui_web_click.jet's `render(n)`, called repeatedly by a click
//     handler: each call resets to the same first key ("render#0") ->
//     the one box is found and updated in place, never duplicated.
//   - 197_ui_showcase.jet's `initApp()`, called once, painting several
//     distinct cards via a shared `paint_stat_card` helper: each nested
//     `createBackend()` call increments the *same* "initApp" scope's
//     counter ("initApp#0", "initApp#1", …) -> each card gets its own box.
let jetDomScopeName = "__top__";
let jetDomScopeCounter = 0;
let jetDomScopeDepth = 0;
const jetDomBoxRegistry = new Map();
let jetDomTouchedBackends = new Set();

export function perfNow() {
  return typeof performance === "undefined" ? 0 : performance.now();
}

export function perfRecord(symbol, eventClass, started) {
  globalThis.__jetPerfRecord?.(String(symbol), String(eventClass), started);
}

export function enterRenderScope(name) {
  if (jetDomScopeDepth === 0) {
    jetDomScopeName = name;
    jetDomScopeCounter = 0;
    jetDomTouchedBackends = new Set();
  }
  jetDomScopeDepth++;
}

export function exitRenderScope() {
  jetDomScopeDepth = Math.max(0, jetDomScopeDepth - 1);
  if (jetDomScopeDepth === 0) {
    const prefix = `${jetDomScopeName}#`;
    for (const [key, record] of jetDomBoxRegistry) {
      const backendKey = key.split("/")[0];
      if (key.startsWith(prefix) && !jetDomTouchedBackends.has(backendKey)) {
        record.element?.remove?.();
        jetDomBoxRegistry.delete(key);
      }
    }
  }
}

export function createBackend() {
  const boxKey = `${jetDomScopeName}#${jetDomScopeCounter++}`;
  jetDomTouchedBackends.add(boxKey);
  return {
    kind: "dom",
    commands: [],
    root: jetDomContainer(),
    boxKey,
    focusNodes: [],
    focusedIndex: -1,
  };
}

export function measure(node, constraint) {
  const naturalWidth = node.kind === "box"
    ? node.children.reduce((width, child) => Math.max(width, child.width), 0)
    : node.width;
  const naturalHeight = node.kind === "box"
    ? node.children.reduce((height, child) => height + child.height, 0)
    : node.height;
  const width = Math.min(Math.max(naturalWidth, constraint.minWidth), constraint.maxWidth);
  const height = Math.min(Math.max(naturalHeight, constraint.minHeight), constraint.maxHeight);
  return { width, height };
}

export function layout(backend, node, frame) {
  backend.frame = frame;
  backend.node = node;
  return frame;
}

// D-STYLESHAPE1=A wiring: pick readable text color (WCAG-style relative
// luminance threshold) so a dark fill gets light text and vice versa,
// instead of hardcoding one text color regardless of the node's fill.
function jetReadableTextColor(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex || "");
  if (!m) return "#111";
  const n = parseInt(m[1], 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.6 ? "#111" : "#fff";
}

export function paint(backend, node) {
  const perfStarted = perfNow();
  const frame = backend.frame ?? { x: 0, y: 0, width: node.width, height: node.height };
  const live = new Set();
  const activeElement = typeof document !== "undefined"
    ? document.activeElement ?? null
    : null;
  const activeKey = activeElement?.dataset?.jetKey ?? null;
  const backendPrefix = `${backend.boxKey}/`;
  const activeInBackend = activeKey !== null
    && (activeKey === backend.boxKey || activeKey.startsWith(backendPrefix));
  const externalActive = activeElement !== null
    && activeElement !== document.body
    && activeKey === null;
  const focusedKey = (activeInBackend ? activeKey : null)
    ?? backend.focusNodes[backend.focusedIndex]?.dataset?.jetKey
    ?? null;
  backend.focusNodes = [];
  const render = (current, currentFrame, path) => {
    if (current.kind === "box") {
      let y = currentFrame.y;
      current.children.forEach((child, index) => {
        render(child, { x: currentFrame.x, y, width: currentFrame.width, height: child.height }, `${path}/${index}`);
        y += child.height;
      });
      return;
    }
    const fillColor = current.color ?? "#000000";
    if (current.kind !== "text") {
      backend.commands.push(`fill({x:${currentFrame.x},y:${currentFrame.y},w:${currentFrame.width},h:${currentFrame.height}}, ${fillColor})`);
    }
    backend.commands.push(`text({x:${currentFrame.x},y:${currentFrame.y},w:${currentFrame.width},h:${currentFrame.height}}, ${current.label})`);
    live.add(path);
    if (!backend.root) return;
    const tag = current.kind === "button" ? "button" : current.kind === "textInput" ? "input" : current.kind === "text" ? "span" : "div";
    let record = jetDomBoxRegistry.get(path);
    if (record && record.tag !== tag) {
      record.element?.remove?.();
      jetDomBoxRegistry.delete(path);
      record = null;
    }
    let box = record?.element;
    if (!box) {
      box = document.createElement(tag);
      box.dataset.jetNode = "1";
      box.dataset.jetKey = path;
      box.style.position = "absolute";
      box.style.boxSizing = "border-box";
      box.style.border = "1px solid rgba(0,0,0,0.15)";
      box.style.borderRadius = "6px";
      box.style.font = "14px system-ui, sans-serif";
      box.style.display = "flex";
      box.style.alignItems = "center";
      box.style.justifyContent = "center";
      backend.root.appendChild(box);
      jetDomBoxRegistry.set(path, { element: box, tag });
    }
    box.style.left = `${currentFrame.x}px`;
    box.style.top = `${currentFrame.y}px`;
    box.style.width = `${currentFrame.width}px`;
    box.style.height = `${currentFrame.height}px`;
    box.style.background = current.kind === "text" ? "transparent" : fillColor;
    box.style.color = jetReadableTextColor(fillColor);
    if (tag === "input") box.value = current.label;
    else box.textContent = current.label;
    if (current.role) {
      box.setAttribute?.("role", current.role);
      box.setAttribute?.("aria-label", current.label);
    } else {
      box.removeAttribute?.("role");
      box.removeAttribute?.("aria-label");
    }
    if (current.role === "button" || current.role === "textbox") {
      backend.focusNodes.push(box);
    }
  };
  render(node, frame, backend.boxKey);
  if (backend.root) {
    const prefix = `${backend.boxKey}/`;
    for (const [key, record] of jetDomBoxRegistry) {
      if ((key === backend.boxKey || key.startsWith(prefix)) && !live.has(key)) {
        record.element?.remove?.();
        jetDomBoxRegistry.delete(key);
      }
    }
  }
  const preservedIndex = focusedKey === null
    ? -1
    : backend.focusNodes.findIndex((element) => element.dataset?.jetKey === focusedKey);
  backend.focusedIndex = preservedIndex >= 0
    ? preservedIndex
    : backend.focusNodes.length ? 0 : -1;
  if (backend.focusedIndex >= 0 && (activeInBackend || (!externalActive && activeKey === null))) {
    backend.focusNodes[backend.focusedIndex].focus?.();
  }
  perfRecord(jetDomScopeName, "dom", perfStarted);
  return backend;
}

export function commands(backend) {
  return backend.commands.slice();
}

export function onEvent(backend, event) {
  if (event.kind === "key" && event.code === "") {
    return "Ignored";
  }
  if (event.kind === "resize" && (event.width <= 0 || event.height <= 0)) {
    return "Ignored";
  }
  if (event.kind === "key" && event.code === "Tab" && backend.focusNodes.length) {
    backend.focusedIndex = (backend.focusedIndex + 1) % backend.focusNodes.length;
    backend.focusNodes[backend.focusedIndex].focus?.();
    return "Handled";
  }
  return "Handled";
}

export function setFocusGroup(backend, nodes) {
  const labels = nodes.filter((node) => node.role === "button" || node.role === "textbox").map((node) => node.label);
  backend.focusNodes = Array.from(backend.root?.children ?? []).filter((element) => labels.includes(element.getAttribute?.("aria-label") ?? element.textContent));
  backend.focusedIndex = backend.focusNodes.length ? 0 : -1;
  backend.focusNodes[0]?.focus?.();
}

export function focusedLabel(backend) {
  if (backend.focusedIndex < 0) return "";
  const element = backend.focusNodes[backend.focusedIndex];
  return String(element?.getAttribute?.("aria-label") ?? element?.textContent ?? "");
}

function query(selector) {
  if (typeof document === "undefined" || !document.querySelector) return null;
  return document.querySelector(String(selector));
}

function normalizeEvent(ev) {
  return {
    kind: String(ev?.type ?? ""),
    key: String(ev?.key ?? ""),
    code: String(ev?.code ?? ""),
    value: ev?.target && "value" in ev.target ? String(ev.target.value ?? "") : "",
    checked: !!(ev?.target && ev.target.checked),
  };
}

export function on(selector, eventName, handler, symbol) {
  const el = query(selector);
  if (!el || !el.addEventListener) return "Missing";
  const handlerSymbol = symbol || jetDomScopeName;
  el.addEventListener(String(eventName), (ev) => {
    const started = perfNow();
    try {
      const result = handler(normalizeEvent(ev));
      if (result && typeof result.finally === "function") {
        return result.finally(() => perfRecord(handlerSymbol, "event", started));
      }
      perfRecord(handlerSymbol, "event", started);
      return result;
    } catch (error) {
      perfRecord(handlerSymbol, "event", started);
      throw error;
    }
  });
  return "Bound";
}

export function value(selector) {
  const el = query(selector);
  if (!el) return "";
  if ("value" in el) return String(el.value ?? "");
  return String(el.textContent ?? "");
}

const fallbackStorage = { local: new Map(), session: new Map() };

function storage(kind) {
  const name = kind === "session" ? "sessionStorage" : "localStorage";
  const candidate = globalThis?.[name];
  if (
    candidate &&
    typeof candidate.getItem === "function" &&
    typeof candidate.setItem === "function" &&
    typeof candidate.removeItem === "function"
  ) {
    return candidate;
  }
  const map = fallbackStorage[kind === "session" ? "session" : "local"];
  return {
    getItem(key) {
      key = String(key);
      return map.has(key) ? map.get(key) : null;
    },
    setItem(key, value) {
      map.set(String(key), String(value));
    },
    removeItem(key) {
      map.delete(String(key));
    },
    clear() {
      map.clear();
    },
  };
}

export function storageGet(kind, key) {
  const value = storage(kind).getItem(String(key));
  return value == null ? null : String(value);
}

export function storageSet(kind, key, value) {
  storage(kind).setItem(String(key), String(value));
  return null;
}

export function storageRemove(kind, key) {
  storage(kind).removeItem(String(key));
  return null;
}

export function storageClear(kind) {
  storage(kind).clear();
  return null;
}

export function makeNode(label, width, height, color) {
  return {
    kind: "custom",
    label: String(label),
    width,
    height,
    color: color != null ? String(color) : undefined,
    role: color != null ? "label" : null,
    children: [],
  };
}

export function makeNodeRole(label, width, height, role) {
  return { kind: role === "button" ? "button" : role === "textbox" ? "textInput" : "custom", label: String(label), width, height, role, children: [] };
}

export function makeText(text) {
  const label = String(text);
  return { kind: "text", label, width: Array.from(label).length, height: 1, role: "label", children: [] };
}

export function makeButton(text) {
  const label = String(text);
  return { kind: "button", label, width: Array.from(label).length + 4, height: 1, role: "button", children: [] };
}

export function makeBox(children) {
  const list = Array.from(children ?? []);
  return {
    kind: "box",
    label: "",
    width: list.reduce((width, child) => Math.max(width, child.width), 0),
    height: list.reduce((height, child) => height + child.height, 0),
    role: "group",
    children: list,
  };
}

export function makeConstraint(minW, minH, maxW, maxH) {
  return { minWidth: minW, minHeight: minH, maxWidth: maxW, maxHeight: maxH };
}

export function makeRect(x, y, width, height) {
  return { x, y, width, height };
}

export function makeKeyEvent(code) {
  return { kind: "key", code: String(code) };
}

export function makeResizeEvent(width, height) {
  return { kind: "resize", width, height };
}

export function ariaRoleButton() { return "button"; }
export function ariaRoleTextInput() { return "textbox"; }
export function ariaRoleLabel() { return "label"; }
export function ariaRoleContainer() { return "group"; }

// D-RENDERTGT2=A carried into JS (Phase 7 web/DOM backend): a minimal
// reactive runtime mirroring the Rust `JetSignal`/`jet_reactive_effect`
// prelude (crates/jet-codegen/src/Prelude/CoreLib.rs). An "observer" is the
// closure currently (re)running; reading a signal while an observer is
// active subscribes it; `set` re-runs every subscriber synchronously.
const jetReactiveObservers = [];
const jetReactiveRootEffects = new Set();
let jetReactiveNextObserver = 1;

function jetReactiveActiveObserver() {
  return jetReactiveObservers.length > 0
    ? jetReactiveObservers[jetReactiveObservers.length - 1]
    : null;
}

/** D-RENDERTGT2=A: `reactive.signal(initial)` → a `{ get, set }` cell. */
export function makeSignal(initial) {
  const cell = { value: initial, subs: new Map() };
  return {
    get() {
      const obs = jetReactiveActiveObserver();
      if (obs && !cell.subs.has(obs.id)) {
        cell.subs.set(obs.id, new WeakRef(obs));
        obs.dependencies.add(cell);
      }
      return cell.value;
    },
    set(value) {
      cell.value = value;
      for (const [id, weak] of Array.from(cell.subs)) {
        const sub = weak.deref();
        if (sub) sub.run();
        else cell.subs.delete(id);
      }
    },
  };
}

/**
 * D-RENDERTGT2=A: `ui.reactive_render(() => { ... })` — run `body` now, and
 * again whenever a signal it read changes. Mirrors `jet_reactive_effect`.
 */
export function makeEffect(body) {
  const observer = {
    id: jetReactiveNextObserver++,
    active: true,
    running: false,
    body,
    dependencies: new Set(),
    run() {
      if (!this.active || this.running) return;
      this.running = true;
      for (const cell of this.dependencies) cell.subs.delete(this.id);
      this.dependencies.clear();
      jetReactiveObservers.push(this);
      try {
        this.body();
      } finally {
        jetReactiveObservers.pop();
        this.running = false;
      }
    },
  };
  observer.run();
  return {
    unsubscribe() {
      if (!observer.active) return;
      observer.active = false;
      for (const cell of observer.dependencies) cell.subs.delete(observer.id);
      observer.dependencies.clear();
      observer.body = null;
    },
    isActive() {
      return observer.active;
    },
  };
}

/** Runtime-owned rendering effect; public `reactive.effect` returns its handle. */
export function reactiveRender(body) {
  jetReactiveRootEffects.add(makeEffect(body));
}

export async function instantiateWasm(wasmPath, imports = {}) {
  if (typeof WebAssembly === "undefined") {
    throw new Error("WebAssembly is not available in this runtime");
  }
  const source = await loadBytes(wasmPath);
  const { instance } = await WebAssembly.instantiate(source, { env: imports });
  return instance;
}

/** D-JSBIND1=A: marshal ABI-safe values at the JS/WASM boundary. */
export function marshalAbi(value, kind) {
  if (kind === "list-int") {
    return Array.isArray(value) ? value.map((x) => Number(x)) : [];
  }
  if (kind === "struct-point") {
    return { x: Number(value?.x ?? 0), y: Number(value?.y ?? 0) };
  }
  return value;
}

/** D-JSBIND1=A: read ABI-safe return values from WASM. */
export function unmarshalAbi(value, kind) {
  if (kind === "list-int") {
    return Array.isArray(value) ? value.map((x) => Number(x)) : [];
  }
  if (typeof value === "bigint") {
    return Number(value);
  }
  return value;
}

async function loadBytes(path) {
  if (typeof process !== "undefined" && process.versions?.node) {
    const fs = await import("node:fs");
    return fs.readFileSync(path);
  }
  const response = await fetch(path);
  if (!response.ok) {
    throw new Error(`failed to load wasm module: ${path}`);
  }
  return response.arrayBuffer();
}
