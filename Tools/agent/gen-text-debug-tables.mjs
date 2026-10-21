#!/usr/bin/env node
// Offline pinned-source checker for the existing Trap Rust-str Debug table.
// Input: extracted rustc-1.97.1-src/library (pinned tarball sha256 below).
// Regenerate: node Tools/agent/gen-text-debug-tables.mjs /path/to/library
// Optional third argument: byte-per-codepoint mask from pinned rustc's str Debug
// (1 when Debug starts with "\\u{"); checks every valid non-ASCII scalar.
// Optional fourth argument: input/debug-output hex TSV from the same oracle;
// regenerates the C witness's golden vectors, including embedded NUL input.
// Reference semantics: core/src/fmt/mod.rs:2916-2960 and char/methods.rs:474-487.
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
const version = '1.97.1';
const commit = '8bab26f4f68e0e26f0bb7960be334d5b520ea452';
const tarSha = '622c2b429c53cbfdc0dd3a51d03554e91cd63ebec1912c1f5709640cdfef1a9d';
const inputs = {
  'core/src/unicode/printable.rs': '491bb78890521eab319bd4c45c6c33ab4c3eedd6aff4b5de2b8ddfda8abe7d78',
  'core/src/unicode/unicode_data.rs': '4438831f7c584a35457022c4016ab8a295f242ff809b81a5c81b9bb14dc5e166',
};
const root = process.argv[2];
if (!root) throw new Error('usage: gen-text-debug-tables.mjs <rustc-1.97.1-src/library>');
function source(name) {
  const bytes = readFileSync(path.join(root, name));
  const sha = createHash('sha256').update(bytes).digest('hex');
  if (sha !== inputs[name]) throw new Error(`${name}: checksum ${sha} != pinned ${inputs[name]}`);
  return bytes.toString('utf8');
}
const printable = source('core/src/unicode/printable.rs');
const unicode = source('core/src/unicode/unicode_data.rs');
function requiredMatch(text, regex, label) {
  const match = text.match(regex);
  if (!match) throw new Error(`missing pinned source structure: ${label}`);
  return match[1];
}
function numbers(text) { return [...text.matchAll(/0x[0-9a-f]+|\d+/g)].map(m => Number(m[0])); }
function array(name) {
  return numbers(requiredMatch(printable, new RegExp(`const ${name}: [^=]+ = &\\[([\\s\\S]*?)\\];`), name));
}
const planes = [0, 1].map(p => ({ uppers: array(`SINGLETONS${p}U`), lowers: array(`SINGLETONS${p}L`), normal: array(`NORMAL${p}`) }));
const tail = [...printable.matchAll(/if (0x[0-9a-f]+) <= x && x < (0x[0-9a-f]+)/g)].map(m => [Number(m[1]), Number(m[2])]);
if (tail.length !== 9) throw new Error('unexpected printable tail ranges');
function printableCheck(cp, plane) {
  const x = cp & 0xffff, upper = x >>> 8;
  let lowerStart = 0;
  for (let i = 0; i < plane.uppers.length; i += 2) {
    const [u, count] = plane.uppers.slice(i, i + 2);
    if (upper < u) break;
    if (upper === u) for (let j = lowerStart; j < lowerStart + count; ++j) if (plane.lowers[j] === (x & 255)) return false;
    lowerStart += count;
  }
  let remaining = x, current = true;
  for (let i = 0; i < plane.normal.length;) {
    const byte = plane.normal[i++];
    const length = byte & 128 ? ((byte & 127) << 8) | plane.normal[i++] : byte;
    remaining -= length;
    if (remaining < 0) break;
    current = !current;
  }
  return current;
}
function isPrintable(cp) {
  if (cp < 32) return false;
  if (cp < 127) return true;
  if (cp < 0x10000) return printableCheck(cp, planes[0]);
  if (cp < 0x20000) return printableCheck(cp, planes[1]);
  return !tail.some(([start, end]) => start <= cp && cp < end);
}
const grapheme = requiredMatch(unicode, /pub mod grapheme_extend \{([\s\S]*?)\n\}/, 'grapheme_extend');
const headers = [...requiredMatch(grapheme, /static SHORT_OFFSET_RUNS:[^=]+ = \[([\s\S]*?)\];/, 'SHORT_OFFSET_RUNS')
  .matchAll(/ShortOffsetRunHeader::new\((\d+), (\d+)\)/g)].map(m => [Number(m[1]), Number(m[2])]);
