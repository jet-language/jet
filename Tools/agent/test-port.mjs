#!/usr/bin/env node
// Transitional Rust-test porter. Removal point: M7, when tests/**/*.rs is empty.
// This is migration tooling, not the test harness: Tools/testkit is the Jet kit.
// node Tools/agent/test-port.mjs --census OUT.tsv [--inventory TESTS.tsv]
// node Tools/agent/test-port.mjs --emit tests/ownership.rs::NAME --bless-from JET [--dry-run]
// --emit FILE::* checks every function, keeps going, and reports hand-port refusals.
// Unsupported syntax/API facts and failed original assertions are HAND-PORT,
// not permission to weaken a test. RETIRED rows name Rust-only subjects.
// --report FILE records each decision; --exclusions FILE lists raw #[test]
// occurrences hidden in strings/comments. Source matrices retain every row.
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
      tokens: tokens.slice(open + 1, end), body: text.slice(tokens[open].end, tokens[end].start) });
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
  const rustLaw = /selfhost_(?:parser|terminators)|suite_membership|ban_bare_panic|ban_jetpack_unwrap_expect/u.test(fn.file);
  const jetLaw = !rustLaw && /corpus_policy|diagnostic_(?:gate|pages)|(?:ledger|registry)_.*(?:policy|coverage)|case_law/u.test(`${fn.file} ${fn.name}`);
  if (jetLaw) { disposition = 'guard'; target = `tests/guards/${suite}.jet#${fn.name}`; }
  else if (surfaces.includes('compile') && /\.\s*(?:edit|safety|applicability)\b/u.test(code)) { disposition = 'CLI case'; target = `tests/cases/${suite}/${fn.name}.case`; }
  else if (surfaces.includes('run')) { disposition = 'golden'; target = `tests/run/${suite}/${fn.name}.jet`; }
  else if (surfaces.includes('cli')) { disposition = 'CLI case'; target = `tests/cases/${suite}/${fn.name}.case`; }
  else if (surfaces.includes('rust') || rustLaw || (surfaces.includes('scan') && /\.rs["']/u.test(body) && !/\.jet["']|Docs\//u.test(body))) { disposition = 'retire-with-Rust'; target = 'none: Rust emitter, crate internals, Rust law or cargo partition'; }
  else if (surfaces.includes('compile')) { disposition = 'UI'; target = `tests/ui/${suite}/${fn.name}${/\bcompile_with_path\b/u.test(code) ? '/' : '.jet'}`; }
  else if (surfaces.includes('scan') && !surfaces.includes('internal')) { disposition = 'guard'; target = `tests/guards/${suite}.jet#${fn.name}`; }
  else { disposition = 'retire-with-Rust'; target = 'none: Rust crate implementation or harness unit'; }
  return { ...fn, surfaces: surfaces.join(','), disposition, target };
}

export function censusTsv(census) {
  return 'file\tfn\tlines\tsurfaces\tdisposition\ttarget\n' + census.functions.map((fn) => {
    const row = classify(fn, census.helpers);
    return [row.file, row.name, `${row.line}-${row.endLine}`, row.surfaces || '-', row.disposition, row.target].join('\t');
  }).join('\n') + '\n';
}

// A deliberately closed Rust expression subset. Unknown syntax, unknown calls,
// unmodeled control flow and generated-Rust assertions are hand-port, NEVER
// assumed true. Parse the complete function before invoking or writing anything.
class Parser {
  constructor(tokens) { this.tokens = tokens; this.i = 0; }
  peek(value) { return this.tokens[this.i]?.value === value; }
  take() { return this.tokens[this.i++] ?? refuse('unexpected end of expression'); }
  need(value) { if (!this.peek(value)) refuse(`expected ${value}, got ${this.tokens[this.i]?.raw ?? 'end'}`); return this.take(); }
  group(open = '(', close = ')') {
    this.need(open); const args = [];
    while (!this.peek(close)) { args.push(this.expr()); if (!this.peek(',')) break; this.take(); }
    this.need(close); return args;
  }
  expr(min = 0) {
    let node;
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
        node = this.peek('(') ? { type: 'call', name, args: this.group() } : { type: 'variable', name };
      } else refuse(`unmodeled token ${token.raw} at line ${token.line}`);
    }
    while (this.i < this.tokens.length) {
      if (this.peek('.')) {
        this.take(); const name = this.take().value;
        node = this.peek('(') ? { type: 'method', object: node, name, args: this.group() } : { type: 'field', object: node, name };
      } else if (this.peek('[')) { const [index] = this.group('[', ']'); node = { type: 'index', object: node, index }; }
      else {
        const op = this.tokens[this.i]?.value;
        const precedence = { '||': 1, '&&': 2, '|': 3, '==': 4, '!=': 4, '>': 4, '<': 4, '>=': 4, '<=': 4 }[op];
        if (!precedence || precedence < min) break;
        this.take(); node = { type: 'binary', op, left: node, right: this.expr(precedence + 1) };
      }
    }
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

function checkAst(node) {
  if (!node) return;
  const allowedCalls = ['jet::compile', 'jet::compile_with_path', 'include_str!', 'assert!', 'assert_eq!', 'assert_ne!', 'matches!', 'panic!', 'format!', 'common::unique_tmp', 'fs::create_dir_all', 'fs::write', 'common::build_and_run', 'tir_support::assert_tiers_agree'];
  const allowedMethods = ['iter', 'any', 'all', 'find', 'filter', 'collect', 'count', 'len', 'is_empty', 'contains', 'starts_with', 'ends_with', 'as_str', 'expect', 'expect_err', 'unwrap', 'unwrap_err', 'unwrap_or_else', 'is_ok', 'is_err', 'is_some', 'is_none', 'trim', 'replace', 'to_string', 'to_owned', 'join', 'to_str'];
  if (node.type === 'call' && !allowedCalls.includes(node.name)) refuse(`unmodeled call ${node.name}`);
  if (node.type === 'method' && !allowedMethods.includes(node.name)) refuse(`unmodeled method ${node.name}`);
  if (node.type === 'field' && !['lints', 'code', 'what', 'why', 'fix', 'severity'].includes(node.name)) refuse(`unmodeled field ${node.name}`);
  // Message operands are never assertions; panic's arguments are only failure text.
  const assertion = node.type === 'call' && ['assert!', 'assert_eq!', 'assert_ne!'].includes(node.name);
  for (const [key, value] of Object.entries(node)) {
    if (key === 'args' && assertion) value.slice(0, node.name === 'assert!' ? 1 : 2).forEach(checkAst);
    else if (key === 'args' && node.type === 'call' && node.name === 'panic!') continue;
    else if (Array.isArray(value)) value.forEach(checkAst);
    else if (value && typeof value === 'object') checkAst(value);
  }
}

function invoke(binary, args, cwd, timeout) {
  const run = spawnSync(binary, args, { cwd, encoding: 'utf8', timeout, maxBuffer: 16 * 1024 * 1024,
    env: { ...process.env, NO_COLOR: '1', CLICOLOR: '0', JET_NO_UPDATE_CHECK: '1' } });
  if (run.error || run.signal || run.status === null || run.status === 101) refuse(`reference invocation failed: ${run.error?.message ?? run.signal ?? run.status}`);
  return { code: run.status, stdout: run.stdout, stderr: run.stderr };
}

function machineDiagnostics(run) {
  const result = [];
  for (const line of `${run.stdout}\n${run.stderr}`.split('\n').filter((line) => line.trim())) {
    let row; try { row = JSON.parse(line); } catch { refuse(`reference did not emit a JSON report: ${line.slice(0, 120)}`); }
    if (row.schema === 'jet.status/v1' && row.ok === true) continue;
    if (row.schema !== 'jet.report/v3' || typeof row.code !== 'string' || typeof row.what !== 'string' || typeof row.why !== 'string' || typeof row.fix !== 'string') refuse('unknown machine diagnostic schema');
    result.push(row);
  }
  return result;
}

export function assertionPlan(fn) {
  const statements = new Parser(fn.tokens ?? lexRust(fn.body)).statements();
  for (const statement of statements) checkAst(statement);
  return statements;
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
      if (node.name === 'common::build_and_run') {
        if (args.length !== 3 || typeof args[2] !== 'string') refuse('unsupported run arguments');
        const observed = await context.observe(args[2], 'run'); context.observations.push(observed);
        return [observed.code, observed.stdout, observed.stderr];
      }
      if (node.name === 'tir_support::assert_tiers_agree') {
        // A single reference run is NOT evidence of the original all-tier assertion.
        refuse('all-tier helper requires a hand port and per-tier evidence');
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

export async function checkOriginal(fn, reference, root = ROOT) {
  const plan = assertionPlan(fn), env = {};
  const context = { fn, root, reference, observations: [], assertions: 0 };
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
  const verbose = nodes.some((node) => node.type === 'field' && node.name === 'lints');
  const suite = fn.file.replace(/^tests\//u, '').replace(/\.rs$/u, '').replaceAll('/', '_');
  let ordinal = 0;
  context.observe = async (source, mode, project = null) => {
    const name = ++ordinal === 1 ? fn.name : `${fn.name}_${ordinal}`;
    const category = mode === 'run' ? 'run' : verbose ? 'cases' : 'ui';
    const entry = project?.entry ?? `${name}.jet`;
    const path = category === 'cases' ? `tests/cases/${suite}/${name}.fixture/${entry}` : project ? `tests/${category}/${suite}/${name}/${entry}` : `tests/${category}/${suite}/${name}.jet`;
    const observation = await reference(source, mode, { path, ...(project ? { project } : {}), verbose: mode === 'compile' && verbose });
    return { ...observation, project };
  };
  const execute = async (statements, local) => {
    for (const statement of statements) {
      if (statement.type === 'loop') {
        const values = await evaluate(statement.iterable, local, context);
        if (!Array.isArray(values)) refuse('loop iterable is not a finite extracted array');
        for (const value of values) {
          if (statement.names.length > 1 && (!Array.isArray(value) || value.length !== statement.names.length)) refuse('loop tuple arity mismatch');
          const inner = { ...local };
          statement.names.forEach((name, i) => { inner[name] = statement.names.length === 1 ? value : value[i]; });
          await execute(statement.statements, inner);
        }
      } else {
        const value = await evaluate(statement.expr, local, context);
        if (statement.names) {
          if (statement.names.length > 1 && (!Array.isArray(value) || value.length !== statement.names.length)) refuse('tuple arity mismatch');
          statement.names.forEach((name, i) => { local[name] = statement.names.length === 1 ? value : value[i]; });
        }
      }
    }
  };
  await execute(plan, env);
  if (!context.observations.length) refuse('no supported Jet source invocation');
  return context;
}

function renderedOnly(text) {
  return text.replace(/\n\n(?:\d+ problems? found|ok: `[^\n]+` has no errors \(\d+ warnings?\))\nrun `jet explain [^\n]+` to learn more\n$/u, '\n');
}

export function makeReference(binary, root, scratch, timeout = 60000) {
  let index = 0;
  const reference = async (source, mode, metadata = {}) => {
    const dir = join(scratch, `source-${index++}`); mkdirSync(dir, { recursive: true });
    const shown = metadata.path ?? 'tests/ui/test_port/run.jet', file = join(dir, shown);
    if (metadata.project) for (const input of metadata.project.files) {
      const target = join(dirname(file), relative(dirname(metadata.project.entry), input.path));
      mkdirSync(dirname(target), { recursive: true }); writeFileSync(target, input.content);
    }
    mkdirSync(dirname(file), { recursive: true }); writeFileSync(file, source);
    if (mode === 'run') return { source, mode, ...invoke(binary, ['run', shown], dir, timeout) };
    const verbose = metadata.verbose === true;
    const cwd = verbose ? dirname(file) : dir, argument = verbose ? basename(file) : shown;
    const flags = verbose ? ['--verbose'] : [];
    const machine = invoke(binary, ['check', '--json', ...flags, argument], cwd, timeout);
    const diagnostics = machineDiagnostics(machine);
    if (machine.code !== 0 && !diagnostics.some((row) => row.severity.toLowerCase() === 'error')) refuse('failed check without an error diagnostic');
    const human = invoke(binary, ['check', '--no-color', ...flags, argument], cwd, timeout);
    if (human.code !== machine.code) refuse('reference human/JSON exit disagreement');
    const raw = renderedOnly(`${human.stdout}${human.stderr}`);
    const renderedCodes = [...raw.matchAll(/^(?:Error|Warning|Stop) \[([A-Z]\d+)\]/gmu)].map((match) => match[1]).sort();
    const machineCodes = diagnostics.map((row) => row.code).sort();
    if (!isDeepStrictEqual(renderedCodes, machineCodes)) refuse(`JSON/rendered diagnostic code disagreement: ${JSON.stringify({ machineCodes, renderedCodes })}`);
    const normalize = (text) => text.replaceAll(`${dir}/`, '').replaceAll(`${root}/`, '').split(/\r?\n/u)
      .filter((line, i, lines) => !(i === lines.length - 1 && line === '') && !line.includes('Blocking waiting for file lock on package cache'))
      .map((line) => `${line}\n`).join('');
    const caseStream = (text) => text.replaceAll(cwd, '{work}').replaceAll(root, '{root}').replaceAll(scratch, '{scratch}');
    return { source, mode, code: machine.code, diagnostics, verbose,
      stdout: verbose ? caseStream(human.stdout) : human.stdout, stderr: verbose ? caseStream(human.stderr) : human.stderr,
      rendered: diagnostics.length ? normalize(raw) : '(no errors)\n' };
  };
  reference.createProject = () => new InputPath({ directory: mkdtempSync(join(scratch, 'input-project-')), files: new Map() });
  return reference;
}

// Every observation is checked first; only then construct the write set. No
// partial cases when a later source/assertion fails. Existing cases are refused.
export async function emitFunction(fn, options) {
  const root = options.root ?? ROOT;
  const checked = await checkOriginal(fn, options.reference, root);
  const suite = fn.file.replace(/^tests\//u, '').replace(/\.rs$/u, '').replaceAll('/', '_');
  const files = [];
  checked.observations.forEach((observation, i) => {
    const name = i === 0 ? fn.name : `${fn.name}_${i + 1}`;
    const prefix = `tests/${observation.mode === 'run' ? 'run' : 'ui'}/${suite}/${name}`;
    const path = `${prefix}.jet`;
    if (observation.project && !observation.verbose) {
      for (const input of observation.project.files) files.push([`${prefix}/${input.path}`, input.content]);
      files.push([`${prefix}/stderr`, observation.rendered]);
      return;
    }
    // Lint assertions require verbose output: preserve that invocation as a
    // CLI case instead of adding a second meaning to the plain UI convention.
    if (observation.mode === 'compile' && observation.verbose) {
      const casePrefix = `tests/cases/${suite}/${name}`;
      if (observation.project) for (const input of observation.project.files) files.push([`${casePrefix}.fixture/${input.path}`, input.content]);
      else files.push([`${casePrefix}.fixture/${name}.jet`, observation.source]);
      files.push([`${casePrefix}.stdout`, observation.stdout], [`${casePrefix}.stderr`, observation.stderr],
        [`${casePrefix}.case`, `fixture: ${name}.fixture\nstep: jet check --verbose ${observation.project?.entry ?? `${name}.jet`}\nexit: ${observation.code}\nstdout: = ${name}.stdout\nstderr: = ${name}.stderr\n`]);
      return;
    }
    files.push([path, observation.source]);
    if (observation.mode === 'compile') files.push([`${prefix}.stderr`, observation.rendered]);
    else {
      if (observation.code !== 0 && ![1, 70].includes(observation.code)) refuse(`golden cannot preserve exit ${observation.code}`);
      if (observation.code === 0) { files.push([`${prefix}.out`, observation.stdout], [`${prefix}.err.out`, observation.stderr]); }
      else {
        // Expected-failure goldens don't assert stdout or exact exit; retain a
        // .case sidecar that does, so no assertion is weakened on conversion.
        files.push([`${prefix}.err`, observation.stderr], [`${prefix}.out`, observation.stdout], [`${prefix}.case`, `step: built\nexit: ${observation.code}\nstdout: = ${name}.out\nstderr: = ${name}.err\n`]);
      }
    }
  });
  if (!options.dryRun) {
    for (const [path] of files) if (existsSync(join(root, path))) refuse(`case already exists: ${path}`);
    for (const [path, content] of files) { mkdirSync(dirname(join(root, path)), { recursive: true }); writeFileSync(join(root, path), content, { flag: 'wx' }); }
  }
  return { status: 'CONVERTED', function: `${fn.file}::${fn.name}`, assertions: checked.assertions, sources: checked.observations.length, paths: files.map(([path]) => path) };
}

export async function main(argv) {
  const options = { root: ROOT, dryRun: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--dry-run') options.dryRun = true;
    else if (['--root', '--inventory', '--census', '--emit', '--bless-from', '--scratch', '--report', '--exclusions'].includes(arg)) {
      if (!argv[i + 1]) refuse(`missing value for ${arg}`);
      options[arg.slice(2)] = argv[++i];
    } else refuse(`unknown option ${arg}`);
  }
  options.root = resolve(options.root);
  const census = collectFunctions(options.root, options.inventory);
  if (options.census) { mkdirSync(dirname(resolve(options.census)), { recursive: true }); writeFileSync(options.census, censusTsv(census)); }
  if (options.exclusions) writeFileSync(options.exclusions, 'file\tline\treason\n' + census.excluded.map((row) => [row.file, row.line, row.reason].join('\t')).join('\n') + '\n');
  const counts = { files: census.files.length, functions: census.functions.length,
    parts: census.functions.filter((fn) => dirname(fn.file) !== 'tests').length,
    topLevel: census.functions.filter((fn) => dirname(fn.file) === 'tests').length, dispositions: {} };
  for (const fn of census.functions) { const { disposition } = classify(fn, census.helpers); counts.dispositions[disposition] = (counts.dispositions[disposition] ?? 0) + 1; }
  if (!options.emit) { console.log(JSON.stringify(counts)); return 0; }
  if (!options['bless-from']) refuse('--emit requires --bless-from JET');
  const split = options.emit.lastIndexOf('::');
  if (split < 0) refuse('--emit expects FILE::FN (or FILE::*)');
  const file = options.emit.slice(0, split), name = options.emit.slice(split + 2);
  const selected = census.functions.filter((fn) => fn.file === file && (name === '*' || name === fn.name));
  if (!selected.length) refuse(`function not found: ${options.emit}`);
  const scratchRoot = resolve(options.scratch ?? process.env.JET_SCRATCH_ROOT ?? join(process.env.HOME, '.cache/jet-dev/scratch/TestW4'));
  mkdirSync(scratchRoot, { recursive: true });
  const scratch = mkdtempSync(join(scratchRoot, 'test-port-'));
  const reference = makeReference(resolve(options['bless-from']), options.root, scratch);
  const results = [];
  try {
    for (const fn of selected) {
      const classified = classify(fn, census.helpers);
      let result;
      if (classified.disposition === 'retire-with-Rust') result = { status: 'RETIRED', function: `${fn.file}::${fn.name}`, reason: classified.target };
      else {
        try { result = await emitFunction(fn, { ...options, reference }); }
        catch (error) {
          const primaryEdit = /\.(?:edit|safety|applicability)\b/u.test(fn.body);
          const reason = primaryEdit ? `asserts primary Diagnostic.edit / Diagnostic.safety; jet.report/v3 exposes only all_edits (${error.message})` : error.message;
          result = { status: 'HAND-PORT', function: `${fn.file}::${fn.name}`, reason };
        }
      }
      result.disposition = classified.disposition; result.target = classified.target;
      results.push(result); console.log(JSON.stringify(result));
    }
  } finally { rmSync(scratch, { recursive: true, force: true }); }
  const summary = { ...counts, selected: selected.length, converted: results.filter((row) => row.status === 'CONVERTED').length,
    handPort: results.filter((row) => row.status === 'HAND-PORT').length, retired: results.filter((row) => row.status === 'RETIRED').length };
  if (options.report) writeFileSync(options.report, JSON.stringify({ summary, results }, null, 2) + '\n');
  console.log(JSON.stringify(summary));
  return name === '*' ? 0 : results.some((row) => row.status === 'HAND-PORT') ? 2 : 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).then((code) => { process.exitCode = code; }).catch((error) => { console.error(`REFUSED: ${error.message}`); process.exitCode = 2; });
}
