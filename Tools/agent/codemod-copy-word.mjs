#!/usr/bin/env node
// D-COPY-WORD1=A cutover codemod: prefix `~operand` -> `operand.copy()`.
// usage: node Tools/agent/codemod-copy-word.mjs [--write] (--all | <file>...)
//   --all  every tracked .jet file, the Rust test trees that embed Jet
//          programs, and the normative spec documents. Re-runnable: a tree
//          with no `~x` left reports rewrites=0.
// Prints every rewrite as `file:line: before => after` and `FLAG file:line:`
// for sites it refuses (left untouched for manual review).
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';

const write = process.argv.includes('--write');
const all = process.argv.includes('--all');
const files = all
  ? execFileSync('git', ['ls-files', '*.jet', 'tests/*.rs', 'crates/*/tests/*.rs', 'crates/*.rs', 'Docs/spec/spec.md', 'Docs/spec/reference/*.md'], { encoding: 'utf8' })
      .split('\n')
      .filter(Boolean)
  : process.argv.slice(2).filter((a) => !a.startsWith('--'));
const ident = /[A-Za-z0-9_]/;
const closer = { '(': ')', '[': ']', '{': '}' };
let total = 0;
let flagged = 0;

function balanced(s, i) {
  // s[i] is an opener; return index after its matching closer, or -1.
  const stack = [closer[s[i]]];
  let j = i + 1;
  let inStr = false;
  while (j < s.length && stack.length) {
    const c = s[j];
    if (c === '\n' && stack.length && s[i] !== '(' && s[i] !== '[' && s[i] !== '{') return -1;
    if (inStr) {
      if (c === '\\') { j += 2; continue; }
      if (c === '"') inStr = false;
      j++;
      continue;
    }
    if (c === '"') { inStr = true; j++; continue; }
    if (c in closer) stack.push(closer[c]);
    else if (c === ')' || c === ']' || c === '}') {
      if (stack[stack.length - 1] !== c) return -1;
      stack.pop();
    }
    j++;
  }
  return stack.length ? -1 : j;
}

function scanIdent(s, j) {
  while (j < s.length && ident.test(s[j])) j++;
  return j;
}

// Returns {end, prefix, endsTry, flag} for the unary operand starting at i.
function operand(s, i) {
  let j = i;
  let prefix = false;
  while ('^&-!*'.includes(s[j])) { prefix = true; j++; }
  const start = j;
  const c = s[j];
  if (c === undefined) return { flag: 'eof' };
  if (/[A-Za-z_]/.test(c)) {
    j = scanIdent(s, j);
    // struct literal head `Point{`
    if (/[A-Z]/.test(c) && s[j] === '{') {
      const k = balanced(s, j);
      if (k < 0) return { flag: 'unbalanced struct literal' };
      j = k;
    }
  } else if (c === '(' || c === '[' || c === '{') {
    const k = balanced(s, j);
    if (k < 0) return { flag: 'unbalanced' };
    j = k;
  } else if (c === '.' && /[A-Z]/.test(s[j + 1] || '')) {
    j = scanIdent(s, j + 1);
  } else {
    return { flag: `operand starts with ${JSON.stringify(c)}` };
  }
  if (j === start) return { flag: 'empty operand' };
  let endsTry = false;
  for (;;) {
    if (s[j] === '.' && /[A-Za-z_]/.test(s[j + 1] || '')) {
      j = scanIdent(s, j + 1);
      endsTry = false;
      continue;
    }
    if (s[j] === '.' && /[0-9]/.test(s[j + 1] || '')) {
      j = scanIdent(s, j + 1);
      endsTry = false;
      continue;
    }
    if (s[j] === '?' && s[j + 1] === '.' && /[A-Za-z_]/.test(s[j + 2] || '')) {
      j = scanIdent(s, j + 2);
      endsTry = false;
      continue;
    }
    if (s[j] === '(' || s[j] === '[') {
      const k = balanced(s, j);
      if (k < 0) return { flag: 'unbalanced postfix' };
      j = k;
      endsTry = false;
      continue;
    }
    if (s[j] === '?' && s[j + 1] !== '?' && s[j + 1] !== '.' && s[j + 1] !== '(') {
      j++;
      endsTry = true;
      continue;
    }
    break;
  }
  // refuse shapes the scanner cannot prove
  const rest = s.slice(j, j + 3);
  if (/^\s*\{/.test(s.slice(j, s.indexOf('\n', j) < 0 ? s.length : s.indexOf('\n', j))) && /[)\]]$/.test(s.slice(start, j))) {
    return { flag: 'call followed by `{` (trailing lambda?)' };
  }
  if (rest[0] === '<' && /[A-Za-z]/.test(s[j - 1])) return { flag: 'turbofish-like `<`' };
  return { end: j, prefix, endsTry };
}

