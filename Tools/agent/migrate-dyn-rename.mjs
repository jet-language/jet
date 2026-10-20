#!/usr/bin/env node
// Owner ruling 2026-10-09: Dyn is the new name for DataTree (no compatibility aliases).
// node Tools/agent/migrate-dyn-rename.mjs --check|--write [absolute file/directory ...]
// node Tools/agent/migrate-dyn-rename.mjs --self-test
// --write skips every porcelain-dirty path, records owner handoffs, and never edits Rust.
// --check --clean checks committed clean files only. --owner-write PATH... is for
// owners applying the migration to their own dirty files after reviewing the handoff.
// Canonical evidence: Syntax/MathLayout.jet TYPE_DATA; Foundation/Dyn.rs;
// TIR/TIR.jet Dyn* operations; Sema/Calls/Data.jet sema_dyn_type; JetStd/Dyn*.rs.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve, relative, extname } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const SCRATCH = resolve(homedir(), '.cache/jet-dev/scratch/DynRename');
const SELF = relative(ROOT, fileURLToPath(import.meta.url));
const oldName = 'Data' + 'Tree';
const retired = /DataTree|data_tree|datatree|DATA_TREE/;
const words = /[\p{L}_$][\p{L}\p{N}_$]*/gu;
const frozenABI = /^(?:jet_rt_|jet_datatree_|DATA_TREE_|data_tree_)/;

