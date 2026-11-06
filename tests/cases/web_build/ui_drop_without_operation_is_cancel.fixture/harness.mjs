// A DOM stub with no browser: the runtime's default provider listens on the
// document, and the harness fires drag/drop events at it before the run.
const listeners = new Map();
const body = { children: [], appendChild(child) { this.children.push(child); return child; } };
globalThis.document = {
  body,
  createElement: (tag) => ({ tagName: tag.toUpperCase(), style: {}, children: [], addEventListener() {}, appendChild() {} }),
  getElementById: () => null,
  addEventListener(type, handler) {
    listeners.set(type, [...(listeners.get(type) ?? []), handler]);
  },
};
const zone = { id: "zone" };
const fire = (type, dropEffect, text = "") => {
  const event = { type, target: zone, preventDefault() {}, dataTransfer: { dropEffect, getData: (kind) => (kind === "text/plain" ? text : "") } };
  for (const handler of listeners.get(type) ?? []) handler(event);
};

const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const instance = await instantiateWasm("./app.wasm");
fire("dragenter", "none");
fire("drop", "none", "refused");
fire("dragenter", "move");
fire("drop", "move", "kept");
fire("dragleave", "none");
instance.exports.jet_export_run();
if (takeWasmError(instance.exports)?.tag !== "Ok") throw new Error("unexpected Wasm outcome");
