// A page whose browser is the one `node page.mjs <browser>` names: `share`
// has the Web Share API and the person shares, `dismiss` has it and the
// person dismisses the share sheet, and `copy` has none but a clipboard.
// Each lends its share sheet and clipboard only while the person's gesture
// is active; clicks on the board's buttons are the gestures. The DOM is a
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

const browser = process.argv[2];
let active = false;
const refused = (name) => Promise.reject(Object.assign(new Error(name), { name }));
const later = (value) => new Promise((resolve) => setTimeout(() => resolve(value), 10));
const share = ({ url }) => {
  if (!active) return refused("NotAllowedError");
  if (browser === "dismiss") {
    console.log("the person dismissed the share sheet");
    return later(undefined).then(() => refused("AbortError"));
  }
  console.log("shared: " + url);
  return later(undefined);
};
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {
    userActivation: {
      get isActive() {
        return active;
      },
    },
    share: browser === "copy" ? undefined : share,
    clipboard: {
      writeText: (text) => {
        if (!active) return refused("NotAllowedError");
        console.log("copied: " + text);
        return later(undefined);
      },
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
console.log("clicked Share");
await click("Share");
await click("Notices");
