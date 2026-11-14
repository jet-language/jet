class Element {
  constructor(tagName) {
    this.tagName = tagName.toUpperCase();
    this.children = [];
    this.listeners = new Map();
    this.textContent = "";
  }

  appendChild(child) {
    this.children.push(child);
    return child;
  }

  addEventListener(name, handler) {
    const handlers = this.listeners.get(name) ?? [];
    handlers.push(handler);
    this.listeners.set(name, handlers);
  }

  click() {
    for (const handler of this.listeners.get("click") ?? []) handler({ type: "click", target: this });
  }
}

export function installDom() {
  const root = new Element("main");
  const elements = new Map([["app", root]]);
  return {
    createElement(tag) { return new Element(tag); },
    getElementById(id) { return elements.get(id) ?? null; },
  };
}
