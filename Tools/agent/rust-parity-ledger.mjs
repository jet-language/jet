#!/usr/bin/env node
// Rust-exit parity ledger.
//
// Maps every Rust item and #[test] in the Rust tree pinned at --rev to its Jet
// counterpart in the Jet tree pinned at --jet-rev, and censuses Jet CLI
// refusals. Both sides are read from git objects, so the output is independent
// of the working tree and keeps working after the Rust tree is deleted.
//
// Secondary Jet trees (--jet-refs, default origin/port/main,cutover-ready) are
// indexed too. A name found only there is reported as not-ported with evidence
// `only-on:<ref>:<rule>`, and every Jet definition found only there is listed in
// offdev.tsv.
//
// Writes Docs/audits/rust-exit-parity/{items.tsv,tests.tsv,cli.tsv,offdev.tsv,
// summary.json,SUMMARY.md}; SUMMARY.md is the owner summary. It also reports the
// hand-checked matcher samples listed in Docs/audits/rust-exit-parity/samples/rounds.tsv
// and applies the reviewed triage of every unmatched row (triage.tsv, with group
// rationales in triage-groups.tsv), all read from the working tree.
//
// Usage: node Tools/agent/rust-parity-ledger.mjs --rev <commit> [--jet-rev <commit>]
//          [--jet-refs <ref,ref>] [--check]
//   --jet-rev   Jet tree to read (default: HEAD at run time).
//   --jet-refs  secondary Jet trees; empty string disables them.
//   --check     regenerate in memory at the commits the committed summary.json
//               records (unless given explicitly); print `differs: <path>` per
//               mismatch to stderr and exit 1 if committed outputs differ or the
//               triage leaves a row unclassified, cites a missing Jet file,
//               has a row matching nothing or a group without a rationale.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const OUT_REL = 'Docs/audits/rust-exit-parity';
const OUTPUTS = ['items.tsv', 'tests.tsv', 'cli.tsv', 'offdev.tsv', 'summary.json', 'SUMMARY.md'];
const MAPPING_PATH = 'Compiler/JetParser/Tests/RustTestMapping.tsv';
const RETIRED_PATH = 'Tools/testkit/retired-with-rust.tsv';
const TRIAGE_FILE = 'triage.tsv';
const GROUPS_FILE = 'triage-groups.tsv';
const CLASSES = ['covered', 'missing', 'obsolete'];

const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

function fail(message, code = 2) {
  process.stderr.write(`rust-parity-ledger: ${message}\n`);
  process.exit(code);
}

