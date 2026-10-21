#!/usr/bin/env node
// D-SIGIL-ALGEBRA1=B. Usage: --check|--write [absolute file/directory ...]
// --self-test exercises precedence and opaque/non-value grammar. Only .jet files
// are visited: generator templates and other-language sources are never inputs.
// Grammar: JetParser/Source/Parser/Expressions.jet:128-155,278-286;
// compounds: Statements.jet:1831-1835; E0398: State.jet:343-349.
// ^ was power in ratified Jet. Explicit, reviewed caret decisions below retain
// the original meaning of Rust-origin XOR sites, independent of directory.
// Strings (including interpolations), prep, foreign bodies, types and arm heads
// are opaque. Dirty files are reported and never written. --write emits a JSON
// receipt outside the repository for exact-path commits and owner handoff.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { tokenize } from './migrate-copy-sigil.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const SCRATCH = resolve(homedir(), '.cache/jet-dev/scratch/BitOpsMigration');
const methods = { '&': 'bit_and', '|': 'bit_or', '~|': 'bit_xor', '^': 'pow' };
const precedence = { '??': 1, '..': 2, '..<': 2, '||': 3, '&&': 4, '==': 5, '!=': 5, '<': 5, '<=': 5, '>': 5, '>=': 5, '<=>': 5, '|': 6, '~|': 7, '&': 8, '<<': 10, '>>': 10, '+': 11, '-': 11, '*': 12, '/': 12, '/%': 12, '%': 12, '%%': 12, '^': 14 };
const keywords = new Set(['fn', 'if', 'else', 'loop', 'return', 'break', 'yield', 'in', 'use', 'as', 'struct', 'enum', 'trait', 'type', 'impl', 'prep', 'let', 'where', 'task', 'shared']);
const groups = new Set(['(', '[', '{']);
// Loop-control `next` is a whole statement (Statements.jet:552-554). Elsewhere it is an ordinary identifier operand (`next & x`, `x | next`).
function loopNext(ts, n) {
  const t = ts[n], prev = ts[n - 1], following = ts[n + 1];
  if (t?.text !== 'next' || (prev && !prev.beforeTerminator && ![';', '{', '}'].includes(prev.text))) return false;
  return t.beforeTerminator || !following || following.text === ';' || following.text === '}' || (following.text === '(' && ts[n + 2]?.kind === 'identifier' && ts[n + 3]?.text === ')');
}
// Each line was reviewed for meaning; XOR rows are latent power-vs-XOR bugs.
const caretDecisions = new Map([
  ['Compiler/JetCli/Source/Cli/Dev/DbOptions.jet:310', ['pow', 'UTF-8 payload modulus is two raised to payload width']],
  ['Compiler/JetRuntime/Source/Alloc.jet:216', ['bit_xor', 'unsigned ordering flips the sign bit']],
  ['Compiler/JetRuntime/Source/CAbiSession.jet:30', ['bit_xor', 'unsigned session ordering flips the sign bit']],
  ['Compiler/JetRuntime/Source/Collections.jet:741', ['bit_xor', 'addition overflow compares operand/result sign bits']],
  ['Compiler/JetRuntime/Source/Collections.jet:750', ['bit_xor', 'subtraction overflow compares operand/result sign bits']],
  ['Compiler/JetRuntime/Source/Collections.jet:1254', ['bit_xor', 'iterator addition overflow compares sign bits']],
  ['Compiler/JetRuntime/Source/CountMinSketch.jet:22', ['bit_xor', 'FNV hash mixes each byte before multiplying']],
  ['Compiler/JetRuntime/Source/HostNetPoller.jet:28', ['bit_xor', 'bucket hash mixes shifted address bits']],
  ['Compiler/JetRuntime/Source/HostNetResolver.jet:13', ['bit_xor', '32-bit sign extension flips bit 31 before subtracting']],
  ['Compiler/JetRuntime/Source/MemRegistry.jet:24', ['bit_xor', 'unsigned ordering flips the sign bit']],
  ['Compiler/JetRuntime/Source/MemStorage.jet:42', ['bit_xor', 'unsigned ordering flips the sign bit']],
  ['Compiler/JetRuntime/Source/TaskGroup.jet:160', ['bit_xor', 'unsigned task priority ordering flips the sign bit']],
  ['Compiler/JetRuntime/Source/TaskGroup.jet:161', ['bit_xor', 'unsigned task priority ordering flips the sign bit']],
  ['Compiler/JetRuntime/Source/World.jet:395', ['bit_xor', 'xorshift PRNG mixes a left-shifted state']],
  ['Compiler/JetRuntime/Source/World.jet:396', ['bit_xor', 'xorshift PRNG mixes a masked right-shifted state']],
  ['Core/Math/Math.jet:336', ['pow', 'integer exponentiation helper base and exponent']],
  ['Examples/features/math/int_runtime_boundary.jet:25', ['pow', 'exact integer power boundary example 3 raised to 80']],
  ['Jetpack/Store/Tests/ArtifactStore.jet:492', ['bit_xor', 'tamper test toggles one bit of the last stored byte']],
]);

