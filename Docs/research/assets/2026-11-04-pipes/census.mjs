// Pipe census: where data flows through chains that dot methods cannot reach.
// Usage: node census.mjs <repo-root> [out.json]   (scans tracked .jet files in Core, Compiler, Jetpack, Examples, Tools)
// Writes the per-site records to out.json (default ./census.json) and prints a summary.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const root = process.argv[2];
const DIRS = ['Core', 'Compiler', 'Jetpack', 'Examples', 'Tools'];
const gitEnv = { ...process.env }; delete gitEnv.GIT_INDEX_FILE;
const files = execFileSync('git', ['ls-files', '--', ...DIRS.map(d => `${d}/*.jet`)], {
  cwd: root, encoding: 'utf8', env: gitEnv, maxBuffer: 1 << 28,
}).split('\n').filter(Boolean);
// Wrappers that are sinks or constructors, not data stages.
const SINKS = new Set(['print', 'eprint', 'assert', 'assert_eq', 'assert_ne', 'panic', 'require', 'Ok', 'Err', 'Val', 'Some', 'debug', 'println']);
const KEYWORDS = new Set(['if', 'loop', 'fn', 'return', 'match', 'else', 'in', 'use', 'pub', 'struct', 'enum', 'impl', 'trait', 'break', 'next', 'defer', 'self']);

// ---------- lexer: strip comments and string bodies, keep line numbers ----------
function lex(src) {
  const toks = [];
  let i = 0, line = 1;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    if (c === '\n') { line++; i++; toks.push({ t: 'nl', line }); continue; }
    if (c === ' ' || c === '\t' || c === '\r') { i++; continue; }
    if (c === '/' && src[i + 1] === '/') { while (i < n && src[i] !== '\n') i++; continue; }
    if (c === '/' && src[i + 1] === '*') { i += 2; while (i < n && !(src[i] === '*' && src[i + 1] === '/')) { if (src[i] === '\n') line++; i++; } i += 2; continue; }
    if (c === '"') {
      const triple = src.startsWith('"""', i);
      i += triple ? 3 : 1;
      while (i < n) {
        if (triple && src.startsWith('"""', i)) { i += 3; break; }
        if (!triple && src[i] === '"') { i++; break; }
        if (src[i] === '\\') { i += 2; continue; }
        if (src[i] === '\n') line++;
        i++;
      }
      toks.push({ t: 'str', line });
      continue;
    }
    if (c === "'" && (src[i + 2] === "'" || (src[i + 1] === '\\' && src[i + 3] === "'"))) {
      i += src[i + 1] === '\\' ? 4 : 3; toks.push({ t: 'str', line }); continue;
    }
    if (/[A-Za-z_]/.test(c)) {
      let j = i; while (j < n && /[A-Za-z0-9_]/.test(src[j])) j++;
      toks.push({ t: 'id', v: src.slice(i, j), line }); i = j; continue;
    }
    if (/[0-9]/.test(c)) { let j = i; while (j < n && /[0-9A-Za-z_.]/.test(src[j]) && !(src[j] === '.' && src[j + 1] === '.')) j++; toks.push({ t: 'num', line }); i = j; continue; }
    const two = src.slice(i, i + 2);
    if (['::', ':=', '->', '??', '?(', '..', '==', '!=', '<=', '>=', '&&', '||'].includes(two)) { toks.push({ t: 'op', v: two, line }); i += 2; continue; }
    toks.push({ t: 'op', v: c, line }); i++;
  }
  return toks;
}

