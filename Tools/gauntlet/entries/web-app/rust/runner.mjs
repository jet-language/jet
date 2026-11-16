import { installDom } from "./fixtures/domshim.mjs";
import { readFile } from "node:fs/promises";

const document = installDom();
globalThis.document = document;
const bytes = await readFile(new URL("./main.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes);
const counter = instance.exports;

// Mount the counter: Wasm owns the state, the DOM shows it.
const root = document.getElementById("app");
const display = document.createElement("div");
const increment = document.createElement("button");
const reset = document.createElement("button");
increment.textContent = "increment";
reset.textContent = "reset";
const render = (count) => { display.textContent = `count: ${count}`; };
increment.addEventListener("click", () => render(counter.increment()));
reset.addEventListener("click", () => render(counter.reset()));
render(counter.count());
root.appendChild(display);
root.appendChild(increment);
root.appendChild(reset);

for (let i = 0; i < 3; i += 1) increment.click();
console.log(display.textContent);
reset.click();
console.log(display.textContent);
