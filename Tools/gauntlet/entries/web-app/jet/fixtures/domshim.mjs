class Element {
  constructor(tagName) {
    this.tagName = tagName.toUpperCase();
    this.children = [];
    this.dataset = {};
    this.style = {};
    this.listeners = new Map();
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

  addEventListener(name, handler) {
    const handlers = this.listeners.get(name) ?? [];
    handlers.push(handler);
    this.listeners.set(name, handlers);
  }

  click() {
    for (const handler of this.listeners.get("click") ?? []) handler({ type: "click", target: this });
  }

  setAttribute() {}
  removeAttribute() {}
}

export function installDom() {
  const body = new Element("body");
  const byId = new Map();
  const document = {
    body,
    createElement(tag) { return new Element(tag); },
    getElementById(id) { return byId.get(id) ?? null; },
  };
  body.appendChild = (child) => {
    if (child.id) byId.set(child.id, child);
    return Element.prototype.appendChild.call(body, child);
  };
  return document;
}
