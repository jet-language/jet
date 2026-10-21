#!/usr/bin/env node
// Reintegration list: every Jet definition that exists only off the dev tree.
//
// The off-dev set is recomputed from git objects with the rust-parity-ledger
// rules: a fn, struct/enum/type/trait/interface or #Test description in a secondary
// ref is off-dev when the dev tree's index lacks its name. Each row gets one verdict,
// first match wins:
//   1. covered  name-case           same normalised key (lowercase, alnum only) as a dev def name or #Test description
//   2. covered  test-desc-prefix    off-dev #Test name equals a dev #Test description minus its 1-2 leading words
//   3. covered  name-prefix-strip   ref name minus 1-2 leading `word_` segments is a dev name (remainder >= 5 chars)
//   4. covered  name-prefix-add     dev name minus 1-2 leading `word_` segments equals the ref name (remainder >= 5 chars)
//   5. covered  same-body           a dev def in the same file (same path, or same path with `_` removed and
//                                   case folded) has a body with token-level similarity >= 0.80.
//                                   Tokens = identifiers + literals; bodies under 6 tokens are not compared.
//   6. retired-on-dev               the def's file is absent on dev and dev deleted that path (or its PascalCase
//                                   variant); evidence = deleting commit `<hash> <subject>`.
//   7. missing                      otherwise; evidence = the def's first line.
//
// Area view (reintegrate-areas.tsv): for every ref file holding an off-dev def, the dev owner is
//   the same path; else the same path after package moves (PACKAGE_MOVES) with each segment
//   lowercased and `_`/`-` removed (renamed-file); else the same directory, normalised the same way;
//   else the same package (first two directory segments, normalised). status: same-file |
//   renamed-file | superseded (dir/package owner with dev_lines >= ref_lines) | partial
//   (dir/package owner smaller) | absent (dev has nothing there).
// Every dev-side path lookup (same-body files, deletions, area owners) applies PACKAGE_MOVES first.
//
// Reads git objects only (ls-tree, one cat-file --batch per tree, one git log for deletions), so the
// result depends on the audited commits alone, never on the worktree or the live dev tip.
// Writes Docs/audits/rust-exit-parity/{reintegrate.tsv,reintegrate-areas.tsv,reintegrate-summary.md,
// reintegrate-refs.json}; reintegrate-refs.json pins the audited commits.
//
// Usage: node Tools/agent/rust-parity-reintegrate.mjs [--jet-rev <commit>] [--jet-refs <ref,ref>] [--check]
//   --jet-rev   dev tree (default: refs.jet_rev in summary.json, the parity ledger's pin).
//   --jet-refs  secondary refs, resolved to commits (default: refs.jet_refs in summary.json, by recorded SHA).
//   --check     regenerate in memory at the commits pinned in reintegrate-refs.json (the flags override
//               them); print `differs: <path>` per mismatch to stderr and exit 1 if committed outputs differ.
//               Passes until the outputs are deliberately regenerated at new commits.

import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const OUT_REL = 'Docs/audits/rust-exit-parity';
const SUMMARY_REL = `${OUT_REL}/summary.json`;
const PINS = 'reintegrate-refs.json';
const PINS_REL = `${OUT_REL}/${PINS}`;
const OUTPUTS = ['reintegrate.tsv', 'reintegrate-areas.tsv', 'reintegrate-summary.md', PINS];
const JET_DIRS = ['Compiler/', 'Jetpack/', 'Core/', 'Tools/', 'Examples/', 'tests/'];
const isJetScope = (p) => p.endsWith('.jet') && JET_DIRS.some((d) => p.startsWith(d));
const SIM = 0.8;
const MIN_TOKENS = 6;
const LCS_CAP = 1500;
const PORT_MAIN = 'origin/port/main';
const PORT_BASE = '88658d237';

const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const sanitize = (s) => String(s ?? '-').replace(/[\t\r\n]+/g, ' ').trim() || '-';
const normKey = (s) => s.toLowerCase().replace(/[^a-z0-9]/g, '');
const normName = (s) => s.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '');
const pathKey = (p) => p.toLowerCase().replace(/_/g, '');
const normSeg = (s) => s.toLowerCase().replace(/[_-]/g, '');
const normPath = (p) => p.split('/').map(normSeg).join('/');
const dirOf = (p) => p.slice(0, Math.max(0, p.lastIndexOf('/')));
const pkgOf = (dir) => normPath(dir.split('/').slice(0, 2).join('/'));
const indentOf = (s) => /^\s*/.exec(s)[0].length;
const linesOf = (text) => {
  if (!text) return 0;
  const n = text.split('\n').length;
  return text.endsWith('\n') ? n - 1 : n;
};
const table = (head, rows) =>
  `| ${head.join(' | ')} |\n|${head.map(() => '---').join('|')}|\n` + rows.map((r) => `| ${r.join(' | ')} |\n`).join('');