// ---------- expression spine analysis ----------
// For each call token, build: kind (free|module|method|ctor|sink), args (token ranges).
function analyzeCalls(toks, aliases) {
  // match parens
  const match = new Map(); const stack = [];
  toks.forEach((tk, k) => {
    if (tk.t !== 'op') return;
    if (tk.v === '(' || tk.v === '?(' || tk.v === '[' || tk.v === '{') stack.push(k);
    else if (tk.v === ')' || tk.v === ']' || tk.v === '}') { const o = stack.pop(); if (o !== undefined) { match.set(o, k); match.set(k, o); } }
  });
  const calls = new Map(); // open-paren index -> call
  for (let k = 1; k < toks.length; k++) {
    const tk = toks[k];
    if (tk.t !== 'op' || tk.v !== '(') continue;
    let h = k - 1;
    // generic call f<T>(…): skip back over a balanced <...>
    if (toks[h].t === 'op' && toks[h].v === '>') {
      let depth = 0, q = h;
      for (; q >= 0 && q > h - 40; q--) {
        if (toks[q].t === 'op' && toks[q].v === '>') depth++;
        else if (toks[q].t === 'op' && toks[q].v === '<') { depth--; if (depth === 0) break; }
        else if (toks[q].t === 'nl' || (toks[q].t === 'op' && /[();=]/.test(toks[q].v))) { q = -1; break; }
      }
      if (q > 0 && toks[q - 1].t === 'id') h = q - 1; else continue;
    }
    const head = toks[h];
    if (head.t !== 'id' || KEYWORDS.has(head.v)) continue;
    const prev = toks[h - 1];
    let kind;
    if (prev && prev.t === 'op' && prev.v === '.') {
      const recv = toks[h - 2];
      const before = toks[h - 3];
      const recvIsBareId = recv && recv.t === 'id' && !(before && before.t === 'op' && (before.v === '.' || before.v === ')'));
      if (recvIsBareId && aliases.has(recv.v)) kind = 'module';
      else if (recvIsBareId && /^[A-Z]/.test(recv.v)) kind = 'static';
      else kind = 'method';
    } else if (prev && prev.t === 'id' && prev.v === 'fn') continue;
    else if (SINKS.has(head.v)) kind = 'sink';
    else if (/^[A-Z]/.test(head.v)) kind = 'ctor';
    else kind = 'free';
    const close = match.get(k);
    if (close === undefined) continue;
    // split args at top-level commas
    const args = []; let start = k + 1; let depth = 0;
    for (let q = k + 1; q < close; q++) {
      const t = toks[q];
      if (t.t === 'op' && ['(', '?(', '[', '{'].includes(t.v)) depth++;
      else if (t.t === 'op' && [')', ']', '}'].includes(t.v)) depth--;
      else if (depth === 0 && t.t === 'op' && t.v === ',') { args.push([start, q]); start = q + 1; }
    }
    if (close > start) args.push([start, close]);
    calls.set(k, { k, head: h, kind, name: head.v, close, args, line: head.line });
  }
  return { calls, match };
}

// The "flow" of an argument range: the outermost call that spans the whole range
// (allowing a trailing `?(…)` and allowing a leading receiver for method chains).
function wholeCallIn(range, toks, calls, match) {
  let [a, b] = range;
  while (a < b && toks[a].t === 'nl') a++;
  while (b > a && toks[b - 1].t === 'nl') b--;
  // strip leading capability marks
  if (a < b && toks[a].t === 'op' && (toks[a].v === '&' || toks[a].v === '^')) a++;
  // strip trailing ?(…)
  if (b - 1 > a && toks[b - 1].t === 'op' && toks[b - 1].v === ')') {
    const o = match.get(b - 1);
    if (o !== undefined && toks[o].v === '?(') b = o;
  }
  if (b - 1 <= a || toks[b - 1].t !== 'op' || toks[b - 1].v !== ')') return null;
  const open = match.get(b - 1);
  const c = calls.get(open);
  if (!c) return null;
  // the call must start the range (free) or its receiver chain must start the range (method)
  if (c.kind === 'method') {
    // receiver chain spans a..head-2; it must be a simple chain (ids, dots, calls, index)
    for (let q = a; q < c.head - 1; q++) {
      const t = toks[q];
      if (t.t === 'op' && !['.', '(', ')', '[', ']', '?(', '{', '}', ',', '::'].includes(t.v)) {
        // operators at top level of receiver mean it is not a pure chain
        let inside = false;
        for (let r = a; r < q; r++) { const m = match.get(r); if (m !== undefined && m > q && r < q) { inside = true; break; } }
        if (!inside) return null;
      }
    }
    return c;
  }
  return c.head === a || (c.head === a + 2 && toks[a + 1].v === '.') ? c : null;
}

