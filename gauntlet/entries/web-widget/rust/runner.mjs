import { readFile } from "node:fs/promises";

const bytes = await readFile(new URL("./main.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes);

let sum = 0;
for (let i = 1; i <= 12; i += 1) {
  const value = instance.exports.triangular(i);
  console.log(`t${i} ${value}`);
  sum += value;
}
console.log(`sum ${sum}`);
