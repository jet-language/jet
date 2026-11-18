// A minimal page for main.jet's build: a body the scene mounts into
// (#jet-app), elements that keep their ARIA role and name, the focus, and
// three controls outside the scene (#plain, #focus, #backends) whose clicks
// run the program's routes, each painting another tree.
class FakeElement {
  constructor(tag, doc) {
    this.tagName = tag.toUpperCase();
    this.ownerDocument = doc;
    this.style = {};
    this.children = [];
    this.textContent = "";
    this.value = "";
    this.id = "";
    this.role = null;
    this.ariaLabel = null;
    this.parent = null;
    this.listeners = new Map();
  }
  appendChild(child) {
    child.parent = this;
    this.children.push(child);
    return child;
  }
  remove() {
    if (this.parent) this.parent.children = this.parent.children.filter((child) => child !== this);
    this.parent = null;
    if (this.ownerDocument.activeElement === this) this.ownerDocument.activeElement = null;
  }
  focus() {
    this.ownerDocument.activeElement = this;
  }
  addEventListener(name, handler) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }
  // Settles once each listener's run has ended.
  click() {
    return Promise.all((this.listeners.get("click") ?? []).map((handler) => handler({ type: "click", target: this })));
  }
}
class FakeDocument {
  constructor() {
    this.activeElement = null;
    this.byId = new Map();
    this.body = new FakeElement("body", this);
    this.controls = new Map(["#plain", "#focus", "#backends"].map((selector) => [selector, new FakeElement("button", this)]));
  }
  addEventListener() {}
  createElement(tag) {
    return new FakeElement(tag, this);
  }
  getElementById(id) {
    return this.byId.get(id) ?? null;
  }
  querySelector(selector) {
    return this.controls.get(selector) ?? null;
  }
}
const doc = new FakeDocument();
const append = doc.body.appendChild.bind(doc.body);
doc.body.appendChild = (element) => {
  if (element.id) doc.byId.set(element.id, element);
  return append(element);
};
globalThis.document = doc;

const shown = (container) => container.children.map((element) => `${element.tagName}:${element.role}:${element.ariaLabel}`).join("|");
const named = (container, name) => container.children.find((element) => element.ariaLabel === name);
const focused = (container) => `${container.children.includes(doc.activeElement)}:${doc.activeElement?.ariaLabel}`;

const { jet_main } = await import("./app.js");
await jet_main();
const container = doc.getElementById("jet-app");
console.log(shown(container));
await doc.controls.get("#plain").click();
console.log(shown(container));
await doc.controls.get("#focus").click();
named(container, "Cancel").focus();
await doc.controls.get("#focus").click();
console.log(`focus=${focused(container)}`);
await doc.controls.get("#backends").click();
console.log(shown(container));
