// A minimal page for web_api.jet's build: one input (#new-task) that keeps
// its listeners, and the page's localStorage. The program's run binds its
// route and writes `tasks`; typing in the input runs the route's handler,
// which reads the input and writes `draft`.
class FakeElement {
  constructor(id) {
    this.id = id;
    this.value = "";
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
    this.input = new FakeElement("new-task");
  }
  addEventListener() {}
  querySelector(selector) {
    return selector === "#new-task" ? this.input : null;
  }
}
class FakeStorage {
  constructor() {
    this.map = new Map();
  }
  getItem(key) {
    return this.map.has(String(key)) ? this.map.get(String(key)) : null;
  }
  setItem(key, value) {
    this.map.set(String(key), String(value));
  }
  removeItem(key) {
    this.map.delete(String(key));
  }
  clear() {
    this.map.clear();
  }
}
globalThis.document = new FakeDocument();
globalThis.localStorage = new FakeStorage();

const { jet_main } = await import("./app.js");
await jet_main();
console.log(`tasks=${localStorage.getItem("tasks")}`);
document.input.value = "write flagship slice";
await document.input.dispatchEvent({ type: "input" });
console.log(`draft=${localStorage.getItem("draft")}`);
