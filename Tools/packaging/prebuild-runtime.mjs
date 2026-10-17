#!/usr/bin/env node
// Package checked Jet runtime MIR as one object, then resolve permitted C
// platform dependencies with Jet's own linker. Never invoke a Rust compiler.
import { nativeCCompiler } from './native-c-compiler.mjs';
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { nativePackRecipe, readNativeRuntimePack } from './runtime-pack-cache.mjs';
import { invokeStageZeroFactory, readStageZeroFactory } from '../agent/stage1/native-factory.mjs';

let repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const [compiler, destination, ...extra] = process.argv.slice(2);
const usage = 'prebuild-runtime.mjs <compiler> [runtime-dir] [--factory-record <admitted-stage0-receipt>] [--repo <exact-source-export>]';
if (!compiler) throw Error(usage);
let factoryRecord = null;
for (let i = 0; i < extra.length; i += 2) {
  if (!extra[i + 1] || extra[i + 1].startsWith('--')) throw Error(usage);
  if (extra[i] === '--factory-record') factoryRecord = resolve(extra[i + 1]);
  else if (extra[i] === '--repo') repo = resolve(extra[i + 1]);
  else throw Error(usage);
}
const jet = resolve(compiler);
const out = resolve(destination ?? join(dirname(jet), 'jet-runtime'));
const musl = process.env.JET_PACK_MUSL_LIB;
if (!musl) throw Error('JET_PACK_MUSL_LIB must name the static musl libc.a directory');
const cc = nativeCCompiler;
const platform = [resolve(musl, 'libc.a'), ...['libgcc_eh.a', 'libgcc.a'].map(name => {
  const path = execFileSync(cc, [`-print-file-name=${name}`], { encoding: 'utf8' }).trim();
  if (!existsSync(path)) throw Error(`${cc} did not locate ${name}: ${path}`);
  return resolve(path);
})];
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
function inventory(files) {
  const hash = createHash('sha256');
  const rows = [...new Set(files)].sort().map(path => {
    const bytes = readFileSync(join(repo, path));
    hash.update(`${Buffer.byteLength(path)}:${path}:${bytes.length}:`).update(bytes);
    return { path, sha256: sha256(bytes) };
  });
  return { sha256: hash.digest('hex'), files: rows };
}
function jetFiles(directory) {
  return readdirSync(join(repo, directory), { withFileTypes: true }).flatMap(entry => {
    const path = `${directory}/${entry.name}`;
    return entry.isDirectory() ? jetFiles(path) : entry.name.endsWith('.jet') ? [path] : [];
  });
}
const manifest = 'Compiler/JetRuntime/sources.list';
const sources = readFileSync(join(repo, manifest), 'utf8').split('\n').map(row => row.trim()).filter(row => row && !row.startsWith('#'));
const source = inventory([manifest, 'Compiler/JetRuntime/package.jet', ...sources]);
const backend = inventory(jetFiles('Compiler/JetBackend/Source'));
const lifecycle = inventory(['Compiler/JetRuntime/lifecycle.list']);
const platformInputs = platform.map(path => ({ path, sha256: sha256(readFileSync(path)), language: 'C', role: 'platform' }));
const compilerSha256 = sha256(readFileSync(jet));
if (factoryRecord && resolve(readStageZeroFactory(factoryRecord, repo).factory.path) !== jet) throw Error('runtime compiler does not match admitted factory');
const recipeSha256 = nativePackRecipe(source.sha256, backend.sha256, lifecycle.sha256,
  platformInputs.map(row => row.sha256), compilerSha256);
