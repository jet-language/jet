// A minimal page for main.jet's build: the document keeps its body, the
// elements appended to it and its event listeners, and says whether it
// shows (visibilityState); the page's global object keeps its own. Once
// the program's run returned, the page hides the document and shows it
// again, the browser freezes the page and resumes it, and the page goes
// away.
class Target {
  constructor() {
    this.listeners = new Map();
  }
  addEventListener(name, handler) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }
  // Settles once each listener's run has ended.
  dispatch(name, fields = {}) {
    const event = { type: name, target: this, ...fields };
    return Promise.all((this.listeners.get(name) ?? []).map((handler) => handler(event)));
  }
}

class Element extends Target {
  constructor(tag) {
    super();
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.style = {};
    this.textContent = "";
    this.id = "";
    this.parent = null;
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
}

const body = new Element("body");
const byId = new Map();
body.appendChild = (child) => {
  if (child.id) byId.set(child.id, child);
  return Element.prototype.appendChild.call(body, child);
};
const page = new Target();
const document = Object.assign(new Target(), {
  body,
  visibilityState: "visible",
  createElement: (tag) => new Element(tag),
  getElementById: (id) => byId.get(id) ?? null,
});
globalThis.document = document;
globalThis.addEventListener = (name, handler) => page.addEventListener(name, handler);

const { jet_main } = await import("./app.js");
await jet_main();
const shown = (state) => {
  document.visibilityState = state;
  return document.dispatch("visibilitychange");
};
await shown("hidden");
await shown("visible");
await document.dispatch("freeze");
await document.dispatch("resume");
await page.dispatch("pagehide", { persisted: false });
