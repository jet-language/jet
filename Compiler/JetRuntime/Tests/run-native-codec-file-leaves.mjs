#!/usr/bin/env node
// First-RC witness against the admitted standalone Jet object and C platform.
// --test executes a named Jet test hook in a fresh native runtime library.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { nativePackHash, readNativeRuntimePack } from '../../../Tools/packaging/runtime-pack-cache.mjs';
import { fileObject } from '../../../Tools/packaging/publish-native-runtime-pack.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
import { parseRuntimeExports, runtimeLifecycleList } from '../../../Tools/agent/runtime-route-abi.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, compiler, runtime, ...extra] = process.argv.slice(2);
const test = extra.length === 2 && extra[0] === '--test' ? extra[1] : null;
const fixtures = { CodecCSV: 'jet_rt_codec_csv_fixture', CodecEnv: 'jet_rt_codec_env_fixture', DynCodec: 'jet_rt_dyn_codec_fixture', CompilerFileLeaves: 'jet_rt_compiler_file_leaves_fixture' };
if (!output || !compiler || !runtime || (extra.length && (!test || !fixtures[test]))) throw Error('usage: run-native-codec-file-leaves.mjs OUT STAGE2_JET RC_RUNTIME_DIR [--test CodecCSV|CodecEnv|DynCodec|CompilerFileLeaves]');
const out = resolve(output), jet = resolve(compiler), runtimeDirectory = resolve(runtime);
mkdirSync(out);
function receipt(name, value) {
  writeFileSync(join(out, name), JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
}
function command(name, bin, args, cwd = root) {
  const result = spawnSync(bin, args, { cwd, env: process.env, encoding: 'utf8', maxBuffer: 1 << 27, timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? (test ? 120000 : 0)) });
  receipt(`${name}.receipt.json`, { bin, args, cwd, status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr });
  if (result.error || result.status !== 0) throw Error(`${name}: ${result.error?.message ?? result.signal ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
const admitted = readNativeRuntimePack(runtimeDirectory);
const publishedObject = join(runtimeDirectory, admitted.key, 'jet_runtime_jet.o');
const object = join(out, 'jet_runtime_jet.o');
writeFileSync(object, admitted.files.get('jet_runtime_jet.o'), { flag: 'wx' });
const objectHandle = fileObject(object);
if (objectHandle.split(':')[0] !== admitted.provenance.objectSha256) throw Error('standalone object snapshot differs from admitted provenance');
const archives = admitted.provenance.cPlatformArchives;
if (!Array.isArray(archives) || !archives.length || archives.some(row => row.language !== 'C' || row.role !== 'platform')) throw Error('runtime provenance lacks canonical C platform inputs');
const admittedLibc = archives.find(row => basename(row.path) === 'libc.a');
const musl = process.env.JET_PACK_MUSL_LIB ?? (test && admittedLibc ? dirname(admittedLibc.path) : null);
if (!musl) throw Error('JET_PACK_MUSL_LIB must name the admitted static musl directory');
const libc = resolve(musl, 'libc.a'), libcHandle = fileObject(libc);
if (!archives.some(row => basename(row.path) === 'libc.a' && row.sha256 === libcHandle.split(':')[0])) throw Error('musl libc does not match runtime provenance');
const archiveInputs = [];
for (const row of archives) {
  const path = basename(row.path) === 'libc.a' ? libc : resolve(row.path), handle = fileObject(path);
  if (handle.split(':')[0] !== row.sha256) throw Error(`C platform archive differs from runtime provenance: ${path}`);
  archiveInputs.push({ path, handle });
}
if (test) {
  const unit = join(out, 'unit');
  command('assemble-fixture', process.execPath, [join(root, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), unit, '--assemble-only', '--test', test]);
  const file = join(unit, 'unit.jet'), text = readFileSync(file, 'utf8'), prefix = join(unit, 'fixture');
  const lifecycle = join(unit, 'lifecycle.list');
  writeFileSync(lifecycle, runtimeLifecycleList(parseRuntimeExports(text, 'unit.jet')));
  const lowered = runNativeLibrary({ jet, file, prefix, lifecycle, env: { ...process.env, JET_TOOLCHAIN_ROOT: root, JET_STORE_DIR: join(out, 'store') } });
  receipt('lower-fixture.json', lowered);
  if (lowered.problem) throw Error(`native ${test} fixture: ${lowered.problem.detail}`);
  const source = join(out, 'fixture-driver.c');
  const signature = test === 'CompilerFileLeaves' ? 'int64_t' : 'void';
  const call = test === 'CompilerFileLeaves'
    ? `Text *path = jet_rt_string_from_static(fixture_path, (int64_t)strlen(fixture_path));\nfixture_result = ${fixtures[test]}((int64_t)(intptr_t)path);\njet_rt_string_drop(path);`
    : `fixture_result = ${fixtures[test]}();`;
  const pathDeclaration = test === 'CompilerFileLeaves' ? 'static char *fixture_path;\n' : '';
  const pathSetup = test === 'CompilerFileLeaves' ? 'if (argc != 2) return 99;\nfixture_path = argv[1];\n' : '';
  const pathCleanup = test === 'CompilerFileLeaves' ? 'if (unlink(fixture_path) != 0 && fixture_result == 0 && status == 0) return 98;\n' : '';
  writeFileSync(source, `#include <stdint.h>\n#include <string.h>\n#include <unistd.h>\ntypedef struct Text Text;\nextern char **environ;\nextern void jet_rt_host_arguments(int64_t, char **, char **);\nextern int32_t jet_rt_main(void (*body)(void));\nextern Text *jet_rt_string_from_static(const void *, int64_t);\nextern void jet_rt_string_drop(Text *);\nextern int64_t ${fixtures[test]}(${signature});\nstatic int64_t fixture_result;\n${pathDeclaration}static void exercise(void) {\n${call}\n}\nint main(int argc, char **argv) {\n${pathSetup}jet_rt_host_arguments(argc, argv, environ);\nint32_t status = jet_rt_main(exercise);\n${pathCleanup}return status != 0 ? status : (int)fixture_result;\n}\n`);
  const cObject = join(out, 'fixture-driver.o'), executable = join(out, 'fixture-driver'), fixtureObject = `${prefix}.o`;
  const crt = ['crt1.o', 'crti.o', 'crtn.o'].map(name => resolve(musl, name));
  const inputs = [...archiveInputs, ...crt.map(path => ({ path, handle: fileObject(path) })), { path: fixtureObject, handle: fileObject(fixtureObject) }, { path: file, handle: fileObject(file) }];
  command('compile-fixture-driver', join(root, 'Tools/agent/jet-env'), [nativeCCompiler, '-std=c11', '-O1', '-fno-stack-protector', '-fno-PIE', '-fno-builtin', '-Wall', '-Wextra', '-Werror', '-c', source, '-o', cObject]);
  command('link-fixture-driver', join(root, 'Tools/agent/jet-env'), [nativeCCompiler, '-nostdlib', '-static', '-no-pie', crt[0], crt[1], cObject, '-Wl,--start-group', fixtureObject, ...archiveInputs.map(row => row.path), '-Wl,--end-group', crt[2], '-o', executable]);
  for (const row of inputs) if (fileObject(row.path) !== row.handle) throw Error(`fixture input changed: ${row.path}`);
  const result = command('run-fixture', executable, test === 'CompilerFileLeaves' ? [join(out, 'file-leaves.data')] : []);
  if (result.signal || result.stdout !== '' || result.stderr !== '') throw Error(`native ${test} fixture output differs`);
  receipt('fixture-result.json', { status: 0, test, export: fixtures[test], compiler: fileObject(jet), object: fileObject(fixtureObject), source: fileObject(source), executable: fileObject(executable), stdout: result.stdout, stderr: result.stderr });
  console.log(`native runtime Jet fixture ${test}: ok`);
  process.exit(0);
}
function pinnedSource(path) {
  const row = admitted.provenance.source.files.find(row => row.path === path), bytes = readFileSync(join(root, path));
  if (!row || nativePackHash(bytes) !== row.sha256) throw Error(`fixture ABI source differs from published RC: ${path}`);
  return { path, handle: fileObject(join(root, path)), text: bytes.toString('utf8') };
}
function literal(source, name) {
  const match = new RegExp(`^${name}\\s*::\\s*comptime\\s*\\{\\s*I64\\{(\\d+)\\}\\s*\\}`, 'm').exec(source.text);
  if (!match) throw Error(`missing canonical native ABI literal ${name}`);
  return Number(match[1]);
}
const dataAbi = pinnedSource('Compiler/JetBackend/Source/DataABI.jet');
const collections = pinnedSource('Compiler/JetRuntime/Source/Collections.jet');
const macros = ['BOOL', 'INT', 'FLOAT', 'NUMBER', 'TEXT'].map(kind => {
  const name = `DATA_TREE_${kind}`;
  return `-D${name}=${literal(dataAbi, name)}`;
});
const stride = literal(collections, 'RT_HOST_U8_STRIDE');
if (stride !== 1) throw Error(`native C U8 elements require packed stride 1, got ${stride}`);
macros.push(`-DRT_HOST_U8_STRIDE=${stride}`);
const source = join(root, 'Compiler/JetRuntime/Tests/NativeCodecFileLeavesWitness.c');
const sourceHandle = fileObject(source), compilerHandle = fileObject(jet);
const crt = ['crt1.o', 'crti.o', 'crtn.o'].map(name => ({ path: resolve(musl, name), handle: fileObject(resolve(musl, name)) }));
receipt('admission.json', { runtimeDirectory, runtimeKey: admitted.key, provenance: admitted.provenance, publishedObject, linkedObject: object, object: objectHandle, compiler: { path: jet, object: compilerHandle }, witness: { path: source, object: sourceHandle }, u8Stride: stride, abiSources: [dataAbi, collections].map(({ path, handle }) => ({ path, handle })), archives: archiveInputs, crt });
const cObject = join(out, 'codec-file-leaves.o'), link = join(out, 'link');
command('compile', join(root, 'Tools/agent/jet-env'), [nativeCCompiler, '-std=c11', '-O1', '-fno-stack-protector', '-fno-PIE', '-fno-builtin', '-Wall', '-Wextra', '-Werror', '-pedantic', ...macros, '-c', source, '-o', cObject]);
command('assemble-linker', process.execPath, [join(root, 'Compiler/JetBackend/Tests/run-link.mjs'), link]);
const executable = join(out, 'native-codec-file-leaves');
command('link', jet, ['run', join(link, 'unit.jet'), '--', executable, crt[0].path, crt[1].path, cObject, object, ...archiveInputs.map(row => row.path), crt[2].path], link);
for (const row of [...archiveInputs, ...crt]) if (fileObject(row.path) !== row.handle) throw Error(`C platform input changed during link: ${row.path}`);
for (const row of [dataAbi, collections]) if (fileObject(join(root, row.path)) !== row.handle) throw Error(`canonical ABI source changed during proof: ${row.path}`);
if (fileObject(source) !== sourceHandle || fileObject(jet) !== compilerHandle || fileObject(object) !== objectHandle) throw Error('witness, compiler, or admitted object changed during proof');
const result = command('run', executable, []);
const expected = 'CHECK OK — Jet-only Dyn encodes, FieldError frames, JSON, CSV and env codecs and shared buffered lines\n';
if (result.stdout !== expected || result.stderr !== '') throw Error('codec/file runtime witness output differs from its exact success receipt');
receipt('result.json', { status: 0, runtimeKey: admitted.key, object: objectHandle, compiler: compilerHandle, witness: sourceHandle, cObject: fileObject(cObject), executable: fileObject(executable), stdout: result.stdout, stderr: result.stderr });
console.log(`native codec/file leaves: ok (${admitted.key})`);
