// A DOM stub with no browser: the runtime's default provider listens on the
// document. The page registers its shortcuts, and the user presses keys just
// before the page first polls the shortcut queue: the harness observes that
// first poll on the default provider and fires the key presses there.
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
const editor = { id: "editor" };
const press = (key, modifiers = {}) => {
  const event = {
    type: "keydown",
    target: editor,
    key,
    ctrlKey: !!modifiers.ctrl,
    altKey: !!modifiers.alt,
    shiftKey: !!modifiers.shift,
    metaKey: !!modifiers.meta,
    defaultPrevented: false,
    preventDefault() { this.defaultPrevented = true; },
  };
  for (const handler of listeners.get("keydown") ?? []) handler(event);
  return event;
};

const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const instance = await instantiateWasm("./app.wasm");
const provider = globalThis.jetUiProvider;
const eventPop = provider.eventPop;
let pressed = null;
provider.eventPop = (queue) => {
  if (queue === 2 && pressed === null) {
    pressed = [
      ["ctrl+s", press("s", { ctrl: true })],
      ["meta+s", press("s", { meta: true })],
      ["alt+shift+K", press("K", { alt: true, shift: true })],
      ["ctrl+p", press("p", { ctrl: true })],
      ["s", press("s")],
    ];
  }
  return eventPop(queue);
};
instance.exports.jet_export_run();
if (takeWasmError(instance.exports)?.tag !== "Ok") throw new Error("unexpected Wasm outcome");
for (const [name, event] of pressed ?? []) console.log(`${name} default prevented: ${event.defaultPrevented}`);