function parseArgs(argv) {
  const opts = { rev: null, jetRev: null, jetRefs: null, check: false };
  const value = (arg, i, name) => {
    if (arg === name) {
      const v = argv[i + 1];
      if (v === undefined) fail(`${name} needs a value`);
      return { v, skip: 1 };
    }
    if (arg.startsWith(name + '=')) return { v: arg.slice(name.length + 1), skip: 0 };
    return null;
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    let hit;
    if ((hit = value(arg, i, '--rev'))) {
      opts.rev = hit.v;
      i += hit.skip;
    } else if ((hit = value(arg, i, '--jet-rev'))) {
      opts.jetRev = hit.v;
      i += hit.skip;
    } else if ((hit = value(arg, i, '--jet-refs'))) {
      opts.jetRefs = hit.v;
      i += hit.skip;
    } else if (arg === '--check') {
      opts.check = true;
    } else if (arg === '-h' || arg === '--help') {
      process.stdout.write(
        'usage: node Tools/agent/rust-parity-ledger.mjs --rev <commit> [--jet-rev <commit>] [--jet-refs <ref,ref>] [--check]\n',
      );
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!opts.rev && !opts.check) fail('--rev <commit> is required');
  return opts;
}

// ---------------------------------------------------------------------------
// Git access (all reads go through git objects)
// ---------------------------------------------------------------------------

function gitList(args) {
  return execFileSync('git', args, { cwd: ROOT, maxBuffer: Infinity })
    .toString('utf8')
    .split('\0')
    .filter(Boolean);
}

// `git ls-tree -r` entries: { type, sha, path }.
function lsTreeEntries(rev) {
  return gitList(['ls-tree', '-r', '-z', rev]).map((entry) => {
    const tab = entry.indexOf('\t');
    const [, type, sha] = entry.slice(0, tab).split(' ');
    return { type, sha, path: entry.slice(tab + 1) };
  });
}

// Git's empty blob; identical empty files must not count as relocations.
const EMPTY_BLOB = 'e69de29bb2d1d6434b8b29ae775ad8c2e48c5391';

// .rs files outside the Rust scope (Source/, crates/, tests/*.rs).
function isOutsideRust(p) {
  return p.endsWith('.rs') && !p.startsWith('Source/') && !p.startsWith('crates/') && !/^tests\/[^/]+\.rs$/.test(p);
}

function resolveCommit(name) {
  try {
    return execFileSync('git', ['rev-parse', '--verify', '--quiet', `${name}^{commit}`], { cwd: ROOT })
      .toString('utf8')
      .trim();
  } catch {
    return fail(`not a commit: ${name}`);
  }
}

// Streams blobs for `specs` (`<rev>:<path>`) through one cat-file batch and
// calls cb(index, text) for each, in order.
function eachBlob(specs, cb) {
  if (specs.length === 0) return;
  const blobs = execFileSync('git', ['cat-file', '--batch'], {
    cwd: ROOT,
    maxBuffer: Infinity,
    input: specs.join('\n') + '\n',
  });
  let pos = 0;
  specs.forEach((spec, i) => {
    const nl = blobs.indexOf(0x0a, pos);
    const header = blobs.toString('utf8', pos, nl);
    const m = / blob (\d+)$/.exec(header);
    if (!m) fail(`git cat-file failed for ${spec}: ${header}`);
    const size = Number(m[1]);
    cb(i, blobs.toString('utf8', nl + 1, nl + 1 + size));
    pos = nl + 1 + size + 1;
  });
}

function readBlobText(rev, path) {
  try {
    return execFileSync('git', ['show', `${rev}:${path}`], { cwd: ROOT, maxBuffer: Infinity }).toString('utf8');
  } catch {
    return fail(`${path} is not present at ${rev}`);
  }
}

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

function isRustScope(p) {
  if (!p.endsWith('.rs')) return false;
  if (p === 'build.rs') return true;
  if (
    p.startsWith('Source/') ||
    p.startsWith('crates/') ||
    p.startsWith('Compiler/Bootstrap/') ||
    p.startsWith('Tools/stage0-repack/')
  ) {
    return true;
  }
  return p.startsWith('tests/') && !/^tests\/(cases|compiled_workloads|compiler-proof|fixtures)\//.test(p);
}

const JET_DIRS = ['Compiler/', 'Jetpack/', 'Core/', 'Tools/', 'Examples/', 'tests/'];
const isJetScope = (p) => p.endsWith('.jet') && JET_DIRS.some((d) => p.startsWith(d));

// Rust paths whose items are dropped from the parity count.
function rustEmitPath(p) {
  return (
    /^crates\/jet-codegen\/src\/Codegen\/(?:MIRRust|Rust)[^/]*\.rs$/.test(p) ||
    p.startsWith('crates/jet-rt/') ||
    p.startsWith('crates/jet-codegen/src/Prelude/') ||
    /rust_emit|RustEmit|emit_rust/.test(p)
  );
}

function rustOnlyGluePath(p) {
  return p === 'build.rs' || /(^|\/)build\.rs$/.test(p) || p.startsWith('Tools/stage0-repack/') || p.startsWith('Compiler/Bootstrap/');
}

const STD_TRAITS = new Set([
  'Display', 'Debug', 'Clone', 'Default', 'PartialEq', 'Eq', 'Hash', 'PartialOrd', 'Ord',
  'From', 'Into', 'TryFrom', 'Drop', 'Deref', 'DerefMut', 'AsRef', 'Iterator', 'IntoIterator',
  'FromStr', 'Error', 'Serialize', 'Deserialize',
]);
const isStdTrait = (t) => STD_TRAITS.has(t) || /^Fn(Mut|Once)?$/.test(t);

// Generic names that never count as a name-exact match (rule 3 falls through).
const STOPLIST = new Set([
  'new', 'default', 'fmt', 'from', 'into', 'len', 'is_empty', 'get', 'set', 'push', 'pop', 'run',
  'main', 'parse', 'build', 'clone', 'eq', 'hash', 'drop', 'next', 'iter', 'as_str', 'to_string',
  'write', 'read', 'open', 'close', 'name', 'kind', 'value', 'text', 'span', 'apply', 'check', 'init',
  'reset', 'clear', 'insert', 'remove', 'contains', 'update', 'finish', 'start', 'emit', 'visit',
  'lower', 'render', 'process', 'handle',
]);

const FN_LIKE = new Set(['fn', 'method', 'macro']);
const TYPE_LIKE = new Set(['struct', 'enum', 'union', 'trait', 'type']);

// ---------------------------------------------------------------------------
// Lexer: strips comments and string/char contents while preserving line and
// column shape (every code line has the same length as its source line).
// ---------------------------------------------------------------------------

export function lex(text, rust) {
  const lines = text.split('\n');
  const n = lines.length;
  const code = new Array(n);
  const comments = new Array(n).fill(null);
  const starts = new Array(n);
  const lits = [];
  let mode = 'code'; // code | block | str | raw | triple
  // Jet `"..{expr}.."` interpolations: one frame per open `{`, holding its nested brace depth.
  const interp = [];
  let quote = '"';
  let escapes = true;
  let blockDepth = 0;
  let rawHashes = 0;
  let lit = null;
  let litLine = 0;

  for (let ln = 0; ln < n; ln++) {
    let s = lines[ln];
    if (s.endsWith('\r')) s = s.slice(0, -1);
    starts[ln] = mode;
    const len = s.length;
    let out = '';
    let i = 0;
    while (i < len) {
      const c = s[i];
      if (mode === 'block') {
        if (c === '/' && s[i + 1] === '*') {
          blockDepth++;
          out += '  ';
          i += 2;
        } else if (c === '*' && s[i + 1] === '/') {
          blockDepth--;
          out += '  ';
          i += 2;
          if (blockDepth === 0) mode = 'code';
        } else {
          out += ' ';
          i++;
        }
        continue;
      }
      if (mode === 'triple') {
        if (s.startsWith('"""', i)) {
          mode = 'code';
          out += '"  ';
          i += 3;
        } else {
          out += ' ';
          i++;
        }
        continue;
      }
      if (mode === 'str' || mode === 'raw') {
        if (mode === 'str' && escapes && c === '\\') {
          const pair = s.slice(i, i + 2);
          if (lit !== null) lit += pair;
          out += ' '.repeat(pair.length);
          i += pair.length;
          continue;
        }
        if (!rust && mode === 'str' && quote === '"' && c === '{') {
          if (s[i + 1] === '{') {
            if (lit !== null) lit += '{{';
            out += '  ';
            i += 2;
          } else {
            interp.push(0);
            mode = 'code';
            out += ' ';
            i++;
          }
          continue;
        }
        const closes = mode === 'raw' ? c === '"' && s.startsWith('#'.repeat(rawHashes), i + 1) : c === quote;
        if (closes) {
          const w = mode === 'raw' ? 1 + rawHashes : 1;
          if (lit !== null) lits.push({ line: litLine, text: lit });
          lit = null;
          mode = 'code';
          out += c + ' '.repeat(w - 1);
          i += w;
          continue;
        }
        if (lit !== null) lit += c;
        out += ' ';
        i++;
        continue;
      }
      // code
      if (interp.length > 0 && (c === '{' || c === '}')) {
        const top = interp.length - 1;
        if (c === '{') {
          interp[top]++;
        } else if (interp[top] === 0) {
          interp.pop();
          mode = 'str';
          quote = '"';
          escapes = true;
          out += ' ';
          i++;
          continue;
        } else {
          interp[top]--;
        }
        out += ' ';
        i++;
        continue;
      }
      if (c === '/' && s[i + 1] === '/') {
        comments[ln] = s.slice(i + 2);
        out += ' '.repeat(len - i);
        i = len;
        break;
      }
      if (rust && c === '/' && s[i + 1] === '*') {
        mode = 'block';
        blockDepth = 1;
        out += '  ';
        i += 2;
        continue;
      }
      if (!rust && s.startsWith('"""', i)) {
        mode = 'triple';
        out += '"  ';
        i += 3;
        continue;
      }
      if (c === '"' || (!rust && c === '`')) {
        mode = 'str';
        quote = c;
        escapes = c === '"';
        lit = c === '"' ? '' : null;
        litLine = ln + 1;
        out += c;
        i++;
        continue;
      }
      if (rust && (c === 'r' || (c === 'b' && s[i + 1] === 'r')) && (i === 0 || !/\w/.test(s[i - 1]))) {
        const j0 = c === 'b' ? i + 2 : i + 1;
        let j = j0;
        let h = 0;
        while (s[j] === '#') {
          h++;
          j++;
        }
        if (s[j] === '"') {
          mode = 'raw';
          rawHashes = h;
          lit = '';
          litLine = ln + 1;
          out += ' '.repeat(j + 1 - i);
          i = j + 1;
          continue;
        }
      }
      if (c === "'") {
        if (s[i + 1] === '\\') {
          const j = s.indexOf("'", i + 3);
          if (j !== -1) {
            out += ' '.repeat(j + 1 - i);
            i = j + 1;
            continue;
          }
        } else if (s[i + 2] === "'") {
          out += '   ';
          i += 3;
          continue;
        }
      }
      out += c;
      i++;
    }
    if ((mode === 'str' || mode === 'raw') && lit !== null) lit += '\n';
    code[ln] = out.length < len ? out + ' '.repeat(len - out.length) : out;
  }
  return { code, comments, starts, lits };
}

function braceDelta(s) {
  let d = 0;
  for (let i = 0; i < s.length; i++) {
    const ch = s.charCodeAt(i);
    if (ch === 123) d++;
    else if (ch === 125) d--;
  }
  return d;
}

// Last line (0-based) of the item whose keyword sits on `start`.
export function extentEnd(code, start) {
  let paren = 0;
  let brack = 0;
  let brace = 0;
  let opened = false;
  for (let l = start; l < code.length; l++) {
    const s = code[l];
    for (let i = 0; i < s.length; i++) {
      switch (s[i]) {
        case '(':
          paren++;
          break;
        case ')':
          paren--;
          break;
        case '[':
          brack++;
          break;
        case ']':
          brack--;
          break;
        case '{':
          brace++;
          opened = true;
          break;
        case '}':
          brace--;
          if (opened && brace <= 0) return l;
          break;
        case ';':
          if (brace === 0 && paren <= 0 && brack <= 0) return l;
          break;
        default:
      }
    }
  }
  return code.length - 1;
}

// ---------------------------------------------------------------------------
// Rust scanning
// ---------------------------------------------------------------------------

const VIS = String.raw`(?:pub(?:\s*\([^)]*\))?\s+)?`;
const MODS = String.raw`(?:(?:default|const|async|unsafe|safe|extern(?:\s*"")?)\s+)*`;
const RUST_ITEMS = [
  ['fn', new RegExp(`^${VIS}${MODS}fn\\s+([A-Za-z_]\\w*)`)],
  ['struct', new RegExp(`^${VIS}struct\\s+([A-Za-z_]\\w*)`)],
  ['enum', new RegExp(`^${VIS}enum\\s+([A-Za-z_]\\w*)`)],
  ['union', new RegExp(`^${VIS}union\\s+([A-Za-z_]\\w*)`)],
  ['trait', new RegExp(`^${VIS}(?:unsafe\\s+|auto\\s+)?trait\\s+([A-Za-z_]\\w*)`)],
  ['type', new RegExp(`^${VIS}type\\s+([A-Za-z_]\\w*)[^;{]*=`)],
  ['macro', /^macro_rules!\s*([A-Za-z_]\w*)/],
  ['mod', new RegExp(`^${VIS}mod\\s+([A-Za-z_]\\w*)`)],
  ['impl', /^(?:unsafe\s+)?impl\b/],
];
const TEST_ATTR = /^#!?\[\s*(?:[A-Za-z_]\w*\s*::\s*)*test(?:_case\w*)?\s*(?:[\](,(]|$)/;
const CFG_TEST_ATTR = /^#!?\[\s*cfg\s*\(\s*test\s*\)\s*\]/;

function matchRustItem(c) {
  for (const [kind, re] of RUST_ITEMS) {
    const m = re.exec(c);
    if (m) return { kind, name: m[1] ?? null };
  }
  return null;
}

// `impl<..> Trait for Type<..> where ..` -> { owner: Type, trait: Trait | null }
function parseImplHeader(header) {
  let s = header.replace(/^\s*(?:unsafe\s+)?impl\b/, '');
  // Drop balanced generics, ignoring the `>` of `->`.
  let out = '';
  let depth = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (c === '<') depth++;
    else if (c === '>' && s[i - 1] !== '-' && depth > 0) depth--;
    else if (depth === 0) out += c;
  }
  s = out.replace(/\swhere\b.*$/, '');
  const forAt = s.search(/\sfor\s/);
  let traitPart = null;
  let typePart = s;
  if (forAt !== -1) {
    traitPart = s.slice(0, forAt);
    typePart = s.slice(forAt).replace(/^\s*for\s+/, '');
  }
  const lastToken = (text) => {
    const tail = text.split('::').pop() ?? '';
    const tokens = tail.replace(/'\w+/g, ' ').match(/[A-Za-z_]\w*/g);
    return tokens ? tokens[tokens.length - 1] : tail.trim();
  };
  return {
    owner: lastToken(typePart),
    trait: traitPart === null ? null : lastToken(traitPart),
  };
}

function snake(name) {
  return name
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1_$2')
    .toLowerCase();
}

function scanRust(file, text) {
  const { code, lits } = lex(text, true);
  const rawLines = text.split('\n');
  const n = code.length;

  const depthBefore = new Array(n);
  let depth = 0;
  for (let ln = 0; ln < n; ln++) {
    depthBefore[ln] = depth;
    depth += braceDelta(code[ln]);
  }

  const scopes = [];
  const items = [];
  const tests = [];
  const regions = [];
  let pendTest = false;
  let pendCfg = false;

  for (let ln = 0; ln < n; ln++) {
    const c = code[ln].trim();
    if (c === '') continue;
    if (c.startsWith('#')) {
      if (TEST_ATTR.test(c)) pendTest = true;
      if (CFG_TEST_ATTR.test(c)) pendCfg = true;
      continue;
    }
    const hit = matchRustItem(c);
    if (!hit) {
      pendTest = false;
      pendCfg = false;
      continue;
    }
    const end = extentEnd(code, ln);
    const region = pendCfg || (hit.kind === 'mod' && hit.name === 'tests');
    if (hit.kind === 'impl') {
      let header = code[ln];
      let k = ln;
      while (!/[{;]/.test(header) && k + 1 < n && k < ln + 30) {
        k++;
        header += ' ' + code[k];
      }
      const cut = header.search(/[{;]/);
      if (cut >= 0) header = header.slice(0, cut);
      const { owner, trait } = parseImplHeader(header);
      scopes.push({ start: ln, end, depth: depthBefore[ln], owner, trait });
    } else if (hit.kind === 'trait') {
      scopes.push({ start: ln, end, depth: depthBefore[ln], owner: hit.name, trait: null });
      items.push({ kind: 'trait', name: hit.name, start: ln, end, owner: null, trait: null });
    } else if (hit.kind === 'mod') {
      // Modules carry no ledger row; they only mark test regions.
    } else if (hit.kind === 'fn' && pendTest) {
      tests.push({
        file,
        line: ln + 1,
        name: hit.name,
        start: ln,
        end,
        body: rawLines.slice(ln, end + 1).join('\n'),
        lits: lits.filter((l) => l.line - 1 >= ln && l.line - 1 <= end).map((l) => l.text),
      });
      regions.push([ln, end]);
    } else {
      items.push({ kind: hit.kind, name: hit.name, start: ln, end, owner: null, trait: null });
    }
    if (region) regions.push([ln, end]);
    pendTest = false;
    pendCfg = false;
  }

  // Innermost impl/trait body directly containing a line (methods live at body depth).
  const owningScope = (ln) => {
    let best = null;
    for (const s of scopes) {
      if (ln > s.start && ln <= s.end && depthBefore[ln] === s.depth + 1) {
        if (best === null || s.start > best.start) best = s;
      }
    }
    return best;
  };
  const inRegion = (ln) => regions.some(([a, b]) => ln >= a && ln <= b);

  const kept = [];
  let helpers = 0;
  for (const it of items) {
    const sc = owningScope(it.start);
    if (it.kind === 'fn' && sc) {
      it.kind = 'method';
      it.owner = sc.owner;
      it.trait = sc.trait;
    } else if (it.kind === 'type' && sc) {
      continue; // associated type inside an impl/trait body
    }
    if (inRegion(it.start)) {
      if (it.kind === 'fn' || it.kind === 'method') helpers++;
      continue;
    }
    it.file = file;
    kept.push(it);
  }
  return { items: kept, tests, helpers };
}

// ---------------------------------------------------------------------------
// Jet index
// ---------------------------------------------------------------------------

const REF_RE = /((?:[A-Za-z0-9_.-]+\/)*[A-Za-z0-9_-]+\.rs)(?![A-Za-z0-9_])(:\d+(?:[-,]\d+)?)?((?:::[A-Za-z_]\w*)*)/g;
const CALL_RE = /\b[A-Za-z_]\w*_not_ported(?:_output)?\s*\(/g;
const E2104_TEXT = 'is not built yet';

function lineSpec(group) {
  if (!group) return [];
  const m = /^:(\d+)(?:-(\d+)|,(\d+))?$/.exec(group);
  if (!m) return [];
  const a = Number(m[1]);
  if (m[2]) return [[a, Number(m[2])]];
  if (m[3]) return [[a, a], [Number(m[3]), Number(m[3])]];
  return [[a, a]];
}

function normName(s) {
  return s.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '');
}

function newJetIndex(ref) {
  return {
    ref,
    fns: new Map(),
    types: new Map(),
    suffixFns: new Map(),
    suffixTypes: new Map(),
    // `Owner::method` -> loc, for fns declared directly in an impl/struct/enum body.
    methods: new Map(),
    // `alias A :: B` -> B
    aliases: new Map(),
    // lowercased type name with underscores removed -> files defining it
    typeFiles: new Map(),
    // file -> Map(fn name -> loc)
    fileFns: new Map(),
    // Rust names cited in backticks by a comment that says `Rust` (no file path needed).
    rustNames: new Map(),
    // String literals that are exactly one compound identifier (registry/generated tables).
    strNames: new Map(),
    testDescs: new Map(),
    testFns: new Map(),
    portRefs: new Map(),
    cli: [],
    defs: [],
  };
}

function firstLiteralArg(s, from) {
  const args = [];
  let depth = 0;
  let argStart = from;
  let i = from;
  while (i < s.length) {
    const c = s[i];
    if (c === '"') {
      let j = i + 1;
      while (j < s.length && s[j] !== '"') j += s[j] === '\\' ? 2 : 1;
      i = j + 1;
      continue;
    }
    if (c === '(' || c === '[' || c === '{') {
      depth++;
    } else if (c === ')' || c === ']' || c === '}') {
      if (depth === 0) {
        args.push(s.slice(argStart, i));
        break;
      }
      depth--;
    } else if (c === ',' && depth === 0) {
      args.push(s.slice(argStart, i));
      argStart = i + 1;
    }
    i++;
  }
  for (const a of args) {
    const m = /^\s*"((?:[^"\\]|\\.)*)"\s*$/.exec(a);
    if (m) return m[1];
  }
  return null;
}