// Packages that moved between the secondary refs and dev: ref path prefix -> dev path prefix.
const PACKAGE_MOVES = [
  ['Compiler/JetPkgModel/', 'Jetpack/PackageModel/'],
  ['Compiler/JetStore/', 'Jetpack/Store/'],
];
function devPath(p) {
  for (const [from, to] of PACKAGE_MOVES) if (p.startsWith(from)) return to + p.slice(from.length);
  return p;
}

// PascalCase variant of a path: each `_`-separated segment capitalised, `_` removed.
function pascalPath(p) {
  return p
    .split('/')
    .map((seg) => seg.split('_').map((w) => (w ? w[0].toUpperCase() + w.slice(1) : w)).join(''))
    .join('/');
}

function fail(message, code = 2) {
  process.stderr.write(`rust-parity-reintegrate: ${message}\n`);
  process.exit(code);
}

function parseArgs(argv) {
  const opts = { jetRev: null, jetRefs: null, check: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const eq = arg.indexOf('=');
    const name = eq >= 0 ? arg.slice(0, eq) : arg;
    if (name === '--jet-rev' || name === '--jet-refs') {
      const v = eq >= 0 ? arg.slice(eq + 1) : argv[++i];
      if (v === undefined) fail(`${name} needs a value`);
      if (name === '--jet-rev') opts.jetRev = v;
      else opts.jetRefs = v;
    } else if (arg === '--check') {
      opts.check = true;
    } else if (arg === '-h' || arg === '--help') {
      process.stdout.write(
        'usage: node Tools/agent/rust-parity-reintegrate.mjs [--jet-rev <commit>] [--jet-refs <ref,ref>] [--check]\n',
      );
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

// ---------------------------------------------------------------------------
// Git access (object reads only)
// ---------------------------------------------------------------------------

function git(args, options = {}) {
  return execFileSync('git', args, { cwd: ROOT, maxBuffer: Infinity, ...options });
}

function resolveCommit(name) {
  try {
    return git(['rev-parse', '--verify', '--quiet', `${name}^{commit}`]).toString('utf8').trim();
  } catch {
    return fail(`not a commit: ${name}`);
  }
}

function lsTreeFiles(rev) {
  return git(['ls-tree', '-r', '-z', '--name-only', rev]).toString('utf8').split('\0').filter(Boolean);
}

// One cat-file batch: spec (`<rev>:<path>`) -> text, or null when the object is missing.
function catFileBatch(specs) {
  const unique = [...new Set(specs)];
  const out = new Map();
  if (unique.length === 0) return out;
  const buf = git(['cat-file', '--batch'], { input: unique.join('\n') + '\n' });
  let pos = 0;
  for (const spec of unique) {
    const nl = buf.indexOf(10, pos);
    const header = buf.toString('utf8', pos, nl);
    pos = nl + 1;
    const m = / blob (\d+)$/.exec(header);
    if (!m) {
      out.set(spec, null);
      continue;
    }
    const size = Number(m[1]);
    out.set(spec, buf.toString('utf8', pos, pos + size));
    pos += size + 1;
  }
  return out;
}

// ---------------------------------------------------------------------------
// Jet lexing and definitions (rust-parity-ledger's scanJet view of a file)
// ---------------------------------------------------------------------------

// Code-only view: `//` comments and string/backtick contents become spaces, line
// shape is kept. starts[ln] is `code` when the line begins outside a string literal.
function codeLines(text) {
  const lines = text.split('\n');
  const code = new Array(lines.length);
  const starts = new Array(lines.length);
  let str = null; // open quote character while inside a literal
  for (let ln = 0; ln < lines.length; ln++) {
    let s = lines[ln];
    if (s.endsWith('\r')) s = s.slice(0, -1);
    starts[ln] = str === null ? 'code' : 'str';
    let out = '';
    let i = 0;
    while (i < s.length) {
      const c = s[i];
      if (str !== null) {
        if (str === '"' && c === '\\') {
          const w = Math.min(2, s.length - i);
          out += ' '.repeat(w);
          i += w;
          continue;
        }
        if (c === str) {
          str = null;
          out += c;
        } else {
          out += ' ';
        }
        i++;
        continue;
      }
      if (c === '/' && s[i + 1] === '/') {
        out += ' '.repeat(s.length - i);
        break;
      }
      if (c === '"' || c === '`') {
        str = c;
        out += c;
        i++;
        continue;
      }
      out += c;
      i++;
    }
    code[ln] = out.length < s.length ? out + ' '.repeat(s.length - out.length) : out;
  }
  return { code, starts };
}

// fn / type / #Test definitions of one Jet file, in line order.
function jetDefs(text) {
  const { code, starts } = codeLines(text);
  const raw = text.split('\n');
  const defs = [];
  code.forEach((c, ln) => {
    const line = ln + 1;
    for (const m of c.matchAll(/\bfn\s+([A-Za-z_]\w*)/g)) defs.push({ kind: 'fn', name: m[1], line });
    const tm = /^\s*(?:pub\s+)?(struct|enum|type|trait|interface)\s+([A-Za-z_]\w*)/.exec(c);
    if (tm) defs.push({ kind: tm[1], name: tm[2], line });
    if (starts[ln] === 'code') {
      const dm = /^\s*#Test\(\s*"((?:[^"\\]|\\.)*)"/.exec(raw[ln]);
      if (dm) defs.push({ kind: 'test', name: dm[1], line });
    }
  });
  return defs;
}

// ---------------------------------------------------------------------------
// Dev-side body matching (brace-matched defs over comment-blanked code)
// ---------------------------------------------------------------------------

// Comments and string/backtick literal contents become spaces (newlines kept).
const BLANK_RE = /\/\/[^\n]*|\/\*[\s\S]*?\*\/|"(?:[^"\\\n]|\\.)*"|`[^`]*`/g;
function blankCode(raw) {
  return raw.replace(BLANK_RE, (m) => {
    const body = m.replace(/[^\n]/g, ' ');
    if (m[0] === '"' || m[0] === '`') return m[0] + body.slice(2, -1) + m[m.length - 1];
    return body;
  });
}

const TOKEN_RE = /\/\/[^\n]*|\/\*[\s\S]*?\*\/|"(?:[^"\\\n]|\\.)*"|`[^`]*`|[A-Za-z_]\w*|\d[\w.]*/g;
const tokenInterner = new Map();
function tokenIds(text) {
  const ids = [];
  for (const m of text.matchAll(TOKEN_RE)) {
    if (m[0][0] === '/') continue;
    let id = tokenInterner.get(m[0]);
    if (id === undefined) {
      id = tokenInterner.size;
      tokenInterner.set(m[0], id);
    }
    ids.push(id);
  }
  return Int32Array.from(ids);
}

