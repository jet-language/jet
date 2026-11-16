import { readFile } from "node:fs/promises";
import { installDom } from "./fixtures/domshim.mjs";

const document = installDom();
globalThis.document = document;
const bytes = await readFile(new URL("./main.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes);
const app = instance.exports;
const decoder = new TextDecoder();
const text = (length) => decoder.decode(new Uint8Array(app.memory.buffer, app.label(), length));

const root = document.getElementById("app");
const display = document.createElement("div");
const increment = document.createElement("button");
const reset = document.createElement("button");
increment.textContent = "increment";
reset.textContent = "reset";
increment.addEventListener("click", () => { display.textContent = text(app.increment()); });
reset.addEventListener("click", () => { display.textContent = text(app.reset()); });
display.textContent = text(app.mount());
root.appendChild(display);
root.appendChild(increment);
root.appendChild(reset);

for (let i = 0; i < 3; i += 1) increment.click();
console.log(display.textContent);
reset.click();
console.log(display.textContent);
