import { installDom } from "./fixtures/domshim.mjs";
import "./wasm_exec.js";
import { readFile } from "node:fs/promises";

const document = installDom();
globalThis.document = document;
const go = new Go();
const bytes = await readFile(new URL("./main.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes, go.importObject);
go.run(instance);

const root = document.getElementById("app");
const button = (text) => root.children.find((child) => child.tagName === "BUTTON" && child.textContent === text);
const display = root.children.find((child) => child.tagName === "DIV");
for (let i = 0; i < 3; i += 1) button("increment").click();
console.log(display.textContent);
button("reset").click();
console.log(display.textContent);