function receiverCall(c, toks, calls, match) {
  // for a method call, the receiver range is from start-of-chain to head-2
  if (c.kind !== 'method' && c.kind !== 'module') return null;
  const end = c.head - 1; // the '.'
  let q = end - 1;
  if (q < 0) return null;
  if (toks[q].t === 'op' && toks[q].v === ')') {
    const o = match.get(q);
    if (o === undefined) return null;
    if (toks[o].v === '?(') { q = o - 1; if (toks[q]?.v !== ')') return null; }
    const open = match.get(q);
    return calls.get(open) || null;
  }
  return null;
}

function spine(c, toks, calls, match, seen) {
  // returns array of {kind,name,line} from outermost to innermost data source
  const out = [];
  let cur = c;
  while (cur && !seen.has(cur.k)) {
    seen.add(cur.k);
    out.push({ kind: cur.kind, name: cur.name, line: cur.line });
    if (cur.kind === 'method') { cur = receiverCall(cur, toks, calls, match); if (cur) out[out.length - 1].via = 'receiver'; continue; }
    // prefix call: follow the first argument that is a whole call
    let nextCall = null;
    for (const [ai, r] of cur.args.entries()) { const w = wholeCallIn(r, toks, calls, match); if (w && !(w.kind === 'method' && MEASURE.has(w.name))) { nextCall = w; out[out.length - 1].via = ai === 0 ? 'arg0' : `arg${ai}`; out[out.length - 1].argc = cur.args.length; break; } }
    cur = nextCall;
  }
  return out;
}

const isPrefix = (k) => k === 'free' || k === 'module' || k === 'static';
// Methods that measure a side value (a length, a span) rather than carry the data onward.
const MEASURE = new Set(['len', 'count_bytes', 'count', 'hash', 'span', 'size', 'is_empty']);

