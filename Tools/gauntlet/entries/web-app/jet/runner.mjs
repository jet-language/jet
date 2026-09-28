import { installDom } from "./fixtures/domshim.mjs";

const document = installDom();
globalThis.document = document;
const { jet_main } = await import("./.jet/build/app.js");
await jet_main();
await new Promise((resolve) => setImmediate(resolve));

const container = document.getElementById("jet-app");
const button = (text) => container.children.find((child) => child.textContent === text);
const display = () => container.children.find((child) => child.tagName === "DIV");
for (let i = 0; i < 3; i += 1) button("increment").click();
console.log(display().textContent);
button("reset").click();
console.log(display().textContent);
