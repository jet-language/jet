#!/usr/bin/env node
// Clone gate: inventories every deep copy of a large compiler value in a
// generated stage-zero.rs, maps it to its Jet function and source line, ranks
// it by static execution frequency, and optionally checks it against an
// allow-list. See README.md for the model and the output columns.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { decodeJet } from '../jetc-profile/profile.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));

// Seed types and their size class: 3 = whole program/graph, 2 = one item or
// module, 1 = one syntax/typed node. Every other type takes the largest class
// it contains by value (Vec/Box/Option/Result/tuple); JetMap, JetShared, Rc
// and Arc share their payload, so a copy through them is shallow.
export const DEFAULT_SEEDS = new Map([
  ['SemaRegistrationGraph', 3], ['Program', 3], ['MIRProgram', 3],
  ['SemaRegistrationModule', 2], ['SemaRegistrationFunction', 2], ['SemaRegistrationNominal', 2],
  ['SemaGraphModuleResult', 2], ['Item', 2], ['Func', 2], ['TFunc', 2], ['MIRFunction', 2],
  ['Stmt', 1], ['Expr', 1], ['TStmt', 1], ['TExpr', 1],
]);
const TIER_NAME = ['', 'node', 'item', 'program'];
// Item-sized seeds whose sequences are whole-program values wherever stored.
const MODULE_SIZED = new Set(['SemaRegistrationModule', 'SemaGraphModuleResult']);
const SHALLOW = new Set(['JetMap', 'JetShared', 'JetSharedSlot', 'Rc', 'Arc', 'Weak', 'JetAbsent', 'PhantomData', 'JetSet']);
const SEQUENCE = new Set(['Vec', 'VecDeque', '[]']);
const WRAPPER = new Set(['Box', 'Option', 'Result', 'JetOutcome', '()', 'Cow', 'RefCell', 'Cell']);
const UNKNOWN = null;
const NEVER = { n: '!', a: [] };

// ---------------------------------------------------------------- types