// `~` that is not the Jet copy mark: JSON Pointer escapes, version ranges,
// store key separators, foreign-language generators (C, Octave), terminal
// escapes, and approximations (`~L1234`, `~half`). The diagnostic registry and
// the parser/formatter/Syntax sources that implement the mark are edited by
// hand; generated files are rewritten through their source.
const SKIP_FILES = new Set([
  'Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet',
  'crates/jet-codegen/src/Prelude/Diagnostics.jet',
  'crates/jet-foundation/src/TargetMachine.rs',
  'crates/jet-pkg-model/src/ComBind.rs',
  'crates/jet-pkg-model/src/OctaveBind.rs',
  'crates/jet-pkg-model/src/CBind.rs',
  'crates/jet-codegen/src/Prelude/Core/LazyTablePlan.rs',
  'crates/jet-store/src/lib.rs',
  'crates/jet-repl/src/Term.rs',
  'crates/jetpack/src/JetOS/generation_files.rs',
  'crates/jetpack-bin/tests/compiler_extension_e2e.rs',
  'crates/jet-foundation/src/Syntax/core_surface.rs',
  'crates/jet-foundation/src/Syntax/math_layout.rs',
  'crates/jet-lexer/src/Lexer/Scan.rs',
  'crates/jet-lexer/src/Lexer/Tokens.rs',
  'crates/jet-parser/src/Parser/Expressions/binary_unary.rs',
  'crates/jet-parser/src/Parser/Expressions/providers.rs',
  'crates/jet-parser/src/Parser/Expressions/ptr_tuple.rs',
  'crates/jet-parser/src/Formatter/Expressions.rs',
  // Hand-owned: the parser that teaches the retired mark (E0941).
  'Compiler/JetParser/Source/Parser/Expressions.jet',
  // The E0941 fixture keeps the retired spelling it teaches.
  'tests/ui/copy_sigil_retired_e0941.jet',
]);
const SKIP_TEXT = [
  ['Core/encoding/json.jet', 'escape `~` and `/`'],
  ['Core/encoding/json.jet', 'in a JSON Pointer must be'],
  ['Examples/features/serde/json_pointer.jet', 'spells `~` inside one step'],
  ['Jetpack/Provider/Source/Registries/ScriptRegistryProvider.jet', 'upper bound of `~`'],
  ['Jetpack/Store/Source/ArtifactStore/Identity.jet', 'LTO entries join with `~`'],
  ['crates/jet-cli/src/CLI.rs', '~half'],
  ['crates/jet-foundation/src/AST/types.rs', 'was spelled `~T`'],
  ['crates/jet-foundation/src/Syntax/dictionary.rs', 'retired copy mark'],
  ['crates/jet-foundation/src/Syntax.rs', 'teaches E0941'],
  ['Docs/spec/spec.md', 'the retired prefix `~x` teaches E0941'],
  ['Docs/spec/spec.md', 'The plan rows use `+`, `-`, and `~`'],
];
const SKIP_OPERAND = /^L\d+$/;

const CODE = 0;
const COMMENT = 1;
const STRING = 2;
const RAW = 3;
// Rust string literal: embedded Jet program or diagnostic text.
const JETSTR = 4;

