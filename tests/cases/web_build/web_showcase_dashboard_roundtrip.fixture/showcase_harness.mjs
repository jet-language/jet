
class FakeElement {
  constructor(tag, doc) { this.tagName = tag; this.ownerDocument = doc; this.style = {}; this.dataset = {}; this.children = []; this.textContent = ""; this.id = ""; this.attrs = new Map(); }
  appendChild(child) { this.children.push(child); return child; }
  setAttribute(name, value) { this.attrs.set(String(name), String(value)); }
  getAttribute(name) { return this.attrs.get(String(name)) ?? null; }
  removeAttribute(name) { this.attrs.delete(String(name)); }
  focus() { this.ownerDocument.activeElement = this; }
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

// The real companion HTML owns this control outside Jet's render tree.
const boostButton = doc.createElement("button");
boostButton.id = "boost-btn";
doc.body.appendChild(boostButton);

const { init_app, init_fuel } = await import("./app.js");
const unwrap = (outcome) => outcome?.tag === "Ok" ? outcome.values[0] : (() => { throw new Error(`unexpected outcome: ${JSON.stringify(outcome)}`); })();
const boosts = unwrap(init_app());
const elapsed = unwrap(init_fuel());

const container = doc.getElementById("jet-app");
const boxes = () => container.children.map((c) => ({ text: c.textContent, bg: c.style.background, color: c.style.color, role: c.getAttribute("role"), aria: c.getAttribute("aria-label") }));
const find = (prefix) => boxes().find((b) => b.text.startsWith(prefix));

console.log(`nodes=${container.children.length}`);
for (const b of boxes()) {
  console.log(`box text=${JSON.stringify(b.text)} bg=${b.bg} color=${b.color} role=${b.role} aria=${JSON.stringify(b.aria)}`);
}

boosts.set(boosts.get() + 1n);
boosts.set(boosts.get() + 1n);
console.log(`boosts: ${find("Boosts").text}`);
boostButton.focus();
boosts.set(boosts.get() + 1n);
console.log(`external-focus=${doc.activeElement === boostButton}`);

for (const t of [0, 150, 300, 450, 600, 900]) {
  elapsed.set(t);
  console.log(`elapsed=${t}: ${find("Fuel").text}`);
}
