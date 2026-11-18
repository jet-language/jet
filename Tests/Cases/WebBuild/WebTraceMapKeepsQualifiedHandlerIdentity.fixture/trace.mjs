// Prints the functions app.wasm's `jet.lines` section (the location table
// a trace's frames read, JetWeb Module.jet web_location_table) names, each
// with its file.
import { readFileSync } from "node:fs";

const module = new WebAssembly.Module(readFileSync("app.wasm"));
const [section] = WebAssembly.Module.customSections(module, "jet.lines");
const bytes = new Uint8Array(section);
let at = 0;
const leb = (signed) => {
  let value = 0;
  let shift = 0;
  let byte = 0;
  do {
    byte = bytes[at++];
    value |= (byte & 0x7f) << shift;
    shift += 7;
  } while (byte & 0x80);
  return signed && shift < 32 && byte & 0x40 ? value | (-1 << shift) : value;
};
const text = () => {
  const size = leb(false);
  at += size;
  return new TextDecoder().decode(bytes.subarray(at - size, at));
};
const files = [];
for (let count = leb(false); count > 0; count -= 1) files.push(text());
for (let count = leb(false); count > 0; count -= 1) {
  leb(false);
  const name = text();
  const file = files[leb(false)];
  leb(false); // its declaration line
  for (let left = leb(false); left > 0; left -= 1) {
    leb(false);
    leb(true);
  }
  console.log(`fn ${name} ${file}`);
}