// Per-character lexical context of a Jet source file: code (including string
// interpolation holes), comments, quoted text, and backtick raw text.
function jetContexts(s) {
  const ctx = new Uint8Array(s.length);
  // stack of frames: {kind: 'code', depth} | {kind: 'str', triple}
  const stack = [{ kind: 'code', depth: 0 }];
  let i = 0;
  while (i < s.length) {
    const top = stack[stack.length - 1];
    const c = s[i];
    if (top.kind === 'code') {
      if (c === '/' && s[i + 1] === '/') {
        while (i < s.length && s[i] !== '\n') ctx[i++] = COMMENT;
        continue;
      }
      if (c === '/' && s[i + 1] === '*') {
        let depth = 0;
        while (i < s.length) {
          if (s[i] === '/' && s[i + 1] === '*') { depth++; ctx[i++] = COMMENT; ctx[i++] = COMMENT; continue; }
          if (s[i] === '*' && s[i + 1] === '/') { depth--; ctx[i++] = COMMENT; ctx[i++] = COMMENT; if (!depth) break; continue; }
          ctx[i++] = COMMENT;
        }
        continue;
      }
      if (c === '`') {
        ctx[i++] = RAW;
        while (i < s.length && s[i] !== '`') ctx[i++] = RAW;
        if (i < s.length) ctx[i++] = RAW;
        continue;
      }
      if (c === '"') {
        const triple = s[i + 1] === '"' && s[i + 2] === '"';
        const n = triple ? 3 : 1;
        for (let k = 0; k < n; k++) ctx[i++] = STRING;
        stack.push({ kind: 'str', triple });
        continue;
      }
      if (c === '{') top.depth++;
      if (c === '}') {
        if (top.depth === 0 && stack.length > 1) {
          stack.pop();
          ctx[i++] = STRING;
          continue;
        }
        top.depth--;
      }
      ctx[i++] = CODE;
      continue;
    }
    // inside quoted text
    if (c === '\\') { ctx[i++] = STRING; if (i < s.length) ctx[i++] = STRING; continue; }
    if (c === '{' && s[i + 1] === '{') { ctx[i++] = STRING; ctx[i++] = STRING; continue; }
    if (c === '}' && s[i + 1] === '}') { ctx[i++] = STRING; ctx[i++] = STRING; continue; }
    if (c === '{') { ctx[i++] = STRING; stack.push({ kind: 'code', depth: 0 }); continue; }
    if (c === '"' && (!top.triple || (s[i + 1] === '"' && s[i + 2] === '"'))) {
      const n = top.triple ? 3 : 1;
      for (let k = 0; k < n; k++) ctx[i++] = STRING;
      stack.pop();
      continue;
    }
    ctx[i++] = STRING;
  }
  return ctx;
}

// Rust source: `~` is never Rust syntax, so comments and string literals
// (embedded Jet programs, diagnostic text) are the only sites. Strings count
// as Jet code; comments as prose.
function rustContexts(s) {
  const ctx = new Uint8Array(s.length);
  let i = 0;
  while (i < s.length) {
    const c = s[i];
    if (c === '/' && s[i + 1] === '/') {
      while (i < s.length && s[i] !== '\n') ctx[i++] = COMMENT;
      continue;
    }
    if (c === '/' && s[i + 1] === '*') {
      while (i < s.length && !(s[i] === '*' && s[i + 1] === '/')) ctx[i++] = COMMENT;
      ctx[i++] = COMMENT; ctx[i++] = COMMENT;
      continue;
    }
    if (c === "'" && s[i + 2] === "'") { ctx[i] = ctx[i + 1] = ctx[i + 2] = RAW; i += 3; continue; }
    if (c === 'r' && (s[i + 1] === '#' || s[i + 1] === '"') && !ident.test(s[i - 1] || '')) {
      let j = i + 1;
      let hashes = 0;
      while (s[j] === '#') { hashes++; j++; }
      if (s[j] === '"') {
        const close = '"' + '#'.repeat(hashes);
        const end = s.indexOf(close, j + 1);
        const stop = end < 0 ? s.length : end + close.length;
        for (; i < stop; i++) ctx[i] = JETSTR;
        continue;
      }
    }
    if (c === '"') {
      ctx[i++] = JETSTR;
      while (i < s.length && s[i] !== '"') {
        if (s[i] === '\\') ctx[i++] = JETSTR;
        ctx[i++] = JETSTR;
      }
      ctx[i++] = JETSTR;
      continue;
    }
    ctx[i++] = RAW;
  }
  return ctx;
}

