#!/usr/bin/env node
// Parent-process Jet runtime candidate producer (not an admission bypass).
// Usage: Tools/agent/jet-env node Tools/packaging/prebuild-runtime-shared.mjs INPUTS.json OUT
// INPUTS schema (all commits are full SHAs; every hash is lowercase SHA-256):
// {source:{root,sha}, landed:{root,sha}, compiler:{path,sha256,sourceSHA},
//  e2:{root,sha,compiler,compilerSHA256,object,objectSHA256},
//  layers:{l7a,l8a1,l8a2,l8a4}, libraryInstallRelative:"jet-runtime/jet_runtime.so"}
// Main supplies pins only after landing. An unset E2 FINAL or missing prerequisite
// refuses before lowering. OUT must not exist. The receipt seals a CANDIDATE;
// the consumer's private loader still checks the pinned receipt before symbol use.
import { nativeCCompiler } from './native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, realpathSync, renameSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { exactExportABI, exportRows, sealObject, sealShared } from './runtime-shared-seal.mjs';
import { embeddedAdmission, installRelative } from './runtime-admission.mjs';

const [inputPath, output, ...extra] = process.argv.slice(2);
if (!inputPath || !output || extra.length) throw Error('usage: prebuild-runtime-shared.mjs INPUTS.json OUT');
const config = JSON.parse(readFileSync(inputPath, 'utf8'));
const out = resolve(output);
const libraryInstallRelative = installRelative(config.libraryInstallRelative);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const fileHash = path => hash(readFileSync(path));
const requireSHA = (value, label) => {
  if (!/^[a-f0-9]{40}$/.test(value ?? '')) throw Error(`${label}: missing full landed commit pin (job remains held)`);
  return value;
};
const requireHash = (value, label) => {
  if (!/^[a-f0-9]{64}$/.test(value ?? '')) throw Error(`${label}: missing SHA-256 pin`);
  return value;
};
function command(bin, args, cwd, env = process.env, log) {
  const result = spawnSync(bin, args, { cwd, env, encoding: 'utf8', maxBuffer: 1 << 27 });
  if (log) writeFileSync(log, `${result.stdout ?? ''}${result.stderr ?? ''}`);
  if (result.error || result.status !== 0) throw Error(`${bin} ${args.join(' ')}: ${result.error ?? result.stderr ?? result.status}`);
  return result.stdout;
}
const git = (root, ...args) => command('git', ['-C', root, ...args]).trim();
function pinnedTree(tree, label) {
  if (!tree?.root) throw Error(`${label}: missing worktree`);
  requireSHA(tree.sha, label);
  tree.root = realpathSync(tree.root);
  if (git(tree.root, 'rev-parse', 'HEAD') !== tree.sha) throw Error(`${label}: worktree no longer matches pin`);
  if (git(tree.root, 'status', '--porcelain')) throw Error(`${label}: dirty worktree`);
}
function ancestor(root, older, newer, label) {
  command('git', ['-C', root, 'merge-base', '--is-ancestor', older, newer]);
  if (git(root, 'rev-parse', `${older}^{commit}`) !== older) throw Error(`${label}: not a full commit`);
}
function pinnedFile(path, expected, label) {
  requireHash(expected, label);
  if (fileHash(path) !== expected) throw Error(`${label}: file differs from SHA-256 pin`);
}
function source(root, path) { return readFileSync(join(root, path), 'utf8'); }
const runtime = 'Compiler/JetRuntime';
requireSHA(config.e2?.sha, 'E2 FINAL');
pinnedTree(config.source, 'source composition');
pinnedTree(config.landed, 'landed head');
pinnedTree(config.e2, 'E2 FINAL');
ancestor(config.landed.root, config.source.sha, config.landed.sha, 'source composition');
ancestor(config.source.root, config.e2.sha, config.source.sha, 'E2 FINAL');
for (const name of ['l7a', 'l8a1', 'l8a2', 'l8a4']) {
  const sha = requireSHA(config.layers?.[name], name);
  ancestor(config.source.root, sha, config.source.sha, name);
  ancestor(config.landed.root, sha, config.landed.sha, name);
}
if (config.compiler?.sourceSHA !== config.source.sha) throw Error('composed runtime compiler must bind the composed source SHA');
pinnedFile(config.compiler.path, config.compiler.sha256, 'composed compiler');
pinnedFile(config.e2.compiler, config.e2.compilerSHA256, 'FINAL E2 compiler');
pinnedFile(config.e2.object, config.e2.objectSHA256, 'FINAL E2 object');
// Runtime optimization is source-selected, not an environment switch. The O1
// static bundle has a different owner/epoch and must not silently enter here.
const loweringPath = 'Compiler/JetBackend/Source/Image/RuntimeLibrary.jet';
for (const tree of [config.source.root, config.e2.root]) {
  if (source(tree, loweringPath).split('\n').some(line => !line.trim().startsWith('//') && /\blir_optimize_module\s*\(/.test(line))) throw Error('O1 runtime-library hook is not an O0 parent runtime input');
}
const manifestPath = `${runtime}/exports.list`;
const manifest = source(config.source.root, manifestPath);
const rows = exportRows(manifest);
const expectedNames = rows.map(row => row.name).sort();
const e2Exports = exportRows(source(config.e2.root, manifestPath)).map(row => row.name).sort();
for (const name of e2Exports) if (!expectedNames.includes(name)) throw Error(`composed export union dropped frozen E2 export ${name}`);
// Schema identity belongs to the pinned Core image/getter inventory, never to
// new runtime .so exports. The runtime manifest is the complete landed union.
const initializers = rows.filter(row => row.init !== null);
if (!initializers.some(row => row.name === 'jet_rt_alloc_init' && row.init === 0) || !initializers.some(row => row.name === 'jet_rt_thread_init' && row.init === 2)) throw Error('landed lifecycle inventory lacks allocator0 or thread2 initialization');
const paths = source(config.source.root, `${runtime}/sources.list`).split('\n').map(line => line.trim()).filter(line => line && !line.startsWith('#'));
if (!paths.length || new Set(paths).size !== paths.length || paths.some(path => !/^(Compiler\/JetRuntime\/Source\/|Compiler\/JetBackend\/Source\/OS\/)[A-Za-z0-9_/]+\.jet$/.test(path))) throw Error('runtime source manifest is empty, duplicated, or not Jet-only');
const sourceFiles = Object.fromEntries(paths.map(path => [path, fileHash(join(config.source.root, path))]));
// The Source descriptor provider is parked by Main. It must not leak into the
// parent library through an old proof composition.
if (paths.some(path => path.endsWith('/SharedSource.jet'))) throw Error('parked Source descriptor provider is not a library input');
if (rows.some(row => row.name.startsWith('jet_rt_source_') || row.name === 'jet_rt_native_shared_source_new')) throw Error('parked Source descriptor exports are not a library input');
if (existsSync(out)) throw Error('refusing to overwrite producer evidence');
mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'inputs.json'), JSON.stringify(config, null, 2) + '\n');
const statuses = [];
let failed = false;
function step(name, action) {
  try { action(); statuses.push([name, 0]); }
  catch (error) { failed = true; statuses.push([name, 1]); writeFileSync(join(out, `${name}.error`), `${error.stack ?? error}\n`); }
  writeFileSync(join(out, 'steps.tsv'), statuses.map(([name, rc]) => `${name}\trc=${rc}`).join('\n') + '\n');
}
function lower(tree, compiler, dir, log) {
  command(process.execPath, [join(tree, 'Compiler/JetBackend/Tests/run-runtime.mjs'), dir], tree,
    { ...process.env, JET_FRONT_JETC0: compiler, JET_STORE_DIR: join(dir, 'store') }, join(out, log));
}
step('e2-byte-identity', () => {
  const dir = join(out, 'e2');
  lower(config.e2.root, config.e2.compiler, dir, 'e2.log');
  if (!readFileSync(join(dir, 'jet_runtime_jet.o')).equals(readFileSync(config.e2.object))) throw Error('canonical E2 FINAL object byte identity failed');
});
const object = join(out, 'runtime/jet_runtime_jet.o');
step('runtime-lower', () => lower(config.source.root, config.compiler.path, join(out, 'runtime'), 'runtime.log'));
let audit, exportABI;
step('object-seal', () => {
  audit = sealObject(object, rows);
  exportABI = exactExportABI(JSON.parse(readFileSync(join(out, 'runtime/jet_runtime_jet.abi.json'), 'utf8')), expectedNames);
  const allowed = new Set(source(config.source.root, `${runtime}/imports.list`).split('\n').map(line => line.trim().split(/\s+/)[0]).filter(name => name && !name.startsWith('#')));
  const unlisted = audit.imports.filter(name => !allowed.has(name));
  if (unlisted.length) throw Error(`unlisted object imports: ${unlisted.join(', ')}`);
  writeFileSync(join(out, 'object-seal.json'), JSON.stringify(audit, null, 2) + '\n');
});
const candidate = join(out, 'jet_runtime.so.candidate');
step('shared-link', () => {
  if (!audit) throw Error('object seal prerequisite failed');
  const script = join(out, 'exports.map');
  // An anonymous version map creates no extra version-node export.
  writeFileSync(script, `{ global:\n${expectedNames.map(name => `  ${name};`).join('\n')}\nlocal: *;\n};\n`);
  command(nativeCCompiler, ['-shared', '-nostartfiles', '-Wl,-Bsymbolic', '-Wl,-z,defs', '-Wl,-z,text', '-Wl,-z,now', '-Wl,-z,relro', `-Wl,--version-script=${script}`, '-Wl,-soname,jet_runtime.so', object, '-lpthread', '-ldl', '-lm', '-lc', '-o', candidate], config.source.root, process.env, join(out, 'link.log'));
});
let seal;
step('shared-seal', () => {
  if (!audit) throw Error('object seal prerequisite failed');
  seal = sealShared(candidate, rows, audit.lifecycle);
  writeFileSync(join(out, 'shared-seal.json'), JSON.stringify(seal, null, 2) + '\n');
});
step('pinned-inputs-unchanged', () => {
  pinnedTree(config.source, 'source composition'); pinnedTree(config.e2, 'E2 FINAL'); pinnedTree(config.landed, 'landed head');
  pinnedFile(config.compiler.path, config.compiler.sha256, 'composed compiler');
  pinnedFile(config.e2.compiler, config.e2.compilerSHA256, 'FINAL E2 compiler');
  pinnedFile(config.e2.object, config.e2.objectSHA256, 'FINAL E2 object');
});
if (failed) throw Error(`parent runtime producer failed; see ${out}/steps.tsv (no receipt published)`);
const receipt = {
  kind: 'jet-parent-runtime-candidate', sourceSHA: config.source.sha, landedSHA: config.landed.sha,
  e2SHA: config.e2.sha, layers: config.layers, optimization: 'O0',
  library: 'jet_runtime.so', libraryInstallRelative, librarySHA256: fileHash(candidate), objectSHA256: fileHash(object),
  compilerSHA256: config.compiler.sha256, e2CompilerSHA256: config.e2.compilerSHA256, e2ObjectSHA256: config.e2.objectSHA256,
  exportManifestSHA256: hash(manifest), runtimeLoweringSHA256: fileHash(join(config.source.root, loweringPath)), e2Exports, sourceFiles, exportABI, ...seal,
};
const receiptBytes = JSON.stringify(receipt, null, 2) + '\n';
const embedded = embeddedAdmission(Buffer.from(receiptBytes), receipt.librarySHA256, libraryInstallRelative);
writeFileSync(join(out, 'runtime-admission.rs.candidate'), embedded.source);
writeFileSync(join(out, 'runtime-shared.json.candidate'), receiptBytes);
renameSync(candidate, join(out, receipt.library));
renameSync(join(out, 'runtime-admission.rs.candidate'), join(out, 'runtime-admission.rs'));
renameSync(join(out, 'runtime-shared.json.candidate'), join(out, 'runtime-shared.json'));
console.log(JSON.stringify({ library: join(out, receipt.library), receipt: join(out, 'runtime-shared.json'), receiptSHA256: embedded.receiptSHA256, admissionSource: join(out, 'runtime-admission.rs'), libraryInstallRelative }));
