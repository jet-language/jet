import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

// A second build writes the same module and the same map.
const wasm = readFileSync('.jet/build/app.wasm');
const map = readFileSync('.jet/build/app.wasm.map', 'utf8');
assert.ok(wasm.equals(readFileSync('first-app.wasm')));
assert.equal(map, readFileSync('first-map.json', 'utf8'));

// The map names the entry relative to its directory, carries its text, and
// names no host path.
const parsed = JSON.parse(map);
assert.equal(parsed.version, 3);
assert.equal(parsed.file, 'app.wasm');
assert.deepEqual(parsed.sources, ['main.jet']);
const src = readFileSync('private/build-host/project/main.jet', 'utf8');
assert.deepEqual(parsed.sourcesContent, [src]);
assert.ok(!map.includes('/private/build-host'));
assert.ok(!map.includes(process.cwd()));

// The user's string that looks like a source-map marker stays verbatim data.
assert.ok(wasm.includes('//# __jet_source_map line 999'));

// Sections: the code section's bounds, and the last section is the
// `sourceMappingURL` custom section whose payload is the length-prefixed
// name of the map.
function leb(at) {
  let value = 0, shift = 0, byte;
  do {
    byte = wasm[at++];
    value += (byte & 127) * 2 ** shift;
    shift += 7;
  } while (byte & 128);
  return [value, at];
}
let at = 8, code = null, last = null;
while (at < wasm.length) {
  const id = wasm[at];
  const [size, start] = leb(at + 1);
  if (id === 10) code = [start, start + size];
  last = [id, start, start + size];
  at = start + size;
}
assert.equal(at, wasm.length);
assert.ok(code);
assert.equal(last[0], 0);
const [nameLen, nameAt] = leb(last[1]);
assert.equal(wasm.subarray(nameAt, nameAt + nameLen).toString(), 'sourceMappingURL');
const [urlLen, urlAt] = leb(nameAt + nameLen);
assert.equal(urlAt + urlLen, last[2]);
assert.equal(wasm.subarray(urlAt, urlAt + urlLen).toString(), 'app.wasm.map');

// Mappings: one generated line whose columns are module byte offsets inside
// the code section. A four-field segment lands on a non-blank line of
// main.jet; a one-field segment starts code the map leaves out (Core's).
// Both statements are mapped, and nothing maps to the marker's "line 999".
const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function segment(encoded) {
  const values = [];
  let value = 0n, shift = 0n;
  for (const char of encoded) {
    const digit = alphabet.indexOf(char);
    assert.ok(digit >= 0);
    value |= BigInt(digit & 31) << shift;
    if (digit & 32) shift += 5n;
    else {
      const magnitude = value >> 1n;
      values.push(Number(value & 1n ? -magnitude : magnitude));
      value = 0n;
      shift = 0n;
    }
  }
  assert.equal(shift, 0n);
  return values;
}
const generatedLines = parsed.mappings.split(';');
assert.equal(generatedLines.length, 1);
const lines = src.split('\n');
const mapped = new Set();
let column = 0, source = 0, line = 0, original = 0;
for (const encoded of generatedLines[0].split(',').filter((s) => s.length)) {
  const values = segment(encoded);
  assert.ok(values.length === 1 || values.length === 4, encoded);
  column += values[0];
  assert.ok(column >= code[0] && column < code[1]);
  if (values.length === 1) continue;
  source += values[1];
  line += values[2];
  original += values[3];
  assert.equal(source, 0);
  assert.equal(original, 0);
  assert.ok(line >= 0 && line < lines.length && lines[line].trim() !== '');
  mapped.add(line);
}
const first = lines.findIndex((l) => l.trim() === 'first: 1');
const print = lines.findIndex((l) => l.trim().startsWith('print('));
assert.ok(first >= 0 && print >= 0);
assert.ok(mapped.has(first), `line ${first + 1} is not mapped`);
assert.ok(mapped.has(print), `line ${print + 1} is not mapped`);
