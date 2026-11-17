// A minimal DOM for main.jet's build: elements keep their children, text and
// event listeners, and the body registers an appended element by id. A file
// input's click is the person picking crew-tasks.csv in the chooser, after
// the page's code returned; the board's drop is the person dropping it with
// a note from the file manager.
import fs from "node:fs";

class Element {
  constructor(tag) {
    this.tagName = tag.toLowerCase();
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
  append(child) {
    return this.appendChild(child);
  }
  remove() {
    if (!this.parent) return;
    this.parent.children = this.parent.children.filter((child) => child !== this);
    this.parent = null;
  }
  addEventListener(name, handler) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }
  // Settles once each handler's run has ended, true when no listener
  // prevented the default.
  async dispatch(name, fields = {}) {
    let prevented = false;
    const event = {
      type: name,
      target: this,
      ...fields,
      preventDefault() {
        prevented = true;
      },
    };
    await Promise.all((this.listeners.get(name) ?? []).map((handler) => handler(event)));
    return !prevented;
  }
  click() {
    if (this.tagName !== "input" || this.type !== "file") return;
    console.log(`the chooser offers ${this.accept}, ${this.parent === body ? "from a hidden input on the page" : "from nowhere"}`);
    setTimeout(() => {
      this.files = [csv];
      this.dispatch("change");
    }, 0);
  }
}

const csv = new File([fs.readFileSync("../../../crew-tasks.csv")], "crew-tasks.csv", { type: "text/csv" });
const note = new File(["call the hinge shop\n"], "note.txt", { type: "text/plain" });
const body = new Element("body");
const byId = new Map();
body.appendChild = (child) => {
  if (child.id) byId.set(child.id, child);
  return Element.prototype.appendChild.call(body, child);
};
globalThis.document = {
  body,
  createElement: (tag) => new Element(tag),
  getElementById: (id) => byId.get(id) ?? null,
  addEventListener: () => {},
};

const { jet_main } = await import("./app.js");
await jet_main();
const container = document.getElementById("jet-app");
const scene = () => container.children.map((child) => `${child.tagName}:${JSON.stringify(child.textContent)}`).join(" ");
console.log(`the chooser's input is gone: ${!body.children.some((child) => child.tagName === "input")}`);
console.log(`scene: ${scene()}`);
const board = container.children[0];
console.log(`the board takes a drag: ${!(await board.dispatch("dragover"))}`);
await board.dispatch("drop", { dataTransfer: { files: [csv, note], getData: () => "" } });
console.log(`scene: ${scene()}`);
await board.dispatch("drop", { dataTransfer: { files: [note], getData: () => "" } });
console.log(`after a drop of only the note, scene: ${scene()}`);
