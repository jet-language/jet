// A minimal page for main.jet's build (web/.jet/build), its files beside
// the case's work folder three levels up, served from the server's origin
// (http://127.0.0.1:$CREW_PORT/): the document keeps its body, the
// elements appended to it and its event listeners, and says whether it
// shows (visibilityState); the page's global object keeps its own, and
// its fetch keeps each reply's promise. Once the program's run returned,
// the browser freezes the page (and the page writes asleep for the
// server), waits for the server to listen (server.ready) and resumes the
// page. The resume sends the queued write in a task of its own; once its
// reply came, the page goes away (and writes done, then asks the server
// once more so it sees it).
import { existsSync, writeFileSync } from "node:fs";

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
globalThis.location = { href: `http://127.0.0.1:${process.env.CREW_PORT}/` };
const replies = [];
const fetched = globalThis.fetch;
globalThis.fetch = (url, init) => {
  const reply = fetched(url, init);
  replies.push(reply.then(() => {}, () => {}));
  return reply;
};

const work = new URL("../../../", import.meta.url);
const { jet_main } = await import("./app.js");
await jet_main();
await document.dispatch("freeze");
writeFileSync(new URL("asleep", work), "");
for (let waited = 0; !existsSync(new URL("server.ready", work)); waited += 10) {
  if (waited > 120000) throw new Error("the server never listened");
  await new Promise((done) => setTimeout(done, 10));
}
const before = replies.length;
await document.dispatch("resume");
for (let waited = 0; replies.length === before; waited += 10) {
  if (waited > 120000) throw new Error("the resume never sent the queued write");
  await new Promise((done) => setTimeout(done, 10));
}
await Promise.all(replies);
await new Promise((done) => setTimeout(done, 0));
await page.dispatch("pagehide", { persisted: false });
writeFileSync(new URL("done", work), "");
// The server ends at its next request once the page is done.
await fetched(new URL("/api/count", globalThis.location.href), { method: "POST", body: "[]" });