// Brace-matched extent of a def starting at code line `start` (0-based).
function extentOf(code, start) {
  const n = code.length;
  const defIndent = indentOf(code[start]);
  let depth = 0;
  let opened = false;
  let paren = 0;
  const limit = Math.min(n, start + 400);
  for (let i = start; i < limit; i++) {
    const line = code[i];
    for (let k = 0; k < line.length; k++) {
      const ch = line[k];
      if (ch === '{') {
        depth++;
        opened = true;
      } else if (ch === '}') depth--;
      else if (ch === '(') paren++;
      else if (ch === ')') paren--;
    }
    if (opened && depth <= 0) return i;
    if (!opened && paren <= 0) {
      const t = line.trimEnd();
      const continues = /(->|,|=|&&|\|\||\+|-|:|\.)$/.test(t) || t === '';
      const next = i + 1 < n ? code[i + 1] : null;
      const nextOpens = next !== null && next.trim().startsWith('{');
      const nextIsBody = next === null || (next.trim() !== '' && indentOf(next) > defIndent) || nextOpens;
      if (!continues && !nextIsBody && i >= start) return i;
      if (!continues && next !== null && next.trim() === '' && i > start) return i;
    }
  }
  return Math.max(start, limit - 1);
}

function scanFile(text) {
  const raw = text.split('\n');
  const code = blankCode(text).split('\n');
  const defs = [];
  for (let ln = 0; ln < code.length; ln++) {
    const c = code[ln];
    for (const m of c.matchAll(/\bfn\s+([A-Za-z_]\w*)/g)) defs.push({ kind: 'fn', name: m[1], start: ln });
    const tm = /^\s*(?:pub\s+)?(struct|enum|type|trait|interface)\s+([A-Za-z_]\w*)/.exec(c);
    if (tm) defs.push({ kind: tm[1], name: tm[2], start: ln });
    if (/^\s*#Test\(/.test(c)) {
      const dm = /^\s*#Test\(\s*"((?:[^"\\]|\\.)*)"/.exec(raw[ln]);
      if (dm) defs.push({ kind: 'test', name: dm[1], start: ln });
    }
  }
  for (const d of defs) {
    d.end = extentOf(code, d.start);
    d.line = d.start + 1;
    d.firstLine = raw[d.start].trim();
    d.bodyText = raw.slice(d.start, d.end + 1).join('\n');
    d.tokens = null;
    d.bag = null;
  }
  return defs;
}

function tokensOf(def) {
  if (!def.tokens) def.tokens = tokenIds(def.bodyText);
  return def.tokens;
}

function bagOf(def) {
  if (!def.bag) {
    const bag = new Map();
    for (const t of tokensOf(def)) bag.set(t, (bag.get(t) ?? 0) + 1);
    def.bag = bag;
  }
  return def.bag;
}