for (const file of files) {
  if (SKIP_FILES.has(file)) continue;
  const src = fs.readFileSync(file, 'utf8');
  let out = '';
  let last = 0;
  let changed = 0;
  const ctx = file.endsWith('.jet') ? jetContexts(src) : file.endsWith('.md') ? new Uint8Array(src.length).fill(COMMENT) : rustContexts(src);
  for (let i = 0; i < src.length; i++) {
    if (src[i] !== '~') continue;
    const prev = src[i - 1] || '';
    const next = src[i + 1] || '';
    const where = ctx[i];
    const lineNo = src.slice(0, i).split('\n').length;
    const lineStart = src.lastIndexOf('\n', i) + 1;
    const lineEnd = src.indexOf('\n', i) < 0 ? src.length : src.indexOf('\n', i);
    const line = src.slice(lineStart, lineEnd).trim();
    if (SKIP_TEXT.some(([f, t]) => f === file && line.includes(t))) continue;
    // a lone `~` quoted as the copy mark in prose or teaching text
    if (where !== CODE && prev === '`' && next === '`') {
      out += src.slice(last, i) + '.copy()';
      last = i + 1;
      changed++;
      console.log(`${file}:${lineNo}: \`~\` => \`.copy()\``);
      continue;
    }
    if (next === '|' || next === '~' || next === '>' || next === '=' || prev === '~') continue;
    if (/\s/.test(next) || next === '' || /[0-9]/.test(next) || next === '"' || next === '`' || next === "'" || next === '/' || next === ')' || next === ',' || next === ']' || next === '.' && !/[A-Z]/.test(src[i + 2] || '')) {
      if (where === CODE) {
        flagged++;
        console.log(`FLAG ${file}:${lineNo}: [code ~ before ${JSON.stringify(next)}] ${line}`);
      }
      continue;
    }
    if (ident.test(prev) || prev === ')' || prev === ']') continue;
    // Inside text: only the backticked teaching form `~x` is Jet; anything
    // else (embedded programs, foreign source) is reviewed by hand.
    if ((where === STRING || where === RAW) && prev !== '`') {
      flagged++;
      console.log(`FLAG ${file}:${lineNo}: [inside text] ${line}`);
      continue;
    }
    const r = operand(src, i + 1);
    if (!r.flag && SKIP_OPERAND.test(src.slice(i + 1, r.end))) continue;
    if (r.flag) {
      flagged++;
      console.log(`FLAG ${file}:${lineNo}: [${r.flag}] ${line}`);
      continue;
    }
    const original = src.slice(i + 1, r.end);
    let body = original;
    // `~(a.b[c])` -> `a.b[c].copy()` when the parenthesized operand is a plain postfix chain
    if (body.startsWith('(') && body.endsWith(')') && balanced(body, 0) === body.length) {
      const inner = body.slice(1, -1);
      const ri = operand(inner, 0);
      if (!ri.flag && ri.end === inner.length && !ri.prefix && !ri.endsTry) body = inner;
    }
    const wrap = r.prefix || r.endsTry;
    const repl = wrap ? `(${body}).copy()` : `${body}.copy()`;
    out += src.slice(last, i) + repl;
    last = r.end;
    i = r.end - 1;
    changed++;
    console.log(`${file}:${lineNo}: ~${original} => ${repl}`);
  }
  out += src.slice(last);
  total += changed;
  if (write && changed) fs.writeFileSync(file, out);
}
console.error(`rewrites=${total} flagged=${flagged}`);
