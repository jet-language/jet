// A minimal DOM: elements keep their children, style, text, value and event
// listeners; the body registers an appended element by id.
class Element {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.style = {};
    this.textContent = "";
    this.value = "";
    this.id = "";
    this.parent = null;
    this.listeners = new Map();
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
    if (!this.listeners.has(name)) this.listeners.set(name, []);
    this.listeners.get(name).push(handler);
  }
  // Runs the `type` listeners on one event; whether one prevented its default.
  dispatch(type, fields) {
    let prevented = false;
    const event = { type, target: this, preventDefault: () => { prevented = true; }, ...fields };
    for (const handler of this.listeners.get(type) ?? []) handler(event);
    return prevented;
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
  const rows = root.children.map((child) => `${child.tagName} ${JSON.stringify(child.tagName === "INPUT" ? child.value : child.textContent)} top=${child.style.top} height=${child.style.height}`);
  console.log(`${when}: ${rows.join(" | ")}`);
};
const input = () => root.children.find((child) => child.tagName === "INPUT");
const transfer = (plain, uris) => ({ getData: (format) => (format === "text/plain" ? plain : format === "text/uri-list" ? uris : "") });
show("mounted");
console.log(`dragover accepted: ${input().dispatch("dragover", {})}`);
input().dispatch("drop", { dataTransfer: transfer("hello", "") });
show("after a text drop");
input().dispatch("drop", { dataTransfer: transfer("note", "# picked\nhttps://example.test/a\n\nhttps://example.test/b\n") });
show("after a text and two links");
