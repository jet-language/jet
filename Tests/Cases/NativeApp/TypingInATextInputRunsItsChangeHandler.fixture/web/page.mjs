// A minimal page for main.jet's build (web/.jet/build): elements keep their
// children, style, text, value, selection and event listeners; focusing one
// makes it the document's active element, and the body registers an
// appended element by id. The person focuses the input, types "h" and then
// "i", then deletes the "i": each edit sets the input's value and caret,
// then fires its input event, as a browser does.
class Element {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.style = {};
    this.textContent = "";
    this.value = "";
    this.selectionStart = 0;
    this.selectionEnd = 0;
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
  focus() {
    document.activeElement = this;
  }
  setSelectionRange(start, end) {
    this.selectionStart = start;
    this.selectionEnd = end;
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
  activeElement: null,
  createElement: (tag) => new Element(tag),
  getElementById: (id) => byId.get(id) ?? null,
};

const { jet_main } = await import("./app.js");
await jet_main();
const root = document.getElementById("jet-app");
const input = () => root.children.find((child) => child.tagName === "INPUT");
const show = (when) => {
  const note = root.children.find((child) => child.tagName === "SPAN").textContent;
  const focused = document.activeElement === input();
  console.log(`${when}: ${JSON.stringify(note)}, input ${JSON.stringify(input().value)}, focused ${focused}, caret ${input().selectionStart}`);
};
const edit = async (value) => {
  const typed = input();
  typed.value = value;
  typed.setSelectionRange(value.length, value.length);
  await typed.dispatch("input");
};
show("mounted");
input().focus();
await edit("h");
show("typed h");
await edit("hi");
show("typed i");
await edit("h");
show("deleted the i");
