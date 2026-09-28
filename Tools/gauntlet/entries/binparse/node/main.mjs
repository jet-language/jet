import { readFileSync } from "node:fs";

const input = process.argv[2] ?? "records.bin";
const data = readFileSync(input);
if (data.subarray(0, 4).toString("ascii") !== "JGB1") throw new Error("bad magic");
const count = data.readUInt32LE(4);
let offset = 8;
let sum7 = 0;
let hash = 0xcbf29ce484222325n;
const mask = 0xffffffffffffffffn;
for (let index = 0; index < count; index += 1) {
  const id = data.readUInt32LE(offset);
  const value = data.readDoubleLE(offset + 4);
  const nameLength = data.readUInt16LE(offset + 12);
  offset += 14;
  if (id % 7 === 0) sum7 += value;
  for (let i = 0; i < nameLength; i += 1) {
    hash = ((hash ^ BigInt(data[offset + i])) * 0x100000001b3n) & mask;
  }
  offset += nameLength;
}
console.log(`records ${count}`);
console.log(`sum7 ${sum7.toFixed(6)}`);
console.log(`fnv ${hash.toString(16).padStart(16, "0")}`);
