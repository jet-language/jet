#!/usr/bin/env node
// Recover byte spans from package-unit.mjs's ordered source markers without
// reassembling or changing the checked source. MIR spans are UTF-8 byte offsets.
import { readFileSync, writeFileSync } from "node:fs";
const [input, output] = process.argv.slice(2);
if (!input || !output) throw new Error("usage: self-host-map.mjs <assembled selfhost.jet> <compiler.map.json>");
const bytes = readFileSync(input);
const text = bytes.toString("utf8");
const markers = [...text.matchAll(/^\/\/ \[unit source: (.+)\]\r?$/gm)];
if (markers.length === 0) throw new Error(`no package source markers in ${input}`);
let charOffset = 0, byteOffset = 0;
const starts = markers.map((marker) => {
  byteOffset += Buffer.byteLength(text.slice(charOffset, marker.index), "utf8");
  charOffset = marker.index;
  return byteOffset;
});
const source_segments = markers.map((marker, index) => ({
  source_file: marker[1], generated: { start: starts[index], end: starts[index + 1] ?? bytes.length },
}));
writeFileSync(output, JSON.stringify({ source_segments }, null, 2) + "\n");
console.log(`SELFHOST MAP sources=${source_segments.length} bytes=${bytes.length}`);
