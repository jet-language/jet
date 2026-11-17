// A minimal page for main.jet's build (web/.jet/build): elements keep their
// children, text, attributes, properties and event listeners; the body
// registers an appended element by id, and the document keeps its
// capturing listeners. The person drags the card to Doing, drags the note
// to Done, which refuses it, and drops text of another program's on Done,
// which refuses that too. Then they move the card to Done with the
// keyboard alone (Space, Right, Enter), pick it up and put it back
// (Space, Escape), and move it back to Doing from its context menu, which
// the note's lacks: no drop zone takes a note. Then the Notices button
// prints the app's notices.
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
const named = (text) => container.children.find((child) => child.textContent === text);
const panel = () => container.children[0].style.background;
const outlined = () => {
  const zones = ["Doing", "Done"].filter((text) => (named(text).style.outline ?? "") !== "");
  return zones.length === 0 ? "none" : zones.map((text) => `${text} (${named(text).style.outline})`).join(", ");
};
const menu = () => body.children.find((child) => child.getAttribute("role") === "menu") ?? null;
const shown = () => {
  const open = menu();
  if (!open) return "closed";
  return open.children.map((entry) => (entry.getAttribute("role") === "separator" ? "---" : entry.textContent)).join(" | ");
};
// A drag's data, which carries files or another program's text.
const transfer = (files, text) => {
  const kept = new Map();
  return {
    files,
    setData: (type, value) => kept.set(type, value),
    getData: (type) => kept.get(type) ?? text,
  };
};
const drag = async (from, to) => {
  const source = named(from);
  const data = transfer([], "");
  await source.dispatch("dragstart", { dataTransfer: data });
  const kept = await named(to).dispatch("drop", { dataTransfer: data });
  await source.dispatch("dragend", { dataTransfer: data });
  console.log(`dragged ${from} to ${to}: ${kept ? "the browser kept it" : "the page took it"}; panel ${panel()}`);
};
const key = async (name) => {
  const kept = await named("Ship it").dispatch("keydown", { key: name });
  console.log(`${JSON.stringify(name)}: ${kept ? "passed on" : "taken"}; outlined ${outlined()}; panel ${panel()}`);
};
const rightClick = async (text) => {
  const kept = await named(text).dispatch("contextmenu", { clientX: 40, clientY: 30 });
  console.log(`right-click ${text}: browser menu ${kept ? "shown" : "suppressed"}; menu ${shown()}`);
};

const card = named("Ship it");
console.log(`board: panel ${panel()}; the card drags: ${card.draggable === true}, takes focus: ${card.tabIndex === 0}; the note takes focus: ${named("Standup notes").tabIndex === 0}`);
await drag("Ship it", "Doing");
await drag("Standup notes", "Done");
const dropped = await named("Done").dispatch("drop", { dataTransfer: transfer([], "Standup notes, from the team's wiki") });
console.log(`dropped another program's text on Done: ${dropped ? "the browser kept it" : "the page took it"}; panel ${panel()}`);
await key(" ");
await key("ArrowRight");
await key("Enter");
await key(" ");
await key("Escape");
await rightClick("Standup notes");
await rightClick("Ship it");
await menu().children.find((entry) => entry.textContent === "Move Ship it to Doing").dispatch("click");
console.log(`chose Move Ship it to Doing: menu ${shown()}; panel ${panel()}`);
await named("Notices").dispatch("click");
