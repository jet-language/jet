import { installDom } from "./fixtures/domshim.mjs";

const document = installDom();
globalThis.document = document;
const { mount } = await import("./app.mjs");
const root = document.getElementById("app");
const { display, increment, reset } = mount(root);
for (let i = 0; i < 3; i += 1) increment.click();
console.log(display.textContent);
reset.click();
console.log(display.textContent);
