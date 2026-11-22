// A minimal browser page with `sessionStorage` persisted across reloads in
// `session.json` beside the page, for testing `ui.saved` on `--target web`.
// Modes:
//   first  - mounts empty draft, types "half-typed", prints updated view
//   reload - fresh process/DOM reload, verifies "half-typed" restored, appends "!"
//   verify - second reload, verifies "half-typed!" restored
import fs from "node:fs";

const mode = process.argv[2] ?? "first";
const storeFile = "session.json";
const readStore = () => (fs.existsSync(storeFile) ? JSON.parse(fs.readFileSync(storeFile, "utf8")) : {});
const writeStore = (data) => fs.writeFileSync(storeFile, JSON.stringify(data));

globalThis.sessionStorage = {
  getItem(key) {
    const data = readStore();
    return Object.prototype.hasOwnProperty.call(data, key) ? data[key] : null;
  },
  setItem(key, value) {
    const data = readStore();
    data[key] = String(value);
    writeStore(data);
  },
  removeItem(key) {
    const data = readStore();
    delete data[key];
    writeStore(data);
  },
  clear() {
    writeStore({});
  },
};

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
  visibilityState: "visible",
  addEventListener() {},
  createElement: (tag) => new Element(tag),
  getElementById: (id) => byId.get(id) ?? null,
};
globalThis.location = { pathname: "/", search: "" };

const { jet_main } = await import("./app.js");
await jet_main();

const root = () => document.getElementById("jet-app");
const input = () => root().children.find((child) => child.tagName === "INPUT");
const label = () => root().children.find((child) => child.tagName === "SPAN" && child.textContent.startsWith("Comment:"));
const show = (when) => {
  console.log(`${when}: ${JSON.stringify(label().textContent)}, input ${JSON.stringify(input().value)}`);
};
const edit = async (value) => {
  const field = input();
  field.focus();
  field.value = value;
  field.setSelectionRange(value.length, value.length);
  await field.dispatch("input");
};

show(`${mode} mounted`);
if (mode === "first") {
  await edit("half-typed");
  show("first typed");
} else if (mode === "reload") {
  await edit("half-typed!");
  show("reload edited");
}
