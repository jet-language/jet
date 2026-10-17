#!/usr/bin/env node
// D-COPY-WORD1: migrate the retired prefix ~ to the built-in .copy() method.
// Usage: node Tools/agent/migrate-copy-sigil.mjs --check|--write [file/directory ...]
//        node Tools/agent/migrate-copy-sigil.mjs --self-test
// File owners only: --owner-write /absolute/owned/file.jet [...] explicitly
// authorizes writes to those dirty files; directories and implicit tree denied.
// --check lists every site (including dirty files), with package totals; exits 1
// when sites remain. --write refuses tracked/untracked dirty files, recording
// them in ~/.cache/jet-dev/scratch/CopySigilMigration/dirty-files.tsv.
// Concurrent owners may reserve clean paths in scratch/reserved-files.tsv
// (one repo-relative path per line). These are recorded, not overwritten.
// Lexer reference: JetLexer/Source/Lexer/{Scan,Payload}.jet. Parser reference:
// JetParser/Source/Parser/Expressions.jet: prefix minimum 13, power 14,
// postfix calls/fields/indexes/optional fields/?; ! is NOT a postfix operator.
// Single TOKEN_TILDE has no type, pattern, effect-row or binary meaning in the
// current parser. Distinct ~~ (retired trait attachment), ~| and ~|= are kept.
// Interpolation holes in ordinary strings are migrated as expressions; plain
// string text, raw strings (URL{"…"}), chars, comments, prep template bodies and
// template holes (\${…}) are opaque. E0941 UI fixtures are recognized by their
// adjacent .stderr snapshot or their explicit E0941 fixture name.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const SCRATCH = resolve(homedir(), '.cache/jet-dev/scratch/CopySigilMigration');
const operators = ['~|=', '<<=', '>>=', '<=>', '...', '..<', '/%=', '%%=', '~~', '~|', ':=', '::', '->', '=>', '?.', '??', '++', '--', '==', '!=', '<=', '>=', '&&', '||', '<<', '>>', '+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '/%', '%%', '..', '<:', ':>'];
const idStart = /[\p{L}_$]/u, idContinue = /[\p{L}\p{N}_$]/u;
const pairs = new Map([['(', ')'], ['[', ']'], ['{', '}']]);

function commentEnd(s, i) {
  if (s.startsWith('//', i) || s.startsWith('#!/', i)) {
    const end = s.indexOf('\n', i); return end < 0 ? s.length : end;
  }
  let depth = 1; i += 2;
  while (i < s.length && depth) {
    if (s.startsWith('/*', i)) { depth++; i += 2; }
    else if (s.startsWith('*/', i)) { depth--; i += 2; }
    else i++;
  }
  return i;
}
function fenceEnd(s, i) {
  const start = i; while (s[i] === '`') i++;
  const width = i - start;
  while (i < s.length) {
    if (s[i] !== '`') { i++; continue; }
    const run = i; while (s[i] === '`') i++;
    if (i - run === width) return i;
  }
  return i;
}
function charEnd(s, i) {
  i++;
  if (s[i] === '\\') i += 2;
  else if (i < s.length) i += String.fromCodePoint(s.codePointAt(i)).length;
  while (i < s.length && s[i] !== "'") i++;
  return Math.min(i + 1, s.length);
}
function holeEnd(s, i, triple, template = false, spans) {
  const open = i; let depth = 1; i++;
  while (i < s.length && depth) {
    if (!triple && s[i] === '\n') return i;
    if (!template && (s.startsWith('//', i) || s.startsWith('/*', i))) i = commentEnd(s, i);
    else if (!template && s[i] === '"') i = stringEnd(s, i, false);
    else if (!template && s[i] === "'") i = charEnd(s, i);
    else if (!template && s[i] === '`') i = fenceEnd(s, i);
    else if (s[i] === '{') { depth++; i++; }
    else if (s[i] === '}') { depth--; i++; if (!depth) spans?.push({ open, close: i - 1, template }); }
    else i++;
  }
  return i;
}
function stringEnd(s, start, raw, spans) {
  const triple = s.startsWith('"""', start), quote = triple ? '"""' : '"';
  let i = start + quote.length;
  while (i < s.length) {
    if (!triple && s[i] === '\n') return i;
    if (s.startsWith(quote, i)) return i + quote.length;
    if (s.startsWith('{{', i) || s.startsWith('}}', i)) { i += 2; continue; }
    if (s[i] === '{') { i = holeEnd(s, i, triple, s.slice(i - 2, i) === '\\$', spans); continue; }
    if (s[i] === '\\' && (!raw || s[i + 1] === '"')) i += 2;
    else i++;
  }
  return i;
}
const suppressed = new Set(['.', '?.', '&&', '||', '+', '-', '*', '/', '/%', '%', '%%', '==', '!=', '<', '>', '<=', '>=', '<=>', '&', '|', '^', '~|', '<<', '>>', '??', ':>']);
function endsStatement(t) {
  return t.kind === 'literal' || (t.kind === 'identifier' && !['fn', 'if', 'else', 'loop', 'use', 'struct', 'enum', 'trait', 'in'].includes(t.text)) || [')', ']', '}', '?', '!', '++', '--', '>', '>>', ':>'].includes(t.text);
}
function dotArm(ts, n) {
  if (ts[n].text !== '.') return false;
  let k = n + 1;
  if (ts[k]?.text === '{') return ['->', '=>'].includes(ts[ts[k].close + 1]?.text);
  if (!/^[A-Z]/.test(ts[k]?.text ?? '') && ts[k]?.text !== 'null') return false;
  while (k < ts.length) {
    k++;
    while (ts[k]?.text === '.' && /^[A-Z]/.test(ts[k + 1]?.text ?? '')) k += 2;
    if (ts[k]?.text === '(' && ts[k].close !== undefined) k = ts[k].close + 1;
    if (ts[k]?.text !== '|') break;
    k++;
    if (ts[k]?.text !== '.') return false;
    k++;
  }
  return ['->', '=>', 'if'].includes(ts[k]?.text);
}
export function tokenize(source) {
  const tokens = []; let i = 0;
  while (i < source.length) {
    const start = i, c = String.fromCodePoint(source.codePointAt(i));
    if (/\s/u.test(c)) { i += c.length; continue; }
    if (source.startsWith('//', i) || source.startsWith('/*', i) || source.startsWith('#!/', i)) { i = commentEnd(source, i); continue; }
    let kind = 'symbol';
    if (c === '"') {
      const previous = tokens.at(-1), head = tokens.at(-2);
      const raw = previous?.text === '{' && (head?.text === '.' || /^[A-Z]/.test(head?.text ?? ''));
      i = stringEnd(source, i, raw); kind = 'literal';
    } else if (c === '`') { i = fenceEnd(source, i); kind = 'literal'; }
    else if (c === "'") { i = charEnd(source, i); kind = 'literal'; }
    else if (idStart.test(c)) {
      i += c.length;
      while (i < source.length) {
        const next = String.fromCodePoint(source.codePointAt(i));
        if (!idContinue.test(next)) break;
        i += next.length;
      }
      kind = 'identifier';
    } else if (/[0-9]/.test(c)) {
      const number = source.slice(i).match(/^(?:0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|[0-9][0-9_]*(?:\.[0-9][0-9_]*)?(?:[eE][+-]?[0-9_]+)?)(?:[\p{L}_][\p{L}\p{N}_]*)?/u);
      i += number[0].length; kind = 'literal';
    } else {
      const operator = operators.find((op) => source.startsWith(op, i));
      i += operator?.length ?? c.length;
    }
    tokens.push({ text: source.slice(start, i), start, end: i, kind });
  }
  const stack = [];
  for (let n = 0; n < tokens.length; n++) {
    const t = tokens[n];
    if (pairs.has(t.text)) stack.push(n);
    else if ([...pairs.values()].includes(t.text)) {
      const open = stack.at(-1);
      if (open !== undefined && pairs.get(tokens[open].text) === t.text) {
        stack.pop(); tokens[open].close = n;
      }
    }
  }
  for (let n = 1; n < tokens.length; n++) {
    const prev = tokens[n - 1], t = tokens[n];
    if (!source.slice(prev.end, t.start).includes('\n') || !endsStatement(prev)) continue;
    const place = ['&', '^'].includes(t.text) && tokens[n + 1]?.kind === 'identifier' && t.end === tokens[n + 1].start;
    const scope = t.text === '.' && (tokens[n + 2]?.text === '{' || (tokens[n + 2]?.text === '(' && tokens[tokens[n + 2].close + 1]?.text === '{'));
    t.beforeTerminator = (!suppressed.has(t.text) || dotArm(tokens, n) || scope || place) && ![')', ']'].includes(t.text);
  }
  // prep code templates are not ordinary runtime expressions. Balance their
  // first body, rather than matching text (braces in strings are already opaque).
  for (let n = 0; n < tokens.length; n++) {
    if (tokens[n].text !== 'prep' || tokens[n - 1]?.text === '.') continue;
    let body = n + 1;
    while (body < tokens.length && !['{', ';'].includes(tokens[body].text)) body++;
    if (tokens[body]?.text === '{' && tokens[body].close !== undefined) {
      for (let k = n; k <= tokens[body].close; k++) tokens[k].template = true;
      n = tokens[body].close;
    }
  }
  return tokens;
}

const prefixBefore = new Set(['(', '[', '{', ',', ';', ':', '=', ':=', '::', '->', '=>', '+', '-', '*', '/', '/%', '%', '%%', '&', '|', '^', '~|', '~|=', '!', '~', '&&', '||', '??', '==', '!=', '<', '>', '<=', '>=', '<=>', '<<', '>>', '..', '..<', '...', '+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '<<=', '>>=', 'return', 'if', 'else', 'in', 'yield', 'break', 'copy', 'shared']);
function isPrefix(source, ts, n) {
  const prev = ts[n - 1];
  return !prev || prefixBefore.has(prev.text) || source.slice(prev.end, ts[n].start).includes('\n');
}
function operand(ts, n) {
  const start = n, t = ts[n];
  if (!t) throw new Error('copy sigil has no operand');
  let simple = true;
  if (['~', '-', '!', '*', '&', '^', '...', 'copy', 'shared', '++', '--'].includes(t.text)) {
    const inner = operand(ts, n + 1); n = inner.next; simple = false;
  } else if (pairs.has(t.text)) {
    if (t.close === undefined) throw new Error(`unclosed operand ${t.text}`);
    n = t.close + 1;
  } else if (t.text === '.' && ts[n + 1]?.kind === 'identifier') n += 2;
  else if (t.kind === 'identifier' || t.kind === 'literal') n++;
  else throw new Error(`unsupported copy operand ${t.text}`);
  while (n < ts.length) {
    const next = ts[n];
    if (next.beforeTerminator) break;
    if (['(', '[', '{'].includes(next.text) && next.close !== undefined) { n = next.close + 1; continue; }
    if (['.', '?.'].includes(next.text) && (ts[n + 1]?.kind === 'identifier' || ts[n + 1]?.text === '*')) { n += 2; continue; }
    if (next.text === '?') { n++; continue; }
    if (next.text === '<') {
      // Parser's turbofish lookahead: consume only a balanced type argument
      // group followed by a call, literal body, or capitalized-owner member.
      let k = n + 1, depth = 1;
      for (; k < ts.length && depth; k++) {
        if (ts[k].text === '<') depth++;
        if (ts[k].text === '>') depth--;
        if (ts[k].text === '>>') depth -= 2;
        if ([';', '=', '->', '{', '}'].includes(ts[k].text)) break;
      }
      if (depth === 0 && (['(', '{'].includes(ts[k]?.text) || (ts[k]?.text === '.' && /^[A-Z]/.test(t.text)))) { n = k; continue; }
    }
    break;
  }
  // Prefix minimum 13 includes power (14), unlike the other binary operators.
  if (ts[n]?.text === '^' && !ts[n].beforeTerminator) { n = operand(ts, n + 1).next; simple = false; }
  return { next: n, end: ts[n - 1].end, simple, start: ts[start].start };
}
function collect(text, base, source, sites) {
  const ts = tokenize(text);
  for (let n = 0; n < ts.length; n++) {
    const t = ts[n];
    if (t.kind === 'literal' && t.text.startsWith('"') && !t.template) {
      // Interpolation holes are ordinary expressions; raw strings stay opaque.
      const raw = ts[n - 1]?.text === '{' && (ts[n - 2]?.text === '.' || /^[A-Z]/.test(ts[n - 2]?.text ?? ''));
      if (raw) continue;
      const spans = [];
      stringEnd(text, t.start, false, spans);
      for (const hole of spans) if (!hole.template) collect(text.slice(hole.open + 1, hole.close), base + hole.open + 1, source, sites);
      continue;
    }
    if (t.text !== '~' || t.template || !isPrefix(text, ts, n)) continue;
    const start = base + t.start, line = source.slice(0, start).split('\n').length;
    let value;
    try { value = operand(ts, n + 1); }
    catch (error) { throw new Error(`line ${line}: ${error.message}`); }
    sites.push({ start, operandStart: base + value.start, end: base + value.end, simple: value.simple, line, column: start - source.lastIndexOf('\n', start - 1) });
  }
}
export function migrate(source) {
  const sites = [];
  collect(source, 0, source, sites);
  sites.sort((a, b) => a.start - b.start);
  // Insertion events handle nested copies without overlapping replacements.
  const events = new Map();
  const add = (at, text) => events.set(at, (events.get(at) ?? '') + text);
  for (const site of sites.toReversed()) {
    if (!site.simple) add(site.operandStart, '(');
    add(site.end, site.simple ? '.copy()' : ').copy()');
  }
  const removed = new Set(sites.map((site) => site.start));
  let result = '';
  for (let i = 0; i <= source.length; i++) {
    result += events.get(i) ?? '';
    if (i < source.length && !removed.has(i)) result += source[i];
  }
  return { source: result, sites };
}
function deliberate(root, file) {
  if (!/(?:^|\/)ui\//.test(file) && !/JetParser\/Tests\//.test(file)) return false;
  return /e0941/i.test(file) || (existsSync(resolve(root, file.replace(/\.jet$/, '.stderr'))) && /\bE0941\b/.test(readFileSync(resolve(root, file.replace(/\.jet$/, '.stderr')), 'utf8')));
}
function filesAt(path) {
  if (!statSync(path).isDirectory()) return /\.(?:jet|jet\.template|template)$/.test(path) ? [path] : [];
  return readdirSync(path, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)).flatMap((entry) => {
    if (['.git', 'node_modules', 'target', '.agent-worktrees'].includes(entry.name) || entry.isSymbolicLink()) return [];
    return filesAt(resolve(path, entry.name));
  });
}
function dirtyPaths(root) {
  const status = execFileSync('git', ['status', '--porcelain=v1', '-z', '--untracked-files=all'], { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }).split('\0');
  const dirty = new Set();
  for (let n = 0; n < status.length; n++) {
    if (!status[n]) continue;
    dirty.add(status[n].slice(3));
    if (/[RC]/.test(status[n].slice(0, 2))) dirty.add(status[++n]);
  }
  return dirty;
}
export function owner(file) {
  if (/JetLexer\/Source\/Lexer\/Scan.jet|JetCli\/Source\/Cli\/StructureRegistry.jet/.test(file)) return 'ToolsCensusFix';
  if (/JetCodegen\/|JetEval\/|JetOptimizer\/Source\/Pipeline.jet|JetFoundation\/Source\/Target\/Layout.jet|JetCli\/Source\/Cli\/(?:CommandOutput|ForeignLink|Dev\/DbRows).jet/.test(file)) return 'DupRestoreRest';
  if (/JetFoundation\/Source\/Collections.jet|^crates\/.*\.jet$/.test(file)) return 'RelocateLead';
  if (/JetRuntime\/(?:Source\/(?:Compiler|Dyn)|Tests\/Compiler)|JetBackend\/Source\/(?:DataABI|Lower\/(?:Codecs|RuntimeRoutes))|JetFoundation\/Source\/Registry\/RuntimeStopExports/.test(file)) return 'RtPortMath';
  if (/JetBackend\/Source\/Lower\/(?:Constants|Statics|Entry|Callable|Test)\.jet|JetBackend\/Tests\/(?:Fixtures\/(?:Constants|Statics)\.jet|LowerFixtures\.jet)/.test(file)) return 'RustLead8';
  if (/JetBackend\/Source\/Lower\/(?:Lower|Dyn\w*|Realtime|Combinators)\.jet|JetFoundation\/Source\/(?:MIR|TIR)/.test(file)) return 'JetCheckLead';
  if (/JetBackend\/Source\/(?:Image|Opt|LIR|X64|OS)\/|JetBackend\/Tests\//.test(file)) return 'BackendX64Session';
  if (/JetParser\/|JetSema\/|JetDriver\/Source\/Driver\/MarkerGroups/.test(file)) return 'ParserSemaRestore';
  if (/JetRuntime\/(?:Source\/(?:RuntimePack|RuntimeLibrary)|sources.list|imports.list)|JetCli\/Source\/Cli\/Request.jet/.test(file)) return 'RuntimePackLead';
  if (/JetDriver\/Source\/Driver\/Pipeline.jet/.test(file)) return 'RuntimeLead12';
  if (/^Tools\/testkit\//.test(file)) return 'RegistryRunnerLead';
  if (/^(?:Core|Jetpack)\/|JetRuntime\//.test(file)) return 'CoreJetpackFix';
  if (/^crates\/|^Compiler\/Bootstrap\//.test(file)) return 'DevGreenLead';
  return 'unknown';
}
function selfTest() {
  const cases = [
    ['x :: ~foo.bar(1)[i].baz', 'x :: foo.bar(1)[i].baz.copy()'],
    ['return ~(a + b)', 'return (a + b).copy()'],
    ['f(~x, ~[Int]{1, 2}, ~Point{x: 1})', 'f(x.copy(), [Int]{1, 2}.copy(), Point{x: 1}.copy())'],
    ['a := ~x + y * ~z', 'a := x.copy() + y * z.copy()'],
    ['if ~trimmed == "#" -> return ~result.output', 'if trimmed.copy() == "#" -> return result.output.copy()'],
    ['~x ^ 2 + y', '(x ^ 2).copy() + y'],
    ['~!x', '(!x).copy()'],
    ['~(~x)', '(x.copy()).copy()'],
    ['~ ~x', ' (x.copy()).copy()'],
    ['(~call()?)', '(call()?.copy())'],
    ['~x!other', 'x.copy()!other'],
    ['~decode<Order>(raw).field', 'decode<Order>(raw).field.copy()'],
    ['~Pool<Box<T>>.new().x', 'Pool<Box<T>>.new().x.copy()'],
    ['a ~| b\na ~|= b\nfn Thing~~method() {}', 'a ~| b\na ~|= b\nfn Thing~~method() {}'],
    ['"~x {f("~inner")}" `~raw` \'~\' // ~comment\n/* ~a /* ~b */ */\nx :: ~ok', '"~x {f("~inner")}" `~raw` \'~\' // ~comment\n/* ~a /* ~b */ */\nx :: ok.copy()'],
    ['"""~x {f("~inner")}\n~x""" ``~x ` ~x``\nx :: ~ok', '"""~x {f("~inner")}\n~x""" ``~x ` ~x``\nx :: ok.copy()'],
    ['prep { ~template }\nprep loop field in T.$fields { ~field }\n~runtime', 'prep { ~template }\nprep loop field in T.$fields { ~field }\nruntime.copy()'],
    ['~f(~x).g(~y)[~i]', 'f(x.copy()).g(y.copy())[i.copy()].copy()'],
    ['return ~x\n    .Next(y) -> ~y', 'return x.copy()\n    .Next(y) -> y.copy()'],
    ['x :: ~options\n^scanner', 'x :: options.copy()\n^scanner'],
    ['x :: ~foo\n  .bar()[i]', 'x :: foo\n  .bar()[i].copy()'],
    ['x :: ~error.message\n[String]{}', 'x :: error.message.copy()\n[String]{}'],
    ['"v={~x} ~y {a}"', '"v={x.copy()} ~y {a}"'],
    ['"{~(a[1..<n])}"', '"{(a[1..<n]).copy()}"'],
    ['"{~a.b(c)}"', '"{a.b(c).copy()}"'],
    ['"{~f("x{~y}")}"', '"{f("x{y.copy()}").copy()}"'],
    ['"{~Point{x: 1}} {{~x}}"', '"{Point{x: 1}.copy()} {{~x}}"'],
    ['"""a {~x}\n~plain"""', '"""a {x.copy()}\n~plain"""'],
    ['URL{"a/{~x}"}', 'URL{"a/{~x}"}'],
  ];
  for (const [input, expected] of cases) {
    assert.equal(migrate(input).source, expected, input);
    assert.equal(migrate(expected).sites.length, 0, `idempotence: ${input}`);
  }
  console.log(`copy-sigil self-test: ${cases.length} cases passed`);
}
function main(args) {
  if (args.length === 1 && args[0] === '--self-test') { selfTest(); return; }
  const requested = args.shift(), ownerWrite = requested === '--owner-write';
  const mode = ownerWrite ? '--write' : requested;
  if (!['--check', '--write'].includes(mode) || args.some((arg) => arg.startsWith('--'))) throw new Error('usage: --check|--write [file/directory ...], --owner-write /absolute/file [...], or --self-test');
  if (ownerWrite && (!args.length || args.some((arg) => !isAbsolute(arg) || !statSync(arg).isFile()))) throw new Error('--owner-write requires explicit absolute file paths owned by the caller');
  const paths = args.length ? args.map((arg) => resolve(ROOT, arg)) : [ROOT];
  const files = [...new Set(paths.flatMap(filesAt))].sort();
  const dirty = dirtyPaths(ROOT), counts = new Map(), skipped = [], changed = [];
  const reservations = resolve(SCRATCH, 'reserved-files.tsv');
  const reserved = new Set(existsSync(reservations) ? readFileSync(reservations, 'utf8').trim().split('\n') : []);
  let total = 0, errors = 0;
  for (const path of files) {
    const file = relative(ROOT, path);
    if (file.startsWith('../')) throw new Error(`outside worktree: ${path}`);
    if (deliberate(ROOT, file)) continue;
    const source = readFileSync(path, 'utf8');
    let migrated;
    try { migrated = migrate(source); }
    catch (error) { console.error(`${file}: ${error.message}`); errors++; continue; }
    if (!migrated.sites.length) continue;
    const packageName = file.split('/').slice(0, /^(Compiler|Core|Jetpack|Tools)\//.test(file) ? 2 : 1).join('/');
    counts.set(packageName, (counts.get(packageName) ?? 0) + migrated.sites.length);
    total += migrated.sites.length;
    for (const site of migrated.sites) console.log(`${file}:${site.line}:${site.column}\t${source.slice(site.start, site.end).replaceAll('\n', '\\n')}`);
    if (mode === '--write') {
      // Recheck just before each write so another agent's edits aren't captured.
      if (!ownerWrite && (reserved.has(file) || dirty.has(file) || dirtyPaths(ROOT).has(file))) {
        skipped.push([owner(file), file, migrated.sites.length]); continue;
      }
      if (readFileSync(path, 'utf8') !== source) throw new Error(`concurrent edit: ${file}`);
      writeFileSync(path, migrated.source); changed.push(file);
    }
  }
  for (const [name, count] of [...counts].sort()) console.log(`package\t${name}\t${count}`);
  mkdirSync(SCRATCH, { recursive: true });
  if (!ownerWrite) writeFileSync(resolve(SCRATCH, mode === '--check' ? 'check-counts.tsv' : 'write-counts.tsv'), [...counts].sort().map(([name, count]) => `${name}\t${count}\n`).join(''));
  console.log(`copy-sigil sites: ${total}`);
  if (mode === '--write' && !ownerWrite) {
    mkdirSync(SCRATCH, { recursive: true });
    writeFileSync(resolve(SCRATCH, 'dirty-files.tsv'), 'owner\tfile\tsites\n' + skipped.sort((a, b) => a.join('\t').localeCompare(b.join('\t'))).map((row) => row.join('\t') + '\n').join(''));
    writeFileSync(resolve(SCRATCH, 'changed-files.tsv'), changed.map((file) => file + '\n').join(''));
    console.log(`changed files: ${changed.length}; skipped dirty files: ${skipped.length}`);
  }
  if (errors || (mode === '--check' && total)) process.exitCode = 1;
}
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) main(process.argv.slice(2));