export function owner(path) {
  if (/^Compiler\/JetBackend\/Source\/Image\/(RuntimePack|RuntimeLibrary)\.jet$|^Compiler\/JetRuntime\/[^/]+\.list$|^Compiler\/JetCli\/Source\/Cli\/Request\.jet$/.test(path)) return 'RuntimePackLead';
  if (/^Compiler\/JetRuntime\/.*(?:Dyn|DataTree|JSON|Compiler)/.test(path)) return 'RtPortMath';
  if (/Pipeline\.jet$|(?:run-runtime|run-scan-goldens|test-runtime-object)\.mjs$/.test(path)) return 'RuntimeLead12';
  if (/^Compiler\/JetBackend\/Source\/Lower\/(Constants|Statics|Entry|Callable|Test)\.jet$/.test(path)) return 'RustLead8';
  if (/^Compiler\/JetBackend\/Source\/(Image|Opt|LIR|X64)\//.test(path)) return 'BackendX64Session';
  if (/\/Lower\/|\/MIR\//.test(path)) return 'JetCheckLead';
  if (/^Compiler\/Jet(?:Parser|Sema)\//.test(path)) return 'ParserSemaRestore';
  if (/^Tools\/testkit\//.test(path)) return 'RegistryRunnerLead';
  if (/^crates\/|^Compiler\/Bootstrap\//.test(path)) return 'DevGreenLead';
  if (/^(Core|Jetpack)\/|^Compiler\/JetRuntime\//.test(path)) return 'CoreJetpackFix';
  return 'Main';
}
function target(path) {
  if (path === SELF || /(?:^|\/)(?:\.git|node_modules|\.tower|target|dist)(?:\/|$)/.test(path)) return false;
  if (/^Docs\/(?:audits|research|proposals)\//i.test(path)) return false;
  if (/\.(?:rs|c|h|png|jpg|svg|pdf|lock|wasm|a|o)$/.test(path)) return false;
  return /\.(?:jet|mjs|md|golden|snapshot|expected|stdout|stderr|out|snap|template)$/.test(path) || /^tests\/cases\//.test(path);
}
function git(args) { return execFileSync('git', args, { cwd: ROOT, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }); }
function tracked() { return git(['ls-files', '-z']).split('\0').filter(Boolean); }
function dirty() {
  const fields = git(['status', '--porcelain=v1', '-z', '--untracked-files=all']).split('\0');
  const result = new Set();
  for (let i = 0; i < fields.length; i++) {
    if (!fields[i]) continue;
    const status = fields[i].slice(0, 2); result.add(fields[i].slice(3));
    if (/[RC]/.test(status) && fields[i + 1]) result.add(fields[++i]);
  }
  return result;
}
function candidate(word) {
  return word.replaceAll(oldName, 'Dyn').replaceAll('data_tree', 'dyn').replaceAll('datatree', 'dyn').replaceAll('DATA_TREE', 'DYN');
}
export function mapping(files) {
  const canonical = new Set(); const old = new Set();
  for (const path of files) {
    if (!/\.(?:rs|jet|mjs)$/.test(path) || !existsSync(resolve(ROOT, path))) continue;
    for (const word of readFileSync(resolve(ROOT, path), 'utf8').match(words) ?? []) {
      if (retired.test(word)) old.add(word); else if (/Dyn|dyn|DYN/.test(word)) canonical.add(word);
    }
  }
  const map = new Map([[oldName, 'Dyn'], ['sema_data_tree_type', 'sema_dyn_type'], ['data_tree', 'dyn_value']]);
  for (const word of old) if (!map.has(word) && !frozenABI.test(word) && canonical.has(candidate(word))) map.set(word, candidate(word));
  return { map, missing: [...old].filter(word => !map.has(word)).sort() };
}
function replaceWords(text, map, sites, offset) {
  return text.replace(words, (word, index) => {
    const replacement = map.get(word);
    if (!replacement || replacement === word) return word;
    sites.push({ offset: offset + index, from: word, to: replacement }); return replacement;
  });
}
function quoteEnd(source, start, quote) {
  let i = start + quote.length;
  while (i < source.length) {
    if (source[i] === '\\') { i += 2; continue; }
    if (source.startsWith(quote, i)) return i + quote.length;
    if (quote[0] !== "'" && source[i] === '{' && !source.startsWith('{{', i)) {
      let depth = 1; i++;
      while (i < source.length && depth) {
        if ('"\'`'.includes(source[i])) i = quoteEnd(source, i, source.startsWith('"""', i) ? '"""' : source[i]);
        else if (source.startsWith('//', i) || source.startsWith('/*', i)) i = commentEnd(source, i);
        else { if (source[i] === '{') depth++; else if (source[i] === '}') depth--; i++; }
      }
    } else if (source.startsWith('{{', i) || source.startsWith('}}', i)) i += 2;
    else i++;
  }
  return source.length;
}
function commentEnd(source, start) {
  if (source.startsWith('//', start)) { const end = source.indexOf('\n', start); return end < 0 ? source.length : end; }
  let depth = 1, i = start + 2;
  while (i < source.length && depth) {
    if (source.startsWith('/*', i)) { depth++; i += 2; }
    else if (source.startsWith('*/', i)) { depth--; i += 2; }
    else i++;
  }
  return i;
}
// Only API-bearing literals are migrated: compiler type/operation registries,
// embedded Jet fixtures, diagnostics, and API documentation. Ordinary payload
// strings (including JSON keys and filename references) remain opaque.
function apiLiteral(text, before, path) {
  if (/Prelude\/CoreLib\/JetStd\/DataTree\.rs/.test(text) && existsSync(resolve(ROOT, 'crates/jet-codegen/src/Prelude/CoreLib/JetStd/Dyn.rs'))) return true;
  if (/\.(?:rs|jet|c|mjs)(?:["'`]|$)/.test(text) && !/\s/.test(text.slice(1, -1))) return false;
  if (/^(?:Compiler|crates)\//.test(path) && /^(?:"|')DataTree(?:"|')$/.test(text)) return true;
  if (/jet_foundation_datatree|::DataTree/.test(text)) return true;
  if (/\b(?:fn |use core|enum |struct |mod |Type\.Named|jet_std::DataTree|DataTree[.:]|DataTree`|DataTree variants|DataTree carrier|DataTree constructors|DataTree and DBValue|DataTree numeric range)/.test(text)) return true;
  if (/^\s*core\.encoding\./m.test(text) || /\bconsole\.log\($/.test(before)) return true;
  if (/\b(?:name|owner|type_exports|members|replacement|why|reason|message|source|rejected|clean|reference|core_sig_named|fx_named|fx_type_def|mir_type_id|policy_named_type)\s*[:(=][\s\S]*$/.test(before.slice(-180))) return true;
  return false;
}
export function migrate(source, path, map = new Map([[oldName, 'Dyn']])) {
  const sites = []; let output = '', i = 0;
  if (/\.(?:md|golden|snapshot|expected|stdout|stderr|out|snap|template)$/.test(path)) {
    return { text: replaceWords(source, map, sites, 0), sites };
  }
  while (i < source.length) {
    const start = i;
    if (source.startsWith('//', i) || source.startsWith('/*', i)) {
      i = commentEnd(source, i); const text = source.slice(start, i);
      // Documentation comments, not historical decision IDs or unrelated notes.
      const doc = text.startsWith('///') || /`DataTree|DataTree\.|\bDataTree\b/.test(text);
      output += doc ? replaceWords(text, map, sites, start) : text;
    } else if ('"\'`'.includes(source[i])) {
      const quote = source.startsWith('"""', i) ? '"""' : source[i];
      i = quoteEnd(source, i, quote); const text = source.slice(start, i);
      output += apiLiteral(text, source.slice(0, start), path) ? replaceWords(text, map, sites, start) : text;
    } else if (/[\p{L}_$]/u.test(source[i])) {
      i++; while (i < source.length && /[\p{L}\p{N}_$]/u.test(source[i])) i++;
      output += replaceWords(source.slice(start, i), map, sites, start);
    } else { output += source[i++]; }
  }
  return { text: output, sites };
}
export function selfTest() {
  assert.equal(migrate('fn f(x: DataTree) -> DataTree { DataTree.Null }', 'Core/a.jet').text, 'fn f(x: Dyn) -> Dyn { Dyn.Null }');
  assert.equal(migrate('print("DataTree"); // unrelated data_tree_record\n', 'Examples/a.jet').text, 'print("DataTree"); // unrelated data_tree_record\n');
  assert.equal(migrate('/// Decode `DataTree`.\n', 'Core/a.jet').text, '/// Decode `Dyn`.\n');
  assert.equal(migrate('Type.Named{name: "DataTree"}', 'Compiler/a.jet').text, 'Type.Named{name: "Dyn"}');
  assert.equal(migrate('const source = "fn f(v: DataTree) -> DataTree";', 'Tools/a.mjs').text, 'const source = "fn f(v: Dyn) -> Dyn";');
  assert.equal(migrate('const file = "DataTree.rs";', 'Tools/a.mjs').text, 'const file = "DataTree.rs";');
  assert.equal(migrate('jet_rt_data_tree_clone(v)', 'Compiler/a.jet').text, 'jet_rt_data_tree_clone(v)');
  assert.equal(migrate('DataTreehouse DataTree', 'Docs/spec/a.md').text, 'DataTreehouse Dyn');
  assert.equal(migrate('/* nested /* DataTree */ */ DataTree', 'Core/a.jet').text, '/* nested /* Dyn */ */ Dyn');
  console.log('Dyn rename self-test: 9 passed');
}
export function main(args = process.argv.slice(2)) {
  if (args.includes('--self-test')) { selfTest(); return; }
  const mode = args.find(arg => ['--check', '--write', '--owner-write'].includes(arg));
  if (!mode || args.some(arg => arg.startsWith('--') && !['--check', '--write', '--owner-write', '--clean', '--resume'].includes(arg))) throw new Error('expected --check|--write|--owner-write [paths], or --self-test');
  const filters = args.filter(arg => !arg.startsWith('--')).map(path => resolve(path));
  if (mode === '--owner-write' && !filters.length) throw new Error('--owner-write requires explicit owned paths');
  const all = tracked(); const changed = dirty(); const { map, missing } = mapping(all);
  const prior = args.includes('--resume') ? JSON.parse(readFileSync(resolve(SCRATCH, 'written-files.json'), 'utf8')) : [];
  const owned = new Set(prior.map(row => row.path));
  const files = [...new Set([...all, ...changed])].filter(path => target(path) && existsSync(resolve(ROOT, path)) && (!filters.length || filters.some(filter => resolve(ROOT, path) === filter || resolve(ROOT, path).startsWith(filter + '/'))));
  const skipped = [], written = [], totals = {}, leftovers = []; let count = 0;
  for (const path of files) {
    if (args.includes('--clean') && changed.has(path)) continue;
    const source = readFileSync(resolve(ROOT, path), 'utf8');
    if (!retired.test(source)) continue;
    const result = migrate(source, path, map);
    for (const match of source.matchAll(words)) if (retired.test(match[0]) && !map.has(match[0])) leftovers.push(`${owner(path)}\t${path}\t${match[0]}`);
    if (!result.sites.length) continue;
    if (mode === '--write' && changed.has(path) && !owned.has(path)) { skipped.push(`${owner(path)}\t${resolve(ROOT, path)}\t${result.sites.length}`); continue; }
    count += result.sites.length; const area = path.split('/')[0]; totals[area] = (totals[area] ?? 0) + result.sites.length;
    console.log(`${path}\t${result.sites.length}`);
    if (mode !== '--check') { writeFileSync(resolve(ROOT, path), result.text); written.push({ path, count: result.sites.length }); }
  }
  mkdirSync(SCRATCH, { recursive: true });
  if (mode === '--write') {
    writeFileSync(resolve(SCRATCH, 'dirty-files.tsv'), skipped.sort().join('\n') + '\n');
    const combined = new Map(prior.map(row => [row.path, row]));
    for (const row of written) combined.set(row.path, { path: row.path, count: row.count + (combined.get(row.path)?.count ?? 0) });
    writeFileSync(resolve(SCRATCH, 'written-files.json'), JSON.stringify([...combined.values()], null, 2) + '\n');
  }
  writeFileSync(resolve(SCRATCH, 'leftovers.tsv'), [...new Set(leftovers)].sort().join('\n') + '\n');
  writeFileSync(resolve(SCRATCH, 'mapping.json'), JSON.stringify({ mapping: Object.fromEntries(map), missing }, null, 2) + '\n');
  console.log(JSON.stringify({ mode, sites: count, files: mode === '--check' ? undefined : written.length, skipped: skipped.length, totals }));
  if (mode === '--check' && count) process.exitCode = 1;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();