function lcs(a, b) {
  const m = b.length;
  let prev = new Int32Array(m + 1);
  let cur = new Int32Array(m + 1);
  for (let i = 1; i <= a.length; i++) {
    cur[0] = 0;
    const ai = a[i - 1];
    for (let j = 1; j <= m; j++) {
      cur[j] = ai === b[j - 1] ? prev[j - 1] + 1 : prev[j] > cur[j - 1] ? prev[j] : cur[j - 1];
    }
    [prev, cur] = [cur, prev];
  }
  return prev[m];
}

// Similarity 2*LCS/(n+m) over token ids, with a bag-intersection upper bound first.
function similarity(aTokens, aBag, bTokens, bBag) {
  const n = aTokens.length;
  const m = bTokens.length;
  if (n < MIN_TOKENS || m < MIN_TOKENS) return 0;
  let bag = 0;
  const small = aBag.size <= bBag.size ? aBag : bBag;
  const large = small === aBag ? bBag : aBag;
  for (const [t, c] of small) {
    const d = large.get(t);
    if (d) bag += Math.min(c, d);
  }
  if ((2 * bag) / (n + m) < SIM) return 0;
  const a = n > LCS_CAP ? aTokens.subarray(0, LCS_CAP) : aTokens;
  const b = m > LCS_CAP ? bTokens.subarray(0, LCS_CAP) : bTokens;
  return (2 * lcs(a, b)) / (a.length + b.length);
}

// Remainders of a name minus 1-2 leading `_` segments; each keeps >= 3 segments.
function stripPrefixes(name) {
  const segs = name.split('_');
  const out = [];
  for (let i = 1; i <= 2 && segs.length - i >= 3; i++) out.push(segs.slice(i).join('_'));
  return out;
}