// User foreign closures link the Jet-only runtime object with these exact
// platform archives once (Compiler/JetCli ForeignLink.jet); ship them beside it.
function publishPlatform() {
  const directory = join(out, 'foreign/platform');
  mkdirSync(directory, { recursive: true });
  for (const path of platform) copyFileSync(path, join(directory, basename(path)));
}
if (existsSync(join(out, 'native.key'))) {
  try {
    const cached = readNativeRuntimePack(out, recipeSha256);
    noRust(join(out, cached.key, 'jet_runtime_jet.o'));
    noRust(join(out, cached.key, 'jet_runtime.pack.o'));
    publishPlatform();
    console.log(join(out, cached.key, 'jet_runtime.pack.o'));
    process.exit(0);
  } catch (error) {
    console.error(`native runtime cache miss: ${error.message}`);
  }
}
function noRust(path) {
  if (/\.(?:rlib|rs)$/u.test(path)) throw Error(`Rust runtime input forbidden: ${path}`);
  const symbols = execFileSync('nm', ['--format=posix', path], { encoding: 'utf8', maxBuffer: 1 << 28 });
  const forbidden = symbols.split('\n').filter(row => /(?:^|\s)(?:_R[A-Z]|_ZN\d|__rust_|__rdl_)/u.test(row));
  if (forbidden.length) throw Error(`Rust symbols in ${path}:\n${forbidden.join('\n')}`);
}
for (const path of platform) noRust(path);
mkdirSync(out, { recursive: true });
const work = mkdtempSync(join(tmpdir(), 'jet-runtime-pack-'));
try {
  const library = join(work, 'library');
  execFileSync(process.execPath, [join(repo, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), library, '--assemble-only'], { stdio: 'inherit' });
  const prefix = join(work, 'jet_runtime_jet');
  const pack = join(work, 'jet_runtime.pack.o');
  const policy = {
    JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix,
    JET_FRONT_LIBRARY_LIFECYCLE: join(repo, 'Compiler/JetRuntime/lifecycle.list'),
    JET_FRONT_RUNTIME_PACK_OUTPUT: pack,
    JET_FRONT_RUNTIME_PLATFORM_INPUTS: platform.join('\n'),
  };
  let factory = null;
  if (factoryRecord) {
    factory = invokeStageZeroFactory({ recordFile: factoryRecord, repo, sourceRoot: library, entry: 'unit.jet', work: join(out, 'factory'), policy });
  } else {
    // A native compiler: the runtime-library seam in the hosted source loader
    // (Request.jet jet_cli_compile_for_target) under `jet doc` (checked MIR,
    // no lowering of the unit itself; its pages land in the temporary work
    // directory). Native `jet check` aborts before compiling in every native
    // compiler so far, and `doc --check` exits 1 on undocumented public items.
    const run = spawnSync(jet, ['doc', join(library, 'unit.jet')], {
      cwd: library, stdio: 'inherit',
      env: { ...process.env, JET_TOOLCHAIN_ROOT: repo, ...policy },
    });
    if (run.error) throw run.error;
    if (run.status !== 0) throw Error(`Jet runtime production failed (${run.status})`);
  }
  for (const path of [prefix + '.o', pack]) noRust(path);
  const abiBytes = readFileSync(prefix + '.abi.json');
  const abi = JSON.parse(abiBytes);
  if (!Array.isArray(abi) || !abi.length) throw Error('runtime ABI inventory is empty');
  const exportNames = readFileSync(prefix + '.exports', 'utf8').trim().split('\n').sort();
  if (JSON.stringify(abi.map(row => row.symbol).sort()) !== JSON.stringify(exportNames)) throw Error('runtime ABI and checked exports disagree');
  const abiSha256 = sha256(abiBytes);
  const identity = { format: 'jet-native-runtime/v1', source: source.sha256, abi: abiSha256, backend: backend.sha256, lifecycle: lifecycle.sha256, platform: platformInputs.map(row => row.sha256) };
  const key = sha256(JSON.stringify(identity));
  const directory = join(out, key);
  mkdirSync(directory, { recursive: true });
  copyFileSync(prefix + '.o', join(directory, 'jet_runtime_jet.o'));
  copyFileSync(prefix + '.abi.json', join(directory, 'runtime.abi.json'));
  copyFileSync(prefix + '.exports', join(directory, 'runtime.exports'));
  copyFileSync(pack, join(directory, 'jet_runtime.pack.o'));
  const provenance = { backend: 'jet-native', runtimeKey: key, objectSha256: sha256(readFileSync(prefix + '.o')), packSha256: sha256(readFileSync(pack)), rustObjects: 0, llvmObjects: 0, abiSha256, source, backendInputs: backend, lifecycle, cPlatformArchives: platformInputs, compilerSha256, recipeSha256 };
  if (factory) provenance.bootstrapFactory = { sourceCommit: factory.sourceCommit, protocol: factory.compilerProtocol, receipt: factory.receipt, keepGoingReport: factory.census, producerSha256: sha256(readFileSync(fileURLToPath(import.meta.url))), harnessSha256: sha256(readFileSync(new URL('../agent/stage1/native-factory.mjs', import.meta.url))) };
  writeFileSync(join(directory, 'provenance.json'), JSON.stringify(provenance, null, 2) + '\n');
  writeFileSync(join(out, 'provenance.json'), JSON.stringify(provenance, null, 2) + '\n');
  publishPlatform();
  writeFileSync(join(out, 'native.key.new'), key + '\n');
  renameSync(join(out, 'native.key.new'), join(out, 'native.key'));
  console.log(join(directory, 'jet_runtime.pack.o'));
} catch (error) {
  for (const suffix of ['.issues', '.pack-issues']) {
    const path = join(work, 'jet_runtime_jet' + suffix);
    if (existsSync(path)) writeFileSync(join(out, 'runtime' + suffix), readFileSync(path));
  }
  throw error;
} finally {
  rmSync(work, { recursive: true, force: true });
}
