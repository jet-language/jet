// A minimal DOM: elements keep their children, style, text and click
// listeners; the body registers an appended element by id.
class Element {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.style = {};
    this.textContent = "";
    this.id = "";
    this.parent = null;
    this.clicks = [];
  }
  appendChild(child) {
    child.parent = this;
    this.children.push(child);
    return child;
  }
  remove() {
    if (!this.parent) return;
    this.parent.children = this.parent.children.filter((child) => child !== this);
    this.parent = null;
  }
  addEventListener(name, handler) {
    if (name === "click") this.clicks.push(handler);
  }
  click() {
    for (const handler of this.clicks) handler({ type: "click", target: this });
  }
}
const body = new Element("body");
const byId = new Map();
body.appendChild = (child) => {
  if (child.id) byId.set(child.id, child);
  return Element.prototype.appendChild.call(body, child);
};
globalThis.document = { body, createElement: (tag) => new Element(tag), getElementById: (id) => byId.get(id) ?? null };

const { jet_main } = await import("./app.js");
await jet_main();

const root = document.getElementById("jet-app");
const show = (when) => {
  const rows = root.children.map((child) => `${child.tagName} ${JSON.stringify(child.textContent)} top=${child.style.top} height=${child.style.height}${child.style.background ? " background=" + child.style.background : ""}`);
  console.log(`${when}: ${rows.join(" | ")}`);
};
const button = (label) => root.children.find((child) => child.tagName === "BUTTON" && child.textContent === label);
show("mounted");
for (let i = 0; i < 3; i += 1) button("increment").click();
show("after 3 increments");
button("reset").click();
show("after reset");
