class FakeElement {
  constructor(tag) {
    this.tagName = tag;
    this.style = {};
    this.dataset = {};
    this.children = [];
    this.textContent = "";
    this.id = "";
    this.attributes = new Map();
    this.listeners = new Map();
    this.parentNode = null;
  }
  appendChild(child) {
    child.parentNode = this;
    this.children.push(child);
    return child;
  }
  remove() {
    if (!this.parentNode) return;
    this.parentNode.children = this.parentNode.children.filter((child) => child !== this);
  }
  addEventListener(name, handler) {
    const handlers = this.listeners.get(name) ?? [];
    handlers.push(handler);
    this.listeners.set(name, handlers);
  }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  removeAttribute(name) { this.attributes.delete(name); }
  focus() {}
}
class FakeDocument {
  constructor() {
    this.body = new FakeElement("body");
    this.body.ownerDocument = this;
    this.activeElement = null;
    this._byId = new Map();
  }
  createElement(tag) {
    const element = new FakeElement(tag);
    element.ownerDocument = this;
    return element;
  }
  getElementById(id) { return this._byId.get(id) ?? null; }
}
const document = new FakeDocument();
const appendBody = document.body.appendChild.bind(document.body);
document.body.appendChild = (element) => {
  if (element.id) document._byId.set(element.id, element);
  return appendBody(element);
};
globalThis.document = document;

const { jet_main } = await import("./app.js");
await jet_main();
const root = document.getElementById("jet-app");
const text = root?.children.map((child) => child.textContent).join("") ?? "";
if (!text.includes("hello, world")) {
  throw new Error(`expected rendered greeting, got ${JSON.stringify(text)}`);
}
console.log(text);
