// A minimal page for main.jet's build: one button (#go) that keeps its
// listeners, and the page's localStorage. The program's run binds the
// button's click; a click runs the handler, which calls the program's
// exported `double` and writes `clicked`.
class FakeElement {
  constructor(id) {
    this.id = id;
    this.textContent = "";
    this.listeners = new Map();
  }
  addEventListener(name, handler) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }
  // Settles once each listener's run has ended.
  dispatchEvent(event) {
    event.target = this;
    return Promise.all((this.listeners.get(event.type) ?? []).map((handler) => handler(event)));
  }
}
class FakeDocument {
  constructor() {
    this.button = new FakeElement("go");
  }
  addEventListener() {}
  querySelector(selector) {
    return selector === "#go" ? this.button : null;
  }
}
const stored = new Map();
globalThis.document = new FakeDocument();
globalThis.localStorage = {
  getItem: (key) => stored.get(key) ?? null,
  setItem: (key, value) => stored.set(key, String(value)),
  removeItem: (key) => stored.delete(key),
  clear: () => stored.clear(),
};

const { jet_main } = await import("./app.js");
await jet_main();
await document.button.dispatchEvent({ type: "click" });
console.log(`clicked=${stored.get("clicked")}`);
