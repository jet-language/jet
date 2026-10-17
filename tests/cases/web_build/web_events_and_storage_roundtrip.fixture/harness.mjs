
class FakeElement {
  constructor(id) {
    this.id = id;
    this.value = "";
    this.textContent = "";
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
    this.input = new FakeElement("new-task");
  }
  querySelector(sel) {
    if (sel === "#new-task") return this.input;
    return null;
  }
  getElementById(id) {
    return id === "new-task" ? this.input : null;
  }
  createElement(tag) {
    return new FakeElement(tag);
  }
}
class FakeStorage {
  constructor() { this.map = new Map(); }
  getItem(key) { return this.map.has(String(key)) ? this.map.get(String(key)) : null; }
  setItem(key, value) { this.map.set(String(key), String(value)); }
  removeItem(key) { this.map.delete(String(key)); }
  clear() { this.map.clear(); }
}
globalThis.document = new FakeDocument();
globalThis.localStorage = new FakeStorage();

const { init } = await import("./app.js");
init();
console.log(`tasks=${localStorage.getItem("tasks")}`);
document.input.value = "write flagship slice";
document.input.dispatchEvent({ type: "input" });
console.log(`draft=${localStorage.getItem("draft")}`);