// Dev description minus 1-2 leading words (package words), as normalised keys.
function stripDescWords(desc) {
  const words = desc.trim().split(/\s+/);
  const out = [];
  for (let i = 1; i <= 2 && words.length - i >= 1; i++) out.push(words.slice(i).join(' '));
  return out;
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

function generate({ jetRev, refs }) {
  const jetPaths = lsTreeFiles(jetRev).filter(isJetScope).sort(cmp);
  const jetBlobs = catFileBatch(jetPaths.map((p) => `${jetRev}:${p}`));
  const jetText = (p) => jetBlobs.get(`${jetRev}:${p}`);

  // Dev index for the off-dev test: the names rule (ledger's scanJet index).
  const devFns = new Set();
  const devTypes = new Set();
  const devTestDescs = new Set();
  const devTestFns = new Set();
  for (const p of jetPaths) {
    const testsDir = /(^|\/)[Tt]ests?\//.test(p);
    for (const d of jetDefs(jetText(p))) {
      if (d.kind === 'fn') {
        devFns.add(d.name);
        if (testsDir) devTestFns.add(normName(d.name.replace(/^test_/, '')));
      } else if (d.kind === 'test') {
        devTestDescs.add(normName(d.name));
      } else {
        devTypes.add(d.name);
      }
    }
  }
  const devHas = (d) => {
    if (d.kind === 'fn') return devFns.has(d.name);
    if (d.kind === 'test') {
      const k = normName(d.name);
      return devTestDescs.has(k) || devTestFns.has(k) || devTestFns.has(normName(d.name.replace(/^test_/, '')));
    }
    return devTypes.has(d.name);
  };

  // Off-dev definitions, recomputed from each secondary ref's tree.
  const refBlobs = new Map(); // `${ref}\t${file}` -> text
  const offRows = [];
  for (const ref of refs) {
    const paths = lsTreeFiles(ref.sha).filter(isJetScope).sort(cmp);
    const blobs = catFileBatch(paths.map((p) => `${ref.sha}:${p}`));
    for (const p of paths) {
      const text = blobs.get(`${ref.sha}:${p}`);
      refBlobs.set(`${ref.name}\t${p}`, text);
      for (const d of jetDefs(text)) {
        if (!devHas(d)) offRows.push({ ref: ref.name, file: p, line: d.line, kind: d.kind, name: d.name });
      }
    }
  }
  offRows.sort((a, b) => cmp(a.ref, b.ref) || cmp(a.file, b.file) || a.line - b.line || cmp(a.name, b.name));

  // Dev index for classification.
  const devNameKeys = new Map(); // key -> { loc, name }
  const devTestKeys = new Map(); // key(test desc) -> loc
  const devTestStripped = new Map(); // key(desc minus 1-2 words) -> { loc, name }
  const devStripped = new Map(); // key(name minus 1-2 leading segments) -> loc
  const devByPathKey = new Map(); // pathKey -> [path]
  const devDefs = new Map(); // path -> defs
  const devLines = new Map(); // path -> line count
  for (const p of jetPaths) {
    const text = jetText(p);
    const defs = scanFile(text);
    devDefs.set(p, defs);
    devLines.set(p, linesOf(text));
    const pk = pathKey(p);
    if (!devByPathKey.has(pk)) devByPathKey.set(pk, []);
    devByPathKey.get(pk).push(p);
    for (const d of defs) {
      const loc = `${p}:${d.line}`;
      const key = normKey(d.name);
      if (key && !devNameKeys.has(key)) devNameKeys.set(key, { loc, name: d.name });
      if (d.kind === 'test') {
        if (key && !devTestKeys.has(key)) devTestKeys.set(key, loc);
        for (const rest of stripDescWords(d.name)) {
          const k = normKey(rest);
          if (k.length >= 5 && !devTestStripped.has(k)) devTestStripped.set(k, { loc, name: d.name });
        }
      } else if (d.name.includes('_')) {
        for (const rest of stripPrefixes(d.name)) {
          const k = normKey(rest);
          if (k.length >= 5 && !devStripped.has(k)) devStripped.set(k, loc);
        }
      }
    }
  }

  // Ref defs (scanned lazily per ref file).
  const refScan = new Map();
  const refDefAt = (ref, file, line) => {
    const k = `${ref}\t${file}`;
    if (!refScan.has(k)) refScan.set(k, scanFile(refBlobs.get(k)));
    return refScan.get(k).find((d) => d.line === line) ?? null;
  };

  // Deletions on the dev tree: one git log for every dev-absent path (and its PascalCase variant).
  const absentPaths = new Set();
  for (const r of offRows) {
    const p = devPath(r.file);
    if (!devByPathKey.has(pathKey(p))) {
      absentPaths.add(p);
      absentPaths.add(pascalPath(p));
    }
  }
  const deletions = new Map(); // path -> `${hash} ${subject}`
  if (absentPaths.size) {
    const log = git([
      '--literal-pathspecs',
      'log',
      '--no-renames',
      '--diff-filter=D',
      '--name-only',
      '--abbrev=12',
      '--format=@@%h%x09%s',
      jetRev,
      '--',
      ...absentPaths,
    ]).toString('utf8');
    let current = null;
    for (const line of log.split('\n')) {
      if (line.startsWith('@@')) {
        const tab = line.indexOf('\t');
        current = `${line.slice(2, tab)} ${line.slice(tab + 1)}`;
      } else if (line.trim() && current) {
        if (!deletions.has(line)) deletions.set(line, current);
      }
    }
  }

  // Classify.
  const results = [];
  for (const r of offRows) {
    const out = { ...r, verdict: 'missing', dev: '-', evidence: '' };
    const key = normKey(r.name);
    const refDef = refDefAt(r.ref, r.file, r.line);
    const refText = refBlobs.get(`${r.ref}\t${r.file}`);
    const firstLine = refDef ? refDef.firstLine : refText == null ? '(file absent on ref)' : '-';

    // 1. case-variant / exact name (a #Test description counts as a dev name)
    const hit = devNameKeys.get(key);
    if (hit) {
      let how = 'name-case';
      if (r.kind === 'test' && devTestKeys.has(key)) how = 'test-desc';
      else if (hit.name === r.name) how = 'name-exact';
      Object.assign(out, { verdict: 'covered', dev: `${hit.loc} ${hit.name}`, evidence: how });
      results.push(out);
      continue;
    }

    // 2. off-dev #Test name equals a dev #Test description without its package words
    if (r.kind === 'test' && devTestStripped.has(key)) {
      const h = devTestStripped.get(key);
      Object.assign(out, { verdict: 'covered', dev: `${h.loc} ${h.name}`, evidence: 'test-desc-prefix' });
      results.push(out);
      continue;
    }

    // 3. ref name carries a package prefix dev lacks
    let found = null;
    if (r.name.includes('_')) {
      for (const rest of stripPrefixes(r.name)) {
        const k = normKey(rest);
        if (k.length >= 5 && devNameKeys.has(k)) {
          found = { how: 'name-prefix-strip', loc: `${devNameKeys.get(k).loc} ${devNameKeys.get(k).name}` };
          break;
        }
      }
    }
    // 4. dev name carries a package prefix the ref lacks
    if (!found && devStripped.has(key)) found = { how: 'name-prefix-add', loc: devStripped.get(key) };
    if (found) {
      Object.assign(out, { verdict: 'covered', dev: found.loc, evidence: found.how });
      results.push(out);
      continue;
    }

    // 5. same file (same path, or path-normalised) with an 80%-similar body
    const movedFile = devPath(r.file);
    const candPaths = devByPathKey.get(pathKey(movedFile)) ?? [];
    let sameBody = null;
    if (refDef && candPaths.length) {
      const rt = tokenIds(refDef.bodyText);
      if (rt.length >= MIN_TOKENS) {
        const rb = new Map();
        for (const t of rt) rb.set(t, (rb.get(t) ?? 0) + 1);
        let best = 0;
        for (const p of candPaths) {
          for (const d of devDefs.get(p) ?? []) {
            const dt = tokensOf(d);
            if (dt.length < MIN_TOKENS) continue;
            const s = similarity(rt, rb, dt, bagOf(d));
            if (s >= SIM && s > best) {
              best = s;
              sameBody = { loc: `${p}:${d.line}`, name: d.name, s };
            }
          }
        }
      }
    }
    if (sameBody) {
      Object.assign(out, {
        verdict: 'covered',
        dev: `${sameBody.loc} ${sameBody.name}`,
        evidence: `same-body ${sameBody.s.toFixed(3)}`,
      });
      results.push(out);
      continue;
    }

    // 6. file deliberately deleted on dev
    if (candPaths.length === 0) {
      const del = deletions.get(movedFile) ?? deletions.get(pascalPath(movedFile));
      if (del) {
        Object.assign(out, { verdict: 'retired-on-dev', dev: '-', evidence: del });
        results.push(out);
        continue;
      }
    }

    // 7. missing
    out.evidence = firstLine;
    results.push(out);
  }

  const tsv =
    ['ref', 'file', 'line', 'kind', 'name', 'verdict', 'dev_equivalent', 'evidence'].join('\t') +
    '\n' +
    results
      .map((o) =>
        [o.ref, o.file, o.line, o.kind, o.name, o.verdict, sanitize(o.dev), sanitize(o.evidence)].map(sanitize).join('\t'),
      )
      .map((l) => l + '\n')
      .join('');

  // Area view: one record per ref file holding an off-dev def.
  const devSet = new Set(jetPaths);
  const devByNormPath = new Map();
  const devByDir = new Map();
  const devByPkg = new Map();
  const push = (m, k, v) => {
    if (!m.has(k)) m.set(k, []);
    m.get(k).push(v);
  };
  for (const p of jetPaths) {
    push(devByNormPath, normPath(p), p);
    push(devByDir, normPath(dirOf(p)), p);
    push(devByPkg, pkgOf(dirOf(p)), p);
  }
  const sumDev = (paths) => paths.reduce((n, p) => n + devLines.get(p), 0);

  const groups = new Map(); // `${ref}\t${file}` -> { ref, file, missing, covered }
  for (const o of results) {
    const k = `${o.ref}\t${o.file}`;
    if (!groups.has(k)) groups.set(k, { ref: o.ref, file: o.file, missing: 0, covered: 0 });
    const g = groups.get(k);
    if (o.verdict === 'missing') g.missing++;
    else if (o.verdict === 'covered') g.covered++;
  }
  const areas = [...groups.values()]
    .map((g) => {
      const refLines = linesOf(refBlobs.get(`${g.ref}\t${g.file}`));
      let owners = [];
      let scope = 'none';
      let status;
      const file = devPath(g.file);
      if (devSet.has(file)) {
        owners = [file];
        scope = 'file';
        status = file === g.file ? 'same-file' : 'renamed-file';
      } else if (devByNormPath.has(normPath(file))) {
        owners = devByNormPath.get(normPath(file));
        scope = 'file';
        status = 'renamed-file';
      } else {
        const dir = dirOf(file);
        if (devByDir.has(normPath(dir))) {
          owners = devByDir.get(normPath(dir));
          scope = 'dir';
        } else if (devByPkg.has(pkgOf(dir))) {
          owners = devByPkg.get(pkgOf(dir));
          scope = 'pkg';
        }
        if (owners.length === 0) status = 'absent';
        else status = sumDev(owners) >= refLines ? 'superseded' : 'partial';
      }
      const devDirs = [...new Set(owners.map(dirOf))].sort(cmp);
      const devOwner =
        scope === 'file' ? owners.join(',') : scope === 'none' ? '-' : `${scope}:${devDirs.join(',')}`;
      return {
        ref: g.ref,
        file: g.file,
        refLines,
        devOwner,
        owners,
        devLines: sumDev(owners),
        missing: g.missing,
        covered: g.covered,
        status,
      };
    })
    .sort((a, b) => cmp(a.ref, b.ref) || cmp(a.file, b.file));

  const areasTsv =
    ['ref', 'ref_file', 'ref_lines', 'dev_owner', 'dev_lines', 'missing_defs', 'covered_defs', 'status'].join('\t') +
    '\n' +
    areas
      .map((a) =>
        [a.ref, a.file, a.refLines, a.devOwner, a.devLines, a.missing, a.covered, a.status].map(sanitize).join('\t') + '\n',
      )
      .join('');

  // Directory aggregate (per ref x directory of its ref files).
  const dirMap = new Map();
  for (const a of areas) {
    const dir = dirOf(a.file);
    const k = `${a.ref}\t${dir}`;
    if (!dirMap.has(k)) {
      dirMap.set(k, { ref: a.ref, dir, files: 0, refLines: 0, owners: new Set(), missing: 0, covered: 0, statusLines: new Map() });
    }
    const d = dirMap.get(k);
    d.files++;
    d.refLines += a.refLines;
    for (const p of a.owners) d.owners.add(p);
    d.missing += a.missing;
    d.covered += a.covered;
    d.statusLines.set(a.status, (d.statusLines.get(a.status) ?? 0) + a.refLines);
  }
  const dirs = [...dirMap.values()]
    .map((d) => ({
      ...d,
      devLines: sumDev([...d.owners]),
      dominant: [...d.statusLines].sort((x, y) => y[1] - x[1] || cmp(x[0], y[0]))[0][0],
    }))
    .sort((a, b) => b.missing - a.missing || cmp(a.ref, b.ref) || cmp(a.dir, b.dir));

  // Summary.
  const verdictOf = (ref) => results.filter((o) => o.ref === ref);
  const verdicts = (ref) => {
    const rows = verdictOf(ref);
    const c = (v) => rows.filter((o) => o.verdict === v).length;
    return [rows.length, c('covered'), c('retired-on-dev'), c('missing')];
  };
  const evidenceKey = (o) => (o.verdict === 'covered' ? o.evidence.split(' ')[0] : o.verdict === 'missing' ? 'missing' : 'retired-on-dev');
  const evidenceKeys = [...new Set(results.map(evidenceKey))].sort(cmp);
  const STATUSES = ['same-file', 'renamed-file', 'superseded', 'partial', 'absent'];
  const statusCount = (ref, s, field) =>
    areas.filter((a) => a.ref === ref && a.status === s).reduce((n, a) => n + (field === 'files' ? 1 : a.missing), 0);

  const portSha = refs.find((r) => r.name === PORT_MAIN)?.sha;
  let portNote = `\`${PORT_MAIN}\` is not among the secondary refs.\n`;
  if (portSha) {
    let base = null;
    try {
      base = resolveCommit(PORT_BASE);
    } catch {
      base = null;
    }
    let merged = false;
    try {
      git(['merge-base', '--is-ancestor', portSha, jetRev]);
      merged = true;
    } catch {
      merged = false;
    }
    const log = base
      ? git(['log', '--oneline', '--no-decorate', '--abbrev=12', `${PORT_BASE}..${portSha}`]).toString('utf8').trimEnd()
      : `(${PORT_BASE} is not in this repository)`;
    portNote =
      `\`${PORT_MAIN}\` (\`${portSha}\`) is ${merged ? '' : 'not '}an ancestor of the dev tree \`${jetRev}\`. ` +
      `\`git log --oneline ${PORT_BASE}..${portSha}\`:\n\n` +
      '```\n' +
      log +
      '\n```\n';
  }

  const md = [];
  md.push('# Rust exit parity: reintegration list\n');
  md.push('Generated by `node Tools/agent/rust-parity-reintegrate.mjs`; do not edit. `--check` regenerates it.\n');
  md.push(`Dev tree: \`${jetRev}\`. Secondary refs: ${refs.map((r) => `\`${r.name}\` (\`${r.sha}\`)`).join(', ') || 'none'}.\n`);
  md.push(
    'Off-dev definitions: Jet fn, struct/enum/type/trait/interface and #Test definitions of a secondary ref whose name the dev tree lacks, recomputed from git objects. `reintegrate.tsv` has one row per definition; `reintegrate-areas.tsv` has one row per ref file holding one.\n',
  );
  md.push(
    [
      'Verdicts, first match wins: `name-case`, `test-desc` (name or #Test description on dev), `test-desc-prefix` (off-dev #Test name equals a dev #Test description minus 1-2 leading words), `name-prefix-strip` / `name-prefix-add` (1-2 leading `word_` segments, remainder >= 3 segments and >= 5 chars), `same-body` (token LCS similarity >= 0.80 with a same-file dev def), `retired-on-dev` (file deleted on dev), `missing`.\n',
      `Package moves applied to every dev-side path lookup: ${PACKAGE_MOVES.map(([a, b]) => `\`${a}\` -> \`${b}\``).join(', ')}.\n`,
      'Area status: `same-file`; `renamed-file` (same path after package moves, lowercasing segments and removing `_`/`-`); `superseded` (dev owner in the same directory, or else the same package, has at least the ref file\'s lines); `partial` (owner smaller); `absent` (dev has nothing in that directory or package). `covered_defs` counts `covered` verdicts only; `retired-on-dev` defs appear in neither count.\n',
    ].join('\n'),
  );

  md.push('## Verdicts per ref\n');
  md.push(
    table(
      ['ref', 'definitions', 'covered', 'retired-on-dev', 'missing'],
      refs.map((r) => [`\`${r.name}\``, ...verdicts(r.name).map(String)]),
    ),
  );
  md.push('## Verdict evidence per ref\n');
  md.push(
    table(
      ['evidence', ...refs.map((r) => `\`${r.name}\``)],
      evidenceKeys.map((k) => [
        `\`${k}\``,
        ...refs.map((r) => String(verdictOf(r.name).filter((o) => evidenceKey(o) === k).length)),
      ]),
    ),
  );
  md.push('## Area status per ref (ref files)\n');
  md.push(
    table(
      ['status', ...refs.map((r) => `\`${r.name}\``)],
      STATUSES.map((s) => [s, ...refs.map((r) => String(statusCount(r.name, s, 'files')))]),
    ),
  );
  md.push('## Area status per ref (missing defs in those files)\n');
  md.push(
    table(
      ['status', ...refs.map((r) => `\`${r.name}\``)],
      STATUSES.map((s) => [s, ...refs.map((r) => String(statusCount(r.name, s, 'missing')))]),
    ),
  );
  md.push('## Directories\n');
  md.push(
    'Every directory of a ref file holding an off-dev def. `ref lines` and `dev lines` sum the ref files and the distinct dev owner files; `dominant` is the status holding the most ref lines. Sorted by missing defs.\n',
  );
  md.push(
    table(
      ['ref', 'directory', 'ref files', 'ref lines', 'dev lines', 'missing defs', 'covered defs', 'dominant status'],
      dirs.map((d) => [
        `\`${d.ref}\``,
        `\`${d.dir}\``,
        String(d.files),
        String(d.refLines),
        String(d.devLines),
        String(d.missing),
        String(d.covered),
        d.dominant,
      ]),
    ),
  );
  const absent = dirs.filter((d) => d.dominant === 'absent');
  md.push('## Absent directories\n');
  md.push(
    `${absent.length} directories are dominated by \`absent\` files (dev has nothing in the directory or its package):\n`,
  );
  md.push(
    absent.map((d) => `- \`${d.ref}\` \`${d.dir}\`: ${d.refLines} ref lines, ${d.missing} missing defs`).join('\n') + '\n',
  );
  md.push('## port/main\n');
  md.push(portNote);

  return {
    'reintegrate.tsv': tsv,
    'reintegrate-areas.tsv': areasTsv,
    'reintegrate-summary.md': md.join('\n'),
    [PINS]: JSON.stringify({ jet_rev: jetRev, jet_refs: refs.map((r) => ({ name: r.name, sha: r.sha })) }, null, 2) + '\n',
  };
}

