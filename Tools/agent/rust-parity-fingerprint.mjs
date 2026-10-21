#!/usr/bin/env node
// Behavioral parity verifier for the Rust exit.
//
// The name-based ledger (rust-parity-ledger.mjs, items.tsv) says which Jet item
// carries a Rust item's name. This tool checks behavior instead: for every
// in-scope Rust item it extracts a fingerprint (diagnostic codes, message
// strings, enum variants constructed, numeric literals, callee names, branch
// count; fields and variants for types), fingerprints every Jet item the same
// way, and scores the Rust fingerprint against each candidate Jet counterpart
// (the ledger's pointer, name matches, and a fingerprint search over the whole
// Jet tree, so renamed ports are found). A Jet candidate is scored together
// with the Jet items it calls in the same package, because ports often split a
// Rust function into helpers.
//
// Classes:
//   PORTED           the Jet counterpart carries the Rust behavior
//   PARTIAL          a Jet counterpart exists but misses listed behavior
//   PORTED-UNMERGED  ported only on a secondary Jet tree (origin/port/main,
//                    cutover-ready), not on the primary tree
//   ABSENT           no Jet counterpart
//   OBSOLETE         retired with Rust, with the reason (ledger drop reason,
//                    reviewed triage group, or Rust std-trait glue)
// Runtime route rows (`route` kind) are the live Core runtime routes whose C
// export no Jet runtime #Export(c) defines.
//
// Reads the Rust tree at the ledger's pinned commit and the Jet trees from git
// objects; writes Docs/audits/rust-exit-parity/{verified.tsv,VERIFIED.md}.
// Hand spot-check verdicts are read from samples/verified-spot*.tsv
// (rust_file, line, name, class, verdict, correct_class, note).
//
// Usage: node Tools/agent/rust-parity-fingerprint.mjs [--repo <git dir>]
//          [--jet-rev <commit>] [--jet-refs <ref,ref>] [--sample <n> <out.tsv>]
//   --repo     git checkout to read objects from (default: this script's repo)
//   --sample   also write a stratified hand-check sample: n rows per
//              area x class (at least 5% of every class), deterministic.

import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { extentEnd, lex } from './rust-parity-ledger.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const OUT_REL = 'Docs/audits/rust-exit-parity';

