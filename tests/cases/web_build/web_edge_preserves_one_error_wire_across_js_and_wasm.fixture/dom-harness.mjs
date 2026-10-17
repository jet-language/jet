
process.on("unhandledRejection", (error) => {
  if (error?.name !== "JetError") throw error;
});
class FakeElement {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.style = {};
    this.children = [];
    this.textContent = "";
    this.id = "";
    this.hidden = true;
  }
  appendChild(child) { this.children.push(child); return child; }
}
class FakeDocument {
  constructor() {
    this._byId = new Map();
    this.body = new FakeElement("body");
    const append = this.body.appendChild.bind(this.body);
    this.body.appendChild = (element) => {
      if (element.id) this._byId.set(element.id, element);
      return append(element);
    };
  }
  createElement(tag) { return new FakeElement(tag); }
  getElementById(id) { return this._byId.get(id) ?? null; }
}
globalThis.document = new FakeDocument();

const { showRuntimeError } = await import("./jet_dom_runtime.js");
const { jet_main } = await import("./app.js");
try {
  await jet_main();
  throw new Error("unexpected success");
} catch (error) {
  for (const field of ["message", "code", "cause", "journey", "toJSON"]) {
    if (!(field in error)) throw new Error("typed edge lost " + field);
  }
  if (!(error instanceof Error) || error.name !== "JetError") throw new Error("edge did not use JetError");
  const returnedFrame = showRuntimeError(error);
  const overlay = document.getElementById("jet-runtime-overlay");
  if (!overlay || overlay.hidden || returnedFrame !== error.frame || overlay.textContent !== error.frame) {
    throw new Error("DOM overlay did not render the shared report frame");
  }
  console.log(JSON.stringify({
    kind: error.constructor?.name,
    name: error.name,
    code: error.code,
    wire: error.toJSON(),
    cause: error.cause?.message ?? null,
    journey: error.journey ?? "",
    frame: error.frame,
    overlay: { id: overlay.id, hidden: overlay.hidden, textContent: overlay.textContent },
  }));
}
