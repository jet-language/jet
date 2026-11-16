
class FakeElement {
  constructor(id) {
    this.id = id;
    this.listeners = new Map();
  }
  addEventListener(name, handler) {
    const list = this.listeners.get(name) ?? [];
    list.push(handler);
    this.listeners.set(name, list);
  }
  dispatchEvent(ev) {
    ev.target = this;
    for (const handler of this.listeners.get(ev.type) ?? []) handler(ev);
  }
}
class FakeDocument {
  constructor() {
    this.button = new FakeElement("go");
  }
  querySelector(sel) {
    return sel === "#go" ? this.button : null;
  }
  getElementById(id) {
    return id === "go" ? this.button : null;
  }
  createElement(tag) {
    return new FakeElement(tag);
  }
}
globalThis.document = new FakeDocument();

const { init } = await import("./app.js");
init();
document.button.dispatchEvent({ type: "click" });
