
class FakeElement {
  constructor(tag) { this.tagName = tag; this.style = {}; this.dataset = {}; this.children = []; this.textContent = ""; this.id = ""; this.listeners = new Map(); }
  appendChild(child) { this.children.push(child); return child; }
  addEventListener(name, handler) { const list = this.listeners.get(name) ?? []; list.push(handler); this.listeners.set(name, list); }
  click() { for (const handler of this.listeners.get("click") ?? []) handler({ type: "click", target: this }); }
  querySelector(sel) {
    if (sel === "[data-jet-node]") return this.children.find((c) => c.dataset.jetNode) ?? null;
    return null;
  }
}
class FakeDocument {
  constructor() { this.body = new FakeElement("body"); this._byId = new Map(); }
  createElement(tag) { return new FakeElement(tag); }
  getElementById(id) { return this._byId.get(id) ?? null; }
}
const doc = new FakeDocument();
const origAppend = doc.body.appendChild.bind(doc.body);
doc.body.appendChild = (el) => { if (el.id) doc._byId.set(el.id, el); return origAppend(el); };
globalThis.document = doc;

const jetDom = await import("./jet_dom_runtime.js");
const measuredButton = jetDom.measure(
  jetDom.makeButton("Add one"),
  jetDom.makeConstraint(0, 0, 320, 150),
);
if (measuredButton.height <= 2) throw new Error(`button measurement collapsed: ${measuredButton.height}`);

const { jet_main } = await import("./app.js");
await jet_main();
const container = doc.getElementById("jet-app");
const text = () => container.children.map((child) => child.textContent).join("");
const button = container.children.find((child) => child.tagName === "button");
if (button.style.height !== `${measuredButton.height}px`) {
  throw new Error(`button frame height drifted: ${button.style.height}`);
}
console.log(`initial: children=${container.children.length} text=${JSON.stringify(text())}`);
button.click();
console.log(`after click: children=${container.children.length} text=${JSON.stringify(text())}`);
