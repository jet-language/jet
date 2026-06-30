// D-DOMGEN1=A (c123 M2): first-party DOM/runtime shim for generated web JS.
// Generated `app.js` imports these primitives; the loader wires WASM exports.

export function print(value) {
  const text = String(value);
  if (typeof console !== "undefined" && console.log) {
    console.log(text);
  }
  return text;
}

export function createBackend() {
  return { kind: "dom", commands: [] };
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

export function paint(backend, node) {
  const frame = backend.frame ?? { x: 0, y: 0, width: node.width, height: node.height };
  const fill = `fill({x:${frame.x},y:${frame.y},w:${frame.width},h:${frame.height}}, #000000)`;
  const text = `text({x:${frame.x},y:${frame.y},w:${frame.width},h:${frame.height}}, ${node.label})`;
  backend.commands.push(fill);
  backend.commands.push(text);
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

export function makeNode(label, width, height) {
  return { label: String(label), width, height };
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