// `primary` marks the Jet tree under classification; only it contributes CLI census rows.
function scanJet(file, text, idx, primary) {
  const { code, comments, starts, lits } = lex(text, false);
  for (const l of lits) {
    if (/^[A-Za-z_]\w*$/.test(l.text) && isCompound(l.text) && !idx.strNames.has(l.text)) {
      idx.strNames.set(l.text, `${file}:${l.line}`);
    }
  }
  const rawLines = text.split('\n');
  const census = primary && (file.startsWith('Compiler/') || file.startsWith('Jetpack/'));
  const testsDir = /(^|\/)[Tt]ests?\//.test(file);
  const n = code.length;

  // Brace depth and the stack of fn bodies, so CLI rows can name their enclosing fn.
  let depth = 0;
  const fnStack = [];
  let pendingFn = null;
  // impl/struct/enum bodies whose direct fns are methods of `name`.
  const ownerStack = [];
  let pendingOwner = null;
  const fileFns = new Map();
  idx.fileFns.set(file, fileFns);

  for (let ln = 0; ln < n; ln++) {
    const c = code[ln];
    const line = ln + 1;
    const enclosingFn = fnStack.length ? fnStack[fnStack.length - 1].name : null;

    const om = /^\s*(?:pub\s+)?(?:impl|struct|enum|interface|trait)\s+([A-Za-z_]\w*)/.exec(c);
    if (om) pendingOwner = om[1];
    const am = /^\s*(?:pub\s+)?alias\s+([A-Za-z_]\w*)\s*::\s*([A-Za-z_]\w*)/.exec(c);
    if (am && !idx.aliases.has(am[1])) idx.aliases.set(am[1], am[2]);
    const top = ownerStack.length ? ownerStack[ownerStack.length - 1] : null;
    const owner = om ? om[1] : top && top.depth === depth ? top.name : null;

    for (const m of c.matchAll(/\bfn\s+([A-Za-z_]\w*)/g)) {
      const name = m[1];
      pendingFn = name;
      idx.defs.push({ kind: 'fn', name, file, line });
      if (!idx.fns.has(name)) idx.fns.set(name, `${file}:${line}`);
      if (!fileFns.has(name)) fileFns.set(name, `${file}:${line}`);
      if (owner !== null && !idx.methods.has(`${owner}::${name}`)) idx.methods.set(`${owner}::${name}`, `${file}:${line}`);
      if (name.includes('_') && name.length > 1) {
        for (let p = 1; p < name.length; p++) {
          if (name[p] !== '_' || p + 1 >= name.length) continue;
          const key = name.slice(p + 1);
          if (key.includes('_') && !idx.suffixFns.has(key)) idx.suffixFns.set(key, `${file}:${line}`);
        }
      }
      if (testsDir) {
        const key = normName(name.replace(/^test_/, ''));
        if (!idx.testFns.has(key)) idx.testFns.set(key, `${file}:${line}`);
      }
    }

    const tm = /^\s*(?:pub\s+)?(struct|enum|type|trait|interface)\s+([A-Za-z_]\w*)/.exec(c);
    if (tm) {
      const name = tm[2];
      idx.defs.push({ kind: tm[1], name, file, line });
      if (!idx.types.has(name)) idx.types.set(name, `${file}:${line}`);
      const flat = name.toLowerCase().replace(/_/g, '');
      if (!idx.typeFiles.has(flat)) idx.typeFiles.set(flat, []);
      if (!idx.typeFiles.get(flat).includes(file)) idx.typeFiles.get(flat).push(file);
      for (let p = 1; p < name.length; p++) {
        if (name.length - p < 8) break;
        const key = name.slice(p);
        if (!idx.suffixTypes.has(key)) idx.suffixTypes.set(key, `${file}:${line}`);
      }
    }

    if (starts[ln] === 'code') {
      const dm = /^\s*#Test\(\s*"((?:[^"\\]|\\.)*)"/.exec(rawLines[ln]);
      if (dm) {
        idx.defs.push({ kind: 'test', name: dm[1], file, line });
        const key = normName(dm[1]);
        if (!idx.testDescs.has(key)) idx.testDescs.set(key, `${file}:${line}`);
      }
    }

    for (let i = 0; i < c.length; i++) {
      const ch = c[i];
      if (ch === '{') {
        depth++;
        if (pendingOwner !== null) {
          ownerStack.push({ name: pendingOwner, depth });
          pendingOwner = null;
        } else if (pendingFn !== null) {
          fnStack.push({ name: pendingFn, depth });
          pendingFn = null;
        }
      } else if (ch === '}') {
        if (fnStack.length && fnStack[fnStack.length - 1].depth === depth) fnStack.pop();
        else if (ownerStack.length && ownerStack[ownerStack.length - 1].depth === depth) ownerStack.pop();
        depth--;
      } else if (ch === ';' && pendingFn !== null) {
        pendingFn = null; // bodiless declaration
      }
    }
    if (pendingOwner !== null && !om) pendingOwner = null; // header without a body on its own line

    const comment = comments[ln];
    if (comment !== null && /\bRust\b/.test(comment)) {
      for (const bm of comment.matchAll(/`([^`]*)`/g)) {
        const t = /^\s*([A-Za-z_][\w:.]*)\s*(?:\(\s*\))?\s*$/.exec(bm[1]);
        if (!t) continue;
        const segs = t[1].split(/::|\./).filter(Boolean);
        const last = segs[segs.length - 1];
        if (last && !idx.rustNames.has(last)) idx.rustNames.set(last, `${file}:${line}`);
      }
    }

    if (comment !== null && comment.includes('.rs')) {
      const lineNames = new Set();
      for (const bm of comment.matchAll(/`([^`]*)`/g)) {
        const t = /^\s*([A-Za-z_][\w:.]*)\s*(?:\(\s*\))?\s*$/.exec(bm[1]);
        if (t) for (const seg of t[1].split(/::|\./)) if (seg) lineNames.add(seg);
      }
      for (const rm of comment.matchAll(REF_RE)) {
        const path = rm[1];
        const names = new Set(lineNames);
        for (const seg of rm[3].split('::')) if (seg) names.add(seg);
        const base = path.split('/').pop();
        if (!idx.portRefs.has(base)) idx.portRefs.set(base, []);
        idx.portRefs.get(base).push({ jetFile: file, jetLine: line, path, lines: lineSpec(rm[2]), names });
      }
    }

    if (census && c.trim() !== '') {
      const surfaceFor = () => {
        for (const m of c.matchAll(CALL_RE)) {
          if (/\bfn\s+$/.test(c.slice(0, m.index))) continue;
          return firstLiteralArg(rawLines[ln], m.index + m[0].length) ?? '<dynamic>';
        }
        return null;
      };
      let surface = surfaceFor();
      const raw = rawLines[ln];
      if (surface === null && raw.includes('E2104') && raw.includes(E2104_TEXT)) {
        const m = /"((?:[^"\\]|\\.)*is not built yet(?:[^"\\]|\\.)*)"/.exec(raw);
        surface = m ? m[1] : '<dynamic>';
      }
      if (surface !== null && surface !== '?') {
        idx.cli.push({
          file,
          line,
          surface: surface.replace(/[\t\r\n]+/g, ' '),
          fn: (enclosingFn ?? '-').replace(/[\t\r\n]+/g, ' '),
        });
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

function pathMatches(full, cited) {
  return full === cited || full.endsWith('/' + cited);
}

function dropReason(it) {
  if (it.file.startsWith('crates/vendor/')) return 'vendored-upstream';
  if (it.file.startsWith('tests/')) return 'rust-test-harness';
  if (rustEmitPath(it.file)) return 'rust-emit-backend';
  if (FN_LIKE.has(it.kind) && it.kind !== 'macro' && (it.name.startsWith('emit_rust') || it.name.includes('_rust_source'))) {
    return 'rust-emit-backend';
  }
  if (rustOnlyGluePath(it.file)) return 'rust-only-glue';
  if (it.kind === 'method' && it.trait && isStdTrait(it.trait)) return 'rust-only-glue';
  return null;
}

// Multi-word identifiers (`snake_case`, `PascalCase`); single words are too generic to match loosely.
const isCompound = (name) => /[A-Za-z0-9]_[A-Za-z0-9]/.test(name) || /[A-Z][a-z0-9]+[A-Z]/.test(name);

// PascalCase words of a Rust file stem: `CppBind.rs` -> Cpp, Bind; `call_args.rs` -> Call, Args.
function stemWords(file) {
  const stem = basename(file).replace(/\.rs$/, '');
  return snake(stem)
    .split('_')
    .filter((w) => w.length >= 2 && w !== 'mod' && w !== 'lib')
    .map((w) => w[0].toUpperCase() + w.slice(1));
}

// Classifies against one Jet index. Returns null when nothing matches.
function matchItem(it, jet) {
  const refs = jet.portRefs.get(basename(it.file)) ?? [];
  for (const r of refs) {
    if (!pathMatches(it.file, r.path)) continue;
    const lineHit = r.lines.some(([a, b]) => a <= it.end + 1 && b >= it.start + 1);
    if (lineHit || r.names.has(it.name)) {
      return { evidence: 'port-comment', jet: `${r.jetFile}:${r.jetLine}` };
    }
  }

  if (FN_LIKE.has(it.kind)) {
    if (!STOPLIST.has(it.name) && jet.fns.has(it.name)) {
      return { evidence: 'name-exact', jet: jet.fns.get(it.name) };
    }
    if (it.kind === 'method' && /^\w+$/.test(it.owner ?? '')) {
      const loc = jet.fns.get(`${snake(it.owner)}_${it.name}`);
      if (loc) return { evidence: 'name-exact', jet: loc };
    }
    if (it.name.split('_').filter(Boolean).length >= 2 && jet.suffixFns.has(it.name)) {
      return { evidence: 'name-prefixed', jet: jet.suffixFns.get(it.name) };
    }
  } else if (TYPE_LIKE.has(it.kind)) {
    if (jet.types.has(it.name)) {
      return { evidence: 'name-exact', jet: jet.types.get(it.name) };
    }
    if (it.name.length >= 8 && jet.suffixTypes.has(it.name)) {
      return { evidence: 'name-prefixed', jet: jet.suffixTypes.get(it.name) };
    }
  }

  // A Jet comment that names the Rust item (`Rust \`name\``) without a file path;
  // single-word names are too generic to count.
  const compound = isCompound(it.name);
  if (compound && !STOPLIST.has(it.name) && jet.rustNames.has(it.name)) {
    return { evidence: 'rust-name-comment', jet: jet.rustNames.get(it.name) };
  }

  if (FN_LIKE.has(it.kind)) {
    // `Owner::name` -> a Jet method `name` on Owner (or the type Owner aliases).
    if (it.kind === 'method' && /^\w+$/.test(it.owner ?? '')) {
      for (const o of [it.owner, jet.aliases.get(it.owner)]) {
        const loc = o ? jet.methods.get(`${o}::${it.name}`) : undefined;
        if (loc) return { evidence: 'jet-method', jet: loc };
      }
    }
    // `<type>_<rest>` -> fn `rest` in the file that defines the Jet type spelled `<type>`.
    const parts = it.name.split('_').filter(Boolean);
    for (let k = 1; k < parts.length; k++) {
      const rest = parts.slice(k).join('_');
      if (STOPLIST.has(rest) || rest.length < 4) continue;
      for (const f of jet.typeFiles.get(parts.slice(0, k).join('')) ?? []) {
        const loc = jet.fileFns.get(f)?.get(rest);
        if (loc) return { evidence: 'type-prefixed', jet: loc };
      }
    }
    // fn `<stem word>_<name>` named after the Rust file.
    if (!STOPLIST.has(it.name)) {
      for (const w of stemWords(it.file)) {
        const loc = jet.fns.get(`${w.toLowerCase()}_${it.name}`);
        if (loc) return { evidence: 'stem-prefixed', jet: loc };
      }
    }
  } else if (TYPE_LIKE.has(it.kind)) {
    // type `<Stem word><Name>` named after the Rust file (`CppBind.rs` Scalar -> CppScalar).
    for (const w of stemWords(it.file)) {
      const loc = jet.types.get(`${w}${it.name}`);
      if (loc) return { evidence: 'stem-prefixed', jet: loc };
    }
  }
  // A Jet table row naming the item as a string (`{name: "ProcessChild", ...}`).
  if (compound && jet.strNames.has(it.name)) {
    return { evidence: 'jet-string-table', jet: jet.strNames.get(it.name) };
  }
  return null;
}

// Working/primary Jet tree wins. Order: relocation (kept), drop, primary name and
// port-comment rules, primary file-level citation (ported-differs), then secondary
// refs: name rules first, then file-level (`only-on:<ref>:<rule>`, not-ported).
function fileLevel(it, jet) {
  const refs = jet.portRefs.get(basename(it.file)) ?? [];
  const hit = refs.find((r) => pathMatches(it.file, r.path));
  return hit ? `${hit.jetFile}:${hit.jetLine}` : null;
}

function classifyItem(it, primary, offs, relocate) {
  const moved = relocate(it);
  if (moved) return { status: 'kept', evidence: moved.evidence, jet: moved.jet };

  const reason = dropReason(it);
  if (reason) return { status: 'dropped', evidence: reason, jet: '-' };

  const own = matchItem(it, primary);
  // A string-table row is a representation of the item, not its implementation.
  if (own) return { status: own.evidence === 'jet-string-table' ? 'ported-differs' : 'ported', evidence: own.evidence, jet: own.jet };

  const ownFile = fileLevel(it, primary);
  if (ownFile) return { status: 'ported-differs', evidence: 'file-level', jet: ownFile };

  for (const off of offs) {
    const hit = matchItem(it, off);
    if (hit) {
      return { status: 'not-ported', evidence: `only-on:${off.ref}:${hit.evidence}`, jet: `${off.ref}:${hit.jet}` };
    }
  }
  for (const off of offs) {
    const hit = fileLevel(it, off);
    if (hit) return { status: 'not-ported', evidence: `only-on:${off.ref}:file-level`, jet: `${off.ref}:${hit}` };
  }
  return { status: 'not-ported', evidence: 'none', jet: '-' };
}

function isFixture(lit, jetFiles) {
  if (!lit || lit.includes('{') || lit.includes('\n') || lit.includes('..')) return false;
  const rel = lit.replace(/^\//, '');
  const byExt = /\.(jet|stderr|out|case)$/.test(rel);
  const byDir = /^(tests\/ui\/|Examples\/|tests\/cases\/)/.test(rel);
  return (byExt || byDir) && jetFiles.has(rel) ? rel : false;
}

// `tests/cases/**/<rust test name>.case`; prefers a case dir equal to the Rust file stem.
function caseFor(t, cases) {
  const list = cases.get(t.name);
  if (!list) return null;
  const stem = basename(t.file).replace(/\.rs$/, '');
  return list.find((p) => p.split('/').includes(stem)) ?? list[0];
}

const TEST_STOPWORDS = new Set(['a', 'an', 'the', 'is', 'are', 'and', 'or', 'of', 'to', 'in', 'on', 'for', 'with', 'it', 'its', 'be', 'test']);
const testTokens = (key) => key.split('_').filter((w) => w && !TEST_STOPWORDS.has(w));

// Jet test names (#Test descriptions, test fns, case and tests/ui stems) as token lists,
// with a token -> entry posting list, and every word-boundary suffix of each name with
// separators removed (`publication sign hex roundtrips` -> `hexroundtrips`, ...).
function testNameIndex(jet, extra) {
  const entries = [];
  const postings = new Map();
  const suffixes = new Map();
  const add = (key, loc) => {
    const words = key.split('_').filter(Boolean);
    for (let k = 0; k < words.length; k++) {
      const joined = words.slice(k).join('');
      if (!suffixes.has(joined)) suffixes.set(joined, loc);
    }
    const toks = testTokens(key);
    if (toks.length === 0) return;
    const i = entries.length;
    entries.push({ toks, set: new Set(toks), loc });
    for (const w of new Set(toks)) {
      if (!postings.has(w)) postings.set(w, []);
      postings.get(w).push(i);
    }
  };
  for (const [k, loc] of jet.testDescs) add(k, loc);
  for (const [k, loc] of jet.testFns) add(k, loc);
  for (const [k, loc] of extra) add(normName(k), loc);
  return { entries, postings, suffixes };
}

// A Jet test name ending with the Rust name once separators are dropped: a prefixed
// description, or words split or joined differently (`roundtrippable` / `round trippable`).
function suffixTestMatch(key, names) {
  const joined = key.replace(/_/g, '');
  if (!key.includes('_') || joined.length < 12) return null;
  const loc = names.suffixes.get(joined);
  return loc ? { evidence: 'jet-test-suffix', jet: loc } : null;
}

const isSubsequence = (short, long) => {
  let j = 0;
  for (const w of long) if (j < short.length && w === short[j]) j++;
  return j === short.length;
};

// A Jet test whose name carries every Rust word in order and little else (prefixed or
// lightly reworded descriptions), else one sharing most words both ways.
function tokenTestMatch(key, names) {
  const rt = testTokens(key);
  if (rt.length < 3) return null;
  let rarest = null;
  for (const w of new Set(rt)) {
    const p = names.postings.get(w);
    if (!p) continue;
    if (rarest === null || p.length < rarest.length) rarest = p;
  }
  if (rarest === null) return null;
  let overlap = null;
  for (const i of rarest) {
    const e = names.entries[i];
    if (rt.length >= 0.6 * e.toks.length && isSubsequence(rt, e.toks)) return { evidence: 'jet-test-words', jet: e.loc };
    if (overlap === null) {
      const shared = new Set(rt.filter((w) => e.set.has(w))).size;
      if (shared >= 4 && shared >= 0.8 * new Set(rt).size && shared >= 0.5 * e.set.size) overlap = e.loc;
    }
  }
  return overlap === null ? null : { evidence: 'jet-test-overlap', jet: overlap };
}

function classifyTest(t, ctx) {
  const mapped = ctx.mapping.get(`${t.file}\t${t.name}`);
  if (mapped) return { status: 'mapped', evidence: 'mapping-tsv', jet: mapped };

  const key = normName(t.name.replace(/^test_/, ''));
  if (ctx.jet.testDescs.has(key)) return { status: 'mapped', evidence: 'jet-test-name', jet: ctx.jet.testDescs.get(key) };
  if (ctx.jet.testFns.has(key)) return { status: 'mapped', evidence: 'jet-test-name', jet: ctx.jet.testFns.get(key) };

  // A Jet port comment (`Rust: tests/x.rs:123`) whose cited range starts inside the test or
  // on the line above it (`#[test]`), or naming it; a file header citing a range that spans
  // many tests does not count.
  for (const r of ctx.jet.portRefs.get(basename(t.file)) ?? []) {
    if (!pathMatches(t.file, r.path)) continue;
    if (r.lines.some(([a]) => a >= t.start && a <= t.end + 1) || r.names.has(t.name)) {
      return { status: 'mapped', evidence: 'port-comment', jet: `${r.jetFile}:${r.jetLine}` };
    }
  }

  const caseRel = caseFor(t, ctx.cases);
  if (caseRel) {
    if (ctx.retiredIds.has(caseRel)) return { status: 'retired', evidence: 'retired-with-rust', jet: caseRel };
    return { status: 'mapped', evidence: 'testkit-case', jet: caseRel };
  }

  const ui = ctx.uiStems.get(t.name);
  if (ui) return { status: 'mapped', evidence: 'ui-fixture', jet: ui };

  for (const lit of t.lits) {
    const rel = isFixture(lit, ctx.jetFiles);
    if (rel) return { status: 'mapped', evidence: 'fixture', jet: rel };
  }

  for (const r of ctx.retired) {
    if (t.body.includes(r)) return { status: 'retired', evidence: 'retired-with-rust', jet: r };
  }

  if (rustEmitPath(t.file) || /rust_emit|emit_rust|rustc/.test(t.name)) {
    return { status: 'dropped', evidence: 'rust-emit-backend', jet: '-' };
  }

  const near = suffixTestMatch(key, ctx.testNames) ?? tokenTestMatch(key, ctx.testNames);
  if (near) return { status: 'mapped', ...near };

  for (const off of ctx.offs) {
    const loc = off.testDescs.get(key) ?? off.testFns.get(key);
    if (loc) return { status: 'not-mapped', evidence: `only-on:${off.ref}:jet-test-name`, jet: `${off.ref}:${loc}` };
  }
  const stem = basename(t.file).replace(/\.rs$/, '');
  if (ctx.caseDirs.has(`tests/cases/${stem}`)) {
    return { status: 'not-mapped', evidence: `case-dir:tests/cases/${stem}`, jet: '-' };
  }
  return { status: 'not-mapped', evidence: 'none', jet: '-' };
}

// ---------------------------------------------------------------------------
// Triage: the reviewed class of every unmatched row
// ---------------------------------------------------------------------------

// triage.tsv rows: kind (item|test), rust_file (a Rust path, or a directory prefix ending
// in `/`), name (`Owner::name`, `name`, or `*` for every other row of that file or
// directory), class (covered|missing|obsolete), group, evidence. The most specific row
// wins: the exact file before directory prefixes (longest first); within one path
// `Owner::name`, then `name`, then `*`. triage-groups.tsv rows: group, class, rationale.
// Malformed rows are reported as problems, never silently applied.
function loadTriage() {
  const problems = [];
  const read = (file, head) => {
    const p = join(ROOT, OUT_REL, file);
    if (!existsSync(p)) return [];
    const lines = readFileSync(p, 'utf8').split('\n');
    if (lines[0] !== head.join('\t')) {
      problems.push(`${file}:1: header must be ${head.join(' ')}`);
      return [];
    }
    return lines
      .slice(1)
      .map((l, i) => ({ at: `${file}:${i + 2}`, cols: l.replace(/\r$/, '').split('\t') }))
      .filter((r) => r.cols.some(Boolean));
  };
  const rules = new Map();
  for (const { at, cols } of read(TRIAGE_FILE, ['kind', 'rust_file', 'name', 'class', 'group', 'evidence'])) {
    const [kind, path, name, cls, group, evidence] = cols;
    if (cols.length !== 6 || cols.some((c) => !c.trim())) problems.push(`${at}: needs 6 non-empty columns`);
    else if (kind !== 'item' && kind !== 'test') problems.push(`${at}: kind \`${kind}\` is not item or test`);
    else if (!CLASSES.includes(cls)) problems.push(`${at}: class \`${cls}\` is not ${CLASSES.join(', ')}`);
    else {
      const key = `${kind}\t${path}`;
      if (!rules.has(key)) rules.set(key, new Map());
      const prior = rules.get(key).get(name);
      if (prior) problems.push(`${at}: repeats ${prior.at}`);
      else rules.get(key).set(name, { at, cls, group, evidence, hits: 0 });
    }
  }
  const groups = new Map();
  for (const { at, cols } of read(GROUPS_FILE, ['group', 'class', 'rationale'])) {
    const [group, cls, rationale] = cols;
    if (cols.length !== 3 || cols.some((c) => !c.trim())) problems.push(`${at}: needs 3 non-empty columns`);
    else if (!CLASSES.includes(cls)) problems.push(`${at}: class \`${cls}\` is not ${CLASSES.join(', ')}`);
    else if (groups.has(group)) problems.push(`${at}: repeats group \`${group}\``);
    else groups.set(group, { at, cls, rationale, items: 0, tests: 0, rustLines: 0 });
  }
  return { rules, groups, problems };
}

function triageRule(triage, kind, file, names) {
  const paths = [file];
  for (let i = file.lastIndexOf('/'); i > 0; i = file.lastIndexOf('/', i - 1)) paths.push(file.slice(0, i + 1));
  for (const p of paths) {
    const byName = triage.rules.get(`${kind}\t${p}`);
    if (!byName) continue;
    for (const n of names) {
      const r = byName.get(n);
      if (r) return r;
    }
  }
  return null;
}

// `path/File.ext:LINE` citations in triage evidence (extensionless files such as a
// tests/ui `stderr` too), with the cited definition's name when written
// `path:LINE (name ...)`; a citation qualified by a ref (`origin/port/main:path:LINE`)
// names another tree and is not checked here.
const CITE_RE = /(?<![:\w./-])((?:[A-Za-z0-9_.-]+\/)+[A-Za-z0-9_.-]+):(\d+)(?:-\d+)?(?:\s*\(\s*([A-Za-z_][\w.:]*))?/g;

// Sets cls/group/classEvidence on every row: the triage class for unmatched rows
// (`unclassified` when no triage row applies), `-` for rows a ledger rule matched.
// Checks every citation against the Jet tree (or the pinned Rust tree for Rust paths)
// and returns the class counts and every triage problem.
function applyTriage(itemRows, testRows, ctx) {
  const triage = loadTriage();
  const problems = [...triage.problems];
  const zero = () => ({ covered: 0, missing: 0, obsolete: 0, unclassified: 0 });
  const counts = { items: zero(), tests: zero() };
  // Per area (`crates/<crate>`, `Source/<file>`, `tests`): rows and Rust lines per class,
  // and rows per missing group.
  const areas = {};
  const apply = (r, kind, unmatched, names) => {
    if (!unmatched) {
      Object.assign(r, { cls: '-', group: '-', classEvidence: '-' });
      return;
    }
    const rule = triageRule(triage, kind, r.file, names);
    if (rule) {
      rule.hits++;
      Object.assign(r, { cls: rule.cls, group: rule.group, classEvidence: rule.evidence });
      const lines = r.end - r.start + 1;
      const g = triage.groups.get(rule.group);
      if (g) {
        g[kind === 'item' ? 'items' : 'tests']++;
        g.rustLines += lines;
      }
      const area = r.file.startsWith('tests/') ? 'tests' : r.file.split('/').slice(0, 2).join('/');
      const a = (areas[area] ??= { groups: {} });
      const c = (a[rule.cls] ??= { items: 0, tests: 0, rust_lines: 0 });
      c[kind === 'item' ? 'items' : 'tests']++;
      c.rust_lines += lines;
      if (rule.cls === 'missing') bump(a.groups, rule.group);
    } else {
      Object.assign(r, { cls: 'unclassified', group: '-', classEvidence: '-' });
    }
    bump(counts[kind === 'item' ? 'items' : 'tests'], r.cls);
  };
  for (const r of itemRows) {
    const names = r.owner ? [`${r.owner}::${r.name}`, r.name, '*'] : [r.name, '*'];
    apply(r, 'item', r.status === 'not-ported', names);
  }
  for (const t of testRows) apply(t, 'test', t.status === 'not-mapped', [t.name, '*']);

  const cites = new Map();
  for (const byName of triage.rules.values()) {
    for (const rule of byName.values()) {
      if (rule.hits === 0) problems.push(`${rule.at}: matches no unmatched row`);
      const g = triage.groups.get(rule.group);
      if (!g) problems.push(`${rule.at}: group \`${rule.group}\` has no rationale in ${GROUPS_FILE}`);
      else if (g.cls !== rule.cls) problems.push(`${rule.at}: class ${rule.cls} differs from group \`${rule.group}\` (${g.cls})`);
      const found = [...rule.evidence.matchAll(CITE_RE)];
      if (rule.cls === 'covered' && found.length === 0) problems.push(`${rule.at}: covered evidence cites no path:LINE`);
      for (const m of found) {
        if (!cites.has(m[1])) cites.set(m[1], []);
        cites.get(m[1]).push({ at: rule.at, line: Number(m[2]), name: m[3] ?? null });
      }
    }
  }
  for (const [name, g] of triage.groups) {
    if (g.items + g.tests === 0) problems.push(`${g.at}: group \`${name}\` classifies no row`);
  }
  const sources = [...cites.keys()].map((p) => (ctx.jetFiles.has(p) ? `${ctx.jetRev}:${p}` : ctx.rustPaths.has(p) ? `${ctx.rev}:${p}` : null));
  const known = [...cites.keys()].filter((_, i) => sources[i] !== null);
  for (const [p, uses] of cites) {
    if (!ctx.jetFiles.has(p) && !ctx.rustPaths.has(p)) for (const u of uses) problems.push(`${u.at}: cites \`${p}\`, absent from the Jet and Rust trees`);
  }
  // A citation naming its definition (`path:LINE (name)`, a compound identifier) must
  // find that name within three lines of LINE (its last `.`/`::` segment, also as the
  // tail of a module-prefixed identifier), so drifted or invented lines are caught.
  eachBlob(sources.filter(Boolean), (i, text) => {
    const lines = text.split('\n');
    for (const u of cites.get(known[i])) {
      if (u.line < 1 || u.line > lines.length) {
        problems.push(`${u.at}: cites \`${known[i]}:${u.line}\`, past its ${lines.length} lines`);
        continue;
      }
      const word = u.name === null ? null : u.name.split(/::|\./).filter(Boolean).pop();
      if (!word || !isCompound(word) || basename(known[i]).replace(/\.[^.]+$/, '') === word) continue;
      const window = lines.slice(Math.max(0, u.line - 4), u.line + 3).join('\n');
      if (!new RegExp(`(?<![A-Za-z0-9])${word}(?![A-Za-z0-9_])`).test(window)) {
        problems.push(`${u.at}: \`${word}\` is not within 3 lines of \`${known[i]}:${u.line}\``);
      }
    }
  });
  problems.sort(cmp);

  const groups = [...triage.groups]
    .map(([group, g]) => ({ group, class: g.cls, items: g.items, tests: g.tests, rust_lines: g.rustLines, rationale: g.rationale }))
    .sort((a, b) => b.items + b.tests - (a.items + a.tests) || cmp(a.group, b.group));
  return { items: counts.items, tests: counts.tests, areas, groups, problems };
}

// The triage sections of the owner summary: class counts, missing and obsolete rows
// per area, then every missing (behavior groups, then test-only groups), obsolete and
// covered group with its size and rationale, then every problem.
function triageSummary(t) {
  const out = ['## Triage of unmatched rows\n'];
  out.push(
    'Every `not-ported` item and `not-mapped` test carries a reviewed class from `triage.tsv` (columns `class`, `group`, `class_evidence` of `items.tsv` and `tests.tsv`): ' +
      '`covered` (a Jet counterpart exists; the evidence cites its `path:line`), `missing` (Jet lacks the behavior; `partial:` names what Jet has and lacks, `untested:` a test whose behavior Jet has without a Jet test), ' +
      '`obsolete` (no meaning in Jet: Rust-host plumbing, a retired surface, or a Jet design that makes it unnecessary; cited). ' +
      'Group rationales live in `triage-groups.tsv`. `rust lines` sums the Rust source lines of the rows. ' +
      'The ledger checks every cited Jet `path:line` exists and that a cited definition name sits within three lines of it; `--check` fails on any unclassified row or triage problem.\n',
  );
  const classes = [...CLASSES, 'unclassified'];
  out.push(table(['class', 'items', 'tests', 'rows'], classes.map((c) => [c, count(t.items[c]), count(t.tests[c]), count((t.items[c] ?? 0) + (t.tests[c] ?? 0))])));
  const areaRows = (cls) =>
    Object.entries(t.areas)
      .filter(([, a]) => a[cls])
      .sort((x, y) => y[1][cls].rust_lines - x[1][cls].rust_lines || cmp(x[0], y[0]));
  out.push('## Missing by area\n');
  out.push('Rust area (`crates/<crate>`, `Source/<file>`, every integration test under `tests`), largest Rust line count first, with its five largest missing groups.\n');
  out.push(
    table(
      ['area', 'items', 'tests', 'rust lines', 'largest groups'],
      areaRows('missing').map(([area, a]) => [
        `\`${area}\``,
        count(a.missing.items),
        count(a.missing.tests),
        count(a.missing.rust_lines),
        byCount(a.groups).slice(0, 5).map(([g, n]) => `\`${g}\` ${n}`).join(', '),
      ]),
    ),
  );
  out.push('## Obsolete by area\n');
  out.push(table(['area', 'items', 'tests', 'rust lines'], areaRows('obsolete').map(([area, a]) => [`\`${area}\``, count(a.obsolete.items), count(a.obsolete.tests), count(a.obsolete.rust_lines)])));
  const section = (title, note, gs) => {
    out.push(`## ${title}\n`);
    out.push(`${note} ${gs.length} groups.\n`);
    out.push(table(['group', 'items', 'tests', 'rust lines', 'rationale'], gs.map((g) => [`\`${g.group}\``, count(g.items), count(g.tests), count(g.rust_lines), g.rationale])));
  };
  const missing = t.groups.filter((g) => g.class === 'missing');
  section('Missing behavior (owner list)', 'Groups holding Rust items Jet lacks, largest first; each names what is absent.', missing.filter((g) => g.items > 0));
  section('Missing tests', 'Groups of Rust tests only: no Jet test exercises them (`untested:` when the behavior exists in Jet, `absent:` when it was not found).', missing.filter((g) => g.items === 0));
  section('Obsolete groups (owner approval needed to drop)', 'Rows dropped from parity, with the reason; the owner approves each group.', t.groups.filter((g) => g.class === 'obsolete'));
  section('Covered groups', 'Jet counterparts the name matcher missed; every row cites its Jet location in `class_evidence`.', t.groups.filter((g) => g.class === 'covered'));
  out.push('## Triage problems\n');
  out.push(t.problems.length === 0 ? 'None.\n' : t.problems.map((p) => `- ${p}`).join('\n') + '\n');
  return out.join('\n');
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

const row = (cols) => cols.map((v) => String(v).replace(/[\t\r\n]+/g, ' ')).join('\t') + '\n';

function sortKeys(v) {
  if (Array.isArray(v)) return v.map(sortKeys);
  if (v && typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v).sort(cmp)) o[k] = sortKeys(v[k]);
    return o;
  }
  return v;
}

function bump(obj, key) {
  obj[key] = (obj[key] ?? 0) + 1;
}

// `origin/port/main` -> `port_main`, `cutover-ready` -> `cutover_ready`
const refKey = (name) => name.replace(/^origin\//, '').replace(/[^A-Za-z0-9]+/g, '_').replace(/^_+|_+$/g, '');

function generate({ rev, jetRev, refs }) {
  // Rust side: one cat-file batch over the pinned tree.
  const rustPaths = gitList(['ls-tree', '-r', '-z', '--name-only', rev]).filter(isRustScope).sort(cmp);
  const rustItems = [];
  const rustTests = [];
  let helpers = 0;
  const rustSha = new Map(lsTreeEntries(rev).map((e) => [e.path, e.sha]));
  eachBlob(rustPaths.map((p) => `${rev}:${p}`), (i, text) => {
    const r = scanRust(rustPaths[i], text);
    rustItems.push(...r.items.map((it) => ({ ...it, sha: rustSha.get(rustPaths[i]) })));
    rustTests.push(...r.tests);
    helpers += r.helpers;
  });

  // Primary Jet side: the tree at --jet-rev.
  const jetEntries = lsTreeEntries(jetRev);
  const jetFileList = jetEntries.map((e) => e.path).sort(cmp);
  const jetFiles = new Set(jetFileList);
  const jetPaths = jetFileList.filter(isJetScope);
  const jet = newJetIndex(jetRev);
  eachBlob(jetPaths.map((p) => `${jetRev}:${p}`), (i, text) => scanJet(jetPaths[i], text, jet, true));

  // Secondary Jet trees.
  const offs = refs.map((r) => {
    const idx = newJetIndex(r.name);
    const paths = gitList(['ls-tree', '-r', '-z', '--name-only', r.sha]).filter(isJetScope).sort(cmp);
    eachBlob(paths.map((p) => `${r.sha}:${p}`), (i, text) => scanJet(paths[i], text, idx, false));
    return idx;
  });

  const mapping = new Map();
  for (const line of readBlobText(jetRev, MAPPING_PATH).split('\n').slice(1)) {
    const cols = line.replace(/\r$/, '').split('\t');
    if (cols.length < 5 || !cols[0]) continue;
    const key = `${cols[0]}\t${cols[2]}`;
    if (!mapping.has(key)) mapping.set(key, `${cols[3]}:${cols[4]}`);
  }

  const retired = [];
  for (const line of readBlobText(jetRev, RETIRED_PATH).split('\n')) {
    if (line === '' || line.startsWith('#')) continue;
    const id = line.split('\t')[0].trim();
    if (id) retired.push(id);
  }
  const retiredIds = new Set(retired);

  const cases = new Map();
  for (const p of jetFileList) {
    if (!p.startsWith('tests/cases/') || !p.endsWith('.case')) continue;
    const base = basename(p).slice(0, -'.case'.length);
    if (!cases.has(base)) cases.set(base, []);
    cases.get(base).push(p);
  }

  // tests/ui fixtures by file stem (`<rust test name>.stderr` and siblings).
  const uiStems = new Map();
  for (const p of jetFileList) {
    if (!p.startsWith('tests/ui/')) continue;
    const stem = basename(p).replace(/\.[^.]+$/, '');
    if (!uiStems.has(stem)) uiStems.set(stem, p);
  }

  // Tests whose Rust file stem names a case directory.
  const caseDirs = new Set();
  for (const p of jetFileList) {
    const m = /^(tests\/cases\/[^/]+)\//.exec(p);
    if (m) caseDirs.add(m[1]);
  }

  // Relocated Rust files: blob-identical copies outside the Rust scope (primary
  // tree first, then cutover-ready), and Support-crate copies at the same
  // crate + src-relative path.
  const relocBySha = new Map();
  const relocEdit = new Map();
  const relocSources = [{ ref: jetRev, entries: jetEntries }];
  for (const r of refs) if (r.name === 'cutover-ready') relocSources.push({ ref: r.sha, entries: lsTreeEntries(r.sha) });
  for (const src of relocSources) {
    for (const e of src.entries) {
      if (e.type !== 'blob') continue;
      if (isOutsideRust(e.path) && e.sha !== EMPTY_BLOB) {
        if (!relocBySha.has(e.sha)) relocBySha.set(e.sha, []);
        relocBySha.get(e.sha).push({ ref: src.ref, path: e.path });
      }
      const sup = /^Support\/([^/]+)\/src\/(.+\.rs)$/.exec(e.path);
      if (sup && !relocEdit.has(`${sup[1]}/${sup[2]}`)) {
        relocEdit.set(`${sup[1]}/${sup[2]}`, { ref: src.ref, path: e.path });
      }
    }
  }
  const relocate = (it) => {
    const bySha = (relocBySha.get(it.sha) ?? []).find((c) => c.path !== it.file);
    if (bySha) return { evidence: 'relocated', jet: `${bySha.ref}:${bySha.path}` };
    const m = /^crates\/([^/]+)\/src\/(.+\.rs)$/.exec(it.file);
    const edited = m ? relocEdit.get(`${m[1]}/${m[2]}`) : undefined;
    if (edited) return { evidence: 'relocated-edited', jet: `${edited.ref}:${edited.path}` };
    return null;
  };

  // Items.
  const itemRows = rustItems
    .map((it) => ({ ...it, ...classifyItem(it, jet, offs, relocate) }))
    .sort((a, b) => cmp(a.file, b.file) || a.start - b.start || cmp(a.kind, b.kind) || cmp(a.name, b.name));

  const itemsByStatus = { kept: 0, dropped: 0, 'ported-differs': 0, 'not-ported': 0, ported: 0 };
  const itemsByEvidence = {};
  const droppedByReason = {};
  const notPortedByArea = {};
  const notPortedByFile = {};
  const droppedByFile = {};
  for (const r of itemRows) {
    bump(itemsByStatus, r.status);
    bump(itemsByEvidence, r.evidence);
    if (r.status === 'dropped') bump(droppedByReason, r.evidence);
    if (r.status === 'not-ported') bump(notPortedByArea, r.file.split('/').slice(0, 2).join('/'));
    if (r.status === 'not-ported') bump(notPortedByFile, r.file);
    if (r.status === 'dropped') bump(droppedByFile, r.file);
  }

  // Tests.
  const caseNames = [...cases].map(([name, list]) => [name, list[0]]);
  const testNames = testNameIndex(jet, [...caseNames, ...uiStems]);
  const ctx = { mapping, jet, offs, cases, caseDirs, uiStems, testNames, jetFiles, retired, retiredIds };
  const testRows = rustTests
    .map((t) => ({ ...t, ...classifyTest(t, ctx) }))
    .sort((a, b) => cmp(a.file, b.file) || a.line - b.line || cmp(a.name, b.name));

  const testsByStatus = { dropped: 0, mapped: 0, 'not-mapped': 0, retired: 0 };
  const testsByEvidence = {};
  const notMappedByFile = {};
  for (const t of testRows) {
    bump(testsByStatus, t.status);
    bump(testsByEvidence, t.evidence);
    if (t.status === 'not-mapped') bump(notMappedByFile, t.file);
  }

  const triage = applyTriage(itemRows, testRows, { jetFiles, jetRev, rev, rustPaths: new Set(rustPaths) });

  const itemsTsv =
    row(['rust_file', 'line', 'end', 'kind', 'owner', 'name', 'status', 'evidence', 'jet', 'class', 'group', 'class_evidence']) +
    itemRows
      .map((r) =>
        row([r.file, r.start + 1, r.end + 1, r.kind, r.owner ?? '-', r.name, r.status, r.evidence, r.jet, r.cls, r.group, r.classEvidence]),
      )
      .join('');
  const testsTsv =
    row(['rust_file', 'line', 'name', 'status', 'evidence', 'jet', 'class', 'group', 'class_evidence']) +
    testRows.map((t) => row([t.file, t.line, t.name, t.status, t.evidence, t.jet, t.cls, t.group, t.classEvidence])).join('');

  // CLI census (primary tree only).
  const cliRows = jet.cli.sort((a, b) => cmp(a.file, b.file) || a.line - b.line);
  const cliTsv =
    row(['file', 'line', 'surface', 'fn']) + cliRows.map((c) => row([c.file, c.line, c.surface, c.fn])).join('');
  const surfaces = [...new Set(cliRows.map((c) => c.surface))].sort(cmp);

  // Off-dev definitions: defined on a secondary ref and absent from the primary index by name.
  const offdevRows = [];
  for (const off of offs) {
    for (const d of off.defs) {
      let exists;
      if (d.kind === 'fn') {
        exists = jet.fns.has(d.name);
      } else if (d.kind === 'test') {
        const k = normName(d.name);
        exists = jet.testDescs.has(k) || jet.testFns.has(k) || jet.testFns.has(normName(d.name.replace(/^test_/, '')));
      } else {
        exists = jet.types.has(d.name);
      }
      if (!exists) offdevRows.push({ ref: off.ref, ...d });
    }
  }
  offdevRows.sort((a, b) => cmp(a.ref, b.ref) || cmp(a.file, b.file) || a.line - b.line || cmp(a.name, b.name));
  const offdevTsv =
    row(['ref', 'file', 'line', 'kind', 'name']) +
    offdevRows.map((r) => row([r.ref, r.file, r.line, r.kind, r.name])).join('');

  const offdevSummary = {};
  for (const off of offs) offdevSummary[off.ref] = { total: 0, by_top_dir: {} };
  for (const r of offdevRows) {
    const s = offdevSummary[r.ref];
    s.total++;
    bump(s.by_top_dir, r.file.split('/')[0]);
  }

  const refsSummary = { rev, jet_rev: jetRev, jet_refs: refs.map((r) => ({ name: r.name, sha: r.sha })) };
  for (const r of refs) refsSummary[refKey(r.name)] = r.sha;

  // Rust that stays after the cutover: every .rs/Cargo file of the Jet tree
  // outside the ledger's Rust scope, grouped by directory with its reason.
  const keptRust = {};
  for (const e of lsTreeEntries(jetRev)) {
    if (e.type !== 'blob' || isRustScope(e.path)) continue;
    if (!e.path.endsWith('.rs') && !/(^|\/)Cargo\.(toml|lock)$/.test(e.path)) continue;
    const dir = e.path.split('/').slice(0, 3).join('/');
    keptRust[dir] ??= { files: 0, reason: keptRustReason(e.path) };
    keptRust[dir].files++;
  }

  const summary = sortKeys({
    rev,
    refs: refsSummary,
    items: {
      total: itemRows.length,
      by_status: itemsByStatus,
      by_evidence: itemsByEvidence,
      dropped_by_reason: droppedByReason,
      not_ported_by_area: notPortedByArea,
      dropped_by_file: droppedByFile,
      not_ported_by_file: notPortedByFile,
    },
    test_helpers: helpers,
    tests: {
      total: testRows.length,
      by_status: testsByStatus,
      by_evidence: testsByEvidence,
      not_mapped_by_file: notMappedByFile,
    },
    cli: { total: cliRows.length, surfaces },
    kept_rust: keptRust,
    offdev: offdevSummary,
    triage,
  });

  return {
    'items.tsv': itemsTsv,
    'tests.tsv': testsTsv,
    'cli.tsv': cliTsv,
    'offdev.tsv': offdevTsv,
    'summary.json': JSON.stringify(summary, null, 2) + '\n',
    'SUMMARY.md': ownerSummary(summary, currentRows(itemRows, testRows)),
  };
}

// Why a Rust file outside the ledger's scope stays in the tree.
function keptRustReason(p) {
  if (p.startsWith('Jetpack/Integrations/Source/Foreign/RustBridge/')) return 'Rust FFI bridge runtime and support crates';
  if (p.startsWith('Examples/ffi/') || p.startsWith('tests/fixtures/') || p.startsWith('tests/cases/')) return 'Rust FFI example or fixture';
  if (p.startsWith('Tools/editors/zed/')) return 'Zed editor extension (Zed extensions are Rust)';
  if (/^(Tools\/(gauntlet|agent-eval|perf|ci)|tests\/(compiled_workloads|compiler-proof)|Compiler\/JetRuntime\/Tests)\//.test(p)) return 'language-comparison peer or reference';
  return 'unclassified';
}

const count = (n) => String(n ?? 0);
const table = (head, rows) =>
  `| ${head.join(' | ')} |\n|${head.map(() => '---').join('|')}|\n` + rows.map((r) => `| ${r.join(' | ')} |\n`).join('');
const byCount = (obj) => Object.entries(obj ?? {}).sort((a, b) => b[1] - a[1] || cmp(a[0], b[0]));

// Current classification of every Rust item and test, keyed the way sample files name
// them: Rust file basename, 1-based line, last name segment (`Owner::name`, `Owner.name`).
const sampleKey = (file, line, name) => `${basename(file)}\t${line}\t${name.split(/::|\./).pop()}`;
function currentRows(itemRows, testRows) {
  const m = new Map();
  for (const r of itemRows) m.set(sampleKey(r.file, r.start + 1, r.name), { status: r.status, evidence: r.evidence });
  for (const t of testRows) m.set(sampleKey(t.file, t.line, t.name), { status: t.status, evidence: t.evidence });
  return m;
}

// Hand-checked samples of the matcher's output, listed by samples/rounds.tsv
// (round, kind, matcher, pool, file); each file is a TSV with rust_file, line, name and
// verdict columns. Rates count only rows the current ledger still classifies as sampled:
// kind `items`/`tests`: rows still unmatched (found|partial|obsolete|absent);
// kind `fp-items`/`fp-tests`: rows still matched by the sampled evidence (correct|related|wrong).
function residualSample(current) {
  const dir = join(ROOT, OUT_REL, 'samples');
  const index = join(dir, 'rounds.tsv');
  if (!existsSync(index)) return '';
  const out = ['## Matcher residual (hand-checked samples)\n'];
  out.push(
    'Random rows hand-checked against the Jet tree; every row with its exact Jet location or the searches run is in `samples/`. ' +
      '`now` counts the sampled rows the current ledger still classifies the sampled way (still unmatched, or still matched by the same evidence); verdicts and rates are over those rows. ' +
      'For unmatched rows, `found` is a matcher false negative (a Jet counterpart exists) and `partial` is related Jet code covering part of the job. ' +
      'For loosely matched rows, `wrong` is a matcher false positive.\n',
  );
  const rows = [];
  for (const line of readFileSync(index, 'utf8').split('\n').slice(1)) {
    if (!line.trim()) continue;
    const [round, kind, matcher, pool, file] = line.split('\t');
    const lines = readFileSync(join(dir, file), 'utf8').split('\n').filter((l) => l.trim());
    const head = lines[0].split('\t');
    const at = (cols, name) => cols[head.indexOf(name)];
    const fp = kind.startsWith('fp-');
    const counts = {};
    let now = 0;
    for (const l of lines.slice(1)) {
      const cols = l.split('\t');
      const cur = current.get(sampleKey(at(cols, 'rust_file'), at(cols, 'line'), at(cols, 'name')));
      const still = fp
        ? cur !== undefined && cur.evidence === at(cols, 'evidence') && (cur.status === 'ported' || cur.status === 'mapped')
        : cur !== undefined && (cur.status === 'not-ported' || cur.status === 'not-mapped');
      if (!still) continue;
      now++;
      bump(counts, at(cols, 'verdict'));
    }
    const pct = (k) => (now === 0 ? '-' : `${Math.round((100 * (counts[k] ?? 0)) / now)}%`);
    const verdicts = Object.entries(counts).sort((a, b) => cmp(a[0], b[0])).map(([k, v]) => `${k} ${v}`).join(', ') || '-';
    const fpart = now === 0 ? '-' : `${Math.round((100 * ((counts.found ?? 0) + (counts.partial ?? 0))) / now)}%`;
    const rate = fp ? `wrong ${pct('wrong')}, related ${pct('related')}` : `found ${pct('found')}, found+partial ${fpart}`;
    rows.push([round, kind, matcher, pool, String(lines.length - 1), String(now), verdicts, rate, `\`samples/${file}\``]);
  }
  out.push(table(['round', 'kind', 'matcher', 'pool', 'rows', 'now', 'verdicts', 'rate', 'evidence'], rows));
  return out.join('\n');
}

// The owner summary: every count, the matcher's hand-checked residual, every
// not-ported area and file, every dropped Rust file, every not-mapped test file,
// every CLI refusal and every kept Rust directory. `current` keys the ledger rows
// for the sample files.
function ownerSummary(s, current) {
  const it = s.items;
  const ts = s.tests;
  const out = [];
  out.push('# Rust exit parity ledger: owner summary\n');
  out.push('Generated by `node Tools/agent/rust-parity-ledger.mjs`; do not edit. `--check` regenerates it.\n');
  out.push(`Rust tree: \`${s.refs.rev}\`. Jet tree: \`${s.refs.jet_rev}\`. Secondary Jet trees: ${s.refs.jet_refs.map((r) => `\`${r.name}\` (\`${r.sha}\`)`).join(', ') || 'none'}.\n`);
  out.push('Rows: `items.tsv` (one per Rust fn, method, type, trait, macro), `tests.tsv` (one per Rust `#[test]`), `cli.tsv` (Jet CLI refusals), `offdev.tsv` (Jet definitions that exist only on a secondary tree). Statuses:\n');
  out.push([
    '- `ported`: a Jet item cites the Rust item (`port-comment`: `// Rust: path:line`; `rust-name-comment`: a comment naming Rust `name`), carries its exact or prefixed name (`name-exact`, `name-prefixed`), is a method of the same type (`jet-method`), is `<rest>` in the file defining the Jet type spelled by the name\'s leading words (`type-prefixed`), or is prefixed by a word of the Rust file name (`stem-prefixed`).',
    '- `ported-differs`: `file-level` (a Jet file cites the Rust file, but no Jet item matches this item by name or line) or `jet-string-table` (a Jet string literal names the item, as a generated or registry table row, but no Jet definition does).',
    '- `kept`: the Rust file survives the cutover at a new path (FFI bridge runtime and support crates).',
    '- `not-ported`: no Jet counterpart on the Jet tree; `only-on:<ref>` means the port exists only on that secondary tree.',
    '- `dropped`: retired with Rust: `rust-emit-backend` (the Rust-emitting backend and its runtime prelude), `rust-only-glue` (build scripts, the Rust bootstrap host, stage0 repack, std trait impls), `rust-test-harness` (non-test helpers of the Rust integration tests), `vendored-upstream`.',
    '- tests: `mapped` (`mapping-tsv`; `jet-test-name`: same #Test description or test fn; `port-comment`: a Jet comment cites a line of the test; `testkit-case`; `ui-fixture`: `tests/ui/<name>.*`; `fixture`; `jet-test-suffix`: a Jet test name ends with the Rust name once separators are dropped; `jet-test-words`: a Jet test name holds every Rust word in order plus at most 40% more; `jet-test-overlap`: at least 4 shared words, 80% of the Rust words and half of the Jet words), `retired` (listed in `Tools/testkit/retired-with-rust.tsv`), `dropped` (tests of the Rust-emit backend), `not-mapped` (`case-dir:` names the testkit directory of the same Rust file).',
  ].join('\n') + '\n');
  out.push('## Items\n');
  out.push(table(['status', 'rows'], byCount(it.by_status).map(([k, v]) => [k, count(v)])));
  out.push(table(['evidence', 'rows'], byCount(it.by_evidence).map(([k, v]) => [`\`${k}\``, count(v)])));
  out.push('## Tests\n');
  out.push(table(['status', 'rows'], byCount(ts.by_status).map(([k, v]) => [k, count(v)])));
  out.push(table(['evidence', 'rows'], byCount(ts.by_evidence).map(([k, v]) => [`\`${k}\``, count(v)])));
  out.push(triageSummary(s.triage));
  out.push(residualSample(current));
  out.push('## Not-ported items by area\n');
  out.push('Area = first two path segments of the Rust file; every row: `items.tsv` where status is `not-ported`.\n');
  out.push(table(['area', 'not-ported items'], byCount(it.not_ported_by_area).map(([a, n]) => [`\`${a}\``, count(n)])));
  out.push('## CLI refusals\n');
  out.push(`${s.cli.total} call sites refuse a surface as not yet ported (\`cli.tsv\`). Surfaces:\n`);
  out.push(s.cli.surfaces.map((x) => `- \`${x}\``).join('\n') + '\n');
  out.push('## Off-dev Jet work\n');
  out.push(table(['ref', 'definitions only there', 'by top directory'], Object.entries(s.offdev).sort((a, b) => cmp(a[0], b[0])).map(([k, v]) => [`\`${k}\``, count(v.total), byCount(v.by_top_dir).map(([d, n]) => `${d} ${n}`).join(', ')])));
  out.push('## Rust kept after the cutover\n');
  out.push(table(['directory', 'files', 'reason'], Object.entries(s.kept_rust).sort((a, b) => cmp(a[0], b[0])).map(([d, v]) => [`\`${d}\``, count(v.files), v.reason])));
  out.push('## Not-ported items by Rust file\n');
  out.push('Every row: `items.tsv` where status is `not-ported`.\n');
  out.push(table(['Rust file', 'not-ported items'], byCount(it.not_ported_by_file).map(([f, n]) => [`\`${f}\``, count(n)])));
  out.push('## Dropped items by Rust file\n');
  out.push('Every row: `items.tsv` where status is `dropped`; the evidence column is the reason.\n');
  out.push(table(['Rust file', 'dropped items'], byCount(it.dropped_by_file).map(([f, n]) => [`\`${f}\``, count(n)])));
  out.push('## Not-mapped tests by Rust file\n');
  out.push(table(['Rust file', 'not-mapped tests'], byCount(ts.not_mapped_by_file).map(([f, n]) => [`\`${f}\``, count(n)])));
  return out.join('\n');
}

function committedSummary() {
  try {
    return JSON.parse(readFileSync(join(ROOT, OUT_REL, 'summary.json'), 'utf8'));
  } catch {
    return null;
  }
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  // --check defaults to the commits the committed ledger was generated from.
  const recorded = opts.check ? committedSummary()?.refs : null;
  if (opts.check && !recorded && !opts.rev) fail('no committed summary.json; pass --rev');
  const rev = resolveCommit(opts.rev ?? recorded.rev);
  const jetRev = resolveCommit(opts.jetRev ?? recorded?.jet_rev ?? 'HEAD');
  const refs =
    opts.jetRefs === null && recorded?.jet_refs
      ? recorded.jet_refs.map((r) => ({ name: r.name, sha: resolveCommit(r.sha) }))
      : (opts.jetRefs ?? 'origin/port/main,cutover-ready')
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean)
          .map((name) => ({ name, sha: resolveCommit(name) }));

  const outputs = generate({ rev, jetRev, refs });
  const outDir = join(ROOT, OUT_REL);
  const triage = JSON.parse(outputs['summary.json']).triage;
  const unclassified = triage.items.unclassified + triage.tests.unclassified;
  for (const p of triage.problems) process.stderr.write(`triage: ${p}\n`);
  if (unclassified > 0) process.stderr.write(`triage: ${unclassified} unmatched rows have no class\n`);
  const triageOk = unclassified === 0 && triage.problems.length === 0;

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
      process.stderr.write('rust-parity-ledger: committed outputs are stale; rerun without --check\n');
    }
    if (differing.length > 0 || !triageOk) process.exit(1);
    process.stdout.write(`rust-parity-ledger: up to date (${rev}, jet ${jetRev}); every unmatched row classified\n`);
    return;
  }

  mkdirSync(outDir, { recursive: true });
  for (const name of OUTPUTS) writeFileSync(join(outDir, name), outputs[name]);
  process.stdout.write(`rust-parity-ledger: wrote ${OUT_REL}/ for ${rev} (jet ${jetRev})\n`);
}

// Imported by the fingerprint verifier for its lexer; only a direct run regenerates the ledger.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
