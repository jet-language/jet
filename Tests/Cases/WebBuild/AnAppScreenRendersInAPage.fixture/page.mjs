// A minimal page at `/` for main.jet's build: elements keep their children,
// style, text and event listeners, the body registers an appended element
// by id, and the document takes listeners, which never hear anything, so
// the host gives the page its default UI provider. Once the program's run
// returned, the page shows what launch drew, then clicks Add and shows what
// the app drew again.
class Element {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.style = {};
    this.textContent = "";
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
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }
  // Runs the `type` listeners on one event; settles once each handler's run
  // has ended.
  async dispatch(type) {
    const event = { type, target: this, preventDefault() {} };
    await Promise.all((this.listeners.get(type) ?? []).map((handler) => handler(event)));
  }
}
const body = new Element("body");
const byId = new Map();
body.appendChild = (child) => {
  if (child.id) byId.set(child.id, child);
  return Element.prototype.appendChild.call(body, child);
};
globalThis.document = {
  body,
  visibilityState: "visible",
  addEventListener() {},
  createElement: (tag) => new Element(tag),
  getElementById: (id) => byId.get(id) ?? null,
};
globalThis.location = { pathname: "/", search: "" };

const { jet_main } = await import("./app.js");
await jet_main();
const root = () => document.getElementById("jet-app");
const show = (when) => {
  const shown = root()?.children.map((child) => `${child.tagName} ${JSON.stringify(child.textContent)}`) ?? [];
  console.log(`${when}: ${shown.length === 0 ? "nothing" : shown.join(" | ")}`);
};
show("after launch");
await root()
  .children.find((child) => child.textContent === "Add")
  .dispatch("click");
show("after a click on Add");
