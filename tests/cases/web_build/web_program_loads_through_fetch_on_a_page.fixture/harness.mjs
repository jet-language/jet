import { readFileSync } from "node:fs";
const fetched = [];
globalThis.fetch = async (url) => {
  const name = new URL(url).pathname.split("/").pop();
  fetched.push(name);
  const bytes = readFileSync(new URL(name, import.meta.url));
  return { ok: true, status: 200, arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
};
const { runProgram } = await import("./jet_dom_runtime.js");
const status = await runProgram(new URL("http://page.invalid/app.wasm"));
console.log("status " + status + "; fetched " + fetched.join(" "));
