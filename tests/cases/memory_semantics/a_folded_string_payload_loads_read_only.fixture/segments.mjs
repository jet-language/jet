// Finds the folded String's payload (SIZE bytes of "x") in the ELF executable
// PATH and prints whether the PT_LOAD that maps it is writable.
//
//   node segments.mjs PATH SIZE
import { readFileSync } from "node:fs";

const image = readFileSync(process.argv[2]);
const size = Number(process.argv[3]);
let run = 0;
let end = -1;
for (let at = 0; at < image.length; at++) {
  run = image[at] === 0x78 ? run + 1 : 0;
  if (run === size) {
    end = at + 1;
    break;
  }
}
if (end < 0) {
  console.log("payload: absent");
  process.exit(1);
}
const start = end - size;
const headers = Number(image.readBigUInt64LE(32));
const entry = image.readUInt16LE(54);
const count = image.readUInt16LE(56);
for (let index = 0; index < count; index++) {
  const at = headers + index * entry;
  if (image.readUInt32LE(at) !== 1) continue;
  const flags = image.readUInt32LE(at + 4);
  const offset = Number(image.readBigUInt64LE(at + 8));
  const fileSize = Number(image.readBigUInt64LE(at + 32));
  if (start >= offset && end <= offset + fileSize) {
    console.log(`payload: in a ${flags & 2 ? "writable" : "read-only"} PT_LOAD`);
    process.exit(0);
  }
}
console.log("payload: in no PT_LOAD");
process.exit(1);
