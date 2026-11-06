// A DOM stub with no browser: the document keeps its listeners and the
// harness fires browser events at them. The page installs no provider, so the
// runtime installs its default one; the program's lines are Core's decoding.
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
const writes = [];
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { clipboard: { writeText: async (text) => { writes.push(text); } } },
});
const notes = { id: "notes" };
const fire = (type, fields = {}) => {
  const event = { type, target: notes, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, ...fields };
  for (const handler of listeners.get(type) ?? []) handler(event);
  return event;
};
const transfer = (data) => ({ dropEffect: "copy", getData: (kind) => data[kind] ?? "" });

const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const instance = await instantiateWasm("./app.wasm");
fire("paste", { clipboardData: { getData: (kind) => (kind === "text/plain" ? "pasted \u241f text" : "") } });
fire("compositionstart", { data: "" });
fire("compositionupdate", { data: "か\u241fな\x1f" });
fire("compositionend", { data: "かな" });
fire("compositionend", { data: "" });
fire("dragenter", { dataTransfer: transfer({}) });
const over = fire("dragover", { dataTransfer: transfer({}) });
const drop = fire("drop", { dataTransfer: transfer({ "text/plain": "hel\u241flo\x1f", "text/uri-list": "# a comment\r\nhttps://jet-lang.dev/\r\n" }) });
fire("dragleave", { dataTransfer: { dropEffect: "none", getData: () => "" } });
instance.exports.jet_export_run();
if (takeWasmError(instance.exports)?.tag !== "Ok") throw new Error("unexpected Wasm outcome");
console.log(`dragover accepted: ${over.defaultPrevented}; drop accepted: ${drop.defaultPrevented}`);
console.log(`navigator.clipboard.writeText: ${JSON.stringify(writes)}`);
