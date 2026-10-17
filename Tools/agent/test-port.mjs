#!/usr/bin/env node
// Transitional Rust-test porter. Removal point: M7, when tests/**/*.rs is empty.
// This is migration tooling, not the test harness: Tools/testkit is the Jet kit.
// node Tools/agent/test-port.mjs --census OUT.tsv [--inventory TESTS.tsv]
// node Tools/agent/test-port.mjs --emit tests/ownership.rs::NAME --bless-from JET [--dry-run]
// --emit FILE::* checks every function, keeps going, and reports hand-port refusals.
// --attempt FILE.tsv selects human-PORT overrides: one FILE::FN per first column
// (optional "function" header/comments). With --emit, selection is its intersection.
// Only census retirement is bypassed; original assertions and rule G still apply.
// --analyze OUT.tsv [--attempt FILE.tsv] [--slice-map FILES.tsv] is static:
// all parser/model blockers, no reference execution or fixture writes. The
// slice map has slice/file columns; classes escape literal semicolons as \;.
// A clean model is still runtime-unknown, never a reference-run PASS.
// --draft OUT [--attempt HAND.tsv] writes incomplete, assertion-annotated
// historical drafts ONLY outside the checkout. DRAFTED is never CONVERTED.
// Unsupported syntax/API facts and failed original assertions are HAND-PORT,
// not permission to weaken a test. RETIRED rows name Rust-only subjects.
// --report FILE records each decision; --exclusions FILE lists raw #[test]
// occurrences hidden in strings/comments. Source matrices retain every row.
// Mixed functions retain Jet assertions and report only Rust-derived assertions
// as retired. Runtime receipts use the ref artifact; all-tier origins must agree.
// --timeout SEC defaults to600s. Reference process failures are ERROR, not
// HAND-PORT; every invocation records elapsed_ms and any ERROR makes exit nonzero.
// Exit-101 receipts retain child stderr; reasons include its first20 lines.
// Until M3/W2, plain UI blessing mirrors the kit's public check/render rules;
// W2 replaces this mirror with testkit ui --bless. No new fixture directives.
import { readFileSync, readdirSync, existsSync, realpathSync, mkdirSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { dirname, basename, resolve, relative, join, extname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const refuse = (message) => { throw new Error(message); };

function decodeString(raw) {
  if (/^b?r#*"/u.test(raw)) return raw.slice(raw.indexOf('"') + 1, raw.lastIndexOf('"'));
  return raw.slice(1, -1).replace(/\\(?:u\{([\da-fA-F_]+)\}|x([\da-fA-F]{2})|([nrt0\\"'])|\r?\n\s*)/gu,
    (_, unicode, hex, short) => unicode ? String.fromCodePoint(parseInt(unicode.replaceAll('_', ''), 16)) : hex ? String.fromCharCode(parseInt(hex, 16)) : short ? ({ n: '\n', r: '\r', t: '\t', 0: '\0', '\\': '\\', '"': '"', "'": "'" })[short] : '');
}

// Preserve byte offsets and lines while hiding comments and all string bodies.
// In particular, Rust snippets containing #[test] are not physical test functions.
export function lexRust(text) {
  const out = [];
  let i = 0, line = 1;
  const advance = (end) => { line += (text.slice(i, end).match(/\n/gu) ?? []).length; i = end; };
  while (i < text.length) {
    if (/\s/u.test(text[i])) { advance(i + 1); continue; }
    if (text.startsWith('//', i)) { const end = text.indexOf('\n', i); advance(end < 0 ? text.length : end); continue; }
    if (text.startsWith('/*', i)) {
      let depth = 1, end = i + 2;
      while (depth && end < text.length) {
        if (text.startsWith('/*', end)) { depth++; end += 2; }
        else if (text.startsWith('*/', end)) { depth--; end += 2; }
        else end++;
      }
      if (depth) refuse(`unterminated comment at line ${line}`);
      advance(end); continue;
    }
    const start = i, startLine = line;
    const raw = text.slice(i).match(/^(?:b)?r(#{0,255})"/u);
    let kind = 'punct', value;
    if (raw) {
      const end = text.indexOf(`"${raw[1]}`, i + raw[0].length);
      if (end < 0) refuse(`unterminated raw string at line ${line}`);
      advance(end + raw[1].length + 1); kind = 'string';
    } else if (text[i] === '"' || (text[i] === "'" && /^'(?:\\.|[^'\\\n])'/u.test(text.slice(i)))) {
      const quote = text[i]; let end = i + 1;
      while (end < text.length && text[end] !== quote) { if (text[end] === '\\') end++; end++; }
      if (end >= text.length) refuse(`unterminated literal at line ${line}`);
      advance(end + 1); kind = 'string';
    } else {
      const word = text.slice(i).match(/^(?:[A-Za-z_][A-Za-z_0-9]*|\d+(?:_\d+)*(?:\.\d+)?)/u);
      const operator = text.slice(i).match(/^(?:::|==|!=|&&|\|\||>=|<=|->|=>|\.\.=|\.\.)/u);
      if (word) { advance(i + word[0].length); kind = /^\d/u.test(word[0]) ? 'number' : 'word'; }
      else advance(i + (operator?.[0].length ?? 1));
    }
    const rawText = text.slice(start, i);
    value = kind === 'string' ? decodeString(rawText) : rawText;
    out.push({ value, raw: rawText, kind, start, end: i, line: startLine, endLine: line });
  }
  return out;
}

function closeAt(tokens, index) {
  const pairs = { '(': ')', '[': ']', '{': '}' }, stack = [pairs[tokens[index]?.value]];
  if (!stack[0]) refuse('expected opening delimiter');
  for (let i = index + 1; i < tokens.length; i++) {
    const token = tokens[i];
    if (token.kind === 'string') continue;
    if (Object.hasOwn(pairs, token.value)) stack.push(pairs[token.value]);
    else if ([')', ']', '}'].includes(token.value)) {
      if (stack.pop() !== token.value) refuse(`unbalanced delimiter at line ${token.line}`);
      if (!stack.length) return i;
    }
  }
  refuse(`unclosed delimiter at line ${tokens[index].line}`);
}

export function functionsIn(text, file = '') {
  const tokens = lexRust(text), result = [];
  let test = false, attributeStart = 0;
  for (let i = 0; i < tokens.length; i++) {
    if (tokens[i].kind === 'punct' && tokens[i].value === '#' && tokens[i + 1]?.value === '[') {
      const end = closeAt(tokens, i + 1);
      if (tokens[i + 2]?.value === 'test' && end === i + 3) { test = true; attributeStart = i; }
      i = end; continue;
    }
    if (tokens[i].kind !== 'word' || tokens[i].value !== 'fn' || tokens[i + 1]?.kind !== 'word') continue;
    const name = tokens[i + 1].value;
    let open = i + 2;
    while (open < tokens.length && !['{', ';'].includes(tokens[open].value)) open++;
    if (tokens[open]?.value !== '{') { test = false; continue; }
    const end = closeAt(tokens, open);
    result.push({ file, name, test, line: tokens[test ? attributeStart : i].line, endLine: tokens[end].endLine,
      tokens: tokens.slice(open + 1, end), lexicalTokens: tokens, signature: tokens.slice(i + 2, open),
      bodyOffset: tokens[open].end, body: text.slice(tokens[open].end, tokens[end].start) });
    test = false; i = end;
  }
  return result;
}

function rustFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? rustFiles(path) : entry.isFile() && extname(path) === '.rs' ? [path] : [];
  });
}

function topLevelSplit(tokens, separator) {
  const parts = []; let start = 0;
  for (let i = 0; i < tokens.length; i++) {
    if (tokens[i].kind === 'string') continue;
    if (['(', '[', '{'].includes(tokens[i].value)) { i = closeAt(tokens, i); continue; }
    if (tokens[i].value === separator) { parts.push(tokens.slice(start, i)); start = i + 1; }
  }
  parts.push(tokens.slice(start)); return parts;
}

// Blocks are statement boundaries even without semicolons. Keep if/else
// chains together: a failed condition must not authorize either branch.
function topLevelStatements(tokens) {
  const parts = []; let start = 0;
  for (let i = 0; i < tokens.length; i++) {
    if (tokens[i].kind === 'string') continue;
    if (['(', '[', '{'].includes(tokens[i].value)) {
      let end = closeAt(tokens, i);
      if (tokens[i].value === '{' && (i === start || ['if', 'for', 'while', 'loop', 'match', 'unsafe'].includes(tokens[start]?.value))) {
        while (tokens[end + 1]?.value === 'else' && tokens[end + 1]?.kind !== 'string') {
          let open = end + 2;
          while (open < tokens.length && (tokens[open].value !== '{' || tokens[open].kind === 'string')) open++;
          if (open >= tokens.length) break;
          end = closeAt(tokens, open);
        }
        parts.push(tokens.slice(start, end + 1)); start = end + 1;
      }
      i = end; continue;
    }
    if (tokens[i].value === ';') { if (i > start) parts.push(tokens.slice(start, i)); start = i + 1; }
  }
  if (start < tokens.length) parts.push(tokens.slice(start));
  return parts;
}

// Rust-emitter facts die at cutover, but their neighboring Jet facts do not.
// Trace only explicit Rust provenance; unknown syntax still reaches the closed
// parser and is refused. Never supply pretend generated Rust to the evaluator.
export function retireRustAssertions(fn) {
  const retired = [], tokens = fn.tokens ?? lexRust(fn.body);
  let jetAssertions = 0;
  const origin = (list, names) => {
    const words = list.filter((token) => token.kind !== 'string').map((token) => token.value);
    if (words.some((word) => ['Command', 'include_str', 'write'].includes(word)) &&
      list.some((token) => token.kind === 'string' && (token.value === 'rustc' || /\.rs$/u.test(token.value)))) return true;
    for (let i = 0; i < list.length; i++) {
      const token = list[i];
      if (token.kind === 'string') continue;
      if (token.value === '|' && list[i + 1]?.kind === 'word') {
        const end = list.findIndex((candidate, j) => j > i && candidate.value === '|');
        if (end >= 0) {
          const inner = new Set(names);
          for (const param of list.slice(i + 1, end)) if (param.kind === 'word') inner.delete(param.value);
          return origin(list.slice(end + 1), inner);
        }
      }
      if (['(', '[', '{'].includes(token.value)) {
        const end = closeAt(list, i);
        if (origin(list.slice(i + 1, end), names)) return true;
        i = end; continue;
      }
      if ((token.value === '.' && list[i + 1]?.value === 'rust') || (names.has(token.value) && !['.', '::'].includes(list[i - 1]?.value)) || /^(?:strip_vetted\w*|add_generated_rust)$/u.test(token.value)) return true;
    }
    return false;
  };
  const prune = (list, names) => {
    if (list[0]?.kind !== 'string' && list[0]?.value === '(' && closeAt(list, 0) === list.length - 1) {
      const inner = prune(list.slice(1, -1), names);
      return inner.length ? [list[0], ...inner, list.at(-1)] : [];
    }
    for (const operator of ['||', '&&']) {
      const parts = topLevelSplit(list, operator);
      if (parts.length > 1) {
        const kept = parts.map((part) => prune(part, names)).filter((part) => part.length);
        const delimiter = list.find((token) => token.value === operator);
        return kept.flatMap((part, i) => i ? [delimiter, ...part] : part);
      }
    }
    return origin(list, names) ? [] : list;
  };
  const process = (input, names) => {
    const kept = [];
    for (let start = 0; start < input.length;) {
      let end = start;
      const control = ['for', 'if'].includes(input[start].value);
      while (end < input.length) {
        const token = input[end];
        if (token.kind !== 'string' && ['(', '[', '{'].includes(token.value)) {
          if (control && token.value === '{') break;
          end = closeAt(input, end);
        } else if (token.kind !== 'string' && token.value === ';') break;
        end++;
      }
      if (end >= input.length) { kept.push(...input.slice(start)); break; }
      if (control && input[end].value === '{') {
        const close = closeAt(input, end), header = input.slice(start, end), innerNames = new Set(names);
        if (header[0].value === 'for') {
          const inAt = header.findIndex((token) => token.value === 'in');
          if (origin(header.slice(inAt + 1), names)) for (const token of header.slice(1, inAt)) if (token.kind === 'word') innerNames.add(token.value);
        }
        const body = process(input.slice(end + 1, close), innerNames);
        const rustAvailability = /^if (?:common :: )?have_rustc \( \)$/u.test(header.map((token) => token.raw).join(' '));
        if (body.length) {
          if (rustAvailability && !header.some((token) => token.value === '!')) kept.push(...body);
          else kept.push(...header, input[end], ...body, input[close]);
        }
        start = close + 1; continue;
      }
      const statement = input.slice(start, end), semicolon = input[end];
      if (statement[0]?.value === 'let') {
        const equal = statement.findIndex((token) => token.value === '='), colon = statement.findIndex((token) => token.value === ':');
        const bindings = statement.slice(1, colon >= 0 && colon < equal ? colon : equal).filter((token) => token.kind === 'word' && token.value !== 'mut').map((token) => token.value);
        const rust = equal >= 0 && origin(statement.slice(equal + 1), names);
        for (const name of bindings) { if (rust) names.add(name); else names.delete(name); }
        if (!rust) {
          if (statement.some((token) => token.kind === 'word' && /^(?:expect_err|build_and_run|dev_run|run_interpreted|assert_tiers_agree)$/u.test(token.value))) jetAssertions++;
          kept.push(...statement, semicolon);
        }
      } else if (['assert', 'assert_eq', 'assert_ne'].includes(statement[0]?.value) && statement[1]?.value === '!' && statement[2]?.value === '(') {
        const close = closeAt(statement, 2), args = topLevelSplit(statement.slice(3, close), ',');
        const count = statement[0].value === 'assert' ? 1 : 2;
        const rust = args.slice(0, count).some((arg) => origin(arg, names));
        let predicate = args[0];
        if (rust) {
          retired.push({ line: statement[0].line, reason: 'generated-Rust assertion retires with the Rust emitter' });
          predicate = count === 1 ? prune(predicate, names) : [];
        }
        if (predicate.length) {
          jetAssertions++;
          const comma = statement.find((token) => token.kind !== 'string' && token.value === ',');
          kept.push(...statement.slice(0, 3), ...predicate,
            ...args.slice(1).flatMap((arg) => arg.length ? [comma, ...arg] : []), ...statement.slice(close), semicolon);
        }
      } else if (!origin(statement, names)) {
        const code = statement.filter((token) => token.kind !== 'string').map((token) => token.value).join(' ');
        if (/\bexpect_err\b|\b(?:build_and_run|dev_run|run_interpreted|assert_tiers_agree)\b/u.test(code)) jetAssertions++;
        kept.push(...statement, semicolon);
      }
      start = end + 1;
    }
    return kept;
  };
  const remaining = process(tokens, new Set());
  return { tokens: remaining, retired, jetAssertions };
}

// Recursion already covers the inventory; module traversal additionally handles
// #[path] includes outside the conventional tree and fails on unresolved modules.
export function collectFunctions(root = ROOT, inventory = null) {
  const files = new Set(rustFiles(join(root, 'tests')).map((file) => realpathSync(file)));
  if (inventory) for (const row of readFileSync(inventory, 'utf8').trimEnd().split('\n').slice(1)) {
    const file = resolve(root, row.split('\t')[0]);
    if (!existsSync(file)) refuse(`inventory file missing: ${file}`);
    files.add(realpathSync(file));
  }
  const modules = [], all = [], excluded = [];
  for (const file of files) {
    const shown = relative(root, file), text = readFileSync(file, 'utf8'), tokens = lexRust(text);
    const physicalAttributes = new Set(tokens.filter((token, i) => token.kind === 'punct' && token.value === '#' && tokens[i + 1]?.value === '[' && tokens[i + 2]?.value === 'test' && tokens[i + 3]?.value === ']').map((token) => token.start));
    for (const match of text.matchAll(/#\[test\]/gu)) {
      if (!physicalAttributes.has(match.index)) excluded.push({ file: shown, line: 1 + (text.slice(0, match.index).match(/\n/gu) ?? []).length, reason: 'inside string or comment' });
    }
    try { all.push(...functionsIn(text, shown)); }
    catch (error) { refuse(`${shown}: ${error.message}`); }
    let explicit = null;
    for (let i = 0; i < tokens.length; i++) {
      if (tokens[i].kind === 'punct' && tokens[i].value === '#' && tokens[i + 1]?.value === '[') {
        const end = closeAt(tokens, i + 1);
        if (tokens[i + 2]?.value === 'path' && tokens[i + 3]?.value === '=' && tokens[i + 4]?.kind === 'string') explicit = tokens[i + 4].value;
        i = end; continue;
      }
      if (tokens[i].kind !== 'word' || tokens[i].value !== 'mod' || tokens[i + 2]?.value !== ';') continue;
      const name = tokens[i + 1].value;
      const bases = [dirname(file), join(dirname(file), basename(file, '.rs'))];
      const candidates = explicit ? [resolve(dirname(file), explicit)] : bases.flatMap((base) => [join(base, `${name}.rs`), join(base, name, 'mod.rs')]);
      const found = candidates.find(existsSync);
      if (!found) refuse(`unresolved mod ${name} in ${shown}`);
      files.add(realpathSync(found)); modules.push({ file: shown, module: name, target: relative(root, found) }); explicit = null;
    }
  }
  for (const fn of all) fn.moduleBindings = modules.filter(module => module.file === fn.file);
  const tests = all.filter((fn) => fn.test).sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
  const keys = new Set();
  for (const fn of tests) { const key = `${fn.file}:${fn.line}`; if (keys.has(key)) refuse(`duplicate physical function: ${key}`); keys.add(key); }
  return { files: [...files].map((file) => relative(root, file)).sort(), functions: tests, helpers: all.filter((fn) => !fn.test), modules, excluded };
}

const probes = {
  run: /\b(?:build_and_run(?:_with_cwd)?|dev_run|run_interpreted|run_interpreter_(?:checked_)?bundle|assert_tiers_agree|jet_cli_output_with_stdin)\b/u,
  cli: /CARGO_BIN_EXE_(?:jet|jetpack|jetos)|\b(?:jet_bin|jetpack_bin|jetos_bin|resolve_or_build_bin)\b/u,
  compile: /\bjet\s*::\s*(?:compile\w*|check\w*|render_diagnostics)\b/u,
  rust: /\.\s*rust\b|\brustc\b|\b(?:add_generated_rust|strip_vetted\w*)\b/u,
  internal: /\b(?:jet\w*|jetpack)\s*::\s*(?:[A-Z]\w*|parse\w*|lex\w*|lower\w*)/u,
  scan: /\b(?:read_dir|read_to_string|include_str|walkdir|WalkDir|CorpusPolicy)\b|git ls-files/u,
};

const helperIndexes = new WeakMap();
function helperIndex(helpers) {
  if (!helperIndexes.has(helpers)) {
    const index = new Map();
    for (const helper of helpers) {
      if (!index.has(helper.name)) index.set(helper.name, []);
      index.get(helper.name).push(helper);
    }
    helperIndexes.set(helpers, index);
  }
  return helperIndexes.get(helpers);
}

export function classify(fn, helpers = []) {
  // Resolve calls from each function's lexical context. A common::foo call
  // must not pick an unrelated foo in another part file or inside a string.
  const index = helperIndex(helpers), pending = [fn], seen = new Set([fn]);
  let body = '', code = '';
  const literals = [];
  while (pending.length) {
    const caller = pending.pop(), tokens = caller.tokens ?? lexRust(caller.body);
    body += '\n' + tokens.map((token) => token.kind === 'string' ? JSON.stringify(token.value) : token.raw).join(' ');
    code += '\n' + tokens.map((token) => token.kind === 'string' ? '__literal__' : token.raw).join(' ');
    literals.push(...tokens.filter((token) => token.kind === 'string').map((token) => token.value));
    for (let i = 0; i < tokens.length; i++) {
      if (tokens[i].kind !== 'word' || tokens[i + 1]?.value !== '(' || tokens[i - 1]?.value === '.') continue;
      let qualifier = '';
      for (let j = i - 1; tokens[j]?.value === '::' && tokens[j - 1]?.kind === 'word'; j -= 2) qualifier = `${tokens[j - 1].value}::${qualifier}`;
      for (const helper of index.get(tokens[i].value) ?? []) {
        const local = !qualifier && helper.file === caller.file;
        const common = qualifier === 'common::' && helper.file.startsWith('tests/common/');
        const module = qualifier && qualifier.split('::').filter(Boolean).at(-1) === basename(helper.file, '.rs');
        const sibling = !qualifier && dirname(helper.file) === dirname(caller.file) && dirname(caller.file) !== 'tests';
        if (seen.has(helper) || (!local && !common && !module && !sibling)) continue;
        seen.add(helper); pending.push(helper);
      }
    }
  }
  const surfaces = Object.entries(probes).filter(([name, probe]) => probe.test(code)
    || (name === 'cli' && literals.some((literal) => /^CARGO_BIN_EXE_(?:jet|jetpack|jetos)$/u.test(literal)))
    || (name === 'rust' && literals.includes('rustc'))
    || (name === 'scan' && literals.includes('git ls-files'))).map(([name]) => name);
  const suite = fn.file.replace(/^tests\//u, '').replace(/\.rs$/u, '').replace(/\//gu, '_');
  let disposition, target;
  const retirement = retireRustAssertions(fn);
  const rustOnly = surfaces.includes('rust') && !retirement.jetAssertions && !surfaces.includes('run') && !surfaces.includes('cli');
  const rustLaw = /selfhost_(?:parser|terminators)|suite_membership|ban_bare_panic|ban_jetpack_unwrap_expect/u.test(fn.file);
  const jetLaw = !rustLaw && /corpus_policy|diagnostic_(?:gate|pages)|(?:ledger|registry)_.*(?:policy|coverage)|case_law/u.test(`${fn.file} ${fn.name}`);
  if (jetLaw) { disposition = 'guard'; target = `tests/guards/${suite}.jet#${fn.name}`; }
  else if (surfaces.includes('compile') && /\.\s*(?:edit|safety|applicability)\b/u.test(code)) { disposition = 'CLI case'; target = `tests/cases/${suite}/${fn.name}.case`; }
  else if (surfaces.includes('run')) { disposition = 'golden'; target = `tests/run/${suite}/${fn.name}.jet`; }
  else if (surfaces.includes('cli')) { disposition = 'CLI case'; target = `tests/cases/${suite}/${fn.name}.case`; }
  else if (rustOnly || rustLaw || (surfaces.includes('scan') && /\.rs["']/u.test(body) && !/\.jet["']|Docs\//u.test(body))) { disposition = 'retire-with-Rust'; target = 'none: Rust emitter, crate internals, Rust law or cargo partition'; }
  else if (surfaces.includes('compile')) { disposition = 'UI'; target = `tests/${/\.\s*lints\b/u.test(code) ? 'ui_lint' : 'ui'}/${suite}/${fn.name}${/\bcompile_with_path\b/u.test(code) ? '/' : '.jet'}`; }
  else if (surfaces.includes('scan') && !surfaces.includes('internal')) { disposition = 'guard'; target = `tests/guards/${suite}.jet#${fn.name}`; }
  else { disposition = 'retire-with-Rust'; target = 'none: Rust crate implementation or harness unit'; }
  if (retirement.retired.length && disposition !== 'retire-with-Rust') disposition += '+retire-rust-asserts';
  return { ...fn, helpers, surfaces: surfaces.join(','), disposition, target };
}

export function censusTsv(census) {
  return 'file\tfn\tlines\tsurfaces\tdisposition\ttarget\n' + census.functions.map((fn) => {
    const row = classify(fn, census.helpers);
    return [row.file, row.name, `${row.line}-${row.endLine}`, row.surfaces || '-', row.disposition, row.target].join('\t');
  }).join('\n') + '\n';
}

// A deliberately closed Rust expression subset. Unknown syntax, unknown calls,
// unmodeled control flow and remaining Rust API facts are hand-port, NEVER
// assumed true. Parse the complete function before invoking or writing anything.
class Parser {
  constructor(tokens) { this.tokens = tokens; this.i = 0; }
  peek(value) { return this.tokens[this.i]?.kind !== 'string' && this.tokens[this.i]?.value === value; }
  take() { return this.tokens[this.i++] ?? refuse('unexpected end of expression'); }
  need(value) { if (!this.peek(value)) refuse(`expected ${value}, got ${this.tokens[this.i]?.raw ?? 'end'}`); return this.take(); }
  group(open = '(', close = ')') {
    this.need(open); const args = [];
    while (!this.peek(close)) { args.push(this.expr()); if (!this.peek(',')) break; this.take(); }
    this.need(close); return args;
  }
  expr(min = 0) {
    const start = this.tokens[this.i]; let node;
    if (this.peek('!') || this.peek('&') || this.peek('*')) { const op = this.take().value; node = { type: 'unary', op, arg: this.expr(9) }; }
    else if (this.peek('|') || this.peek('||')) {
      const zero = this.take().value === '||';
      const param = zero ? '_' : this.take().value;
      if (!zero) this.need('|');
      let body;
      if (this.peek('{')) { this.take(); body = this.expr(); if (this.peek(';')) this.take(); this.need('}'); }
      else body = this.expr();
      node = { type: 'closure', param, body };
    } else if (this.peek('(')) { const args = this.group(); node = args.length === 1 ? args[0] : { type: 'array', args }; }
    else if (this.peek('[')) node = { type: 'array', args: this.group('[', ']') };
    else {
      const token = this.take();
      if (token.kind === 'string') node = { type: 'literal', value: token.value };
      else if (token.kind === 'number') {
        const value = Number(token.value.replaceAll('_', ''));
        if (!Number.isSafeInteger(value)) refuse('numeric assertion is outside the exact integer subset');
        node = { type: 'literal', value };
      }
      else if (['true', 'false'].includes(token.value)) node = { type: 'literal', value: token.value === 'true' };
      else if (token.kind === 'word') {
        let name = token.value;
        while (this.peek('::')) { this.take(); name += `::${this.take().value}`; }
        if (this.peek('!')) { this.take(); name += '!'; }
        node = this.peek('(') ? { type: 'call', name, args: this.group(), line: token.line, offset: token.start } : { type: 'variable', name, line: token.line, offset: token.start };
      } else refuse(`unmodeled token ${token.raw} at line ${token.line}`);
    }
    while (this.i < this.tokens.length) {
      if (this.peek('.')) {
        this.take(); const member = this.take(), name = member.value;
        node = this.peek('(') ? { type: 'method', object: node, name, args: this.group(), line: member.line, offset: member.start } : { type: 'field', object: node, name, line: member.line, offset: member.start };
      } else if (this.peek('[')) { const [index] = this.group('[', ']'); node = { type: 'index', object: node, index }; }
      else {
        const op = this.tokens[this.i]?.value;
        const precedence = { '||': 1, '&&': 2, '|': 3, '==': 4, '!=': 4, '>': 4, '<': 4, '>=': 4, '<=': 4 }[op];
        if (!precedence || precedence < min) break;
        this.take(); node = { type: 'binary', op, left: node, right: this.expr(precedence + 1) };
      }
    }
    node.line ??= start?.line; node.offset ??= start?.start;
    return node;
  }
  binding() {
    if (this.peek('&')) this.take();
    return this.peek('(') ? this.group().map((node) => node.type === 'variable' ? node.name : refuse('unsupported binding')) : [this.take().value];
  }
  statements(stop = null) {
    const result = [];
    while (this.i < this.tokens.length && (!stop || !this.peek(stop))) {
      if (this.peek('for')) {
        this.take(); const names = this.binding(); this.need('in');
        const iterable = this.expr(); this.need('{');
        const statements = this.statements('}'); this.need('}');
        result.push({ type: 'loop', names, iterable, statements });
        continue;
      }
      if (this.peek('let')) {
        this.take(); if (this.peek('mut')) this.take();
        const names = this.binding();
        if (this.peek(':')) {
          this.take();
          while (!this.peek('=')) this.take();
        }
        this.need('='); result.push({ names, expr: this.expr() });
      } else result.push({ expr: this.expr() });
      this.need(';');
    }
    return result;
  }
}

// Input paths are real scratch files, never simulated command results. Keep
// their manifest so the same project can be restaged at its final case path.
class InputPath {
  constructor(project, path = '') { this.project = project; this.path = path; }
  disk() { return join(this.project.directory, this.path); }
  child(name) {
    if (typeof name !== 'string' || name.startsWith('/') || name.split('/').includes('..')) refuse('input fixture path escapes its root');
    return new InputPath(this.project, join(this.path, name));
  }
}

function checkAst(node, blockers = null) {
  if (!node) return;
  const allowedCalls = ['jet::compile', 'jet::compile_with_path', 'test_port::codes', 'include_str!', 'assert!', 'assert_eq!', 'assert_ne!', 'matches!', 'panic!', 'format!', 'common::unique_tmp', 'fs::create_dir_all', 'fs::write', 'common::build_and_run', 'tir_support::build_and_run', 'tir_support::assert_tiers_agree'];
  const allowedMethods = ['iter', 'any', 'all', 'find', 'filter', 'collect', 'count', 'len', 'is_empty', 'contains', 'starts_with', 'ends_with', 'as_str', 'expect', 'expect_err', 'unwrap', 'unwrap_err', 'unwrap_or_else', 'is_ok', 'is_err', 'is_some', 'is_none', 'trim', 'replace', 'to_string', 'to_owned', 'join', 'to_str'];
  const unsupported = message => blockers ? blockers.add(message, node) : refuse(message);
  if (node.helperBlocker) unsupported(node.helperBlocker);
  else if (node.type === 'call' && !allowedCalls.includes(node.name)) unsupported(`unmodeled call ${node.name}`);
  if (node.type === 'method' && !allowedMethods.includes(node.name)) unsupported(`unmodeled method ${node.name}`);
  if (node.type === 'field' && !['lints', 'code', 'what', 'why', 'fix', 'severity'].includes(node.name)) unsupported(`unmodeled field ${node.name}`);
  // Message operands are never assertions; panic's arguments are only failure text.
  const assertion = node.type === 'call' && ['assert!', 'assert_eq!', 'assert_ne!'].includes(node.name);
  for (const [key, value] of Object.entries(node)) {
    if (key === 'args' && assertion) value.slice(0, node.name === 'assert!' ? 1 : 2).forEach(child => checkAst(child, blockers));
    else if (key === 'args' && node.type === 'call' && node.name === 'panic!') continue;
    else if (Array.isArray(value)) value.forEach(child => checkAst(child, blockers));
    else if (value && typeof value === 'object') checkAst(value, blockers);
  }
}

function invoke(binary, args, cwd, timeout, stdin, receipts, captureStreams = false) {
  const env = { ...process.env, NO_COLOR: '1', CLICOLOR: '0', TERM: 'dumb', JET_RECEIPT_BYPASS: '1', JET_REPL_HISTORY: 'off' };
  delete env.JET_COMPILER_EXTENSION;
  delete env.JET_SCHEMA_CACHE_DIR;
  const started = performance.now();
  const run = spawnSync(binary, args, { cwd, encoding: 'utf8', timeout, input: stdin, maxBuffer: 16 * 1024 * 1024, env });
  const elapsed_ms = Math.round(performance.now() - started);
  const receipt = { binary, args, cwd, elapsed_ms, code: run.status, signal: run.signal, error: run.error?.message ?? null };
  if (captureStreams) { receipt.stdout = run.stdout; receipt.stderr = run.stderr; }
  if (run.status === 101) receipt.stderr = run.stderr ?? '';
  receipts.push(receipt);
  if (run.error || run.signal || run.status === null || run.status === 101) {
    const excerpt = run.status === 101 ? (run.stderr ?? '').split(/\r?\n/u).slice(0, 20).join('\n').trimEnd() : '';
    throw Object.assign(new Error(`reference invocation failed: ${run.error?.message ?? run.signal ?? run.status}${excerpt ? `\n${excerpt}` : ''}`), { status: 'ERROR' });
  }
  return { code: run.status, stdout: run.stdout, stderr: run.stderr, elapsed_ms };
}

export function machineDiagnostics(run) {
  const result = [];
  const object = (value, label) => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) refuse(`${label} is not an object`);
  };
  const string = (row, key) => { if (typeof row[key] !== 'string') refuse(`machine record is missing string ${key}`); };
  const report = (row) => {
    object(row, 'report');
    if (row.schema !== 'jet.report/v3') refuse(`unknown machine diagnostic schema: ${JSON.stringify(row.schema ?? row.contract ?? null)}`);
    for (const key of ['moment', 'severity', 'code', 'what', 'why', 'fix']) string(row, key);
    if (Object.hasOwn(row, 'fix_edits') && Object.hasOwn(row, 'no_fix_reason')) refuse('report contains both fix_edits and no_fix_reason');
    if (Object.hasOwn(row, 'fix_edits')) {
      if (!Array.isArray(row.fix_edits)) refuse('fix_edits is not an array');
      for (const edit of row.fix_edits) {
        object(edit, 'report edit'); object(edit.span, 'edit span');
        for (const key of ['file', 'new_text', 'safety']) string(edit, key);
        for (const key of ['start', 'end']) if (!Object.hasOwn(edit.span, key)) refuse(`edit span missing ${key}`);
      }
    }
    if (Object.hasOwn(row, 'no_fix_reason')) {
      object(row.no_fix_reason, 'no_fix_reason');
      string(row.no_fix_reason, 'kind'); string(row.no_fix_reason, 'next');
      if (!['behavior', 'design', 'ambiguous'].includes(row.no_fix_reason.kind)) refuse('unknown no-fix reason kind');
      if (!row.no_fix_reason.next.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '') || /\p{Cc}/u.test(row.no_fix_reason.next)) refuse('no-fix next action is not printable');
    }
    result.push(row);
  };
  if (!`${run.stdout}\n${run.stderr}`.trim()) refuse('reference emitted no machine record');
  for (const line of `${run.stdout}\n${run.stderr}`.split('\n').filter((line) => line.trim())) {
    let row; try { row = JSON.parse(line); } catch { refuse(`reference did not emit a JSON report: ${line.slice(0, 120)}`); }
    object(row, 'machine record');
    if (row.schema === 'jet.status/v1') {
      string(row, 'action');
      if (!row.action || /\p{Cc}/u.test(row.action) || Object.hasOwn(row, 'status') || typeof row.ok !== 'boolean' || !Array.isArray(row.reports)) refuse('invalid jet.status/v1 record');
      row.reports.forEach(report);
    } else if (row.contract === 'jet.check/v1') {
      if (row.status !== 'passed' || row.schema_version !== 1 || !Array.isArray(row.rows)) refuse('invalid jet.check/v1 success record');
      object(row.provenance, 'check provenance');
      if (!Number.isSafeInteger(row.provenance.diagnostics) || row.provenance.diagnostics < 0) refuse('invalid check diagnostics count');
      for (const proof of row.rows) {
        object(proof, 'check proof row');
        for (const key of ['output', 'name', 'status', 'detail', 'diagnostic']) string(proof, key);
        if (!['passed', 'not applicable'].includes(proof.status)) refuse(`unsuccessful check proof row: ${proof.status}`);
      }
      if (run.code !== 0) refuse('success check record with failing exit');
      // Proof-row diagnostic labels describe unavailable projections, not errors.
    } else report(row);
  }
  return result;
}

// Only resolve supported bootstrap helpers from their lexical binding. A same-
// named local function is not the common/tir_support contract.
function helperBinding(fn, name) {
  const helpers = fn.helpers ?? [], parts = name.replace(/^crate::/u, '').split('::');
  let path = parts;
  const imports = fn.lexicalTokens ?? [];
  const expand = (tokens, prefix = []) => {
    for (const entry of topLevelSplit(tokens, ',')) {
      const brace = entry.findIndex(token => token.value === '{');
      if (brace >= 0) {
        expand(entry.slice(brace + 1, closeAt(entry, brace)), [...prefix, ...entry.slice(0, brace).filter(token => token.kind === 'word').map(token => token.value)]);
        continue;
      }
      const alias = entry.findIndex(token => token.value === 'as');
      const words = entry.slice(0, alias < 0 ? entry.length : alias).filter(token => token.kind === 'word').map(token => token.value);
      const full = [...prefix, ...words].filter(word => word !== 'crate');
      const bound = alias < 0 ? full.at(-1) : entry[alias + 1]?.value;
      if (parts[0] === bound) path = [...full, ...parts.slice(1)];
    }
  };
  for (let i = 0; i < imports.length; i++) {
    if (imports[i].kind !== 'string' && ['{', '('].includes(imports[i].value)) { i = closeAt(imports, i); continue; }
    if (imports[i].kind !== 'word' || imports[i].value !== 'use') continue;
    let end = i + 1;
    while (end < imports.length && imports[end].value !== ';') end++;
    expand(imports.slice(i + 1, end)); i = end;
  }
  if (path.length === 2) {
    const module = fn.moduleBindings?.find(binding => binding.module === path[0]);
    if (module) {
      const canonical = ['common', 'tir_support'].find(name => module.target === `tests/${name}/mod.rs`);
      if (canonical) path[0] = canonical;
      else if (['common', 'tir_support'].includes(path[0])) return null;
    }
  }
  const leaf = path.at(-1);
  const local = helpers.filter(helper => helper.name === leaf && (helper.file === fn.file ||
    (dirname(fn.file) !== 'tests' && dirname(helper.file) === dirname(fn.file))));
  if (path.length === 1 && local.length === 1) return { helper: local[0], name: leaf };
  if (path.length === 2 && ['common', 'tir_support'].includes(path[0])) {
    const helper = helpers.find(candidate => candidate.name === leaf && candidate.file === `tests/${path[0]}/mod.rs`);
    return { helper, name: path.join('::') };
  }
  return null;
}

function canonicalRunCalls(fn, node) {
  if (!node || typeof node !== 'object') return;
  if (node.type === 'call') {
    const binding = helperBinding(fn, node.name);
    if (binding && ['common::build_and_run', 'tir_support::build_and_run', 'tir_support::assert_tiers_agree'].includes(binding.name)) {
      const helper = binding.helper, raw = helper?.tokens.map(token => token.raw).join(' ');
      const expected = {
        'common::build_and_run': 'build_and_run_with_cwd ( prefix , name , src , false )',
        'tir_support::build_and_run': 'let ( code , stdout , _stderr ) = build_and_run_full ( "jet_tir_test" , name , src ) ; ( code , stdout )',
        'tir_support::assert_tiers_agree': 'assert_tiers_agree_with_package ( name , src , expected_stdout , None ) ;',
      };
      if (!helper || raw === expected[binding.name]) node.name = binding.name;
      else node.helperBlocker = `unmodeled call ${node.name}: helper body differs from ${binding.name}`;
    } else if (binding?.helper?.name === 'build_and_run') {
      // Local wrappers qualify by exact body, not by their familiar name.
      const helper = binding.helper, raw = helper.tokens.map(token => token.raw).join(' ');
      const direct = raw.match(/^common :: build_and_run \( ("(?:\\.|[^"\\])*") , name , src \)$/u);
      const projection = raw.match(/^let \( code , stdout , _stderr \) = common :: build_and_run \( ("(?:\\.|[^"\\])*") , name , src \) ; \( code , stdout \)$/u);
      const signature = helper.signature?.map(token => token.raw).join(' ');
      const streams = direct ? 3 : projection ? 2 : 0;
      if (streams && signature === `( name : & str , src : & str ) -> ( i32 , String${streams === 3 ? ' , String' : ''} )`) {
        node.name = 'common::build_and_run';
        node.args.unshift({ type: 'literal', value: decodeString((direct ?? projection)[1]) });
        node.runStreams = streams;
      }
    }
  }
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) value.forEach(child => canonicalRunCalls(fn, child));
    else if (value && typeof value === 'object') canonicalRunCalls(fn, value);
  }
}

// Complete local helper contracts, never a whitelist of the word `codes`.
// compile returns errors only and drops successful lints; check returns every
// diagnostic in order. Raw Loader/Sema bodies remain HAND-PORT: replacing them
// with Driver would add build facts, extension hooks and package lint policy.
function codesHelperKind(helper) {
  if (helper?.name !== 'codes') return null;
  const body = helper.tokens.map(token => token.raw).join(' ');
  const signature = helper.signature?.map(token => token.raw).join(' ');
  if (signature === '( src : & str ) -> Vec < String >') {
    if (body === 'match jet :: compile ( src ) { Ok ( _ ) => Vec :: new ( ) , Err ( diags ) => diags . iter ( ) . map ( | d | d . code . clone ( ) ) . collect ( ) , }' ||
        body === 'match jet :: compile ( src ) { Ok ( _ ) => Vec :: new ( ) , Err ( diags ) => diags . into_iter ( ) . map ( | d | d . code . to_string ( ) ) . collect ( ) , }') return 'compile-errors';
    if (body === 'jet :: compile ( src ) . err ( ) . unwrap ( ) . into_iter ( ) . map ( | d | d . code . to_string ( ) ) . collect ( )') return 'required-errors';
  }
  if (signature === '( source : & str , stem : & str ) -> Vec < String >' &&
      body === 'let root = common :: unique_tmp ( stem ) ; let path = root . join ( "main.jet" ) ; fs :: write ( & path , source ) . unwrap ( ) ; let diagnostics = jet :: check_with_path ( path . to_str ( ) . unwrap ( ) ) ; let _ = fs :: remove_dir_all ( root ) ; diagnostics . into_iter ( ) . map ( | diagnostic | diagnostic . code ) . collect ( )') return 'staged-all';
  if (signature === '( entry : & str ) -> Vec < String >' &&
      body === 'jet :: Driver :: check_file_with_effect_facts ( entry , None , false ) . 0 . iter ( ) . map ( | d | d . code . clone ( ) ) . collect ( )') return 'path-all';
  if (signature === '( diags : & [ jet :: Diagnostics :: Diagnostic ] ) -> BTreeSet < String >' &&
      body === 'diags . iter ( ) . map ( | d | d . code . clone ( ) ) . collect ( )') {
    const lexical = helper.lexicalTokens.filter(token => token.kind !== 'string').map(token => token.raw).join(' ');
    if (/(?:^|; )use std :: collections :: BTreeSet ;/u.test(lexical)) return 'diagnostic-set';
  }
  return null;
}

function canonicalCodesCalls(fn, node) {
  if (!node || typeof node !== 'object') return;
  if (node.type === 'call') {
    const binding = helperBinding(fn, node.name);
    if (binding?.helper?.name === 'codes') {
      const kind = codesHelperKind(binding.helper);
      if (!kind) refuse(`unmodeled call ${node.name}: local codes body or signature has no exact projection`);
      node.name = 'test_port::codes';
      node.codesKind = kind;
    }
  }
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) value.forEach(child => canonicalCodesCalls(fn, child));
    else if (value && typeof value === 'object') canonicalCodesCalls(fn, value);
  }
}

function availabilityGuards(fn, tokens) {
  const output = [];
  const rustc = name => ['common::have_rustc', 'tir_support::have_rustc'].includes(helperBinding(fn, name)?.name);
  for (let i = 0; i < tokens.length;) {
    // A saved availability value qualifies only for this adjacent, immutable,
    // one-use early return. Do not erase later uses or mutations of the value.
    if (tokens[i]?.value === 'let' && tokens[i + 1]?.kind === 'word' && tokens[i + 1].value !== 'mut' && tokens[i + 2]?.value === '=') {
      const alias = tokens[i + 1].value;
      let cursor = i + 3;
      while (tokens[cursor]?.kind === 'word' || tokens[cursor]?.value === '::') cursor++;
      const name = tokens.slice(i + 3, cursor).map(token => token.value).join('');
      if (rustc(name) && tokens[cursor]?.value === '(' && tokens[cursor + 1]?.value === ')' && tokens[cursor + 2]?.value === ';' &&
          tokens[cursor + 3]?.value === 'if' && tokens[cursor + 4]?.value === '!' && tokens[cursor + 5]?.value === alias && tokens[cursor + 6]?.value === '{') {
        const close = closeAt(tokens, cursor + 6);
        if (!tokens.slice(close + 1).some(token => token.kind === 'word' && token.value === alias) && tokens[close + 1]?.value !== 'else') {
          const guard = [...tokens.slice(cursor + 3, cursor + 5), ...tokens.slice(i + 3, cursor + 2), ...tokens.slice(cursor + 6, close + 1)];
          if (!availabilityGuards(fn, guard).length) { i = close + 1; continue; }
        }
      }
    }
    if (tokens[i]?.value !== 'if' || tokens[i]?.kind === 'string') { output.push(tokens[i++]); continue; }
    let cursor = i + 1, negative = tokens[cursor]?.value === '!';
    if (negative) cursor++;
    const start = cursor;
    while (tokens[cursor]?.kind === 'word' || tokens[cursor]?.value === '::') cursor++;
    const name = tokens.slice(start, cursor).map(token => token.value).join('');
    let accepted = negative && rustc(name);
    if (!negative) {
      const binding = helperBinding(fn, name), body = binding?.helper?.tokens ?? [];
      const raw = body.map(token => token.raw).join(' ');
      const call = raw.match(/^! ((?:[A-Za-z_]\w* :: )*[A-Za-z_]\w*) \( \)$/u);
      accepted = binding?.helper?.name === 'skip_without_rustc' && !!call
        && ['common::have_rustc', 'tir_support::have_rustc'].includes(helperBinding({ ...binding.helper, helpers: fn.helpers }, call[1].replaceAll(' ', ''))?.name);
    }
    if (!accepted || tokens[cursor]?.value !== '(' || tokens[cursor + 1]?.value !== ')' || tokens[cursor + 2]?.value !== '{') { output.push(tokens[i++]); continue; }
    const open = cursor + 2, close = closeAt(tokens, open), body = tokens.slice(open + 1, close);
    let j = 0;
    while (body[j]?.value === 'eprintln' && body[j + 1]?.value === '!' && body[j + 2]?.value === '(') {
      const end = closeAt(body, j + 2), args = body.slice(j + 3, end);
      // A literal note has no expression side effects or original assertions.
      if (args.length !== 1 || args[0].kind !== 'string' || body[end + 1]?.value !== ';') break;
      j = end + 2;
    }
    const tail = body.slice(j).map(token => token.raw).join(' ');
    if (!['return ;', 'return'].includes(tail) || tokens[close + 1]?.value === 'else') { output.push(tokens[i++]); continue; }
    i = close + 1;
  }
  return output;
}

export function assertionPlan(fn) {
  const retirement = retireRustAssertions(fn);
  const statements = new Parser(availabilityGuards(fn, retirement.tokens)).statements();
  statements.forEach(node => canonicalRunCalls(fn, node));
  statements.forEach(node => canonicalCodesCalls(fn, node));
  for (const statement of statements) checkAst(statement);
  Object.defineProperty(statements, 'retiredAssertions', { value: retirement.retired });
  return statements;
}

// Analysis never observes a reference process or emits fixtures. The exact
// parser/model remains authoritative; balanced recovery inspects every failed
// statement's nested bodies/operands instead of hiding its later blockers.
export function analyzeFunction(fn) {
  const classes = new Set(), occurrences = [], seen = new Set(), normalize = error => error.message.replace(/ at line \d+$/u, '');
  const blockers = { add(message, location = {}) {
    classes.add(message);
    const line = location.line ?? fn.line, offset = location.offset ?? location.start ?? line;
    const key = `${offset}:${message}`;
    if (!seen.has(key)) { seen.add(key); occurrences.push({ class: message, line, offset }); }
  } };
  const retirement = retireRustAssertions(fn), tokens = availabilityGuards(fn, retirement.tokens);
  const bindings = new Set();
  const model = (node, scope) => {
    if (!node || typeof node !== 'object') return;
    if (node.type === 'variable' && !scope.has(node.name)) blockers.add(`unbound value ${node.name}`, node);
    if (node.type === 'closure') { model(node.body, new Set([...scope, node.param])); return; }
    for (const [key, value] of Object.entries(node)) {
      if (key === 'args' && node.type === 'call' && node.name === 'panic!') continue;
      let children = Array.isArray(value) ? value : [value];
      if (key === 'args' && node.type === 'call' && ['assert!', 'assert_eq!', 'assert_ne!'].includes(node.name)) children = children.slice(0, node.name === 'assert!' ? 1 : 2);
      children.forEach(child => model(child, scope));
    }
  };
  const modelStatements = (statements, scope) => {
    for (const statement of statements) {
      if (statement.type === 'loop') {
        model(statement.iterable, scope);
        modelStatements(statement.statements, new Set([...scope, ...statement.names]));
      } else {
        model(statement.expr, scope);
        statement.names?.forEach(name => scope.add(name));
      }
    }
  };
  const scanOperands = list => {
    for (let i = 0; i < list.length; i++) {
      const token = list[i];
      if (token.kind === 'string') continue;
      if (token.value === '{') { const end = closeAt(list, i); scanStatements(list.slice(i + 1, end)); i = end; continue; }
      if (token.kind === 'word' && !['::'].includes(list[i - 1]?.value)) {
        let endName = i, name = token.value;
        while (list[endName + 1]?.kind === 'punct' && list[endName + 1]?.value === '::' && list[endName + 2]?.kind === 'word') { name += `::${list[endName + 2].value}`; endName += 2; }
        if (list[endName + 1]?.kind === 'punct' && list[endName + 1]?.value === '!') { name += '!'; endName++; }
        if (list[endName + 1]?.kind === 'punct' && list[endName + 1]?.value === '(') {
          const end = closeAt(list, endName + 1), method = list[i - 1]?.value === '.';
          checkAst({ type: method ? 'method' : 'call', name, args: [], line: token.line, offset: token.start }, blockers);
          let args = topLevelSplit(list.slice(endName + 2, end), ',');
          if (['assert!', 'assert_eq!', 'assert_ne!'].includes(name)) args = args.slice(0, name === 'assert!' ? 1 : 2);
          if (name !== 'panic!') args.forEach(scanOperands);
          i = end; continue;
        }
        if (list[i - 1]?.value === '.') checkAst({ type: 'field', name, line: token.line, offset: token.start }, blockers);
      }
      if (['(', '['].includes(token.value)) { const end = closeAt(list, i); scanOperands(list.slice(i + 1, end)); i = end; }
    }
  };
  const scanStatements = list => {
    for (const segment of topLevelStatements(list)) {
      if (!segment.length) continue;
      // Try the complete statement first, retaining parsed expressions even if
      // its final terminator or a later unsupported operator refuses.
      const semicolon = { kind: 'punct', value: ';', raw: ';', line: segment.at(-1).line };
      const parser = new Parser(segment[0]?.value === 'for' && segment.at(-1)?.value === '}' ? segment : [...segment, semicolon]);
      try {
        const statements = parser.statements();
        statements.forEach(node => canonicalRunCalls(fn, node));
        statements.forEach(node => canonicalCodesCalls(fn, node));
        statements.forEach(node => checkAst(node, blockers));
        modelStatements(statements, bindings);
      }
      catch (error) { blockers.add(normalize(error), parser.tokens[parser.i] ?? segment[0]); scanOperands(segment); }
    }
  };
  scanStatements(tokens);
  return { file: fn.file, fn: fn.name, classes: [...classes].sort(), occurrences: occurrences.sort((a, b) => a.offset - b.offset), reference: 'runtime-unknown' };
}

function rustEqual(a, b) {
  const scalar = (value) => value === undefined || value === null || ['string', 'number', 'boolean'].includes(typeof value)
    || (Array.isArray(value) && value.every(scalar));
  if (!scalar(a) || !scalar(b)) refuse('whole Rust struct equality is not represented by the machine projection');
  return isDeepStrictEqual(a, b);
}

async function evaluate(node, env, context) {
  const ev = (arg, local = env) => evaluate(arg, local, context);
  switch (node.type) {
    case 'literal': return node.value;
    case 'variable': if (!Object.hasOwn(env, node.name)) refuse(`unbound value ${node.name}`); return env[node.name];
    case 'array': return Promise.all(node.args.map((arg) => ev(arg)));
    case 'unary': { const value = await ev(node.arg); return node.op === '!' ? !value : value; }
    case 'field': { const value = await ev(node.object); if (!Object.hasOwn(value ?? {}, node.name)) refuse(`missing field ${node.name}`); return value[node.name]; }
    case 'index': { const value = await ev(node.object), index = await ev(node.index); if (!Object.hasOwn(value, index)) refuse('out of bounds'); return value[index]; }
    case 'closure': return (value) => ev(node.body, { ...env, [node.param]: value });
    case 'binary': {
      // Evaluate both operands so no unsupported operation hides behind short circuiting.
      const a = await ev(node.left), b = await ev(node.right);
      if (['>', '<', '>=', '<='].includes(node.op)) {
        if (typeof a !== typeof b || !['string', 'number', 'boolean'].includes(typeof a)) refuse('unmodeled Rust ordering');
        const comparison = typeof a === 'string' ? Buffer.compare(Buffer.from(a), Buffer.from(b)) : a === b ? 0 : a < b ? -1 : 1;
        return ({ '>': comparison > 0, '<': comparison < 0, '>=': comparison >= 0, '<=': comparison <= 0 })[node.op];
      }
      return ({ '==': () => rustEqual(a, b), '!=': () => !rustEqual(a, b), '&&': () => a && b,
        '||': () => a || b, '|': () => [a, b].flat() })[node.op]();
    }
    case 'call': {
      if (['assert!', 'assert_eq!', 'assert_ne!'].includes(node.name)) {
        const args = await Promise.all(node.args.slice(0, node.name === 'assert!' ? 1 : 2).map((arg) => ev(arg)));
        const ok = node.name === 'assert!' ? args[0] === true : node.name === 'assert_eq!' ? rustEqual(args[0], args[1]) : !rustEqual(args[0], args[1]);
        if (!ok) refuse(`original ${node.name} not satisfied: ${JSON.stringify(args)}`);
        context.assertions++; return;
      }
      if (node.name === 'panic!') refuse('original panic branch reached');
      const args = await Promise.all(node.args.map((arg) => ev(arg)));
      if (node.name === 'matches!') return (Array.isArray(args[1]) ? args[1] : [args[1]]).includes(args[0]);
      if (node.name === 'common::unique_tmp') {
        if (typeof context.reference.createProject !== 'function') refuse('reference has no input-project staging');
        return context.reference.createProject();
      }
      if (['fs::create_dir_all', 'fs::write'].includes(node.name)) {
        if (!(args[0] instanceof InputPath)) refuse('filesystem setup outside a staged input project');
        const path = args[0];
        if (node.name === 'fs::create_dir_all') mkdirSync(path.disk(), { recursive: true });
        else {
          if (typeof args[1] !== 'string' || !path.path) refuse('nontext fixture input');
          writeFileSync(path.disk(), args[1]);
          path.project.files.set(path.path, args[1]);
        }
        return { ok: true, value: undefined };
      }
      if (node.name === 'format!') {
        if (typeof args[0] !== 'string') refuse('format source template is not a string');
        let positional = 1;
        return args[0].replace(/\{\{|\}\}|\{([^{}]*)\}|[{}]/gu, (match, field) => {
          if (match === '{{') return '{';
          if (match === '}}') return '}';
          if (field === undefined || field.includes(':')) refuse(`unsupported source format ${match}`);
          const value = field === '' ? args[positional++] : Object.hasOwn(env, field) ? env[field] : refuse(`unbound format field ${field}`);
          if (!['string', 'number', 'boolean'].includes(typeof value)) refuse('unsupported source format value');
          return String(value);
        });
      }
      if (node.name === 'include_str!') {
        if (args.length !== 1 || typeof args[0] !== 'string') refuse('nonliteral include_str');
        const path = resolve(dirname(join(context.root, context.fn.file)), args[0]);
        if (!path.startsWith(`${context.root}/`)) refuse('include_str escapes repository');
        return readFileSync(path, 'utf8');
      }
      if (node.name === 'test_port::codes') {
        const kind = node.codesKind;
        if (kind === 'diagnostic-set') {
          if (args.length !== 1 || !Array.isArray(args[0]) || args[0].some(d => typeof d?.code !== 'string')) refuse('codes needs a diagnostic slice');
          // Rust String ordering is UTF-8 byte ordering, not JS UTF-16 order.
          return [...new Set(args[0].map(d => d.code))]
            .map(value => ({ value, bytes: Buffer.from(value, 'utf8') }))
            .sort((a, b) => Buffer.compare(a.bytes, b.bytes)).map(row => row.value);
        }
        if (!['compile-errors', 'required-errors', 'staged-all', 'path-all'].includes(kind)) refuse('codes call has no proven helper body');
        const all = kind === 'staged-all' || kind === 'path-all';
        let source, project = null;
        if (kind === 'path-all') {
          const path = args[0];
          if (args.length !== 1 || !(path instanceof InputPath) || !path.project.files.has(path.path)) refuse('Driver codes needs a staged input path');
          source = readFileSync(path.disk(), 'utf8');
          project = { entry: path.path, files: [...path.project.files].map(([path, content]) => ({ path, content })) };
        } else {
          if (args.length !== (kind === 'staged-all' ? 2 : 1) || typeof args[0] !== 'string' ||
              (kind === 'staged-all' && typeof args[1] !== 'string')) refuse('unsupported source codes arguments');
          source = args[0];
          if (kind === 'staged-all') project = { entry: 'main.jet', files: [{ path: 'main.jet', content: source }] };
        }
        const observed = await context.observe(source, 'compile', project, { allDiagnostics: all });
        context.observations.push(observed);
        if (kind === 'required-errors') {
          context.assertions++;
          if (observed.code === 0) refuse('original codes compile.err().unwrap() not satisfied');
        }
        if (!all && observed.code === 0) return [];
        return observed.diagnostics.filter(d => all || d.severity.toLowerCase() === 'error').map(d => d.code);
      }
      if (['jet::compile', 'jet::compile_with_path'].includes(node.name)) {
        let project = null;
        if (node.name === 'jet::compile_with_path') {
          if (!(args[1] instanceof InputPath) || typeof args[0] !== 'string') refuse('compile_with_path needs a staged input path');
          if (readFileSync(args[1].disk(), 'utf8') !== args[0]) refuse('compile source differs from its staged entry');
          project = { entry: args[1].path, files: [...args[1].project.files].map(([path, content]) => ({ path, content })) };
        } else if (args.length !== 1 || typeof args[0] !== 'string') refuse('unsupported compile arguments');
        const observed = await context.observe(args[0], 'compile', project);
        context.observations.push(observed); context.assertions++;
        return observed.code === 0 ? { ok: true, value: { lints: observed.diagnostics } } : { ok: false, value: observed.diagnostics };
      }
      if (['common::build_and_run', 'tir_support::build_and_run'].includes(node.name)) {
        const tir = node.name === 'tir_support::build_and_run', sourceIndex = tir ? 1 : 2;
        if (args.length !== sourceIndex + 1 || typeof args[sourceIndex] !== 'string') refuse('unsupported run arguments');
        const observed = await context.observe(args[sourceIndex], 'run'); context.observations.push(observed);
        return (tir || node.runStreams === 2) ? [observed.code, observed.stdout] : [observed.code, observed.stdout, observed.stderr];
      }
      if (node.name === 'tir_support::assert_tiers_agree') {
        if (args.length !== 3 || typeof args[1] !== 'string' || typeof args[2] !== 'string') refuse('unsupported all-tier helper arguments');
        const observed = await context.observe(args[1], 'run');
        context.observations.push(observed); context.assertions++;
        if (observed.code !== 0 || observed.stdout !== args[2]) refuse('original all-tier stdout/exit assertion not satisfied');
        return undefined;
      }
      refuse(`unmodeled call ${node.name}`); break;
    }
    case 'method': {
      const value = await ev(node.object);
      if (['expect', 'expect_err', 'unwrap', 'unwrap_err', 'unwrap_or_else'].includes(node.name)) {
        if (value && typeof value.ok === 'boolean') {
          const wantOk = !['expect_err', 'unwrap_err'].includes(node.name);
          if (value.ok !== wantOk) refuse(`original ${node.name} not satisfied`);
          return value.value;
        }
        if (value === undefined || value === null) refuse(`original ${node.name} found no matching diagnostic`);
        return value;
      }
      const args = await Promise.all(node.args.map((arg) => ev(arg)));
      switch (node.name) {
        case 'iter': return value;
        case 'as_str': return value;
        case 'join': if (!(value instanceof InputPath)) refuse('join receiver is not an input path'); return value.child(args[0]);
        case 'to_str': if (!(value instanceof InputPath)) refuse('to_str receiver is not an input path'); return value;
        case 'collect': case 'to_string': case 'to_owned': return value;
        case 'is_some': return value !== null && value !== undefined;
        case 'is_none': return value === null || value === undefined;
        case 'replace': return value.replaceAll(args[0], args[1]);
        case 'is_ok': return value.ok === true;
        case 'is_err': return value.ok === false;
        case 'any': case 'all': case 'find': case 'filter': {
          if (!Array.isArray(value) || typeof args[0] !== 'function') refuse('invalid iterator predicate');
          const matches = await Promise.all(value.map(args[0]));
          if (node.name === 'any') return matches.some((match) => match === true);
          if (node.name === 'all') return matches.every((match) => match === true);
          if (node.name === 'find') return value[matches.findIndex((match) => match === true)];
          return value.filter((_, i) => matches[i] === true);
        }
        case 'count': case 'len': return typeof value === 'string' ? Buffer.byteLength(value, 'utf8') : value.length;
        case 'is_empty': return value.length === 0;
        case 'contains': return value.includes(args[0]);
        case 'starts_with': return value.startsWith(args[0]);
        case 'ends_with': return value.endsWith(args[0]);
        case 'trim': return value.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '');
        default: refuse(`unmodeled method ${node.name}`);
      }
      break;
    }
    default: refuse(`unmodeled expression ${node.type}`);
  }
}

async function executeStatements(statements, local, context) {
  for (const statement of statements) {
    if (statement.type === 'loop') {
      const values = await evaluate(statement.iterable, local, context);
      if (!Array.isArray(values)) refuse('loop iterable is not a finite extracted array');
      for (const value of values) {
        if (statement.names.length > 1 && (!Array.isArray(value) || value.length !== statement.names.length)) refuse('loop tuple arity mismatch');
        const inner = { ...local };
        statement.names.forEach((name, i) => { inner[name] = statement.names.length === 1 ? value : value[i]; });
        await executeStatements(statement.statements, inner, context);
      }
    } else {
      const value = await evaluate(statement.expr, local, context);
      if (statement.names) {
        if (statement.names.length > 1 && (!Array.isArray(value) || value.length !== statement.names.length)) refuse('tuple arity mismatch');
        statement.names.forEach((name, i) => { local[name] = statement.names.length === 1 ? value : value[i]; });
      }
    }
  }
}

export async function checkOriginal(fn, reference, root = ROOT) {
  const plan = assertionPlan(fn), env = {};
  const context = { fn, root, reference, observations: [], assertions: 0, retiredAssertions: plan.retiredAssertions };
  const nodes = [];
  const walk = (node) => {
    if (!node || typeof node !== 'object') return;
    if (node.type) nodes.push(node);
    for (const value of Object.values(node)) {
      if (Array.isArray(value)) value.forEach(walk);
      else if (value && typeof value === 'object') walk(value);
    }
  };
  plan.forEach(walk);
  const lint = nodes.some((node) => node.type === 'field' && node.name === 'lints');
  const stderrVariables = new Set(['stderr', '_stderr', 'err', '_err']);
  const trackStreams = (statements) => {
    for (const statement of statements) {
      if (statement.statements) { trackStreams(statement.statements); continue; }
      if (!statement.names) continue;
      if (statement.expr.type === 'call' && statement.expr.name === 'common::build_and_run' && statement.expr.runStreams !== 2) stderrVariables.add(statement.names.at(-1));
      if (statement.expr.type === 'variable' && stderrVariables.has(statement.expr.name)) statement.names.forEach((name) => stderrVariables.add(name));
    }
  };
  trackStreams(plan);
  const containsRun = (node) => {
    if (!node || typeof node !== 'object') return false;
    if (node.type === 'call' && node.name === 'common::build_and_run' && node.runStreams !== 2) return true;
    return Object.values(node).some((value) => Array.isArray(value) ? value.some(containsRun) : containsRun(value));
  };
  const readsStderr = nodes.some((node) => (node.type === 'variable' && stderrVariables.has(node.name)) ||
    (node.type === 'call' && ['assert!', 'assert_eq!', 'assert_ne!'].includes(node.name) && node.args.slice(0, node.name === 'assert!' ? 1 : 2).some(containsRun)));
  const allTier = nodes.some(node => node.type === 'call' && node.name === 'tir_support::assert_tiers_agree');
  const agreement = allTier || fn.tokens.some((token) => token.kind === 'word' && /^(?:dev_run|run_interpreted)$/u.test(token.value));
  const interpreterAgreement = allTier || fn.tokens.some((token) => token.kind === 'word' && token.value === 'run_interpreted');
  const suite = fn.file.replace(/^tests\//u, '').replace(/\.rs$/u, '').replaceAll('/', '_');
  let ordinal = 0;
  context.observe = async (source, mode, project = null, projection = {}) => {
    const name = ++ordinal === 1 ? fn.name : `${fn.name}_${ordinal}`;
    const category = mode === 'run' ? 'run' : lint ? 'ui_lint' : 'ui';
    if (project) {
      const original = project.entry, oldName = basename(original), oldStem = basename(original, '.jet');
      const siblings = project.files.filter((file) => file.path !== original);
      if (siblings.some((file) => file.content.includes(oldName) || file.content.includes(`"./${oldStem}"`) || file.content.includes(`'./${oldStem}'`))) refuse(`project sibling references renamed entry ${oldName}`);
      const files = project.files.map((file) => ({ path: file.path === original ? 'run.jet' : relative(dirname(original), file.path), content: file.content }));
      if (files.some((file) => file.path.startsWith('../'))) refuse('project input outside the entry directory');
      project = { entry: 'run.jet', files };
    }
    const entry = project?.entry ?? `${name}.jet`;
    const path = project ? `tests/${category}/${suite}/${name}/${entry}` : `tests/${category}/${suite}/${name}.jet`;
    const observation = await reference(source, mode, { path, ...(project ? { project } : {}), ...projection, lint: mode === 'compile' && lint, agreement: mode === 'run' && agreement, interpreterAgreement: mode === 'run' && interpreterAgreement });
    return { ...observation, project, readsStderr, allDiagnostics: projection.allDiagnostics === true };
  };
  await executeStatements(plan, env, context);
  if (!context.observations.length) refuse('no supported Jet source invocation');
  return context;
}

function externalDraftRoot(path, root) {
  const output = resolve(path), checkout = realpathSync(root);
  let ancestor = output;
  while (!existsSync(ancestor)) ancestor = dirname(ancestor);
  const physical = resolve(realpathSync(ancestor), relative(ancestor, output));
  const common = spawnSync('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], { cwd: checkout, encoding: 'utf8' });
  const roots = [checkout];
  if (common.status === 0 && basename(common.stdout.trim()) === '.git') roots.push(dirname(realpathSync(common.stdout.trim())));
  if (roots.some(directory => physical === directory || physical.startsWith(`${directory}/`))) refuse('draft output must be outside the checkout');
  return physical;
}

export async function draftFunction(fn, options) {
  const root = options.root ?? ROOT, output = externalDraftRoot(options.draft, root);
  if (!options.converterSha) refuse('draft requires the converter source sha');
  const suite = fn.file.replace(/^tests\//u, '').replace(/\.rs$/u, '').replaceAll('/', '_');
  const directory = join(output, options.slice ?? 'unassigned', suite), stem = fn.name;
  mkdirSync(directory, { recursive: true });
  const casePath = join(directory, `${stem}.case`), filesRoot = join(directory, `${stem}.files`);
  mkdirSync(filesRoot, { recursive: true });
  const analysis = analyzeFunction(fn), todos = [...analysis.occurrences], seen = new Set(todos.map(row => `${row.offset}:${row.class}`));
  const note = (message, token) => {
    const row = { class: message.split('\n')[0], line: token?.line ?? fn.line, offset: token?.offset ?? token?.start ?? fn.bodyOffset };
    const key = `${row.offset}:${row.class}`;
    if (!seen.has(key)) { seen.add(key); todos.push(row); }
  };
  const env = {}, reference = options.reference, captured = [], sources = [], mapped = [];
  const firstInvocation = reference.invocations?.length ?? 0;
  const context = { fn, root, reference, observations: [], assertions: 0, retiredAssertions: [] };
  context.observe = async (source, mode, project = null, projection = {}) => {
    const ordinal = sources.length + 1, entry = project?.entry ?? `${stem}.jet`;
    const staged = `tests/${mode === 'run' ? 'run' : 'ui'}/${suite}/${stem}/${entry}`, scope = `source-${ordinal}`;
    const files = project?.files ?? [{ path: entry, content: source }];
    for (const input of files) {
      const path = join(filesRoot, scope, dirname(staged), relative(dirname(entry), input.path));
      if (!path.startsWith(`${filesRoot}/`)) refuse('draft fixture escapes output root');
      mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, input.content);
    }
    sources.push({ source, mode, project, staged, scope });
    const before = reference.invocations?.length ?? 0;
    let observation;
    try {
      observation = await reference(source, mode, { path: staged, ...(project ? { project } : {}), ...projection, agreement: context.allTier === true, interpreterAgreement: context.allTier === true });
      return observation;
    } finally {
      for (const receipt of reference.invocations?.slice(before) ?? []) {
        // A failed spawn has no observed exit. Never fabricate its expectations.
        if (receipt.code === null) continue;
        if (typeof receipt.stdout !== 'string' || typeof receipt.stderr !== 'string') refuse('draft reference did not retain captured streams');
        const number = captured.length + 1, stdout = `${stem}.capture-${number}.stdout`, stderr = `${stem}.capture-${number}.stderr`;
        const rebase = value => value.replaceAll(receipt.cwd, `{work}/${scope}`);
        const program = receipt.binary === options.binary ? 'jet' : rebase(receipt.binary);
        const command = [program, ...receipt.args.map(rebase)].map(arg => /\s/u.test(arg) ? JSON.stringify(arg) : arg).join(' ');
        const caseLines = [`step: ${command}`, `cwd: {work}/${scope}`, `exit: ${receipt.code}`, `stdout: = ${stdout}`, `stderr: = ${stderr}`];
        writeFileSync(join(directory, stdout), receipt.stdout);
        writeFileSync(join(directory, stderr), receipt.stderr);
        captured.push({ number, caseLines, receipt });
      }
      if (observation) context.observations.push(observation);
    }
  };
  const tokens = availabilityGuards(fn, retireRustAssertions(fn).tokens);
  for (const segment of topLevelStatements(tokens)) {
    if (!segment.length) continue;
    const terminator = { kind: 'punct', value: ';', raw: ';', line: segment.at(-1).line };
    let plan;
    try {
      plan = new Parser(segment[0].value === 'for' && segment.at(-1).value === '}' ? segment : [...segment, terminator]).statements();
      plan.forEach(node => canonicalRunCalls(fn, node));
      plan.forEach(node => canonicalCodesCalls(fn, node));
      plan.forEach(node => checkAst(node));
    } catch {
      // Unknown statements may consume or mutate an earlier binding. Later
      // statements are independent only if their inputs are still known.
      for (const token of segment) if (token.kind === 'word' && Object.hasOwn(env, token.value)) delete env[token.value];
      continue;
    } // Full analysis records each unsupported construct.
    const before = context.assertions;
    const allTier = node => node && typeof node === 'object' && ((node.type === 'call' && node.name === 'tir_support::assert_tiers_agree') ||
      Object.values(node).some(value => Array.isArray(value) ? value.some(allTier) : allTier(value)));
    context.allTier = plan.some(allTier);
    try {
      await executeStatements(plan, env, context);
      if (context.assertions > before && captured.length) mapped.push({ start: segment[0].start, end: segment.at(-1).end, capture: captured.at(-1).number });
    } catch (error) { note(error.message, segment[0]); }
  }
  // Preserve raw inputs even when their eventual invocation is unsupported.
  for (const [name, value] of Object.entries(env)) {
    if (value instanceof InputPath) for (const [path, content] of value.project.files) {
      const target = join(filesRoot, 'inputs', name, path);
      mkdirSync(dirname(target), { recursive: true }); writeFileSync(target, content);
    }
    else if (typeof value === 'string' && /(?:^|\n)\s*(?:fn |use |struct |#)/u.test(value)) {
      const target = join(filesRoot, 'bindings', `${name}.jet`);
      mkdirSync(dirname(target), { recursive: true }); writeFileSync(target, value);
    }
  }
  const literals = fn.tokens.filter(token => token.kind === 'string');
  for (const [index, token] of literals.entries()) if (/(?:^|\n)\s*(?:fn |use |struct |#)/u.test(token.value)) {
    const path = join(filesRoot, 'literals', `${index + 1}.jet`);
    mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, token.value);
  }
  writeFileSync(join(directory, `${stem}.literals.json`), JSON.stringify(literals.map(token => ({ line: token.line, raw: token.raw, value: token.value })), null, 2) + '\n');
  const assertions = [], original = fn.tokens;
  for (let i = 0, statementStart = 0; i < original.length; i++) {
    const token = original[i];
    if (token.kind === 'string') continue;
    if (token.value === ';') statementStart = i + 1;
    const macro = /^(?:(?:debug_)?assert(?:_[A-Za-z_0-9]+)?|panic|unreachable)$/u.test(token.value) && original[i + 1]?.kind === 'punct' && original[i + 1]?.value === '!';
    const helper = /^assert_/u.test(token.value) && original[i + 1]?.kind === 'punct' && original[i + 1]?.value === '(';
    const method = ['expect', 'expect_err', 'unwrap', 'unwrap_err', 'unwrap_or_else'].includes(token.value) && original[i - 1]?.value === '.' && original[i + 1]?.kind === 'punct' && original[i + 1]?.value === '(';
    if (!macro && !helper && !method) continue;
    const open = i + (macro ? 2 : 1), close = closeAt(original, open);
    const start = method ? original[statementStart].start : token.start, end = original[close].end;
    const source = fn.body.slice(start - fn.bodyOffset, end - fn.bodyOffset);
    const match = mapped.find(range => token.start >= range.start && token.end <= range.end);
    assertions.push(`# assertion ${match ? `mapped: captured step ${match.capture}` : 'NOT MAPPED'} at ${fn.file}:${token.line}\n` + source.split('\n').map(line => `# ${line}`).join('\n'));
  }
  const header = `# ${fn.file}::${fn.name} at ${fn.file}:${fn.line}; converter ${options.converterSha}\n# DRAFT — not a case; finish and move into tests/\n# HISTORICAL SMOKE; partial captures are not blessed expectations\n`;
  writeFileSync(casePath, [header, `fixture: ${stem}.files`, ...captured.flatMap(row => row.caseLines), ...assertions,
    ...todos.sort((a, b) => a.offset - b.offset).map(row => `# TODO(port): ${row.class} at ${fn.file}:${row.line}`)].join('\n') + '\n');
  writeFileSync(join(directory, `${stem}.receipts.json`), JSON.stringify({ sources, invocations: reference.invocations?.slice(firstInvocation) ?? [] }, null, 2) + '\n');
  return { status: 'DRAFTED', underlyingStatus: 'HAND-PORT', function: `${fn.file}::${fn.name}`, slice: options.slice ?? '-', draft: casePath,
    todoCount: todos.length, capturedSteps: captured.length, assertionComments: assertions.length, runtimeUnknown: true };
}

function renderedOnly(text) {
  return text.replace(/\n\n(?:\d+ problems? found|ok: `[^\n]+` has no errors \(\d+ warnings?\))\nrun `jet explain [^\n]+` to learn more\n$/u, '\n');
}

export function makeReference(binary, root, scratch, timeout = 600000, captureStreams = false) {
  let index = 0;
  const invocations = [];
  const call = (program, args, cwd, stdin = '') => invoke(program, args, cwd, timeout, stdin, invocations, captureStreams);
  const reference = async (source, mode, metadata = {}) => {
    const dir = join(scratch, `source-${index++}`); mkdirSync(dir, { recursive: true });
    const shown = metadata.path ?? 'tests/ui/test_port/run.jet', file = join(dir, shown);
    if (metadata.project) for (const input of metadata.project.files) {
      const target = join(dirname(file), relative(dirname(metadata.project.entry), input.path));
      mkdirSync(dirname(target), { recursive: true }); writeFileSync(target, input.content);
    }
    mkdirSync(dirname(file), { recursive: true }); writeFileSync(file, source);
    if (mode === 'run') {
      const built = call(binary, ['build', shown, '--profile=debug'], dir);
      if (built.code !== 0 || /Warning \[L0507\]/u.test(built.stderr)) refuse(`reference ref-tier build failed: exit ${built.code}: ${built.stderr}`);
      // Progress output omits `built:`; use CmdCompile's documented bin_path.
      const entryDirectory = dirname(file);
      let projectRoot = entryDirectory;
      for (let candidate = entryDirectory; ; candidate = dirname(candidate)) {
        if (existsSync(join(candidate, 'package.jet'))) { projectRoot = candidate; break; }
        if (dirname(candidate) === candidate) break;
      }
      const artifact = join(projectRoot, '.jet/build', basename(file, '.jet').replaceAll('.', '_'));
      if (!existsSync(artifact)) throw Object.assign(new Error(`successful reference build is missing artifact: ${artifact}`), { status: 'ERROR' });
      const run = call(artifact, [], dir, metadata.stdin ?? '');
      if (metadata.agreement) {
        const tiers = [['default', ['run', shown]]];
        if (metadata.interpreterAgreement) tiers.push(['interpreter', ['run', '--interpret', shown]]);
        for (const [tier, args] of tiers) {
          const other = call(binary, args, dir, metadata.stdin ?? '');
          if (['code', 'stdout', 'stderr'].some((key) => other[key] !== run[key])) refuse(`I9 ref/${tier} exit/stdout/stderr disagreement`);
        }
      }
      return { source, mode, ...run };
    }
    const lint = metadata.lint === true;
    const cwd = dir, argument = shown;
    const flags = lint || metadata.allDiagnostics === true ? ['--verbose'] : [];
    const machine = call(binary, ['check', '--json', ...flags, argument], cwd);
    const diagnostics = machineDiagnostics(machine);
    if (machine.code !== 0 && !diagnostics.some((row) => row.severity.toLowerCase() === 'error')) refuse('failed check without an error diagnostic');
    const human = call(binary, ['check', ...flags, argument], cwd);
    if (human.code !== machine.code) refuse('reference human/JSON exit disagreement');
    const raw = renderedOnly(human.stderr);
    const renderedCodes = [...raw.matchAll(/^(?:Error|Warning|Stop) \[([A-Z]\d+)\]/gmu)].map((match) => match[1]).sort();
    const machineCodes = diagnostics.map((row) => row.code).sort();
    if (!isDeepStrictEqual(renderedCodes, machineCodes)) {
      const warningSuccess = machine.code === 0 && !lint && /^(?:Warning|Stop) \[/mu.test(raw);
      refuse(`${warningSuccess ? 'success-with-warnings, non-lint: ' : ''}JSON/rendered diagnostic code disagreement: ${JSON.stringify({ machineCodes, renderedCodes })}`);
    }
    const normalize = (text) => text.replaceAll(`${dir}/`, '').replaceAll(`${root}/`, '').split(/\r?\n/u)
      .filter((line, i, lines) => !(i === lines.length - 1 && line === '') && !line.includes('Blocking waiting for file lock on package cache'))
      .map((line) => `${line}\n`).join('');
    if (lint && (machine.code !== 0 || !diagnostics.some((row) => row.severity.toLowerCase() === 'warning'))) refuse('lint fixture must compile and emit at least one lint');
    return { source, mode, code: machine.code, diagnostics, lint, stdout: human.stdout, stderr: human.stderr,
      rendered: lint || machine.code !== 0 ? normalize(raw) : '(no errors)\n' };
  };
  reference.invocations = invocations;
  reference.createProject = () => new InputPath({ directory: mkdtempSync(join(scratch, 'input-project-')), files: new Map() });
  return reference;
}

// Every observation is checked first; only then construct the write set. No
// partial cases when a later source/assertion fails. Existing cases are refused.
// The caller supplies its helper-aware classified row; never make a second
// disposition decision after lexical helper resolution.
export async function emitFunction(fn, options) {
  const root = options.root ?? ROOT;
  if (typeof fn.disposition !== 'string') refuse('emitFunction requires a classified function');
  if (fn.disposition === 'retire-with-Rust' && !options.attempted) refuse('Rust-only function must be retired, not emitted');
  const checked = await checkOriginal(fn, options.reference, root);
  const suite = fn.file.replace(/^tests\//u, '').replace(/\.rs$/u, '').replaceAll('/', '_');
  const files = [];
  checked.observations.forEach((observation, i) => {
    if (observation.allDiagnostics && observation.diagnostics.some(d => d.severity.toLowerCase() !== 'error')) refuse('all-code warning projection needs an explicit verbose CLI case, not an error-only UI snapshot');
    const name = i === 0 ? fn.name : `${fn.name}_${i + 1}`;
    const prefix = `tests/${observation.mode === 'run' ? 'run' : observation.lint ? 'ui_lint' : 'ui'}/${suite}/${name}`;
    const path = `${prefix}.jet`;
    if (observation.project) {
      for (const input of observation.project.files) files.push([`${prefix}/${input.path}`, input.content]);
      files.push([`${prefix}/${observation.lint ? 'warn' : 'stderr'}`, observation.rendered]);
      return;
    }
    files.push([path, observation.source]);
    if (observation.mode === 'compile') files.push([`${prefix}.${observation.lint ? 'warn' : 'stderr'}`, observation.rendered]);
    else {
      if (observation.code !== 0 && ![1, 70].includes(observation.code)) refuse(`golden cannot preserve exit ${observation.code}`);
      if (observation.code === 0) {
        files.push([`${prefix}.out`, observation.stdout]);
        if (observation.stderr || observation.readsStderr) files.push([`${prefix}.stderr.out`, observation.stderr]);
      }
      else {
        // Expected-failure goldens don't assert stdout or exact exit; retain a
        // .case sidecar that does, so no assertion is weakened on conversion.
        files.push([`${prefix}.err.out`, observation.stderr], [`${prefix}.stdout`, observation.stdout], [`${prefix}.case`, `step: built\nexit: ${observation.code}\nstdout: = ${name}.stdout\nstderr: = ${name}.err.out\n`]);
      }
    }
  });
  if (!options.dryRun) {
    for (const [path] of files) if (existsSync(join(root, path))) refuse(`case already exists: ${path}`);
    for (const [path, content] of files) { mkdirSync(dirname(join(root, path)), { recursive: true }); writeFileSync(join(root, path), content, { flag: 'wx' }); }
  }
  return { status: 'CONVERTED', function: `${fn.file}::${fn.name}`, assertions: checked.assertions,
    retiredAssertions: checked.retiredAssertions.length, retiredAssertionRows: checked.retiredAssertions,
    sources: checked.observations.length, paths: files.map(([path]) => path) };
}

export async function main(argv) {
  const options = { root: ROOT, dryRun: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--dry-run') options.dryRun = true;
    else if (['--root', '--inventory', '--census', '--emit', '--attempt', '--analyze', '--draft', '--converter-sha', '--slice-map', '--bless-from', '--scratch', '--report', '--exclusions', '--timeout'].includes(arg)) {
      if (!argv[i + 1]) refuse(`missing value for ${arg}`);
      options[arg.slice(2)] = argv[++i];
    } else refuse(`unknown option ${arg}`);
  }
  options.root = resolve(options.root);
  if (options.draft) options.draft = externalDraftRoot(options.draft, options.root);
  const timeout = options.timeout === undefined ? 600000 : Number(options.timeout) * 1000;
  if (!Number.isSafeInteger(timeout) || timeout <= 0) refuse('--timeout must be positive seconds with millisecond precision');
  const census = collectFunctions(options.root, options.inventory);
  if (options.census) { mkdirSync(dirname(resolve(options.census)), { recursive: true }); writeFileSync(options.census, censusTsv(census)); }
  if (options.exclusions) writeFileSync(options.exclusions, 'file\tline\treason\n' + census.excluded.map((row) => [row.file, row.line, row.reason].join('\t')).join('\n') + '\n');
  const counts = { files: census.files.length, functions: census.functions.length,
    parts: census.functions.filter((fn) => dirname(fn.file) !== 'tests').length,
    topLevel: census.functions.filter((fn) => dirname(fn.file) === 'tests').length, dispositions: {} };
  for (const fn of census.functions) { const { disposition } = classify(fn, census.helpers); counts.dispositions[disposition] = (counts.dispositions[disposition] ?? 0) + 1; }
  if (!options.emit && !options.attempt && !options.analyze && !options.draft) { console.log(JSON.stringify(counts)); return 0; }
  if (!options.analyze && !options['bless-from']) refuse('--emit/--attempt requires --bless-from JET');
  const attempts = new Set(options.attempt ? readFileSync(options.attempt, 'utf8').split(/\r?\n/u)
    .map((line) => line.split('\t')[0].trim()).filter((id) => id && id !== 'function' && !id.startsWith('#')) : []);
  if (options.attempt) {
    const known = new Set(census.functions.map((fn) => `${fn.file}::${fn.name}`));
    for (const id of attempts) if (!known.has(id)) refuse(`attempt function not found: ${id}`);
  }
  let file, name;
  if (options.emit) {
    const split = options.emit.lastIndexOf('::');
    if (split < 0) refuse('--emit expects FILE::FN (or FILE::*)');
    file = options.emit.slice(0, split); name = options.emit.slice(split + 2);
  }
  const selected = census.functions.filter((fn) => (!options.emit || fn.file === file && (name === '*' || name === fn.name))
    && (!options.attempt || attempts.has(`${fn.file}::${fn.name}`)));
  if (!selected.length) refuse(`function not found: ${options.emit ?? options.attempt}`);
  if (options.analyze) {
    const slices = new Map(options['slice-map'] ? readFileSync(options['slice-map'], 'utf8').trimEnd().split(/\r?\n/u).slice(1)
      .map(line => { const [slice, file] = line.split('\t'); return [file, slice]; }) : []);
    const rows = selected.map(fn => {
      const owning = fn.file.match(/^tests\/([a-z0-9_]+?)(?:_parts)?\/.+\.rs$/u);
      const slice = slices.get(fn.file) ?? (owning ? slices.get(`tests/${owning[1]}.rs`) : undefined);
      if (options['slice-map'] && !slice) refuse(`analysis slice missing: ${fn.file}`);
      return { ...analyzeFunction(classify(fn, census.helpers)), slice: slice ?? '-' };
    });
    writeFileSync(options.analyze, 'file\tfn\tslice\tclasses\treference\n' + rows.map(row =>
      [row.file, row.fn, row.slice, row.classes.map(name => name.replaceAll('\\', '\\\\').replaceAll(';', '\\;')).join(';'), row.reference].join('\t')).join('\n') + '\n');
    console.log(JSON.stringify({ analyzed: rows.length, staticallyClean: rows.filter(row => !row.classes.length).length, reference: 'runtime-unknown' }));
    return 0;
  }
  const scratchRoot = resolve(options.scratch ?? process.env.JET_SCRATCH_ROOT ?? join(process.env.HOME, '.cache/jet-dev/scratch/TestW4'));
  mkdirSync(scratchRoot, { recursive: true });
  const scratch = mkdtempSync(join(scratchRoot, 'test-port-'));
  const reference = makeReference(resolve(options['bless-from']), options.root, scratch, timeout, !!options.draft);
  const results = [];
  if (options.draft) {
    const slices = new Map(options['slice-map'] ? readFileSync(options['slice-map'], 'utf8').trimEnd().split(/\r?\n/u).slice(1)
      .map(line => { const [slice, file] = line.split('\t'); return [file, slice]; }) : []);
    const sha = options['converter-sha'] ?? spawnSync('git', ['rev-parse', 'HEAD'], { cwd: options.root, encoding: 'utf8' }).stdout?.trim();
    if (!sha) refuse('cannot determine converter sha for draft');
    try {
      for (const fn of selected) {
        const owning = fn.file.match(/^tests\/([a-z0-9_]+?)(?:_parts)?\/.+\.rs$/u);
        const slice = slices.get(fn.file) ?? (owning ? slices.get(`tests/${owning[1]}.rs`) : undefined) ?? 'unassigned';
        if (options['slice-map'] && slice === 'unassigned') refuse(`draft slice missing: ${fn.file}`);
        const result = await draftFunction(classify(fn, census.helpers), { ...options, reference, binary: resolve(options['bless-from']), converterSha: sha, slice });
        results.push(result); console.log(JSON.stringify(result));
        const rows = results.filter(row => row.slice === slice).sort((a, b) => a.todoCount - b.todoCount || b.capturedSteps - a.capturedSteps);
        writeFileSync(join(options.draft, slice, 'index.tsv'), 'fn\tdraft\tTODO_count\tcaptured_step_count\n' +
          rows.map(row => [row.function, row.draft, row.todoCount, row.capturedSteps].join('\t')).join('\n') + '\n');
      }
    } finally { rmSync(scratch, { recursive: true, force: true }); }
    const summary = { selected: selected.length, drafted: results.length, converted: 0, underlyingStatus: 'HAND-PORT',
      slices: Object.fromEntries([...new Set(results.map(row => row.slice))].map(slice => {
        const rows = results.filter(row => row.slice === slice);
        return [slice, { drafted: rows.length, TODOs: rows.reduce((sum, row) => sum + row.todoCount, 0), capturedSteps: rows.reduce((sum, row) => sum + row.capturedSteps, 0) }];
      })) };
    if (options.report) writeFileSync(options.report, JSON.stringify({ summary, results }, null, 2) + '\n');
    console.log(JSON.stringify(summary)); return 0;
  }
  try {
    for (const fn of selected) {
      const classified = classify(fn, census.helpers);
      const invocationStart = reference.invocations.length;
      let result;
      const retiredAssertions = retireRustAssertions(fn).retired.length;
      const attempted = attempts.has(`${fn.file}::${fn.name}`);
      if (classified.disposition === 'retire-with-Rust' && !attempted) result = { status: 'RETIRED', function: `${fn.file}::${fn.name}`, reason: classified.target };
      else {
        try { result = await emitFunction(classified, { ...options, reference, attempted }); }
        catch (error) {
          const primaryEdit = error.status !== 'ERROR' && /\.(?:edit|safety|applicability)\b/u.test(fn.body);
          const reason = primaryEdit ? `asserts primary Diagnostic.edit / Diagnostic.safety; jet.report/v3 exposes only all_edits (${error.message})` : error.message;
          result = { status: error.status === 'ERROR' ? 'ERROR' : 'HAND-PORT', function: `${fn.file}::${fn.name}`, reason };
        }
      }
      result.disposition = classified.disposition; result.target = classified.target;
      if (attempted) result.attempted = true;
      result.retiredAssertions = retiredAssertions;
      result.referenceInvocations = reference.invocations.slice(invocationStart);
      result.elapsed_ms = result.referenceInvocations.reduce((sum, invocation) => sum + invocation.elapsed_ms, 0);
      result.mechanicallyEligible = result.status === 'CONVERTED';
      if (result.status === 'HAND-PORT') {
        try { assertionPlan(fn); result.mechanicallyEligible = true; } catch { /* Exact parse blocker remains in reason. */ }
      }
      results.push(result); console.log(JSON.stringify(result));
    }
  } finally { rmSync(scratch, { recursive: true, force: true }); }
  const summary = { ...counts, selected: selected.length, converted: results.filter((row) => row.status === 'CONVERTED').length,
    handPort: results.filter((row) => row.status === 'HAND-PORT').length, retired: results.filter((row) => row.status === 'RETIRED').length,
    errors: results.filter((row) => row.status === 'ERROR').length, eligible: results.filter((row) => row.mechanicallyEligible).length,
    timeout_ms: timeout, retiredAssertions: results.reduce((sum, row) => sum + row.retiredAssertions, 0) };
  if (options.report) writeFileSync(options.report, JSON.stringify({ summary, results }, null, 2) + '\n');
  console.log(JSON.stringify(summary));
  return summary.errors || (name !== '*' && summary.handPort) ? 2 : 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).then((code) => { process.exitCode = code; }).catch((error) => { console.error(`REFUSED: ${error.message}`); process.exitCode = 2; });
}
