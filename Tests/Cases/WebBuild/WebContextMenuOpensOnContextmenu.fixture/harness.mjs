// A minimal DOM: elements keep their children, text, attributes and event
// listeners; the body registers an appended element by id, and the document
// keeps its capturing listeners.
class Element {
  constructor(tag) {
    this.tagName = tag.toLowerCase();
    this.children = [];
    this.style = {};
    this.textContent = "";
    this.id = "";
    this.parent = null;
    this.disabled = false;
    this.attributes = new Map();
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
  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }
  getAttribute(name) {
    return this.attributes.get(name) ?? null;
  }
  contains(other) {
    for (let at = other; at; at = at.parent) if (at === this) return true;
    return false;
  }
  addEventListener(name, handler) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }
  // Dispatches `name` as a browser does: the document's capturing listeners
  // first, then the element's own unless one stopped it. Settles once each
  // handler's run has ended, true when no listener prevented the default.
  async dispatch(name, fields = {}) {
    let stopped = false;
    let prevented = false;
    const event = {
      type: name,
      target: this,
      ...fields,
      preventDefault() {
        prevented = true;
      },
      stopPropagation() {
        stopped = true;
      },
    };
    const runs = [];
    for (const handler of [...(capturing.get(name) ?? [])]) runs.push(handler(event));
    if (!stopped) for (const handler of this.listeners.get(name) ?? []) runs.push(handler(event));
    await Promise.all(runs);
    return !prevented;
  }
}
const body = new Element("body");
const byId = new Map();
body.appendChild = (child) => {
  if (child.id) byId.set(child.id, child);
  return Element.prototype.appendChild.call(body, child);
};
const capturing = new Map();
globalThis.document = {
  body,
  createElement: (tag) => new Element(tag),
  getElementById: (id) => byId.get(id) ?? null,
  addEventListener: (name, handler, capture) => {
    if (capture === true) capturing.set(name, [...(capturing.get(name) ?? []), handler]);
  },
  removeEventListener: (name, handler, capture) => {
    if (capture === true) capturing.set(name, (capturing.get(name) ?? []).filter((each) => each !== handler));
  },
};

const { jet_main } = await import("./app.js");
await jet_main();
const container = document.getElementById("jet-app");
const scene = () => container.children.map((child) => `${child.tagName}:${JSON.stringify(child.textContent)}`).join(" ");
const menu = () => body.children.find((child) => child.getAttribute("role") === "menu") ?? null;
const shown = () => {
  const open = menu();
  if (!open) return "closed";
  const entries = open.children.map((entry) => {
    const role = entry.getAttribute("role");
    if (role === "separator") return "---";
    return `${JSON.stringify(entry.textContent)}${entry.disabled ? " (disabled)" : ""}`;
  });
  return `at ${open.style.left},${open.style.top}: ${entries.join(" | ")}`;
};
const panel = () => container.children.find((child) => child.listeners.has("contextmenu"));
const rightClick = async () => {
  const kept = await panel().dispatch("contextmenu", { clientX: 40, clientY: 30 });
  console.log(`right-click: browser menu ${kept ? "shown" : "suppressed"}; menu ${shown()}`);
};
const choose = async (label) => {
  await menu().children.find((entry) => entry.textContent === label).dispatch("click");
  console.log(`chose ${label}: menu ${shown()}; scene ${scene()}`);
};

console.log(`initial: ${scene()}`);
console.log(`the button has no menu: ${!container.children.find((child) => child.tagName === "button").listeners.has("contextmenu")}`);
await rightClick();
await choose("Add Task");
await rightClick();
const outside = container.children.find((child) => child.tagName === "button");
await outside.dispatch("click");
console.log(`click outside: menu ${shown()}; scene ${scene()}`);
await rightClick();
const kept = await body.dispatch("keydown", { key: "Escape" });
console.log(`escape: ${kept ? "passed on" : "taken"}; menu ${shown()}`);
await rightClick();
await choose("Quit");
await choose("Share");
console.log("notices:");
await container.children.find((child) => child.tagName === "button").dispatch("click");