function foreignOpaque(source) {
  // FFI bodies are lexically raw; braces/quotes in C must not enter Jet's
  // interpolated-string scanner. Replace raw contents with spaces, not bytes
  // from the original, retaining exact JS offsets for subsequent edits.
  return source.replace(/(#FFI\s*\([^)]*\)[\s\S]*?\bfn\b[^{}]*\{\s*""")([\s\S]*?)(""")/g,
    (_, head, body, end) => head + body.replace(/[^\n\r]/g, ' ') + end);
}
function contexts(ts, source) {
  const blocked = new Set();
  const mask = (a, b) => { for (let k = a; k < b; k++) blocked.add(k); };
  for (let n = 0; n < ts.length; n++) {
    if (ts[n].template) blocked.add(n);
    if (ts[n].text === 'prep' && ts[n - 1]?.text !== '.') {
      let k = n + 1;
      while (k < ts.length && !['{', ';'].includes(ts[k].text) && !ts[k].beforeTerminator) k++;
      if (ts[k]?.text === '{' && ts[k].close !== undefined) {
        k = ts[k].close + 1;
        while (ts[k]?.text === 'else') {
          k++;
          while (k < ts.length && !['{', ';'].includes(ts[k].text)) k++;
          if (ts[k]?.close === undefined) break;
          k = ts[k].close + 1;
        }
      }
      mask(n, k);
    }
    if (ts[n].text === 'fn') {
      let k = n + 1;
      while (k < ts.length && !['{', ';'].includes(ts[k].text)) {
        if (groups.has(ts[k].text) && ts[k].close !== undefined) k = ts[k].close + 1;
        else k++;
      }
      mask(n, k);
    }
    if (ts[n].text === 'type') {
      let k = n + 1;
      while (k < ts.length && ts[k].text !== ';' && !ts[k].beforeTerminator) k++;
      mask(n, k);
    }
    // A type annotation ends at its initializer/separator, not at a union bar.
    // Colons in literal fields/call labels have value RHS, so only declarations
    // and parameter signatures (already masked above) use this branch.
    if (['struct', 'enum', 'trait'].includes(ts[n].text)) {
      let k = n + 1;
      while (k < ts.length && ts[k].text !== '{') k++;
      mask(n, k);
      const close = ts[k]?.close;
      if (close !== undefined) {
        for (let j = k + 1; j < close; j++) {
          // Method signatures nest parameter annotations; only field colons at body depth declare fields.
          if (groups.has(ts[j].text) && ts[j].close !== undefined) { j = ts[j].close; continue; }
          if (ts[j].text !== ':') continue;
          let end = j + 1;
          while (end < close && ![',', ';', '=', '::', ':='].includes(ts[end].text) && !ts[end].beforeTerminator) {
            if (groups.has(ts[end].text) && ts[end].close !== undefined) end = ts[end].close + 1;
            else end++;
          }
          mask(j + 1, end); j = end - 1;
        }
      }
    }
    // Dispatch heads are patterns, including numeric/string alternatives.
    if (ts[n].text === '{' && (ts[n - 1]?.text === '==' || ts[n - 1]?.text === 'if')) {
      const close = ts[n].close;
      if (close === undefined) continue;
      let head = n + 1;
      for (let k = head; k < close; k++) {
        if (groups.has(ts[k].text) && ts[k].close !== undefined) { k = ts[k].close; continue; }
        if (['->', '=>'].includes(ts[k].text)) { mask(head, k); head = k + 1; }
        else if (ts[k].beforeTerminator || ts[k].text === ';') head = k;
      }
    }
    // Legacy closure bars are never binary operations. Current Jet lambdas
    // use parameter parentheses, which carry the same annotation protection.
    if (ts[n].text === '|' && (n === 0 || ['=', '::', ':=', '(', ',', '->', '{'].includes(ts[n - 1].text))) {
      let k = n + 1;
      while (k < ts.length && ts[k].text !== '|' && !ts[k].beforeTerminator) k++;
      if (ts[k]?.text === '|') mask(n, k + 1);
    }
    if (ts[n].text === '(' && ts[n].close !== undefined) {
      const close = ts[n].close;
      if (['->', '=>'].includes(ts[close + 1]?.text) && ts.slice(n + 1, close).some((t) => t.text === ':')) mask(n, close + 1);
    }
    if (ts[n].text === '[' && ts[n].close !== undefined && ts[ts[n].close + 1]?.text === '{') mask(n + 1, ts[n].close);
    if (ts[n].text === '[' && ts[n].close !== undefined && ts[n - 1]?.text === '-' && ts[ts[n].close + 1]?.text === '>') mask(n, ts[n].close + 2);
    if (ts[n].text === ':' && ts[n + 1]?.text !== '=' && (ts[n + 1]?.text === '&' || /^[A-Z]/.test(ts[n + 1]?.text ?? ''))) {
      let k = n + 1;
      while (k < ts.length && !ts[k].beforeTerminator && !['=', '::', ':=', ',', ';', '}', '{', '->'].includes(ts[k].text)) {
        if (groups.has(ts[k].text) && ts[k].close !== undefined) k = ts[k].close + 1;
        else k++;
      }
      if (ts[k]?.text === '=') mask(n + 1, k);
    }
    if (ts[n].text === '.' && /^[A-Z]/.test(ts[n + 1]?.text ?? '')) {
      let k = n + 2, hasBar = false;
      while (k < ts.length) {
        if (groups.has(ts[k].text) && ts[k].close !== undefined) { k = ts[k].close + 1; continue; }
        if (ts[k].text === '|' && ts[k + 1]?.text === '.') { hasBar = true; k += 3; continue; }
        if (ts[k].text === '.' && ts[k + 1]?.kind === 'identifier') { k += 2; continue; }
        break;
      }
      if (hasBar) mask(n, k);
    }
  }
  return blocked;
}

export function migrate(source, { caretAt = () => 'pow' } = {}) {
  const ts = tokenize(foreignOpaque(source)), blocked = contexts(ts, source);
  const sites = [], unresolved = [], seen = new Set();
  for (const t of ts) if (t.text === '^') t.caretMethod = caretAt(source.slice(0, t.start).split('\n').length);
  const prec = (token) => token.text === '^' && token.caretMethod === 'bit_xor' ? 7 : precedence[token.text];
  const method = (token) => token.text === '^' ? token.caretMethod : methods[token.text];
  function parse(n, minimum = 1) {
    const t = ts[n];
    if (!t || blocked.has(n) || t.template || (t.text === 'next' ? loopNext(ts, n) : keywords.has(t.text))) return null;
    let next = n + 1, simple = true, children = [];
    if (['-', '!', '&', '^', '~', '*', '...'].includes(t.text)) {
      const inner = parse(n + 1, 13);
      if (!inner) return null;
      next = inner.next; simple = false; children = [inner];
    } else if (groups.has(t.text) && t.close !== undefined) next = t.close + 1;
    else if (t.text === '.' && ts[next]?.kind === 'identifier') next++;
    else if (!['identifier', 'literal'].includes(t.kind)) return null;
    if (t.kind === 'literal' && /^\d/.test(t.text)) simple = false;
    while (next < ts.length && !blocked.has(next) && !ts[next].beforeTerminator) {
      const p = ts[next];
      if (['.', '?.'].includes(p.text) && (ts[next + 1]?.kind === 'identifier' || ts[next + 1]?.text === '*')) next += 2;
      else if (['(', '['].includes(p.text) && p.close !== undefined) next = p.close + 1;
      else if (p.text === '{' && p.close !== undefined && /^[A-Z]/.test(t.text)) next = p.close + 1;
      else if (p.text === '?') next++;
      else if (p.text === '<' && ts[next - 1].end === p.start) {
        let k = next + 1, depth = 1;
        while (k < ts.length && depth > 0 && !ts[k].beforeTerminator && ![';', '{', '='].includes(ts[k].text)) {
          if (ts[k].text === '<') depth++;
          if (ts[k].text === '>') depth--;
          if (ts[k].text === '>>') depth -= 2;
          k++;
        }
        if (depth <= 0 && ['(', '.', '{', '?.'].includes(ts[k]?.text)) { for (let j = next; j < k; j++) blocked.add(j); next = k; }
        else break;
      } else break;
    }
    let left = { start: t.start, end: ts[next - 1].end, next, simple, children };
    while (next < ts.length && !blocked.has(next) && !ts[next].beforeTerminator) {
      const op = ts[next], level = prec(op);
      if (!level || level < minimum) break;
      const right = parse(next + 1, level + (op.text === '^' && op.caretMethod === 'pow' ? 0 : 1));
      if (!right) break;
      left = { start: left.start, end: right.end, next: right.next, simple: !!method(op), children: [left, right], op, method: method(op) };
      next = right.next;
    }
    return left;
  }
  function collect(node) {
    for (const child of node.children) collect(child);
    if (node.op?.text === '^' && !node.method && !seen.has(node.op.start)) {
      seen.add(node.op.start);
      unresolved.push({ line: source.slice(0, node.op.start).split('\n').length, before: source.slice(node.start, node.end), method: null, reason: 'requires individual caret meaning review' });
    }
    if (node.method && !seen.has(node.op.start)) {
      seen.add(node.op.start);
      sites.push({ start: node.start, end: node.end, operator: node.op.text, method: node.method, line: source.slice(0, node.op.start).split('\n').length, before: source.slice(node.start, node.end), node });
    }
  }
  // Visit every group interior too: parse() treats a postfix group as one atom.
  for (let n = 0; n < ts.length; n++) {
    if (blocked.has(n)) continue;
    const node = parse(n);
    if (!node) continue;
    collect(node);
    // No skipping here: group interiors must be examined, while seen prevents
    // duplicate sites. Maximal outer nodes below own overlapping repairs.
  }
  function render(node) {
    if (node.method) {
      const [left, right] = node.children;
      let receiver = render(left);
      if (!left.simple) receiver = `(${receiver})`;
      const trivia = source.slice(left.end, node.op.start) + source.slice(node.op.end, right.start);
      const comments = /\/\/|\/\*/.test(trivia) ? trivia : '';
      return `${receiver}.${node.method}(${comments}${render(right)})`;
    }
    return patched(node.start, node.end, node.children);
  }
  function patched(start, end, candidates) {
    const nodes = candidates.filter((n) => n.start >= start && n.end <= end).sort((a, b) => a.start - b.start || b.end - a.end);
    let cursor = start, result = '';
    for (const node of nodes) {
      if (node.start < cursor) continue;
      result += source.slice(cursor, node.start) + render(node); cursor = node.end;
    }
    return result + source.slice(cursor, end);
  }
  // Group atoms must include the independently discovered inner repairs.
  const active = sites.map((s) => s.node);
  function renderWithGroups(node) {
    if (node.method) return render(node);
    return patched(node.start, node.end, active.filter((n) => n !== node && n.start >= node.start && n.end <= node.end));
  }
  // Attach inner roots to atoms (not binary children, which already have ASTs).
  function attach(node) {
    if (!node.children.length) node.children = active.filter((n) => n !== node && n.start > node.start && n.end < node.end);
    else for (const child of node.children) attach(child);
  }
  for (const node of active) attach(node);
  const result = patched(0, source.length, active);
  for (const site of sites) { site.after = renderWithGroups(site.node); delete site.node; }
  return { source: result, sites: sites.sort((a, b) => a.start - b.start), unresolved };
}
function filesAt(path) {
  if (!statSync(path).isDirectory()) return path.endsWith('.jet') ? [path] : [];
  return readdirSync(path, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)).flatMap((entry) =>
    entry.isSymbolicLink() || ['.git', 'target', 'node_modules', '.agent-worktrees'].includes(entry.name) ? [] : filesAt(resolve(path, entry.name)));
}
function dirtyPaths() {
  const rows = execFileSync('git', ['status', '--porcelain=v1', '-z', '--untracked-files=all'], { cwd: ROOT, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }).split('\0');
  const dirty = new Set();
  for (let n = 0; n < rows.length; n++) {
    if (!rows[n]) continue;
    dirty.add(rows[n].slice(3));
    if (/[RC]/.test(rows[n].slice(0, 2))) dirty.add(rows[++n]);
  }
  return dirty;
}
function owner(file) {
  if (/JetBackend\/Source\/Lower\/(?:Constants|Statics|Entry|Callable|Test)\.jet/.test(file)) return 'RustLead8';
  if (/JetBackend\/Source\/Lower\/(?:Lower|Dyn\w*|Realtime|Combinators)\.jet/.test(file)) return 'JetCheckLead';
  if (/JetDriver\/Source\/Driver\/Pipeline.jet|JetBackend\/Source\/Image\/Object.jet/.test(file)) return 'RuntimeLead12';
  return 'unknown (ask Main)';
}
function selfTest() {
  const cases = [
    ['a | b & c', 'a.bit_or(b.bit_and(c))'],
    ['x ^ y + z', 'x.pow(y) + z'],
    ['x ~| y + z', 'x.bit_xor(y + z)'],
    ['a + b | c << d + e', '(a + b).bit_or(c << d + e)'],
    ['a & b | c', 'a.bit_and(b).bit_or(c)'],
    ['a | b | c', 'a.bit_or(b).bit_or(c)'],
    ['a ^ b ^ c', 'a.pow(b.pow(c))'],
    ['-a ^ b', '-a.pow(b)'],
    ['(-a) & b', '(-a).bit_and(b)'],
    ['f(a | b, &x, &items.push(c & d))', 'f(a.bit_or(b), &x, &items.push(c.bit_and(d)))'],
    ['out[i & 3] = U8{x & 255}', 'out[i.bit_and(3)] = U8{x.bit_and(255)}'],
    ['((a << 6) | b) & 255', '((a << 6).bit_or(b)).bit_and(255)'],
    ['a || b && c', 'a || b && c'],
    ['if x == { .A | .B -> a | b\n 0 | 3 -> c & d\n else -> 0 }', 'if x == { .A | .B -> a.bit_or(b)\n 0 | 3 -> c.bit_and(d)\n else -> 0 }'],
    ['fn f(x: A | B) -> T (E | F)! { x & y }', 'fn f(x: A | B) -> T (E | F)! { x.bit_and(y) }'],
    ['struct S { x: A | B\n y: &T }\ntype X = A | B', 'struct S { x: A | B\n y: &T }\ntype X = A | B'],
    ['x :: |a, b| a | b', 'x :: |a, b| a.bit_or(b)'],
    ['f :: (x: A | B) -> x & y', 'f :: (x: A | B) -> x.bit_and(y)'],
    ['prep { a | b }\nprep loop x in T.$fields { a & b }\nx & y', 'prep { a | b }\nprep loop x in T.$fields { a & b }\nx.bit_and(y)'],
    ['"a | b {c & d}" `a & b` \'|\' // a | b\n/* a & b */ a | b', '"a | b {c & d}" `a & b` \'|\' // a | b\n/* a & b */ a.bit_or(b)'],
    ['#FFI(c) fn f() { """int f() { return "a" | b; }""" }\na | b', '#FFI(c) fn f() { """int f() { return "a" | b; }""" }\na.bit_or(b)'],
    ['a\n&items.push(b)', 'a\n&items.push(b)'],
    ['f<T>(x) | y', 'f<T>(x).bit_or(y)'],
    ['tail :: [Int | String]{2, \"three\"}', 'tail :: [Int | String]{2, \"three\"}'],
    ['if ty == .A(x) | .B(x) -> a | b', 'if ty == .A(x) | .B(x) -> a.bit_or(b)'],
    ['task ^sender { a & b }', 'task ^sender { a.bit_and(b) }'],
    ['f :: (x: Int) -[FS | Net]> x & y', 'f :: (x: Int) -[FS | Net]> x.bit_and(y)'],
    ['v: A | B = f()', 'v: A | B = f()'],
    ['a /* keep */ | b', 'a.bit_or( /* keep */  b)'],
    ['prep if x { a | b } else { c & d }\nx | y', 'prep if x { a | b } else { c & d }\nx.bit_or(y)'],
    ['trait T {\n    fn m(x: A) { x & y }\n}', 'trait T {\n    fn m(x: A) { x.bit_and(y) }\n}'],
    ['trait T {\n    fn m(a: A, b: B) -> Int { a & b }\n}', 'trait T {\n    fn m(a: A, b: B) -> Int { a.bit_and(b) }\n}'],
    ['next ~| x', 'next.bit_xor(x)'],
    ['next & x', 'next.bit_and(x)'],
    ['x & next', 'x.bit_and(next)'],
    ['loop {\n    next\n    a & b\n}', 'loop {\n    next\n    a.bit_and(b)\n}'],
    ['loop {\n    if c { next }\n    a | b\n}', 'loop {\n    if c { next }\n    a.bit_or(b)\n}'],
    ['fn __helper(x: U64) -> U64 { x & 255 | __flag }\nfn __next(next: U64) -> U64 { next ~| __helper(next) }\nstruct __Box { __v: A | B\n next: &T }', 'fn __helper(x: U64) -> U64 { x.bit_and(255).bit_or(__flag) }\nfn __next(next: U64) -> U64 { next.bit_xor(__helper(next)) }\nstruct __Box { __v: A | B\n next: &T }'],
  ];
  for (const [input, expected] of cases) {
    assert.equal(migrate(input).source, expected, input);
    assert.equal(migrate(expected).sites.length, 0, `idempotence: ${input}`);
  }
  assert.equal(migrate('a ^ b + c', { caretAt: () => 'bit_xor' }).source, 'a.bit_xor(b + c)');
  console.log(`bit-operator self-test: ${cases.length + 1} cases passed`);
}
function main(args) {
  if (args.length === 1 && args[0] === '--self-test') return selfTest();
  const mode = args.shift();
  if (!['--check', '--write'].includes(mode) || args.some((a) => a.startsWith('--'))) throw new Error('usage: --check|--write [paths ...] or --self-test');
  const files = [...new Set((args.length ? args : ['Compiler', 'Core', 'Jetpack', 'Tools/testkit', 'Examples']).flatMap((p) => filesAt(resolve(ROOT, p))))].sort();
  const receipt = { mode, counts: { Compiler: 0, Core: 0, Jetpack: 0, 'Tools/testkit': 0, Examples: 0 }, changed: [], dirty: [], sites: [], unresolved: [] }, initialDirty = dirtyPaths();
  for (const path of files) {
    const file = relative(ROOT, path);
    if (file.startsWith('../')) throw new Error(`outside worktree: ${path}`);
    // Dedicated retirement diagnostics are intentional failing fixtures.
    const stderr = path.replace(/\.jet$/, '.stderr');
    if (existsSync(stderr) && /\bE0398\b/.test(readFileSync(stderr, 'utf8'))) continue;
    const source = readFileSync(path, 'utf8');
    const migrated = migrate(source, { caretAt: (line) => caretDecisions.get(`${file}:${line}`)?.[0] ?? null });
    for (const site of migrated.unresolved) {
      receipt.unresolved.push({ file, ...site });
      console.error(`${file}:${site.line}\tAMBIGUOUS ^\t${site.before}`);
    }
    if (!migrated.sites.length) continue;
    const area = file.startsWith('Tools/testkit/') ? 'Tools/testkit' : file.split('/')[0];
    receipt.counts[area] = (receipt.counts[area] ?? 0) + migrated.sites.length;
    for (const site of migrated.sites) {
      receipt.sites.push({ file, ...site });
      console.log(`${file}:${site.line}\t${site.operator} → ${site.method}\t${site.before.replaceAll('\n', '\\n')} → ${site.after.replaceAll('\n', '\\n')}`);
    }
    if (mode === '--write') {
      if (initialDirty.has(file) || dirtyPaths().has(file)) { receipt.dirty.push({ file, owner: owner(file), sites: migrated.sites.length }); continue; }
      if (readFileSync(path, 'utf8') !== source) throw new Error(`concurrent edit: ${file}`);
      writeFileSync(path, migrated.source); receipt.changed.push(file);
    }
  }
  mkdirSync(SCRATCH, { recursive: true });
  const carets = receipt.sites.filter((s) => s.operator === '^').map((s) => `${s.file}:${s.line}\t${s.before}\t${s.method}\t${caretDecisions.get(`${s.file}:${s.line}`)?.[1] ?? 'AMBIGUOUS'}\t${s.method === 'bit_xor' ? 'latent semantic bug: was parsed as power' : 'power'}\n`);
  writeFileSync(resolve(SCRATCH, 'caret-sites.tsv'), 'site\texpression\tmethod\treason\tstatus\n' + carets.join(''));
  writeFileSync(resolve(SCRATCH, `${mode.slice(2)}.json`), JSON.stringify(receipt, null, 2) + '\n');
  console.log(JSON.stringify({ counts: receipt.counts, total: receipt.sites.length, changed: receipt.changed.length, dirty: receipt.dirty }));
  if (receipt.unresolved.length || (mode === '--check' && receipt.sites.length)) process.exitCode = 1;
}
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) main(process.argv.slice(2));