function fail(message) {
  process.stderr.write(`rust-parity-fingerprint: ${message}\n`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = { repo: ROOT, jetRev: 'HEAD', jetRefs: 'origin/port/main,cutover-ready', sample: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--repo') opts.repo = argv[++i];
    else if (a === '--jet-rev') opts.jetRev = argv[++i];
    else if (a === '--jet-refs') opts.jetRefs = argv[++i];
    else if (a === '--sample') opts.sample = { n: Number(argv[++i]), out: argv[++i] };
    else fail(`unknown argument: ${a}`);
  }
  return opts;
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

let REPO = ROOT;
const git = (args, input) =>
  execFileSync('git', args, { cwd: REPO, maxBuffer: Infinity, ...(input === undefined ? {} : { input }) });
const resolveCommit = (name) => git(['rev-parse', '--verify', `${name}^{commit}`]).toString('utf8').trim();

function lsTree(rev, paths) {
  return git(['ls-tree', '-r', '-z', rev, '--', ...paths])
    .toString('utf8')
    .split('\0')
    .filter(Boolean)
    .map((e) => {
      const tab = e.indexOf('\t');
      const [, type, sha] = e.slice(0, tab).split(' ');
      return { type, sha, path: e.slice(tab + 1) };
    })
    .filter((e) => e.type === 'blob');
}

// Reads blobs by sha in one cat-file batch: Map(sha -> text).
function readBlobs(shas) {
  const out = new Map();
  const uniq = [...new Set(shas)];
  for (let k = 0; k < uniq.length; k += 4000) {
    const part = uniq.slice(k, k + 4000);
    const buf = git(['cat-file', '--batch'], part.join('\n') + '\n');
    let pos = 0;
    for (const sha of part) {
      const nl = buf.indexOf(0x0a, pos);
      const m = / blob (\d+)$/.exec(buf.toString('utf8', pos, nl));
      if (!m) fail(`cat-file failed for ${sha}`);
      const size = Number(m[1]);
      out.set(sha, buf.toString('utf8', nl + 1, nl + 1 + size));
      pos = nl + 1 + size + 1;
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

// In-scope Rust paths -> area. Order matters (first prefix wins).
const AREAS = [
  ['crates/jet-lexer/', 'lexer'],
  ['crates/jet-parser/', 'parser'],
  ['crates/jet-sema/', 'sema'],
  ['crates/jet-comptime/', 'comptime'],
  ['crates/jet-evalhost/', 'evalhost'],
  ['crates/jet-codegen/src/Prelude/', 'core'],
  ['crates/jet-codegen/', 'codegen'],
  ['crates/jet-driver/', 'driver'],
  ['crates/jet-foundation/', 'foundation'],
  ['crates/jet-rt/', 'runtime'],
  ['crates/jet-cli/', 'cli'],
  ['Source/', 'cli'],
];
const areaOf = (p) => AREAS.find(([pre]) => p.startsWith(pre))?.[1] ?? null;
const isTestPath = (p) => /(^|\/)(tests?|Tests?|benches|examples)\//.test(p) || /(^|\/)tests?\.rs$/.test(p);

// Jet directories where an area's port is expected; a thin Rust item (too
// little behavior to fingerprint) only accepts a same-name Jet item here.
const AREA_DIRS = {
  lexer: ['Compiler/JetLexer/', 'Compiler/JetParser/'],
  parser: ['Compiler/JetParser/', 'Compiler/JetLexer/'],
  sema: ['Compiler/JetSema/', 'Compiler/JetFoundation/'],
  comptime: ['Compiler/'],
  evalhost: ['Compiler/'],
  codegen: ['Compiler/JetCodegen/', 'Compiler/JetBackend/', 'Compiler/JetOptimizer/', 'Compiler/JetFoundation/'],
  driver: ['Compiler/JetDriver/', 'Compiler/JetBuildSession/', 'Compiler/JetQueries/', 'Compiler/JetCli/'],
  foundation: ['Compiler/', 'Core/'],
  cli: ['Compiler/'],
  core: ['Core/', 'Compiler/JetRuntime/', 'Compiler/Prelude/'],
  runtime: ['Compiler/JetRuntime/', 'Core/'],
};
const JET_ROOTS = ['Compiler', 'Core', 'Jetpack'];
const isJetSource = (p) => p.endsWith('.jet') && !/(^|\/)(Tests?|tests?|Fixtures?|fixtures)\//.test(p);

// ---------------------------------------------------------------------------
// Fingerprints
// ---------------------------------------------------------------------------

const W = { code: 5, msg: 3, word: 0.5, variant: 1.5, number: 1, member: 1, callee: 0.5 };
const CODE_RE = /\b[EWL]\d{4}\b/g;
const KEYWORDS = new Set(
  'if else while for loop match return fn let mut in as move ref where impl dyn box unsafe async await self super crate use mod pub struct enum trait type const static break continue yield do sizeof is not and or with defer when then try catch'.split(
    ' ',
  ),
);
const IGNORED_VARIANTS = new Set(['some', 'none', 'ok', 'err', 'self']);
// Callees too generic to count (std collection/string/option plumbing).
const CALL_STOP = new Set(
  'new default clone len push pop get insert remove contains iter iter_mut into_iter map map_err and_then ok_or ok_or_else unwrap unwrap_or unwrap_or_else unwrap_or_default expect is_some is_none is_ok is_err to_string to_owned as_str as_ref as_mut into from collect extend join split trim starts_with ends_with format write writeln println eprintln is_empty first last chars bytes find filter filter_map any all count sum max min sort sort_by sort_by_key dedup retain entry or_insert or_insert_with or_default cloned copied rev enumerate zip take skip peekable next borrow borrow_mut lock read write take replace drain truncate with_capacity reserve append to_vec to_lowercase to_uppercase parse eq ne cmp partial_cmp hash fmt display debug assert assert_eq assert_ne some ok err box rc arc vec string as_bytes as_slice get_mut keys values values_mut range push_str concat slice size of'.split(
    ' ',
  ),
);

const key = (s) => s.toLowerCase().replace(/_/g, '');
const snakeWords = (s) =>
  s
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1_$2')
    .toLowerCase()
    .split(/_+/)
    .filter(Boolean);
// Spelling words that carry no meaning across the port (`jet_rt_`, `jet_std_` prefixes).
const NAME_STOP = new Set(['jet', 'rt', 'std', 'core', 'native', 'raw', 'lib', 'jetlib']);
const nameWords = (s) => snakeWords(s).filter((w) => !NAME_STOP.has(w));
// Shared share of name words, `check_value_tail` vs `sema_check_value_tail`,
// `jet_scheduler_current_task_control` vs `scheduler_control_current`.
function nameAffinity(rustName, rustOwner, jetName) {
  const a = new Set(nameWords(rustName));
  const b = new Set(nameWords(jetName));
  let hit = 0;
  for (const w of a) if (b.has(w)) hit++;
  if (rustOwner && rustOwner !== '-') for (const w of nameWords(rustOwner)) if (b.has(w) && !a.has(w)) hit += 0.5;
  return hit / Math.max(1, a.size, Math.min(b.size, a.size + 2));
}

// A Jet item's owner or file carries a word of the Rust owner (or, for a free
// function, of the Rust file name): `JetArena::reset` in a file about arenas.
function ownerAffinity(r, it) {
  const words = nameWords(r.scope).filter((w) => w.length >= 3 && w !== 'mod');
  const where = (key(it.owner ?? '') + it.file.toLowerCase()).replace(/[^a-z0-9]/g, '');
  return words.some((w) => where.includes(w));
}

// Single-word Rust names (`reset`, `fmt`, `ready`) match too many Jet items by name alone.
const isGenericName = (name) => nameWords(name).length <= 1;

function normMsg(s, rust) {
  let t = s;
  if (rust) t = t.replace(/\{\{|\}\}/g, ' ').replace(/\{[^{}]*\}/g, ' ');
  t = t.replace(/\\[nrt0"'\\]/g, ' ').toLowerCase();
  const words = t.match(/[a-z0-9]+/g) ?? [];
  return words;
}

function normNumber(tok) {
  const t = tok.replace(/_/g, '').toLowerCase();
  try {
    if (/^0[xbo]/.test(t)) return BigInt(t).toString();
  } catch {
    return null;
  }
  if (/^\d+$/.test(t)) return BigInt(t).toString();
  const f = Number(t);
  return Number.isFinite(f) ? String(f) : null;
}
const NUM_RE =
  /(?<![A-Za-z0-9_.])(0x[0-9a-fA-F_]+|0b[01_]+|0o[0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)(?:_?(?:[iu](?:8|16|32|64|128|size)|f32|f64))?(?![A-Za-z0-9_])/g;
const CALL_RE_RUST = /\b([A-Za-z_][A-Za-z0-9_]*)\s*(?:::\s*<[^()]*?>\s*)?\(/g;
const CALL_RE_JET = /\b([A-Za-z_][A-Za-z0-9_]*)\s*(?:\[[^\[\]()]*\]\s*)?\(/g;

// Fingerprint of code lines [a, b] (0-based, inclusive) of one lexed file.
// `type` items fingerprint their direct members instead of a body.
function fingerprint(lx, a, b, rust, kind) {
  const fp = { codes: new Set(), msgs: new Map(), words: new Set(), variants: new Set(), numbers: new Set(), callees: new Set(), members: new Set(), branches: 0 };
  const lits = lx.litsByLine;
  // Internal-invariant text (`panic!`, `unreachable!`, `.expect(..)`, asserts) is not
  // user-facing behavior; a port may word or drop it freely.
  const INVARIANT = /\b(?:panic|unreachable|todo|unimplemented|assert|assert_eq|assert_ne|debug_assert\w*|bug)!\s*\(|\.expect\s*\(/;
  const invariantLine = (ln) => rust && (INVARIANT.test(lx.code[ln]) || (ln > a && INVARIANT.test(lx.code[ln - 1]) && !/[;{}]\s*$/.test(lx.code[ln - 1])));
  for (let ln = a; ln <= b; ln++) {
    if (invariantLine(ln)) continue;
    for (const t of lits.get(ln + 1) ?? []) {
      for (const m of t.matchAll(CODE_RE)) fp.codes.add(m[0]);
      const words = normMsg(t, rust);
      if (words.length >= 2) fp.msgs.set(words.join(' '), t);
      else if (words.length === 1 && words[0].length >= 4 && !/^\d+$/.test(words[0])) fp.words.add(words[0]);
    }
  }
  const typeLike = kind === 'struct' || kind === 'enum' || kind === 'union' || kind === 'trait' || kind === 'interface';
  if (typeLike) {
    let depth = 0;
    for (let ln = a; ln <= b; ln++) {
      const c = lx.code[ln];
      if (depth === 1 && ln > a) {
        const field = /^\s*(?:pub(?:\s*\([^)]*\))?\s+)?([a-z_][A-Za-z0-9_]*)\s*:(?!:)/.exec(c);
        const variant = /^\s*([A-Z][A-Za-z0-9_]*)\s*(?:[({,=]|$)/.exec(c);
        const fn = /^\s*(?:pub\s+)?(?:async\s+|unsafe\s+|const\s+)*fn\s+([A-Za-z_]\w*)/.exec(c);
        const m = fn ?? field ?? variant;
        if (m) fp.members.add(key(m[1]));
      }
      for (const ch of c) {
        if (ch === '{') depth++;
        else if (ch === '}') depth--;
      }
    }
    return fp;
  }
  for (let ln = a; ln <= b; ln++) {
    const c = lx.code[ln];
    for (const m of c.matchAll(CODE_RE)) fp.codes.add(m[0]);
    if (rust) {
      for (const m of c.matchAll(/\b([A-Z][A-Za-z0-9]*)::([A-Z][A-Za-z0-9_]*)\b(?!\s*::)/g)) {
        if (/^[A-Z0-9_]+$/.test(m[2]) && m[2].length > 1) continue; // associated const
        const v = key(m[2]);
        if (!IGNORED_VARIANTS.has(v)) fp.variants.add(v);
      }
      fp.branches += (c.match(/=>/g) ?? []).length + (c.match(/\bif\b/g) ?? []).length;
    } else {
      for (const m of c.matchAll(/\.([A-Z][A-Za-z0-9]*)\b/g)) {
        const v = key(m[1]);
        if (!IGNORED_VARIANTS.has(v)) fp.variants.add(v);
      }
      if (ln > a) {
        const ifs = (c.match(/\bif\b/g) ?? []).length;
        const arrows = (c.match(/->/g) ?? []).length;
        fp.branches += ifs + Math.max(0, arrows - ifs);
      }
    }
    if (ln > a || !/\bfn\b/.test(c)) {
      for (const m of c.matchAll(NUM_RE)) {
        const n = normNumber(m[1]);
        if (n !== null && n !== '0' && n !== '1' && n !== '2') fp.numbers.add(n);
      }
    }
    const callRe = rust ? CALL_RE_RUST : CALL_RE_JET;
    for (const m of c.matchAll(callRe)) {
      const name = m[1];
      if (KEYWORDS.has(name)) continue;
      if (ln === a && /\bfn\s+$/.test(c.slice(0, m.index))) continue; // the item's own name
      if (/^[A-Z]/.test(name)) {
        if (!/^[A-Z0-9_]+$/.test(name)) {
          const v = key(name);
          if (!IGNORED_VARIANTS.has(v)) fp.variants.add(v);
        }
        continue;
      }
      if (CALL_STOP.has(name) || name.length < 4) continue;
      fp.callees.add(key(name));
    }
  }
  return fp;
}

function weightOf(fp) {
  return (
    fp.codes.size * W.code +
    fp.msgs.size * W.msg +
    fp.words.size * W.word +
    fp.variants.size * W.variant +
    fp.numbers.size * W.number +
    fp.members.size * W.member
  );
}

function unionFp(fps) {
  const u = { codes: new Set(), msgs: new Map(), words: new Set(), variants: new Set(), numbers: new Set(), callees: new Set(), members: new Set(), branches: 0, msgWords: new Set(), msgText: '' };
  for (const f of fps) {
    for (const x of f.codes) u.codes.add(x);
    for (const [k, v] of f.msgs) u.msgs.set(k, v);
    for (const x of f.words) u.words.add(x);
    for (const x of f.variants) u.variants.add(x);
    for (const x of f.numbers) u.numbers.add(x);
    for (const x of f.callees) u.callees.add(x);
    for (const x of f.members) u.members.add(x);
    u.branches += f.branches;
  }
  for (const k of u.msgs.keys()) for (const w of k.split(' ')) u.msgWords.add(w);
  for (const w of u.words) u.msgWords.add(w);
  u.msgText = ' ' + [...u.msgs.keys()].join(' | ') + ' ';
  return u;
}

function msgFound(m, j) {
  if (j.msgs.has(m)) return true;
  if (m.length >= 12 && j.msgText.includes(m)) return true;
  const words = m.split(' ').filter((w) => w.length >= 3 && !/^\d+$/.test(w));
  if (words.length >= 3) {
    let hit = 0;
    for (const w of words) if (j.msgWords.has(w)) hit++;
    if (hit / words.length >= 0.8) return true;
  }
  if (words.length > 0 && words.length < 3 && words.every((w) => j.msgWords.has(w))) return true;
  return false;
}

// Number of Jet items carrying a feature (`v:<variant>`, `e:<member>`, ...); set
// once the Jet index exists. Common variants and members weigh less.
let DF = () => 0;
const rareWeight = (base, df) => (df <= 30 ? base : df <= 200 ? base * 0.66 : base * 0.25);

// Rust fingerprint r against Jet closure j. `distinct` counts matched features
// specific enough to identify a counterpart (a code, a message, a rare variant,
// member or a 3+ digit number).
function compare(r, j) {
  let total = 0;
  let got = 0;
  let distinct = 0;
  const miss = { codes: [], msgs: [], variants: [], numbers: [], members: [], callees: [] };
  for (const c of r.codes) {
    total += W.code;
    if (j.codes.has(c)) {
      got += W.code;
      distinct++;
    } else miss.codes.push(c);
  }
  for (const [m, raw] of r.msgs) {
    total += W.msg;
    if (msgFound(m, j)) {
      got += W.msg;
      distinct++;
    } else miss.msgs.push(raw);
  }
  for (const w of r.words) {
    total += W.word;
    if (j.msgWords.has(w) || j.msgs.has(w)) got += W.word;
  }
  for (const v of r.variants) {
    const df = DF('v:' + v);
    const w = rareWeight(W.variant, df);
    total += w;
    if (j.variants.has(v) || j.members.has(v)) {
      got += w;
      if (df <= 30) distinct++;
    } else miss.variants.push(v);
  }
  for (const n of r.numbers) {
    total += W.number;
    if (j.numbers.has(n)) {
      got += W.number;
      if (n.length >= 3) distinct++;
    } else miss.numbers.push(n);
  }
  for (const m of r.members) {
    const df = DF('e:' + m);
    const w = rareWeight(W.member, df);
    total += w;
    if (j.members.has(m)) {
      got += w;
      if (df <= 30 && m.length >= 5) distinct++;
    } else miss.members.push(m);
  }
  const core = total;
  let calleeTotal = 0;
  let calleeGot = 0;
  for (const c of r.callees) {
    calleeTotal += W.callee;
    let hit = j.callees.has(c);
    if (!hit && c.length >= 6) for (const x of j.callees) if (x.endsWith(c) || c.endsWith(x)) { hit = x.length >= 5; if (hit) break; }
    if (hit) calleeGot += W.callee;
    else miss.callees.push(c);
  }
  total += calleeTotal;
  got += calleeGot;
  return {
    score: total > 0 ? got / total : 0,
    core,
    coreScore: core > 0 ? (got - calleeGot) / core : null,
    calleeScore: calleeTotal > 0 ? calleeGot / calleeTotal : null,
    calleeCount: r.callees.size,
    distinct,
    miss,
    branches: [r.branches, j.branches],
  };
}

// ---------------------------------------------------------------------------
// Lexed files
// ---------------------------------------------------------------------------

function lexFile(text, rust) {
  const lx = lex(text, rust);
  const litsByLine = new Map();
  for (const l of lx.lits) {
    if (!litsByLine.has(l.line)) litsByLine.set(l.line, []);
    litsByLine.get(l.line).push(l.text);
  }
  return { code: lx.code, litsByLine };
}

// ---------------------------------------------------------------------------
// Jet index
// ---------------------------------------------------------------------------

const JET_ITEM_RE = /^\s*(?:#[A-Za-z_]\w*(?:\([^)]*\))?\s+)*(?:pub\s+|priv\s+)?(fn|struct|enum|interface|trait|type|union|impl)\s+([A-Za-z_][A-Za-z0-9_]*)/;

function scanJetFile(file, text, ref) {
  const lx = lexFile(text, false);
  const n = lx.code.length;
  const depthBefore = new Array(n);
  let d = 0;
  for (let ln = 0; ln < n; ln++) {
    depthBefore[ln] = d;
    for (const ch of lx.code[ln]) {
      if (ch === '{') d++;
      else if (ch === '}') d--;
    }
  }
  const heads = [];
  for (let ln = 0; ln < n; ln++) {
    const m = JET_ITEM_RE.exec(lx.code[ln]);
    if (m) heads.push({ ln, kind: m[1], name: m[2], depth: depthBefore[ln] });
  }
  const items = [];
  const scopes = [];
  for (let i = 0; i < heads.length; i++) {
    const h = heads[i];
    let end = extentEnd(lx.code, h.ln);
    for (let k = i + 1; k < heads.length; k++) {
      if (heads[k].depth <= h.depth) {
        if (heads[k].ln - 1 < end) end = heads[k].ln - 1;
        break;
      }
    }
    if (end < h.ln) end = h.ln;
    if (h.kind === 'impl') {
      scopes.push({ start: h.ln, end, depth: h.depth, name: h.name });
      continue;
    }
    if (h.kind === 'struct' || h.kind === 'enum' || h.kind === 'interface' || h.kind === 'trait') scopes.push({ start: h.ln, end, depth: h.depth, name: h.name });
    items.push({ file, ref, line: h.ln + 1, end: end + 1, kind: h.kind, name: h.name, start: h.ln, endLn: end, depth: h.depth });
  }
  for (const it of items) {
    let owner = null;
    for (const s of scopes) if (it.start > s.start && it.start <= s.end && it.depth === s.depth + 1) owner = s.name;
    it.owner = owner;
    it.fp = fingerprint(lx, it.start, it.endLn, false, it.kind);
  }
  return items;
}

// Top-level Jet package of a path (`Compiler/JetSema`, `Core/Text`, `Jetpack/Store`).
const pkgOf = (p) => p.split('/').slice(0, 2).join('/');

function buildJetIndex(items) {
  const byName = new Map(); // key(name) -> items
  const byPkgName = new Map(); // pkg \0 key(name) -> items
  for (const it of items) {
    const k = key(it.name);
    if (!byName.has(k)) byName.set(k, []);
    byName.get(k).push(it);
    const pk = `${it.ref}\0${pkgOf(it.file)}\0${k}`;
    if (!byPkgName.has(pk)) byPkgName.set(pk, []);
    byPkgName.get(pk).push(it);
  }
  // Inverted feature index for the fingerprint search.
  const post = new Map();
  const add = (f, it) => {
    let l = post.get(f);
    if (!l) post.set(f, (l = []));
    if (l.length <= 400 && l[l.length - 1] !== it) l.push(it);
  };
  for (const it of items) {
    const fp = it.fp;
    for (const c of fp.codes) add('c:' + c, it);
    for (const m of fp.msgs.keys()) add('m:' + m, it);
    for (const v of fp.variants) add('v:' + v, it);
    for (const nm of fp.numbers) if (nm.length >= 3) add('n:' + nm, it);
    for (const c of fp.callees) if (c.length >= 8) add('f:' + c, it);
    for (const m of fp.members) if (m.length >= 5) add('e:' + m, it);
  }
  const byFileLine = new Map();
  for (const it of items) {
    const fk = `${it.ref}\0${it.file}`;
    if (!byFileLine.has(fk)) byFileLine.set(fk, []);
    byFileLine.get(fk).push(it);
  }
  // Name word -> items, for renamed ports (`jet_rt_codec_decode_u64` for `jet_decode`).
  const byWord = new Map();
  for (const it of items) {
    for (const w of new Set(nameWords(it.name))) {
      if (w.length < 3) continue;
      let l = byWord.get(w);
      if (!l) byWord.set(w, (l = []));
      l.push(it);
    }
  }
  return { items, byName, byPkgName, byWord, post, byFileLine, closureMemo: new Map() };
}

// Jet item plus the items it calls in its own package (one level).
function closureOf(idx, it) {
  let c = idx.closureMemo.get(it);
  if (c) return c;
  const fps = [it.fp];
  const pkg = pkgOf(it.file);
  let n = 0;
  for (const callee of it.fp.callees) {
    const hits = idx.byPkgName.get(`${it.ref}\0${pkg}\0${callee}`);
    if (!hits) continue;
    for (const h of hits.slice(0, 3)) if (h !== it) fps.push(h.fp);
    if (++n >= 60) break;
  }
  c = unionFp(fps);
  idx.closureMemo.set(it, c);
  return c;
}

// Smallest Jet item containing `line` of `file` on `ref`, else the next item within 3 lines.
function itemAt(idx, ref, file, line) {
  const list = idx.byFileLine.get(`${ref}\0${file}`);
  if (!list) return null;
  let best = null;
  for (const it of list) {
    if (line >= it.line && line <= it.end && (best === null || it.end - it.line < best.end - best.line)) best = it;
  }
  if (best) return best;
  for (const it of list) if (it.line >= line && it.line <= line + 3) return it;
  return null;
}

// ---------------------------------------------------------------------------
// Runtime route census (live Core routes with no Jet runtime export)
// ---------------------------------------------------------------------------

function exportsOf(text) {
  const names = [];
  const lines = text.split('\n');
  lines.forEach((line, index) => {
    if (!line.startsWith('#Export(c)')) return;
    const rest = line.slice('#Export(c)'.length).trim();
    let at = index;
    let next = rest;
    if (!next) {
      at = lines.findIndex((row, k) => k > index && row && !row.startsWith('#') && !row.startsWith('//'));
      next = at >= 0 ? lines[at] : '';
    }
    const name = /^(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/.exec(next ?? '')?.[1];
    if (name) names.push({ name, line: at + 1 });
  });
  return names;
}

const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

// The Core call registry links a Rust Prelude symbol to its Core member and to
// the Jet body that now serves it: a runtime #Export(c) of the route's export,
// or the member's body in a source-owned Core module. `live` lists the routes
// a Jet program can reach whose export no Jet runtime defines.
function coreRegistry(jetFiles, idx) {
  const read = (p) => jetFiles.get(p) ?? fail(`route census needs ${p}`);
  const runtimeFiles = read('Compiler/JetRuntime/sources.list')
    .split('\n')
    .map((s) => s.trim())
    .filter((s) => s && !s.startsWith('#'))
    .map((s) => (s.startsWith('Compiler/') ? s : join('Compiler/JetRuntime', s)));
  const runtime = new Map();
  for (const f of runtimeFiles) if (jetFiles.has(f)) for (const e of exportsOf(jetFiles.get(f))) runtime.set(e.name, { file: f, line: e.line });
  const owned = new Map();
  for (const line of read('Compiler/JetFoundation/Source/Registry/CoreSourceOwned.jet').split('\n')) {
    const m = /CoreSourceOwnedRow\{module_name: "([^"]+)", alias: "[^"]*", path: "([^"]+)", owner: "([^"]*)", owned_members: \[String\]\{([^}]*)\}/.exec(line);
    if (!m) continue;
    const k = m[3] || m[1];
    const row = owned.get(k) ?? { paths: [], members: new Set() };
    row.paths.push(m[2]);
    if (!m[3]) for (const x of m[4].matchAll(/"([^"]+)"/g)) row.members.add(x[1]);
    owned.set(k, row);
  }
  const routes = read('Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet');
  const exportOf = new Map([...routes.matchAll(/LowerRoute\{symbol: "([^"]+)", export: "([^"]+)"/g)].map((m) => [m[1], m[2]]));
  const callRows = [];
  for (const line of read('Compiler/JetFoundation/Source/Registry/CoreCallRows.jet').split('\n')) {
    const m = /module_name: "([^"]+)", member: "([^"]+)".*symbol: CoreCallSymbol\.Prelude\{name: "([^"]+)"\}/.exec(line);
    if (m) callRows.push({ module: m[1], member: m[2], symbol: m[3] });
  }
  const callSites = (module, member) => {
    const qualified = new RegExp(`\\b${esc(module)}\\.${esc(member)}\\(`, 'g');
    const sites = [];
    for (const [rel, t] of jetFiles) {
      if (!t.includes(member)) continue;
      let n = (t.match(qualified) ?? []).length;
      for (const u of t.matchAll(new RegExp(`^use ${esc(module)}(?: as ([A-Za-z_][A-Za-z0-9_]*))?\\s*$`, 'gm'))) {
        const alias = u[1] ?? module.split('.').pop();
        n += (t.match(new RegExp(`\\b${esc(alias)}\\.${esc(member)}\\(`, 'g')) ?? []).length;
      }
      for (const u of t.matchAll(new RegExp(`^use ${esc(module)}\\.\\[([^\\]]*)\\]`, 'gm'))) {
        if (u[1].split(',').map((s) => s.trim()).includes(member)) n += (t.match(new RegExp(`(?<![.\\w])${esc(member)}\\(`, 'g')) ?? []).length;
      }
      if (n) sites.push(`${rel}:${n}`);
    }
    return sites;
  };
  const link = new Map(); // Rust symbol or export name -> { call, exp, symbol, jet (item) | null, live }
  const live = [];
  for (const u of callRows) {
    const exp = exportOf.get(u.symbol) ?? u.symbol.split('::').pop();
    const call = `${u.module}.${u.member}`;
    let jet = null;
    const rt = runtime.get(exp);
    if (rt) jet = itemAt(idx, '', rt.file, rt.line);
    const own = owned.get(u.module);
    if (!jet && own?.members.has(u.member)) jet = (idx.byName.get(key(u.member)) ?? []).find((it) => it.ref === '' && own.paths.some((p) => it.file.startsWith(p.replace(/\/?$/, p.endsWith('.jet') ? '' : '/')))) ?? null;
    const entry = { call, exp, symbol: u.symbol, jet, live: false, sites: '' };
    if (!rt && exportOf.has(u.symbol) && !own?.members.has(u.member)) {
      const sites = callSites(u.module, u.member);
      if (sites.length) {
        entry.live = true;
        entry.sites = sites.slice(0, 3).join(' ');
        live.push(entry);
      }
    }
    for (const k of [u.symbol, exp]) if (!link.has(k) || (entry.live && !link.get(k).live)) link.set(k, entry);
  }
  return { link, live };
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

const STD_TRAITS = new Set(
  'Display Debug Clone Copy Default PartialEq Eq Hash PartialOrd Ord From Into TryFrom TryInto Drop Deref DerefMut AsRef AsMut Borrow Iterator IntoIterator DoubleEndedIterator ExactSizeIterator FromStr Error Serialize Deserialize Send Sync Write Read Index IndexMut Add Sub Mul Div Rem Neg Not BitAnd BitOr BitXor Shl Shr AddAssign SubAssign MulAssign DivAssign Sum Extend FromIterator Future Unpin'.split(
    ' ',
  ),
);

// Whether Rust line `ln` (0-based) sits directly in an `extern "C" { ... }` block.
function inExternBlock(lx, ln) {
  const indent = (s) => s.length - s.trimStart().length;
  const own = indent(lx.code[ln]);
  for (let k = ln - 1; k >= 0; k--) {
    const c = lx.code[k];
    if (c.trim() === '' || indent(c) >= own) continue;
    return /^\s*(?:unsafe\s+)?extern\b[^{]*\{\s*$/.test(c);
  }
  return false;
}

// Trait of the impl block enclosing Rust line `ln` (0-based), or null.
function implTraitAt(lx, ln) {
  const indent = (s) => s.length - s.trimStart().length;
  const own = indent(lx.code[ln]);
  for (let k = ln - 1; k >= 0; k--) {
    const c = lx.code[k];
    if (c.trim() === '' || indent(c) >= own) continue;
    if (!/^\s*(?:unsafe\s+)?impl\b/.test(c)) return null;
    let header = c;
    for (let j = k + 1; j < Math.min(lx.code.length, k + 6) && !header.includes('{'); j++) header += ' ' + lx.code[j];
    header = header.split('{')[0];
    const m = /\bimpl\b(?:\s*<.*?>)?\s+(.+?)\s+for\s+/.exec(header);
    if (!m) return null;
    return m[1].replace(/<.*$/, '').split('::').pop().replace(/^[?!]/, '').trim();
  }
  return null;
}

function splitPointer(p) {
  // `path:line`, `<ref>:path:line` or `<sha>:path`.
  if (!p || p === '-') return null;
  const m = /^(?:([^:]+):)?([^:]+\.jet):(\d+)$/.exec(p);
  if (!m) return null;
  return { ref: m[1] ?? null, file: m[2], line: Number(m[3]) };
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  REPO = resolve(opts.repo);
  const outDir = join(ROOT, OUT_REL);
  const summary = JSON.parse(readFileSync(join(outDir, 'summary.json'), 'utf8'));
  const rustRev = resolveCommit(summary.rev);
  const jetRev = resolveCommit(opts.jetRev);
  const refs = opts.jetRefs
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
    .map((name) => ({ name, sha: resolveCommit(name) }));
  const log = (s) => process.stderr.write(`[fingerprint] ${s}\n`);

  // Ledger rows in scope.
  const lines = readFileSync(join(outDir, 'items.tsv'), 'utf8').split('\n');
  const head = lines[0].split('\t');
  const col = Object.fromEntries(head.map((h, i) => [h, i]));
  const rows = [];
  for (const l of lines.slice(1)) {
    if (!l) continue;
    const f = l.split('\t');
    const area = areaOf(f[col.rust_file]);
    if (!area || isTestPath(f[col.rust_file])) continue;
    rows.push({
      area,
      file: f[col.rust_file],
      line: Number(f[col.line]),
      end: Number(f[col.end]),
      kind: f[col.kind],
      owner: f[col.owner],
      name: f[col.name],
      status: f[col.status],
      evidence: f[col.evidence],
      jet: f[col.jet],
      cls: f[col.class],
      group: f[col.group],
      clsEvidence: f[col.class_evidence],
      // Owner type, or for a free function the Rust file stem: words a renamed Jet port
      // often keeps (`DeterministicWorld::enter` -> `world_scope_enter`).
      scope: f[col.owner] && f[col.owner] !== '-' ? f[col.owner] : f[col.rust_file].split('/').pop().replace(/\.rs$/, ''),
    });
  }
  log(`in-scope Rust rows: ${rows.length}`);

  // Rust fingerprints.
  const rustFiles = [...new Set(rows.map((r) => r.file))];
  const rustEntries = lsTree(rustRev, rustFiles);
  const rustBlobs = readBlobs(rustEntries.map((e) => e.sha));
  const rustLex = new Map();
  for (const e of rustEntries) rustLex.set(e.path, lexFile(rustBlobs.get(e.sha), true));
  for (const r of rows) {
    const lx = rustLex.get(r.file);
    if (!lx) fail(`Rust file missing at ${rustRev}: ${r.file}`);
    const kind = r.kind === 'method' || r.kind === 'macro' ? 'fn' : r.kind;
    r.fp = fingerprint(lx, r.line - 1, r.end - 1, true, kind);
    r.weight = weightOf(r.fp);
    r.trait = r.kind === 'method' ? implTraitAt(lx, r.line - 1) : null;
    r.bodiless = r.kind === 'fn' && !lx.code.slice(r.line - 1, r.end).some((l) => l.includes('{')) && inExternBlock(lx, r.line - 1);
  }
  log('Rust fingerprints done');

  // Jet trees: primary in full, secondaries only where they differ from it.
  const primaryEntries = lsTree(jetRev, JET_ROOTS).filter((e) => e.path.endsWith('.jet'));
  const primaryShas = new Map(primaryEntries.map((e) => [e.path, e.sha]));
  const secondary = [];
  for (const r of refs) {
    for (const e of lsTree(r.sha, JET_ROOTS)) {
      if (!isJetSource(e.path) || primaryShas.get(e.path) === e.sha) continue;
      secondary.push({ ...e, ref: r.name });
    }
  }
  const blobs = readBlobs([...primaryEntries.map((e) => e.sha), ...secondary.map((e) => e.sha)]);
  const jetItems = [];
  const jetFiles = new Map();
  for (const e of primaryEntries) {
    const text = blobs.get(e.sha);
    jetFiles.set(e.path, text);
    if (isJetSource(e.path)) jetItems.push(...scanJetFile(e.path, text, ''));
  }
  for (const e of lsTree(jetRev, ['Compiler/JetRuntime/sources.list'])) jetFiles.set(e.path, readBlobs([e.sha]).get(e.sha));
  for (const e of lsTree(jetRev, ['Tools', 'Examples']).filter((x) => x.path.endsWith('.jet'))) {
    jetFiles.set(e.path, readBlobs([e.sha]).get(e.sha));
  }
  const nPrimary = jetItems.length;
  for (const e of secondary) jetItems.push(...scanJetFile(e.path, blobs.get(e.sha), e.ref));
  log(`Jet items: ${nPrimary} primary, ${jetItems.length - nPrimary} secondary`);
  const idx = buildJetIndex(jetItems);
  const N = jetItems.length;
  DF = (f) => idx.post.get(f)?.length ?? 0;
  // Prelude symbols the native lowering implements inline: compared against a
  // symbol (`symbol == "jet_map_setdefault"`, `"jet_x" ->`) in JetBackend/JetCodegen.
  const lowered = new Map();
  for (const [file, text] of jetFiles) {
    if (!/^Compiler\/(JetBackend|JetCodegen)\//.test(file) || file.endsWith('/RuntimeRoutes.jet') || !isJetSource(file)) continue;
    const ls = text.split('\n');
    ls.forEach((l, i) => {
      for (const m of l.matchAll(/==\s*"(jet_\w+)"/g)) {
        if (!lowered.has(m[1])) lowered.set(m[1], { file, line: i + 1 });
      }
    });
  }
  const registry = coreRegistry(jetFiles, idx);
  log(`Core registry: ${registry.link.size} linked symbols, ${registry.live.length} live routes without a Jet runtime export`);
  // A secondary-tree Compiler file outside that tree's compiler unit (or a
  // Jetpack CLI file) was never compiled there: a draft, not a port.
  const compiledOn = new Map();
  for (const r of refs) {
    const list = lsTree(r.sha, ['Compiler/Bootstrap/sources.list']);
    const text = list.length ? readBlobs([list[0].sha]).get(list[0].sha) : '';
    compiledOn.set(r.name, new Set(text.split('\n').map((s) => s.trim()).filter((s) => s && !s.startsWith('#'))));
  }
  const isDraft = (it) => it.ref !== '' && ((it.file.startsWith('Compiler/') && !compiledOn.get(it.ref).has(it.file)) || it.file.startsWith('Jetpack/CLI/'));

  // First Jet runtime/Core call site of an OS symbol a Rust `extern` block declared.
  const externUse = (name) => {
    const re = new RegExp(`\\b${esc(name)}\\s*\\(`);
    for (const [file, text] of jetFiles) {
      if (!/^(Compiler|Core)\//.test(file) || !text.includes(name)) continue;
      const i = text.search(re);
      if (i >= 0) return `${file}:${text.slice(0, i).split('\n').length}`;
    }
    return null;
  };

  const triageGroups = new Map();
  for (const l of readFileSync(join(outDir, 'triage-groups.tsv'), 'utf8').split('\n').slice(1)) {
    const [g, c, why] = l.split('\t');
    if (g) triageGroups.set(g, { cls: c, why });
  }

  // Jet trees outside the compiler/Core/runtime scope never count as an in-area port.
  const OUT_OF_SCOPE_JET = ['Compiler/JetDevserver/', 'Compiler/JetRepl/', 'Compiler/JetCanvas/', 'Compiler/JetDebug/'];
  const inAreaDirs = (area, it) => AREA_DIRS[area].some((d) => it.file.startsWith(d)) && !OUT_OF_SCOPE_JET.some((d) => it.file.startsWith(d));

  const nameCandidates = (r) => {
    const out = new Set();
    const k = key(r.name);
    const ks = [k];
    if (r.owner && r.owner !== '-') ks.push(key(r.owner) + k);
    for (const kk of ks) for (const it of idx.byName.get(kk) ?? []) out.add(it);
    // Jet names that end with the Rust name (`sema_checked_foreign_boundary`).
    if (r.name.includes('_') && k.length >= 8) {
      for (const [jk, its] of idx.byName) if (jk.length > k.length && jk.endsWith(k)) for (const it of its) out.add(it);
    }
    // Prelude spellings: `jet_<module words>_<member>` and `Jet<Type>::<member>` name a Core
    // member `<member>` whose owner or file carries the module words.
    const scoped = (member, words) => {
      for (const it of idx.byName.get(key(member)) ?? []) {
        const where = key(it.owner ?? '') + it.file.toLowerCase().replace(/[^a-z0-9]/g, '');
        if (words.every((w) => where.includes(w))) out.add(it);
      }
    };
    const pm = /^jet(?:lib)?_(?:std_|rt_|core_)?(.+)$/.exec(r.name);
    if (pm) {
      const toks = pm[1].split('_').filter(Boolean);
      for (let s = 1; s < toks.length; s++) scoped(toks.slice(s).join('_'), toks.slice(0, s).map((w) => w.toLowerCase()));
    }
    if (r.owner && /^Jet[A-Z]/.test(r.owner)) scoped(r.name, snakeWords(r.owner.slice(3)));
    return [...out];
  };

  const searchCandidates = (r) => {
    const score = new Map();
    const bump = (f, w) => {
      const l = idx.post.get(f);
      if (!l || l.length > 400) return;
      const idf = Math.log(N / l.length);
      for (const it of l) score.set(it, (score.get(it) ?? 0) + w * idf);
    };
    for (const c of r.fp.codes) bump('c:' + c, W.code);
    for (const m of r.fp.msgs.keys()) bump('m:' + m, W.msg);
    for (const v of r.fp.variants) bump('v:' + v, W.variant);
    for (const n of r.fp.numbers) if (n.length >= 3) bump('n:' + n, W.number);
    for (const c of r.fp.callees) if (c.length >= 8) bump('f:' + c, W.callee);
    for (const m of r.fp.members) if (m.length >= 5) bump('e:' + m, W.member);
    return [...score.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([it]) => it);
  };

  const fmtJet = (it) => `${it.ref ? it.ref + ':' : ''}${it.file}:${it.line}`;
  const fmtMiss = (cmpr) => {
    const m = cmpr.miss;
    const parts = [];
    if (m.codes.length) parts.push(`codes ${m.codes.join(',')}`);
    if (m.msgs.length) parts.push(`msgs ${m.msgs.slice(0, 4).map((s) => JSON.stringify(s.length > 70 ? s.slice(0, 70) + '…' : s)).join(' ')}${m.msgs.length > 4 ? ` +${m.msgs.length - 4}` : ''}`);
    if (m.variants.length) parts.push(`variants ${m.variants.slice(0, 8).join(',')}${m.variants.length > 8 ? ` +${m.variants.length - 8}` : ''}`);
    if (m.members.length) parts.push(`members ${m.members.slice(0, 8).join(',')}${m.members.length > 8 ? ` +${m.members.length - 8}` : ''}`);
    if (m.numbers.length) parts.push(`numbers ${m.numbers.slice(0, 6).join(',')}`);
    const [rb, jb] = cmpr.branches;
    if (rb >= 6 && jb < rb * 0.5) parts.push(`branches ${rb}->${jb}`);
    return parts.join('; ') || '-';
  };

  // Evaluates candidates; returns the best slot per tree (primary / secondary).
  // `eff` is the score, less 0.15 for a fingerprint-only hit outside the area's
  // directories whose name shares nothing with the Rust name (a coincidental
  // message or variant overlap); ranking adds small area/ledger/name bonuses.
  const evaluate = (r, cands) => {
    const prim = [];
    const sec = [];
    const generic = isGenericName(r.name);
    const typeLike = r.kind === 'struct' || r.kind === 'enum' || r.kind === 'trait' || r.kind === 'type';
    const scopeWords = nameWords(r.scope).filter((w) => w.length >= 3);
    for (const { it, via } of cands) {
      const c = compare(r.fp, closureOf(idx, it));
      const aff = nameAffinity(r.name, r.scope, it.name);
      const inArea = inAreaDirs(r.area, it);
      const owned = ownerAffinity(r, it) || nameWords(it.name).some((w) => scopeWords.includes(w));
      // Every owner (or file-stem) word in the Jet owner, name or file stem.
      const where = key(it.owner ?? '') + key(it.name) + key(it.file.split('/').pop().replace(/\.jet$/, ''));
      const ownedStrict = scopeWords.length > 0 && scopeWords.every((w) => w === 'mod' || where.includes(w));
      // A candidate must be identifiable: a trusted pointer, a name sharing words with the
      // Rust name (single-word names also need all the owner's or file's words in the Jet
      // owner, name or file), or several matched codes/messages/rare features. Data
      // tables (registry rows, embedded sources) match many messages and only count by name.
      const named = aff >= 0.5 && (!generic || ownedStrict);
      const table = it.fp.msgs.size > 150;
      // An item with a single message or code is identified by matching it in the area.
      const needed = inArea && r.fp.codes.size + r.fp.msgs.size === 1 ? 1 : (inArea ? 2 : 3) + (typeLike ? 1 : 0);
      const vouched = via === 'trusted' || via === 'registry' || via === 'lowering' || named || (!table && c.distinct >= needed);
      let eff = Math.max(0, c.score - ((via === 'fingerprint' || via === 'callees') && !inArea && aff < 0.2 ? 0.15 : 0));
      if (!vouched) eff = Math.min(eff, 0.29);
      const rank = eff + (inArea ? 0.03 : 0) + (via === 'trusted' || via === 'registry' || via === 'lowering' ? 0.1 : 0) + 0.1 * aff + (owned ? 0.03 : 0);
      const slot = { it, cmp: c, via, eff, rank, aff, inArea, owned, ownedStrict, named, vouched };
      (it.ref === '' ? prim : sec).push(slot);
      if (process.env.FP_DEBUG === r.name) process.stderr.write(`[debug ${r.name}] ${it.ref}:${it.file}:${it.line} ${it.name} via=${via} score=${c.score.toFixed(2)} eff=${eff.toFixed(2)} aff=${aff.toFixed(2)} inArea=${inArea} owned=${owned}/${ownedStrict} named=${named} vouched=${vouched} distinct=${c.distinct}\n`);
    }
    prim.sort((a, b) => b.rank - a.rank);
    sec.sort((a, b) => b.rank - a.rank);
    return { best: prim[0] ?? null, bestSec: sec[0] ?? null, prim, sec };
  };

  const out = [];
  const routed = new Set();
  // Traits the Rust-emitted program protocol defines (JetShow, JetDebug, ...).
  const preludeTraits = new Set(rows.filter((r) => r.area === 'core' && r.kind === 'trait').map((r) => r.name));
  let done = 0;
  // Area-wide codes and messages, for items spread over many Jet items.
  const areaUnions = new Map();
  const areaUnionOf = (area) => {
    let u = areaUnions.get(area);
    if (u) return u;
    u = { codes: new Set(), msgKeys: new Set(), msgText: '' };
    const parts = [];
    for (const it of idx.items) {
      if (it.ref !== '' || !inAreaDirs(area, it)) continue;
      for (const c of it.fp.codes) u.codes.add(c);
      for (const m of it.fp.msgs.keys()) if (!u.msgKeys.has(m)) {
        u.msgKeys.add(m);
        parts.push(m);
      }
    }
    u.msgText = ' ' + parts.join(' | ') + ' ';
    areaUnions.set(area, u);
    return u;
  };
  const spread = (r) => {
    const u = areaUnionOf(r.area);
    let total = 0;
    let got = 0;
    const missCodes = [];
    const missMsgs = [];
    const byFile = new Map();
    const credit = (f) => {
      for (const it of idx.post.get(f) ?? []) {
        if (it.ref !== '' || !inAreaDirs(r.area, it)) continue;
        const e = byFile.get(it.file) ?? { n: 0, line: it.line };
        e.n++;
        e.line = Math.min(e.line, it.line);
        byFile.set(it.file, e);
      }
    };
    for (const c of r.fp.codes) {
      total += W.code;
      if (u.codes.has(c)) {
        got += W.code;
        credit('c:' + c);
      } else missCodes.push(c);
    }
    for (const [m, raw] of r.fp.msgs) {
      total += W.msg;
      if (u.msgKeys.has(m) || (m.length >= 12 && u.msgText.includes(m))) {
        got += W.msg;
        credit('m:' + m);
      } else missMsgs.push(raw);
    }
    const top = [...byFile.entries()].sort((a, b) => b[1].n - a[1].n).slice(0, 3);
    const parts = [];
    if (missCodes.length) parts.push(`codes ${missCodes.join(',')}`);
    if (missMsgs.length) parts.push(`msgs ${missMsgs.slice(0, 4).map((s) => JSON.stringify(s.length > 70 ? s.slice(0, 70) + '…' : s)).join(' ')}${missMsgs.length > 4 ? ` +${missMsgs.length - 4}` : ''}`);
    return { score: total ? got / total : 0, missCodes, files: top.map(([f]) => f), line: top.length ? top[0][1].line : 0, missText: parts.join('; ') || '-' };
  };

  for (const r of rows) {
    if (++done % 5000 === 0) log(`classified ${done}/${rows.length}`);
    const rec = { r, cls: null, jet: '-', score: '-', missing: '-', basis: '-', reason: '-' };
    out.push(rec);
    // Obsolete by reviewed reason; a Jet counterpart, if found, still wins below, except
    // that Rust language glue (std-trait impls, Rust-emit protocol traits, macro_rules)
    // never counts as ported through a mere same-name match.
    let obsolete = null;
    let glue = false;
    if (r.status === 'dropped' && r.evidence === 'rust-only-glue') obsolete = 'rust-only-glue: build script / Rust bootstrap host / std-trait glue retired with Rust';
    else if (r.cls === 'obsolete') obsolete = `${r.group}: ${triageGroups.get(r.group)?.why ?? r.clsEvidence}`;
    else if (r.trait && STD_TRAITS.has(r.trait)) {
      obsolete = `rust-std-trait: \`impl ${r.trait}\` glue; Jet derives or spells this behavior through its own interfaces`;
      glue = true;
    } else if (r.area === 'core' && ((r.trait && preludeTraits.has(r.trait)) || r.kind === 'trait')) {
      obsolete = `rust-emit-protocol: ${r.kind === 'trait' ? 'trait' : `\`impl ${r.trait}\``} of the Rust-emitted program protocol; Jet's backend dispatches show/debug/clone/drop natively`;
      glue = true;
    } else if (r.kind === 'macro') {
      obsolete = 'rust-macro: a macro_rules shim over Rust syntax; Jet has no macro layer';
      glue = true;
    }

    // A live Core route whose export no Jet runtime defines: an absent runtime leaf.
    const reg = r.area === 'core' || r.area === 'runtime' ? registry.link.get(r.name) ?? registry.link.get(`${r.owner}::${r.name}`) : null;
    if (reg?.live) {
      routed.add(reg);
      rec.cls = 'ABSENT';
      rec.basis = 'route-census';
      rec.missing = `no Jet runtime #Export(c) ${reg.exp} for live route ${reg.symbol} (${reg.call}); called at ${reg.sites}`;
      rec.routeGroup = reg.call.split('.').slice(0, 2).join('.');
      continue;
    }

    const cands = [];
    const seen = new Set();
    const addC = (it, via) => {
      if (it && !seen.has(it)) {
        seen.add(it);
        cands.push({ it, via });
      }
    };
    if (reg?.jet) addC(reg.jet, 'registry');
    const low = lowered.get(r.name);
    if (low) addC(itemAt(idx, '', low.file, low.line), 'lowering');
    // The ledger pointer is trusted only when a Jet comment cites the Rust item, or the
    // reviewed triage cites the covering Jet line; its name rules are re-checked here.
    const trustedPointer = /(^|:)(port-comment|rust-name-comment)$/.test(r.evidence);
    const pointers = [[splitPointer(r.jet), trustedPointer], ...[...(r.clsEvidence ?? '').matchAll(/((?:[A-Za-z0-9_.-]+\/)+[A-Za-z0-9_.-]+\.jet):(\d+)/g)].map((m) => [{ ref: null, file: m[1], line: Number(m[2]) }, r.cls === 'covered'])];
    for (const [p, trusted] of pointers) {
      if (!p) continue;
      const refName = p.ref === null ? '' : refs.find((x) => x.name === p.ref || x.sha === p.ref)?.name;
      if (refName === undefined) continue;
      addC(itemAt(idx, refName, p.file, p.line) ?? (refName ? itemAt(idx, '', p.file, p.line) : null), trusted ? 'trusted' : 'ledger');
    }
    const named = nameCandidates(r);
    const namedArea = named.filter((it) => inAreaDirs(r.area, it));
    const byAff = (list) => list.map((it) => [it, nameAffinity(r.name, r.scope, it.name)]).sort((a, b) => b[1] - a[1] || Number(inAreaDirs(r.area, b[0])) - Number(inAreaDirs(r.area, a[0]))).map(([it]) => it);
    for (const it of byAff(namedArea.length ? namedArea : named).slice(0, 40)) addC(it, 'name');
    // Renamed ports: items sharing most name words (with the owner's or file's words),
    // found through the rarest word.
    const rare = [...nameWords(r.name), ...nameWords(r.scope)]
      .filter((w) => w.length >= 3)
      .map((w) => idx.byWord.get(w) ?? [])
      .filter((l) => l.length > 0 && l.length <= 5000)
      .sort((a, b) => a.length - b.length)[0];
    if (rare) {
      const close = byAff(rare.filter((it) => nameAffinity(r.name, r.scope, it.name) >= 0.5));
      for (const it of close.slice(0, 30)) addC(it, 'words');
    }
    const thin = r.weight < 2;
    if (!thin) for (const it of searchCandidates(r)) addC(it, 'fingerprint');
    else if (r.fp.callees.size >= 3) for (const it of searchCandidatesByCallees(idx, r, N)) addC(it, 'callees');

    const { best, bestSec, prim, sec } = evaluate(r, cands);
    // PARTIAL means missing diagnostics, user-facing messages or collapsed branches; a
    // counterpart that keeps all of those and differs only in variant, member or number
    // spelling is a port.
    const collapsed = (cm) => cm.branches[0] >= 6 && cm.branches[1] < cm.branches[0] * 0.5;
    const take = (slot, cls, basis) => {
      let c = cls === 'PORTED-UNMERGED' && isDraft(slot.it) ? 'DRAFT-ONLY' : cls;
      let b = basis;
      if (c === 'PARTIAL' && slot.cmp.miss.codes.length === 0 && slot.cmp.miss.msgs.length === 0 && !collapsed(slot.cmp) && slot.vouched) {
        c = 'PORTED';
        b = `${basis}:shape`;
      }
      rec.cls = c;
      rec.jet = fmtJet(slot.it);
      rec.score = slot.cmp.score.toFixed(2);
      rec.missing = c === 'PORTED' ? (b.endsWith(':shape') ? `spelling differs: ${fmtMiss(slot.cmp)}` : '-') : fmtMiss(slot.cmp);
      rec.basis = b;
      rec.slot = slot;
    };
    const pinned = (s) => s.via === 'trusted' || s.via === 'registry' || s.via === 'lowering';

    if (r.bodiless) {
      // An `extern` declaration: the port is the Jet runtime calling the same OS symbol.
      const use = externUse(r.name);
      if (use) {
        rec.cls = 'PORTED';
        rec.jet = use;
        rec.basis = 'extern-use';
      } else {
        rec.cls = 'OBSOLETE';
        rec.basis = 'extern-decl';
        obsolete = 'rust-extern-decl: a libc/OS declaration for the Rust host; no Jet code calls this symbol';
      }
    } else if (thin) {
      // Too little behavior to fingerprint: a pinned body (Jet comment citing the Rust,
      // registry, inline lowering), a same-name item in the area (or in a Jet file named
      // after the Rust owner), or a strong callee match.
      const calleesAgree = (s) => s.cmp.calleeScore === null || s.cmp.calleeScore >= 0.25 || s.cmp.calleeCount < 3;
      const acceptable = (s) => pinned(s) || (!glue && s.named && (s.inArea || (s.ownedStrict && s.aff >= 0.75)) && calleesAgree(s));
      const hit = prim.find(acceptable);
      const calleeHit = !glue && best && best.cmp.calleeCount >= 3 && best.eff >= 0.6 && best.cmp.calleeScore >= 0.6 && (best.cmp.coreScore === null || best.cmp.coreScore >= 0.5);
      const secHit = sec.find(acceptable);
      if (hit) take(hit, 'PORTED', `thin:${hit.via}`);
      else if (calleeHit) take(best, 'PORTED', 'thin:callees');
      else if (secHit) take(secHit, 'PORTED-UNMERGED', `thin:${secHit.via}`);
      else {
        rec.cls = obsolete ? 'OBSOLETE' : 'ABSENT';
        rec.basis = 'thin';
        if (best) rec.missing = `nearest ${fmtJet(best.it)} (${best.via}, ${best.cmp.score.toFixed(2)})`;
      }
    } else {
      const primaryScore = best ? best.eff : 0;
      const ported = best && best.eff >= 0.7 && best.cmp.miss.codes.length === 0;
      // A strongly named in-area counterpart that keeps every code and message is a port
      // whose variants, members or numbers are merely spelled differently.
      const namedPort = prim.find((s) => s.named && s.aff >= 0.75 && (s.inArea || s.ownedStrict) && s.cmp.miss.codes.length === 0 && s.cmp.miss.msgs.length === 0 && s.cmp.score >= 0.4);
      // A named in-area dev counterpart outranks a fuller secondary-tree copy: the dev item is the port.
      const devNamed = prim.find((s) => s.named && s.inArea && s.eff >= 0.3);
      if (ported) take(best, 'PORTED', best.via);
      else if (namedPort) take(namedPort, 'PORTED', `${namedPort.via}:named`);
      else if (devNamed) take(devNamed, 'PARTIAL', devNamed.via);
      else if (bestSec && bestSec.eff >= 0.5 && bestSec.eff >= primaryScore + 0.15) {
        take(bestSec, 'PORTED-UNMERGED', bestSec.via);
        if (best && primaryScore >= 0.3) rec.missing = `dev partial ${fmtJet(best.it)} (${primaryScore.toFixed(2)}); ${rec.missing}`;
      } else if (best && (primaryScore >= 0.3 || pinned(best))) take(best, 'PARTIAL', best.via);
      else {
        rec.cls = obsolete ? 'OBSOLETE' : 'ABSENT';
        rec.basis = 'fingerprint';
        if (best) rec.missing = `nearest ${fmtJet(best.it)} (${best.via}, ${primaryScore.toFixed(2)})`;
      }
    }
    // Rust language glue is only ported when a pinned body or a distinctive fingerprint
    // (two matched codes, messages or rare features) says so, never by its name alone.
    if (glue && rec.slot && !pinned(rec.slot) && rec.slot.cmp.distinct < 2) {
      rec.cls = 'OBSOLETE';
      rec.missing = `name-only match ${rec.jet}`;
      rec.jet = '-';
      rec.score = '-';
    }
    // A large Rust item (a 1000-line dispatcher) is usually spread over many Jet items:
    // measure its codes and messages against the whole area instead of one counterpart.
    const lines = r.end - r.line + 1;
    if (lines >= SPREAD_LINES && r.fp.codes.size + r.fp.msgs.size >= 3 && rec.cls !== 'PORTED' && rec.cls !== 'PORTED-UNMERGED' && rec.cls !== 'DRAFT-ONLY') {
      const s = spread(r);
      const single = best ? best.eff : 0;
      if (s.score >= 0.3 && s.score > single) {
        rec.cls = s.score >= 0.85 && s.missCodes.length === 0 ? 'PORTED' : 'PARTIAL';
        rec.jet = s.files.length ? `${s.files[0]}:${s.line}` : rec.jet;
        rec.score = s.score.toFixed(2);
        rec.basis = 'spread';
        rec.missing = rec.cls === 'PORTED' ? `spread over ${s.files.join(', ')}` : `spread over ${s.files.join(', ')}; ${s.missText}`;
      }
    }
    if (rec.cls === 'OBSOLETE') rec.reason = obsolete;
    else if (obsolete && rec.cls !== 'ABSENT') rec.reason = `would be obsolete (${obsolete.split(':')[0]}) but a Jet counterpart exists`;
    if (r.status === 'kept') rec.basis += ';kept-only-on-cutover-ready';
  }

  // Live routes no Rust row names: standalone runtime leaves.
  for (const reg of registry.live) {
    if (routed.has(reg)) continue;
    out.push({
      r: { area: 'runtime-route', file: '-', line: 0, end: 0, kind: 'route', owner: reg.call, name: reg.exp, weight: 0 },
      cls: 'ABSENT',
      jet: '-',
      score: '-',
      missing: `no Jet runtime #Export(c) ${reg.exp} for live route ${reg.symbol}; called at ${reg.sites}`,
      basis: 'route-census',
      reason: '-',
      routeGroup: reg.call.split('.').slice(0, 2).join('.'),
    });
  }
  log(`live routes without a Jet runtime export: ${registry.live.length} (${routed.size} matched to a Rust row)`);

  const packages = buildPackages(out);
  writeOutputs(outDir, out, packages, { rustRev, jetRev, refs });
  if (opts.sample) writeSample(out, opts.sample);
}

function searchCandidatesByCallees(idx, r, N) {
  const score = new Map();
  for (const c of r.fp.callees) {
    const l = idx.post.get('f:' + c);
    if (!l || l.length > 400) continue;
    const idf = Math.log(N / l.length);
    for (const it of l) score.set(it, (score.get(it) ?? 0) + idf);
  }
  return [...score.entries()].sort((a, b) => b[1] - a[1]).slice(0, 8).map(([it]) => it);
}

// ---------------------------------------------------------------------------
// Work packages: ABSENT and PARTIAL rows, ~2-4h of porting each
// ---------------------------------------------------------------------------

// Rust lines per package: a careful 1:1 port runs at roughly 150-250 Rust lines an hour.
const PACKAGE_LINES = 600;
// Rust items at least this long are also measured against their whole area.
const SPREAD_LINES = 120;

function buildPackages(out) {
  // Jet target per Rust file: the file most of its ported rows point to.
  const target = new Map();
  for (const o of out) {
    if (o.cls !== 'PORTED' && o.cls !== 'PARTIAL') continue;
    const f = o.jet.replace(/:\d+$/, '');
    const t = target.get(o.r.file) ?? new Map();
    t.set(f, (t.get(f) ?? 0) + 1);
    target.set(o.r.file, t);
  }
  const targetOf = (file) => {
    const t = target.get(file);
    if (!t) return null;
    return [...t.entries()].sort((a, b) => b[1] - a[1])[0][0];
  };
  const work = out.filter((o) => (o.cls === 'ABSENT' || o.cls === 'PARTIAL') && o.basis !== 'route-census');
  // Group by area, then Rust directory, in file/line order.
  work.sort((a, b) => (a.r.area < b.r.area ? -1 : a.r.area > b.r.area ? 1 : a.r.file < b.r.file ? -1 : a.r.file > b.r.file ? 1 : a.r.line - b.r.line));
  const packages = [];
  let cur = null;
  const dirOf = (f) => f.split('/').slice(0, -1).join('/');
  for (const o of work) {
    // PARTIAL rows cost about a third of an absent row (the item exists; behavior is added).
    const cost = Math.max(3, o.r.end - o.r.line + 1) * (o.cls === 'PARTIAL' ? 0.35 : 1);
    if (!cur || cur.area !== o.r.area || (dirOf(o.r.file) !== cur.dir && cur.lines >= PACKAGE_LINES * 0.5) || cur.lines + cost > PACKAGE_LINES * 1.4) {
      cur = { id: null, area: o.r.area, dir: dirOf(o.r.file), lines: 0, absent: 0, partial: 0, files: new Map(), targets: new Map() };
      packages.push(cur);
    }
    cur.lines += cost;
    if (o.cls === 'ABSENT') cur.absent++;
    else cur.partial++;
    cur.files.set(o.r.file, (cur.files.get(o.r.file) ?? 0) + 1);
    const t = o.cls === 'PARTIAL' ? o.jet.replace(/:\d+$/, '') : targetOf(o.r.file);
    if (t) cur.targets.set(t, (cur.targets.get(t) ?? 0) + 1);
    o.pkg = cur;
  }
  // Route leaves: one package per Core module group.
  const routeGroups = new Map();
  for (const o of out) {
    if (o.basis !== 'route-census') continue;
    const g = o.routeGroup;
    if (!routeGroups.has(g)) routeGroups.set(g, []);
    routeGroups.get(g).push(o);
  }
  // Route module groups pack whole into packages, in module order.
  const routePkgs = [];
  const cost = (o) => Math.max(10, o.r.end - o.r.line + 1);
  for (const [g, list] of [...routeGroups.entries()].sort()) {
    const lines = list.reduce((s, o) => s + cost(o), 0);
    let p = routePkgs.find((x) => x.lines + lines <= PACKAGE_LINES && x.groups.length < 6);
    if (!p) routePkgs.push((p = { id: null, area: 'runtime-route', dir: '-', lines: 0, absent: 0, partial: 0, files: new Map(), targets: new Map(), groups: [] }));
    p.groups.push(g);
    for (const o of list) {
      p.lines += cost(o);
      p.absent++;
      if (o.r.file !== '-') p.files.set(o.r.file, (p.files.get(o.r.file) ?? 0) + 1);
      o.pkg = p;
    }
    p.targets.set('Compiler/JetRuntime/Source/', (p.targets.get('Compiler/JetRuntime/Source/') ?? 0) + list.length);
  }
  packages.push(...routePkgs);
  const perArea = new Map();
  for (const p of packages) {
    const n = (perArea.get(p.area) ?? 0) + 1;
    perArea.set(p.area, n);
    p.id = `${p.area}-${String(n).padStart(3, '0')}`;
  }
  return packages;
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

const cell = (v) => String(v ?? '-').replace(/[\t\r\n]+/g, ' ') || '-';
const CLASS_ORDER = ['PORTED', 'PARTIAL', 'PORTED-UNMERGED', 'DRAFT-ONLY', 'ABSENT', 'OBSOLETE'];

// Hand verdicts score the current classification: a sampled row is correct when
// its current class equals the hand-checked `correct_class`, whichever classifier
// revision drew the sample. Also reports the sampled revision's own agreement.
function spotChecks(outDir, out) {
  const dir = join(outDir, 'samples');
  const key3 = (f, l, n) => `${f}\t${l}\t${n}`;
  const current = new Map(out.map((o) => [key3(o.r.file, o.r.line, o.r.name), o.cls]));
  const res = new Map();
  let sampled = 0;
  let sampledAgree = 0;
  const files = existsSync(dir) ? readdirSync(dir).filter((f) => /^verified-spot.*\.tsv$/.test(f)).sort() : [];
  for (const f of files) {
    const ls = readFileSync(join(dir, f), 'utf8').split('\n');
    const h = ls[0].split('\t');
    const c = Object.fromEntries(h.map((x, i) => [x, i]));
    for (const l of ls.slice(1)) {
      if (!l.trim()) continue;
      const v = l.split('\t');
      const truth = v[c.correct_class];
      const cls = current.get(key3(v[c.rust_file], v[c.line], v[c.name]));
      if (!cls || !CLASS_ORDER.includes(truth)) continue;
      sampled++;
      if (v[c.verdict] === 'agree') sampledAgree++;
      const s = res.get(cls) ?? { checked: 0, agree: 0, wrong: new Map() };
      s.checked++;
      if (cls === truth) s.agree++;
      else s.wrong.set(truth, (s.wrong.get(truth) ?? 0) + 1);
      res.set(cls, s);
    }
  }
  return { files, res, sampled, sampledAgree };
}

function writeOutputs(outDir, out, packages, meta) {
  const head = ['area', 'rust_file', 'line', 'end', 'kind', 'owner', 'name', 'class', 'jet', 'score', 'rust_weight', 'basis', 'missing', 'reason', 'package'];
  let tsv = head.join('\t') + '\n';
  for (const o of out) {
    tsv += [o.r.area, o.r.file, o.r.line, o.r.end, o.r.kind, o.r.owner || '-', o.r.name, o.cls, o.jet, o.score, o.r.weight.toFixed(1), o.basis, o.missing, o.reason, o.pkg?.id ?? '-'].map(cell).join('\t') + '\n';
  }
  writeFileSync(join(outDir, 'verified.tsv'), tsv);

  const areas = [...new Set(out.map((o) => o.r.area))].sort();
  const count = (pred) => out.filter(pred).length;
  const lines = (pred) => out.filter(pred).reduce((s, o) => s + Math.max(0, o.r.end - o.r.line + 1), 0);
  let md = `# Rust exit: verified behavioral parity\n\nGenerated by \`node Tools/agent/rust-parity-fingerprint.mjs\`; do not edit. Rows: \`verified.tsv\`.\n\n`;
  md += `Rust tree \`${meta.rustRev}\`; Jet tree \`${meta.jetRev}\`; secondary Jet trees ${meta.refs.map((r) => `\`${r.name}\` (\`${r.sha.slice(0, 12)}\`)`).join(', ')}.\n\n`;
  md += `Scope: every Rust item of the compiler (jet-lexer, jet-parser, jet-sema, jet-comptime, jet-evalhost, jet-codegen, jet-driver, jet-foundation, jet-cli, \`Source/\`), the Core library (\`crates/jet-codegen/src/Prelude/\`) and the runtime (jet-rt), test files excluded, plus every live Core runtime route whose C export no Jet runtime \`#Export(c)\` defines.\n\n`;
  md += `Method: each Rust item's fingerprint (diagnostic codes, message strings, enum variants constructed, numeric literals, callee names, branch count; fields, variants and methods for types) is scored against candidate Jet items (the Core call registry's body for a Prelude symbol, the name ledger's pointer, same-name and Prelude-spelling items, and a fingerprint search over the whole Jet tree), each taken together with the Jet items it calls in its own package. Score = matched weight / Rust weight (code 5, message 3, variant 1.5, number 1, member 1, callee 0.5, single word 0.5); a fingerprint-only hit outside the area's directories whose name shares no word with the Rust name loses 0.15.\n\n`;
  md += `- \`PORTED\`: score >= 0.70 and every diagnostic code present; or, for a thin item (fingerprint weight < 2), the registry's body or a same-name item in the area's Jet directories (\`basis\` \`thin:*\`).\n- \`PARTIAL\`: the best Jet counterpart scores 0.30-0.70, misses a diagnostic code, or is the registry's body but scores lower; \`missing\` lists the absent codes, messages, variants, members, numbers and a branch collapse.\n- \`PORTED-UNMERGED\`: the best counterpart exists only on a secondary tree, in a file that tree compiled.\n- \`DRAFT-ONLY\`: the best counterpart exists only on a secondary tree, in a Compiler file outside that tree's compiler unit (\`Compiler/Bootstrap/sources.list\`) or a Jetpack CLI file: an uncompiled transliteration, not a port.\n- \`ABSENT\`: no Jet counterpart scores 0.30 (\`missing\` names the nearest miss); \`basis\` \`route-census\` marks a live Core route whose export no Jet runtime \`#Export(c)\` defines.\n- \`OBSOLETE\`: retired with Rust; \`reason\` is the ledger drop reason, the reviewed triage group rationale, or Rust std-trait glue.\n\n`;

  md += `## Counts per area\n\n| area | rows | ${CLASS_ORDER.join(' | ')} | absent Rust lines | partial Rust lines |\n|---|---|${CLASS_ORDER.map(() => '---').join('|')}|---|---|\n`;
  for (const a of [...areas, 'all']) {
    const inA = (o) => a === 'all' || o.r.area === a;
    md += `| ${a} | ${count(inA)} | ${CLASS_ORDER.map((c) => count((o) => inA(o) && o.cls === c)).join(' | ')} | ${lines((o) => inA(o) && o.cls === 'ABSENT')} | ${lines((o) => inA(o) && o.cls === 'PARTIAL')} |\n`;
  }

  md += `\n## Counts per basis\n\n| class | basis | rows |\n|---|---|---|\n`;
  const byBasis = new Map();
  for (const o of out) byBasis.set(`${o.cls}\t${o.basis}`, (byBasis.get(`${o.cls}\t${o.basis}`) ?? 0) + 1);
  for (const [k, v] of [...byBasis.entries()].sort()) md += `| ${k.split('\t').join(' | ')} | ${v} |\n`;

  const spot = spotChecks(outDir, out);
  md += `\n## Hand spot-check\n\nSamples: ${spot.files.length ? spot.files.map((f) => `\`samples/${f}\``).join(', ') : 'none'}: rows drawn per area and class (at least 5% of each class of the sampled revision), each read by hand against the Rust and Jet code; \`correct_class\` is the hand verdict. Accuracy below scores the current class of every sampled row against it (the sampled revision itself agreed on ${spot.sampledAgree} of ${spot.sampled} rows).\n\n| class | rows | checked | share | correct | accuracy | hand class when wrong |\n|---|---|---|---|---|---|---|\n`;
  for (const c of CLASS_ORDER) {
    const s = spot.res.get(c) ?? { checked: 0, agree: 0, wrong: new Map() };
    const rowsC = count((o) => o.cls === c);
    md += `| ${c} | ${rowsC} | ${s.checked} | ${rowsC ? ((100 * s.checked) / rowsC).toFixed(1) : '0'}% | ${s.agree} | ${s.checked ? ((100 * s.agree) / s.checked).toFixed(1) + '%' : '-'} | ${[...s.wrong.entries()].map(([k, v]) => `${k} ${v}`).join(', ') || '-'} |\n`;
  }

  md += `\n## Top partial ports by missing behavior\n\nRanked by Rust weight times the unmatched share.\n\n| rust item | lines | jet | score | missing |\n|---|---|---|---|---|\n`;
  const partial = out.filter((o) => o.cls === 'PARTIAL').sort((a, b) => b.r.weight * (1 - Number(b.score)) - a.r.weight * (1 - Number(a.score)));
  for (const o of partial.slice(0, 150)) md += `| \`${o.r.file}:${o.r.line}\` ${o.r.owner && o.r.owner !== '-' ? o.r.owner + '::' : ''}${o.r.name} | ${o.r.end - o.r.line + 1} | \`${o.jet}\` | ${o.score} | ${o.missing.replace(/\|/g, '\\|')} |\n`;

  md += `\n## Porting work packages\n\nEvery ABSENT and PARTIAL row belongs to one package (\`package\` column of \`verified.tsv\`). A package is about ${PACKAGE_LINES} Rust lines of porting (a PARTIAL row costs a third of its lines), about 2-4 hours. Targets are the Jet files the same Rust files' ported rows already live in.\n\n| package | absent | partial | est. Rust lines | Rust files | Jet targets |\n|---|---|---|---|---|---|\n`;
  for (const p of packages) {
    const files = [...p.files.entries()].sort((a, b) => b[1] - a[1]).map(([f, n]) => `\`${f}\` (${n})`).join(', ');
    const tg = [...p.targets.entries()].sort((a, b) => b[1] - a[1]).slice(0, 3).map(([f]) => `\`${f}\``).join(', ') || `new file under ${AREA_DIRS[p.area]?.[0] ?? 'Compiler/JetRuntime/'}`;
    md += `| ${p.id}${p.groups ? ` (${p.groups.join(', ')})` : ''} | ${p.absent} | ${p.partial} | ${Math.round(p.lines)} | ${files || '-'} | ${tg} |\n`;
  }

  md += `\n## Ported only on a secondary tree\n\nPORTED-UNMERGED and DRAFT-ONLY rows by secondary file.\n\n| secondary file | rows | Rust files |\n|---|---|---|\n`;
  const unmerged = new Map();
  for (const o of out) {
    if (o.cls !== 'PORTED-UNMERGED' && o.cls !== 'DRAFT-ONLY') continue;
    const f = o.jet.replace(/:\d+$/, '');
    const e = unmerged.get(f) ?? { n: 0, rust: new Set() };
    e.n++;
    e.rust.add(o.r.file);
    unmerged.set(f, e);
  }
  for (const [f, e] of [...unmerged.entries()].sort((a, b) => b[1].n - a[1].n)) md += `| \`${f}\` | ${e.n} | ${[...e.rust].slice(0, 4).map((x) => `\`${x}\``).join(', ')}${e.rust.size > 4 ? ` +${e.rust.size - 4}` : ''} |\n`;

  md += `\n## Obsolete reasons\n\n| reason | rows |\n|---|---|\n`;
  const reasons = new Map();
  for (const o of out) if (o.cls === 'OBSOLETE') reasons.set(o.reason.split(':')[0], (reasons.get(o.reason.split(':')[0]) ?? 0) + 1);
  for (const [k, v] of [...reasons.entries()].sort((a, b) => b[1] - a[1])) md += `| ${k} | ${v} |\n`;
  writeFileSync(join(outDir, 'VERIFIED.md'), md);
}

// Deterministic stratified sample: max(n, 5% of the stratum) per area x class.
function writeSample(out, { n, out: path }) {
  const strata = new Map();
  for (const o of out) {
    const k = `${o.r.area}\t${o.cls}`;
    if (!strata.has(k)) strata.set(k, []);
    strata.get(k).push(o);
  }
  let seed = 20261021;
  const rand = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
  let tsv = 'rust_file\tline\tend\tkind\towner\tname\tclass\tjet\tscore\tbasis\tmissing\treason\tverdict\tcorrect_class\tnote\n';
  for (const [, list] of [...strata.entries()].sort()) {
    const want = Math.min(list.length, Math.max(n, Math.ceil(list.length * 0.05)));
    const pool = list.slice();
    for (let i = 0; i < want; i++) {
      const j = i + Math.floor(rand() * (pool.length - i));
      [pool[i], pool[j]] = [pool[j], pool[i]];
      const o = pool[i];
      tsv += [o.r.file, o.r.line, o.r.end, o.r.kind, o.r.owner || '-', o.r.name, o.cls, o.jet, o.score, o.basis, o.missing, o.reason, '', '', ''].map(cell).join('\t') + '\n';
    }
  }
  writeFileSync(path, tsv);
}

main();