const typeCache = new Map();
export function parseType(text) {
  if (text == null) return UNKNOWN;
  const key = text.trim();
  if (typeCache.has(key)) return typeCache.get(key);
  let i = 0;
  const s = key;
  const ws = () => { while (i < s.length && /\s/.test(s[i])) i++; };
  function one() {
    ws();
    if (s[i] === '&') {
      i++; ws();
      if (s[i] === "'") { while (i < s.length && /[\w']/.test(s[i])) i++; ws(); }
      if (s.startsWith('mut ', i)) i += 4;
      return one();
    }
    if (s.startsWith('dyn ', i)) i += 4;
    if (s.startsWith('impl ', i)) i += 5;
    if (s[i] === '(') {
      i++; const args = [];
      ws();
      while (i < s.length && s[i] !== ')') { const at = i; args.push(one()); ws(); if (s[i] === ',') i++; ws(); if (i === at) i++; }
      i++;
      return args.length === 0 ? { n: 'unit', a: [] } : { n: '()', a: args };
    }
    if (s[i] === '[') {
      i++; const elem = one(); ws();
      if (s[i] === ';') { while (i < s.length && s[i] !== ']') i++; }
      i++;
      return { n: '[]', a: [elem] };
    }
    let name = '';
    for (;;) {
      const m = /^[A-Za-z_]\w*/.exec(s.slice(i));
      if (!m) break;
      name = m[0]; i += m[0].length;
      if (s.startsWith('::', i)) { i += 2; continue; }
      break;
    }
    ws();
    const args = [];
    if (s[i] === '<') {
      i++; ws();
      while (i < s.length && s[i] !== '>') {
        const at = i;
        if (s[i] === "'") { while (i < s.length && /[\w']/.test(s[i])) i++; } else args.push(one());
        ws(); if (s[i] === ',') i++; ws();
        if (i === at) i++;
      }
      i++;
    }
    return { n: name || '?', a: args };
  }
  let t;
  try { t = one(); } catch { t = UNKNOWN; }
  typeCache.set(key, t);
  return t;
}

export function shortName(ident) {
  if (!ident.startsWith('__jet_')) return ident;
  const decoded = decodeJet(ident);
  const tail = decoded.slice(decoded.lastIndexOf(':') + 1);
  return tail || ident;
}

export function showType(t) {
  if (t === UNKNOWN) return '?';
  if (t.n === '[]') return `[${showType(t.a[0])}]`;
  if (t.n === '()') return `(${t.a.map(showType).join(', ')})`;
  if (t.n === 'JetOutcome' && t.a.length === 2 && t.a[1].n === 'JetAbsent') return `${showType(t.a[0])}?`;
  const name = shortName(t.n);
  return t.a.length ? `${name}<${t.a.map(showType).join(', ')}>` : name;
}

function unwrapRef(t) {
  while (t && (t.n === 'Box' || t.n === 'Rc' || t.n === 'Arc') && t.a.length) t = t.a[0];
  return t;
}
function elemOf(t) {
  t = unwrapRef(t);
  if (!t) return UNKNOWN;
  if (SEQUENCE.has(t.n) && t.a.length) return t.a[0];
  return UNKNOWN;
}
function payloadOf(t) {
  t = unwrapRef(t);
  if (!t) return UNKNOWN;
  if ((t.n === 'Option' || t.n === 'Result' || t.n === 'JetOutcome') && t.a.length) return t.a[0];
  return UNKNOWN;
}
function errOf(t) {
  t = unwrapRef(t);
  if (t && (t.n === 'Result' || t.n === 'JetOutcome') && t.a.length > 1) return t.a[1];
  return UNKNOWN;
}

// ---------------------------------------------------------------- lexical masking

// Replace string/char literal contents and comments with spaces, keeping
// column positions, so brackets and keywords can be scanned safely.
function stringEnd(line, from, raw) {
  if (raw < 0) {
    for (let k = from; k < line.length; k++) {
      const c = line[k];
      if (c === '\\') { k++; continue; }
      if (c === '"') return k + 1;
    }
    return -1;
  }
  const close = '"' + '#'.repeat(raw);
  const at = line.indexOf(close, from);
  return at < 0 ? -1 : at + close.length;
}

export function maskLine(line, st) {
  const n = line.length;
  const out = [];
  let copyFrom = 0;
  const blank = (from, to) => { out.push(line.slice(copyFrom, from), ' '.repeat(Math.max(0, to - from))); copyFrom = Math.max(from, to); };
  let i = 0;
  if (st.comment) {
    const end = line.indexOf('*/');
    if (end < 0) return ' '.repeat(n);
    blank(0, end + 2); st.comment = false; i = end + 2;
  } else if (st.str) {
    const end = stringEnd(line, 0, st.raw);
    if (end < 0) return ' '.repeat(n);
    blank(0, end - 1 - Math.max(st.raw, 0)); st.str = false; i = end;
  }
  while (i < n) {
    const c = line[i];
    if (c === '"') {
      let j = i - 1; let hashes = 0;
      while (j >= 0 && line[j] === '#') { hashes++; j--; }
      const raw = j >= 0 && line[j] === 'r' && (j === 0 || !/\w/.test(line[j - 1]) || line[j - 1] === 'b') ? hashes : -1;
      const end = stringEnd(line, i + 1, raw);
      if (end < 0) { blank(i + 1, n); st.str = true; st.raw = raw; i = n; break; }
      blank(i + 1, end - 1 - Math.max(raw, 0)); i = end; continue;
    }
    if (c === "'") {
      if (line[i + 1] === '\\') {
        const close = line.indexOf("'", i + 3);
        if (close > 0) { blank(i + 1, close); i = close + 1; continue; }
      } else if (line[i + 2] === "'") { blank(i + 1, i + 2); i += 3; continue; }
      else if (line[i + 3] === "'" && /[\ud800-\udbff]/.test(line[i + 1] ?? '')) { blank(i + 1, i + 3); i += 4; continue; }
      i++; continue;
    }
    if (c === '/' && line[i + 1] === '/') { blank(i, n); i = n; break; }
    if (c === '/' && line[i + 1] === '*') {
      const end = line.indexOf('*/', i + 2);
      if (end < 0) { blank(i, n); st.comment = true; i = n; break; }
      blank(i, end + 2); i = end + 2; continue;
    }
    i++;
  }
  if (copyFrom === 0) return line;
  out.push(line.slice(copyFrom));
  return out.join('');
}

const OPEN = { ')': '(', ']': '[', '}': '{' };
function matchOpen(m, close) {
  let depth = 0;
  for (let k = close; k >= 0; k--) {
    const c = m[k];
    if (c === ')' || c === ']' || c === '}') depth++;
    else if (c === '(' || c === '[' || c === '{') { depth--; if (depth === 0) return k; }
  }
  return -1;
}
function matchClose(m, open) {
  let depth = 0;
  for (let k = open; k < m.length; k++) {
    const c = m[k];
    if (c === '(' || c === '[' || c === '{') depth++;
    else if (c === ')' || c === ']' || c === '}') { depth--; if (depth === 0) return k; }
  }
  return -1;
}
// Start index of the postfix receiver that ends just before `dot`.
function receiverStart(m, dot) {
  let i = dot - 1;
  while (i >= 0) {
    const c = m[i];
    if (c in OPEN) { i = matchOpen(m, i); if (i < 0) return -1; i--; continue; }
    if (c === '.' || c === ':' || /\w/.test(c)) { i--; continue; }
    if (c === '"') { const j = m.lastIndexOf('"', i - 1); if (j < 0) return -1; i = j - 1; continue; }
    break;
  }
  return i + 1;
}
// Depth-0 comma-separated argument spans of the call whose `(` is at `open`.
function argSpans(m, open) {
  const close = matchClose(m, open);
  if (close < 0) return [];
  const spans = []; let depth = 0; let from = open + 1;
  for (let k = open + 1; k < close; k++) {
    const c = m[k];
    if (c === '(' || c === '[' || c === '{') depth++;
    else if (c === ')' || c === ']' || c === '}') depth--;
    else if (c === ',' && depth === 0) { spans.push([from, k]); from = k + 1; }
  }
  if (m.slice(from, close).trim()) spans.push([from, close]);
  return spans;
}

// ---------------------------------------------------------------- expression typing

const TOKEN = /\s*(?:([A-Za-z_]\w*)|(\d\w*)|("[^"]*")|('\w+)|(::|=>|->|\.\.=?|&&|\|\||==|!=|<=|>=|[()[\]{}.,;*&!<>|?:=+\-/%^#@~]))/y;
function tokenize(text) {
  const toks = [];
  TOKEN.lastIndex = 0;
  let m;
  while (TOKEN.lastIndex < text.length && (m = TOKEN.exec(text))) {
    if (m[1]) toks.push({ k: 'id', v: m[1] });
    else if (m[2]) toks.push({ k: 'num', v: m[2] });
    else if (m[3]) toks.push({ k: 'str', v: m[3] });
    else if (m[4]) toks.push({ k: 'label', v: m[4] });
    else if (m[5]) toks.push({ k: 'p', v: m[5] });
    else break;
  }
  return toks;
}

class Typer {
  constructor(model, fn, locals, env) {
    this.model = model; this.fn = fn; this.locals = locals; this.env = env;
  }
  run(text, env = this.env) {
    const saved = [this.toks, this.i, this.env];
    this.toks = tokenize(text); this.i = 0; this.env = env;
    let t;
    try { t = this.expr(); } catch { t = UNKNOWN; }
    [this.toks, this.i, this.env] = saved;
    return t;
  }
  peek(o = 0) { return this.toks[this.i + o]; }
  is(v, o = 0) { const t = this.toks[this.i + o]; return t !== undefined && t.v === v; }
  next() { return this.toks[this.i++]; }
  expect(v) { const t = this.next(); if (!t || t.v !== v) throw new Error(`expected ${v}`); }
  skipGroup() {
    const open = this.next().v; const close = { '(': ')', '[': ']', '{': '}' }[open];
    let depth = 1;
    while (this.i < this.toks.length && depth > 0) {
      const v = this.next().v;
      if (v === open) depth++; else if (v === close) depth--;
    }
  }
  skipTo(stops) {
    let depth = 0;
    while (this.i < this.toks.length) {
      const v = this.peek().v;
      if (depth === 0 && stops.has(v)) return;
      if (v === '(' || v === '[' || v === '{') depth++;
      else if (v === ')' || v === ']' || v === '}') { if (depth === 0) return; depth--; }
      this.i++;
    }
  }
  expr() {
    const t = this.unary();
    const op = this.peek();
    if (op && op.k === 'p' && ['==', '!=', '<', '>', '<=', '>=', '&&', '||', '+', '-', '/', '%', '..', '..='].includes(op.v)) {
      this.next(); this.expr();
      return ['==', '!=', '<', '>', '<=', '>=', '&&', '||'].includes(op.v) ? { n: 'bool', a: [] } : t;
    }
    return t;
  }
  unary() {
    if (this.is('*')) { this.next(); return unwrapRef(this.unary()); }
    if (this.is('&')) { this.next(); if (this.is('mut')) this.next(); return this.unary(); }
    if (this.is('!') || this.is('-')) { this.next(); return this.unary(); }
    return this.postfix(this.primary());
  }
  primary() {
    const tok = this.peek();
    if (!tok) throw new Error('eof');
    if (tok.v === '(') {
      this.next();
      if (this.is(')')) { this.next(); return { n: 'unit', a: [] }; }
      const first = this.expr();
      if (this.is(',')) {
        const items = [first];
        while (this.is(',')) { this.next(); if (this.is(')')) break; items.push(this.expr()); }
        this.expect(')');
        return { n: '()', a: items };
      }
      this.expect(')');
      return first;
    }
    if (tok.v === '{') { this.skipGroup(); return UNKNOWN; }
    if (tok.k === 'num') { this.next(); return { n: 'num', a: [] }; }
    if (tok.k === 'str') { this.next(); return { n: 'str', a: [] }; }
    if (tok.v === 'match') { this.next(); return this.matchExpr(); }
    if (tok.v === 'if' || tok.v === 'loop' || tok.v === 'unsafe') throw new Error('statement');
    if (tok.k !== 'id') throw new Error(`primary ${tok.v}`);
    const segs = [this.next().v];
    while (this.is('::')) {
      this.next();
      if (this.is('<')) { this.skipAngles(); continue; }
      segs.push(this.next().v);
    }
    const name = segs[segs.length - 1];
    if (this.is('!')) {
      this.next();
      const g = this.peek();
      if (g && (g.v === '(' || g.v === '[' || g.v === '{')) this.skipGroup();
      if (name === 'unreachable' || name === 'panic' || name === 'jet_panic') return NEVER;
      if (name === 'matches') return { n: 'bool', a: [] };
      return UNKNOWN;
    }
    if (this.is('(')) return this.call(segs);
    if (this.is('{') && /^[A-Z_]/.test(name) && segs.length >= 1 && name !== name.toLowerCase()) {
      this.skipGroup();
      return this.model.types.has(name) ? { n: name, a: [] } : (segs.length > 1 ? { n: segs[segs.length - 2], a: [] } : UNKNOWN);
    }
    if (segs.length === 1) return this.lookup(name);
    if (segs.length > 1 && this.model.types.has(segs[segs.length - 2])) return { n: segs[segs.length - 2], a: [] };
    return UNKNOWN;
  }
  skipAngles() {
    let depth = 0;
    do {
      const v = this.next().v;
      if (v === '<') depth++; else if (v === '>') depth--;
    } while (depth > 0 && this.i < this.toks.length);
  }
  lookup(name) {
    if (this.env && this.env.has(name)) return this.env.get(name);
    if (this.locals.has(name)) return this.locals.get(name);
    if (this.fn && this.fn.params.has(name)) return this.fn.params.get(name);
    if (name === 'self' && this.fn && this.fn.implType) return parseType(this.fn.implType);
    if (name === 'None') return { n: 'Option', a: [UNKNOWN] };
    return UNKNOWN;
  }
  args() {
    this.expect('(');
    const out = [];
    while (!this.is(')')) {
      const start = this.i;
      let t;
      try { t = this.expr(); } catch { this.i = start; this.skipTo(new Set([',', ')'])); t = UNKNOWN; }
      if (this.i < this.toks.length && !this.is(',') && !this.is(')')) { this.skipTo(new Set([',', ')'])); t = UNKNOWN; }
      out.push(t);
      if (this.is(',')) this.next();
      else if (!this.is(')')) throw new Error('unbalanced arguments');
    }
    this.expect(')');
    return out;
  }
  call(segs) {
    const name = segs[segs.length - 1];
    const args = this.args();
    const a0 = args[0] ?? UNKNOWN;
    switch (name) {
      case 'Some': return { n: 'Option', a: [a0] };
      case 'Ok': return { n: 'Result', a: [a0, UNKNOWN] };
      case 'Box': return { n: 'Box', a: [a0] };
      case 'new': if (segs[segs.length - 2] === 'Box') return { n: 'Box', a: [a0] }; break;
      case 'jet_index_vec_ref': case 'jet_index_vec_mut': case 'jet_index_vec': case 'jet_list_get': return elemOf(a0);
      case 'jet_map_get_opt': { const m = unwrapRef(a0); return { n: 'JetOutcome', a: [m && m.a[1] ? m.a[1] : UNKNOWN, { n: 'JetAbsent', a: [] }] }; }
      case 'jet_slice_vec_range': case 'jet_list_slice': case 'jet_view_copy': { const e = elemOf(a0); return e ? { n: 'Vec', a: [e] } : UNKNOWN; }
      default: break;
    }
    const callee = this.model.fns.get(name);
    if (callee) return callee.ret;
    if (segs.length > 1 && this.model.types.has(segs[segs.length - 2])) return { n: segs[segs.length - 2], a: [] };
    return UNKNOWN;
  }
  postfix(t) {
    for (;;) {
      if (this.is('.')) {
        this.next();
        const tok = this.next();
        if (!tok) throw new Error('eof');
        if (tok.k === 'num') { const u = unwrapRef(t); t = u && u.n === '()' ? u.a[+tok.v] ?? UNKNOWN : UNKNOWN; continue; }
        if (this.is('::')) { this.next(); this.skipAngles(); }
        if (this.is('(')) { const args = this.args(); t = this.method(t, tok.v, args); continue; }
        t = this.model.field(t, tok.v);
        continue;
      }
      if (this.is('[')) {
        const save = this.i; this.skipGroup();
        const inner = this.toks.slice(save + 1, this.i - 1);
        t = inner.some((x) => x.v === '..' || x.v === '..=') ? t : elemOf(t);
        continue;
      }
      if (this.is('?')) { this.next(); t = payloadOf(t); continue; }
      return t;
    }
  }
  method(t, name, args) {
    switch (name) {
      case 'as_ref': case 'as_mut': case 'as_deref': case 'as_deref_mut': case 'borrow': case 'borrow_mut':
      case 'take': case 'clone': case 'to_owned': case 'by_ref': return t;
      case 'expect': case 'unwrap': case 'unwrap_or_else': case 'unwrap_or': case 'unwrap_or_default': case 'unwrap_unchecked': return payloadOf(t);
      case 'unwrap_err': case 'expect_err': return errOf(t);
      case 'to_vec': { const e = elemOf(t); return e ? { n: 'Vec', a: [e] } : UNKNOWN; }
      case 'get': case 'first': case 'last': case 'get_mut': case 'pop': return { n: 'Option', a: [elemOf(t)] };
      case 'len': case 'is_empty': case 'is_some': case 'is_none': case 'is_ok': case 'is_err': return { n: 'num', a: [] };
      default: return UNKNOWN;
    }
  }
  matchExpr() {
    const start = this.i;
    this.skipTo(new Set(['{']));
    const scrutToks = this.toks.slice(start, this.i);
    const scrut = this.subType(scrutToks);
    this.expect('{');
    let result = UNKNOWN;
    while (!this.is('}') && this.i < this.toks.length) {
      const ps = this.i;
      this.skipTo(new Set(['=>']));
      const pat = this.toks.slice(ps, this.i);
      this.expect('=>');
      const env = new Map(this.env ?? []);
      this.model.bindPattern(pat, scrut, env);
      const saved = this.env; this.env = env;
      let body;
      const bs = this.i;
      try { body = this.expr(); } catch { this.i = bs; body = UNKNOWN; }
      this.env = saved;
      this.skipTo(new Set([',', '}']));
      if (this.is(',')) this.next();
      else if (!this.is('}')) throw new Error('unbalanced arms');
      if (result === UNKNOWN && body !== UNKNOWN && body !== NEVER) result = body;
    }
    this.expect('}');
    return result;
  }
  subType(toks) {
    const saved = [this.toks, this.i];
    this.toks = toks; this.i = 0;
    let t;
    try { t = this.expr(); } catch { t = UNKNOWN; }
    [this.toks, this.i] = saved;
    return t;
  }
}

// ---------------------------------------------------------------- model

class Model {
  constructor(seeds) {
    this.types = new Map();      // ident -> {kind, fields: Map, variants: Map(variant -> Map|array)}
    this.variantIndex = new Map(); // variant ident -> [enum ident]
    this.fns = new Map();        // rust name -> fn record
    this.seeds = seeds;
    this.tier = new Map();       // ident -> tier
    this.witness = new Map();    // ident -> seed short name that makes it large
  }
  field(t, name) {
    t = unwrapRef(t);
    if (!t) return UNKNOWN;
    const def = this.types.get(t.n);
    if (!def || !def.fields) return UNKNOWN;
    const ft = def.fields.get(name);
    return ft === undefined ? UNKNOWN : parseType(ft);
  }
  variantField(enumIdent, variant, field) {
    const candidates = enumIdent && this.types.get(enumIdent)?.variants?.has(variant) ? [enumIdent] : (this.variantIndex.get(variant) ?? []);
    for (const e of candidates) {
      const v = this.types.get(e).variants.get(variant);
      if (v instanceof Map && v.has(field)) return parseType(v.get(field));
      if (Array.isArray(v) && typeof field === 'number' && v[field] !== undefined) return parseType(v[field]);
    }
    return UNKNOWN;
  }
  bindPattern(toks, scrut, env) {
    let i = 0;
    while (toks[i] && (toks[i].v === '&' || toks[i].v === 'ref' || toks[i].v === 'mut')) i++;
    const rest = toks.slice(i);
    if (rest.length === 0) return;
    if (rest.length === 1 && rest[0].k === 'id' && /^[a-z_]/.test(rest[0].v) && rest[0].v !== '_') { env.set(rest[0].v, scrut); return; }
    const segs = [];
    let k = 0;
    while (rest[k] && rest[k].k === 'id') { segs.push(rest[k].v); k++; if (rest[k] && rest[k].v === '::') k++; else break; }
    const head = segs[segs.length - 1];
    const enumIdent = segs.length > 1 ? segs[segs.length - 2] : unwrapRef(scrut)?.n;
    const group = rest[k];
    if (!group) return;
    const inner = splitTop(rest.slice(k + 1, rest.length - 1));
    if (group.v === '(') {
      if (head === 'Ok' || head === 'Some') { if (inner[0]) this.bindPattern(inner[0], payloadOf(scrut), env); return; }
      if (head === 'Err') { if (inner[0]) this.bindPattern(inner[0], errOf(scrut), env); return; }
      inner.forEach((p, idx) => this.bindPattern(p, this.variantField(enumIdent === 'Self' ? unwrapRef(scrut)?.n : enumIdent, head, idx), env));
      return;
    }
    if (group.v === '{') {
      for (const part of inner) {
        if (part.length === 0 || part[0].v === '..') continue;
        const fieldName = part[0].v;
        const fieldType = this.types.has(head) ? this.field({ n: head, a: [] }, fieldName) : this.variantField(enumIdent === 'Self' ? unwrapRef(scrut)?.n : enumIdent, head, fieldName);
        if (part.length === 1) env.set(fieldName, fieldType);
        else if (part[1].v === ':') this.bindPattern(part.slice(2), fieldType, env);
      }
    }
  }
  computeTiers() {
    const bySeed = new Map();
    for (const ident of this.types.keys()) {
      const short = shortName(ident);
      if (this.seeds.has(short)) { this.tier.set(ident, this.seeds.get(short)); this.witness.set(ident, short); bySeed.set(ident, true); }
    }
    for (let changed = true; changed;) {
      changed = false;
      for (const [ident, def] of this.types) {
        if (bySeed.has(ident)) continue;
        let best = this.tier.get(ident) ?? 0; let why = this.witness.get(ident);
        const consider = (text) => {
          const r = this.typeTierWhy(parseType(text), false);
          if (r.tier > best) { best = r.tier; why = r.why; }
        };
        if (def.fields) for (const f of def.fields.values()) consider(f);
        if (def.variants) for (const v of def.variants.values()) for (const f of (v instanceof Map ? v.values() : v)) consider(f);
        if (best > (this.tier.get(ident) ?? 0)) { this.tier.set(ident, best); this.witness.set(ident, why); changed = true; }
      }
    }
  }
  isSeed(t) { t = unwrapRef(t); return !!t && this.seeds.has(shortName(t.n)) && this.types.has(t.n); }
  // A sequence of item-sized seeds copied directly is a module's worth (one
  // class up); stored inside another type it is only lifted for module rows.
  typeTierWhy(t, top = true) {
    if (!t || t === NEVER) return { tier: 0 };
    if (SHALLOW.has(t.n)) return { tier: 0 };
    if (SEQUENCE.has(t.n) || (t.n === 'Box' && t.a[0]?.n === '[]')) {
      const elem = t.n === 'Box' ? t.a[0].a[0] : t.a[0];
      const r = this.typeTierWhy(elem, false);
      const lift = this.isSeed(elem) && (top || MODULE_SIZED.has(shortName(unwrapRef(elem).n)));
      if (r.tier > 0 && r.tier < 3 && lift) return { tier: r.tier + 1, why: `[${r.why}]` };
      return r;
    }
    if (WRAPPER.has(t.n)) {
      let best = { tier: 0 };
      for (const a of t.a) { const r = this.typeTierWhy(a, top); if (r.tier > best.tier) best = r; }
      return best;
    }
    const tier = this.tier.get(t.n) ?? 0;
    return tier ? { tier, why: this.witness.get(t.n) } : { tier: 0 };
  }
  // Does a value of type `t` own a value of type `ident` (by value, transitively)?
  owns(t, ident, seen = new Set()) {
    if (!t || t === NEVER || SHALLOW.has(t.n)) return false;
    if (t.n === ident) return true;
    if (t.a.length) return t.a.some((a) => this.owns(a, ident, seen));
    if (seen.has(t.n)) return false;
    seen.add(t.n);
    const def = this.types.get(t.n);
    if (!def) return false;
    const texts = def.fields ? [...def.fields.values()] : [...def.variants.values()].flatMap((v) => (v instanceof Map ? [...v.values()] : v));
    return texts.some((x) => this.owns(parseType(x), ident, seen));
  }
}

function splitTop(toks) {
  const parts = []; let cur = []; let depth = 0;
  for (const t of toks) {
    if (t.v === '(' || t.v === '[' || t.v === '{') depth++;
    else if (t.v === ')' || t.v === ']' || t.v === '}') depth--;
    if (t.v === ',' && depth === 0) { parts.push(cur); cur = []; continue; }
    cur.push(t);
  }
  if (cur.length) parts.push(cur);
  return parts;
}

// ---------------------------------------------------------------- scanning

const FN_HEAD = /^(\s*)(?:pub(?:\([\w ]+\))? )?(?:(?:const|unsafe|async|extern "C") )*fn (\w+)\s*(<[^(]*>)?\s*\(/;
const IMPL_HEAD = /^(\s*)impl(?:<[^>]*>)?\s+(?:(?:[\w:]+(?:<[^{]*>)?)\s+for\s+)?([\w:]+)(?:<[^{]*>)?\s*(?:where[^{]*)?\{\s*$/;
const TYPE_HEAD = /^(\s*)(?:pub(?:\([\w ]+\))? )?(struct|enum) (\w+)(?:<[^{(]*>)?\s*(\{|\()?/;
const STACK_ENTER = /jet_stack_enter\("src\/compiler\.jet", (\d+)u32, "([^"]*)"/;
const LINE_MARK = /"src\/compiler\.jet", (\d+)u32/g;
const SITE = /\.(clone|to_vec|to_owned)\(\)|\bClone::clone\(|\b(jet_map_get_opt|jet_slice_vec_range|jet_list_slice|jet_view_copy)\(/g;

function parseParams(text) {
  const params = new Map();
  let depth = 0; let from = 0; const parts = [];
  for (let k = 0; k < text.length; k++) {
    const c = text[k];
    if (c === '<' || c === '(' || c === '[') depth++;
    else if (c === '>' || c === ')' || c === ']') depth--;
    else if (c === ',' && depth === 0) { parts.push(text.slice(from, k)); from = k + 1; }
  }
  parts.push(text.slice(from));
  for (const p of parts) {
    const m = /^\s*(?:mut\s+)?(\w+)\s*:\s*([\s\S]+?)\s*$/.exec(p);
    if (m) params.set(m[1], parseType(m[2]));
  }
  return params;
}

function scanTypes(lines, model) {
  for (let i = 0; i < lines.length; i++) {
    const m = TYPE_HEAD.exec(lines[i]);
    if (!m) continue;
    const [, indent, kind, ident, open] = m;
    if (open === '(') {
      const body = lines[i].slice(lines[i].indexOf('(') + 1, lines[i].lastIndexOf(')'));
      model.types.set(ident, { kind, fields: new Map(body.split(',').map((f, k) => [String(k), f.replace(/^\s*pub(\([\w ]+\))?\s+/, '').trim()]).filter(([, f]) => f)) });
      continue;
    }
    if (open !== '{') continue;
    const def = kind === 'struct' ? { kind, fields: new Map() } : { kind, variants: new Map() };
    let variant = null;
    let j = i + 1;
    for (; j < lines.length; j++) {
      const l = lines[j];
      if (l.startsWith(indent + '}') && l.length <= indent.length + 2) break;
      const t = l.trim();
      if (!t || t.startsWith('//') || t.startsWith('#[')) continue;
      if (kind === 'struct') {
        const f = /^(?:pub(?:\([\w ]+\))?\s+)?(\w+)\s*:\s*(.+?),?$/.exec(t);
        if (f) def.fields.set(f[1], f[2]);
        continue;
      }
      if (variant) {
        if (t === '},' || t === '}') { variant = null; continue; }
        const f = /^(\w+)\s*:\s*(.+?),?$/.exec(t);
        if (f) def.variants.get(variant).set(f[1], f[2]);
        continue;
      }
      const v = /^(\w+)\s*(\{|\((.*)\))?\s*(?:=\s*[^,]+)?,?\s*$/.exec(t);
      if (!v) continue;
      if (v[2] === '{') {
        variant = v[1]; def.variants.set(variant, new Map());
        const oneLine = /^\w+\s*\{(.*)\},?$/.exec(t);
        if (oneLine) {
          for (const part of oneLine[1].split(',')) { const f = /^\s*(\w+)\s*:\s*(.+?)\s*$/.exec(part); if (f) def.variants.get(variant).set(f[1], f[2]); }
          variant = null;
        }
      } else if (v[3] !== undefined) {
        const parts = []; let depth = 0; let from = 0; const s = v[3];
        for (let k = 0; k < s.length; k++) {
          const c = s[k];
          if (c === '<' || c === '(' || c === '[') depth++; else if (c === '>' || c === ')' || c === ']') depth--;
          else if (c === ',' && depth === 0) { parts.push(s.slice(from, k)); from = k + 1; }
        }
        if (s.slice(from).trim()) parts.push(s.slice(from));
        def.variants.set(v[1], parts.map((p) => p.trim()));
      } else def.variants.set(v[1], new Map());
    }
    model.types.set(ident, def);
    if (def.variants) for (const vname of def.variants.keys()) {
      if (!model.variantIndex.has(vname)) model.variantIndex.set(vname, []);
      model.variantIndex.get(vname).push(ident);
    }
    i = j;
  }
}

// Function headers: name, params, return type, line range (by brace balance).
function scanFunctions(lines, masked, model) {
  const fns = [];
  const st = { str: false, raw: -1, comment: false };
  let implStack = [];
  let depth = 0;
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const m = FN_HEAD.exec(raw);
    if (m && !st.str && !st.comment) {
      // collect header text until the body's opening brace
      let head = ''; let k = i; let body = -1;
      for (; k < lines.length && k < i + 40; k++) {
        const ml = maskLine(lines[k], st);
        masked[k] = ml;
        head += (k === i ? ml : '\n' + ml);
        const at = findBodyOpen(head);
        if (at >= 0) { body = at; break; }
        if (/;\s*$/.test(ml) && !/\(/.test(ml.slice(-1))) break;
      }
      if (body < 0) { i = k; continue; }
      const name = m[2];
      const paren = head.indexOf('(', head.indexOf('fn ' + name) + 3 + name.length);
      const close = matchClose(head, paren);
      const params = parseParams(head.slice(paren + 1, close));
      const retText = /^\s*->\s*([\s\S]*?)\s*(?:where\b[\s\S]*)?$/.exec(head.slice(close + 1, body));
      // body end by brace balance starting at the header's `{`
      let bal = 0; let end = -1;
      const headLines = head.split('\n');
      let offset = 0;
      for (let h = 0; h < headLines.length; h++) {
        const segStart = h === headLines.length - 1 ? body - offset : 0;
        const seg = h === headLines.length - 1 ? headLines[h].slice(segStart) : '';
        bal += braceDelta(seg);
        offset += headLines[h].length + 1;
      }
      let line = k;
      if (bal === 0) end = k;
      while (end < 0 && ++line < lines.length) {
        const ml = maskLine(lines[line], st);
        masked[line] = ml;
        bal += braceDelta(ml);
        if (bal <= 0) end = line;
      }
      if (end < 0) end = lines.length - 1;
      const implType = implStack.length && implStack[implStack.length - 1].depth === depth - 1 ? implStack[implStack.length - 1].type : null;
      const fn = { rust: name, start: i, open: k, end, params, ret: retText ? parseType(retText[1]) : { n: 'unit', a: [] }, implType, jet: null, genLine: 0 };
      fns.push(fn);
      if (!model.fns.has(name) || name.startsWith('__jet_')) model.fns.set(name, fn);
      i = end;
      continue;
    }
    const ml = maskLine(raw, st);
    masked[i] = ml;
    const im = IMPL_HEAD.exec(ml);
    if (im) implStack.push({ depth, type: im[2] });
    depth += braceDelta(ml);
    while (implStack.length && depth <= implStack[implStack.length - 1].depth) implStack.pop();
  }
  return fns;
}
function findBodyOpen(head) {
  // first `{` at paren/angle depth 0 after the parameter list
  const p = head.indexOf('(');
  const close = matchClose(head, p);
  if (close < 0) return -1;
  for (let k = close + 1; k < head.length; k++) {
    if (head[k] === '{') return k;
    if (head[k] === ';') return -1;
  }
  return -1;
}
function braceDelta(s) {
  let d = 0;
  for (let k = 0; k < s.length; k++) { const c = s.charCodeAt(k); if (c === 123) d++; else if (c === 125) d--; }
  return d;
}

const LOOP_HEADER = /(?:^|[\s:;{}])(?:loop|while\b[^{;]*|for\b[^{;]*\bin\b[^{;]*)\s*$/;
const LET = /\blet\s+(?:mut\s+)?(\w+)\s*:\s*([^=;]+?)\s*(?:=|;)/g;

// Walk one function body: loop depth, match bindings, locals, call edges and
// copy sites. Each site is handed to `onSite` with the locals declared so far.
// Match scrutinees and arm patterns are recorded as (line, column) spans and
// sliced from the masked text when a site needs them.
function scanBody(fn, lines, masked, fnNames, onSite) {
  const locals = new Map();
  for (const [k, v] of fn.params) locals.set(k, v);
  const stack = []; // {loop, arms: {scrut, pat, from: [line, col]} | null}
  let pendingMatch = null; // {depth, from: [line, col]}
  let genLine = 0;
  const calls = [];
  const span = (from, li, col) => {
    if (from[0] === li) return masked[li].slice(from[1], col);
    const parts = [masked[from[0]].slice(from[1])];
    for (let k = from[0] + 1; k < li; k++) parts.push(masked[k]);
    parts.push(masked[li].slice(0, col));
    return parts.join(' ');
  };
  for (let li = fn.start; li <= fn.end; li++) {
    const raw = lines[li]; const m = masked[li];
    if (m === undefined) continue;
    const se = STACK_ENTER.exec(raw);
    if (se) { fn.jet = se[2]; fn.genLine = +se[1]; }
    const marks = [];
    for (const x of raw.matchAll(LINE_MARK)) marks.push([x.index, +x[1]]);
    let markIdx = 0;
    const events = new Map();
    for (const x of m.matchAll(SITE)) {
      const op = x[1] ?? (x[2] ? { jet_map_get_opt: 'map_get', jet_slice_vec_range: 'slice', jet_list_slice: 'slice', jet_view_copy: 'slice' }[x[2]] : 'Clone::clone');
      events.set(x.index, { kind: 'site', op, len: x[0].length });
    }
    for (const x of m.matchAll(/\b(__jet_\w+)\b/g)) if (fnNames.has(x[1]) && x[1] !== fn.rust) events.set(x.index, { kind: 'call', callee: x[1] });
    const lets = [...m.matchAll(LET)];
    let letIdx = 0;
    let headerFrom = 0;
    for (let c = 0; c < m.length; c++) {
      const ch = m[c];
      while (markIdx < marks.length && marks[markIdx][0] <= c) genLine = marks[markIdx++][1];
      // a `let` binds after its initializer; record it once the scan passes its start
      while (letIdx < lets.length && lets[letIdx].index + lets[letIdx][0].length <= c) { locals.set(lets[letIdx][1], parseType(lets[letIdx][2])); letIdx++; }
      const ev = events.get(c);
      if (ev) {
        let loopDepth = 0;
        for (const e of stack) if (e.loop) loopDepth++;
        if (ev.kind === 'call') calls.push({ callee: ev.callee, depth: loopDepth });
        else {
          const binds = [];
          for (const e of stack) if (e.arms && e.arms.pat !== null) binds.push({ scrut: e.arms.scrut, pat: e.arms.pat });
          onSite({ line: li, col: c, op: ev.op, len: ev.len, depth: loopDepth, genLine: genLine || fn.genLine, binds }, locals);
        }
      }
      if (ch === 'm' && m.startsWith('match', c) && !/\w/.test(m[c - 1] ?? ' ') && !/\w/.test(m[c + 5] ?? ' ')) {
        pendingMatch = { depth: stack.length, from: [li, c + 5] }; c += 4; continue;
      }
      const top = stack[stack.length - 1];
      if (top && top.arms && top.arms.pat === null && ch === '=' && m[c + 1] === '>') {
        top.arms.pat = span(top.arms.from, li, c); c++; continue;
      }
      if (ch === '(' || ch === '[' || ch === '{') {
        const entry = { loop: false, arms: null };
        if (ch === '{') {
          if (pendingMatch && stack.length === pendingMatch.depth) {
            entry.arms = { scrut: span(pendingMatch.from, li, c), pat: null, from: [li, c + 1] };
            pendingMatch = null;
          } else entry.loop = LOOP_HEADER.test(m.slice(headerFrom, c));
          headerFrom = c + 1;
        }
        stack.push(entry);
        continue;
      }
      if (ch === ')' || ch === ']' || ch === '}') {
        stack.pop();
        if (ch === '}') {
          headerFrom = c + 1;
          // a block-bodied arm may omit its trailing comma
          const parent = stack[stack.length - 1];
          if (parent && parent.arms && parent.arms.pat !== null) { parent.arms.pat = null; parent.arms.from = [li, c + 1]; }
        }
        continue;
      }
      if (ch === ';') headerFrom = c + 1;
      if (top && top.arms && ch === ',') { top.arms.pat = null; top.arms.from = [li, c + 1]; }
    }
    while (letIdx < lets.length) { locals.set(lets[letIdx][1], parseType(lets[letIdx][2])); letIdx++; }
  }
  return { calls };
}

function typeSite(site, fn, locals, masked, model) {
  const m = masked[site.line];
  const typer = new Typer(model, fn, locals, null);
  let env = null;
  for (const b of site.binds) {
    const next = new Map(env ?? []);
    const scrut = typer.run(b.scrut, env);
    model.bindPattern(tokenize(b.pat), scrut, next);
    env = next;
  }
  let recvText; let start; let end;
  if (site.op === 'map_get' || site.op === 'slice' || site.op === 'Clone::clone') {
    const open = site.col + site.len - 1;
    const spans = argSpans(m, open);
    if (!spans.length) return { type: UNKNOWN, recv: '' };
    recvText = m.slice(spans[0][0], spans[0][1]);
    start = site.col; end = matchClose(m, open) + 1;
    let t = typer.run(recvText, env);
    if (site.op === 'map_get') { const u = unwrapRef(t); t = u && u.n === 'JetMap' ? u.a[1] ?? UNKNOWN : UNKNOWN; }
    else if (site.op === 'slice') { const e = elemOf(t); t = e ? { n: 'Vec', a: [e] } : UNKNOWN; }
    return { type: t, recv: recvText.trim(), how: 'arg' };
  }
  start = receiverStart(m, site.col);
  end = site.col + site.len;
  recvText = start < 0 ? '' : m.slice(start, site.col);
  let t = recvText ? typer.run(recvText, env) : UNKNOWN;
  if (t && site.op === 'to_vec') { const e = elemOf(t); t = e ? { n: 'Vec', a: [e] } : UNKNOWN; }
  if (t !== UNKNOWN && t !== NEVER) return { type: t, recv: recvText.trim(), how: 'receiver' };
  // Destination: `x = Some(<site>)`, `let x: T = <site>`, `return <site>`, call argument, struct field.
  const dest = destinationType(m, start, end, fn, locals, typer, env, model);
  if (dest !== UNKNOWN) return { type: dest, recv: recvText.trim(), how: 'destination' };
  return { type: UNKNOWN, recv: recvText.trim(), how: 'unknown' };
}

// Which construct produced the copy. Emitter-shaped classes are fixed in the
// lowering/emission rule; the rest are copies the Jet source asks for.
//   pattern-test   `_vN = Some(<place>.clone())` then `matches!(_vN...)`: a kind test on a place
//   loop-source    the copy feeds `jet_loop_iter_init(_vN...)`: read-only iteration over a place
//   capture        `let __jet_capture_N = <place>.clone()`: a closure captures a borrowed value
//   element        `<list>[i].clone()` bound by value (`x := list[i]`)
//   field          `<place>.field.clone()` bound or passed by value
//   whole          `(*param).clone()`: a borrowed parameter copied whole
//   value          anything else
function shapeOf(site, masked, recv) {
  const m = masked[site.line];
  const before = m.slice(0, site.col);
  if (/let __jet_capture_\d+ = \(*\s*$/.test(before.slice(0, before.length - recv.length))) return 'capture';
  const dest = /^\s*(?:let\s+(?:mut\s+)?)?(_[vl]\d+)\b[^=]*=\s*Some\(/.exec(m);
  if (dest) {
    const name = dest[1];
    const usePattern = new RegExp(`\\b${name}\\.(?:as_ref|take)\\(\\)`);
    for (let k = site.line + 1; k < Math.min(masked.length, site.line + 4); k++) {
      const next = masked[k] ?? '';
      if (!usePattern.test(next)) continue;
      if (/\bmatches!\(/.test(next)) return 'pattern-test';
      if (/\bjet_loop_iter_init\(/.test(next)) return 'loop-source';
      break;
    }
  }
  let bare = recv.trim();
  for (let changed = true; changed;) {
    changed = false;
    if (bare.startsWith('*') || bare.startsWith('&')) { bare = bare.slice(1).trim(); changed = true; }
    if (bare.startsWith('(') && matchClose(bare, 0) === bare.length - 1) { bare = bare.slice(1, -1).trim(); changed = true; }
  }
  if (bare.startsWith('jet_index_vec_ref(') && matchClose(bare, bare.indexOf('(')) === bare.length - 1) return 'element';
  if (/^__jet_\w+$/.test(bare)) return 'whole';
  if (/\.__jet_\w+$/.test(bare)) return 'field';
  return 'value';
}

function stripWrap(text) {
  let s = text.trim();
  for (let changed = true; changed;) {
    changed = false;
    if (s.endsWith(';')) { s = s.slice(0, -1).trim(); changed = true; }
    if (s.startsWith('Some(') && matchClose(s, 4) === s.length - 1) { s = s.slice(5, -1).trim(); changed = true; continue; }
    if (s.startsWith('(') && matchClose(s, 0) === s.length - 1) { s = s.slice(1, -1).trim(); changed = true; }
  }
  return s;
}
function destinationType(m, start, end, fn, locals, typer, env, model) {
  if (start < 0) return UNKNOWN;
  const site = stripWrap(m.slice(start, end));
  const asg = /^\s*(?:let\s+(?:mut\s+)?(\w+)\s*(?::\s*([^=]+?))?|([^=]+?))\s*=\s*(?!=)/.exec(m);
  if (asg) {
    const rhs = stripWrap(m.slice(asg[0].length));
    if (rhs === site) {
      const someWrapped = /^\s*Some\(/.test(m.slice(asg[0].length).replace(/^\(+/, ''));
      let t = asg[2] ? parseType(asg[2]) : asg[1] ? locals.get(asg[1]) ?? UNKNOWN : typer.run(asg[3], env);
      if (someWrapped || (t && t.n === 'Option' && asg[3] && /^_[vl]\d+$/.test(asg[3].trim()))) t = payloadOf(t) ?? t;
      if (t !== UNKNOWN) return t;
    }
  }
  const ret = /^\s*return\s+/.exec(m);
  if (ret && stripWrap(m.slice(ret[0].length)) === site) return fn.ret;
  // enclosing call argument or struct-literal field
  let depth = 0;
  for (let k = start - 1; k >= 0; k--) {
    const c = m[k];
    if (c === ')' || c === ']' || c === '}') depth++;
    else if (c === '(' || c === '[' || c === '{') {
      if (depth > 0) { depth--; continue; }
      if (c === '(') {
        const name = /(\w+)\s*$/.exec(m.slice(0, k));
        const callee = name && model.fns.get(name[1]);
        if (!callee) return UNKNOWN;
        const idx = argSpans(m, k).findIndex(([a, b]) => a <= start && start <= b);
        const param = [...callee.params.values()][idx];
        return param ?? UNKNOWN;
      }
      if (c === '{') {
        const field = /(\w+)\s*:\s*\(*\s*$/.exec(m.slice(0, start));
        const head = /([\w:]+)\s*$/.exec(m.slice(0, k));
        if (!field || !head) return UNKNOWN;
        const segs = head[1].split('::');
        const last = segs[segs.length - 1];
        if (model.types.get(last)?.fields) return model.field({ n: last, a: [] }, field[1]);
        return model.variantField(segs[segs.length - 2], last, field[1]);
      }
      return UNKNOWN;
    }
  }
  if (fn.start === fn.end) return fn.ret;
  return UNKNOWN;
}

// ---------------------------------------------------------------- call graph

function sccs(nodes, succ) {
  const index = new Map(); const low = new Map(); const onStack = new Set();
  const stack = []; const out = []; let next = 0;
  for (const root of nodes) {
    if (index.has(root)) continue;
    const work = [[root, 0]];
    index.set(root, next); low.set(root, next); next++; stack.push(root); onStack.add(root);
    while (work.length) {
      const frame = work[work.length - 1];
      const [v] = frame;
      const ss = succ.get(v) ?? [];
      if (frame[1] < ss.length) {
        const w = ss[frame[1]++];
        if (!index.has(w)) {
          index.set(w, next); low.set(w, next); next++; stack.push(w); onStack.add(w);
          work.push([w, 0]);
        } else if (onStack.has(w)) low.set(v, Math.min(low.get(v), index.get(w)));
        continue;
      }
      work.pop();
      if (work.length) { const u = work[work.length - 1][0]; low.set(u, Math.min(low.get(u), low.get(v))); }
      if (low.get(v) === index.get(v)) {
        const comp = [];
        let w;
        do { w = stack.pop(); onStack.delete(w); comp.push(w); } while (w !== v);
        out.push(comp);
      }
    }
  }
  return out; // reverse topological (callees first)
}

function logAdd(a, b) {
  if (a === -Infinity) return b;
  if (b === -Infinity) return a;
  const hi = Math.max(a, b); const lo = Math.min(a, b);
  return hi + Math.log10(1 + 10 ** (lo - hi));
}

function frequency(fns, edgesByFn) {
  const names = fns.map((f) => f.rust);
  const succ = new Map();
  for (const f of fns) succ.set(f.rust, [...new Set(edgesByFn.get(f.rust).map((e) => e.callee))]);
  const comps = sccs(names, succ);
  const compOf = new Map();
  comps.forEach((c, k) => c.forEach((n) => compOf.set(n, k)));
  const recursive = comps.map((c) => c.length > 1 || (succ.get(c[0]) ?? []).includes(c[0]));
  const incoming = comps.map(() => []);
  const fanIn = new Map();
  for (const f of fns) for (const e of edgesByFn.get(f.rust)) {
    fanIn.set(e.callee, (fanIn.get(e.callee) ?? 0) + 1);
    if (compOf.get(f.rust) !== compOf.get(e.callee)) incoming[compOf.get(e.callee)].push({ from: compOf.get(f.rust), depth: e.depth });
  }
  const nest = new Array(comps.length).fill(0);
  const logCalls = new Array(comps.length).fill(0);
  for (let k = comps.length - 1; k >= 0; k--) {
    let n = 0; let lc = incoming[k].length ? -Infinity : 0;
    for (const e of incoming[k]) { n = Math.max(n, nest[e.from] + e.depth); lc = logAdd(lc, logCalls[e.from] + e.depth); }
    nest[k] = n + (recursive[k] ? 1 : 0);
    logCalls[k] = lc + (recursive[k] ? 1 : 0);
  }
  const out = new Map();
  for (const f of fns) {
    const k = compOf.get(f.rust);
    out.set(f.rust, { nest: nest[k], logCalls: logCalls[k], recursive: recursive[k], fanIn: fanIn.get(f.rust) ?? 0, roots: incoming[k].length === 0 });
  }
  return out;
}

// ---------------------------------------------------------------- source map

function loadSourceMap(opts, stageZero) {
  const segments = [];
  if (opts.map) {
    const json = JSON.parse(fs.readFileSync(opts.map, 'utf8'));
    for (const f of json.files ?? []) segments.push({ start: f.generated_start_line, file: f.path });
  } else {
    const unit = opts.unit ?? path.join(path.dirname(stageZero), 'compiler-project/src/compiler.jet');
    if (!fs.existsSync(unit)) return null;
    const text = fs.readFileSync(unit, 'utf8');
    let line = 1; let at = 0;
    for (;;) {
      const nl = text.indexOf('\n', at);
      if (text.startsWith('// [jet-bootstrap source: ', at)) {
        const close = text.indexOf(']', at);
        segments.push({ start: line + 1, file: text.slice(at + 26, close) });
      }
      if (nl < 0) break;
      at = nl + 1; line++;
    }
  }
  segments.sort((a, b) => a.start - b.start);
  return (genLine) => {
    if (!genLine) return '?';
    let lo = 0; let hi = segments.length - 1; let hit = -1;
    while (lo <= hi) { const mid = (lo + hi) >> 1; if (segments[mid].start <= genLine) { hit = mid; lo = mid + 1; } else hi = mid - 1; }
    return hit < 0 ? `compiler.jet:${genLine}` : `${segments[hit].file}:${genLine - segments[hit].start + 1}`;
  };
}

// ---------------------------------------------------------------- profile

// perf-script text: attributes each sample whose leaf-side frames copy or drop
// a large type to the nearest emitted Jet function on the stack.
function readProfile(file, model) {
  const text = fs.readFileSync(file, 'utf8');
  const rows = new Map();
  let samples = 0; let frames = [];
  const tierOfFrameType = (sym) => {
    const g = /<([^<>,]*(?:<[^<>]*(?:<[^<>]*>[^<>]*)*>)?[^<>,]*)[,>]/.exec(sym);
    if (!g) return null;
    const t = parseType(g[1]);
    let ident = t;
    while (ident && (SEQUENCE.has(ident.n) || WRAPPER.has(ident.n)) && ident.a.length) ident = ident.a[0];
    ident = ident?.n;
    const r = model.typeTierWhy(t);
    return r.tier ? { tier: r.tier, type: showType(t), ident } : null;
  };
  const flush = () => {
    if (!frames.length) return;
    samples++;
    let owner = -1;
    for (let k = 0; k < frames.length; k++) if (model.fns.has(frames[k]) && frames[k].startsWith('__jet_')) { owner = k; break; }
    if (owner > 0) {
      let found = null;
      for (let k = owner - 1; k >= 0; k--) {
        const sym = frames[k];
        const op = /^(clone|to_vec|clone_from|to_owned)</.test(sym) ? 'copy' : /^drop_in_place</.test(sym) ? 'drop' : null;
        if (!op) continue;
        const ty = tierOfFrameType(sym);
        if (ty) { found = { op, ...ty }; break; }
      }
      if (found) {
        const key = `${frames[owner]}\t${found.op}\t${found.type}`;
        const row = rows.get(key) ?? { fn: frames[owner], op: found.op, type: found.type, ident: found.ident, tier: found.tier, samples: 0 };
        row.samples++;
        rows.set(key, row);
      }
    }
    frames = [];
  };
  for (const line of text.split('\n')) {
    if (!line.trim()) { flush(); continue; }
    if (!line.startsWith('\t') && !line.startsWith(' ')) { flush(); continue; }
    const m = /^\s+[0-9a-f]+\s+(.+?)\+0x[0-9a-f]+\s+\(/.exec(line) ?? /^\s+[0-9a-f]+\s+(\S+)/.exec(line);
    if (m) frames.push(m[1].replace(/^jetc_\w+::/, ''));
  }
  flush();
  return { samples, rows: [...rows.values()].sort((a, b) => b.samples - a.samples) };
}

// ---------------------------------------------------------------- allow-list

function loadAllow(file) {
  const rules = [];
  if (!file || !fs.existsSync(file)) return rules;
  fs.readFileSync(file, 'utf8').split('\n').forEach((line, k) => {
    if (!line.trim() || line.startsWith('#')) return;
    const [fnName, type, reason] = line.split('\t');
    if (!reason || !reason.trim()) throw new Error(`${file}:${k + 1}: allow-list rows need jet_function<TAB>type<TAB>reason`);
    rules.push({ fn: fnName.trim(), type: type.trim(), reason: reason.trim() });
  });
  return rules;
}

// ---------------------------------------------------------------- main

function usage() {
  return `usage: clone-gate.mjs <stage-zero.rs> [--out DIR] [--unit compiler.jet | --map compiler.map.json]
       [--profile perf-script.txt] [--allow allow.tsv] [--check] [--check-tier N] [--seed Name=tier ...] [--top N] [--verbose]`;
}

export function analyze(file, opts = {}) {
  const seeds = new Map(DEFAULT_SEEDS);
  for (const s of opts.seeds ?? []) { const [n, t] = s.split('='); seeds.set(n, +t); }
  const lines = fs.readFileSync(file, 'utf8').split('\n');
  const model = new Model(seeds);
  const t0 = Date.now();
  const progress = (what) => { if (opts.verbose) process.stderr.write(`clone-gate: ${what} ${((Date.now() - t0) / 1000).toFixed(1)}s heap ${(process.memoryUsage().heapUsed / 1e9).toFixed(2)}GB\n`); };
  scanTypes(lines, model);
  model.computeTiers();
  progress(`types ${model.types.size}`);
  const masked = new Array(lines.length);
  const allFns = scanFunctions(lines, masked, model);
  progress(`functions ${allFns.length}`);
  const fns = allFns.filter((f) => f.rust.startsWith('__jet_') || lines.slice(f.open, Math.min(f.end, f.open + 400) + 1).some((l) => l.includes('jet_stack_enter("src/compiler.jet"')));
  const fnNames = new Set(fns.filter((f) => f.rust.startsWith('__jet_')).map((f) => f.rust));
  const edgesByFn = new Map();
  const large = [];
  let unknownSites = 0; let totalSites = 0;
  for (const fn of fns) {
    const body = scanBody(fn, lines, masked, fnNames, (s, locals) => {
      totalSites++;
      const typed = typeSite(s, fn, locals, masked, model);
      if (typed.type === UNKNOWN) { unknownSites++; return; }
      const r = model.typeTierWhy(typed.type);
      if (r.tier) large.push({ fn, s, typed, r, shape: shapeOf(s, masked, typed.recv) });
    });
    edgesByFn.set(fn.rust, body.calls);
  }
  progress(`bodies: ${totalSites} copy sites, ${large.length} large`);
  const freq = frequency(fns, edgesByFn);
  const where = loadSourceMap(opts, file) ?? ((g) => (g ? `compiler.jet:${g}` : '?'));
  const allow = loadAllow(opts.allow);
  const sites = [];
  for (const { fn, s, typed, r, shape } of large) {
    const f = freq.get(fn.rust);
    const jet = fn.jet ?? shortName(fn.rust);
    const pathClass = s.depth > 0 ? 'per-loop' : f.nest > 0 ? (f.recursive ? 'per-call' : 'per-item') : f.fanIn > 1 ? 'per-call' : 'once';
    const typeText = showType(typed.type);
    const rule = allow.find((a) => (a.fn === '*' || a.fn === jet) && (a.type === '*' || a.type === typeText));
    sites.push({
      tier: r.tier, why: r.why, type: typeText, op: s.op, path: pathClass,
      nest: f.nest, local: s.depth, logCalls: f.logCalls, fanIn: f.fanIn,
      jet, rust: fn.rust, source: where(s.genLine), fnSource: where(fn.genLine),
      rustLine: s.line + 1, recv: typed.recv, how: typed.how, shape, allow: rule?.reason ?? '', samples: 0, typeNode: typed.type,
    });
  }
  progress(`ranked ${sites.length}`);
  let profile = null;
  if (opts.profile) {
    profile = readProfile(opts.profile, model);
    // A sampled copy or drop of type T inside function F is charged to every
    // static site in F whose copied value owns a T (the drop frees the copy).
    for (const row of profile.rows) {
      row.jet = model.fns.get(row.fn)?.jet ?? shortName(row.fn);
      const matched = sites.filter((s) => s.rust === row.fn && model.owns(s.typeNode, row.ident));
      row.sites = matched.length;
      for (const s of matched) s.samples += row.samples;
    }
  }
  const repeated = (s) => (s.path === 'once' ? 0 : 1);
  sites.sort((a, b) => b.samples - a.samples || b.tier - a.tier || repeated(b) - repeated(a)
    || b.nest + b.local - (a.nest + a.local) || b.logCalls + b.local - (a.logCalls + a.local) || a.rustLine - b.rustLine);
  return { sites, totalSites, unknownSites, fnCount: fns.length, profile, largeTypes: [...model.tier.entries()].filter(([, t]) => t > 0).length };
}

function report(result, opts) {
  const { sites } = result;
  const header = ['rank', 'samples', 'tier', 'type', 'contains', 'op', 'shape', 'path', 'nesting', 'fn_nest', 'loop_depth', 'log10_calls', 'fan_in',
    'jet_function', 'source', 'function_source', 'rust_line', 'typed_by', 'receiver', 'allowed'];
  const tsv = [header.join('\t'), ...sites.map((s, k) => [k + 1, s.samples, TIER_NAME[s.tier], s.type, s.why ?? '', s.op, s.shape, s.path,
    s.nest + s.local, s.nest, s.local, s.logCalls.toFixed(1), s.fanIn, s.jet, s.source, s.fnSource, s.rustLine, s.how,
    s.recv.replace(/\s+/g, ' ').slice(0, 160), s.allow].join('\t'))].join('\n') + '\n';
  const top = opts.top ?? 40;
  const count = (pred) => sites.filter(pred).length;
  const lines = [];
  lines.push(`clone-gate: ${result.totalSites} copy sites in ${result.fnCount} emitted functions; ${result.largeTypes} large types; ${result.unknownSites} sites untyped.`);
  lines.push(`large copies: ${sites.length} (program ${count((s) => s.tier === 3)}, item ${count((s) => s.tier === 2)}, node ${count((s) => s.tier === 1)})`);
  lines.push(`paths: per-loop ${count((s) => s.path === 'per-loop')}, per-item ${count((s) => s.path === 'per-item')}, per-call ${count((s) => s.path === 'per-call')}, once ${count((s) => s.path === 'once')}`);
  lines.push(`receiver typing: ${count((s) => s.how === 'receiver')} by receiver, ${count((s) => s.how === 'destination')} by destination, ${count((s) => s.how === 'arg')} by helper argument`);
  const shapes = new Map();
  for (const s of sites) if (s.path !== 'once') { const k = `${s.shape}/${TIER_NAME[s.tier]}`; shapes.set(k, (shapes.get(k) ?? 0) + 1); }
  lines.push(`repeated-path shapes: ${[...shapes].sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k} ${v}`).join(', ')}`);
  if (result.profile) {
    const p = result.profile;
    lines.push('', `profile: ${p.samples} samples; large copy/drop attributed to the nearest emitted Jet function:`);
    for (const r of p.rows.slice(0, top)) lines.push(`${(100 * r.samples / p.samples).toFixed(1).padStart(6)}%  ${String(r.samples).padStart(6)}  ${r.op.padEnd(4)} ${r.type.padEnd(34)} ${r.jet}  (${r.sites} static site${r.sites === 1 ? '' : 's'})`);
  }
  const byFn = new Map();
  for (const s of sites) {
    const row = byFn.get(s.jet) ?? { jet: s.jet, source: s.fnSource, first: s, n: 0, types: new Set() };
    row.n++; row.types.add(s.type); byFn.set(s.jet, row);
  }
  lines.push('', `top functions (rank order: samples, size class, repeated path, nesting = fn_nest + loop_depth):`);
  for (const r of [...byFn.values()].slice(0, top)) lines.push(`${TIER_NAME[r.first.tier].padEnd(7)} ${String(r.first.nest + r.first.local).padStart(3)} ${String(r.n).padStart(4)}  ${r.jet}  ${r.source}  [${[...r.types].slice(0, 4).join(', ')}]`);
  lines.push('', `top ${top} sites:`);
  for (const s of sites.slice(0, top)) lines.push(`${String(s.samples).padStart(5)} ${TIER_NAME[s.tier].padEnd(7)} ${String(s.nest + s.local).padStart(3)} ${s.path.padEnd(8)} ${s.op.padEnd(6)} ${s.type.padEnd(32)} ${s.jet}  ${s.source}  (rs:${s.rustLine})`);
  return { tsv, summary: lines.join('\n') + '\n' };
}

async function main(argv) {
  const opts = { seeds: [] };
  let file = null;
  for (let k = 0; k < argv.length; k++) {
    const a = argv[k];
    if (a === '--out') opts.out = argv[++k];
    else if (a === '--unit') opts.unit = argv[++k];
    else if (a === '--map') opts.map = argv[++k];
    else if (a === '--profile') opts.profile = argv[++k];
    else if (a === '--allow') opts.allow = argv[++k];
    else if (a === '--check') opts.check = true;
    else if (a === '--verbose') opts.verbose = true;
    else if (a === '--check-tier') opts.checkTier = +argv[++k];
    else if (a === '--seed') opts.seeds.push(argv[++k]);
    else if (a === '--top') opts.top = +argv[++k];
    else if (a === '-h' || a === '--help') { console.log(usage()); return 0; }
    else if (!file) file = a;
    else { console.error(usage()); return 2; }
  }
  if (!file) { console.error(usage()); return 2; }
  opts.allow ??= path.join(HERE, 'allow.tsv');
  const result = analyze(file, opts);
  const { tsv, summary } = report(result, opts);
  if (opts.out) {
    fs.mkdirSync(opts.out, { recursive: true });
    fs.writeFileSync(path.join(opts.out, 'clone-gate.tsv'), tsv);
    fs.writeFileSync(path.join(opts.out, 'summary.txt'), summary);
  }
  process.stdout.write(summary);
  if (opts.check) {
    const tier = opts.checkTier ?? 2;
    const bad = result.sites.filter((s) => s.tier >= tier && s.path !== 'once' && !s.allow);
    if (bad.length) {
      console.error(`clone-gate --check: ${bad.length} ${TIER_NAME[tier]}-or-larger copies on repeated paths without an allow-list reason`);
      for (const s of bad.slice(0, 20)) console.error(`  ${s.path} ${s.type} ${s.jet} ${s.source}`);
      return 1;
    }
    console.log(`clone-gate --check: ok (no unallowed ${TIER_NAME[tier]}-or-larger copies on repeated paths)`);
  }
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await main(process.argv.slice(2));
}
