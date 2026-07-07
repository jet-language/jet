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

export function enterRenderScope(name) {
  if (jetDomScopeDepth === 0) {
    jetDomScopeName = name;
    jetDomScopeCounter = 0;
  }
  jetDomScopeDepth++;
}

export function exitRenderScope() {
  jetDomScopeDepth = Math.max(0, jetDomScopeDepth - 1);
}

export function createBackend() {
  const boxKey = `${jetDomScopeName}#${jetDomScopeCounter++}`;
  return { kind: "dom", commands: [], root: jetDomContainer(), boxKey };
}

export function measure(node, constraint) {
  const width = Math.min(Math.max(node.width, constraint.minWidth), constraint.maxWidth);
  const height = Math.min(Math.max(node.height, constraint.minHeight), constraint.maxHeight);
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
  const frame = backend.frame ?? { x: 0, y: 0, width: node.width, height: node.height };
  const fillColor = node.color ?? "#000000";
  const fill = `fill({x:${frame.x},y:${frame.y},w:${frame.width},h:${frame.height}}, ${fillColor})`;
  const text = `text({x:${frame.x},y:${frame.y},w:${frame.width},h:${frame.height}}, ${node.label})`;
  backend.commands.push(fill);
  backend.commands.push(text);
  if (backend.root) {
    // Looked up by the backend's scope-derived `boxKey` (see
    // `createBackend`/`enterRenderScope` above), not by backend-object
    // identity and not by a global `[data-jet-node]` query — that's what
    // lets several `ui.null_backend()` instances created within one call
    // paint as several distinct, independently updating DOM elements, while
    // a backend re-created by a repeated call to the same entry point still
    // resolves to the same, reused element.
    let box = jetDomBoxRegistry.get(backend.boxKey);
    if (!box) {
      box = document.createElement("div");
      box.dataset.jetNode = "1";
      box.style.position = "absolute";
      box.style.boxSizing = "border-box";
      box.style.border = "1px solid rgba(0,0,0,0.15)";
      box.style.borderRadius = "6px";
      box.style.font = "14px system-ui, sans-serif";
      box.style.display = "flex";
      box.style.alignItems = "center";
      box.style.justifyContent = "center";
      backend.root.appendChild(box);
      jetDomBoxRegistry.set(backend.boxKey, box);
    }
    box.style.left = `${frame.x}px`;
    box.style.top = `${frame.y}px`;
    box.style.width = `${frame.width}px`;
    box.style.height = `${frame.height}px`;
    box.style.background = fillColor;
    box.style.color = jetReadableTextColor(fillColor);
    box.textContent = node.label;
  }
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
  return "Handled";
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

export function on(selector, eventName, handler) {
  const el = query(selector);
  if (!el || !el.addEventListener) return "Missing";
  el.addEventListener(String(eventName), (ev) => handler(normalizeEvent(ev)));
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
  return { label: String(label), width, height, color: color != null ? String(color) : undefined };
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

// D-RENDERTGT2=A carried into JS (Phase 7 web/DOM backend): a minimal
// reactive runtime mirroring the Rust `JetSignal`/`jet_reactive_effect`
// prelude (crates/jet-codegen/src/Prelude/CoreLib.rs). An "observer" is the
// closure currently (re)running; reading a signal while an observer is
// active subscribes it; `set` re-runs every subscriber synchronously.
const jetReactiveObservers = [];

function jetReactiveActiveObserver() {
  return jetReactiveObservers.length > 0
    ? jetReactiveObservers[jetReactiveObservers.length - 1]
    : null;
}

/** D-RENDERTGT2=A: `reactive.signal(initial)` → a `{ get, set }` cell. */
export function makeSignal(initial) {
  const cell = { value: initial, subs: [] };
  return {
    get() {
      const obs = jetReactiveActiveObserver();
      if (obs && !cell.subs.includes(obs)) {
        cell.subs.push(obs);
      }
      return cell.value;
    },
    set(value) {
      cell.value = value;
      const subs = cell.subs.slice();
      for (const sub of subs) {
        sub();
      }
    },
  };
}

/**
 * D-RENDERTGT2=A: `ui.reactive_render(() => { ... })` — run `body` now, and
 * again whenever a signal it read changes. Mirrors `jet_reactive_effect`.
 */
export function reactiveRender(body) {
  const observer = () => {
    jetReactiveObservers.push(observer);
    try {
      body();
    } finally {
      jetReactiveObservers.pop();
    }
  };
  observer();
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
