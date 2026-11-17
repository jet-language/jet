// A page whose browser keeps a clipboard as a browser does: it lends the
// clipboard only while the person's gesture is active, and refuses it
// otherwise. Clicks on the board's buttons are the gestures. The DOM is a
// minimal one: elements keep their children, text and click listeners, and
// the body registers an appended element by id.
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
}
const body = new Element("body");
const byId = new Map();
body.appendChild = (child) => {
  if (child.id) byId.set(child.id, child);
  return Element.prototype.appendChild.call(body, child);
};
globalThis.document = { body, createElement: (tag) => new Element(tag), getElementById: (id) => byId.get(id) ?? null, addEventListener: () => {} };

let active = false;
let clipboard = "";
const refused = () => Promise.reject(Object.assign(new Error("not allowed"), { name: "NotAllowedError" }));
const later = (value) => new Promise((resolve) => setTimeout(() => resolve(value), 10));
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {
    userActivation: {
      get isActive() {
        return active;
      },
    },
    clipboard: {
      writeText: (text) => {
        if (!active) return refused();
        clipboard = text;
        console.log("copied: " + text);
        return later(undefined);
      },
      readText: () => (active ? later(clipboard) : refused()),
    },
  },
});

// The person clicks the button labeled `label`: the gesture lasts until its
// handler's run has ended.
const click = async (label) => {
  const button = document.getElementById("jet-app").children.find((child) => child.tagName === "button" && child.textContent === label);
  active = true;
  await Promise.all(button.clicks.map((handler) => handler({ type: "click", target: button })));
  active = false;
};

const { jet_main } = await import("./app.js");
await jet_main();
console.log("clicked Copy Link");
await click("Copy Link");
clipboard = "Standup notes, from the team's wiki";
console.log("another program copied; clicked Paste");
await click("Paste");
