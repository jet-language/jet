import { readFileSync } from "node:fs";

const input = process.argv[2] ?? "telemetry.bin";
const data = readFileSync(input);
if (data.length < 8 || data.subarray(0, 4).toString("ascii") !== "EDB1") throw new Error("bad input");
const count = data.readUInt32LE(4);
let checksum = 0;
for (const byte of data) checksum += byte;
let offset = 8;
let valid = 0;
const channels = [0, 0, 0, 0];
let sampleMin = 65_535;
let sampleMax = 0;
let sampleSum = 0;
let energySum = 0;
let loadSum = 0;
let tickSum = 0;
for (let index = 0; index < count; index += 1) {
  const channel = data.readUInt8(offset);
  const flags = data.readUInt8(offset + 1);
  const sample = data.readUInt16LE(offset + 2);
  const tick = data.readUInt32LE(offset + 4);
  const energy = data.readUInt16LE(offset + 8);
  const load = data.readUInt16LE(offset + 10);
  offset += 12;
  if (channel >= channels.length) throw new Error("bad channel");
  if (flags === 0) {
    valid += 1;
    channels[channel] += 1;
    if (sample < sampleMin) sampleMin = sample;
    if (sample > sampleMax) sampleMax = sample;
    sampleSum += sample;
    energySum += energy;
    loadSum += load;
    tickSum += tick;
  }
}
console.log(`frames ${count}`);
console.log(`valid ${valid}`);
console.log(`channels ${channels[0]} ${channels[1]} ${channels[2]} ${channels[3]}`);
console.log(`sample ${sampleMin} ${sampleMax} ${sampleSum}`);
console.log(`energy ${energySum}`);
console.log(`load ${loadSum}`);
console.log(`ticks ${tickSum}`);
console.log(`checksum ${checksum}`);
