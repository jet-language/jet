// A minimal DOM: elements keep their children, text and click listeners; the
// body registers an appended element by id.
class Element {
  constructor(tag) {
    this.tagName = tag.toLowerCase();
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
const container = document.getElementById("jet-app");
const text = () => container.children.map((child) => child.textContent).join("");
console.log(`initial: children=${container.children.length} text=${JSON.stringify(text())}`);
container.children.find((child) => child.tagName === "button").click();
console.log(`after click: children=${container.children.length} text=${JSON.stringify(text())}`);