const offsets = numbers(requiredMatch(grapheme, /static OFFSETS:[^=]+ = \[([\s\S]*?)\];/, 'OFFSETS'));
if (headers.length !== 33 || offsets.length !== 767 || headers.at(-1)[1] <= 0x10ffff) throw new Error('unexpected grapheme table');
function isGraphemeExtend(cp) {
  if (cp < 0x300) return false;
  let lo = 0, hi = headers.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (headers[mid][1] <= cp) lo = mid + 1; else hi = mid;
  }
  const last = lo;
  let index = headers[last][0];
  const length = (headers[last + 1]?.[0] ?? offsets.length) - index;
  const total = cp - (headers[last - 1]?.[1] ?? 0);
  let sum = 0;
  for (let i = 0; i < length - 1; ++i) {
    sum += offsets[index];
    if (sum > total) break;
    ++index;
  }
  return index % 2 === 1;
}
const ranges = [];
let graphemeCount = 0;
for (let cp = 0; cp <= 0x10ffff; ++cp) {
  const extend = isGraphemeExtend(cp);
  if (extend) ++graphemeCount;
  if (extend || !isPrintable(cp)) {
    if (ranges.at(-1)?.[1] === cp - 1) ranges.at(-1)[1] = cp; else ranges.push([cp, cp]);
  }
}
if (graphemeCount !== 2232) throw new Error(`pinned Grapheme_Extend count ${graphemeCount} != 2232`);
if (process.argv[3]) {
  const mask = readFileSync(process.argv[3]);
  if (mask.length !== 0x110000) throw new Error('oracle mask must cover all Unicode codepoints');
  let range = 0, checked = 0;
  for (let cp = 128; cp <= 0x10ffff; ++cp) {
    if (cp >= 0xd800 && cp <= 0xdfff) continue;
    while (ranges[range]?.[1] < cp) ++range;
    const escaped = ranges[range] && ranges[range][0] <= cp ? 1 : 0;
    if (mask[cp] !== escaped) throw new Error(`Rust Debug disagreement at U+${cp.toString(16)}`);
    ++checked;
  }
  console.log(`text-debug oracle comparisons: ${checked} non-ASCII scalars; 0 disagreements`);
}
// Keep the canonical table emitted by gen-runtime-trap-messages.mjs from
// JetFoundation/Text/RustDebug.jet. Do not introduce a second runtime table.
const canonicalSource = readFileSync('Compiler/JetRuntime/Source/TrapMessages.jet', 'utf8');
const canonical = [...requiredMatch(canonicalSource,
  /TRAP_DEBUG_RANGES :: prep \{ \[I64\]\{([\s\S]*?)\}\s*\}/, 'TRAP_DEBUG_RANGES')
  .matchAll(/I64\{(\d+)\}/g)].map(m => Number(m[1]));
let oldRange = 0, newRange = 0, compared = 0;
for (let cp = 128; cp <= 0x10ffff; ++cp) {
  if (cp >= 0xd800 && cp <= 0xdfff) continue;
  while (canonical[oldRange + 1] < cp) oldRange += 2;
  while (ranges[newRange]?.[1] < cp) ++newRange;
  const oldEscape = canonical[oldRange] <= cp && cp <= canonical[oldRange + 1];
  const newEscape = !!ranges[newRange] && ranges[newRange][0] <= cp;
  if (oldEscape !== newEscape) throw new Error(`canonical Trap table disagrees at U+${cp.toString(16)}`);
  ++compared;
}
console.log(`canonical Trap debug table: ${canonical.length / 2} ranges; ${compared} non-ASCII comparisons; 0 disagreements`);
console.log(`pinned rustc ${version} source tarball sha256 ${tarSha}; ${graphemeCount} Grapheme_Extend scalars`);
if (process.argv[4]) {
  const rows = readFileSync(process.argv[4], 'utf8').trimEnd().split('\n').map((line, index) => {
    const parts = line.split('\t');
    if (parts.length !== 2 || parts.some(s => !/^(?:[0-9a-f]{2})*$/.test(s))) throw new Error(`invalid golden row ${index}`);
    return parts;
  });
  const literal = hex => `"${hex.match(/../g)?.map(byte => `\\x${byte}`).join('') ?? ''}"`;
  const header = `// GENERATED by Tools/agent/gen-text-debug-tables.mjs from rustc ${version} (${commit}) str Debug.\n`
    + '// Source semantics: library/core/src/fmt/mod.rs:2916-2960. Expected outputs, not native receipts.\n'
    + 'static const struct { const char *input, *expected; int64_t input_length, expected_length; } debug_goldens[] = {\n'
    + rows.map(([input, expected]) => `    { ${literal(input)}, ${literal(expected)}, ${input.length / 2}, ${expected.length / 2} },\n`).join('')
    + '};\n';
  writeFileSync('Compiler/JetRuntime/Tests/TextDebugGoldens.h', header);
  console.log(`text-debug golden vectors: ${rows.length}; pinned rustc ${version}`);
}