// ---------- temporaries chains (line based) ----------
function tempChains(lines) {
  const chains = [];
  const bind = /^(\s*)([a-z_][A-Za-z0-9_]*)\s*(::|:=)\s*(.+)$/;
  const used = (name, text) => new RegExp(`(?<![A-Za-z0-9_.])${name}(?![A-Za-z0-9_])`).test(text);
  const countUses = (name, text) => (text.match(new RegExp(`(?<![A-Za-z0-9_.])${name}(?![A-Za-z0-9_])`, 'g')) || []).length;
  const strip = (s) => s.replace(/"(?:[^"\\]|\\.)*"/g, '""').replace(/\/\/.*$/, '');
  const L = lines.map(strip);
  let i = 0;
  while (i < L.length) {
    const m = bind.exec(L[i]);
    if (!m || /[{(\[,]\s*$/.test(L[i])) { i++; continue; }
    const indent = m[1];
    const stages = [{ line: i + 1, name: m[2], rhs: m[4] }];
    let j = i + 1;
    for (;;) {
      if (j >= L.length) break;
      const prev = stages[stages.length - 1];
      const text = L[j];
      if (!text.startsWith(indent) || /^\s*$/.test(text)) break;
      if (text.length > indent.length && /\s/.test(text[indent.length])) break; // deeper block
      if (/[{(\[,]\s*$/.test(text)) break; // multi-line statement
      const mm = bind.exec(text);
      const rhs = mm ? mm[4] : text.trim();
      if (countUses(prev.name, rhs) !== 1) break;
      // prev.name must not be used after this line in the same block
      let later = false;
      for (let q = j + 1; q < L.length; q++) {
        const t = L[q];
        if (t.trim() && !t.startsWith(indent)) break;
        if (t.trim() && t.length > 0 && t.slice(0, indent.length) === indent && !/\s/.test(t[indent.length] || 'x') && /^\s*}/.test(t)) break;
        if (used(prev.name, t)) { later = true; break; }
      }
      if (later) break;
      if (mm) { stages.push({ line: j + 1, name: mm[2], rhs }); j++; continue; }
      stages.push({ line: j + 1, name: null, rhs }); j++; break;
    }
    if (stages.length >= 3) {
      // classify each hand-off: receiver, first argument, later argument, or other operand
      const handoffs = [];
      for (let s = 1; s < stages.length; s++) {
        const p = stages[s - 1].name; const r = stages[s].rhs.trim();
        if (new RegExp(`^[&^]?${p}\\.`).test(r)) handoffs.push('receiver');
        else if (new RegExp(`\\(\\s*[&^]?${p}\\s*[,)]`).test(r)) handoffs.push('arg0');
        else if (new RegExp(`,\\s*[&^]?${p}\\s*[,)]`).test(r)) handoffs.push('argN');
        else handoffs.push('operand');
      }
      const rhs = stages.map(s => s.rhs);
      chains.push({
        start: stages[0].line, end: stages[stages.length - 1].line, stages: stages.length, handoffs,
        fallback: rhs.some(t => t.includes('??')),
        writes: rhs.some(t => /(^|[(\s,])&[a-z_]/.test(t)),
      });
      i = j;
    } else i++;
  }
  return chains;
}

function chainClass(handoffs) {
  if (handoffs.every(h => h === 'receiver')) return 'receiverOnly';
  if (handoffs.some(h => h === 'operand')) return 'operand';
  if (handoffs.some(h => h === 'argN')) return 'laterArg';
  return 'firstArg';
}

// ---------- per file ----------
const results = { files: 0, perDir: {}, nested: [], mixed: [], temps: [] };
const blank = () => ({
  files: 0, dotChain3: 0, prefixDepth2: 0,
  addressMath3: 0, nested3: 0, nested3FirstArg: 0, nested3LaterArg: 0,
  mixed3: 0, mixed3FirstArg: 0, mixed3LaterArg: 0,
  temps3: 0, tempsReceiverOnly: 0, tempsFirstArg: 0, tempsLaterArg: 0, tempsOperand: 0, tempsFallback: 0, tempsWrites: 0,
});
for (const f of files) {
  const src = readFileSync(`${root}/${f}`, 'utf8');
  results.files++;
  const generated = /generated|do not edit/i.test(src.slice(0, 400));
  const role = generated ? 'generated' : /(^|\/)(Tests?|tests?)\//.test(f) || /Tests?\.jet$/.test(f) ? 'test' : f.startsWith('Examples/') ? 'example' : 'source';
  const dir = f.split('/')[0];
  const ds = [(results.perDir[dir] ??= blank()), (results.perDir[`role:${role}`] ??= blank())];
  const bump = (k) => { for (const d of ds) d[k]++; };
  bump('files');
  const aliases = new Set(['core']);
  for (const m of src.matchAll(/^\s*(?:pub\s+)?use\s+([A-Za-z0-9_.]+)(?:\s*::\s*"[^"]*")?(?:\s+as\s+([A-Za-z_][A-Za-z0-9_]*))?/gm)) {
    // `use core.text` -> text ; `use name :: "./path"` -> name ; `use a.b as c` -> c
    aliases.add(m[2] || m[1].split('.').pop());
  }
  for (const m of src.matchAll(/^\s*(?:pub\s+)?use\s+([A-Za-z_][A-Za-z0-9_]*)\s*::/gm)) aliases.add(m[1]);
  const toks = lex(src);
  const { calls, match } = analyzeCalls(toks, aliases);
  // outermost calls: those not the whole-argument or receiver of another call
  const inner = new Set();
  for (const c of calls.values()) {
    if (c.kind === 'method' || c.kind === 'module') { const r = receiverCall(c, toks, calls, match); if (r) inner.add(r.k); }
    for (const a of c.args) { const w = wholeCallIn(a, toks, calls, match); if (w) inner.add(w.k); }
  }
  for (const c of calls.values()) {
    if (inner.has(c.k)) continue;
    const full = spine(c, toks, calls, match, new Set());
    // sinks (print, assert, Ok, Val) and constructors are transparent wrappers, not stages
    const sink = full.filter(s => s.kind === 'sink' || s.kind === 'ctor').map(s => s.name);
    const sp = full.filter(s => s.kind !== 'sink' && s.kind !== 'ctor');
    if (!sp.length) continue;
    const kinds = sp.map(s => s.kind);
    const allPrefix = kinds.every(isPrefix);
    const allMethod = kinds.every(k => k === 'method');
    // a prefix stage that wraps a method stage: the reader must jump out of a dot chain
    let wrapsChain = false;
    for (let q = 0; q + 1 < kinds.length; q++) if (isPrefix(kinds[q]) && kinds[q + 1] === 'method') wrapsChain = true;
    const laterArg = sp.some(s => s.via && s.via !== 'arg0' && s.via !== 'receiver');
    const rec = { file: f, role, line: sp[0].line, spine: sp.map(s => `${s.kind}:${s.name}${s.via && s.via !== 'receiver' ? '[' + s.via + ']' : ''}`).join(' <- '), sink: sink.join(','), stages: sp.length, laterArg };
    if (sp.length >= 3 && sp.some(s => s.name === 'wrapping')) bump('addressMath3');
    else if (allMethod && sp.length >= 3) bump('dotChain3');
    else if (allPrefix && sp.length === 2) bump('prefixDepth2');
    else if (allPrefix && sp.length >= 3) { bump('nested3'); bump(laterArg ? 'nested3LaterArg' : 'nested3FirstArg'); results.nested.push(rec); }
    else if (!allPrefix && !allMethod && sp.length >= 3 && wrapsChain) { bump('mixed3'); bump(laterArg ? 'mixed3LaterArg' : 'mixed3FirstArg'); results.mixed.push(rec); }
  }
  for (const ch of tempChains(src.split('\n'))) {
    const cls = chainClass(ch.handoffs);
    bump('temps3');
    bump({ receiverOnly: 'tempsReceiverOnly', firstArg: 'tempsFirstArg', laterArg: 'tempsLaterArg', operand: 'tempsOperand' }[cls]);
    if (ch.fallback) bump('tempsFallback');
    if (ch.writes) bump('tempsWrites');
    results.temps.push({ file: f, role, cls, ...ch });
  }
}
writeFileSync(process.argv[3] || 'census.json', JSON.stringify(results, null, 1));
const tot = blank();
for (const [k, d] of Object.entries(results.perDir)) if (!k.startsWith('role:')) for (const [kk, v] of Object.entries(d)) tot[kk] += v;
results.perDir.total = tot;
const keys = Object.keys(tot);
const cols = Object.keys(results.perDir);
console.log(['metric', ...cols].join('\t'));
for (const k of keys) console.log([k, ...cols.map(c => results.perDir[c][k])].join('\t'));

// Focus view: drop the runtime's raw-memory helpers and generated files, then split by what
// a pipe or a method would need (a later argument, or a static or module function).
const focus = (x) => !x.file.startsWith('Compiler/JetRuntime/') && x.role !== 'generated';
const prefixOnly = (x) => !x.laterArg && !/(static|module):/.test(x.spine);
console.log('\nfocus (no Compiler/JetRuntime, no generated files)');
for (const k of ['nested', 'mixed']) {
  const a = results[k].filter(focus);
  console.log(`${k}\ttotal ${a.length}\tfirst-arg free functions only ${a.filter(prefixOnly).length}`
    + `\tlater argument ${a.filter(x => x.laterArg).length}\tstatic or module stage ${a.filter(x => /(static|module):/.test(x.spine)).length}`);
}
const t = results.temps.filter(focus);
const by = (c) => t.filter(x => x.cls === c).length;
console.log(`temps\ttotal ${t.length}\treceiver only ${by('receiverOnly')}\tfirst argument ${by('firstArg')}`
  + `\tlater argument ${by('laterArg')}\toperand ${by('operand')}\twith ?? ${t.filter(x => x.fallback).length}\twith & ${t.filter(x => x.writes).length}`);
