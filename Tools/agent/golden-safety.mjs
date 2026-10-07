// Transitional golden.rs I1 comparator for the Rust-emitting reference CLI.
// Its removal condition is native source-region audit evidence from S12; native
// production objects never need to be translated back into Rust for this scan.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { rustCode } from './test-harness-inventory.mjs';
const COMMON = fileURLToPath(new URL('../../tests/common/mod.rs', import.meta.url));

export function stripVetted(rust, registry = readFileSync(COMMON, 'utf8')) {
  const body = registry.split('pub fn strip_vetted_prelude_modules(')[1]?.split('\n}\n')[0];
  if (!body) throw new Error('missing canonical vetted Prelude registry');
  const modules = new Set([...body.matchAll(/strip_mod\([^\n]*, "([^"]+)"\)/gu)].map((m) => m[1]));
  const markers = new Set([...body.matchAll(/strip_vetted_module\([^\n]*, "([^"]+)"\)/gu)].map((m) => m[1]));
  if (!modules.size || !markers.size) throw new Error('unreadable canonical vetted Prelude registry');
  let text = rust;
  const eraseRegion = (begin, end) => {
    while (true) {
      const start = text.indexOf(begin), stop = text.indexOf(end, start);
      if (start < 0 || stop < start) break;
      text = text.slice(0, start) + text.slice(stop + end.length);
    }
  };
  for (const name of markers) eraseRegion(`// JET_VETTED_UNSAFE_BEGIN: ${name}`, `// JET_VETTED_UNSAFE_END: ${name}`);
  for (const label of ['scheduler-native', 'shared-guard-internal', 'raylib']) eraseRegion(`// jet:${label}-begin`, `// jet:${label}-end`);
  const code = rustCode(text), ranges = [];
  for (const match of code.matchAll(/\bmod\s+(\w+)\s*([;{])/gu)) {
    if (!modules.has(match[1]) && !match[1].startsWith('__jet___c_')) continue;
    let end = match.index + match[0].length;
    if (match[2] === '{') {
      let depth = 1;
      while (end < code.length && depth) { if (code[end] === '{') depth++; else if (code[end] === '}') depth--; end++; }
      if (depth) throw new Error(`unclosed vetted module ${match[1]}`);
    }
    ranges.push([match.index, end]);
  }
  // Overlapping nested vetted modules form one omitted region.
  let output = '', cursor = 0;
  for (const [start, end] of ranges) { if (start >= cursor) output += text.slice(cursor, start); cursor = Math.max(cursor, end); }
  output += text.slice(cursor);
  return output;
}

export function safeGolden(rust, stem, registry) {
  const user = rustCode(stripVetted(rust, registry));
  if (!/\bfn\s+main\s*\(/u.test(user)) return 'generated program has no main';
  if (stem === 'game/raylib_window') return /\bunsafe\s+fn\s+__jet_/u.test(user) ? 'raylib user function is unsafe' : null;
  return /\bunsafe\b/u.test(user) ? 'I1: unsafe outside vetted runtime modules' : null;
}
