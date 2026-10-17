
class FakeElement {
  constructor(tag, doc) { this.tagName = tag.toUpperCase(); this.ownerDocument = doc; this.style = {}; this.dataset = {}; this.children = []; this.textContent = ""; this.value = ""; this.id = ""; this.attrs = new Map(); this.parent = null; }
  appendChild(child) { child.parent = this; this.children.push(child); return child; }
  setAttribute(name, value) { this.attrs.set(String(name), String(value)); }
  getAttribute(name) { return this.attrs.get(String(name)) ?? null; }
  removeAttribute(name) { this.attrs.delete(String(name)); }
  focus() { this.ownerDocument.activeElement = this; }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((child) => child !== this); }
}
class FakeDocument {
  constructor() { this.activeElement = null; this._byId = new Map(); this.body = new FakeElement("body", this); }
  createElement(tag) { return new FakeElement(tag, this); }
  getElementById(id) { return this._byId.get(id) ?? null; }
}
const doc = new FakeDocument();
const origAppend = doc.body.appendChild.bind(doc.body);
doc.body.appendChild = (el) => { if (el.id) doc._byId.set(el.id, el); return origAppend(el); };
globalThis.document = doc;

const { render_tree, render_focus_tree, render_two_backends } = await import("./app.js");
render_tree(true);
let container = doc.getElementById("jet-app");
console.log(container.children.map((el) => `${el.tagName}:${el.getAttribute("role")}:${el.getAttribute("aria-label")}`).join("|"));
render_tree(false);
console.log(container.children.map((el) => `${el.tagName}:${el.getAttribute("role")}:${el.getAttribute("aria-label")}`).join("|"));
render_focus_tree();
const second = container.children.find((el) => el.getAttribute("aria-label") === "Cancel");
second.focus();
render_focus_tree();
console.log(`focus=${doc.activeElement === second}:${doc.activeElement?.getAttribute("aria-label")}`);
render_two_backends();
const backendA = container.children.find((el) => el.getAttribute("aria-label") === "Backend A");
backendA.focus();
render_two_backends();
console.log(`scoped=${doc.activeElement === backendA}:${doc.activeElement?.getAttribute("aria-label")}`);