function refList(opts, recorded) {
  if (opts.jetRefs !== null) {
    return opts.jetRefs
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
      .map((name) => ({ name, sha: resolveCommit(name) }));
  }
  return (recorded?.jet_refs ?? []).map((r) => ({ name: r.name, sha: resolveCommit(r.sha) }));
}

function readJson(rel) {
  try {
    return JSON.parse(readFileSync(join(ROOT, rel), 'utf8'));
  } catch {
    return null;
  }
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  // --check recomputes at the commits these outputs pinned; a regeneration starts from the ledger's pins.
  const source = opts.check ? PINS_REL : SUMMARY_REL;
  const recorded = opts.check ? readJson(PINS_REL) : (readJson(SUMMARY_REL)?.refs ?? null);
  if (opts.jetRev === null && !recorded?.jet_rev) fail(`no recorded jet_rev in ${source}; pass --jet-rev`);
  if (opts.jetRefs === null && !recorded?.jet_refs) fail(`no recorded jet_refs in ${source}; pass --jet-refs`);
  const jetRev = resolveCommit(opts.jetRev ?? recorded.jet_rev);
  const refs = refList(opts, recorded);
  const pinned = `jet ${jetRev}; ${refs.map((r) => `${r.name} ${r.sha}`).join(', ') || 'no secondary refs'}`;

  const outputs = generate({ jetRev, refs });
  const outDir = join(ROOT, OUT_REL);

  if (opts.check) {
    const differing = [];
    for (const name of OUTPUTS) {
      let committed = null;
      try {
        committed = readFileSync(join(outDir, name), 'utf8');
      } catch {
        committed = null;
      }
      if (committed !== outputs[name]) differing.push(`${OUT_REL}/${name}`);
    }
    if (differing.length > 0) {
      for (const f of differing) process.stderr.write(`differs: ${f}\n`);
      process.stderr.write('rust-parity-reintegrate: committed outputs are stale; rerun without --check\n');
      process.exit(1);
    }
    process.stdout.write(`rust-parity-reintegrate: up to date (${pinned})\n`);
    return;
  }

  mkdirSync(outDir, { recursive: true });
  for (const name of OUTPUTS) writeFileSync(join(outDir, name), outputs[name]);
  process.stdout.write(`rust-parity-reintegrate: wrote ${OUT_REL}/ (${pinned})\n`);
}

main();
